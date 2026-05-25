use thiserror::Error;

pub type KernelResult<T> = Result<T, KernelError>;

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
