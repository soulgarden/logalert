use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

pub fn listen_signals() -> std::io::Result<watch::Receiver<bool>> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let (sender, receiver) = watch::channel(false);
    tokio::spawn(async move {
        tokio::select! {
            _ = terminate.recv() => {},
            _ = interrupt.recv() => {},
        }
        sender.send_replace(true);
        log::info!("shutdown signal received");
    });
    Ok(receiver)
}

pub fn is_shutdown(receiver: &watch::Receiver<bool>) -> bool {
    *receiver.borrow() || receiver.has_changed().is_err()
}

pub async fn wait_for_shutdown(receiver: &mut watch::Receiver<bool>) {
    let _ = receiver.wait_for(|stopping| *stopping).await;
}
