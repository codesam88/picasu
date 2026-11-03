use crate::constant::SNAPSHOT_MAX_LIFETIME_MS;
use crate::tasks::queue::submit_expire_check;
use crate::tasks::runtime::spawn;

use std::sync::{Arc, LazyLock};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

/// Bound for the expire-timer reset signal. Resets coalesce: a full inbox
/// means a reset is already queued, so `try_send` dropping an extra one
/// loses nothing.
const RESET_CAPACITY: usize = 16;

static RESET_SENDER: LazyLock<Arc<tokio::sync::Mutex<Option<mpsc::Sender<()>>>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(None)));

pub fn start_expire_check_loop() {
    spawn(async {
        let (tx, mut rx) = mpsc::channel(RESET_CAPACITY);

        // Store the sender for external reset usage
        {
            let mut sender = RESET_SENDER.lock().await;
            *sender = Some(tx);
        }

        loop {
            // Wait for SNAPSHOT_MAX_LIFETIME_MS or receive a reset signal
            let sleep_future = sleep(Duration::from_millis(SNAPSHOT_MAX_LIFETIME_MS));

            tokio::select! {
                () = sleep_future => {
                    // Timeout reached, enqueue the check on the Expire lane;
                    // a queue worker runs it and re-arms this timer.
                    log::debug!("Timeout reached, enqueueing the expire check");
                    if let Err(err) = submit_expire_check().await {
                        log::debug!("expire check submit failed: {err:#}");
                    }
                }
                _ = rx.recv() => {
                    // Received reset signal, restart the timer

                }
            }
        }
    });
}

// Provide a function to reset the countdown timer
pub async fn reset_expire_check_timer() {
    if let Some(sender) = RESET_SENDER.lock().await.as_ref() {
        let _ = sender.try_send(());
    }
}
