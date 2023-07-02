use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::StatusCode;
use serenity::builder::CreateMessage;
use serenity::http::Http;
use serenity::model::channel::{AttachmentType, Message, PrivateChannel};
use serenity::model::id::UserId;
use similar::{ChangeTag, TextDiff};
use tracing::{debug, error, info, warn};

use crate::config::DiscordConfig;
use crate::error::DominionDiscordError;
use crate::notify::Heartbeat;

pub struct DiscordEventHandler {
    http: Http,
    bot_id: UserId,
    owner_dm: PrivateChannel,
    status_msg: Option<Message>,
    purge: bool,
}

impl DiscordEventHandler {
    pub async fn new(cfg: &DiscordConfig) -> Result<DiscordEventHandler, DominionDiscordError> {
        let token = cfg.token.as_str();
        let http = Http::new(token);

        let bot_user = http.get_current_user().await?;
        let bot_app = http.get_current_application_info().await?;

        let owner = bot_app.owner;
        let owner_dm = owner.create_dm_channel(&http).await?;

        http.set_application_id(bot_app.id.0);

        Ok(Self {
            http,
            bot_id: bot_user.id,
            owner_dm,
            status_msg: None,
            purge: cfg.purge,
        })
    }

    fn get_diff(old: &str, new: &str) -> String {
        let diff = TextDiff::from_lines(old, new);
        let mut content = String::new();

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

        content
    }

    async fn send<'msg, M>(&self, m: M) -> Result<Message, DominionDiscordError>
    where
        for<'b> M: FnOnce(&'b mut CreateMessage<'msg>) -> &'b mut CreateMessage<'msg>,
    {
        self.owner_dm
            .send_message(&self.http, m)
            .await
            .map_err(DominionDiscordError::from)
    }

    async fn purge_messages(&self) {
        let last_msg_id = self
            .status_msg
            .as_ref()
            .map(|msg| msg.id)
            .or(self.owner_dm.last_message_id);

        let last_msg_id = match last_msg_id {
            None => {
                info!("No messages to purge");
                return;
            }
            Some(last) => last,
        };

        debug!("Purging Discord messages older than {last_msg_id}");

        let msgs_to_delete = match self
            .owner_dm
            .messages(&self.http, |m| m.before(last_msg_id))
            .await
        {
            Ok(to_delete) => to_delete
                .iter()
                .filter(|msg| msg.author.id == self.bot_id)
                .map(|msg| msg.id)
                .collect::<Vec<_>>(),
            Err(err) => {
                error!(
                    "Failed to fetch messages older than {last_msg_id} on channel {}: {err}",
                    self.owner_dm
                );
                return;
            }
        };

        for msg_id in msgs_to_delete {
            if let Err(err) = self.owner_dm.id.delete_message(&self.http, msg_id).await {
                warn!("Failed to purge message {}: {err}", msg_id);
                return;
            }
        }

        info!("Purged all old messages");
    }

    fn trim(value: &str, max: usize) -> (&str, &str, bool) {
        if value.len() > max {
            (&value[0..max], "\n(...)", true)
        } else {
            (value, "", false)
        }
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

        if self.purge {
            self.purge_messages().await;
        }
    }

    async fn on_changed(&mut self, url: &str, old: &str, new: &str) {
        let diff = DiscordEventHandler::get_diff(old, new);

        // Truncate diff as to not exceed Discord limit of 2000 characters per message
        let (diff_trimmed, diff_suffix, was_trimmed) =
            DiscordEventHandler::trim(diff.as_str(), 1800);

        let content = format!("Found changes in {url}\n```patch\n{diff_trimmed}{diff_suffix}```");

        let mut attachments = vec![
            AttachmentType::from((old.as_bytes(), "old.txt")),
            AttachmentType::from((new.as_bytes(), "new.txt")),
        ];
        if was_trimmed {
            attachments.insert(0, AttachmentType::from((diff.as_bytes(), "diff.patch")));
        }

        let result = self
            .send(|m| m.content(content).add_files(attachments))
            .await;
        if let Err(err) = result {
            error!("Failed to send on change message in Discord: {err}");
        } else {
            self.status_msg = None; // reset status message, so that a new one is sent in the next heartbeat
        }
    }

    async fn on_failed(
        &mut self,
        url: &str,
        reason: &str,
        status: &Option<StatusCode>,
        body: &Option<String>,
    ) {
        let mut attachments: Vec<AttachmentType> = vec![];

        let content = match (reason, status, body) {
            (_, Some(status), Some(body)) => {
                let (body_trimmed, body_suffix, was_trimmed) =
                    DiscordEventHandler::trim(body.as_str(), 1800);

                if was_trimmed {
                    attachments.push(AttachmentType::from((body.as_bytes(), "error.txt")));
                }

                format!("Failed to fetch {url} with status __{status}__ and body:\n```\n{body_trimmed}{body_suffix}\n```")
            }
            (reason, _, _) => {
                let (reason_trimmed, reason_suffix, was_trimmed) =
                    DiscordEventHandler::trim(reason, 1800);

                if was_trimmed {
                    attachments.push(AttachmentType::from((reason.as_bytes(), "error.txt")));
                }

                format!(
                    "Failed to fetch {url} because of:\n```\n{reason_trimmed}{reason_suffix}\n```"
                )
            }
        };

        let result = self
            .send(|m| m.content(content).add_files(attachments))
            .await;
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
                    content += format!("<t:{last_update}:R>").as_str();
                }
            }

            if let Some(last_change) = item.last_change {
                content += format!(", changed <t:{last_change}:R>").as_str();
            }

            if let Some(last_failure) = item.last_failure {
                content += format!(", failed <t:{last_failure}:R>").as_str();
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
