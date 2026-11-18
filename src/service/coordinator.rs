use super::manager::{ServiceRegistry, ServiceRegistryError, ServiceState};
use super::plan::{LifecyclePlan, StartAction, StopAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    Startup,
    Running,
    Shutdown,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleAction {
    Start(StartAction),
    Stop(StopAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleCoordinatorError {
    Registry(ServiceRegistryError),
    ActionInFlight,
    NoActionInFlight,
    UnexpectedService { expected: String, actual: String },
    InvalidPhase(LifecyclePhase),
}

impl std::fmt::Display for LifecycleCoordinatorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registry(error) => write!(formatter, "{error}"),
            Self::ActionInFlight => write!(formatter, "a lifecycle action is already in flight"),
            Self::NoActionInFlight => write!(formatter, "there is no lifecycle action in flight"),
            Self::UnexpectedService { expected, actual } => write!(
                formatter,
                "expected lifecycle result for service '{expected}', received '{actual}'"
            ),
            Self::InvalidPhase(phase) => {
                write!(formatter, "operation is not valid during {phase:?}")
            }
        }
    }
}

impl std::error::Error for LifecycleCoordinatorError {}

impl From<ServiceRegistryError> for LifecycleCoordinatorError {
    fn from(error: ServiceRegistryError) -> Self {
        Self::Registry(error)
    }
}

/// Coordinates ordered lifecycle actions and registry state transitions.
///
/// It never creates, signals, waits for, or reaps processes. A platform runtime
/// performs each returned action and acknowledges its result.
#[derive(Debug, Clone)]
pub struct LifecycleCoordinator {
    plan: LifecyclePlan,
    phase: LifecyclePhase,
    startup_index: usize,
    shutdown_index: usize,
    in_flight: Option<LifecycleAction>,
}

impl LifecycleCoordinator {
    pub fn new(plan: LifecyclePlan) -> Self {
        Self {
            plan,
            phase: LifecyclePhase::Startup,
            startup_index: 0,
            shutdown_index: 0,
            in_flight: None,
        }
    }

    pub const fn phase(&self) -> LifecyclePhase {
        self.phase
    }

    /// Returns the next ordered action and updates the registry to its pending state.
    pub fn next_action(
        &mut self,
        registry: &mut ServiceRegistry,
    ) -> Result<Option<LifecycleAction>, LifecycleCoordinatorError> {
        if self.in_flight.is_some() {
            return Err(LifecycleCoordinatorError::ActionInFlight);
        }

        match self.phase {
            LifecyclePhase::Startup => {
                let Some(action) = self.plan.startup.get(self.startup_index).cloned() else {
                    self.phase = LifecyclePhase::Running;
                    return Ok(None);
                };
                registry.transition(&action.service, ServiceState::Starting, None)?;
                let action = LifecycleAction::Start(action);
                self.in_flight = Some(action.clone());
                Ok(Some(action))
            }
            LifecyclePhase::Shutdown => {
                while let Some(action) = self.plan.shutdown.get(self.shutdown_index).cloned() {
                    let Some(record) = registry.get(&action.service) else {
                        return Err(ServiceRegistryError::NotFound(action.service).into());
                    };
                    if record.state != ServiceState::Running {
                        self.shutdown_index += 1;
                        continue;
                    }
                    let pid = record
                        .pid
                        .ok_or(ServiceRegistryError::PidRequired(ServiceState::Stopping))?;
                    registry.transition(&action.service, ServiceState::Stopping, Some(pid))?;
                    let action = LifecycleAction::Stop(action);
                    self.in_flight = Some(action.clone());
                    return Ok(Some(action));
                }
                self.phase = LifecyclePhase::Complete;
                Ok(None)
            }
            LifecyclePhase::Running | LifecyclePhase::Complete | LifecyclePhase::Failed => Ok(None),
        }
    }

    /// Acknowledges a start action. A missing PID records a failed start.
    pub fn complete_start(
        &mut self,
        registry: &mut ServiceRegistry,
        service: &str,
        pid: Option<u32>,
    ) -> Result<(), LifecycleCoordinatorError> {
        let Some(LifecycleAction::Start(action)) = self.in_flight.as_ref() else {
            return Err(LifecycleCoordinatorError::NoActionInFlight);
        };
        if action.service != service {
            return Err(LifecycleCoordinatorError::UnexpectedService {
                expected: action.service.clone(),
                actual: service.to_owned(),
            });
        }

        match pid {
            Some(value) => registry.transition(service, ServiceState::Running, Some(value))?,
            None => {
                registry.transition(service, ServiceState::Failed, None)?;
                self.phase = LifecyclePhase::Failed;
            }
        }
        self.in_flight = None;
        self.startup_index += 1;
        Ok(())
    }

