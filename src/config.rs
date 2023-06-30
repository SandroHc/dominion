use std::time::Duration;

use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize, Serializer};

const DEFAULT_SMTP_HOST: &str = "127.0.0.1";
const DEFAULT_SMTP_PORT: u16 = lettre::transport::smtp::SMTP_PORT;

const fn default_smtp_port() -> u16 {
    DEFAULT_SMTP_PORT
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub notify: Vec<String>,
    pub http: HttpConfig,
    #[cfg(feature = "email")]
    pub email: MailConfig,
    pub watch: Vec<WatchEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notify: vec!["email".to_string()],
            http: HttpConfig::default(),
            #[cfg(feature = "email")]
            email: MailConfig::default(),
            watch: vec![
                WatchEntry {
                    url: "https://example.com".to_string(),
                    interval: Duration::from_secs(60 * 10), // 10 minutes
                    ignore: vec![],
                },
                WatchEntry {
                    url: "https://example2.com".to_string(),
                    interval: Duration::from_secs(60 * 60 * 24), // 1 day
                    ignore: vec![],
                },
            ],
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HttpConfig {
    pub user_agent: Option<String>,
}

#[cfg(feature = "email")]
#[derive(Debug, Serialize, Deserialize)]
pub struct MailConfig {
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
    pub url: String,
    #[serde(
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration"
    )]
    pub interval: Duration,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
}

fn serialize_duration<S>(value: &Duration, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
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

    s.serialize_str(str.as_str())
}
