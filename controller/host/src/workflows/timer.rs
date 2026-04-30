//! Timer that can be awaited for workflows.

use anyhow::{Result, bail};

use std::{
    fmt::Display,
    time::{Duration, Instant},
};

use tokio::sync::mpsc;

use crate::workflows::progress_windows::ProgressWindowCommands;

/// My timer structure.
///
/// Must be generated via the constructors.
#[derive(Copy, Clone)]
pub struct Timer {
    remaining: Duration,
}

impl Timer {
    /// Create timer from a given duration.
    pub fn new(duration: Duration) -> Self {
        Self {
            remaining: duration,
        }
    }

    /// This runs the timer.
    ///
    /// If the check that is on the timer succeeds, the timer will return `Ok()`. If the timer runs
    /// out and the check afterwords fails, the timer will return an error.
    ///
    /// Arguments:
    /// * The `mpsc` sender to update the timer display.
    /// * A closure that returns a bool. If this returns true, the timer will be prematurely
    ///   terminated.
    ///
    /// Return:
    /// * Ok(()) -> The check returned `true`.
    /// * Err(...) -> The timer ran out and the final check did not return `true`.
    pub async fn run_and_update<F>(
        &mut self,
        tx_display: &mpsc::Sender<ProgressWindowCommands>,
        check: F,
    ) -> Result<()>
    where
        F: Fn() -> bool + Send + 'static,
    {
        let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

        // when this should all end
        let toc = Instant::now() + self.remaining;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {  // update timer every second
                    if check() {
                        let _ = tx_display.send(ProgressWindowCommands::UpdateTimer(String::new())).await;
                        return Ok(());
                    }

                    let tic = Instant::now();
                    if tic >= toc {
                        let _ = tx_display.send(ProgressWindowCommands::UpdateTimer(String::new())).await;
                        bail!("Timer ran out");
                    } else {
                        let _ = tx_display.send(ProgressWindowCommands::UpdateTimer(format!("{self}"))).await;
                        self.remaining = toc - tic;
                    }
                },
                _ = rx_shutdown.recv() => {  // the program is shutting down
                    bail!("Program shutdown received");
                }
            }
        }
    }
}

impl Display for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut rem = self.remaining.as_secs();

        let hrs = rem / 3_600;
        rem -= hrs * 3_600;
        let min = rem / 60;
        rem -= min * 60;
        write!(f, "{:02}:{:02}:{:02}", hrs, min, rem)
    }
}
