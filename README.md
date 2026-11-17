# Finit — Free Init

Finit is a modular init-system and service-management project for Wicofe OS.
The executable is named `finit`.

**Status:** early development foundation (0.1.0). This code is not yet safe to
install as PID 1. Wicofe process, signal, mount, IPC, and power-management ABI
contracts must be specified and implemented before enabling a real init runtime.

## Goals

- Keep the PID 1 critical path small and deterministic.
- Separate lifecycle, process supervision, service management, and CLI code.
- Avoid assuming Linux-specific syscalls are available on Wicofe.
- Keep platform operations behind explicit interfaces.
- Test policy and state transitions on a normal development host.

## CLI convention

All operational commands use the same structure:

```text
finit <domain> <action> [argument]
finit help [domain]
finit --version
```

Examples:

```sh
finit system status
finit system health
finit system diagnostic
finit system shutdown
finit system poweroff
finit system reboot

finit service start network
finit service restart ssh
finit service list
finit process show 1234
finit process signal 1234 TERM

finit boot status
finit target set multi-user
finit mount mount /dev/root /root
finit log service network
finit watchdog status
finit config validate

finit dependency deps network
finit dependency dependent network
finit dependency graph
finit dependency order
finit command list
```

Domains:

- `system`: status, version, info, health, diagnostic, recovery, shutdown,
  reboot, poweroff, halt, and suspend.
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

The CLI validates command shape and reports unimplemented runtime operations
explicitly; it never reports a privileged operation as successful.

## Build and test

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run -- --help
cargo run -- command list
```

## Architecture

```text
src/
├── cli/          command vocabulary and argument parsing
├── core/         lifecycle and process abstractions
├── service/      service registry and restart policy
├── boot/         boot orchestration boundary
├── shutdown/     shutdown orchestration boundary
├── diagnostic/   diagnostic result model
└── main.rs       executable entry point
```

## Wicofe platform contract

Before enabling a real PID 1 runtime, define and test:

1. process creation, exec, wait/reap, process groups, and exit status;
2. signal registration, delivery, masking, and SIGCHLD semantics;
3. mount, filesystem sync, unmount, reboot, and poweroff operations;
4. IPC transport and permissions for CLI-to-PID-1 requests;
5. environment, file descriptor, credential, and resource-limit handling.

POSIX source compatibility does not imply Linux binary ABI compatibility.

## License

MIT
