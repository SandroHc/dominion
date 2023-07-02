use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use directories::ProjectDirs;
use tokio::sync::mpsc::Sender;
use tracing::log::LevelFilter;
use tracing::{debug, info, trace};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::{Layer, Subscriber};
use tracing_subscriber::layer::{Filter, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

use watch::Watcher;

use crate::config::{Config, WatchEntry};
use crate::error::{DominionAsyncError, DominionConfigError, DominionError, DominionLogError};

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
    let (cfg_dir, log_dir) = dirs()?;

    let (cfg, cfg_file) = load_config(cfg_dir)?;
    let _log_guard = init_log(&cfg, log_dir)?;
    info!("Loaded config from '{}'", cfg_file.display());

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

fn dirs() -> Result<(PathBuf, PathBuf), DominionError> {
    let dirs = ProjectDirs::from("net", "SandroHc", "dominion")
        .ok_or(DominionConfigError::BadConfigDirectory)?;

    let config_dir = dirs.config_dir().to_path_buf();
    let log_dir = dirs.data_local_dir().join("logs");

    Ok((config_dir, log_dir))
}

/// Loads the app configurations from a file, or creates one with default values if it doesn't exist.
///
/// On Linux systems, the file can be found on "/home/$USER/.config/dominion/dominion.toml".
fn load_config(cfg_dir: PathBuf) -> Result<(Config, PathBuf), DominionConfigError> {
    let config_file = cfg_dir.join("dominion.toml");
    let config =
        confy::load_path::<Config>(config_file.clone()).map_err(|e| DominionConfigError::Load {
            file: format!("{}", config_file.display()),
            source: e,
        })?;

    Ok((config, config_file))
}

fn init_log(cfg: &Config, default_log_dir: PathBuf) -> Result<WorkerGuard, DominionError> {
    let targets =
        Targets::from_str(&cfg.log.level).map_err(|e| DominionLogError::FilterParsing {
            level: cfg.log.level.clone(),
            source: e,
        })?;

    let max_level = <Targets as Filter<LevelFilter>>::max_level_hint(&targets)
        .unwrap_or(Subscriber::DEFAULT_MAX_LEVEL);

    let file_dir = cfg
        .log
        .file
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or(default_log_dir);
    let file_writer = tracing_appender::rolling::daily(file_dir, "dominion.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_writer);

    tracing_subscriber::fmt()
        .with_max_level(max_level)
        .finish()
        .with(targets)
        .with(Layer::default().with_ansi(false).with_writer(file_writer))
        .try_init()
        .map_err(DominionLogError::from)?;

    Ok(file_guard)
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
            debug!(
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
