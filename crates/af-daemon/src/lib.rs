//! Background services: periodic cache sync, scrobble queue.

use std::time::Duration;
use tokio::sync::mpsc;
use af_core::events::UiCommand;

/// Runs a periodic library sync, sending `StartSync` every `interval`.
/// Skips the first immediate tick so the TUI can finish loading first.
/// Returns when the command channel closes.
pub async fn run_sync_service(
    server_name: String,
    base_url:    String,
    token:       String,
    user_id:     String,
    cmd_tx: mpsc::Sender<UiCommand>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await; // skip the immediate first tick

    loop {
        ticker.tick().await;
        tracing::info!("periodic sync: {server_name}");
        if cmd_tx
            .send(UiCommand::StartSync {
                server_name: server_name.clone(),
                base_url:    base_url.clone(),
                token:       token.clone(),
                user_id:     user_id.clone(),
            })
            .await
            .is_err()
        {
            break; // TUI exited, channel closed
        }
    }
}
