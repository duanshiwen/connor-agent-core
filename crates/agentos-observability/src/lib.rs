//! Structured observability boundary for AgentOS.
//!
//! This crate defines host-facing trace events, metrics, redaction policy, and
//! an in-memory sink suitable for tests and local diagnostics wiring.

use agentos_kernel::KernelErrorCategory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub const CURRENT_OBSERVABILITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityEventKind {
    Model,
    Action,
    Browser,
    Connector,
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

#[derive(Debug, thiserror::Error)]
pub enum ObservabilityExportError {
    #[error("observability export IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("observability export serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservabilityExportMetadata {
    pub export_mode: String,
    pub trace_path: String,
    pub metric_path: String,
    pub retention_days: u32,
    pub redaction: String,
}

#[derive(Debug, Clone)]
pub struct JsonlObservabilityFileSink {
    trace_path: PathBuf,
    metric_path: PathBuf,
    redaction_policy: RedactionPolicy,
    retention_days: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityAccessRole {
    Operator,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryRetentionPolicy {
    pub max_retention_days: u32,
    pub cleanup_job_documented: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryAccessPolicy {
    pub minimum_role: ObservabilityAccessRole,
    pub tenant_partitioning_required: bool,
    pub incident_access_audit_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugBundleAccessWorkflow {
    pub named_incident_required: bool,
    pub operator_approval_required: bool,
    pub secret_scan_required: bool,
    pub expiration_required: bool,
    pub access_audit_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PilotObservabilityOperationsDrill {
    pub export_metadata: ObservabilityExportMetadata,
    pub retention_policy: TelemetryRetentionPolicy,
    pub access_policy: TelemetryAccessPolicy,
    pub debug_bundle_workflow: DebugBundleAccessWorkflow,
}

impl PilotObservabilityOperationsDrill {
    pub fn readiness_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();

        if self.export_metadata.export_mode != "file" {
            blockers.push("production-like file export sink is not selected".to_string());
        }
        if self.export_metadata.redaction.trim().is_empty() {
            blockers.push("export redaction metadata is missing".to_string());
        }
        if self.retention_policy.max_retention_days == 0 {
            blockers.push("retention period must be greater than zero days".to_string());
        }
        if self.retention_policy.max_retention_days > self.export_metadata.retention_days {
            blockers.push("retention policy exceeds export sink retention metadata".to_string());
        }
        if !self.retention_policy.cleanup_job_documented {
            blockers.push("retention cleanup job is not documented".to_string());
        }
        if self.access_policy.minimum_role < ObservabilityAccessRole::Admin {
            blockers.push("telemetry export access must require admin role".to_string());
        }
        if !self.access_policy.tenant_partitioning_required {
            blockers.push("tenant partitioning is not required".to_string());
        }
        if !self.access_policy.incident_access_audit_required {
            blockers.push("incident access audit is not required".to_string());
        }
        if !self.debug_bundle_workflow.named_incident_required {
            blockers.push("debug bundle workflow does not require a named incident".to_string());
        }
        if !self.debug_bundle_workflow.operator_approval_required {
            blockers.push("debug bundle workflow does not require operator approval".to_string());
        }
        if !self.debug_bundle_workflow.secret_scan_required {
            blockers.push("debug bundle secret scan is not required".to_string());
        }
        if !self.debug_bundle_workflow.expiration_required {
            blockers.push("debug bundle expiration is not required".to_string());
        }
        if !self.debug_bundle_workflow.access_audit_required {
            blockers.push("debug bundle access audit is not required".to_string());
        }

        blockers
    }

    pub fn is_ready_for_commercial_pilot(&self) -> bool {
        self.readiness_blockers().is_empty()
    }
}

impl JsonlObservabilityFileSink {
    pub fn open(
        export_root: impl AsRef<Path>,
        redaction_policy: RedactionPolicy,
    ) -> Result<Self, ObservabilityExportError> {
        Self::open_with_retention(export_root, redaction_policy, 14)
    }

    pub fn open_with_retention(
        export_root: impl AsRef<Path>,
        redaction_policy: RedactionPolicy,
        retention_days: u32,
    ) -> Result<Self, ObservabilityExportError> {
        let export_root = export_root.as_ref();
        fs::create_dir_all(export_root)?;
        Ok(Self {
            trace_path: export_root.join("traces.jsonl"),
            metric_path: export_root.join("metrics.jsonl"),
            redaction_policy,
            retention_days,
        })
    }

    pub fn record_trace(&mut self, event: &TraceEvent) -> Result<(), ObservabilityExportError> {
        let redacted = self.redaction_policy.redact_trace_event(event);
        append_jsonl(&self.trace_path, &redacted)
    }

    pub fn record_metric(&mut self, sample: &MetricSample) -> Result<(), ObservabilityExportError> {
        append_jsonl(&self.metric_path, sample)
    }

    pub fn export_metadata(&self) -> ObservabilityExportMetadata {
        ObservabilityExportMetadata {
            export_mode: "file".to_string(),
            trace_path: self.trace_path.to_string_lossy().into_owned(),
            metric_path: self.metric_path.to_string_lossy().into_owned(),
            retention_days: self.retention_days,
            redaction: "trace attributes redacted before write".to_string(),
        }
    }
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<(), ObservabilityExportError> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}
