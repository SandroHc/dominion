use crate::config::MailConfig;
use crate::error::DominionError;
use crate::notify::mail::MailEventHandler;
use crate::NotificationEvent;
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

#[cfg(feature = "email")]
pub mod mail;

#[async_trait]
trait EventHandler {
    async fn on_startup(&self, urls: &[String]);
    async fn on_changed(&self, url: &str, old: &str, new: &str);
    async fn on_failed(&self, url: &str, reason: &str);
}

pub async fn prepare_notifier(
    mail_cfg: &MailConfig,
) -> Result<Sender<NotificationEvent>, DominionError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<NotificationEvent>(1);

    let mail_handler = MailEventHandler::new(mail_cfg).await?;

    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match message {
                NotificationEvent::Startup { urls } => {
                    mail_handler.on_startup(urls.as_slice()).await;
                }
                NotificationEvent::Changed { url, old, new } => {
                    info!("Found changes in {url}");
                    mail_handler
                        .on_changed(url.as_str(), old.as_str(), new.as_str())
                        .await;
                }
                NotificationEvent::NoChanges { url: _ } => {
                    todo!("store absolute times for last recent updates for each url")
                }
                NotificationEvent::Failed { url, reason } => {
                    error!("Failed to fetch {url}: {reason}");
                    mail_handler.on_failed(url.as_str(), reason.as_str()).await;
                }
            }
        }
    });

    Ok(tx)
}
