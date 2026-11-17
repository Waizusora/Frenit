# Finit — Free Init

Finit is a modular init-system and service-management project for Wicofe OS.

**Executable:** `finit`  
**Language:** Rust  
**Status:** early development foundation.

The first implementation milestone is a portable CLI and policy layer. It is not yet safe to run as PID 1; Wicofe process, signal, mount, IPC, and power-management ABI contracts must be implemented and tested first.
