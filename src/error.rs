use crate::NotificationEvent;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DominionError {
    #[error("async error: {0}")]
    Async(#[from] DominionAsyncError),
    #[error("config error: {0}")]
    Config(#[from] DominionConfigError),
    #[cfg(feature = "discord")]
    #[error("Discord error: {0}")]
    Discord(#[from] DominionDiscordError),
    #[error("log error: {0}")]
    Log(#[from] DominionLogError),
    #[cfg(feature = "email")]
    #[error("mail error: {0}")]
    Mail(#[from] DominionMailError),
    #[error("request error: {0}")]
    Request(#[from] DominionRequestError),
    #[error(transparent)]
    Unknown(#[from] Box<dyn std::error::Error + Send>),
}

#[derive(Error, Debug)]
pub enum DominionAsyncError {
    #[error("thread join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("channel send error: {0}")]
    TokioChannelSend(#[from] tokio::sync::mpsc::error::SendError<NotificationEvent>),
}

#[derive(Error, Debug)]
pub enum DominionRequestError {
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid JSON: {0}")]
    InvalidJson(serde_json::Value),
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
}

#[cfg(feature = "discord")]
#[derive(Error, Debug)]
pub enum DominionDiscordError {
    #[error("serenity error: {0}")]
    Serenity(#[from] serenity::Error),
}

#[cfg(feature = "email")]
#[derive(Error, Debug)]
pub enum DominionMailError {
    #[error("email error: {0}")]
    EmailMessage(#[from] lettre::error::Error),
    #[error("email SMTP error: {0}")]
    EmailSmtp(#[from] lettre::transport::smtp::Error),
    #[error("email address error: {0}")]
    EmailAddress(#[from] lettre::address::AddressError),
    #[error("template error: {0}")]
    HandlebarsTemplate(#[from] Box<handlebars::TemplateError>),
    #[error("render error: {0}")]
    HandlebarsRender(#[from] handlebars::RenderError),
}

#[derive(Error, Debug)]
pub enum DominionConfigError {
    #[error(transparent)]
    Generic(#[from] confy::ConfyError),
    #[error("could not determine home directory path")]
    BadConfigDirectory,
    #[error("error loading config from '{file}': {source}")]
    Load {
        file: String,
        source: confy::ConfyError,
    },
}

#[derive(Error, Debug)]
pub enum DominionLogError {
    #[error("init error: {0}")]
    Init(#[from] tracing_subscriber::util::TryInitError),
    #[error("error parsing filter '{level}': {source}")]
    FilterParsing {
        level: String,
        source: tracing_subscriber::filter::ParseError,
    },
}
