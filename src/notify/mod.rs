use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use tracing::{error, info, trace};

use crate::config::{Config, WatchEntry};
use crate::error::DominionError;
use crate::notify::discord::DiscordEventHandler;
use crate::notify::mail::MailEventHandler;
use crate::NotificationEvent;

#[cfg(feature = "discord")]
mod discord;
#[cfg(feature = "email")]
mod mail;

#[async_trait]
trait EventHandler {
    async fn on_startup(&mut self, urls: &[String]);
    async fn on_changed(&mut self, url: &str, old: &str, new: &str);
    async fn on_failed(&mut self, url: &str, reason: &str);
    async fn on_heartbeat(&mut self, status: &Heartbeat);
}

struct Heartbeat {
    items: Vec<HeartbeatItem>,
    dirty: bool,
}

impl Heartbeat {
    fn from(entries: &[WatchEntry]) -> Self {
        let mut heartbeat = Self {
            items: Vec::with_capacity(entries.len()),
            dirty: false,
        };

        entries
            .iter()
            .map(|w| HeartbeatItem::new(w.url.as_str()))
            .for_each(|e| heartbeat.items.push(e));

        heartbeat
    }

    fn update(&mut self, url: &str, update_type: HeartbeatUpdateType) {
        self.dirty = true;

        for item in &mut self.items {
            if item.url == url {
                item.update(update_type);
                return;
            }
        }

        // Didn't find the URL. Weird, but add it
        let mut item = HeartbeatItem::new(url);
        item.update(update_type);
        self.items.push(item);
    }
}

struct HeartbeatItem {
    url: String,
    last_update: Option<u64>,
    last_change: Option<u64>,
    last_failure: Option<u64>,
}

impl HeartbeatItem {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            last_update: None,
            last_change: None,
            last_failure: None,
        }
    }

    fn update(&mut self, update_type: HeartbeatUpdateType) {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();
        let now = Some(epoch);

        self.last_update = now;
        match update_type {
            HeartbeatUpdateType::Change => {
                self.last_change = now;
            }
            HeartbeatUpdateType::Failure => {
                self.last_failure = now;
            }
            HeartbeatUpdateType::NoChange => {}
        }
    }
}

enum HeartbeatUpdateType {
    Change,
    NoChange,
    Failure,
}

pub async fn prepare_notifier(cfg: &Config) -> Result<Sender<NotificationEvent>, DominionError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<NotificationEvent>(1);

    let discord_handler = Arc::new(Mutex::new(DiscordEventHandler::new(&cfg.discord).await?));
    let mail_handler = Arc::new(Mutex::new(MailEventHandler::new(&cfg.email).await?));

    let heartbeat = Arc::new(RwLock::new(Heartbeat::from(cfg.watch.as_slice())));

    // Notifiers
    {
        let discord_handler = discord_handler.clone();
        let mail_handler = mail_handler.clone();
        let heartbeat = heartbeat.clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                match message {
                    NotificationEvent::Startup { urls } => {
                        discord_handler
                            .lock()
                            .await
                            .on_startup(urls.as_slice())
                            .await;
                        mail_handler.lock().await.on_startup(urls.as_slice()).await;
                    }
                    NotificationEvent::Changed { url, old, new } => {
                        info!("Found changes in {url}");
                        heartbeat
                            .write()
                            .await
                            .update(url.as_str(), HeartbeatUpdateType::Change);
                        discord_handler
                            .lock()
                            .await
                            .on_changed(url.as_str(), old.as_str(), new.as_str())
                            .await;
                        mail_handler
                            .lock()
                            .await
                            .on_changed(url.as_str(), old.as_str(), new.as_str())
                            .await;
                    }
                    NotificationEvent::NoChanges { url } => {
                        heartbeat
                            .write()
                            .await
                            .update(url.as_str(), HeartbeatUpdateType::NoChange);
                    }
                    NotificationEvent::Failed { url, reason } => {
                        error!("Failed to fetch {url}: {reason}");
                        heartbeat
                            .write()
                            .await
                            .update(url.as_str(), HeartbeatUpdateType::Failure);
                        discord_handler
                            .lock()
                            .await
                            .on_failed(url.as_str(), reason.as_str())
                            .await;
                        mail_handler
                            .lock()
                            .await
                            .on_failed(url.as_str(), reason.as_str())
                            .await;
                    }
                }
            }
        });
    }

    // Heartbeat
    {
        let heartbeat_interval = cfg.heartbeat;
        let discord_handler = discord_handler.clone();
        let mail_handler = mail_handler.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep_until(Instant::now() + heartbeat_interval).await;
                trace!("Sending heartbeat");

                {
                    let heartbeat_guard = heartbeat.read().await;

                    discord_handler
                        .lock()
                        .await
                        .on_heartbeat(&heartbeat_guard)
                        .await;
                    mail_handler
                        .lock()
                        .await
                        .on_heartbeat(&heartbeat_guard)
                        .await;
                }

                {
                    let mut heartbeat_guard = heartbeat.write().await;
                    heartbeat_guard.dirty = false;
                }
            }
        });
    }

    Ok(tx)
}
