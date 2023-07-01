use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::Sender;
use tracing::{info, trace};

use watch::Watcher;

use crate::config::{Config, WatchEntry};
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
    for entry in &cfg.watch {
        prepare_watcher(entry, tx.clone(), &cfg)?;
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
    entry: &WatchEntry,
    tx: Sender<NotificationEvent>,
    cfg: &Config,
) -> Result<(), DominionError> {
    let tx_spawn = tx;
    let tx_inner = tx_spawn.clone();

    let interval = Arc::new(entry.interval);
    let stagger = Arc::new(entry.stagger);
    let variation = Arc::new(entry.variation);

    let mut watcher = Watcher::new(entry, tx_inner, &cfg.http)?;

    tokio::spawn(async move {
        // Delay initial fetch by `stagger`
        trace!(
            "Doing initial fetch of {} in {}",
            watcher.url,
            config::format_duration(&stagger)
        );
        tokio::time::sleep(*stagger).await;

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

            // Delay next fetch by `interval` plus random variation between 0s and `variation`
            let interval = *interval;
            let var = interval.as_secs_f32() * (*variation) * rand::random::<f32>();
            let var = Duration::from_secs(var as u64);
            let next_fetch = interval + var;
            trace!(
                "Doing next fetch of {} in {}",
                watcher.url,
                config::format_duration(&next_fetch)
            );
            tokio::time::sleep(next_fetch).await;
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
