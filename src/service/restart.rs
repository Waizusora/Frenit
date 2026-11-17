use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartLimit {
    pub max_restarts: u32,
    pub window_seconds: u64,
    pub delay_seconds: u64,
}

impl Default for RestartLimit {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            window_seconds: 30,
            delay_seconds: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartLimitError {
    ZeroWindow,
}

impl std::fmt::Display for RestartLimitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroWindow => write!(formatter, "restart window must be greater than zero"),
        }
    }
}

impl std::error::Error for RestartLimitError {}

impl RestartLimit {
    pub fn validate(self) -> Result<(), RestartLimitError> {
        if self.window_seconds == 0 {
            return Err(RestartLimitError::ZeroWindow);
        }
        Ok(())
    }
}

pub fn should_restart(policy: RestartPolicy, exited_successfully: bool) -> bool {
    match policy {
        RestartPolicy::Never => false,
        RestartPolicy::OnFailure => !exited_successfully,
        RestartPolicy::Always => true,
    }
}

/// Tracks restart scheduling timestamps supplied by a monotonic clock.
///
/// The caller owns the clock and process supervision. This type only enforces
/// policy and rate limits; it does not sleep or restart a process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestartTracker {
    scheduled_at: VecDeque<u64>,
}

impl RestartTracker {
    /// Returns the configured delay when a restart is allowed, and records
    /// that scheduled restart against the rolling window.
    pub fn schedule(
        &mut self,
        policy: RestartPolicy,
        exited_successfully: bool,
        now_seconds: u64,
        limit: RestartLimit,
    ) -> Result<Option<u64>, RestartLimitError> {
        limit.validate()?;

        if !should_restart(policy, exited_successfully) || limit.max_restarts == 0 {
            return Ok(None);
        }

        while self
            .scheduled_at
            .front()
            .is_some_and(|timestamp| now_seconds.saturating_sub(*timestamp) >= limit.window_seconds)
        {
            self.scheduled_at.pop_front();
        }

        if self.scheduled_at.len() >= limit.max_restarts as usize {
            return Ok(None);
        }

        self.scheduled_at.push_back(now_seconds);
        Ok(Some(limit.delay_seconds))
    }

    pub fn scheduled_count(&self) -> usize {
        self.scheduled_at.len()
    }

    pub fn reset(&mut self) {
        self.scheduled_at.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_policy_matches_exit_status() {
        assert!(!should_restart(RestartPolicy::OnFailure, true));
        assert!(should_restart(RestartPolicy::OnFailure, false));
        assert!(should_restart(RestartPolicy::Always, true));
        assert!(!should_restart(RestartPolicy::Never, false));
    }

    #[test]
    fn tracker_enforces_restart_limit_inside_rolling_window() {
        let mut tracker = RestartTracker::default();
        let limit = RestartLimit {
            max_restarts: 2,
            window_seconds: 10,
            delay_seconds: 3,
        };

        assert_eq!(
            tracker.schedule(RestartPolicy::OnFailure, false, 1, limit),
            Ok(Some(3))
        );
        assert_eq!(
            tracker.schedule(RestartPolicy::OnFailure, false, 5, limit),
            Ok(Some(3))
        );
        assert_eq!(
            tracker.schedule(RestartPolicy::OnFailure, false, 8, limit),
            Ok(None)
        );
        assert_eq!(tracker.scheduled_count(), 2);
    }

    #[test]
    fn tracker_expires_entries_at_window_boundary() {
        let mut tracker = RestartTracker::default();
        let limit = RestartLimit {
            max_restarts: 1,
            window_seconds: 10,
            delay_seconds: 0,
        };

        assert_eq!(
            tracker.schedule(RestartPolicy::Always, true, 4, limit),
            Ok(Some(0))
        );
        assert_eq!(
            tracker.schedule(RestartPolicy::Always, true, 14, limit),
            Ok(Some(0))
        );
        assert_eq!(tracker.scheduled_count(), 1);
    }

    #[test]
    fn tracker_respects_policy_and_zero_restart_budget() {
        let mut tracker = RestartTracker::default();
        let limit = RestartLimit::default();

        assert_eq!(
            tracker.schedule(RestartPolicy::Never, false, 1, limit),
            Ok(None)
        );
        assert_eq!(
            tracker.schedule(RestartPolicy::OnFailure, true, 1, limit),
            Ok(None)
        );
        assert_eq!(
            tracker.schedule(
                RestartPolicy::Always,
                false,
                1,
                RestartLimit {
                    max_restarts: 0,
                    ..limit
                }
            ),
            Ok(None)
        );
        assert_eq!(tracker.scheduled_count(), 0);
    }

    #[test]
    fn tracker_rejects_zero_length_window() {
        assert_eq!(
            RestartTracker::default().schedule(
                RestartPolicy::Always,
                false,
                0,
                RestartLimit {
                    window_seconds: 0,
                    ..RestartLimit::default()
                }
            ),
            Err(RestartLimitError::ZeroWindow)
        );
    }

    #[test]
    fn tracker_can_be_reset() {
        let mut tracker = RestartTracker::default();
        tracker
            .schedule(RestartPolicy::Always, false, 1, RestartLimit::default())
            .unwrap();
        tracker.reset();
        assert_eq!(tracker.scheduled_count(), 0);
    }
}
