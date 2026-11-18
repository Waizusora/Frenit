# Frenit — Free Init

Frenit is a modular init-system and service-management project for Wicofe OS. The executable is named `frenit`.

**Status:** early development foundation (0.1.0). This code is not yet safe to install as PID 1. Wicofe process, signal, mount, IPC, and power-management ABI contracts must be specified and implemented before enabling a real init runtime.

## CLI convention

All operational commands use this structure:

```text
frenit <domain> <action> [argument]
frenit help [domain]
frenit --version
```

Examples:

```sh
frenit system status
frenit system health
frenit system diagnostic
frenit system shutdown
frenit system poweroff
frenit system reboot
frenit service start network
frenit service restart ssh
frenit service list
frenit process show 1234
frenit process signal 1234 TERM
frenit boot status
frenit target set multi-user
frenit mount mount /dev/root /root
frenit log service network
frenit watchdog status
frenit config validate
frenit dependency deps network
frenit dependency dependent network
frenit dependency graph
frenit dependency order
frenit command list
```

## Domains

- `system`: status, version, info, health, diagnostic, recovery, shutdown, reboot, poweroff, halt, suspend.
- `service`: service lifecycle and enablement.
- `process`: process inspection and control.
- `boot`: boot status and boot targets.
- `target`: operating targets.
- `mount`: filesystem mount management.
- `log`: boot, service, and kernel logs.
- `watchdog`: watchdog status.
- `config`: configuration display, validation, testing, and reload.
- `dependency`: service dependencies, dependents, graph, and execution order.
- `command`: command/domain listing.

The CLI validates command shape and reports unimplemented runtime operations explicitly; it never reports a privileged operation as successful.

## Service lifecycle foundation

The service lifecycle layer is split into validated definitions, dependency resolution, lifecycle planning, registry state, restart supervision, ordered coordination, and runtime execution.

- `LifecyclePlan` builds deterministic startup actions and reverse-order shutdown actions.
- `ServiceRegistry` tracks service state and maintains an independent restart supervisor for each registered service.
- `LifecycleCoordinator` emits one action at a time, updates pending registry states, and requires the runtime to acknowledge each start/stop result before advancing.
- `service::adapter::RuntimeAdapter` defines the platform boundary for process creation, stop requests, and non-blocking runtime event polling. A stop request is not treated as completed shutdown; the adapter must report completion only after the process is confirmed stopped and reaped. The trait does not prescribe native handles, syscall numbers, or a specific OS ABI.
- `service::executor::RuntimeExecutor` obtains and executes one coordinator action at a time. It acknowledges a start only after the adapter returns a nonzero PID. It leaves a successful stop request in-flight until a later stop event confirms completion; adapter request failures are recorded as failed stops.
- `service::runtime` defines platform-neutral start, stop, and process-exit events, normalized exit status, and categorized runtime failures. Process-exit events carry the observed PID; the dispatcher rejects events whose PID does not match the service's currently tracked process, preventing delayed exit notifications from terminating a replacement process. The dispatcher validates event fields, acknowledges start/stop results through the coordinator, and forwards valid process exits to the registry's per-service restart supervisor.
- The executor's event polling is non-blocking and dispatches at most one event per call. A permitted process-exit restart is placed in `RestartScheduler` using the event's monotonic observation timestamp and the configured delay. `execute_due_restarts(now_seconds, registry)` starts only expired entries; it never sleeps or reads a clock. The scheduler permits one pending restart per service, exposes the next deadline, supports cancellation, and orders due work deterministically. Restart execution failures return the service to `Failed`.
- A failed startup halts further startup actions. The runtime can initiate rollback through shutdown; only services that reached the running state receive stop actions, in reverse dependency order.
- A failed stop is recorded as `Failed` and does not prevent the coordinator from attempting to stop remaining services.
- Runtime-reported start/stop failures, including timeout outcomes, are acknowledged by the coordinator; deadline measurement and process termination remain runtime responsibilities.
- Shutdown suppresses restart scheduling.

The coordinator, executor, and runtime event contract are deliberately platform-neutral. They do not implement native process operations. Runtime adapters own native process operations and translate their results into validated Frenit events. A future Wicofe runtime adapter must perform each emitted action and acknowledge its result using the documented Wicofe process and signal ABI.

## Build and test

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run -- --help
cargo run -- command list
```

## Wicofe platform contract

Before enabling a real PID 1 runtime, define and test process creation/exec/wait/reap, signal delivery and SIGCHLD semantics, mount and power operations, CLI-to-PID-1 IPC, credentials, file descriptors, and resource limits. POSIX source compatibility does not imply Linux binary ABI compatibility.

## Wicofe integration

Frenit is the canonical init-system project for Wicofe. Its cross-component integration boundary is documented in [WIC-SPEC-BOOT-FRENIT-001](https://github.com/Waizusora/Wicofe-Specs/blob/main/boot/FRENIT_INTEGRATION_CONTRACT_0.1.md). Kernel process and boot mechanisms are owned by [Wicofe-Kernel](https://github.com/Waizusora/Wicofe-Kernel), while public syscall definitions are owned by [Wicofe-ABI](https://github.com/Waizusora/Wicofe-ABI).

Native PID 1 integration remains blocked on the kernel/ABI handoff, process lifecycle, signals, VFS, IPC and shutdown contracts. Do not install Frenit as PID 1 until the native QEMU conformance gates pass.

## License

MIT
