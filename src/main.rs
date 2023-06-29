use lettre::message::{IntoBody, SinglePart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde_json::json;
use std::sync::Arc;

use similar::{ChangeTag, TextDiff};
use tokio::sync::mpsc::Sender;
use tracing::{error, info, trace};

use watch::Watcher;

use crate::config::{Config, EmailConfig, WatchEntry};
use crate::email::{CodeBlock, CodeBlockLine};
use crate::error::DominionError;

mod config;
#[cfg(feature = "email")]
mod email;
mod error;
mod watch;

#[derive(Debug)]
pub enum NotificationEvent {
    Startup {
        urls: Vec<String>,
    },
    Changed {
        url: String,
        old: String,
        new: String,
    },
    Failed {
        url: String,
        reason: String,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), DominionError> {
    tracing_subscriber::fmt::init();

    let cfg = load_config().expect("invalid configuration");
    let urls = cfg.watch.iter().map(|w| w.url.clone()).collect();

    let tx = prepare_notifier(cfg.email).await?;
    for entry in cfg.watch {
        prepare_watcher(entry, tx.clone())?;
    }

    info!("Dominion started");
    tx.send(NotificationEvent::Startup { urls }).await?;

    shutdown_on_ctrl_c().await?;
    Ok(())
}

// TODO: move to `notify` module
async fn prepare_notifier(
    mail_cfg: EmailConfig,
) -> Result<Sender<NotificationEvent>, DominionError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<NotificationEvent>(1);

    let mailer = email::mailer(&mail_cfg).await?;
    let from_addr = mail_cfg.from_address;
    let to_addr = mail_cfg.to_address;
    let template_engine = email::template_engine()?;

    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match message {
                NotificationEvent::Startup { urls } => {
                    let mut urls_joined = "".to_string();
                    for url in urls {
                        urls_joined += format!("<li>{url}</li>").as_str();
                    }

                    let content = format!(
                        "<p>Started listening on the following URLs:</p><ol>{urls_joined}</ol>"
                    );
                    let data = json!({ "content": content });

                    let Ok(body) = template_engine.render("template", &data) else {
						error!("Failed to render email template");
						return;
					};

                    let subject = "Startup report";
                    let mail =
                        send_email(&mailer, from_addr.clone(), to_addr.clone(), subject, body)
                            .await;
                    if let Err(err) = mail {
                        error!("Failed to send started email: {err}");
                    }
                }
                NotificationEvent::Changed { url, old, new } => {
                    info!("Found changes in {url}");

                    let content = format!(
                        r#"The following changes were found in <a target="_blank" href="{url}">{url}</a>"#
                    );

                    let diff = TextDiff::from_lines(old.as_str(), new.as_str());
                    let mut lines = vec![];
                    for group in diff.grouped_ops(5) {
                        let (_, start_old_range, start_new_range) =
                            group.first().unwrap().as_tag_tuple();
                        let (_, end_old_range, end_new_range) =
                            group.last().unwrap().as_tag_tuple();

                        lines.push(CodeBlockLine {
                            r#type: "summary".to_string(),
                            old_index: None,
                            new_index: None,
                            content: format!(
                                "@@ -{},{} +{},{} @@",
                                start_old_range.start,
                                end_old_range.end - start_old_range.start,
                                start_new_range.start,
                                end_new_range.end - start_new_range.start
                            ),
                        });

                        for op in group {
                            for change in diff.iter_inline_changes(&op) {
                                let (change_type, sign) = match change.tag() {
                                    ChangeTag::Delete => ("deletion", "-"),
                                    ChangeTag::Insert => ("addition", "+"),
                                    ChangeTag::Equal => ("", "&nbsp;"),
                                };

                                let mut line = sign.to_string();
                                change
                                    .values()
                                    .iter()
                                    .map(|(emphasized, value)| {
                                        (emphasized, value.replace(' ', "&nbsp;"))
                                    })
                                    .map(|(emphasized, value)| {
                                        if *emphasized {
                                            format!(r#"<span class="emphasized">{value}</span>"#)
                                        } else {
                                            value
                                        }
                                    })
                                    .for_each(|value| line.push_str(value.as_str()));

                                lines.push(CodeBlockLine {
                                    r#type: change_type.to_string(),
                                    old_index: change.old_index(),
                                    new_index: change.new_index(),
                                    content: line,
                                });
                            }
                        }
                    }

                    let code = CodeBlock { lines };

                    let data = json!({
                        "content": content,
                        "code": code
                    });

                    let Ok(body) = template_engine.render("template", &data) else {
						error!("Failed to render email template");
						return;
					};

                    let subject = format!("Changes in {}", url);
                    let mail =
                        send_email(&mailer, from_addr.clone(), to_addr.clone(), subject, body)
                            .await;
                    match mail {
                        Ok(_) => trace!("Email for changes in {url} sent"),
                        Err(err) => error!("Failed to send email for changes in {url}: {err}"),
                    }
                }
                NotificationEvent::Failed { url, reason } => {
                    error!("Failed to fetch {url}: {reason}");

                    let content = format!("<p>Failed to fetch {url}</p><p>{reason}</p>");
                    let data = json!({ "content": content });

                    let Ok(body) = template_engine.render("template", &data) else {
						error!("Failed to render email template");
						return;
					};

                    let subject = "Startup report";
                    let mail =
                        send_email(&mailer, from_addr.clone(), to_addr.clone(), subject, body)
                            .await;
                    if let Err(err) = mail {
                        error!("Failed to send failure email: {err}");
                    }
                }
            }
        }
    });

    Ok(tx)
}

async fn send_email<S: Into<String>, B: IntoBody>(
    mailer: &AsyncSmtpTransport<Tokio1Executor>,
    from_addr: String,
    to_addr: String,
    subject: S,
    body: B,
) -> Result<(), DominionError> {
    let mail = Message::builder()
        .from(from_addr.parse()?)
        .to(to_addr.parse()?)
        .subject(subject)
        .singlepart(SinglePart::html(body))?;

    mailer.send(mail).await?;

    Ok(())
}

/// Loads the app configurations from a file, or creates one with default values if it doesn't exist.
///
/// On Linux systems, the file can be found on "/home/$USER/.config/dominion/dominion.toml".
fn load_config() -> Result<Config, DominionError> {
    let config_path = confy::get_configuration_file_path("dominion", "dominion")?;
    info!("Loading config from '{}'", config_path.display());

    let config = confy::load_path::<Config>(config_path)?;

    Ok(config)
}

fn prepare_watcher(entry: WatchEntry, tx: Sender<NotificationEvent>) -> Result<(), DominionError> {
    let tx_spawn = tx;
    let tx_inner = tx_spawn.clone();

    let mut watcher = Watcher::new(entry.url, entry.ignore.as_slice(), tx_inner)?;
    let interval = Arc::new(entry.interval);

    tokio::spawn(async move {
        loop {
            if let Err(err) = watcher.watch().await {
                // Handle error by sending failure notification
                let notify_result = tx_spawn
                    .send(NotificationEvent::Failed {
                        url: watcher.url.clone(),
                        reason: format!("{err}"),
                    })
                    .await;
                if let Err(notify_err) = notify_result {
                    panic!(
                        "watcher failed with [{err}] while checking {}, and then failed again \
					with {notify_err} while sending failure notification",
                        watcher.url
                    );
                }
            }

            tokio::time::sleep(*interval).await;
        }
    });
    Ok(())
}

async fn shutdown_on_ctrl_c() -> Result<(), DominionError> {
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Could not await CTRL-C");

        info!("Stopping Dominion");
    })
    .await?;

    Ok(())
}
