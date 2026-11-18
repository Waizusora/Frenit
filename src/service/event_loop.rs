//! Deterministic, caller-driven runtime event-loop iteration.
//!
//! Each step performs bounded work: due restarts, at most one lifecycle action,
//! and at most one adapter event. The caller owns waiting and monotonic time.

use super::adapter::RuntimeAdapter;
use super::coordinator::{LifecycleCoordinator, LifecyclePhase};
use super::executor::{RuntimeExecutionOutcome, RuntimeExecutor, RuntimeExecutorError};
use super::manager::ServiceRegistry;
use super::restart::RestartLimit;
use super::runtime::RuntimeDispatchOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLoopOutcome {
    pub phase: LifecyclePhase,
    pub due_restarts: Vec<RuntimeExecutionOutcome>,
    pub lifecycle_action: Option<RuntimeExecutionOutcome>,
    pub event: Option<RuntimeDispatchOutcome>,
    pub next_deadline_seconds: Option<u64>,
}

pub struct RuntimeEventLoop;

impl RuntimeEventLoop {
    /// Runs one deterministic iteration using the caller's monotonic timestamp.
    ///
    /// Work is intentionally bounded: every due restart is attempted, followed
    /// by at most one lifecycle action and one queued runtime event.
    pub fn step<A: RuntimeAdapter>(
        executor: &mut RuntimeExecutor<A>,
        coordinator: &mut LifecycleCoordinator,
        registry: &mut ServiceRegistry,
        now_seconds: u64,
        restart_limit: RestartLimit,
    ) -> Result<RuntimeLoopOutcome, RuntimeExecutorError> {
        let due_restarts = executor.execute_due_restarts(now_seconds, registry)?;
        // Resolve an asynchronous result before asking the coordinator for
        // another action. In particular, stop requests remain in-flight until
        // their completion event is consumed.
        let event = executor.poll_event_from_plan(coordinator, registry, restart_limit)?;
        let lifecycle_action = executor.execute_next(coordinator, registry)?;
        Ok(RuntimeLoopOutcome {
            phase: coordinator.phase(),
            due_restarts,
            lifecycle_action,
            event,
            next_deadline_seconds: executor.restart_scheduler().next_deadline(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::adapter::RuntimeAdapter;
    use crate::service::definition::ServiceDefinition;
    use crate::service::manager::{ServiceRecord, ServiceState};
    use crate::service::plan::{LifecyclePlan, StartAction, StopAction};
    use crate::service::restart::RestartPolicy;
    use crate::service::runtime::{ProcessExitStatus, RuntimeEvent, RuntimeFailure};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeAdapter {
        events: VecDeque<RuntimeEvent>,
        next_pid: u32,
    }

    impl RuntimeAdapter for FakeAdapter {
        fn start(&mut self, _action: &StartAction) -> Result<u32, RuntimeFailure> {
            self.next_pid += 1;
            Ok(self.next_pid)
        }

        fn request_stop(&mut self, _action: &StopAction, _pid: u32) -> Result<(), RuntimeFailure> {
            Ok(())
        }

        fn poll_event(&mut self) -> Result<Option<RuntimeEvent>, RuntimeFailure> {
            Ok(self.events.pop_front())
        }
    }

    #[test]
    fn step_executes_one_start_action_and_reports_loop_state() {
        let plan =
            LifecyclePlan::build(&[ServiceDefinition::new("worker", "/sbin/worker")]).unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = ServiceRegistry::default();
        registry
            .register(ServiceRecord::new("worker").unwrap())
            .unwrap();
        let mut executor = RuntimeExecutor::new(FakeAdapter {
            events: VecDeque::new(),
            next_pid: 40,
        });

        let result = RuntimeEventLoop::step(
            &mut executor,
            &mut coordinator,
            &mut registry,
            100,
            RestartLimit::default(),
        )
        .unwrap();

        assert_eq!(result.phase, LifecyclePhase::Startup);
        assert!(matches!(
            result.lifecycle_action,
            Some(RuntimeExecutionOutcome::Started { pid: 41, .. })
        ));
        assert_eq!(result.event, None);
        assert_eq!(result.next_deadline_seconds, None);
        assert_eq!(registry.get("worker").unwrap().state, ServiceState::Running);
    }

    #[test]
    fn step_polls_no_more_than_one_event() {
        let mut definition = ServiceDefinition::new("worker", "/sbin/worker");
        definition.restart_policy = RestartPolicy::OnFailure;
        let plan = LifecyclePlan::build(&[definition]).unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = ServiceRegistry::default();
        registry
            .register(ServiceRecord::new("worker").unwrap())
            .unwrap();
        coordinator.next_action(&mut registry).unwrap();
        coordinator
            .complete_start(&mut registry, "worker", Some(1))
            .unwrap();
        let mut adapter = FakeAdapter {
            events: VecDeque::new(),
            next_pid: 0,
        };
        adapter.events.push_back(RuntimeEvent::ProcessExited {
            service: "worker".into(),
            pid: 1,
            status: ProcessExitStatus::Exited { code: 1 },
            observed_at_seconds: 100,
        });
        adapter.events.push_back(RuntimeEvent::ProcessExited {
            service: "worker".into(),
            pid: 1,
            status: ProcessExitStatus::Exited { code: 1 },
            observed_at_seconds: 101,
        });
        let mut executor = RuntimeExecutor::new(adapter);

        let result = RuntimeEventLoop::step(
            &mut executor,
            &mut coordinator,
            &mut registry,
            100,
            RestartLimit::default(),
        )
        .unwrap();
        assert!(matches!(
            result.event,
            Some(RuntimeDispatchOutcome::ProcessExit(_))
        ));
        assert_eq!(executor.adapter().events.len(), 1);
    }
    #[test]
    fn stop_completion_is_polled_before_requesting_the_next_shutdown_action() {
        let plan = LifecyclePlan::build(&[
            ServiceDefinition::new("alpha", "/sbin/alpha"),
            ServiceDefinition::new("beta", "/sbin/beta"),
        ])
        .unwrap();
        let mut coordinator = LifecycleCoordinator::new(plan);
        let mut registry = ServiceRegistry::default();
        registry
            .register(ServiceRecord::new("alpha").unwrap())
            .unwrap();
        registry
            .register(ServiceRecord::new("beta").unwrap())
            .unwrap();
        let mut executor = RuntimeExecutor::new(FakeAdapter {
            events: VecDeque::new(),
            next_pid: 100,
        });

        // Complete both startup actions and advance the coordinator to Running.
        for _ in 0..3 {
            RuntimeEventLoop::step(
                &mut executor,
                &mut coordinator,
                &mut registry,
                10,
                RestartLimit::default(),
            )
            .unwrap();
        }
        assert_eq!(coordinator.phase(), LifecyclePhase::Running);

        executor
            .begin_shutdown(&mut coordinator, &mut registry)
            .unwrap();

        // First reverse-order stop request remains in-flight awaiting an event.
        let first = RuntimeEventLoop::step(
            &mut executor,
            &mut coordinator,
            &mut registry,
            11,
            RestartLimit::default(),
        )
        .unwrap();
        assert!(matches!(
            first.lifecycle_action,
            Some(RuntimeExecutionOutcome::StopRequested { service, .. }) if service == "beta"
        ));

        executor
            .adapter_mut()
            .events
            .push_back(RuntimeEvent::StopSucceeded {
                service: "beta".into(),
                pid: 102,
            });

        // The queued completion is consumed before requesting alpha's stop.
        let second = RuntimeEventLoop::step(
            &mut executor,
            &mut coordinator,
            &mut registry,
            12,
            RestartLimit::default(),
        )
        .unwrap();
        assert!(matches!(
            second.event,
            Some(RuntimeDispatchOutcome::StopAcknowledged)
        ));
        assert!(matches!(
            second.lifecycle_action,
            Some(RuntimeExecutionOutcome::StopRequested { service, .. }) if service == "alpha"
        ));
        assert_eq!(registry.get("beta").unwrap().state, ServiceState::Inactive);
        assert_eq!(registry.get("alpha").unwrap().state, ServiceState::Stopping);
    }
}
