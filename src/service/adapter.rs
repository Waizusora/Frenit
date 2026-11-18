//! Platform boundary for native service process operations.
//!
//! This module defines the contract implemented by an OS-specific runtime.
//! It deliberately does not select syscalls, process handles, signal values,
//! or a concrete event-loop strategy.

use super::plan::{StartAction, StopAction};
use super::runtime::{RuntimeEvent, RuntimeFailure};

/// Operations required from a platform runtime to execute lifecycle actions.
///
/// Implementations must obey these acknowledgement rules:
/// - `start` returns a nonzero PID only after process creation succeeds.
/// - `request_stop` only requests termination; it must not claim that the
///   process has exited.
/// - A `StopSucceeded` event may be emitted only after the process is
///   confirmed stopped and reaped according to the platform contract.
/// - `ProcessExited` events must carry the PID of the process actually
///   observed exiting, so the dispatcher can reject stale notifications.
/// - `poll_event` is non-blocking and returns at most one queued event.
///
/// Timeout enforcement, process waiting/reaping, and event delivery are
/// responsibilities of the implementation. The lifecycle layer remains
/// platform-neutral.
pub trait RuntimeAdapter {
    /// Creates a process from a validated startup action.
    fn start(&mut self, action: &StartAction) -> Result<u32, RuntimeFailure>;

    /// Requests that the specified process stop.
    ///
    /// A successful return means only that the stop request was accepted;
    /// completion must be reported later through a runtime event.
    fn request_stop(
        &mut self,
        action: &StopAction,
        pid: u32,
    ) -> Result<(), RuntimeFailure>;

    /// Retrieves the next available asynchronous runtime event.
    fn poll_event(&mut self) -> Result<Option<RuntimeEvent>, RuntimeFailure>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeRuntime {
        events: VecDeque<RuntimeEvent>,
        started: Vec<String>,
        stop_requests: Vec<(String, u32)>,
    }

    impl RuntimeAdapter for FakeRuntime {
        fn start(&mut self, action: &StartAction) -> Result<u32, RuntimeFailure> {
            self.started.push(action.service.clone());
            Ok(41)
        }

        fn request_stop(
            &mut self,
            action: &StopAction,
            pid: u32,
        ) -> Result<(), RuntimeFailure> {
            self.stop_requests.push((action.service.clone(), pid));
            Ok(())
        }

        fn poll_event(&mut self) -> Result<Option<RuntimeEvent>, RuntimeFailure> {
            Ok(self.events.pop_front())
        }
    }

    #[test]
    fn adapter_contract_accepts_start_and_stop_requests() {
        let mut runtime = FakeRuntime::default();
        let start = StartAction {
            service: "worker".into(),
            executable: "/sbin/worker".into(),
            arguments: Vec::new(),
            environment: Default::default(),
            working_directory: None,
            timeout_ms: 1_000,
            restart_policy: super::super::restart::RestartPolicy::OnFailure,
        };
        let stop = StopAction {
            service: "worker".into(),
            timeout_ms: 500,
        };

        assert_eq!(runtime.start(&start).unwrap(), 41);
        runtime.request_stop(&stop, 41).unwrap();
        assert_eq!(runtime.started, vec!["worker"]);
        assert_eq!(runtime.stop_requests, vec![("worker".into(), 41)]);
    }

    #[test]
    fn poll_event_is_non_blocking_and_drains_one_event_at_a_time() {
        let mut runtime = FakeRuntime::default();
        runtime.events.push_back(RuntimeEvent::ProcessExited {
            service: "worker".into(),
            pid: 41,
            status: super::super::runtime::ProcessExitStatus::Exited { code: 0 },
            observed_at_seconds: 12,
        });

        assert!(matches!(
            runtime.poll_event().unwrap(),
            Some(RuntimeEvent::ProcessExited { pid: 41, .. })
        ));
        assert_eq!(runtime.poll_event().unwrap(), None);
    }
}
