use async_trait::async_trait;
use serenity::builder::CreateMessage;
use serenity::http::Http;
use serenity::model::channel::{Message, PrivateChannel};
use serenity::model::id::UserId;
use similar::{ChangeTag, TextDiff};
use tracing::error;

use crate::config::DiscordConfig;
use crate::error::DominionDiscordError;

pub struct DiscordEventHandler {
    http: Http,
    dm: PrivateChannel,
    _msg: Option<Message>,
}

impl DiscordEventHandler {
    pub async fn new(cfg: &DiscordConfig) -> Result<DiscordEventHandler, DominionDiscordError> {
        let http = Http::new(cfg.token.as_str());

        let owner = UserId::from(cfg.owner);
        let dm = owner.create_dm_channel(&http).await?;

        Ok(Self {
            http,
            dm,
            _msg: None,
        })
    }

    async fn send<'msg, M>(&self, m: M)
    where
        for<'b> M: FnOnce(&'b mut CreateMessage<'msg>) -> &'b mut CreateMessage<'msg>,
    {
        if let Err(err) = self.dm.send_message(&self.http, m).await {
            error!("Failed to send Discord message: {err}");
        }
    }
}

#[async_trait]
impl crate::notify::EventHandler for DiscordEventHandler {
    async fn on_startup(&mut self, urls: &[String]) {
        let mut urls_joined = String::new();
        for url in urls {
            urls_joined += format!("- {url}").as_str();
        }

        let content = format!("Started listening on the following URLs:\n{urls_joined}");
        self.send(|m| m.content(content)).await;
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

        self.send(|m| m.content(content)).await;
    }

    async fn on_no_changes(&mut self, _url: &str) {
        // TODO: update status message
        // let content = format!("No changes for [{url}](url)");
        // self.send(|m| m.content(content)).await;
    }

    async fn on_failed(&mut self, url: &str, reason: &str) {
        let content = format!("Failed to fetch {url}\n\n{reason}");
        self.send(|m| m.content(content)).await;
    }
}
