use std::sync::Arc;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use lettre::message::IntoBody;
use lettre::transport::smtp::authentication::Credentials;

use similar::TextDiff;
use tokio::sync::mpsc::Sender;
use tracing::{error, info, trace};

use watch::Watcher;

use crate::config::{Config, EmailConfig, WatchEntry};
use crate::error::DominionError;

mod config;
mod error;
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

	let tx = prepare_notifier(cfg.email).await?;
	for entry in cfg.watch {
		prepare_watcher(entry, tx.clone())?;
	}

	info!("Dominion started");
	tx.send(NotificationEvent::Startup { urls }).await?;

	shutdown_on_ctrl_c().await?;
	Ok(())
}

// TODO: move to `notify` module
async fn prepare_notifier(mail_cfg: EmailConfig) -> Result<Sender<NotificationEvent>, DominionError> {
	let (tx, mut rx) = tokio::sync::mpsc::channel::<NotificationEvent>(1);

	let creds = Credentials::new(mail_cfg.smtp_username, mail_cfg.smtp_password);
	let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(mail_cfg.smtp_host.as_str())?
		.credentials(creds)
		.build();
	mailer.test_connection().await?;

	let from_addr = mail_cfg.from_address;
	let to_addr = mail_cfg.to_address;

	tokio::spawn(async move {
		while let Some(message) = rx.recv().await {
			match message {
				NotificationEvent::Startup { urls } => {
					let mut urls_joined = "".to_string();
					for url in urls {
						urls_joined += " - ";
						urls_joined += url.as_str();
						urls_joined += ",\n";
					}

					let subject = "Starting report";
					let body = format!("Started listening on the following URLs:\n{urls_joined}");
					let mail = send_email(&mailer, from_addr.clone(), to_addr.clone(), subject, body).await;
					if let Err(err) = mail {
						error!("Failed to send started email: {err}");
					}
				}
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

					let subject = format!("Changes in {}", url);
					let body = format!("{subject}\n\n{}", diff.unified_diff());
					let mail = send_email(&mailer, from_addr.clone(), to_addr.clone(), subject, body).await;
					match mail {
						Ok(_) => trace!("Email for changes in {url} sent"),
						Err(err) => error!("Failed to send email for changes in {url}: {err}"),
					}
				}
				NotificationEvent::Failed { url, reason } => {
					error!("Failed to fetch {url}: {reason}");

					let subject = format!("Failed to fetch {url}");
					let body = format!("Failed to fetch {url}:\n\n{reason}");
					let mail = send_email(&mailer, from_addr.clone(), to_addr.clone(), subject, body).await;
					if let Err(err) = mail {
						error!("Failed to send email: {err}");
					}
				}
			}
		}
	});

	Ok(tx)
}

async fn send_email<S: Into<String>, B: IntoBody>(
	mailer: &AsyncSmtpTransport<Tokio1Executor>,
	from_addr: String,
	to_addr: String,
	subject: S,
	body: B,
) -> Result<(), DominionError> {
	let mail = Message::builder()
		.from(from_addr.parse()?)
		.to(to_addr.parse()?)
		.subject(subject)
		.body(body)?;

	mailer.send(mail).await?;

	Ok(())
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
