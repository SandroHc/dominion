use std::time::Duration;

use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
	pub notify: Vec<String>,
	#[cfg(feature = "email")]
	pub email: EmailConfig,
	pub watch: Vec<WatchEntry>,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			notify: vec!["email".to_string()],
			#[cfg(feature = "email")]
			email: EmailConfig::default(),
			watch: vec![
				WatchEntry {
					url: "https://example.com".to_string(),
					interval: Duration::from_secs(30),
					ignore: vec![],
				}
			],
		}
	}
}

#[cfg(feature = "email")]
#[derive(Debug, Serialize, Deserialize)]
pub struct EmailConfig {
	pub smtp_host: String,
	pub smtp_username: String,
	pub smtp_password: String,
	pub from_address: String,
	pub to_address: String,
}

#[cfg(feature = "email")]
impl Default for EmailConfig {
	fn default() -> Self {
		Self {
			smtp_host: "".to_string(),
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
	#[serde(deserialize_with = "deserialize_duration")]
	pub interval: Duration,
	#[serde(default)]
	pub ignore: Vec<String>,
}
