#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartLimit {
    pub max_restarts: u32,
    pub window_seconds: u64,
    pub delay_seconds: u64,
}

impl Default for RestartLimit {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            window_seconds: 30,
            delay_seconds: 2,
        }
    }
}

pub fn should_restart(policy: RestartPolicy, exited_successfully: bool) -> bool {
    match policy {
        RestartPolicy::Never => false,
        RestartPolicy::OnFailure => !exited_successfully,
        RestartPolicy::Always => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_policy_matches_exit_status() {
        assert!(!should_restart(RestartPolicy::OnFailure, true));
        assert!(should_restart(RestartPolicy::OnFailure, false));
        assert!(should_restart(RestartPolicy::Always, true));
        assert!(!should_restart(RestartPolicy::Never, false));
    }
}
