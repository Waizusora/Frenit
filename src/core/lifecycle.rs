/// Lifecycle policy only. Platform operations belong behind backend traits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    EarlyBoot,
    Initializing,
    Running,
    ShuttingDown,
    Halted,
    Failed,
}

impl LifecycleState {
    pub fn can_transition_to(self, next: Self) -> bool {
        use LifecycleState::*;
        matches!(
            (self, next),
            (EarlyBoot, Initializing)
                | (EarlyBoot, Failed)
                | (Initializing, Running)
                | (Initializing, ShuttingDown)
                | (Initializing, Failed)
                | (Running, ShuttingDown)
                | (Running, Failed)
                | (ShuttingDown, Halted)
                | (ShuttingDown, Failed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_boot_path_is_valid() {
        assert!(LifecycleState::EarlyBoot.can_transition_to(LifecycleState::Initializing));
        assert!(LifecycleState::Initializing.can_transition_to(LifecycleState::Running));
    }

    #[test]
    fn halted_is_terminal() {
        assert!(!LifecycleState::Halted.can_transition_to(LifecycleState::Running));
    }
}
