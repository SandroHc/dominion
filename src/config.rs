use std::str::FromStr;
use std::time::Duration;

use duration_str::deserialize_duration;
use reqwest::Method;
use serde::{Deserialize, Serialize, Serializer};

const DEFAULT_SMTP_HOST: &str = "127.0.0.1";
const DEFAULT_SMTP_PORT: u16 = lettre::transport::smtp::SMTP_PORT;

const fn default_smtp_port() -> u16 {
    DEFAULT_SMTP_PORT
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Notification methods. Not currently used.
    pub notify: Vec<String>,
    /// Interval between heartbeats
    #[serde(
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration"
    )]
    pub heartbeat: Duration,
    pub log: LogConfig,
    pub http: HttpConfig,
    #[cfg(feature = "discord")]
    pub discord: DiscordConfig,
    #[cfg(feature = "email")]
    pub email: MailConfig,
    pub watch: Vec<WatchEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notify: vec!["email".to_string(), "discord".to_string()],
            heartbeat: Duration::from_secs(60 * 10), // 10 minutes
            log: LogConfig {
                enabled: true,
                level: "warn,dominion=info".to_string(),
                file: None,
            },
            http: HttpConfig::default(),
            #[cfg(feature = "discord")]
            discord: DiscordConfig::default(),
            #[cfg(feature = "email")]
            email: MailConfig::default(),
            watch: vec![
                WatchEntry {
                    protocol: "http".to_string(),
                    url: "https://example.com".to_string(),
                    method: Method::GET,
                    headers: vec![],
                    interval: Duration::from_secs(30),
                    variation: 0.25, // 25% - 1h requests will be in the range of 1h-1h15m
                    stagger: Duration::from_secs(5),
                    ignore: vec![],
                },
                WatchEntry {
                    protocol: "http".to_string(),
                    url: "https://example2.com".to_string(),
                    method: Method::GET,
                    headers: vec![],
                    interval: Duration::from_secs(60 * 10), // 10 minutes
                    variation: default_variation(),
                    stagger: default_stagger(),
                    ignore: vec![],
                },
            ],
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LogConfig {
    pub enabled: bool,
    pub level: String,
    pub file: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HttpConfig {
    pub user_agent: Option<String>,
}

#[cfg(feature = "discord")]
#[derive(Debug, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub token: String,
    pub purge: bool,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: "".to_string(),
            purge: false,
        }
    }
}

#[cfg(feature = "email")]
#[derive(Debug, Serialize, Deserialize)]
pub struct MailConfig {
    pub enabled: bool,
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    pub smtp_use_tls: bool,
    #[serde(default)]
    pub smtp_username: String,
    #[serde(default)]
    pub smtp_password: String,
    pub from_address: String,
    pub to_address: String,
}

#[cfg(feature = "email")]
impl Default for MailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            smtp_host: DEFAULT_SMTP_HOST.to_string(),
            smtp_port: DEFAULT_SMTP_PORT,
            smtp_use_tls: true,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            from_address: "Dominion <dominion@example.com>".to_string(),
            to_address: "".to_string(),
        }
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct WatchEntry {
    #[serde(default = "default_protocol", skip_serializing_if = "skip_protocol")]
    pub protocol: String,

    // HTTP params
    pub url: String,
    #[serde(
        default = "default_method",
        skip_serializing_if = "skip_method",
        serialize_with = "serialize_method",
        deserialize_with = "deserialize_method"
    )]
    pub method: Method,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,

    #[serde(
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration"
    )]
    pub interval: Duration,
    /// Variation in time, in percentage, between requests.
    #[serde(default = "default_variation", skip_serializing_if = "skip_variation")]
    pub variation: f32,
    /// Initial requests will be staggered a random amount between 0s and this value.
    #[serde(
        default = "default_stagger",
        skip_serializing_if = "skip_stagger",
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration"
    )]
    pub stagger: Duration,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
}

