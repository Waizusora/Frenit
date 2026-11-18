use super::manager::{ServiceRecord, ServiceRegistryError, ServiceState};
use super::restart::{
    should_restart, RestartLimit, RestartLimitError, RestartPolicy, RestartTracker,
};

/// Outcome of processing a service process exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitDecision {
    /// The process exited and no restart was requested by policy.
    Stopped,
    /// A restart is permitted after the returned delay.
    RestartScheduled { delay_seconds: u64 },
    /// Policy requested a restart, but the rolling restart budget is exhausted.
    RestartLimitReached,
}

/// Coordinates exit-state transitions and restart policy for one service.
///
/// Create one supervisor per service so restart budgets are never shared
/// between unrelated services. The caller supplies monotonic timestamps and
/// performs any actual waiting or process creation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceSupervisor {
    restart_tracker: RestartTracker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorError {
    ServiceNotRunning(ServiceState),
    Registry(ServiceRegistryError),
    RestartLimit(RestartLimitError),
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServiceNotRunning(state) => {
                write!(
                    formatter,
                    "cannot process process exit while service is {state:?}"
                )
            }
            Self::Registry(error) => write!(formatter, "{error}"),
            Self::RestartLimit(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SupervisorError {}

impl From<ServiceRegistryError> for SupervisorError {
    fn from(error: ServiceRegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<RestartLimitError> for SupervisorError {
    fn from(error: RestartLimitError) -> Self {
        Self::RestartLimit(error)
    }
}

impl ServiceSupervisor {
    /// Applies an observed process exit to a running service record.
    ///
    /// A successful exit becomes Inactive; an unsuccessful exit becomes
    /// Failed. If policy and budget permit a restart, the record advances to
    /// Starting and the caller receives the configured delay. No process or
    /// operating-system operation is performed here.
    pub fn handle_exit(
        &mut self,
        service: &mut ServiceRecord,
        policy: RestartPolicy,
        exited_successfully: bool,
        now_seconds: u64,
        limit: RestartLimit,
    ) -> Result<ExitDecision, SupervisorError> {
        if service.state != ServiceState::Running {
            return Err(SupervisorError::ServiceNotRunning(service.state));
        }

        let delay =
            self.restart_tracker
                .schedule(policy, exited_successfully, now_seconds, limit)?;

        let exit_state = if exited_successfully {
            ServiceState::Inactive
        } else {
            ServiceState::Failed
        };
        service.transition(exit_state, None)?;

        match delay {
            Some(delay_seconds) => {
                service.transition(ServiceState::Starting, None)?;
                Ok(ExitDecision::RestartScheduled { delay_seconds })
            }
            None if should_restart(policy, exited_successfully) => {
                Ok(ExitDecision::RestartLimitReached)
            }
            None => Ok(ExitDecision::Stopped),
        }
    }

    /// Number of restart attempts currently retained in the rolling window.
    pub fn scheduled_count(&self) -> usize {
        self.restart_tracker.scheduled_count()
    }

    /// Clears this service's restart history, for an explicit administrative reset.
    pub fn reset_restart_history(&mut self) {
        self.restart_tracker.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_service() -> ServiceRecord {
        let mut service = ServiceRecord::new("worker").unwrap();
        service.transition(ServiceState::Starting, None).unwrap();
        service.transition(ServiceState::Running, Some(42)).unwrap();
        service
    }

    #[test]
    fn failed_exit_schedules_restart_and_moves_service_to_starting() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = running_service();

        let decision = supervisor
            .handle_exit(
                &mut service,
                RestartPolicy::OnFailure,
                false,
                10,
                RestartLimit::default(),
            )
            .unwrap();

        assert_eq!(
            decision,
            ExitDecision::RestartScheduled { delay_seconds: 2 }
        );
        assert_eq!(service.state, ServiceState::Starting);
        assert_eq!(service.pid, None);
        assert_eq!(supervisor.scheduled_count(), 1);
    }

    #[test]
    fn successful_exit_with_on_failure_policy_stops_cleanly() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = running_service();

        let decision = supervisor
            .handle_exit(
                &mut service,
                RestartPolicy::OnFailure,
                true,
                10,
                RestartLimit::default(),
            )
            .unwrap();

        assert_eq!(decision, ExitDecision::Stopped);
        assert_eq!(service.state, ServiceState::Inactive);
        assert_eq!(service.pid, None);
        assert_eq!(supervisor.scheduled_count(), 0);
    }

    #[test]
    fn always_policy_restarts_after_successful_exit() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = running_service();

        let decision = supervisor
            .handle_exit(
                &mut service,
                RestartPolicy::Always,
                true,
                10,
                RestartLimit::default(),
            )
            .unwrap();

        assert_eq!(
            decision,
            ExitDecision::RestartScheduled { delay_seconds: 2 }
        );
        assert_eq!(service.state, ServiceState::Starting);
    }

    #[test]
    fn never_policy_leaves_failed_service_failed() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = running_service();

        let decision = supervisor
            .handle_exit(
                &mut service,
                RestartPolicy::Never,
                false,
                10,
                RestartLimit::default(),
            )
            .unwrap();

        assert_eq!(decision, ExitDecision::Stopped);
        assert_eq!(service.state, ServiceState::Failed);
        assert_eq!(service.pid, None);
    }

    #[test]
    fn exhausted_restart_budget_keeps_service_failed() {
        let mut supervisor = ServiceSupervisor::default();
        let limit = RestartLimit {
            max_restarts: 1,
            window_seconds: 30,
            delay_seconds: 4,
        };
        let mut first = running_service();

        assert_eq!(
            supervisor
                .handle_exit(&mut first, RestartPolicy::OnFailure, false, 1, limit,)
                .unwrap(),
            ExitDecision::RestartScheduled { delay_seconds: 4 }
        );

        let mut second = running_service();
        assert_eq!(
            supervisor
                .handle_exit(&mut second, RestartPolicy::OnFailure, false, 2, limit,)
                .unwrap(),
            ExitDecision::RestartLimitReached
        );
        assert_eq!(second.state, ServiceState::Failed);
        assert_eq!(second.pid, None);
    }

    #[test]
    fn zero_window_does_not_mutate_service_or_restart_history() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = running_service();
        let invalid_limit = RestartLimit {
            window_seconds: 0,
            ..RestartLimit::default()
        };

        assert!(matches!(
            supervisor.handle_exit(&mut service, RestartPolicy::Always, false, 1, invalid_limit,),
            Err(SupervisorError::RestartLimit(RestartLimitError::ZeroWindow))
        ));
        assert_eq!(service.state, ServiceState::Running);
        assert_eq!(service.pid, Some(42));
        assert_eq!(supervisor.scheduled_count(), 0);
    }

    #[test]
    fn non_running_service_is_rejected_without_mutation() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = ServiceRecord::new("worker").unwrap();

        assert_eq!(
            supervisor.handle_exit(
                &mut service,
                RestartPolicy::Always,
                false,
                1,
                RestartLimit::default(),
            ),
            Err(SupervisorError::ServiceNotRunning(ServiceState::Inactive))
        );
        assert_eq!(service.state, ServiceState::Inactive);
        assert_eq!(supervisor.scheduled_count(), 0);
    }

    #[test]
    fn restart_history_can_be_reset() {
        let mut supervisor = ServiceSupervisor::default();
        let mut service = running_service();
        supervisor
            .handle_exit(
                &mut service,
                RestartPolicy::Always,
                false,
                1,
                RestartLimit::default(),
            )
            .unwrap();

        supervisor.reset_restart_history();
        assert_eq!(supervisor.scheduled_count(), 0);
    }
}
