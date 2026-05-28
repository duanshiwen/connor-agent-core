use agentos_observability::{
    DiagnosticBundleBuilder, ObservabilityEventKind, ObservabilityRedactor, PrivacyMode,
    RedactionPolicy, TraceEvent,
};
use chrono::Utc;
use serde_json::json;

#[test]
fn diagnostic_manifest_records_privacy_mode_and_redaction_sections() {
    let manifest = DiagnosticBundleBuilder::new(PrivacyMode::SensitiveWorkspace)
        .include_section("storage_health", true)
        .skip_section("raw_prompts", "sensitive workspace mode")
        .build();

    assert_eq!(manifest.privacy_mode, PrivacyMode::SensitiveWorkspace);
    assert_eq!(manifest.sections.len(), 2);
    assert!(!manifest.privacy_mode.telemetry_allowed());
    assert!(!manifest.privacy_mode.cloud_sync_allowed());
    assert!(manifest.redaction_summary.contains("redacted"));
}

#[test]
fn diagnostic_redaction_masks_secret_like_values() {
    let event = TraceEvent::new(
        "trace-1",
        ObservabilityEventKind::Kernel,
        "diagnostics",
        "corr-1",
        Utc::now(),
    )
    .with_attribute("api_key", json!("super-secret"))
    .with_attribute(
        "nested",
        json!({ "refresh_token": "token-value", "safe": "ok" }),
    );

    let redacted = RedactionPolicy::default().redact_trace_event(&event);
    assert_eq!(redacted.attributes["api_key"], json!("[REDACTED]"));
    assert_eq!(
        redacted.attributes["nested"]["refresh_token"],
        json!("[REDACTED]")
    );
    assert_eq!(redacted.attributes["nested"]["safe"], json!("ok"));
}
