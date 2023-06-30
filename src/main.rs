use std::sync::Arc;

use tokio::sync::mpsc::Sender;
use tracing::info;

use watch::Watcher;

use crate::config::{Config, HttpConfig, WatchEntry};
use crate::error::{DominionAsyncError, DominionError};

mod config;
mod error;
mod notify;
mod watch;

#[derive(Debug)]
pub enum NotificationEvent {
    Startup {
        urls: Vec<String>,
    },
    Changed {
        url: String,
        old: String,
        new: String,
    },
    NoChanges {
        url: String,
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

    let urls = cfg.watch.iter().map(|w| w.url.clone()).collect();

    let tx = notify::prepare_notifier(&cfg).await?;
    for entry in cfg.watch {
        prepare_watcher(entry, tx.clone(), &cfg.http)?;
    }

    info!("Dominion started");
    tx.send(NotificationEvent::Startup { urls })
        .await
        .map_err(DominionAsyncError::from)?;

    shutdown_on_ctrl_c().await?;
    Ok(())
}

/// Loads the app configurations from a file, or creates one with default values if it doesn't exist.
///
/// On Linux systems, the file can be found on "/home/$USER/.config/dominion/dominion.toml".
fn load_config() -> Result<Config, DominionError> {
    let config_path = confy::get_configuration_file_path("dominion", "dominion")?;
    info!("Loading config from '{}'", config_path.display());

    let config = confy::load_path::<Config>(config_path)?;

    Ok(config)
}

fn prepare_watcher(
    entry: WatchEntry,
    tx: Sender<NotificationEvent>,
    http_cfg: &HttpConfig,
) -> Result<(), DominionError> {
    let tx_spawn = tx;
    let tx_inner = tx_spawn.clone();

    let mut watcher = Watcher::new(entry.url, entry.ignore.as_slice(), tx_inner, &http_cfg)?;
    let interval = Arc::new(entry.interval);

    tokio::spawn(async move {
        loop {
            if let Err(err) = watcher.watch().await {
                // Handle error by sending failure notification
                let notify_result = tx_spawn
                    .send(NotificationEvent::Failed {
                        url: watcher.url.clone(),
                        reason: format!("{err}"),
                    })
                    .await;
                if let Err(notify_err) = notify_result {
                    panic!(
                        "watcher failed with [{err}] while checking {}, and then failed again \
					with {notify_err} while sending failure notification",
                        watcher.url
                    );
                }
            }

            tokio::time::sleep(*interval).await;
        }
    });
    Ok(())
}

async fn shutdown_on_ctrl_c() -> Result<(), DominionAsyncError> {
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Could not await CTRL-C");

        info!("Stopping Dominion");
    })
    .await?;

    Ok(())
}
