//! JSON-safe native bridge contract for AgentOS clients.
//!
//! This crate intentionally exposes a narrow, serialization-first boundary that
//! can later be wrapped by UniFFI, C ABI, Swift Package, or another host bridge.
//! The bridge does not require native clients to depend on Rust generics or
//! trait objects.

use client_substrate::{
    ClientEventCursor, ClientEventEnvelope, ClientSubstrate, ClientSubstrateError,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sync_runtime::{
    KnowledgeEntrySyncStatus, KnowledgeSyncProjection, ServerSyncApplyError, ServerSyncCursor,
    ServerSyncEvent, ServerSyncObjectType, ServerSyncOperation,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BackendApiResponse<T> {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    data: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BackendSyncPullData {
    events: Vec<ServerSyncEvent>,
    #[serde(default)]
    next_after_sequence: u64,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    server_time: i64,
    #[serde(default)]
    schema_version: u32,
}

#[derive(Debug, Error)]
pub enum AgentOsClientBridgeError {
    #[error("invalid bridge argument: {reason}")]
    InvalidArgument { reason: String },
    #[error("substrate error: {reason}")]
    Substrate { reason: String },
    #[error("bridge serialization failed: {reason}")]
    Serialization { reason: String },
    #[error("server sync apply failed: {reason}")]
    ServerSyncApply { reason: String },
}

impl From<ClientSubstrateError> for AgentOsClientBridgeError {
    fn from(value: ClientSubstrateError) -> Self {
        Self::Substrate {
            reason: value.to_string(),
        }
    }
}

impl From<ServerSyncApplyError> for AgentOsClientBridgeError {
    fn from(value: ServerSyncApplyError) -> Self {
        Self::ServerSyncApply {
            reason: value.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub ok: bool,
    pub json: String,
}

impl BridgeResponse {
    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self, AgentOsClientBridgeError> {
        Ok(Self {
            ok: true,
            json: serde_json::to_string(value).map_err(|source| {
                AgentOsClientBridgeError::Serialization {
                    reason: source.to_string(),
                }
            })?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSkillInstructionBundle {
    pub installation_id: String,
    pub skill_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeTaskContext {
    pub user_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_skill_key: Option<String>,
    pub developer_instructions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SageInvocationRequest {
    pub plugin_key: String,
    pub permission_key: String,
    pub risk_level: RuntimeRiskLevel,
    #[serde(default)]
    pub user_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SageRuntimeDecision {
    Allow,
    RequiresConfirmation { reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeKnowledgeCitation {
    pub entry_id: String,
    pub title: String,
    pub excerpt: String,
    pub content_hash: String,
}

#[derive(Clone)]
pub struct AgentOsClientBridge {
    substrate: ClientSubstrate,
}

impl AgentOsClientBridge {
    /// Construct a deterministic test/development bridge. Production hosts
    /// should construct `ClientSubstrate` with production dependencies and pass
    /// it through `from_substrate`.
    pub fn for_local_development() -> Result<Self, AgentOsClientBridgeError> {
        let substrate = ClientSubstrate::builder().build()?;
        Ok(Self { substrate })
    }

    pub fn from_substrate(substrate: ClientSubstrate) -> Self {
        Self { substrate }
    }

    pub fn api_version(&self) -> u32 {
        client_substrate::CLIENT_SUBSTRATE_API_VERSION
    }

    pub fn latest_event_cursor_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.latest_event_cursor())
    }

    pub fn events_after_json(
        &self,
        cursor_json: &str,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        let cursor: ClientEventCursor = serde_json::from_str(cursor_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid event cursor json: {source}"),
            }
        })?;
        let events: Vec<ClientEventEnvelope> = self.substrate.events_after(cursor);
        BridgeResponse::from_serializable(&events)
    }

    pub fn conversation_list_projection_json(
        &self,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.conversation_list_projection())
    }

    pub fn run_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.run_projection())
    }

    pub fn approval_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.approval_projection())
    }

    pub fn storage_health_report_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.storage_health_report())
    }

    pub fn knowledge_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.knowledge_projection())
    }

    pub fn asset_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.asset_projection())
    }

    /// Apply backend M2.3 knowledge sync events to a JSON-encoded knowledge projection.
    ///
    /// `projection_json` should be a serialized `KnowledgeSyncProjection`. Empty input
    /// starts from an empty projection. `events_json` should be a JSON array of
    /// `ServerSyncEvent` values returned by the backend `/api/v1/sync/events` API.
    ///
    /// The returned JSON is the updated `KnowledgeSyncProjection`, including the advanced
    /// cursor. Hosts should durably persist this JSON before acking the backend cursor.
    pub fn apply_knowledge_sync_events_json(
        &self,
        projection_json: &str,
        events_json: &str,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        apply_knowledge_sync_events_json(projection_json, events_json)
    }

    /// Apply a full backend `/api/v1/sync/events` JSON response envelope.
    ///
    /// This accepts the standard API response shape used by the Go backend:
    /// `{"code":0,"data":{"events":[...],"next_after_sequence":...}}`.
    pub fn apply_knowledge_sync_pull_response_json(
        &self,
        projection_json: &str,
        pull_response_json: &str,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        apply_knowledge_sync_pull_response_json(projection_json, pull_response_json)
    }

    /// Apply a full backend `/api/v1/sync/events` response envelope to the general
    /// client-ready sync projection.
    pub fn apply_sync_pull_response_json(
        &self,
        projection_json: &str,
        pull_response_json: &str,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        apply_sync_pull_response_json(projection_json, pull_response_json)
    }

    pub async fn shutdown(&self) -> Result<(), AgentOsClientBridgeError> {
        self.substrate
            .host_api_for_bridge()
            .shutdown()
            .await
            .map_err(|source| AgentOsClientBridgeError::Substrate {
                reason: source.to_string(),
            })
    }
}

