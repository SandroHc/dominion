use std::sync::Arc;

use similar::TextDiff;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

use watch::Watcher;

use crate::config::{Config, WatchEntry};
use crate::error::DominionError;

mod config;
mod error;
mod watch;

#[derive(Debug)]
pub enum NotificationEvent {
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

	let tx = prepare_notifier();

	for entry in cfg.watch {
		prepare_watcher(entry, tx.clone())?;
	}

	info!("Dominion started - notifications will be sent to the email '{}'", cfg.email.send_to);

	shutdown_on_ctrl_c().await?;
	Ok(())
}

// TODO: move to `notify` module
fn prepare_notifier() -> Sender<NotificationEvent> {
	let (tx, mut rx) = tokio::sync::mpsc::channel::<NotificationEvent>(1);

	tokio::spawn(async move {
		while let Some(message) = rx.recv().await {
			match message {
				NotificationEvent::Changed { url, old, new } => {
					let diff = TextDiff::from_lines(old.as_str(), new.as_str());
					info!("Found changes in {url}");
					println!("{}", diff.unified_diff());

					// TODO: create colored diff in HTML format
					// for change in diff.iter_all_changes() {
					// 	let sign = match change.tag() {
					// 		ChangeTag::Delete => "-",
					// 		ChangeTag::Insert => "+",
					// 		ChangeTag::Equal => " ",
					// 	};
					// 	warn!("{}{}", sign, change);
					// }
				}
				NotificationEvent::Failed { url, reason } => {
					error!("Failed to fetch {url}: {reason}");
				}
			}
		}
	});

	tx
}

/// Loads the app configurations from a file, or creates one with default values if it doesn't exist.
///
/// On Linux systems, the file can be found on "/home/$USER/.config/dominion/dominion.toml".
fn load_config() -> Result<Config, DominionError> {
	let config_path = confy::get_configuration_file_path("dominion", "dominion")?;
	info!("Loading config from '{}'", config_path.display());

	let config = confy::load_path::<Config>(config_path)?;
	info!("Loaded config: {:?}", config);

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
				let notify_result = tx_spawn.send(NotificationEvent::Failed {
					url: watcher.url.clone(),
					reason: format!("{err}"),
				}).await;
				if let Err(notify_err) = notify_result {
					panic!("watcher failed with [{err}] while checking {}, and then failed again \
					with {notify_err} while sending failure notification", watcher.url);
				}
			}

			tokio::time::sleep(*interval).await;
		}
	});
	Ok(())
}

async fn shutdown_on_ctrl_c() -> Result<(), DominionError> {
	tokio::spawn(async move {
		tokio::signal::ctrl_c().await.expect("Could not await CTRL-C");

		info!("Stopping Dominion");
	}).await?;

	Ok(())
}