fn default_protocol() -> String {
    "http".to_string()
}

fn skip_protocol(value: &String) -> bool {
    value.is_empty() || value == &default_protocol()
}

fn default_method() -> Method {
    Method::GET
}

fn skip_method(value: &Method) -> bool {
    value == default_method()
}

fn serialize_method<S>(value: &Method, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(value.to_string().as_str())
}

struct MethodDeserializer;

impl<'de> serde::de::Visitor<'de> for MethodDeserializer {
    type Value = Method;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("expected string, e.g. 'GET'")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let duration = Method::from_str(s).map_err(serde::de::Error::custom)?;
        Ok(duration)
    }
}

pub fn deserialize_method<'de, D>(deserializer: D) -> Result<Method, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_any(MethodDeserializer)
}

fn default_variation() -> f32 {
    0.0
}

fn skip_variation(value: &f32) -> bool {
    *value == default_variation()
}

fn default_stagger() -> Duration {
    Duration::from_secs(0)
}

fn skip_stagger(value: &Duration) -> bool {
    value.as_secs_f32() == default_stagger().as_secs_f32()
}

pub fn format_duration(value: &Duration) -> String {
    const SECS_IN_MINUTE: u64 = 60;
    const SECS_IN_HOUR: u64 = SECS_IN_MINUTE * 60;
    const SECS_IN_DAY: u64 = SECS_IN_HOUR * 24;
    const SECS_IN_WEEK: u64 = SECS_IN_DAY * 7;
    const SECS_IN_MONTH: u64 = SECS_IN_DAY * 30;
    const SECS_IN_YEAR: u64 = SECS_IN_MONTH * 12;
    const NANOS_IN_MICRO: u32 = 1000;
    const NANOS_IN_MILLI: u32 = NANOS_IN_MICRO * 1000;

    let mut secs = value.as_secs();
    let mut nanos = value.subsec_nanos();

    let mut str = String::new();

    if secs >= SECS_IN_YEAR {
        let years = secs / SECS_IN_YEAR;
        secs -= years * SECS_IN_YEAR;
        str.push_str(format!("{years}y").as_str());
    }
    if secs >= SECS_IN_MONTH {
        let months = secs / SECS_IN_MONTH;
        secs -= months * SECS_IN_MONTH;
        str.push_str(format!("{months}mon").as_str());
    }
    if secs >= SECS_IN_WEEK {
        let weeks = secs / SECS_IN_WEEK;
        secs -= weeks * SECS_IN_WEEK;
        str.push_str(format!("{weeks}w").as_str());
    }
    if secs >= SECS_IN_DAY {
        let days = secs / SECS_IN_DAY;
        secs -= days * SECS_IN_DAY;
        str.push_str(format!("{days}d").as_str());
    }
    if secs >= SECS_IN_HOUR {
        let hours = secs / SECS_IN_HOUR;
        secs -= hours * SECS_IN_HOUR;
        str.push_str(format!("{hours}h").as_str());
    }
    if secs >= SECS_IN_MINUTE {
        let mins = secs / SECS_IN_MINUTE;
        secs -= mins * SECS_IN_MINUTE;
        str.push_str(format!("{mins}m").as_str());
    }
    if secs > 0 {
        str.push_str(format!("{secs}s").as_str());
    }

    if nanos >= NANOS_IN_MILLI {
        let millis = nanos / NANOS_IN_MILLI;
        nanos -= millis * NANOS_IN_MILLI;
        str.push_str(format!("{millis}ms").as_str());
    }
    if nanos >= NANOS_IN_MICRO {
        let micros = nanos / NANOS_IN_MICRO;
        nanos -= micros * NANOS_IN_MICRO;
        str.push_str(format!("{micros}us").as_str());
    }
    if nanos > 0 {
        str.push_str(format!("{nanos}ns").as_str());
    }

    str
}

fn serialize_duration<S>(value: &Duration, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let str = format_duration(value);
    s.serialize_str(str.as_str())
}
