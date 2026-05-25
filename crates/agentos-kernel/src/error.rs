use thiserror::Error;

pub type KernelResult<T> = Result<T, KernelError>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KernelError {
    #[error("missing required kernel service: {service}")]
    MissingService { service: &'static str },
}
