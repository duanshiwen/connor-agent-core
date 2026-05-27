use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type KernelResult<T> = Result<T, KernelError>;

/// Stable host-facing category for kernel and host API failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelErrorCategory {
    /// Operation can usually be retried or recovered by the runtime.
    Recoverable,
    /// Caller/user can change input, permissions, or configuration to proceed.
    UserActionable,
    /// Internal invariant or integration bug.
    Bug,
    /// External dependency or I/O failure.
    External,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KernelError {
    #[error("missing required kernel service: {service}")]
    MissingService { service: &'static str },

    #[error("invalid kernel lifecycle transition: {from} -> {to}")]
    InvalidLifecycleTransition {
        from: &'static str,
        to: &'static str,
    },

    #[error("service not found in {registry} registry: {service_id}")]
    ServiceNotFound {
        registry: &'static str,
        service_id: String,
    },

    #[error("kernel diagnostics failed: {reason}")]
    DiagnosticsFailed { reason: String },
}

impl KernelError {
    pub fn category(&self) -> KernelErrorCategory {
        match self {
            Self::MissingService { .. } => KernelErrorCategory::Bug,
            Self::InvalidLifecycleTransition { .. } => KernelErrorCategory::Recoverable,
            Self::ServiceNotFound { .. } => KernelErrorCategory::UserActionable,
            Self::DiagnosticsFailed { .. } => KernelErrorCategory::External,
        }
    }
}
