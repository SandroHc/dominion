use crate::NotificationEvent;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DominionError {
    #[error("configuration error: {0}")]
    Config(#[from] confy::ConfyError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid JSON: {0}")]
    InvalidJson(serde_json::Value),
    #[error("thread join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("channel send error: {0}")]
    TokioChannelSend(#[from] tokio::sync::mpsc::error::SendError<NotificationEvent>),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("HTTP request to {url} failed with status {status} and body: {body}")]
    HttpRequestFailed {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    #[cfg(feature = "email")]
    #[error("email error: {0}")]
    EmailMessage(#[from] lettre::error::Error),
    #[cfg(feature = "email")]
    #[error("email SMTP error: {0}")]
    EmailSmtp(#[from] lettre::transport::smtp::Error),
    #[cfg(feature = "email")]
    #[error("email address error: {0}")]
    EmailAddress(#[from] lettre::address::AddressError),

    #[cfg(feature = "email")]
    #[error("template error: {0}")]
    HandlebarsTemplate(#[from] handlebars::TemplateError),
    #[cfg(feature = "email")]
    #[error("render error: {0}")]
    HandlebarsRender(#[from] handlebars::RenderError),

    #[error(transparent)]
    Unknown(#[from] Box<dyn std::error::Error + Send>),
}
