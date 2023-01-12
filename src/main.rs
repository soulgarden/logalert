#![deny(warnings)]
#![forbid(unsafe_code)]

use std::sync::Arc;

use json_env_logger2::builder;
use json_env_logger2::env_logger::Target;
use log::{warn, LevelFilter};

use crate::conf::Conf;
use crate::sender::Sender;
use crate::signals::listen_signals;
use crate::watcher::Watcher;

mod conf;
mod events;
mod response;
mod sender;
mod signals;
mod slack;
mod watcher;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    json_env_logger2::panic_hook();

    let mut builder = builder();

    builder.target(Target::Stdout);
    builder.filter_level(LevelFilter::Debug);
    builder.try_init().unwrap();

    let conf = match Conf::new() {
        Ok(conf) => conf,
        Err(err) => {
            warn!("failed to load configuration, {}", err);

            std::process::exit(1);
        }
    };

    if !conf.is_debug {
        log::set_max_level(LevelFilter::Info);
    }

    let notify = listen_signals();

    let sender_shutdown_notify = notify.clone();
    let watcher_shutdown_notify = notify.clone();

    let sender = Arc::new(Sender::new(conf.clone()));

    let mut watcher = Watcher::new(conf.clone(), sender.clone());

    let result = tokio::try_join!(
        tokio::task::spawn(async move { watcher.run(watcher_shutdown_notify).await }),
        tokio::task::spawn(async move { sender.run(sender_shutdown_notify).await }),
    );

    match result {
        Ok(_) => log::info!("shutdown completed"),
        Err(e) => log::error!("thread join error {}", e),
    }

    Ok(())
}
