use tokio::time::{sleep, Duration};
use tracing::info;

#[derive(Debug, Clone, Copy)]
pub enum RunMode {
    Dev,
    Release,
}

pub struct CrashHandler {
    dev_crash_wait_seconds: u64,
    release_crash_backoff_initial_seconds: u64,
    release_crash_backoff_max_seconds: u64,
    crash_count: usize,
}

impl CrashHandler {
    pub fn new(
        dev_crash_wait_seconds: u64,
        release_crash_backoff_initial_seconds: u64,
        release_crash_backoff_max_seconds: u64,
    ) -> Self {
        Self {
            dev_crash_wait_seconds,
            release_crash_backoff_initial_seconds,
            release_crash_backoff_max_seconds,
            crash_count: 0,
        }
    }

    pub fn reset_crash_count(&mut self) {
        self.crash_count = 0;
    }

    pub fn get_crash_count(&self) -> usize {
        self.crash_count
    }

    pub async fn wait_before_restart(&mut self, mode: RunMode) {
        self.crash_count += 1;

        let delay_seconds = match mode {
            RunMode::Dev => {
                // In dev mode, wait for the configured initial wait time on first crash,
                // then use backoff for subsequent crashes
                if self.crash_count == 1 {
                    self.dev_crash_wait_seconds
                } else {
                    self.calculate_backoff()
                }
            }
            RunMode::Release => {
                // In release mode, use sub-exponential backoff immediately
                self.calculate_backoff()
            }
        };

        info!(
            "Waiting {} seconds before restart (crash count: {}, mode: {:?})",
            delay_seconds, self.crash_count, mode
        );

        sleep(Duration::from_secs(delay_seconds)).await;
    }

    fn calculate_backoff(&self) -> u64 {
        // Sub-exponential backoff: delay = min(initial * 1.5^(attempt - 1), max)
        let initial = self.release_crash_backoff_initial_seconds as f64;
        let max = self.release_crash_backoff_max_seconds as f64;
        let attempt = self.crash_count as f64;

        let delay = initial * 1.5_f64.powf(attempt - 1.0);
        delay.min(max) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_calculation() {
        let mut handler = CrashHandler::new(120, 1, 300);

        // First crash in release mode: 1 second (1 * 1.5^0 = 1)
        handler.crash_count = 1;
        assert_eq!(handler.calculate_backoff(), 1);

        // Second crash: 1.5 seconds (1 * 1.5^1 = 1.5 -> 1 as u64)
        handler.crash_count = 2;
        assert_eq!(handler.calculate_backoff(), 1);

        // Third crash: 2.25 seconds (1 * 1.5^2 = 2.25 -> 2 as u64)
        handler.crash_count = 3;
        assert_eq!(handler.calculate_backoff(), 2);

        // Fourth crash: 3.375 -> 3 seconds
        handler.crash_count = 4;
        assert_eq!(handler.calculate_backoff(), 3);

        // Max backoff
        handler.crash_count = 20;
        assert_eq!(handler.calculate_backoff(), 300); // capped at max
    }

    #[test]
    fn test_reset() {
        let mut handler = CrashHandler::new(120, 1, 300);

        handler.crash_count = 2;
        assert_eq!(handler.get_crash_count(), 2);

        handler.reset_crash_count();
        assert_eq!(handler.get_crash_count(), 0);
    }
}
