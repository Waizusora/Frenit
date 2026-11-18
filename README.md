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
