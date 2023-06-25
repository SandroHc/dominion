use std::time::Duration;

use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
	pub notify: Vec<String>,
	pub email: EmailConfig,
	pub watch: Vec<WatchEntry>,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			notify: vec!["email".to_string()],
			email: EmailConfig {
				send_to: "contact@example.com".to_string(),
			},
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

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct EmailConfig {
	pub send_to: String,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct WatchEntry {
	pub url: String,
	#[serde(deserialize_with = "deserialize_duration")]
	pub interval: Duration,
	#[serde(default)]
	pub ignore: Vec<String>,
}