/// Client-ready projection for backend sync events across object families.
///
/// The projection is intentionally JSON-first and conservative. It stores last accepted
/// snapshots by backend object id for non-knowledge object families, while delegating
/// knowledge merge semantics to `KnowledgeSyncProjection`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ClientReadySyncProjection {
    pub cursor: ServerSyncCursor,
    #[serde(default)]
    pub knowledge: KnowledgeSyncProjection,
    #[serde(default)]
    pub contacts: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub profiles: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub conversations: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub participants: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub conversation_reads: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub messages: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub message_reactions: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub skills: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub agents: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub servers: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub plugins: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub plugin_permissions: HashMap<String, serde_json::Value>,
}

impl ClientReadySyncProjection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds task-scoped runtime instruction bundles from active Skill installation
    /// snapshots. This is the Stage 5B consumption boundary: hosts can prove that
    /// backend Skill Hub installation state has reached the Agent Runtime before
    /// acking the sync cursor.
    pub fn skill_runtime_bundles(&self) -> Vec<RuntimeSkillInstructionBundle> {
        let mut bundles: Vec<RuntimeSkillInstructionBundle> = self
            .skills
            .iter()
            .filter_map(|(installation_id, payload)| {
                if !is_active_payload(payload) {
                    return None;
                }
                let skill_key = payload
                    .get("skill_key")
                    .or_else(|| payload.get("skill_id"))
                    .and_then(|value| value.as_str())?
                    .to_string();
                let instructions = extract_instruction_texts(payload);
                if instructions.is_empty() {
                    return None;
                }
                Some(RuntimeSkillInstructionBundle {
                    installation_id: installation_id.clone(),
                    skill_key,
                    version_id: payload
                        .get("version_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    instructions,
                })
            })
            .collect();
        bundles.sort_by(|left, right| left.skill_key.cmp(&right.skill_key));
        bundles
    }

    pub fn apply_skill_to_task_context(
        &self,
        skill_key: &str,
        user_task: impl Into<String>,
    ) -> Result<RuntimeTaskContext, AgentOsClientBridgeError> {
        let bundle = self
            .skill_runtime_bundles()
            .into_iter()
            .find(|bundle| bundle.skill_key == skill_key)
            .ok_or_else(|| AgentOsClientBridgeError::InvalidArgument {
                reason: format!("active skill `{skill_key}` is not installed in projection"),
            })?;
        Ok(RuntimeTaskContext {
            user_task: user_task.into(),
            applied_skill_key: Some(bundle.skill_key),
            developer_instructions: bundle.instructions,
        })
    }

    /// Evaluates whether a projected SAGE plugin installation can execute a
    /// requested permission. The backend remains the source of truth for grants;
    /// the client/runtime is the execution point that must deny, allow, or require
    /// confirmation before calling a plugin flow.
    pub fn evaluate_sage_invocation(
        &self,
        request: &SageInvocationRequest,
    ) -> SageRuntimeDecision {
        let Some((installation_id, _plugin)) = self.plugins.iter().find(|(_, payload)| {
            is_active_payload(payload)
                && payload
                    .get("plugin_key")
                    .and_then(|value| value.as_str())
                    == Some(request.plugin_key.as_str())
        }) else {
            return SageRuntimeDecision::Deny {
                reason: "plugin_not_installed".to_string(),
            };
        };

        let Some(grant) = self.plugin_permissions.values().find(|payload| {
            is_active_payload(payload)
                && payload
                    .get("installation_id")
                    .and_then(|value| value.as_str())
                    == Some(installation_id.as_str())
                && payload
                    .get("permission_key")
                    .and_then(|value| value.as_str())
                    == Some(request.permission_key.as_str())
        }) else {
            return SageRuntimeDecision::Deny {
                reason: "permission_not_granted".to_string(),
            };
        };

        let requires_confirmation = request.risk_level == RuntimeRiskLevel::High
            || grant
                .get("requires_confirmation")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            || grant
                .get("risk_level")
                .and_then(|value| value.as_str())
                == Some("high");
        if requires_confirmation && !request.user_confirmed {
            return SageRuntimeDecision::RequiresConfirmation {
                reason: "high_risk_action_requires_confirmation".to_string(),
            };
        }
        SageRuntimeDecision::Allow
    }

    /// Returns simple citation-ready knowledge context from the projected personal
    /// knowledge store. Stage 5B intentionally proves the runtime consumption loop
    /// first; richer chunking/reranking can follow after the loop exists.
    pub fn retrieve_knowledge_context(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<RuntimeKnowledgeCitation> {
        let query = query.to_lowercase();
        let mut citations: Vec<RuntimeKnowledgeCitation> = self
            .knowledge
            .entries
            .values()
            .filter(|entry| entry.status == KnowledgeEntrySyncStatus::Active)
            .filter(|entry| {
                query.trim().is_empty()
                    || entry.title.to_lowercase().contains(&query)
                    || entry.content_markdown.to_lowercase().contains(&query)
                    || entry.tags.iter().any(|tag| tag.to_lowercase().contains(&query))
            })
            .map(|entry| RuntimeKnowledgeCitation {
                entry_id: entry.entry_id.clone(),
                title: entry.title.clone(),
                excerpt: excerpt(&entry.content_markdown, 180),
                content_hash: entry.content_hash.clone(),
            })
            .collect();
        citations.sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
        citations.truncate(limit);
        citations
    }

    pub fn apply_events(
        &mut self,
        events: &[ServerSyncEvent],
    ) -> Result<(), AgentOsClientBridgeError> {
        for event in events {
            self.apply_event(event)?;
        }
        Ok(())
    }

    fn apply_event(&mut self, event: &ServerSyncEvent) -> Result<(), AgentOsClientBridgeError> {
        if !self.cursor.should_apply(event) {
            return Ok(());
        }
        event.ensure_supported()?;
        match event.object_type {
            ServerSyncObjectType::Knowledge => {
                self.knowledge.apply_event(event)?;
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Profile => {
                require_operation(event, &[ServerSyncOperation::Updated])?;
                upsert_snapshot(&mut self.profiles, &event.object_id, event.payload.clone());
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Contact => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Created,
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Deleted,
                    ],
                )?;
                apply_snapshot_operation(&mut self.contacts, event);
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Conversation => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Created,
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Read,
                    ],
                )?;
                if event.operation == ServerSyncOperation::Read {
                    upsert_snapshot(
                        &mut self.conversation_reads,
                        &event.object_id,
                        event.payload.clone(),
                    );
                } else {
                    upsert_snapshot(
                        &mut self.conversations,
                        &event.object_id,
                        event.payload.clone(),
                    );
                }
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Participant => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Added,
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Removed,
                    ],
                )?;
                apply_snapshot_operation(&mut self.participants, event);
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Message => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Created,
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Deleted,
                        ServerSyncOperation::ReactionAdded,
                        ServerSyncOperation::ReactionRemoved,
                    ],
                )?;
                match event.operation {
                    ServerSyncOperation::ReactionAdded | ServerSyncOperation::ReactionRemoved => {
                        apply_reaction_operation(&mut self.message_reactions, event);
                    }
                    _ => apply_snapshot_operation(&mut self.messages, event),
                }
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Skill => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Enabled,
                        ServerSyncOperation::Disabled,
                        ServerSyncOperation::Installed,
                        ServerSyncOperation::Uninstalled,
                    ],
                )?;
                apply_snapshot_operation(&mut self.skills, event);
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Agent => {
                require_operation(event, &[ServerSyncOperation::Updated])?;
                upsert_snapshot(&mut self.agents, &event.object_id, event.payload.clone());
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Server => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Added,
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Removed,
                    ],
                )?;
                apply_snapshot_operation(&mut self.servers, event);
                self.cursor.advance_to(event.sequence);
            }
            ServerSyncObjectType::Plugin => {
                require_operation(
                    event,
                    &[
                        ServerSyncOperation::Added,
                        ServerSyncOperation::Updated,
                        ServerSyncOperation::Removed,
                        ServerSyncOperation::Installed,
                        ServerSyncOperation::Uninstalled,
                        ServerSyncOperation::Enabled,
                        ServerSyncOperation::Disabled,
                        ServerSyncOperation::PermissionGranted,
                        ServerSyncOperation::PermissionRevoked,
                    ],
                )?;
                apply_plugin_event(self, event);
                self.cursor.advance_to(event.sequence);
            }
        }
        Ok(())
    }
}