    /// Starts dependency-safe shutdown and suppresses service restarts.
    pub fn begin_shutdown(
        &mut self,
        registry: &mut ServiceRegistry,
    ) -> Result<(), LifecycleCoordinatorError> {
        if self.in_flight.is_some() {
            return Err(LifecycleCoordinatorError::ActionInFlight);
        }
        if !matches!(
            self.phase,
            LifecyclePhase::Startup | LifecyclePhase::Running | LifecyclePhase::Failed
        ) {
            return Err(LifecycleCoordinatorError::InvalidPhase(self.phase));
        }
        registry.begin_shutdown();
        self.phase = LifecyclePhase::Shutdown;
        Ok(())
    }

    /// Acknowledges a stop action; false records a stop failure.
    pub fn complete_stop(
        &mut self,
        registry: &mut ServiceRegistry,
        service: &str,
        stopped: bool,
    ) -> Result<(), LifecycleCoordinatorError> {
        let Some(LifecycleAction::Stop(action)) = self.in_flight.as_ref() else {
            return Err(LifecycleCoordinatorError::NoActionInFlight);
        };
        if action.service != service {
            return Err(LifecycleCoordinatorError::UnexpectedService {
                expected: action.service.clone(),
                actual: service.to_owned(),
            });
        }

        let next = if stopped {
            ServiceState::Inactive
        } else {
            ServiceState::Failed
        };
        registry.transition(service, next, None)?;
        self.in_flight = None;
        self.shutdown_index += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::definition::ServiceDefinition;
    use crate::service::manager::ServiceRecord;

    fn definition(name: &str, dependencies: &[&str]) -> ServiceDefinition {
        let mut item = ServiceDefinition::new(name, format!("/sbin/{name}"));
        item.dependencies = dependencies.iter().map(|name| (*name).to_owned()).collect();
        item
    }

    fn registry_with_services() -> ServiceRegistry {
        let mut registry = ServiceRegistry::default();
        for name in ["device", "network", "app"] {
            registry
                .register(ServiceRecord::new(name).unwrap())
                .unwrap();
        }
        registry
    }

    #[test]
    fn startup_actions_follow_dependency_order_and_update_registry() {
        let plan = LifecyclePlan::build(&[
            definition("app", &["network"]),
            definition("network", &["device"]),
            definition("device", &[]),
        ])
        .unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        for (name, pid) in [("device", 10), ("network", 11), ("app", 12)] {
            let action = coordinator.next_action(&mut registry).unwrap().unwrap();
            assert!(matches!(action, LifecycleAction::Start(ref item) if item.service == name));
            assert_eq!(registry.get(name).unwrap().state, ServiceState::Starting);
            coordinator
                .complete_start(&mut registry, name, Some(pid))
                .unwrap();
            assert_eq!(registry.get(name).unwrap().state, ServiceState::Running);
        }

        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        assert_eq!(coordinator.phase(), LifecyclePhase::Running);
    }

    #[test]
    fn shutdown_actions_follow_reverse_dependency_order() {
        let plan = LifecyclePlan::build(&[
            definition("app", &["network"]),
            definition("network", &["device"]),
            definition("device", &[]),
        ])
        .unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        for (name, pid) in [("device", 10), ("network", 11), ("app", 12)] {
            coordinator.next_action(&mut registry).unwrap();
            coordinator
                .complete_start(&mut registry, name, Some(pid))
                .unwrap();
        }
        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        coordinator.begin_shutdown(&mut registry).unwrap();

        for name in ["app", "network", "device"] {
            let action = coordinator.next_action(&mut registry).unwrap().unwrap();
            assert!(matches!(action, LifecycleAction::Stop(ref item) if item.service == name));
            assert_eq!(registry.get(name).unwrap().state, ServiceState::Stopping);
            coordinator
                .complete_stop(&mut registry, name, true)
                .unwrap();
            assert_eq!(registry.get(name).unwrap().state, ServiceState::Inactive);
        }

        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        assert_eq!(coordinator.phase(), LifecyclePhase::Complete);
        assert!(registry.is_shutting_down());
    }

    #[test]
    fn failed_start_halts_startup_and_can_be_followed_by_shutdown() {
        let plan =
            LifecyclePlan::build(&[definition("device", &[]), definition("app", &["device"])])
                .unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        coordinator.next_action(&mut registry).unwrap();
        coordinator
            .complete_start(&mut registry, "device", None)
            .unwrap();
        assert_eq!(coordinator.phase(), LifecyclePhase::Failed);
        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        coordinator.begin_shutdown(&mut registry).unwrap();
        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        assert_eq!(coordinator.phase(), LifecyclePhase::Complete);
    }

    #[test]
    fn wrong_service_acknowledgement_preserves_in_flight_action() {
        let plan = LifecyclePlan::build(&[definition("device", &[])]).unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        coordinator.next_action(&mut registry).unwrap();
        assert!(matches!(
            coordinator.complete_start(&mut registry, "app", Some(1)),
            Err(LifecycleCoordinatorError::UnexpectedService { .. })
        ));
        assert_eq!(
            registry.get("device").unwrap().state,
            ServiceState::Starting
        );
        coordinator
            .complete_start(&mut registry, "device", Some(1))
            .unwrap();
    }

    #[test]
    fn only_one_action_can_be_in_flight() {
        let plan = LifecyclePlan::build(&[definition("device", &[])]).unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        coordinator.next_action(&mut registry).unwrap();
        assert_eq!(
            coordinator.next_action(&mut registry),
            Err(LifecycleCoordinatorError::ActionInFlight)
        );
    }

    #[test]
    fn startup_failure_can_roll_back_previously_started_services() {
        let plan = LifecyclePlan::build(&[
            definition("device", &[]),
            definition("network", &["device"]),
            definition("app", &["network"]),
        ])
        .unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        coordinator.next_action(&mut registry).unwrap();
        coordinator.complete_start(&mut registry, "device", Some(10)).unwrap();
        coordinator.next_action(&mut registry).unwrap();
        coordinator.complete_start(&mut registry, "network", None).unwrap();

        assert_eq!(coordinator.phase(), LifecyclePhase::Failed);
        assert_eq!(registry.get("device").unwrap().state, ServiceState::Running);
        assert_eq!(registry.get("network").unwrap().state, ServiceState::Failed);
        assert_eq!(registry.get("app").unwrap().state, ServiceState::Inactive);

        coordinator.begin_shutdown(&mut registry).unwrap();
        let action = coordinator.next_action(&mut registry).unwrap().unwrap();
        assert!(matches!(action, LifecycleAction::Stop(ref item) if item.service == "device"));
        coordinator.complete_stop(&mut registry, "device", true).unwrap();

        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        assert_eq!(coordinator.phase(), LifecyclePhase::Complete);
        assert_eq!(registry.get("device").unwrap().state, ServiceState::Inactive);
        assert_eq!(registry.get("network").unwrap().state, ServiceState::Failed);
    }

    #[test]
    fn stop_failure_is_recorded_and_shutdown_continues() {
        let plan = LifecyclePlan::build(&[
            definition("device", &[]),
            definition("network", &["device"]),
        ])
        .unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = registry_with_services();

        for (name, pid) in [("device", 10), ("network", 11)] {
            coordinator.next_action(&mut registry).unwrap();
            coordinator.complete_start(&mut registry, name, Some(pid)).unwrap();
        }
        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        coordinator.begin_shutdown(&mut registry).unwrap();

        let action = coordinator.next_action(&mut registry).unwrap().unwrap();
        assert!(matches!(action, LifecycleAction::Stop(ref item) if item.service == "network"));
        coordinator.complete_stop(&mut registry, "network", false).unwrap();
        assert_eq!(registry.get("network").unwrap().state, ServiceState::Failed);

        let action = coordinator.next_action(&mut registry).unwrap().unwrap();
        assert!(matches!(action, LifecycleAction::Stop(ref item) if item.service == "device"));
        coordinator.complete_stop(&mut registry, "device", true).unwrap();

        assert_eq!(coordinator.next_action(&mut registry).unwrap(), None);
        assert_eq!(coordinator.phase(), LifecyclePhase::Complete);
        assert_eq!(registry.get("device").unwrap().state, ServiceState::Inactive);
        assert_eq!(registry.get("network").unwrap().state, ServiceState::Failed);
    }

}
