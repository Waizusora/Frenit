use std::collections::{BTreeMap, BTreeSet};

use super::definition::{ServiceDefinition, ServiceDefinitionError};

/// Errors found while building a deterministic service startup order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyError {
    InvalidDefinition(ServiceDefinitionError),
    DuplicateService(String),
    MissingDependency { service: String, dependency: String },
    Cycle(Vec<String>),
}

impl std::fmt::Display for DependencyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDefinition(error) => {
                write!(formatter, "invalid service definition: {error}")
            }
            Self::DuplicateService(name) => {
                write!(formatter, "service '{name}' is defined more than once")
            }
            Self::MissingDependency {
                service,
                dependency,
            } => write!(
                formatter,
                "service '{service}' depends on undefined service '{dependency}'"
            ),
            Self::Cycle(services) => write!(
                formatter,
                "service dependency cycle involves: {}",
                services.join(", ")
            ),
        }
    }
}

impl std::error::Error for DependencyError {}

/// Resolves service dependencies into a stable startup order.
///
/// Dependencies always appear before the services that require them. When
/// multiple services are ready, names are ordered lexicographically.
#[derive(Debug, Default, Clone, Copy)]
pub struct DependencyResolver;

impl DependencyResolver {
    /// Returns services in startup order: dependencies before dependents.
    pub fn resolve_startup_order(
        definitions: &[ServiceDefinition],
    ) -> Result<Vec<String>, DependencyError> {
        Self::resolve(definitions)
    }

    /// Returns the exact reverse of startup order for dependency-safe shutdown.
    pub fn resolve_shutdown_order(
        definitions: &[ServiceDefinition],
    ) -> Result<Vec<String>, DependencyError> {
        let mut order = Self::resolve(definitions)?;
        order.reverse();
        Ok(order)
    }

    pub fn resolve(definitions: &[ServiceDefinition]) -> Result<Vec<String>, DependencyError> {
        let mut by_name = BTreeMap::new();

        for definition in definitions {
            definition
                .validate()
                .map_err(DependencyError::InvalidDefinition)?;
            if by_name
                .insert(definition.name.as_str(), definition)
                .is_some()
            {
                return Err(DependencyError::DuplicateService(definition.name.clone()));
            }
        }

        let mut indegree = BTreeMap::new();
        let mut dependents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

        for definition in definitions {
            indegree.insert(definition.name.as_str(), definition.dependencies.len());

            for dependency in &definition.dependencies {
                if !by_name.contains_key(dependency.as_str()) {
                    return Err(DependencyError::MissingDependency {
                        service: definition.name.clone(),
                        dependency: dependency.clone(),
                    });
                }
                dependents
                    .entry(dependency.as_str())
                    .or_default()
                    .insert(definition.name.as_str());
            }
        }

        let mut ready: BTreeSet<&str> = indegree
            .iter()
            .filter_map(|(name, count)| (*count == 0).then_some(*name))
            .collect();
        let mut ordered = Vec::with_capacity(definitions.len());

        while let Some(name) = ready.pop_first() {
            ordered.push(name.to_owned());

            if let Some(children) = dependents.get(name) {
                for child in children {
                    let count = indegree
                        .get_mut(child)
                        .expect("dependent service must have an indegree entry");
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(child);
                    }
                }
            }
        }

        if ordered.len() != definitions.len() {
            let cycle = indegree
                .iter()
                .filter_map(|(name, count)| (*count > 0).then_some((*name).to_owned()))
                .collect();
            return Err(DependencyError::Cycle(cycle));
        }

        Ok(ordered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, dependencies: &[&str]) -> ServiceDefinition {
        let mut definition = ServiceDefinition::new(name, format!("/sbin/{name}"));
        definition.dependencies = dependencies.iter().map(|name| (*name).to_owned()).collect();
        definition
    }

    #[test]
    fn resolves_dependencies_before_dependents() {
        let definitions = vec![
            service("app", &["network", "logger"]),
            service("network", &["device"]),
            service("logger", &[]),
            service("device", &[]),
        ];

        assert_eq!(
            DependencyResolver::resolve(&definitions).unwrap(),
            vec!["device", "logger", "network", "app"]
        );
    }

    #[test]
    fn shutdown_order_is_reverse_of_startup_order() {
        let definitions = vec![
            service("app", &["network"]),
            service("network", &["device"]),
            service("device", &[]),
        ];

        assert_eq!(
            DependencyResolver::resolve_startup_order(&definitions).unwrap(),
            vec!["device", "network", "app"]
        );
        assert_eq!(
            DependencyResolver::resolve_shutdown_order(&definitions).unwrap(),
            vec!["app", "network", "device"]
        );
    }

    #[test]
    fn independent_services_have_deterministic_order() {
        let definitions = vec![
            service("zeta", &[]),
            service("alpha", &[]),
            service("middle", &[]),
        ];

        assert_eq!(
            DependencyResolver::resolve(&definitions).unwrap(),
            vec!["alpha", "middle", "zeta"]
        );
    }

    #[test]
    fn reports_missing_dependency() {
        let definitions = vec![service("app", &["network"])];

        assert_eq!(
            DependencyResolver::resolve(&definitions),
            Err(DependencyError::MissingDependency {
                service: "app".into(),
                dependency: "network".into(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_service_definitions() {
        let definitions = vec![service("logger", &[]), service("logger", &[])];

        assert_eq!(
            DependencyResolver::resolve(&definitions),
            Err(DependencyError::DuplicateService("logger".into()))
        );
    }

    #[test]
    fn detects_direct_and_indirect_cycles() {
        let direct = vec![service("alpha", &["beta"]), service("beta", &["alpha"])];
        assert_eq!(
            DependencyResolver::resolve(&direct),
            Err(DependencyError::Cycle(vec!["alpha".into(), "beta".into()]))
        );

        let indirect = vec![
            service("alpha", &["gamma"]),
            service("beta", &["alpha"]),
            service("gamma", &["beta"]),
        ];
        assert!(matches!(
            DependencyResolver::resolve(&indirect),
            Err(DependencyError::Cycle(names)) if names == vec!["alpha", "beta", "gamma"]
        ));
    }

    #[test]
    fn empty_graph_resolves_to_empty_order() {
        assert_eq!(
            DependencyResolver::resolve(&[]).unwrap(),
            Vec::<String>::new()
        );
    }
}
