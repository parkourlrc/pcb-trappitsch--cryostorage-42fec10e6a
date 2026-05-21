//! Timer that can be awaited for workflows.

use std::time::{Duration, Instant};

use tokio::time::sleep;

/// My timer structure.
///
/// Must be generated via the constructors.
#[derive(Copy, Clone)]
pub struct Timer {
    start_time: Option<Instant>,
    duration: Duration,
}

impl Timer {
    /// Create timer from a given duration.
    pub fn new(dur: Duration) -> Self {
        Self {
            start_time: None,
            duration: dur,
        }
    }

    /// Get remaining time as a duration.
    ///
    /// If the timer is not started, this will start the timer and immediately return the remaining
    /// duration. If the timer is started, this will sleeep for one second and then return the remaining time.
    ///
    /// Return:
    /// - Option of remaining time as a formatted string or None if expired.
    pub async fn remaining_time_once_per_sec(&mut self) -> Option<Duration> {
        if let Some(start_time) = self.start_time {
            sleep(Duration::from_secs(1)).await;
            let elapsed = Instant::now() - start_time;
            if elapsed >= self.duration {
                None
            } else {
                Some(self.duration - elapsed)
            }
        } else {
            self.start_time = Some(Instant::now());
            Some(self.duration)
        }
    }
}
