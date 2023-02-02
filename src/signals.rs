use std::sync::Arc;

use libc::{SIGINT, SIGTERM};
use tokio::signal::unix::SignalKind;
use tokio::sync::Notify;

pub fn listen_signals() -> Arc<Notify> {
    let notify = Arc::new(Notify::new());

    for &signum in [SIGTERM, SIGINT].iter() {
        let mut sig = tokio::signal::unix::signal(SignalKind::from_raw(signum)).unwrap();

        let notify = notify.clone();

        tokio::spawn(async move {
            sig.recv().await;

            notify.notify_waiters();

            log::info!("shutdown signal received");
        });
    }

    log::info!("waiting for signal");

    notify
}
