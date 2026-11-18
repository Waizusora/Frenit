//! Drives lifecycle actions through a platform runtime adapter.
//!
//! The executor bridges planning/state coordination and native process
//! operations. It does not implement process creation, signaling, waiting,
//! or sleeping; those remain the adapter's responsibility.

use super::adapter::RuntimeAdapter;
use super::coordinator::{LifecycleAction, LifecycleCoordinator, LifecycleCoordinatorError};
use super::manager::{ServiceRegistry, ServiceRegistryError, ServiceState};
use super::restart::{RestartLimit, RestartPolicy};
use super::restart_scheduler::{RestartScheduler, RestartSchedulerError};
use super::runtime::{
    dispatch_runtime_event, RuntimeDispatchError, RuntimeDispatchOutcome, RuntimeFailure,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeExecutionOutcome {
    Started {
        service: String,
        pid: u32,
    },
    StartFailed {
        service: String,
        failure: RuntimeFailure,
    },
    StopRequested {
        service: String,
        pid: u32,
    },
    StopRequestFailed {
        service: String,
        failure: RuntimeFailure,
    },
    RestartStarted { service: String, pid: u32 },
    RestartStartFailed { service: String, failure: RuntimeFailure },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeExecutorError {
    Coordinator(LifecycleCoordinatorError),
    Dispatch(RuntimeDispatchError),
    AdapterPoll(RuntimeFailure),
    Registry(ServiceRegistryError),
    Scheduler(RestartSchedulerError),
    MissingStartAction(String),
}

impl std::fmt::Display for RuntimeExecutorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Coordinator(error) => write!(formatter, "{error}"),
            Self::Dispatch(error) => write!(formatter, "{error}"),
            Self::AdapterPoll(error) => {
                write!(formatter, "runtime event polling failed: {error:?}")
            }
            Self::Registry(error) => write!(formatter, "{error}"),
            Self::Scheduler(error) => write!(formatter, "{error}"),
            Self::MissingStartAction(service) => write!(formatter, "service '{service}' has no startup action in the lifecycle plan"),
        }
    }
}

impl std::error::Error for RuntimeExecutorError {}

impl From<LifecycleCoordinatorError> for RuntimeExecutorError {
    fn from(error: LifecycleCoordinatorError) -> Self {
        Self::Coordinator(error)
    }
}

impl From<RuntimeDispatchError> for RuntimeExecutorError {
    fn from(error: RuntimeDispatchError) -> Self {
        Self::Dispatch(error)
    }
}

impl From<ServiceRegistryError> for RuntimeExecutorError {
    fn from(error: ServiceRegistryError) -> Self {
        Self::Registry(error)
    }
}

/// Executes one coordinator action and dispatches asynchronous runtime events.
pub struct RuntimeExecutor<A> {
    adapter: A,
    restart_scheduler: RestartScheduler,
}

impl<A: RuntimeAdapter> RuntimeExecutor<A> {
    pub const fn new(adapter: A) -> Self {
        Self { adapter, restart_scheduler: RestartScheduler::default() }
    }

    pub fn adapter(&self) -> &A {
        &self.adapter
    }

    pub fn adapter_mut(&mut self) -> &mut A {
        &mut self.adapter
    }

    pub fn restart_scheduler(&self) -> &RestartScheduler {
        &self.restart_scheduler
    }

