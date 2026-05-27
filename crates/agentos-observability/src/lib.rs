//! Structured observability boundary for AgentOS.
//!
//! This crate defines host-facing trace events, metrics, redaction policy, and
//! an in-memory sink suitable for tests and local diagnostics wiring.

use agentos_kernel::KernelErrorCategory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const CURRENT_OBSERVABILITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityEventKind {
    Model,
    Action,
    Browser,
    Sync,
    ToolLoop,
    Scheduler,
    Kernel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub kind: ObservabilityEventKind,
    pub scope: String,
    pub correlation_id: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_category: Option<KernelErrorCategory>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Value>,
}

impl TraceEvent {
    pub fn new(
        event_id: impl Into<String>,
        kind: ObservabilityEventKind,
        scope: impl Into<String>,
        correlation_id: impl Into<String>,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: CURRENT_OBSERVABILITY_SCHEMA_VERSION,
            event_id: event_id.into(),
            kind,
            scope: scope.into(),
            correlation_id: correlation_id.into(),
            timestamp,
            operation: None,
            error_category: None,
            attributes: BTreeMap::new(),
        }
    }

    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    pub fn with_error_category(mut self, category: KernelErrorCategory) -> Self {
        self.error_category = Some(category);
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    Counter,
    Gauge,
    LatencyHistogram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricSample {
    pub schema_version: u32,
    pub kind: MetricKind,
    pub name: String,
    pub values: Vec<f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

impl MetricSample {
    pub fn counter(name: impl Into<String>, value: f64) -> Self {
        Self::new(MetricKind::Counter, name, vec![value])
    }

    pub fn gauge(name: impl Into<String>, value: f64) -> Self {
        Self::new(MetricKind::Gauge, name, vec![value])
    }

    pub fn latency_histogram(name: impl Into<String>, values: Vec<f64>) -> Self {
        Self::new(MetricKind::LatencyHistogram, name, values)
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    fn new(kind: MetricKind, name: impl Into<String>, values: Vec<f64>) -> Self {
        Self {
            schema_version: CURRENT_OBSERVABILITY_SCHEMA_VERSION,
            kind,
            name: name.into(),
            values,
            labels: BTreeMap::new(),
        }
    }
}

pub trait ObservabilityRedactor {
    fn redact_trace_event(&self, event: &TraceEvent) -> TraceEvent;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionPolicy {
    secret_keys: Vec<String>,
    replacement: String,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            secret_keys: vec![
                "token".to_string(),
                "password".to_string(),
                "secret".to_string(),
                "api_key".to_string(),
                "credential".to_string(),
            ],
            replacement: "[REDACTED]".to_string(),
        }
    }
}

impl RedactionPolicy {
    pub fn new(secret_keys: Vec<String>, replacement: impl Into<String>) -> Self {
        Self {
            secret_keys,
            replacement: replacement.into(),
        }
    }

    fn should_redact_key(&self, key: &str) -> bool {
        let normalized = key.to_ascii_lowercase();
        self.secret_keys
            .iter()
            .any(|secret| normalized.contains(&secret.to_ascii_lowercase()))
    }

    fn redact_value(&self, key: Option<&str>, value: &Value) -> Value {
        if key.is_some_and(|key| self.should_redact_key(key)) {
            return Value::String(self.replacement.clone());
        }

        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(nested_key, nested_value)| {
                        (
                            nested_key.clone(),
                            self.redact_value(Some(nested_key), nested_value),
                        )
                    })
                    .collect::<Map<String, Value>>(),
            ),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.redact_value(None, item))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
}

impl ObservabilityRedactor for RedactionPolicy {
    fn redact_trace_event(&self, event: &TraceEvent) -> TraceEvent {
        let mut redacted = event.clone();
        redacted.attributes = event
            .attributes
            .iter()
            .map(|(key, value)| (key.clone(), self.redact_value(Some(key), value)))
            .collect();
        redacted
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryObservabilitySink {
    traces: Vec<TraceEvent>,
    metrics: Vec<MetricSample>,
}

impl InMemoryObservabilitySink {
    pub fn record_trace(&mut self, event: TraceEvent) {
        self.traces.push(event);
    }

    pub fn record_metric(&mut self, sample: MetricSample) {
        self.metrics.push(sample);
    }

    pub fn traces(&self) -> &[TraceEvent] {
        &self.traces
    }

    pub fn metrics(&self) -> &[MetricSample] {
        &self.metrics
    }
}
