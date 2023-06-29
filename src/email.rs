use crate::config::EmailConfig;
use crate::error::DominionError;
use handlebars::{no_escape, Handlebars};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use serde::Serialize;

#[derive(Serialize)]
pub struct CodeBlock {
    pub lines: Vec<CodeBlockLine>,
}

#[derive(Serialize)]
pub struct CodeBlockLine {
    /// One of: summary, deletion, addition
    pub r#type: String,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub content: String,
}

pub async fn mailer(
    cfg: &EmailConfig,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, DominionError> {
    let mut mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(cfg.smtp_host.as_str())?;

    if !cfg.smtp_username.is_empty() {
        let creds = Credentials::new(cfg.smtp_username.clone(), cfg.smtp_password.clone());
        mailer = mailer.credentials(creds);
    }

    let mailer = mailer.build();
    mailer.test_connection().await?;

    Ok(mailer)
}

pub fn template_engine<'reg>() -> Result<Handlebars<'reg>, DominionError> {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars.register_escape_fn(no_escape);
    handlebars.register_template_string("template", include_str!("../email/template.hbs"))?;

    Ok(handlebars)
}
