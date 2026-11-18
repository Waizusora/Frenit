use std::collections::BTreeMap;
use std::fmt;

use super::restart::{RestartLimit, RestartPolicy};
use super::supervisor::{ExitDecision, ServiceSupervisor, SupervisorError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Disabled,
    Inactive,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl ServiceState {
    pub const fn can_transition_to(self, next: Self) -> bool {
        use ServiceState::*;

        matches!(
            (self, next),
            (Disabled, Inactive)
                | (Inactive, Disabled)
                | (Inactive, Starting)
                | (Starting, Running)
                | (Starting, Stopping)
                | (Starting, Failed)
                | (Running, Stopping)
                | (Running, Inactive)
                | (Running, Failed)
                | (Stopping, Inactive)
                | (Stopping, Failed)
                | (Failed, Starting)
                | (Failed, Inactive)
                | (Failed, Disabled)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    pub name: String,
    pub state: ServiceState,
    pub pid: Option<u32>,
}

impl ServiceRecord {
    pub fn new(name: impl Into<String>) -> Result<Self, ServiceRegistryError> {
        let name = name.into();
        validate_name(&name)?;

        Ok(Self {
            name,
            state: ServiceState::Inactive,
            pid: None,
        })
    }

    pub fn transition(
        &mut self,
        next: ServiceState,
        pid: Option<u32>,
    ) -> Result<(), ServiceRegistryError> {
        if !self.state.can_transition_to(next) {
            return Err(ServiceRegistryError::InvalidTransition {
                from: self.state,
                to: next,
            });
        }

        match next {
            ServiceState::Running | ServiceState::Stopping => match pid {
                Some(value) if value > 0 => {}
                _ => return Err(ServiceRegistryError::PidRequired(next)),
            },
            ServiceState::Starting => {
                if pid.is_some_and(|value| value == 0) {
                    return Err(ServiceRegistryError::InvalidPid(0));
                }
            }
            ServiceState::Disabled | ServiceState::Inactive | ServiceState::Failed => {
                if pid.is_some() {
                    return Err(ServiceRegistryError::PidNotAllowed(next));
                }
            }
        }

        self.state = next;
        self.pid = pid;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRegistryError {
    InvalidName(String),
    AlreadyExists(String),
    NotFound(String),
    InvalidTransition {
        from: ServiceState,
        to: ServiceState,
    },
    PidRequired(ServiceState),
    PidNotAllowed(ServiceState),
    InvalidPid(u32),
}

impl fmt::Display for ServiceRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(formatter, "invalid service name '{name}'"),
            Self::AlreadyExists(name) => write!(formatter, "service '{name}' already exists"),
            Self::NotFound(name) => write!(formatter, "service '{name}' was not found"),
            Self::InvalidTransition { from, to } => {
                write!(formatter, "invalid service transition: {from:?} -> {to:?}")
            }
            Self::PidRequired(state) => {
                write!(formatter, "a non-zero PID is required for {state:?}")
            }
            Self::PidNotAllowed(state) => write!(formatter, "a PID is not allowed for {state:?}"),
            Self::InvalidPid(pid) => write!(formatter, "invalid process ID {pid}"),
        }
    }
}

impl std::error::Error for ServiceRegistryError {}

#[derive(Debug, Default)]
pub struct ServiceRegistry {
    services: BTreeMap<String, ServiceRecord>,
    supervisors: BTreeMap<String, ServiceSupervisor>,
    shutting_down: bool,
}

impl ServiceRegistry {
    pub fn register(&mut self, record: ServiceRecord) -> Result<(), ServiceRegistryError> {
        validate_name(&record.name)?;
        if self.services.contains_key(&record.name) {
            return Err(ServiceRegistryError::AlreadyExists(record.name));
        }
        let name = record.name.clone();
        self.services.insert(name.clone(), record);
        self.supervisors.insert(name, ServiceSupervisor::default());
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ServiceRecord> {
        self.services.get(name)
    }

    pub fn transition(
        &mut self,
        name: &str,
        next: ServiceState,
        pid: Option<u32>,
    ) -> Result<(), ServiceRegistryError> {
        let record = self
            .services
            .get_mut(name)
            .ok_or_else(|| ServiceRegistryError::NotFound(name.to_owned()))?;
        record.transition(next, pid)
    }

    /// Applies a process exit using the restart budget owned by this service.
    /// During shutdown, restart policy is overridden with `Never`.
    pub fn handle_exit(
        &mut self,
        name: &str,
        policy: RestartPolicy,
        exited_successfully: bool,
        now_seconds: u64,
        limit: RestartLimit,
    ) -> Result<ExitDecision, SupervisorError> {
        let service = self
            .services
            .get_mut(name)
            .ok_or_else(|| SupervisorError::Registry(ServiceRegistryError::NotFound(name.to_owned())))?;
        let supervisor = self
            .supervisors
            .get_mut(name)
            .expect("every registered service has a supervisor");
        let effective_policy = if self.shutting_down {
            RestartPolicy::Never
        } else {
            policy
        };
        supervisor.handle_exit(service, effective_policy, exited_successfully, now_seconds, limit)
    }

    /// Prevents any service exit from scheduling a restart.
    pub fn begin_shutdown(&mut self) {
        self.shutting_down = true;
    }

    /// Re-enables normal restart policy after an aborted shutdown.
    pub fn cancel_shutdown(&mut self) {
        self.shutting_down = false;
    }

    /// Returns whether the registry is currently suppressing restarts.
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    /// Number of restart attempts retained for a specific registered service.
    pub fn scheduled_restart_count(&self, name: &str) -> Option<usize> {
        self.supervisors.get(name).map(ServiceSupervisor::scheduled_count)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ServiceRecord> {
        self.services.values()
    }

    pub fn len(&self) -> usize {
        self.services.len()
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

fn validate_name(name: &str) -> Result<(), ServiceRegistryError> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(ServiceRegistryError::InvalidName(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tracks_services_by_name() {
        let mut registry = ServiceRegistry::default();
        registry
            .register(ServiceRecord::new("logd").unwrap())
            .unwrap();

        assert_eq!(registry.get("logd").unwrap().state, ServiceState::Inactive);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn duplicate_names_are_rejected_without_replacing_records() {
        let mut registry = ServiceRegistry::default();
        registry
            .register(ServiceRecord::new("logd").unwrap())
            .unwrap();

        assert_eq!(
            registry.register(ServiceRecord::new("logd").unwrap()),
            Err(ServiceRegistryError::AlreadyExists("logd".to_owned()))
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn service_names_are_validated() {
        assert!(ServiceRecord::new("network-manager").is_ok());
        assert!(ServiceRecord::new("network.manager_1").is_ok());
        assert!(ServiceRecord::new("").is_err());
        assert!(ServiceRecord::new("../network").is_err());
        assert!(ServiceRecord::new("name with spaces").is_err());
    }

    #[test]
    fn valid_start_and_stop_lifecycle_tracks_pid() {
        let mut record = ServiceRecord::new("logd").unwrap();

        record.transition(ServiceState::Starting, None).unwrap();
        record.transition(ServiceState::Running, Some(42)).unwrap();
        assert_eq!(record.pid, Some(42));

        record.transition(ServiceState::Stopping, Some(42)).unwrap();
        record.transition(ServiceState::Inactive, None).unwrap();
        assert_eq!(record.pid, None);
    }

    #[test]
    fn invalid_transitions_leave_record_unchanged() {
        let mut record = ServiceRecord::new("logd").unwrap();

        assert!(record.transition(ServiceState::Running, Some(42)).is_err());
        assert_eq!(record.state, ServiceState::Inactive);
        assert_eq!(record.pid, None);
    }

    #[test]
    fn running_and_stopping_require_a_nonzero_pid() {
        let mut record = ServiceRecord::new("logd").unwrap();
        record.transition(ServiceState::Starting, None).unwrap();

        assert_eq!(
            record.transition(ServiceState::Running, None),
            Err(ServiceRegistryError::PidRequired(ServiceState::Running))
        );
        assert_eq!(
            record.transition(ServiceState::Running, Some(0)),
            Err(ServiceRegistryError::PidRequired(ServiceState::Running))
        );
        record.transition(ServiceState::Running, Some(7)).unwrap();
        assert_eq!(
            record.transition(ServiceState::Stopping, None),
            Err(ServiceRegistryError::PidRequired(ServiceState::Stopping))
        );
    }

    #[test]
    fn disabled_service_must_be_enabled_before_starting() {
        let mut record = ServiceRecord::new("logd").unwrap();
        record.transition(ServiceState::Disabled, None).unwrap();

        assert!(record.transition(ServiceState::Starting, None).is_err());
        record.transition(ServiceState::Inactive, None).unwrap();
        record.transition(ServiceState::Starting, None).unwrap();
    }

    #[test]
    fn restart_budgets_are_independent_per_registered_service() {
        let mut registry = ServiceRegistry::default();
        registry.register(ServiceRecord::new("alpha").unwrap()).unwrap();
        registry.register(ServiceRecord::new("beta").unwrap()).unwrap();
        for name in ["alpha", "beta"] {
            registry.transition(name, ServiceState::Starting, None).unwrap();
            registry.transition(name, ServiceState::Running, Some(7)).unwrap();
        }

        let limit = RestartLimit { max_restarts: 1, window_seconds: 30, delay_seconds: 3 };
        assert_eq!(registry.handle_exit("alpha", RestartPolicy::OnFailure, false, 1, limit).unwrap(),
            ExitDecision::RestartScheduled { delay_seconds: 3 });
        assert_eq!(registry.handle_exit("beta", RestartPolicy::OnFailure, false, 1, limit).unwrap(),
            ExitDecision::RestartScheduled { delay_seconds: 3 });
        assert_eq!(registry.scheduled_restart_count("alpha"), Some(1));
        assert_eq!(registry.scheduled_restart_count("beta"), Some(1));
    }

    #[test]
    fn shutdown_suppresses_restart_scheduling() {
        let mut registry = ServiceRegistry::default();
        registry.register(ServiceRecord::new("worker").unwrap()).unwrap();
        registry.transition("worker", ServiceState::Starting, None).unwrap();
        registry.transition("worker", ServiceState::Running, Some(9)).unwrap();
        registry.begin_shutdown();

        assert_eq!(registry.handle_exit("worker", RestartPolicy::Always, false, 1, RestartLimit::default()).unwrap(),
            ExitDecision::Stopped);
        let service = registry.get("worker").unwrap();
        assert_eq!(service.state, ServiceState::Failed);
        assert_eq!(registry.scheduled_restart_count("worker"), Some(0));
        assert!(registry.is_shutting_down());
    }

    #[test]
    fn cancelling_shutdown_restores_restart_policy() {
        let mut registry = ServiceRegistry::default();
        registry.register(ServiceRecord::new("worker").unwrap()).unwrap();
        registry.transition("worker", ServiceState::Starting, None).unwrap();
        registry.transition("worker", ServiceState::Running, Some(9)).unwrap();
        registry.begin_shutdown();
        registry.cancel_shutdown();

        assert_eq!(registry.handle_exit("worker", RestartPolicy::Always, false, 1, RestartLimit::default()).unwrap(),
            ExitDecision::RestartScheduled { delay_seconds: 2 });
        assert!(!registry.is_shutting_down());
    }

    #[test]
    fn handling_exit_for_unknown_service_reports_not_found() {
        let mut registry = ServiceRegistry::default();
        assert_eq!(registry.handle_exit("missing", RestartPolicy::Always, false, 1, RestartLimit::default()),
            Err(SupervisorError::Registry(ServiceRegistryError::NotFound("missing".into()))));
    }

    #[test]
    fn registry_transition_reports_missing_service() {
        let mut registry = ServiceRegistry::default();

        assert_eq!(
            registry.transition("missing", ServiceState::Starting, None),
            Err(ServiceRegistryError::NotFound("missing".to_owned()))
        );
    }
}
