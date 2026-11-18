//! Monotonic-time restart queue.
//!
//! The scheduler stores deadlines only. The runtime loop supplies its
//! monotonic clock and decides when to execute due starts; this module never
//! sleeps or performs process operations.

use std::collections::BTreeMap;

use super::plan::StartAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRestart {
    pub action: StartAction,
    pub due_at_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartSchedulerError {
    AlreadyScheduled(String),
}

impl std::fmt::Display for RestartSchedulerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyScheduled(service) => {
                write!(formatter, "service '{service}' already has a pending restart")
            }
        }
    }
}

impl std::error::Error for RestartSchedulerError {}

/// Holds at most one pending restart per service, ordered by deadline then name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestartScheduler {
    pending: BTreeMap<String, ScheduledRestart>,
}

impl RestartScheduler {
    pub fn schedule(
        &mut self,
        action: StartAction,
        now_seconds: u64,
        delay_seconds: u64,
    ) -> Result<u64, RestartSchedulerError> {
        let service = action.service.clone();
        if self.pending.contains_key(&service) {
            return Err(RestartSchedulerError::AlreadyScheduled(service));
        }
        let due_at_seconds = now_seconds.saturating_add(delay_seconds);
        self.pending.insert(
            service,
            ScheduledRestart { action, due_at_seconds },
        );
        Ok(due_at_seconds)
    }

    /// Removes and returns every restart whose deadline is at or before now.
    pub fn take_due(&mut self, now_seconds: u64) -> Vec<ScheduledRestart> {
        let due_names: Vec<_> = self.pending.iter()
            .filter(|(_, item)| item.due_at_seconds <= now_seconds)
            .map(|(name, _)| name.clone())
            .collect();
        due_names.into_iter().filter_map(|name| self.pending.remove(&name)).collect()
    }

    pub fn cancel(&mut self, service: &str) -> bool {
        self.pending.remove(service).is_some()
    }

    pub fn next_deadline(&self) -> Option<u64> {
        self.pending.values().map(|item| item.due_at_seconds).min()
    }

    pub fn pending_count(&self) -> usize { self.pending.len() }
    pub fn is_empty(&self) -> bool { self.pending.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::restart::RestartPolicy;

    fn action(service: &str) -> StartAction {
        StartAction {
            service: service.into(), executable: format!("/sbin/{service}"),
            arguments: Vec::new(), environment: Default::default(),
            working_directory: None, timeout_ms: 1_000,
            restart_policy: RestartPolicy::OnFailure,
        }
    }

    #[test]
    fn deadlines_use_caller_supplied_monotonic_seconds() {
        let mut scheduler = RestartScheduler::default();
        assert_eq!(scheduler.schedule(action("worker"), 10, 3), Ok(13));
        assert!(scheduler.take_due(12).is_empty());
        assert_eq!(scheduler.next_deadline(), Some(13));
        assert_eq!(scheduler.take_due(13)[0].action.service, "worker");
        assert!(scheduler.is_empty());
    }

    #[test]
    fn due_restarts_are_returned_in_deadline_then_name_order() {
        let mut scheduler = RestartScheduler::default();
        scheduler.schedule(action("zeta"), 4, 2).unwrap();
        scheduler.schedule(action("beta"), 5, 1).unwrap();
        scheduler.schedule(action("alpha"), 3, 3).unwrap();
        let due = scheduler.take_due(6);
        assert_eq!(due.iter().map(|item| item.action.service.as_str()).collect::<Vec<_>>(), vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn duplicate_pending_restart_is_rejected_and_can_be_cancelled() {
        let mut scheduler = RestartScheduler::default();
        scheduler.schedule(action("worker"), 1, 5).unwrap();
        assert_eq!(scheduler.schedule(action("worker"), 2, 5), Err(RestartSchedulerError::AlreadyScheduled("worker".into())));
        assert!(scheduler.cancel("worker"));
        assert!(!scheduler.cancel("worker"));
    }

    #[test]
    fn deadline_addition_saturates_instead_of_wrapping() {
        let mut scheduler = RestartScheduler::default();
        assert_eq!(scheduler.schedule(action("worker"), u64::MAX - 1, 5), Ok(u64::MAX));
    }
}
