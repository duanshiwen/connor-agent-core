//! AgentOS kernel composition root.
//!
//! This crate provides the first thin runtime container for composing the
//! existing AgentOS core services. It intentionally avoids implementing domain
//! behavior that belongs in lower-level crates.

mod builder;
mod error;
mod runtime;
mod services;

pub use builder::KernelRuntimeBuilder;
pub use error::{KernelError, KernelResult};
pub use runtime::{KernelHealthReport, KernelRuntime, KernelRuntimeState};
pub use services::KernelServices;