/// Stateless JSON-safe helper for applying backend M2.3 knowledge sync events.
///
/// This free function mirrors the bridge method so FFI or UniFFI layers can expose it
/// without requiring an `AgentOsClientBridge` instance when they only need reducer logic.
pub fn apply_knowledge_sync_events_json(
    projection_json: &str,
    events_json: &str,
) -> Result<BridgeResponse, AgentOsClientBridgeError> {
    let events: Vec<ServerSyncEvent> = serde_json::from_str(events_json).map_err(|source| {
        AgentOsClientBridgeError::InvalidArgument {
            reason: format!("invalid server sync events json: {source}"),
        }
    })?;
    apply_knowledge_sync_events(projection_json, &events)
}

/// Apply a full backend `/api/v1/sync/events` JSON response envelope.
pub fn apply_knowledge_sync_pull_response_json(
    projection_json: &str,
    pull_response_json: &str,
) -> Result<BridgeResponse, AgentOsClientBridgeError> {
    let response: BackendApiResponse<BackendSyncPullData> =
        serde_json::from_str(pull_response_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid backend sync pull response json: {source}"),
            }
        })?;
    if response.code != 0 {
        return Err(AgentOsClientBridgeError::InvalidArgument {
            reason: format!(
                "backend sync pull response failed: code={} message={}",
                response.code, response.message
            ),
        });
    }
    apply_knowledge_sync_events(projection_json, &response.data.events)
}

fn apply_knowledge_sync_events(
    projection_json: &str,
    events: &[ServerSyncEvent],
) -> Result<BridgeResponse, AgentOsClientBridgeError> {
    let mut projection = if projection_json.trim().is_empty() {
        KnowledgeSyncProjection::new()
    } else {
        serde_json::from_str::<KnowledgeSyncProjection>(projection_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid knowledge projection json: {source}"),
            }
        })?
    };
    projection.apply_events(events)?;
    BridgeResponse::from_serializable(&projection)
}

