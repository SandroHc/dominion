use async_trait::async_trait;
use handlebars::{no_escape, Handlebars};
use lettre::message::{Mailbox, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use tracing::{error, trace};

use crate::config::MailConfig;
use crate::error::DominionMailError;
use crate::notify::{EventHandler, Heartbeat};

#[derive(Serialize)]
struct CodeBlock {
    pub lines: Vec<CodeBlockLine>,
}

#[derive(Serialize)]
struct CodeBlockLine {
    /// One of: summary, deletion, addition
    pub r#type: String,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub content: String,
}

pub struct MailEventHandler<'te> {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    template_engine: Handlebars<'te>,
    from_addr: Mailbox,
    to_addr: Mailbox,
}

impl<'te> MailEventHandler<'te> {
    pub async fn new(cfg: &MailConfig) -> Result<MailEventHandler<'te>, DominionMailError> {
        Ok(Self {
            mailer: create_mailer(cfg).await?,
            template_engine: create_template_engine()?,
            from_addr: cfg.from_address.as_str().parse()?,
            to_addr: cfg.to_address.as_str().parse()?,
        })
    }

    async fn send_mail<S: Into<String>>(
        &self,
        subject: S,
        data: serde_json::Value,
    ) -> Result<(), DominionMailError> {
        let body = self.template_engine.render("template", &data)?;
        let mail = Message::builder()
            .from(self.from_addr.clone())
            .to(self.to_addr.clone())
            .subject(subject)
            .singlepart(SinglePart::html(body))?;

        self.mailer.send(mail).await?;

        Ok(())
    }
}

#[async_trait]
impl<'te> EventHandler for MailEventHandler<'te> {
    async fn on_startup(&mut self, urls: &[String]) {
        let mut urls_joined = String::new();
        for url in urls {
            urls_joined += format!("<li>{url}</li>").as_str();
        }

        let content =
            format!("<p>Started listening on the following URLs:</p><ol>{urls_joined}</ol>");

        let subject = "Startup report";
        let body = json!({ "content": content });

        let result = self.send_mail(subject, body).await;
        if let Err(err) = result {
            error!("Failed to send started email: {err}");
        }
    }

    async fn on_changed(&mut self, url: &str, old: &str, new: &str) {
        let content = format!(
            r#"The following changes were found in <a target="_blank" href="{url}">{url}</a>"#
        );

        let diff = TextDiff::from_lines(old, new);
        let mut lines = vec![];
        for group in diff.grouped_ops(5) {
            let (_, start_old_range, start_new_range) = group.first().unwrap().as_tag_tuple();
            let (_, end_old_range, end_new_range) = group.last().unwrap().as_tag_tuple();

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
                        .map(|(emphasized, value)| (emphasized, value.replace(' ', "&nbsp;")))
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

        let subject = format!("Changes in {}", url);
        let body = json!({
            "content": content,
            "code": code
        });

        let result = self.send_mail(subject, body).await;
        match result {
            Ok(_) => trace!("Email for changes in {url} sent"),
            Err(err) => error!("Failed to send email for changes in {url}: {err}"),
        }
    }

    async fn on_failed(
        &mut self,
        url: &str,
        reason: &str,
        _status: &Option<StatusCode>,
        _body: &Option<String>,
    ) {
        let content = format!("<p>Failed to fetch {url}</p><p>{reason}</p>");

        let subject = "Failed report";
        let body = json!({ "content": content });

        let result = self.send_mail(subject, body).await;
        if let Err(err) = result {
            error!("Failed to send failure email: {err}");
        }
    }

    async fn on_heartbeat(&mut self, _status: &Heartbeat) {
        // NO-OP
    }
}

async fn create_mailer(
    cfg: &MailConfig,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, DominionMailError> {
    let host = cfg.smtp_host.clone();

    let mut mailer =
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host.as_str()).port(cfg.smtp_port);

    // Credentials config
    if !cfg.smtp_username.is_empty() {
        let creds = Credentials::new(cfg.smtp_username.clone(), cfg.smtp_password.clone());
        mailer = mailer.credentials(creds);
    }

    // TLS config
    if cfg.smtp_use_tls {
        let tls_params = TlsParameters::new(host)?;
        let tls = if cfg.smtp_port == lettre::transport::smtp::SUBMISSIONS_PORT {
            Tls::Wrapper(tls_params)
        } else {
            Tls::Required(tls_params)
        };
        mailer = mailer.tls(tls);
    } else {
        mailer = mailer.tls(Tls::None);
    }

    let mailer = mailer.build();
    mailer.test_connection().await?;

    Ok(mailer)
}

fn create_template_engine<'te>() -> Result<Handlebars<'te>, DominionMailError> {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars.register_escape_fn(no_escape);
    handlebars
        .register_template_string("template", include_str!("mail.hbs"))
        .map_err(|err| DominionMailError::HandlebarsTemplate(Box::new(err)))?;

    Ok(handlebars)
}
