/// Process operations required by the eventual PID 1 runtime.
/// Implement this against the Wicofe syscall ABI after it is specified.
pub trait ProcessBackend {
    type Error;

    fn wait_any(&mut self, no_hang: bool) -> Result<Option<ChildExit>, Self::Error>;
    fn send_signal(&mut self, pid: u32, signal: i32) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildExit {
    pub pid: u32,
    pub code: i32,
    pub signal: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Terminate,
    Kill,
    Interrupt,
    Child,
}

impl Signal {
    /// Numeric values are deliberately deferred to the Wicofe signal ABI.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Terminate => "TERM",
            Self::Kill => "KILL",
            Self::Interrupt => "INT",
            Self::Child => "CHLD",
        }
    }
}
