use handlebars::{Handlebars, no_escape};
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use lettre::transport::smtp::authentication::Credentials;
use serde::Serialize;
use crate::config::EmailConfig;
use crate::error::DominionError;

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

pub async fn mailer(cfg: &EmailConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, DominionError> {
	let creds = Credentials::new(cfg.smtp_username.clone(), cfg.smtp_password.clone());
	let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(cfg.smtp_host.as_str())?
		.credentials(creds)
		.build();

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
