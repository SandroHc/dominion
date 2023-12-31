use std::collections::hash_map;
use std::hash::{Hash, Hasher};

use regex::Regex;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::ser::PrettyFormatter;
use serde_json::Serializer;
use tokio::sync::mpsc;
use tracing::{debug, info, trace};

use crate::config::{HttpConfig, WatchEntry};
use crate::error::{DominionAsyncError, DominionRequestError};
use crate::NotificationEvent;

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct Watcher {
    pub url: String,
    method: Method,
    headers: Vec<(String, String)>,
    http_client: Client,
    notifier: mpsc::Sender<NotificationEvent>,
    ignore_mask: Option<Regex>,
    last_failed: bool,
    previous: Option<String>,
    previous_hash: u64,
}

impl Watcher {
    pub fn new(
        entry: &WatchEntry,
        notifier: mpsc::Sender<NotificationEvent>,
        http_cfg: &HttpConfig,
    ) -> Result<Self, DominionRequestError> {
        let headers = entry
            .headers
            .iter()
            .map(|h| {
                let (name, value) = h
                    .split_once('=')
                    .expect("malformed header; should be 'name=value'");
                (name.to_string(), value.to_string())
            })
            .collect::<Vec<_>>();

        let mut user_agent = http_cfg.user_agent.clone().unwrap_or_default();
        if user_agent.is_empty() {
            user_agent = DEFAULT_USER_AGENT.to_string();
        }
        debug!("Using user agent: {user_agent}");

        let http_client = Client::builder().user_agent(user_agent).build()?;

        Ok(Self {
            url: entry.url.clone(),
            method: entry.method.clone(),
            headers,
            http_client,
            notifier,
            ignore_mask: Self::build_mask(entry.ignore.as_slice())?,
            last_failed: false,
            previous: None,
            previous_hash: 0,
        })
    }

    fn build_mask(ignore_patterns: &[String]) -> Result<Option<Regex>, DominionRequestError> {
        let ignore_mask = if ignore_patterns.is_empty() {
            None
        } else {
            let mut joined_patterns = "(?:".to_string();
            let mut is_first = true;
            for pattern in ignore_patterns {
                if is_first {
                    is_first = false;
                } else {
                    joined_patterns += "|";
                }
                joined_patterns += pattern;
            }
            joined_patterns += ")";

            let regex = Regex::new(joined_patterns.as_str())?;
            Some(regex)
        };
        Ok(ignore_mask)
    }

    pub async fn watch(&mut self) -> Result<(), DominionAsyncError> {
        if self.previous.is_none() {
            info!("Doing initial fetch of {}", self.url);
        } else {
            info!("Checking {}", self.url);
        }

        match self.fetch().await {
            Ok(content) => {
                self.last_failed = false;

                let current = content;
                let current_masked = self.mask_value(current.clone());
                let current_hash = Watcher::hash(current_masked.as_str());

                if let Some(prev) = &self.previous {
                    if current_hash == self.previous_hash {
                        debug!("No changes in {}", self.url);
                        self.notifier
                            .send(NotificationEvent::NoChanges {
                                url: self.url.clone(),
                            })
                            .await?;
                    } else {
                        self.notifier
                            .send(NotificationEvent::Changed {
                                url: self.url.clone(),
                                old: prev.clone(),
                                new: current.clone(),
                            })
                            .await?;

                        self.previous = Some(current);
                        self.previous_hash = current_hash;
                    }
                } else {
                    self.previous = Some(current);
                    self.previous_hash = current_hash;
                }
            }
            Err(err) => {
                if !self.last_failed {
                    self.last_failed = true;

                    let event = match &err {
                        DominionRequestError::HttpRequestFailed { url, status, body } => {
                            NotificationEvent::Failed {
                                url: url.clone(),
                                reason: format!("{err}"),
                                status: Some(*status),
                                body: Some(body.clone()),
                            }
                        }
                        _ => NotificationEvent::Failed {
                            url: self.url.clone(),
                            reason: format!("{err}"),
                            status: None,
                            body: None,
                        },
                    };

                    self.notifier.send(event).await?;
                }
            }
        }

        Ok(())
    }

    async fn fetch(&self) -> Result<String, DominionRequestError> {
        let mut req = self
            .http_client
            .request(self.method.clone(), self.url.as_str());

        for (name, value) in &self.headers {
            req = req.header(name, value);
        }

        trace!("Fetching {}: {:?}", self.url, req);
        let res = req.send().await?;
        let status = res.status();
        trace!("Fetched {}: {:?}", self.url, res);

        let is_json = res
            .headers()
            .get(CONTENT_TYPE)
            .map(|ct| ct.to_str().unwrap_or_default().contains("json"))
            .unwrap_or(false);

        let text = if is_json {
            let json = res.json::<serde_json::Value>().await?;

            let mut buf = Vec::new();
            let formatter = PrettyFormatter::with_indent(b"    ");
            let mut ser = Serializer::with_formatter(&mut buf, formatter);
            json.serialize(&mut ser)?;

            String::from_utf8(buf).map_err(|_err| DominionRequestError::InvalidJson(json))?
        } else {
            res.text().await?
        };

        if !status.is_success() {
            return Err(DominionRequestError::HttpRequestFailed {
                url: self.url.clone(),
                status,
                body: text,
            });
        }

        Ok(text)
    }

    fn mask_value(&self, value: String) -> String {
        match &self.ignore_mask {
            None => value,
            Some(mask) => mask.replace_all(value.as_str(), "__ignored__").to_string(),
        }
    }

    fn hash(value: &str) -> u64 {
        let mut hasher = hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod test {
    use crate::watch::*;

    #[test]
    fn mask() {
        let ignore_patterns = vec![
            "foo".to_string(),
            "bar".to_string(),
            r#""eventDate": "\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}.\d{3}Z""#.to_string(),
        ];
        let entry = WatchEntry {
            ignore: ignore_patterns,
            ..WatchEntry::default()
        };
        let (tx, _) = mpsc::channel::<NotificationEvent>(1);
        let http_cfg = HttpConfig::default();
        let watcher = Watcher::new(&entry, tx, &http_cfg).unwrap();

        let value = r#"{
	"key": "value",
	"foo": "bar",
	"foo": "bar",
	"key2": "bar",
	"eventDate": "2023-06-25T23:59:59.999Z",
	"eventDate": "2023-06-25T23:59:59.999Z",
	"eventDate": "0000-00-00T00:00:00.000Z",
	"eventDate": "0000-00-00T00:00:00.0000Z",
	"eventDate": "aaaa-aa-aaTaa:aa:aa.aaaZ",
	"foo1": "bar1"
}"#
        .to_string();

        let expected = r#"{
	"key": "value",
	"__ignored__": "__ignored__",
	"__ignored__": "__ignored__",
	"key2": "__ignored__",
	__ignored__,
	__ignored__,
	__ignored__,
	"eventDate": "0000-00-00T00:00:00.0000Z",
	"eventDate": "aaaa-aa-aaTaa:aa:aa.aaaZ",
	"__ignored__1": "__ignored__1"
}"#
        .to_string();

        assert_eq!(watcher.mask_value(value), expected);
    }
}
