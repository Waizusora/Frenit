/// Ordered boot phases. Kernel-facing work is added after Wicofe ABI contracts
/// and mount/device interfaces are stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BootPhase {
    ValidateEnvironment,
    MountVirtualFilesystems,
    PrepareRuntimeDirectories,
    InitializeLogging,
    StartCriticalServices,
    StartDefaultTarget,
}
