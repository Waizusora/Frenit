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

## Build and test

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run -- --help
cargo run -- command
```

## CLI outline

```text
finit status | version | info | help | command
finit service start|stop|restart|reload|enable|disable|list|show|mask|unmask
finit process list|show|kill|signal|terminate|tree
finit boot status|log|list|target|default
finit target list|get|set
finit mount list|status|all|mount|umount|remount
finit log [boot|service|kernel|clear]
finit health | diagnostic | recovery | watchdog status
finit config show|validate|reload|test
finit deps <service> | dependent <service> | graph | order
finit system shutdown|reboot|poweroff|halt|suspend
```

The initial CLI validates command shape and reports unimplemented runtime
operations explicitly; it never reports a privileged operation as successful.

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
