use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryBackoffConfig {
    pub initial_delay: Duration,
    pub multiplier: u32,
    pub max_delay: Duration,
    pub max_attempts: u32,
}

impl Default for RetryBackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(250),
            multiplier: 2,
            max_delay: Duration::from_secs(10),
            max_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryErrorClass {
    RateLimited,
    TransientNetwork,
    Timeout,
    PermissionDenied,
    Validation,
    Unknown,
}

impl RetryErrorClass {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::TransientNetwork | Self::Timeout
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryDecision {
    Retry {
        error_class: RetryErrorClass,
        attempts: u32,
        delay: Duration,
    },
    DoNotRetry {
        error_class: RetryErrorClass,
        attempts: u32,
        reason: String,
    },
    Exhausted {
        error_class: RetryErrorClass,
        attempts: u32,
    },
}

pub trait RetryPolicy: Send + Sync {
    fn classify_error(&self, error: &str, attempts: u32) -> RetryDecision;
    fn backoff_delay(&self, attempts: u32) -> Option<Duration>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultRetryPolicy {
    config: RetryBackoffConfig,
}

impl DefaultRetryPolicy {
    pub fn new(config: RetryBackoffConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &RetryBackoffConfig {
        &self.config
    }
}

impl Default for DefaultRetryPolicy {
    fn default() -> Self {
        Self::new(RetryBackoffConfig::default())
    }
}

impl RetryPolicy for DefaultRetryPolicy {
    fn classify_error(&self, error: &str, attempts: u32) -> RetryDecision {
        let error_class = classify_error_message(error);
        if attempts >= self.config.max_attempts {
            return RetryDecision::Exhausted {
                error_class,
                attempts,
            };
        }
        if !error_class.is_retryable() {
            return RetryDecision::DoNotRetry {
                reason: format!("{error_class:?} is not retryable"),
                error_class,
                attempts,
            };
        }
        match self.backoff_delay(attempts) {
            Some(delay) => RetryDecision::Retry {
                error_class,
                attempts,
                delay,
            },
            None => RetryDecision::Exhausted {
                error_class,
                attempts,
            },
        }
    }

    fn backoff_delay(&self, attempts: u32) -> Option<Duration> {
        if attempts == 0 || attempts >= self.config.max_attempts {
            return None;
        }
        let exponent = attempts.saturating_sub(1);
        let factor = self.config.multiplier.saturating_pow(exponent);
        let delay = self.config.initial_delay.saturating_mul(factor);
        Some(delay.min(self.config.max_delay))
    }
}

pub fn classify_error_message(error: &str) -> RetryErrorClass {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("permission denied")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
        || normalized.contains("missing capability")
    {
        return RetryErrorClass::PermissionDenied;
    }
    if normalized.contains("429")
        || normalized.contains("rate limit")
        || normalized.contains("too many requests")
    {
        return RetryErrorClass::RateLimited;
    }
    if normalized.contains("timeout")
        || normalized.contains("timed out")
        || normalized.contains("deadline")
    {
        return RetryErrorClass::Timeout;
    }
    if normalized.contains("transient")
        || normalized.contains("network")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
        || normalized.contains("temporarily unavailable")
        || normalized.contains("503")
        || normalized.contains("502")
        || normalized.contains("504")
    {
        return RetryErrorClass::TransientNetwork;
    }
    if normalized.contains("validation")
        || normalized.contains("invalid request")
        || normalized.contains("bad request")
        || normalized.contains("400")
    {
        return RetryErrorClass::Validation;
    }
    RetryErrorClass::Unknown
}
