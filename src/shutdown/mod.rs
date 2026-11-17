#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownAction {
    Reboot,
    Poweroff,
    Halt,
    Suspend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPhase {
    StopAcceptingRequests,
    StopOptionalServices,
    StopSystemServices,
    TerminateRemainingProcesses,
    SyncFilesystems,
    UnmountFilesystems,
    ExecuteAction,
}