/// Apply a full backend `/api/v1/sync/events` JSON response envelope to a
/// client-ready projection covering all currently supported backend object families.
pub fn apply_sync_pull_response_json(
    projection_json: &str,
    pull_response_json: &str,
) -> Result<BridgeResponse, AgentOsClientBridgeError> {
    let response: BackendApiResponse<BackendSyncPullData> =
        serde_json::from_str(pull_response_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid backend sync pull response json: {source}"),
            }
        })?;
    if response.code != 0 {
        return Err(AgentOsClientBridgeError::InvalidArgument {
            reason: format!(
                "backend sync pull response failed: code={} message={}",
                response.code, response.message
            ),
        });
    }
    apply_sync_events(projection_json, &response.data.events)
}

fn apply_sync_events(
    projection_json: &str,
    events: &[ServerSyncEvent],
) -> Result<BridgeResponse, AgentOsClientBridgeError> {
    let mut projection = if projection_json.trim().is_empty() {
        ClientReadySyncProjection::new()
    } else {
        serde_json::from_str::<ClientReadySyncProjection>(projection_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid client-ready sync projection json: {source}"),
            }
        })?
    };
    projection.apply_events(events)?;
    BridgeResponse::from_serializable(&projection)
}

fn require_operation(
    event: &ServerSyncEvent,
    allowed: &[ServerSyncOperation],
) -> Result<(), AgentOsClientBridgeError> {
    if allowed
        .iter()
        .any(|operation| operation == &event.operation)
    {
        return Ok(());
    }
    Err(AgentOsClientBridgeError::ServerSyncApply {
        reason: format!(
            "unexpected operation {:?} for object type {:?}",
            event.operation, event.object_type
        ),
    })
}

fn upsert_snapshot(
    map: &mut HashMap<String, serde_json::Value>,
    key: &str,
    payload: serde_json::Value,
) {
    map.insert(key.to_string(), payload);
}

fn apply_snapshot_operation(map: &mut HashMap<String, serde_json::Value>, event: &ServerSyncEvent) {
    match event.operation {
        ServerSyncOperation::Deleted
        | ServerSyncOperation::Removed
        | ServerSyncOperation::Uninstalled => {
            map.remove(&event.object_id);
        }
        _ => upsert_snapshot(map, &event.object_id, event.payload.clone()),
    }
}

fn apply_reaction_operation(map: &mut HashMap<String, serde_json::Value>, event: &ServerSyncEvent) {
    match event.operation {
        ServerSyncOperation::ReactionRemoved => {
            map.remove(&event.object_id);
        }
        _ => upsert_snapshot(map, &event.object_id, event.payload.clone()),
    }
}

fn is_active_payload(payload: &serde_json::Value) -> bool {
    !matches!(
        payload.get("status").and_then(|value| value.as_str()),
        Some("disabled" | "deleted" | "removed" | "uninstalled" | "inactive")
    )
}

