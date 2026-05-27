use agentos_kernel::KernelErrorCategory;
use agentos_observability::{
    InMemoryObservabilitySink, JsonlObservabilityFileSink, MetricKind, MetricSample,
    ObservabilityEventKind, ObservabilityRedactor, RedactionPolicy, TraceEvent,
};
use chrono::{TimeZone, Utc};
use serde_json::json;

#[test]
fn trace_event_schema_serializes_with_kind_scope_and_error_category() {
    let event = TraceEvent::new(
        "evt-1",
        ObservabilityEventKind::Action,
        "action-runtime",
        "conversation-1",
        Utc.with_ymd_and_hms(2026, 5, 27, 12, 0, 0).unwrap(),
    )
    .with_operation("action.execute")
    .with_error_category(KernelErrorCategory::UserActionable)
    .with_attribute("resource", "kb-public");

    let value = serde_json::to_value(&event).unwrap();

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["event_id"], "evt-1");
    assert_eq!(value["kind"], "action");
    assert_eq!(value["scope"], "action-runtime");
    assert_eq!(value["correlation_id"], "conversation-1");
    assert_eq!(value["operation"], "action.execute");
    assert_eq!(value["error_category"], "user_actionable");
    assert_eq!(value["attributes"]["resource"], "kb-public");
}

#[test]
fn metric_sample_schema_supports_counter_gauge_and_latency_histogram() {
    let counter = MetricSample::counter("action.completed", 3.0);
    let gauge = MetricSample::gauge("queue.depth", 7.0);
    let latency = MetricSample::latency_histogram("model.latency_ms", vec![10.0, 25.0, 50.0]);

    assert_eq!(counter.kind, MetricKind::Counter);
    assert_eq!(gauge.kind, MetricKind::Gauge);
    assert_eq!(latency.kind, MetricKind::LatencyHistogram);
    assert_eq!(latency.values, vec![10.0, 25.0, 50.0]);

    let value = serde_json::to_value(latency).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["kind"], "latency_histogram");
    assert_eq!(value["name"], "model.latency_ms");
}

#[test]
fn redaction_policy_masks_secret_like_values_in_trace_attributes() {
    let policy = RedactionPolicy::default();
    let event = TraceEvent::new(
        "evt-2",
        ObservabilityEventKind::Model,
        "model-adapter",
        "run-1",
        Utc.with_ymd_and_hms(2026, 5, 27, 12, 1, 0).unwrap(),
    )
    .with_attribute("prompt", "hello")
    .with_attribute("api_key", "sk-secret")
    .with_attribute("nested", json!({ "password": "p@ss", "safe": "ok" }));

    let redacted = policy.redact_trace_event(&event);

    assert_eq!(redacted.attributes["prompt"], "hello");
    assert_eq!(redacted.attributes["api_key"], "[REDACTED]");
    assert_eq!(redacted.attributes["nested"]["password"], "[REDACTED]");
    assert_eq!(redacted.attributes["nested"]["safe"], "ok");
}

#[test]
fn memory_sink_records_model_action_browser_and_sync_events() {
    let mut sink = InMemoryObservabilitySink::default();
    for (idx, kind) in [
        ObservabilityEventKind::Model,
        ObservabilityEventKind::Action,
        ObservabilityEventKind::Browser,
        ObservabilityEventKind::Sync,
    ]
    .into_iter()
    .enumerate()
    {
        sink.record_trace(TraceEvent::new(
            format!("evt-{idx}"),
            kind,
            "scope",
            "corr",
            Utc.with_ymd_and_hms(2026, 5, 27, 12, idx as u32, 0)
                .unwrap(),
        ));
    }
    sink.record_metric(MetricSample::counter("trace.recorded", 4.0));

    assert_eq!(sink.traces().len(), 4);
    assert_eq!(sink.metrics().len(), 1);
    assert_eq!(sink.traces()[0].kind, ObservabilityEventKind::Model);
    assert_eq!(sink.traces()[1].kind, ObservabilityEventKind::Action);
    assert_eq!(sink.traces()[2].kind, ObservabilityEventKind::Browser);
    assert_eq!(sink.traces()[3].kind, ObservabilityEventKind::Sync);
}

#[test]
fn jsonl_file_sink_exports_redacted_traces_and_metrics() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut sink = JsonlObservabilityFileSink::open(temp_dir.path(), RedactionPolicy::default())
        .expect("file sink should open under host-owned export root");

    let trace = TraceEvent::new(
        "evt-file-1",
        ObservabilityEventKind::Connector,
        "connector-runtime",
        "run-file-1",
        Utc.with_ymd_and_hms(2026, 5, 27, 13, 0, 0).unwrap(),
    )
    .with_operation("gmail.read")
    .with_attribute("access_token", "secret-token")
    .with_attribute("message_count", 3);
    sink.record_trace(&trace).unwrap();
    sink.record_metric(&MetricSample::counter("connector.read.completed", 1.0))
        .unwrap();

    let metadata = sink.export_metadata();
    assert_eq!(metadata.export_mode, "file");
    assert_eq!(metadata.retention_days, 14);
    assert!(metadata.trace_path.ends_with("traces.jsonl"));
    assert!(metadata.metric_path.ends_with("metrics.jsonl"));

    let trace_jsonl = std::fs::read_to_string(temp_dir.path().join("traces.jsonl")).unwrap();
    assert!(trace_jsonl.contains("evt-file-1"));
    assert!(trace_jsonl.contains("[REDACTED]"));
    assert!(!trace_jsonl.contains("secret-token"));

    let metric_jsonl = std::fs::read_to_string(temp_dir.path().join("metrics.jsonl")).unwrap();
    assert!(metric_jsonl.contains("connector.read.completed"));
}
