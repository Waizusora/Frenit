//! Platform-neutral events emitted by a Frenit runtime adapter.
//!
//! Adapters translate native process observations into these events. This
//! module intentionally contains no syscall numbers, OS handles, or process
//! control implementation.

use super::coordinator::{LifecycleCoordinator, LifecycleCoordinatorError};
use super::manager::ServiceRegistry;
use super::restart::{RestartLimit, RestartPolicy};
use super::supervisor::{ExitDecision, SupervisorError};

/// Broad failure categories understood by the lifecycle layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFailureKind {
    ExecutableNotFound,
    PermissionDenied,
    SpawnFailed,
    TimedOut,
    WaitFailed,
    SignalFailed,
    ProtocolError,
    Other,
}

/// A runtime-reported failure with a stable category and diagnostic detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeFailure {
    pub kind: RuntimeFailureKind,
    pub message: String,
}

impl RuntimeFailure {
    pub fn new(kind: RuntimeFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn validate(&self) -> Result<(), RuntimeEventError> {
        if self.message.trim().is_empty() {
            Err(RuntimeEventError::EmptyFailureMessage)
        } else {
            Ok(())
        }
    }
}

/// Normalized process termination information reported by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExitStatus {
    Exited { code: i32 },
    Signaled { signal: i32 },
    Unknown,
}

impl ProcessExitStatus {
    /// Returns true only for a conventional zero exit code.
    pub const fn succeeded(self) -> bool {
        matches!(self, Self::Exited { code: 0 })
    }

    pub const fn validate(self) -> Result<(), RuntimeEventError> {
        match self {
            Self::Signaled { signal: 0 } => Err(RuntimeEventError::InvalidSignal(0)),
            _ => Ok(()),
        }
    }
}

/// Acknowledgement or observation produced by a platform runtime adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    StartSucceeded {
        service: String,
        pid: u32,
    },
    StartFailed {
        service: String,
        failure: RuntimeFailure,
    },
    ProcessExited {
        service: String,
        status: ProcessExitStatus,
        observed_at_seconds: u64,
    },
    StopSucceeded {
        service: String,
    },
    StopFailed {
        service: String,
        failure: RuntimeFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEventError {
    EmptyServiceName,
    InvalidPid(u32),
    EmptyFailureMessage,
    InvalidSignal(i32),
}

impl std::fmt::Display for RuntimeEventError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(formatter, "runtime event has an empty service name"),
            Self::InvalidPid(pid) => write!(formatter, "runtime event has invalid PID {pid}"),
            Self::EmptyFailureMessage => write!(formatter, "runtime failure message is empty"),
            Self::InvalidSignal(signal) => {
                write!(formatter, "runtime event has invalid signal {signal}")
            }
        }
    }
}

impl std::error::Error for RuntimeEventError {}

impl RuntimeEvent {
    pub fn service(&self) -> &str {
        match self {
            Self::StartSucceeded { service, .. }
            | Self::StartFailed { service, .. }
            | Self::ProcessExited { service, .. }
            | Self::StopSucceeded { service }
            | Self::StopFailed { service, .. } => service,
        }
    }

    /// Validates event fields before they are passed to lifecycle state logic.
    pub fn validate(&self) -> Result<(), RuntimeEventError> {
        if self.service().trim().is_empty() {
            return Err(RuntimeEventError::EmptyServiceName);
        }

        match self {
            Self::StartSucceeded { pid: 0, .. } => Err(RuntimeEventError::InvalidPid(0)),
            Self::StartFailed { failure, .. } | Self::StopFailed { failure, .. } => {
                failure.validate()
            }
            Self::ProcessExited { status, .. } => status.validate(),
            _ => Ok(()),
        }
    }
}

/// Result of applying one validated runtime event to Frenit's state model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDispatchOutcome {
    StartAcknowledged,
    StartFailed { failure: RuntimeFailure },
    ProcessExit(ExitDecision),
    StopAcknowledged,
    StopFailed { failure: RuntimeFailure },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDispatchError {
    InvalidEvent(RuntimeEventError),
    Coordinator(LifecycleCoordinatorError),
    Supervisor(SupervisorError),
}

