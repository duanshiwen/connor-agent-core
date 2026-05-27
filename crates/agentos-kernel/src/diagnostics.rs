use std::collections::BTreeMap;

use agentos_config::{AgentOsConfig, RedactedAgentOsConfig};
use agentos_storage::AgentOsStorage;
use serde::{Deserialize, Serialize};

use crate::{KernelHealthReport, KernelResult, KernelRuntimeState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelDiagnosticsBundle {
    pub runtime_config: RedactedRuntimeConfig,
    pub service_health: KernelHealthReport,
    pub storage_manifest: StorageManifestDump,
    pub failure_summary: KernelFailureSummary,
    pub recent_audit_summary: RecentAuditSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedRuntimeConfig {
    pub values: BTreeMap<String, String>,
}

impl RedactedRuntimeConfig {
    pub fn from_raw(values: BTreeMap<String, String>) -> Self {
        let values = values
            .into_iter()
            .map(|(key, value)| {
                if is_sensitive_key(&key) {
                    (key, "<redacted>".to_string())
                } else {
                    (key, value)
                }
            })
            .collect();

        Self { values }
    }

    pub fn from_agentos_config(config: &AgentOsConfig) -> KernelResult<Self> {
        let validation_report = config.validate();
        let mut redacted = Self::from_redacted_agentos_config(config.redacted())?;
        redacted.values.insert(
            "agentos_config_valid".to_string(),
            validation_report.is_valid().to_string(),
        );
        redacted.values.insert(
            "agentos_config_error_count".to_string(),
            validation_report.diagnostics.len().to_string(),
        );
        Ok(redacted)
    }

    pub fn from_redacted_agentos_config(config: RedactedAgentOsConfig) -> KernelResult<Self> {
        let redacted_config = serde_json::to_string(&config).map_err(|err| {
            crate::KernelError::DiagnosticsFailed {
                reason: err.to_string(),
            }
        })?;

        let mut values = BTreeMap::new();
        values.insert("agentos_config".to_string(), redacted_config);
        values.insert("agentos_config_valid".to_string(), "unknown".to_string());
        values.insert(
            "agentos_config_error_count".to_string(),
            "unknown".to_string(),
        );

        Ok(Self { values })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageManifestDump {
    pub status: String,
    pub storage_root: Option<String>,
    pub manifest_version: Option<u32>,
}

impl StorageManifestDump {
    pub fn not_configured() -> Self {
        Self {
            status: "not_configured".to_string(),
            storage_root: None,
            manifest_version: None,
        }
    }

    pub fn from_storage(storage: &AgentOsStorage) -> Self {
        Self {
            status: "configured".to_string(),
            storage_root: Some(storage.root().display().to_string()),
            manifest_version: Some(storage.manifest().storage_version),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelFailureSummary {
    pub status: String,
    pub classifications: Vec<KernelFailureClassification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelFailureClassification {
    pub code: String,
    pub severity: String,
    pub message: String,
}

impl KernelFailureSummary {
    pub fn from_health_and_storage(
        service_health: &KernelHealthReport,
        _storage_manifest: &StorageManifestDump,
    ) -> Self {
        let mut classifications = Vec::new();

        match service_health.state {
            KernelRuntimeState::Recovering => classifications.push(KernelFailureClassification {
                code: "kernel_recovering".to_string(),
                severity: "warning".to_string(),
                message: "kernel runtime is recovering and not currently serving".to_string(),
            }),
            KernelRuntimeState::ShuttingDown | KernelRuntimeState::Shutdown => {
                classifications.push(KernelFailureClassification {
                    code: "kernel_not_running".to_string(),
                    severity: "error".to_string(),
                    message: "kernel runtime is shutting down or already shutdown".to_string(),
                });
            }
            KernelRuntimeState::New
            | KernelRuntimeState::Initialized
            | KernelRuntimeState::Started => {}
        }

        push_missing_service(
            &mut classifications,
            service_health.conversation_kernel_available,
            "conversation_kernel_unavailable",
            "conversation kernel service is unavailable",
        );
        push_missing_service(
            &mut classifications,
            service_health.model_adapter_available,
            "model_adapter_unavailable",
            "model adapter service is unavailable",
        );
        push_missing_service(
            &mut classifications,
            service_health.action_registry_available,
            "action_registry_unavailable",
            "action registry service is unavailable",
        );
        push_missing_service(
            &mut classifications,
            service_health.capability_policy_available,
            "capability_policy_unavailable",
            "capability policy service is unavailable",
        );
        push_missing_service(
            &mut classifications,
            service_health.audit_log_available,
            "audit_log_unavailable",
            "audit log service is unavailable",
        );

        let status = if classifications
            .iter()
            .any(|classification| classification.severity == "error")
        {
            "unavailable"
        } else if classifications.is_empty() {
            "ok"
        } else {
            "degraded"
        }
        .to_string();

        Self {
            status,
            classifications,
        }
    }
}

fn push_missing_service(
    classifications: &mut Vec<KernelFailureClassification>,
    available: bool,
    code: &str,
    message: &str,
) {
    if !available {
        classifications.push(KernelFailureClassification {
            code: code.to_string(),
            severity: "error".to_string(),
            message: message.to_string(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentAuditSummary {
    pub total_events: usize,
    pub recent_events: Vec<AuditEventSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEventSummary {
    pub audit_id: String,
    pub action_id: String,
    pub action_kind: String,
    pub policy_decision: String,
    pub result_status: String,
}

impl RecentAuditSummary {
    pub async fn from_audit_log(audit_log: &dyn audit_log::AuditLog) -> KernelResult<Self> {
        let events =
            audit_log
                .list()
                .await
                .map_err(|err| crate::KernelError::DiagnosticsFailed {
                    reason: err.to_string(),
                })?;
        let total_events = events.len();
        let recent_events = events
            .into_iter()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|event| AuditEventSummary {
                audit_id: event.audit_id,
                action_id: event.action_id,
                action_kind: event.action_kind,
                policy_decision: event.policy_decision,
                result_status: event.result_status,
            })
            .collect();

        Ok(Self {
            total_events,
            recent_events,
        })
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("secret")
        || normalized.contains("token")
        || normalized.contains("password")
        || normalized.contains("api_key")
        || normalized.ends_with("_key")
}

pub(crate) async fn build_diagnostics_bundle(
    runtime_config: BTreeMap<String, String>,
    service_health: KernelHealthReport,
    audit_log: &dyn audit_log::AuditLog,
    storage: Option<&AgentOsStorage>,
) -> KernelResult<KernelDiagnosticsBundle> {
    build_diagnostics_bundle_from_redacted_config(
        RedactedRuntimeConfig::from_raw(runtime_config),
        service_health,
        audit_log,
        storage,
    )
    .await
}

pub(crate) async fn build_diagnostics_bundle_from_redacted_config(
    runtime_config: RedactedRuntimeConfig,
    service_health: KernelHealthReport,
    audit_log: &dyn audit_log::AuditLog,
    storage: Option<&AgentOsStorage>,
) -> KernelResult<KernelDiagnosticsBundle> {
    let storage_manifest = storage
        .map(StorageManifestDump::from_storage)
        .unwrap_or_else(StorageManifestDump::not_configured);
    let failure_summary =
        KernelFailureSummary::from_health_and_storage(&service_health, &storage_manifest);

    Ok(KernelDiagnosticsBundle {
        runtime_config,
        service_health,
        storage_manifest,
        failure_summary,
        recent_audit_summary: RecentAuditSummary::from_audit_log(audit_log).await?,
    })
}
