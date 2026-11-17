use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::restart::RestartPolicy;

/// Declarative service configuration. This model does not execute processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDefinition {
    pub name: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub working_directory: Option<String>,
    pub restart_policy: RestartPolicy,
    pub dependencies: Vec<String>,
    pub enabled: bool,
    pub start_timeout_ms: u64,
    pub stop_timeout_ms: u64,
}

impl ServiceDefinition {
    pub fn new(name: impl Into<String>, executable: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            executable: executable.into(),
            arguments: Vec::new(),
            environment: BTreeMap::new(),
            working_directory: None,
            restart_policy: RestartPolicy::OnFailure,
            dependencies: Vec::new(),
            enabled: true,
            start_timeout_ms: 30_000,
            stop_timeout_ms: 10_000,
        }
    }

    pub fn validate(&self) -> Result<(), ServiceDefinitionError> {
        validate_identifier(&self.name).map_err(ServiceDefinitionError::InvalidName)?;

        if self.executable.trim().is_empty() || self.executable.contains('\0') {
            return Err(ServiceDefinitionError::InvalidExecutable);
        }

        if self.start_timeout_ms == 0 {
            return Err(ServiceDefinitionError::InvalidTimeout("start"));
        }
        if self.stop_timeout_ms == 0 {
            return Err(ServiceDefinitionError::InvalidTimeout("stop"));
        }

        if self
            .working_directory
            .as_ref()
            .is_some_and(|path| path.trim().is_empty() || path.contains('\0'))
        {
            return Err(ServiceDefinitionError::InvalidWorkingDirectory);
        }

        for (key, value) in &self.environment {
            if !is_valid_environment_key(key) {
                return Err(ServiceDefinitionError::InvalidEnvironmentKey(key.clone()));
            }
            if value.contains('\0') {
                return Err(ServiceDefinitionError::InvalidEnvironmentValue(key.clone()));
            }
        }

        let mut dependencies = BTreeSet::new();
        for dependency in &self.dependencies {
            validate_identifier(dependency).map_err(ServiceDefinitionError::InvalidDependency)?;
            if dependency == &self.name {
                return Err(ServiceDefinitionError::SelfDependency(self.name.clone()));
            }
            if !dependencies.insert(dependency) {
                return Err(ServiceDefinitionError::DuplicateDependency(
                    dependency.clone(),
                ));
            }
        }

        if self
            .arguments
            .iter()
            .any(|argument| argument.contains('\0'))
        {
            return Err(ServiceDefinitionError::InvalidArgument);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceDefinitionError {
    InvalidName(String),
    InvalidExecutable,
    InvalidWorkingDirectory,
    InvalidEnvironmentKey(String),
    InvalidEnvironmentValue(String),
    InvalidArgument,
    InvalidDependency(String),
    SelfDependency(String),
    DuplicateDependency(String),
    InvalidTimeout(&'static str),
}

impl fmt::Display for ServiceDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(formatter, "invalid service name '{name}'"),
            Self::InvalidExecutable => write!(
                formatter,
                "service executable must be non-empty and contain no NUL bytes"
            ),
            Self::InvalidWorkingDirectory => write!(
                formatter,
                "working directory must be non-empty and contain no NUL bytes"
            ),
            Self::InvalidEnvironmentKey(key) => {
                write!(formatter, "invalid environment variable name '{key}'")
            }
            Self::InvalidEnvironmentValue(key) => write!(
                formatter,
                "environment variable '{key}' contains a NUL byte"
            ),
            Self::InvalidArgument => write!(formatter, "service argument contains a NUL byte"),
            Self::InvalidDependency(name) => write!(formatter, "invalid dependency name '{name}'"),
            Self::SelfDependency(name) => {
                write!(formatter, "service '{name}' cannot depend on itself")
            }
            Self::DuplicateDependency(name) => {
                write!(formatter, "dependency '{name}' is listed more than once")
            }
            Self::InvalidTimeout(kind) => {
                write!(formatter, "{kind} timeout must be greater than zero")
            }
        }
    }
}

impl std::error::Error for ServiceDefinitionError {}

fn validate_identifier(value: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(value.to_owned())
    }
}

fn is_valid_environment_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_form_a_valid_definition() {
        let definition = ServiceDefinition::new("logd", "/sbin/logd");
        assert!(definition.validate().is_ok());
        assert_eq!(definition.restart_policy, RestartPolicy::OnFailure);
        assert_eq!(definition.start_timeout_ms, 30_000);
    }

    #[test]
    fn rejects_invalid_names_and_empty_executables() {
        assert_eq!(
            ServiceDefinition::new("../logd", "/sbin/logd").validate(),
            Err(ServiceDefinitionError::InvalidName("../logd".to_owned()))
        );
        assert_eq!(
            ServiceDefinition::new("logd", "  ").validate(),
            Err(ServiceDefinitionError::InvalidExecutable)
        );
    }

    #[test]
    fn validates_environment_names_and_values() {
        let mut definition = ServiceDefinition::new("logd", "/sbin/logd");
        definition
            .environment
            .insert("GOOD_KEY".into(), "value".into());
        assert!(definition.validate().is_ok());

        definition.environment.insert("9BAD".into(), "value".into());
        assert_eq!(
            definition.validate(),
            Err(ServiceDefinitionError::InvalidEnvironmentKey("9BAD".into()))
        );
    }

    #[test]
    fn rejects_self_and_duplicate_dependencies() {
        let mut definition = ServiceDefinition::new("logd", "/sbin/logd");
        definition.dependencies.push("logd".into());
        assert_eq!(
            definition.validate(),
            Err(ServiceDefinitionError::SelfDependency("logd".into()))
        );

        definition.dependencies = vec!["network".into(), "network".into()];
        assert_eq!(
            definition.validate(),
            Err(ServiceDefinitionError::DuplicateDependency(
                "network".into()
            ))
        );
    }

    #[test]
    fn rejects_zero_timeouts_and_nul_arguments() {
        let mut definition = ServiceDefinition::new("logd", "/sbin/logd");
        definition.stop_timeout_ms = 0;
        assert_eq!(
            definition.validate(),
            Err(ServiceDefinitionError::InvalidTimeout("stop"))
        );

        definition.stop_timeout_ms = 10_000;
        definition.arguments.push("bad\0argument".into());
        assert_eq!(
            definition.validate(),
            Err(ServiceDefinitionError::InvalidArgument)
        );
    }
}
