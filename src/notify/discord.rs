use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serenity::builder::CreateMessage;
use serenity::http::Http;
use serenity::model::channel::{Message, PrivateChannel};
use serenity::model::id::UserId;
use similar::{ChangeTag, TextDiff};
use tracing::error;

use crate::config::DiscordConfig;
use crate::error::DominionDiscordError;
use crate::notify::Heartbeat;

pub struct DiscordEventHandler {
    http: Http,
    dm: PrivateChannel,
    status_msg: Option<Message>,
}

impl DiscordEventHandler {
    pub async fn new(cfg: &DiscordConfig) -> Result<DiscordEventHandler, DominionDiscordError> {
        let http = Http::new(cfg.token.as_str());

        let owner = UserId::from(cfg.owner);
        let dm = owner.create_dm_channel(&http).await?;

        Ok(Self {
            http,
            dm,
            status_msg: None,
        })
    }

    async fn send<'msg, M>(&self, m: M) -> Result<Message, DominionDiscordError>
    where
        for<'b> M: FnOnce(&'b mut CreateMessage<'msg>) -> &'b mut CreateMessage<'msg>,
    {
        self.dm
            .send_message(&self.http, m)
            .await
            .map_err(DominionDiscordError::from)
    }
}

#[async_trait]
impl crate::notify::EventHandler for DiscordEventHandler {
    async fn on_startup(&mut self, urls: &[String]) {
        let mut content = "Started listening on the following URLs:".to_string();
        for url in urls {
            content += format!("\n- {url}").as_str();
        }

        match self.send(|m| m.content(content)).await {
            Ok(msg) => {
                self.status_msg = Some(msg);
            }
            Err(err) => {
                error!("Failed to send startup message in Discord: {err}");
            }
        }
    }

    async fn on_changed(&mut self, url: &str, old: &str, new: &str) {
        let mut content = format!("Found changes in {url}\n```patch\n");

        let diff = TextDiff::from_lines(old, new);
        for group in diff.grouped_ops(5) {
            let (_, start_old_range, start_new_range) = group.first().unwrap().as_tag_tuple();
            let (_, end_old_range, end_new_range) = group.last().unwrap().as_tag_tuple();

            content += format!(
                "@@ -{},{} +{},{} @@\n",
                start_old_range.start,
                end_old_range.end - start_old_range.start,
                start_new_range.start,
                end_new_range.end - start_new_range.start
            )
            .as_str();

            for op in group {
                for change in diff.iter_changes(&op) {
                    let line = change.value();
                    let prefix = match change.tag() {
                        ChangeTag::Delete => "-",
                        ChangeTag::Insert => "+",
                        ChangeTag::Equal => " ",
                    };
                    let suffix = if change.missing_newline() { "\n" } else { "" };

                    content += format!("{prefix}{line}{suffix}").as_str();
                }
            }
        }

        content += "```";

        let result = self.send(|m| m.content(content)).await;
        if let Err(err) = result {
            error!("Failed to send on change message in Discord: {err}");
        } else {
            self.status_msg = None; // reset status message, so that a new one is sent in the next heartbeat
        }
    }

    async fn on_failed(&mut self, url: &str, reason: &str) {
        let content = format!("Failed to fetch {url}\n\n{reason}");
        let result = self.send(|m| m.content(content)).await;
        if let Err(err) = result {
            error!("Failed to send failure message in Discord: {err}");
        } else {
            self.status_msg = None; // reset status message, so that a new one is sent in the next heartbeat
        }
    }

    async fn on_heartbeat(&mut self, status: &Heartbeat) {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();
        let mut content = format!("**Last updated <t:{}:R>**\n", epoch);
        for item in &status.items {
            content += format!("\n{}\nLast updated ", item.url).as_str();

            match item.last_update {
                None => content += "never",
                Some(last_update) => {
                    content += format!("on <t:{last_update}:R>").as_str();
                }
            }

            if let Some(last_change) = item.last_change {
                content += format!(", changed on <t:{last_change}:R>").as_str();
            }

            if let Some(last_failure) = item.last_failure {
                content += format!(", failed on <t:{last_failure}:R>").as_str();
            }

            content += "\n";
        }

        let result = match &mut self.status_msg {
            None => match self.send(|m| m.content(content)).await {
                Ok(msg) => {
                    self.status_msg = Some(msg);
                    None
                }
                Err(err) => Some(err),
            },
            Some(msg) => msg
                .edit(&self.http, |m| m.content(content))
                .await
                .err()
                .map(DominionDiscordError::from),
        };

        if let Some(err) = result {
            error!("Failed to update status message in Discord: {err}");
        }
    }
}
