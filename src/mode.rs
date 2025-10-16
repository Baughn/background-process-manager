use crate::crash_handler::RunMode;
use chrono::{DateTime, Duration, Utc};
use tokio::sync::RwLock;

pub struct ModeManager {
    current_mode: RwLock<RunMode>,
    last_tool_call: RwLock<DateTime<Utc>>,
    dev_timeout_hours: i64,
}

impl ModeManager {
    pub fn new(dev_timeout_hours: u64) -> Self {
        Self {
            current_mode: RwLock::new(RunMode::Release),
            last_tool_call: RwLock::new(Utc::now()),
            dev_timeout_hours: dev_timeout_hours as i64,
        }
    }

    pub async fn record_tool_call(&self) {
        *self.last_tool_call.write().await = Utc::now();
    }

    pub async fn get_mode(&self) -> RunMode {
        *self.current_mode.read().await
    }

    pub async fn should_switch_to_release(&self) -> bool {
        let current_mode = *self.current_mode.read().await;
        if matches!(current_mode, RunMode::Release) {
            return false; // Already in release mode
        }

        let last_call = *self.last_tool_call.read().await;
        let elapsed = Utc::now() - last_call;
        elapsed > Duration::hours(self.dev_timeout_hours)
    }

    pub async fn switch_to_release(&self) {
        *self.current_mode.write().await = RunMode::Release;
    }

    pub async fn switch_to_dev(&self) {
        *self.current_mode.write().await = RunMode::Dev;
    }

    pub async fn get_time_until_release_mode(&self) -> Option<Duration> {
        let current_mode = *self.current_mode.read().await;
        if matches!(current_mode, RunMode::Release) {
            return None; // Already in release mode
        }

        let last_call = *self.last_tool_call.read().await;
        let elapsed = Utc::now() - last_call;
        let timeout = Duration::hours(self.dev_timeout_hours);

        let remaining = timeout - elapsed;
        if remaining.num_seconds() > 0 {
            Some(remaining)
        } else {
            Some(Duration::seconds(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mode_manager() {
        let manager = ModeManager::new(1); // 1 hour timeout

        // Should start in release mode (designed for system boot)
        assert!(matches!(manager.get_mode().await, RunMode::Release));

        // Switch to dev mode
        manager.switch_to_dev().await;
        assert!(matches!(manager.get_mode().await, RunMode::Dev));

        // Initially should not need to switch back to release
        assert!(!manager.should_switch_to_release().await);

        // Simulate time passing by manually setting last_tool_call to the past
        *manager.last_tool_call.write().await = Utc::now() - Duration::hours(2);

        // Now should switch back to release
        assert!(manager.should_switch_to_release().await);

        manager.switch_to_release().await;
        assert!(matches!(manager.get_mode().await, RunMode::Release));
    }

    #[tokio::test]
    async fn test_tool_call_recording() {
        let manager = ModeManager::new(1);

        // Start in release mode, switch to dev
        manager.switch_to_dev().await;

        // Record a tool call
        manager.record_tool_call().await;

        // Should have time until release mode (close to 1 hour)
        let time_remaining = manager.get_time_until_release_mode().await;
        assert!(time_remaining.is_some());
        // Check we have at least 59 minutes remaining
        assert!(time_remaining.unwrap().num_minutes() >= 59);
    }
}