fn extract_instruction_texts(payload: &serde_json::Value) -> Vec<String> {
    if let Some(instructions) = payload.get("runtime_instructions").and_then(|value| value.as_array()) {
        return instructions
            .iter()
            .filter_map(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    if let Some(instruction) = payload.get("runtime_instruction").and_then(|value| value.as_str()) {
        let instruction = instruction.trim();
        if !instruction.is_empty() {
            return vec![instruction.to_string()];
        }
    }
    if let Some(entrypoint) = payload.get("entrypoint_content").and_then(|value| value.as_str()) {
        let entrypoint = entrypoint.trim();
        if !entrypoint.is_empty() {
            return vec![entrypoint.to_string()];
        }
    }
    Vec::new()
}

fn excerpt(content: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in content.trim().chars().take(max_chars) {
        out.push(ch);
    }
    out
}

fn apply_plugin_event(projection: &mut ClientReadySyncProjection, event: &ServerSyncEvent) {
    match event.operation {
        ServerSyncOperation::PermissionGranted | ServerSyncOperation::PermissionRevoked => {
            let permission_key = event
                .payload
                .get("grant_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let permission = event
                        .payload
                        .get("permission_key")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown");
                    format!("{}:{permission}", event.object_id)
                });
            if event.operation == ServerSyncOperation::PermissionRevoked {
                projection.plugin_permissions.remove(&permission_key);
            } else {
                projection
                    .plugin_permissions
                    .insert(permission_key, event.payload.clone());
            }
        }
        ServerSyncOperation::Uninstalled | ServerSyncOperation::Removed => {
            projection.plugins.remove(&event.object_id);
            let prefix = format!("{}:", event.object_id);
            projection.plugin_permissions.retain(|key, value| {
                key != &event.object_id
                    && !key.starts_with(&prefix)
                    && value.get("installation_id").and_then(|v| v.as_str())
                        != Some(event.object_id.as_str())
            });
        }
        _ => upsert_snapshot(
            &mut projection.plugins,
            &event.object_id,
            event.payload.clone(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_exposes_api_version() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        assert_eq!(bridge.api_version(), 1);
    }

    #[test]
    fn bridge_returns_json_projection() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let response = bridge.conversation_list_projection_json().unwrap();
        assert!(response.ok);
        assert!(response.json.contains("conversations"));
    }

    #[test]
    fn bridge_rejects_invalid_cursor_json() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let err = bridge.events_after_json("not-json").unwrap_err();
        assert!(matches!(
            err,
            AgentOsClientBridgeError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn bridge_exposes_health_and_knowledge_asset_projections() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        assert!(
            bridge
                .storage_health_report_json()
                .unwrap()
                .json
                .contains("healthy")
        );
        assert!(
            bridge
                .knowledge_projection_json()
                .unwrap()
                .json
                .contains("results")
        );
        assert!(
            bridge
                .asset_projection_json()
                .unwrap()
                .json
                .contains("assets")
        );
    }

    #[test]
    fn bridge_applies_knowledge_sync_events_json() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let events = serde_json::json!([
            {
                "id": "evt-1",
                "user_id": "user-1",
                "device_id": "device-b",
                "event_type": "knowledge.created",
                "schema_version": 1,
                "object_type": "knowledge",
                "object_id": "notes/alpha",
                "operation": "created",
                "source_device_id": "device-a",
                "client_event_id": "client-1",
                "payload": {
                    "entry_id": "notes/alpha",
                    "object_id": "notes/alpha",
                    "title": "Alpha",
                    "content_markdown": "# Alpha",
                    "summary": "summary",
                    "tags": ["agentos"],
                    "metadata": {},
                    "source_uri": "",
                    "status": "active",
                    "version": 1,
                    "content_hash": "hash-1",
                    "updated_by_device_id": "device-a",
                    "updated_at": "2026-05-30T02:00:00Z"
                },
                "timestamp": "2026-05-30T02:00:01Z",
                "sequence": 1
            },
            {
                "id": "evt-2",
                "user_id": "user-1",
                "device_id": "device-b",
                "event_type": "knowledge.deleted",
                "schema_version": 1,
                "object_type": "knowledge",
                "object_id": "notes/alpha",
                "operation": "deleted",
                "source_device_id": "device-a",
                "client_event_id": "client-2",
                "payload": {
                    "entry_id": "notes/alpha",
                    "object_id": "notes/alpha",
                    "title": "Alpha",
                    "content_markdown": "# Alpha",
                    "summary": "summary",
                    "tags": ["agentos"],
                    "metadata": {},
                    "source_uri": "",
                    "status": "deleted",
                    "version": 2,
                    "content_hash": "hash-2",
                    "updated_by_device_id": "device-a",
                    "updated_at": "2026-05-30T02:05:00Z",
                    "deleted_at": "2026-05-30T02:05:00Z"
                },
                "timestamp": "2026-05-30T02:05:01Z",
                "sequence": 2
            }
        ]);

        let response = bridge
            .apply_knowledge_sync_events_json("", &events.to_string())
            .unwrap();
        assert!(response.ok);
        let projection: sync_runtime::KnowledgeSyncProjection =
            serde_json::from_str(&response.json).unwrap();
        assert_eq!(projection.cursor.last_applied_sequence, 2);
        let entry = projection.entries.get("notes/alpha").unwrap();
        assert_eq!(
            entry.status,
            sync_runtime::KnowledgeEntrySyncStatus::Deleted
        );
        assert_eq!(entry.version, 2);
    }

    #[test]
    fn bridge_applies_backend_sync_pull_response_json() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let pull_response = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [
                    {
                        "id": "evt-10",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "knowledge.created",
                        "schema_version": 1,
                        "object_type": "knowledge",
                        "object_id": "notes/from-backend-envelope",
                        "operation": "created",
                        "source_device_id": "device-a",
                        "client_event_id": "client-10",
                        "payload": {
                            "entry_id": "notes/from-backend-envelope",
                            "object_id": "notes/from-backend-envelope",
                            "title": "Backend Envelope",
                            "content_markdown": "# Backend Envelope",
                            "summary": "summary",
                            "tags": ["agentos"],
                            "metadata": {"source": "backend"},
                            "source_uri": "",
                            "status": "active",
                            "version": 1,
                            "content_hash": "hash-envelope-1",
                            "updated_by_device_id": "device-a",
                            "updated_at": "2026-05-30T02:10:00Z"
                        },
                        "timestamp": "2026-05-30T02:10:01Z",
                        "sequence": 10
                    }
                ],
                "next_after_sequence": 10,
                "has_more": false,
                "server_time": 1780107001000i64,
                "schema_version": 1
            }
        });

        let response = bridge
            .apply_knowledge_sync_pull_response_json("", &pull_response.to_string())
            .unwrap();
        let projection: sync_runtime::KnowledgeSyncProjection =
            serde_json::from_str(&response.json).unwrap();
        assert_eq!(projection.cursor.last_applied_sequence, 10);
        assert_eq!(
            projection
                .entries
                .get("notes/from-backend-envelope")
                .unwrap()
                .title,
            "Backend Envelope"
        );
    }

    #[test]
    fn bridge_applies_client_ready_sync_pull_response_json() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let pull_response = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [
                    {
                        "id": "evt-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "skill.enabled",
                        "schema_version": 1,
                        "object_type": "skill",
                        "object_id": "superpowers",
                        "operation": "enabled",
                        "source_device_id": "device-a",
                        "client_event_id": "skill-enable-1",
                        "payload": {"skill_id": "superpowers", "object_id": "superpowers", "enabled": true, "config": {}, "updated_by_device_id": "device-a"},
                        "timestamp": "2026-06-01T01:00:00Z",
                        "sequence": 1
                    },
                    {
                        "id": "evt-2",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "agent.updated",
                        "schema_version": 1,
                        "object_type": "agent",
                        "object_id": "assistant-main",
                        "operation": "updated",
                        "source_device_id": "device-a",
                        "client_event_id": "agent-update-1",
                        "payload": {"agent_id": "assistant-main", "object_id": "assistant-main", "display_name": "Assistant", "config": {"model": "fast"}},
                        "timestamp": "2026-06-01T01:01:00Z",
                        "sequence": 2
                    },
                    {
                        "id": "evt-3",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "server.added",
                        "schema_version": 1,
                        "object_type": "server",
                        "object_id": "server-record-1",
                        "operation": "added",
                        "source_device_id": "device-a",
                        "client_event_id": "server-add-1",
                        "payload": {"server_id": "primary", "object_id": "server-record-1", "name": "Primary", "base_url": "https://agent.example", "status": "active"},
                        "timestamp": "2026-06-01T01:02:00Z",
                        "sequence": 3
                    },
                    {
                        "id": "evt-4",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "plugin.installed",
                        "schema_version": 1,
                        "object_type": "plugin",
                        "object_id": "installation-1",
                        "operation": "installed",
                        "source_device_id": "device-a",
                        "client_event_id": "plugin-install-1",
                        "payload": {"installation_id": "installation-1", "plugin_id": "plugin-1", "plugin_key": "com.example.hotel", "version_id": "version-1", "status": "active", "track_mode": "latest_approved"},
                        "timestamp": "2026-06-01T01:03:00Z",
                        "sequence": 4
                    },
                    {
                        "id": "evt-5",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "plugin.permission_granted",
                        "schema_version": 1,
                        "object_type": "plugin",
                        "object_id": "installation-1",
                        "operation": "permission_granted",
                        "source_device_id": "device-a",
                        "client_event_id": "plugin-grant-1",
                        "payload": {"installation_id": "installation-1", "grant_id": "grant-1", "permission_key": "plugin.api.call", "plugin_id": "plugin-1", "risk_level": "low", "status": "active"},
                        "timestamp": "2026-06-01T01:04:00Z",
                        "sequence": 5
                    },
                    {
                        "id": "evt-6",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "knowledge.created",
                        "schema_version": 1,
                        "object_type": "knowledge",
                        "object_id": "notes/client-ready",
                        "operation": "created",
                        "source_device_id": "device-a",
                        "client_event_id": "knowledge-create-1",
                        "payload": {
                            "entry_id": "notes/client-ready",
                            "object_id": "notes/client-ready",
                            "title": "Client Ready",
                            "content_markdown": "# Client Ready",
                            "summary": "summary",
                            "tags": ["agentos"],
                            "metadata": {},
                            "source_uri": "",
                            "status": "active",
                            "version": 1,
                            "content_hash": "hash-client-ready-1",
                            "updated_by_device_id": "device-a",
                            "updated_at": "2026-06-01T01:05:00Z"
                        },
                        "timestamp": "2026-06-01T01:05:00Z",
                        "sequence": 6
                    }
                ],
                "next_after_sequence": 6,
                "has_more": false,
                "server_time": 1780275900000i64,
                "schema_version": 1
            }
        });

        let response = bridge
            .apply_sync_pull_response_json("", &pull_response.to_string())
            .unwrap();
        let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
        assert_eq!(projection.cursor.last_applied_sequence, 6);
        assert!(projection.skills.contains_key("superpowers"));
        assert!(projection.agents.contains_key("assistant-main"));
        assert!(projection.servers.contains_key("server-record-1"));
        assert_eq!(
            projection
                .plugins
                .get("installation-1")
                .unwrap()
                .get("plugin_key")
                .and_then(|value| value.as_str()),
            Some("com.example.hotel")
        );
        assert!(projection.plugin_permissions.contains_key("grant-1"));
        assert!(
            projection
                .knowledge
                .entries
                .contains_key("notes/client-ready")
        );
    }

    #[test]
    fn bridge_applies_conversation_participant_reaction_and_skill_installation_events() {
        let pull_response = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [
                    {
                        "id": "evt-conversation-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "conversation.created",
                        "schema_version": 1,
                        "object_type": "conversation",
                        "object_id": "conversation-1",
                        "operation": "created",
                        "source_device_id": "device-a",
                        "client_event_id": "conversation-create-1",
                        "payload": {"object_id": "conversation-1", "conversation_id": "conversation-1", "type": "private", "name": "DM", "created_by": "user-1", "created_at": "2026-06-01T02:00:00Z", "updated_at": "2026-06-01T02:00:00Z"},
                        "timestamp": "2026-06-01T02:00:00Z",
                        "sequence": 1
                    },
                    {
                        "id": "evt-participant-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "participant.added",
                        "schema_version": 1,
                        "object_type": "participant",
                        "object_id": "conversation-1:user-1",
                        "operation": "added",
                        "source_device_id": "device-a",
                        "client_event_id": "participant-add-1",
                        "payload": {"object_id": "conversation-1:user-1", "conversation_id": "conversation-1", "user_id": "user-1", "role": "member", "status": "active", "joined_at": "2026-06-01T02:00:00Z"},
                        "timestamp": "2026-06-01T02:01:00Z",
                        "sequence": 2
                    },
                    {
                        "id": "evt-conversation-read-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "conversation.read",
                        "schema_version": 1,
                        "object_type": "conversation",
                        "object_id": "conversation-1:user-1",
                        "operation": "read",
                        "source_device_id": "device-a",
                        "client_event_id": "conversation-read-1",
                        "payload": {"object_id": "conversation-1:user-1", "conversation_id": "conversation-1", "user_id": "user-1", "last_read_message_id": "message-1", "last_read_at": "2026-06-01T02:02:00Z"},
                        "timestamp": "2026-06-01T02:02:00Z",
                        "sequence": 3
                    },
                    {
                        "id": "evt-reaction-add-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "message.reaction_added",
                        "schema_version": 1,
                        "object_type": "message",
                        "object_id": "message-1:user-1:👍",
                        "operation": "reaction_added",
                        "source_device_id": "device-a",
                        "client_event_id": "reaction-add-1",
                        "payload": {"object_id": "message-1:user-1:👍", "conversation_id": "conversation-1", "message_id": "message-1", "user_id": "user-1", "emoji": "👍", "created_at": "2026-06-01T02:03:00Z"},
                        "timestamp": "2026-06-01T02:03:00Z",
                        "sequence": 4
                    },
                    {
                        "id": "evt-reaction-remove-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "message.reaction_removed",
                        "schema_version": 1,
                        "object_type": "message",
                        "object_id": "message-1:user-1:👍",
                        "operation": "reaction_removed",
                        "source_device_id": "device-a",
                        "client_event_id": "reaction-remove-1",
                        "payload": {"object_id": "message-1:user-1:👍", "conversation_id": "conversation-1", "message_id": "message-1", "user_id": "user-1", "emoji": "👍"},
                        "timestamp": "2026-06-01T02:04:00Z",
                        "sequence": 5
                    },
                    {
                        "id": "evt-skill-install-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "skill.installed",
                        "schema_version": 1,
                        "object_type": "skill",
                        "object_id": "installation-1",
                        "operation": "installed",
                        "source_device_id": "device-a",
                        "client_event_id": "skill-install-1",
                        "payload": {"object_id": "installation-1", "installation_id": "installation-1", "skill_id": "skill-1", "skill_key": "research.brief", "version_id": "version-1", "track_mode": "latest", "status": "active", "config": {}},
                        "timestamp": "2026-06-01T02:05:00Z",
                        "sequence": 6
                    },
                    {
                        "id": "evt-skill-uninstall-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "skill.uninstalled",
                        "schema_version": 1,
                        "object_type": "skill",
                        "object_id": "installation-removed",
                        "operation": "uninstalled",
                        "source_device_id": "device-a",
                        "client_event_id": "skill-uninstall-1",
                        "payload": {"object_id": "installation-removed", "installation_id": "installation-removed", "skill_id": "skill-removed", "skill_key": "removed.skill", "status": "uninstalled"},
                        "timestamp": "2026-06-01T02:06:00Z",
                        "sequence": 7
                    }
                ],
                "next_after_sequence": 7,
                "has_more": false,
                "server_time": 1780279560000i64,
                "schema_version": 1
            }
        });

        let response = apply_sync_pull_response_json("", &pull_response.to_string()).unwrap();
        let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
        assert_eq!(projection.cursor.last_applied_sequence, 7);
        assert!(projection.conversations.contains_key("conversation-1"));
        assert!(
            projection
                .participants
                .contains_key("conversation-1:user-1")
        );
        assert!(
            projection
                .conversation_reads
                .contains_key("conversation-1:user-1")
        );
        assert!(
            !projection
                .message_reactions
                .contains_key("message-1:user-1:👍")
        );
        assert_eq!(
            projection
                .skills
                .get("installation-1")
                .unwrap()
                .get("skill_key")
                .and_then(|value| value.as_str()),
            Some("research.brief")
        );
        assert!(!projection.skills.contains_key("installation-removed"));
    }

    #[test]
    fn stage5b_skill_sync_projection_reaches_runtime_task_context() {
        let pull_response = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [{
                    "id": "evt-skill-stage5b-1",
                    "user_id": "user-1",
                    "device_id": "device-b",
                    "event_type": "skill.installed",
                    "schema_version": 1,
                    "object_type": "skill",
                    "object_id": "install-research-brief",
                    "operation": "installed",
                    "source_device_id": "device-a",
                    "client_event_id": "install-research-brief-1",
                    "payload": {
                        "object_id": "install-research-brief",
                        "installation_id": "install-research-brief",
                        "skill_key": "research.brief",
                        "version_id": "version-1",
                        "status": "active",
                        "runtime_instructions": [
                            "Write a research brief with Findings, Evidence, Uncertainty, and Next Actions sections."
                        ]
                    },
                    "timestamp": "2026-06-02T02:00:00Z",
                    "sequence": 1
                }],
                "next_after_sequence": 1,
                "has_more": false,
                "server_time": 1780336800000_i64,
                "schema_version": 1
            }
        });

        let response = apply_sync_pull_response_json("", &pull_response.to_string()).unwrap();
        let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
        let context = projection
            .apply_skill_to_task_context("research.brief", "Analyze this article")
            .unwrap();
        assert_eq!(context.applied_skill_key.as_deref(), Some("research.brief"));
        assert!(context.developer_instructions[0].contains("research brief"));

        let uninstall = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [{
                    "id": "evt-skill-stage5b-2",
                    "user_id": "user-1",
                    "device_id": "device-b",
                    "event_type": "skill.uninstalled",
                    "schema_version": 1,
                    "object_type": "skill",
                    "object_id": "install-research-brief",
                    "operation": "uninstalled",
                    "source_device_id": "device-a",
                    "client_event_id": "uninstall-research-brief-1",
                    "payload": {"installation_id": "install-research-brief", "skill_key": "research.brief", "status": "uninstalled"},
                    "timestamp": "2026-06-02T02:01:00Z",
                    "sequence": 2
                }],
                "next_after_sequence": 2,
                "has_more": false,
                "server_time": 1780336860000_i64,
                "schema_version": 1
            }
        });
        let response = apply_sync_pull_response_json(&response.json, &uninstall.to_string()).unwrap();
        let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
        assert!(projection.skill_runtime_bundles().is_empty());
        assert!(projection
            .apply_skill_to_task_context("research.brief", "Analyze this article")
            .is_err());
    }

    #[test]
    fn stage5b_sage_policy_projection_drives_runtime_decision() {
        let pull_response = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [
                    {
                        "id": "evt-plugin-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "plugin.installed",
                        "schema_version": 1,
                        "object_type": "plugin",
                        "object_id": "calendar-installation",
                        "operation": "installed",
                        "source_device_id": "device-a",
                        "client_event_id": "plugin-install-1",
                        "payload": {"installation_id": "calendar-installation", "plugin_key": "mock.calendar", "status": "active"},
                        "timestamp": "2026-06-02T02:10:00Z",
                        "sequence": 1
                    },
                    {
                        "id": "evt-plugin-grant-1",
                        "user_id": "user-1",
                        "device_id": "device-b",
                        "event_type": "plugin.permission_granted",
                        "schema_version": 1,
                        "object_type": "plugin",
                        "object_id": "calendar-installation",
                        "operation": "permission_granted",
                        "source_device_id": "device-a",
                        "client_event_id": "plugin-grant-1",
                        "payload": {"installation_id": "calendar-installation", "grant_id": "grant-calendar-write", "permission_key": "calendar.write", "risk_level": "high", "status": "active"},
                        "timestamp": "2026-06-02T02:11:00Z",
                        "sequence": 2
                    }
                ],
                "next_after_sequence": 2,
                "has_more": false,
                "server_time": 1780337460000_i64,
                "schema_version": 1
            }
        });
        let response = apply_sync_pull_response_json("", &pull_response.to_string()).unwrap();
        let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();

        let request = SageInvocationRequest {
            plugin_key: "mock.calendar".to_string(),
            permission_key: "calendar.write".to_string(),
            risk_level: RuntimeRiskLevel::High,
            user_confirmed: false,
        };
        assert_eq!(
            projection.evaluate_sage_invocation(&request),
            SageRuntimeDecision::RequiresConfirmation {
                reason: "high_risk_action_requires_confirmation".to_string()
            }
        );

        let confirmed = SageInvocationRequest { user_confirmed: true, ..request };
        assert_eq!(
            projection.evaluate_sage_invocation(&confirmed),
            SageRuntimeDecision::Allow
        );

        let denied = SageInvocationRequest {
            plugin_key: "mock.calendar".to_string(),
            permission_key: "calendar.delete".to_string(),
            risk_level: RuntimeRiskLevel::High,
            user_confirmed: true,
        };
        assert_eq!(
            projection.evaluate_sage_invocation(&denied),
            SageRuntimeDecision::Deny {
                reason: "permission_not_granted".to_string()
            }
        );
    }

    #[test]
    fn stage5b_knowledge_projection_supplies_runtime_citations() {
        let pull_response = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "events": [{
                    "id": "evt-kb-runtime-1",
                    "user_id": "user-1",
                    "device_id": "device-b",
                    "event_type": "knowledge.created",
                    "schema_version": 1,
                    "object_type": "knowledge",
                    "object_id": "notes/agentos-stage5b",
                    "operation": "created",
                    "source_device_id": "device-a",
                    "client_event_id": "knowledge-create-1",
                    "payload": {
                        "entry_id": "notes/agentos-stage5b",
                        "object_id": "notes/agentos-stage5b",
                        "title": "Stage 5B Runtime Consumption Loop",
                        "content_markdown": "AgentOS Stage 5B turns backend control-plane state into runtime behavior with Skill, SAGE, and KB loops.",
                        "summary": "summary",
                        "tags": ["agentos", "runtime"],
                        "metadata": {"collection_id": "kb-agentos"},
                        "source_uri": "",
                        "status": "active",
                        "version": 1,
                        "content_hash": "hash-stage5b-1",
                        "updated_by_device_id": "device-a",
                        "updated_at": "2026-06-02T02:20:00Z"
                    },
                    "timestamp": "2026-06-02T02:20:01Z",
                    "sequence": 1
                }],
                "next_after_sequence": 1,
                "has_more": false,
                "server_time": 1780338001000_i64,
                "schema_version": 1
            }
        });
        let response = apply_sync_pull_response_json("", &pull_response.to_string()).unwrap();
        let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
        let citations = projection.retrieve_knowledge_context("runtime", 3);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].entry_id, "notes/agentos-stage5b");
        assert!(citations[0].excerpt.contains("runtime behavior"));
    }

    #[test]
    fn bridge_rejects_backend_sync_error_response() {
        let err = apply_knowledge_sync_pull_response_json(
            "",
            &serde_json::json!({"code": 401, "message": "unauthorized", "data": {"events": []}})
                .to_string(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AgentOsClientBridgeError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn bridge_rejects_invalid_knowledge_sync_json() {
        let err = apply_knowledge_sync_events_json("", "not-json").unwrap_err();
        assert!(matches!(
            err,
            AgentOsClientBridgeError::InvalidArgument { .. }
        ));
    }
}
