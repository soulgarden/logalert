#![deny(warnings)]
#![forbid(unsafe_code)]

use std::time::Duration;

use json_env_logger2::{builder, env_logger::Target};
use log::LevelFilter;
use tokio::sync::watch;

use crate::conf::Conf;
use crate::sender::Sender;
use crate::signals::{listen_signals, wait_for_shutdown};
use crate::watcher::Watcher;

mod conf;
mod entities;
mod sender;
mod signals;
mod watcher;

#[cfg(test)]
mod test_support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    json_env_logger2::panic_hook();
    builder()
        .target(Target::Stdout)
        .filter_level(LevelFilter::Debug)
        .try_init()?;
    let conf = Conf::new().map_err(|error| error.to_string())?;
    if !conf.is_debug {
        log::set_max_level(LevelFilter::Info);
    }
    let mut watcher = Watcher::new(conf.clone())?;
    let mut sender = Sender::new(conf)?;
    run_until_shutdown(
        &mut watcher,
        &mut sender,
        listen_signals()?,
        Duration::from_secs(15),
    )
    .await?;
    log::info!("shutdown completed");
    Ok(())
}

async fn run_until_shutdown(
    watcher: &mut Watcher,
    sender: &mut Sender,
    mut shutdown: watch::Receiver<bool>,
    grace: Duration,
) -> Result<(), String> {
    let processing = watcher.run(sender, shutdown.clone());
    tokio::pin!(processing);
    tokio::select! {
        _ = &mut processing => Ok(()),
        _ = wait_for_shutdown(&mut shutdown) => {
            tokio::time::timeout(grace, &mut processing).await
                .map_err(|_| "shutdown deadline exceeded; an in-flight request may be incomplete".to_string())
        }
    }
}
