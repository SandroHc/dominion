use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

use crate::config::Config;
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
    async fn on_no_changes(&mut self, url: &str);
    async fn on_failed(&mut self, url: &str, reason: &str);
}

pub async fn prepare_notifier(cfg: &Config) -> Result<Sender<NotificationEvent>, DominionError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<NotificationEvent>(1);

    let mut discord_handler = DiscordEventHandler::new(&cfg.discord).await?;
    let mut mail_handler = MailEventHandler::new(&cfg.email).await?;

    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match message {
                NotificationEvent::Startup { urls } => {
                    discord_handler.on_startup(urls.as_slice()).await;
                    mail_handler.on_startup(urls.as_slice()).await;
                }
                NotificationEvent::Changed { url, old, new } => {
                    info!("Found changes in {url}");
                    discord_handler
                        .on_changed(url.as_str(), old.as_str(), new.as_str())
                        .await;
                    mail_handler
                        .on_changed(url.as_str(), old.as_str(), new.as_str())
                        .await;
                }
                NotificationEvent::NoChanges { url } => {
                    discord_handler.on_no_changes(url.as_str()).await;
                    mail_handler.on_no_changes(url.as_str()).await;
                }
                NotificationEvent::Failed { url, reason } => {
                    error!("Failed to fetch {url}: {reason}");
                    discord_handler
                        .on_failed(url.as_str(), reason.as_str())
                        .await;
                    mail_handler.on_failed(url.as_str(), reason.as_str()).await;
                }
            }
        }
    });

    Ok(tx)
}