impl std::fmt::Display for RuntimeDispatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEvent(error) => write!(formatter, "{error}"),
            Self::Coordinator(error) => write!(formatter, "{error}"),
            Self::Supervisor(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RuntimeDispatchError {}

impl From<RuntimeEventError> for RuntimeDispatchError {
    fn from(error: RuntimeEventError) -> Self {
        Self::InvalidEvent(error)
    }
}

impl From<LifecycleCoordinatorError> for RuntimeDispatchError {
    fn from(error: LifecycleCoordinatorError) -> Self {
        Self::Coordinator(error)
    }
}

impl From<SupervisorError> for RuntimeDispatchError {
    fn from(error: SupervisorError) -> Self {
        Self::Supervisor(error)
    }
}

/// Applies an adapter observation to the lifecycle coordinator or service registry.
///
/// The caller supplies the configured restart policy and limit for process-exit
/// handling. This function performs no native process operation or waiting.
pub fn dispatch_runtime_event(
    coordinator: &mut LifecycleCoordinator,
    registry: &mut ServiceRegistry,
    event: RuntimeEvent,
    restart_policy: RestartPolicy,
    restart_limit: RestartLimit,
) -> Result<RuntimeDispatchOutcome, RuntimeDispatchError> {
    event.validate()?;

    match event {
        RuntimeEvent::StartSucceeded { service, pid } => {
            coordinator.complete_start(registry, &service, Some(pid))?;
            Ok(RuntimeDispatchOutcome::StartAcknowledged)
        }
        RuntimeEvent::StartFailed { service, failure } => {
            coordinator.complete_start(registry, &service, None)?;
            Ok(RuntimeDispatchOutcome::StartFailed { failure })
        }
        RuntimeEvent::ProcessExited {
            service,
            status,
            observed_at_seconds,
        } => {
            let decision = registry.handle_exit(
                &service,
                restart_policy,
                status.succeeded(),
                observed_at_seconds,
                restart_limit,
            )?;
            Ok(RuntimeDispatchOutcome::ProcessExit(decision))
        }
        RuntimeEvent::StopSucceeded { service } => {
            coordinator.complete_stop(registry, &service, true)?;
            Ok(RuntimeDispatchOutcome::StopAcknowledged)
        }
        RuntimeEvent::StopFailed { service, failure } => {
            coordinator.complete_stop(registry, &service, false)?;
            Ok(RuntimeDispatchOutcome::StopFailed { failure })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_start_requires_a_valid_service_and_pid() {
        assert!(RuntimeEvent::StartSucceeded {
            service: "network".into(),
            pid: 42,
        }
        .validate()
        .is_ok());
        assert_eq!(
            RuntimeEvent::StartSucceeded {
                service: "network".into(),
                pid: 0,
            }
            .validate(),
            Err(RuntimeEventError::InvalidPid(0))
        );
    }

    #[test]
    fn failure_events_require_diagnostic_messages() {
        let event = RuntimeEvent::StartFailed {
            service: "network".into(),
            failure: RuntimeFailure::new(RuntimeFailureKind::TimedOut, "start deadline expired"),
        };
        assert!(event.validate().is_ok());

        let empty = RuntimeEvent::StopFailed {
            service: "network".into(),
            failure: RuntimeFailure::new(RuntimeFailureKind::WaitFailed, "  "),
        };
        assert_eq!(
            empty.validate(),
            Err(RuntimeEventError::EmptyFailureMessage)
        );
    }

    #[test]
    fn process_exit_success_is_based_only_on_zero_exit_code() {
        assert!(ProcessExitStatus::Exited { code: 0 }.succeeded());
        assert!(!ProcessExitStatus::Exited { code: 1 }.succeeded());
        assert!(!ProcessExitStatus::Signaled { signal: 9 }.succeeded());
        assert!(!ProcessExitStatus::Unknown.succeeded());
    }

    #[test]
    fn process_exit_rejects_zero_signal_number() {
        assert_eq!(
            RuntimeEvent::ProcessExited {
                service: "worker".into(),
                status: ProcessExitStatus::Signaled { signal: 0 },
                observed_at_seconds: 10,
            }
            .validate(),
            Err(RuntimeEventError::InvalidSignal(0))
        );
    }

    #[test]
    fn event_exposes_its_service_name() {
        let event = RuntimeEvent::StopSucceeded {
            service: "storage".into(),
        };
        assert_eq!(event.service(), "storage");
    }
}
