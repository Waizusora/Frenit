use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Disabled,
    Inactive,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    pub name: String,
    pub state: ServiceState,
    pub pid: Option<u32>,
}

#[derive(Debug, Default)]
pub struct ServiceRegistry {
    services: BTreeMap<String, ServiceRecord>,
}

impl ServiceRegistry {
    pub fn insert(&mut self, record: ServiceRecord) -> Option<ServiceRecord> {
        self.services.insert(record.name.clone(), record)
    }

    pub fn get(&self, name: &str) -> Option<&ServiceRecord> {
        self.services.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ServiceRecord> {
        self.services.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tracks_services_by_name() {
        let mut registry = ServiceRegistry::default();
        registry.insert(ServiceRecord {
            name: "logd".into(),
            state: ServiceState::Inactive,
            pid: None,
        });
        assert_eq!(registry.get("logd").unwrap().state, ServiceState::Inactive);
    }
}
