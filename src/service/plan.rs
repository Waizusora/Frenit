use std::collections::BTreeMap;

use super::definition::ServiceDefinition;
use super::dependency::{DependencyError, DependencyResolver};
use super::restart::RestartPolicy;

/// One service entry in a deterministic startup plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartAction {
    pub service: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub working_directory: Option<String>,
    pub timeout_ms: u64,
    pub restart_policy: RestartPolicy,
}

/// One service entry in a deterministic shutdown plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopAction {
    pub service: String,
    pub timeout_ms: u64,
}

/// Validated lifecycle instructions. This is a plan only; it never spawns or
/// signals processes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LifecyclePlan {
    pub startup: Vec<StartAction>,
    pub shutdown: Vec<StopAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecyclePlanError {
    Dependency(DependencyError),
    EnabledServiceDependsOnDisabled { service: String, dependency: String },
}

impl std::fmt::Display for LifecyclePlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dependency(error) => write!(formatter, "{error}"),
            Self::EnabledServiceDependsOnDisabled {
                service,
                dependency,
            } => write!(
                formatter,
                "enabled service '{service}' depends on disabled service '{dependency}'"
            ),
        }
    }
}

impl std::error::Error for LifecyclePlanError {}

impl From<DependencyError> for LifecyclePlanError {
    fn from(error: DependencyError) -> Self {
        Self::Dependency(error)
    }
}

impl LifecyclePlan {
    /// Builds a plan for enabled services only, preserving dependency order.
    /// All definitions are validated before enabled services are selected.
    pub fn build(definitions: &[ServiceDefinition]) -> Result<Self, LifecyclePlanError> {
        let full_order = DependencyResolver::resolve_startup_order(definitions)?;
        let by_name: std::collections::BTreeMap<_, _> = definitions
            .iter()
            .map(|definition| (definition.name.as_str(), definition))
            .collect();

        for definition in definitions.iter().filter(|definition| definition.enabled) {
            for dependency in &definition.dependencies {
                let required = by_name
                    .get(dependency.as_str())
                    .expect("dependency resolver validated dependency existence");
                if !required.enabled {
                    return Err(LifecyclePlanError::EnabledServiceDependsOnDisabled {
                        service: definition.name.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }

        let enabled: std::collections::BTreeSet<_> = definitions
            .iter()
            .filter(|definition| definition.enabled)
            .map(|definition| definition.name.as_str())
            .collect();

        let startup: Vec<_> = full_order
            .into_iter()
            .filter(|name| enabled.contains(name.as_str()))
            .map(|name| {
                let definition = by_name
                    .get(name.as_str())
                    .expect("resolved service must have a definition");
                StartAction {
                    service: name,
                    executable: definition.executable.clone(),
                    arguments: definition.arguments.clone(),
                    environment: definition.environment.clone(),
                    working_directory: definition.working_directory.clone(),
                    timeout_ms: definition.start_timeout_ms,
                    restart_policy: definition.restart_policy,
                }
            })
            .collect();

        let mut shutdown: Vec<_> = startup
            .iter()
            .map(|action| {
                let definition = by_name
                    .get(action.service.as_str())
                    .expect("planned service must have a definition");
                StopAction {
                    service: action.service.clone(),
                    timeout_ms: definition.stop_timeout_ms,
                }
            })
            .collect();
        shutdown.reverse();

        Ok(Self { startup, shutdown })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, dependencies: &[&str]) -> ServiceDefinition {
        let mut definition = ServiceDefinition::new(name, format!("/sbin/{name}"));
        definition.dependencies = dependencies.iter().map(|item| (*item).to_owned()).collect();
        definition
    }

    #[test]
    fn plans_startup_and_reverse_shutdown_with_timeouts() {
        let mut device = service("device", &[]);
        device.start_timeout_ms = 1_000;
        device.stop_timeout_ms = 500;
        let mut app = service("app", &["device"]);
        app.start_timeout_ms = 2_000;
        app.stop_timeout_ms = 750;

        let plan = LifecyclePlan::build(&[app, device]).unwrap();
        assert_eq!(
            plan.startup
                .iter()
                .map(|item| item.service.as_str())
                .collect::<Vec<_>>(),
            vec!["device", "app"]
        );
        assert_eq!(
            plan.shutdown
                .iter()
                .map(|item| item.service.as_str())
                .collect::<Vec<_>>(),
            vec!["app", "device"]
        );
        assert_eq!(plan.startup[0].timeout_ms, 1_000);
        assert_eq!(plan.shutdown[0].timeout_ms, 750);
    }

    #[test]
    fn startup_action_carries_process_configuration() {
        let mut definition = service("worker", &[]);
        definition.arguments = vec!["--foreground".into(), "--level=info".into()];
        definition.environment.insert("MODE".into(), "production".into());
        definition.working_directory = Some("/var/lib/worker".into());

        let plan = LifecyclePlan::build(&[definition]).unwrap();
        let action = &plan.startup[0];

        assert_eq!(action.executable, "/sbin/worker");
        assert_eq!(action.arguments, vec!["--foreground", "--level=info"]);
        assert_eq!(action.environment.get("MODE").map(String::as_str), Some("production"));
        assert_eq!(action.working_directory.as_deref(), Some("/var/lib/worker"));
    }

    #[test]
    fn disabled_services_are_excluded_from_both_plans() {
        let active = service("active", &[]);
        let mut disabled = service("optional", &[]);
        disabled.enabled = false;

        let plan = LifecyclePlan::build(&[disabled, active]).unwrap();
        assert_eq!(plan.startup.len(), 1);
        assert_eq!(plan.startup[0].service, "active");
        assert_eq!(plan.shutdown.len(), 1);
        assert_eq!(plan.shutdown[0].service, "active");
    }

    #[test]
    fn enabled_service_cannot_depend_on_disabled_service() {
        let mut dependency = service("device", &[]);
        dependency.enabled = false;
        let app = service("app", &["device"]);

        assert_eq!(
            LifecyclePlan::build(&[app, dependency]),
            Err(LifecyclePlanError::EnabledServiceDependsOnDisabled {
                service: "app".into(),
                dependency: "device".into(),
            })
        );
    }

    #[test]
    fn all_disabled_definitions_produce_empty_plan() {
        let mut service = service("optional", &[]);
        service.enabled = false;
        assert_eq!(
            LifecyclePlan::build(&[service]).unwrap(),
            LifecyclePlan::default()
        );
    }
}
