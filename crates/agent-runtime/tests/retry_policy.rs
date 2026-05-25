use agent_runtime::{
    DefaultRetryPolicy, RetryBackoffConfig, RetryDecision, RetryErrorClass, RetryPolicy,
};
use std::time::Duration;

#[test]
fn rate_limit_429_is_retryable() {
    let policy = DefaultRetryPolicy::new(RetryBackoffConfig::default());

    let decision = policy.classify_error("model provider returned HTTP 429 rate limit", 1);

    assert!(matches!(
        decision,
        RetryDecision::Retry {
            error_class: RetryErrorClass::RateLimited,
            ..
        }
    ));
}

#[test]
fn transient_network_error_is_retryable() {
    let policy = DefaultRetryPolicy::new(RetryBackoffConfig::default());

    let decision = policy.classify_error("transient network connection reset", 1);

    assert!(matches!(
        decision,
        RetryDecision::Retry {
            error_class: RetryErrorClass::TransientNetwork,
            ..
        }
    ));
}

#[test]
fn permission_denied_is_not_retryable() {
    let policy = DefaultRetryPolicy::new(RetryBackoffConfig::default());

    let decision = policy.classify_error("permission denied: missing capability", 1);

    assert!(matches!(
        decision,
        RetryDecision::DoNotRetry {
            error_class: RetryErrorClass::PermissionDenied,
            ..
        }
    ));
}

#[test]
fn exponential_backoff_increases_until_max_delay() {
    let policy = DefaultRetryPolicy::new(RetryBackoffConfig {
        initial_delay: Duration::from_millis(100),
        multiplier: 2,
        max_delay: Duration::from_millis(250),
        max_attempts: 5,
    });

    assert_eq!(policy.backoff_delay(1), Some(Duration::from_millis(100)));
    assert_eq!(policy.backoff_delay(2), Some(Duration::from_millis(200)));
    assert_eq!(policy.backoff_delay(3), Some(Duration::from_millis(250)));
    assert_eq!(policy.backoff_delay(5), None);
}

#[test]
fn max_attempts_stops_retrying() {
    let policy = DefaultRetryPolicy::new(RetryBackoffConfig {
        max_attempts: 2,
        ..RetryBackoffConfig::default()
    });

    let decision = policy.classify_error("HTTP 429", 2);

    assert!(matches!(
        decision,
        RetryDecision::Exhausted { attempts: 2, .. }
    ));
}