    /// Obtains and executes at most one lifecycle action.
    ///
    /// Start is acknowledged after the adapter returns a valid PID. A stop
    /// request is deliberately left in-flight until a StopSucceeded or
    /// StopFailed event is received through poll_event.
    pub fn execute_next(
        &mut self,
        coordinator: &mut LifecycleCoordinator,
        registry: &mut ServiceRegistry,
    ) -> Result<Option<RuntimeExecutionOutcome>, RuntimeExecutorError> {
        let Some(action) = coordinator.next_action(registry)? else {
            return Ok(None);
        };

        match action {
            LifecycleAction::Start(action) => {
                let service = action.service.clone();
                match self.adapter.start(&action) {
                    Ok(pid) if pid != 0 => {
                        coordinator.complete_start(registry, &service, Some(pid))?;
                        Ok(Some(RuntimeExecutionOutcome::Started { service, pid }))
                    }
                    Ok(_) => {
                        let failure = RuntimeFailure::new(
                            super::runtime::RuntimeFailureKind::ProtocolError,
                            "runtime adapter returned PID 0 after start",
                        );
                        coordinator.complete_start(registry, &service, None)?;
                        Ok(Some(RuntimeExecutionOutcome::StartFailed {
                            service,
                            failure,
                        }))
                    }
                    Err(failure) => {
                        coordinator.complete_start(registry, &service, None)?;
                        Ok(Some(RuntimeExecutionOutcome::StartFailed {
                            service,
                            failure,
                        }))
                    }
                }
            }
            LifecycleAction::Stop(action) => {
                let service = action.service.clone();
                let pid = registry.get(&service).and_then(|record| record.pid).ok_or(
                    RuntimeExecutorError::Coordinator(LifecycleCoordinatorError::NoActionInFlight),
                )?;
                match self.adapter.request_stop(&action, pid) {
                    Ok(()) => Ok(Some(RuntimeExecutionOutcome::StopRequested {
                        service,
                        pid,
                    })),
                    Err(failure) => {
                        coordinator.complete_stop(registry, &service, false)?;
                        Ok(Some(RuntimeExecutionOutcome::StopRequestFailed {
                            service,
                            failure,
                        }))
                    }
                }
            }
        }
    }

    /// Polls one non-blocking adapter event and applies it to lifecycle state.
    pub fn poll_event(
        &mut self,
        coordinator: &mut LifecycleCoordinator,
        registry: &mut ServiceRegistry,
        restart_policy: RestartPolicy,
        restart_limit: RestartLimit,
    ) -> Result<Option<RuntimeDispatchOutcome>, RuntimeExecutorError> {
        let Some(event) = self.adapter.poll_event().map_err(RuntimeExecutorError::AdapterPoll)? else {
            return Ok(None);
        };

        let restart_observation = match &event {
            super::runtime::RuntimeEvent::ProcessExited { service, observed_at_seconds, .. } =>
                Some((service.clone(), *observed_at_seconds)),
            _ => None,
        };
        let outcome = dispatch_runtime_event(
            coordinator, registry, event, restart_policy, restart_limit,
        )?;

        if let (
            Some((service, now_seconds)),
            RuntimeDispatchOutcome::ProcessExit(
                super::supervisor::ExitDecision::RestartScheduled { delay_seconds },
            ),
        ) = (restart_observation, &outcome)
        {
            let action = coordinator.start_action_for(&service)
                .ok_or_else(|| RuntimeExecutorError::MissingStartAction(service.clone()))?;
            self.restart_scheduler.schedule(action, now_seconds, *delay_seconds)
                .map_err(RuntimeExecutorError::Scheduler)?;
        }

        Ok(Some(outcome))
    }

    /// Executes all restart deadlines that have expired at the supplied
    /// monotonic timestamp. No sleeping or clock access occurs here.
    pub fn execute_due_restarts(
        &mut self,
        now_seconds: u64,
        registry: &mut ServiceRegistry,
    ) -> Result<Vec<RuntimeExecutionOutcome>, RuntimeExecutorError> {
        let due = self.restart_scheduler.take_due(now_seconds);
        let mut outcomes = Vec::with_capacity(due.len());

        for scheduled in due {
            let service = scheduled.action.service.clone();
            match self.adapter.start(&scheduled.action) {
                Ok(pid) if pid > 0 => {
                    registry.transition(&service, ServiceState::Running, Some(pid))?;
                    outcomes.push(RuntimeExecutionOutcome::RestartStarted { service, pid });
                }
                Ok(_) => {
                    let failure = RuntimeFailure::new(
                        super::runtime::RuntimeFailureKind::ProtocolError,
                        "runtime adapter returned PID 0 after restart",
                    );
                    registry.transition(&service, ServiceState::Failed, None)?;
                    outcomes.push(RuntimeExecutionOutcome::RestartStartFailed { service, failure });
                }
                Err(failure) => {
                    registry.transition(&service, ServiceState::Failed, None)?;
                    outcomes.push(RuntimeExecutionOutcome::RestartStartFailed { service, failure });
                }
            }
        }

        Ok(outcomes)
    }
}
