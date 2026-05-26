use std::collections::BTreeMap;

use agentos_storage::AgentOsStorage;
use serde::{Deserialize, Serialize};

use crate::{KernelHealthReport, KernelResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelDiagnosticsBundle {
    pub runtime_config: RedactedRuntimeConfig,
    pub service_health: KernelHealthReport,
    pub storage_manifest: StorageManifestDump,
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
    let storage_manifest = storage
        .map(StorageManifestDump::from_storage)
        .unwrap_or_else(StorageManifestDump::not_configured);

    Ok(KernelDiagnosticsBundle {
        runtime_config: RedactedRuntimeConfig::from_raw(runtime_config),
        service_health,
        storage_manifest,
        recent_audit_summary: RecentAuditSummary::from_audit_log(audit_log).await?,
    })
}
