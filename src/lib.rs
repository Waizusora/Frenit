#![forbid(unsafe_code)]

pub mod boot;
pub mod cli;
pub mod core;
pub mod diagnostic;
pub mod service;
pub mod shutdown;

pub const PROJECT_NAME: &str = "Frenit";
pub const PROJECT_DESCRIPTION: &str = "Free Init";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
