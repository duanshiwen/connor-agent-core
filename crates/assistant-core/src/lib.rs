//! # Assistant Core
//!
//! Core Assistant domain types for AgentOS.
//!
//! The Assistant is the default foreground coordinator in user-facing
//! conversations. Browser, Mail, Knowledge, Device, and other resources should
//! be modeled as linked entities, not as foreground conversation participants.

use conversation_core::{
    ConversationId, ConversationKind, Participant, ParticipantId, ParticipantKind,
};
use conversation_kernel::{ConversationKernel, CreateConversationCommand, LinkEntityCommand};
use entity_core::{EntityCapability, EntityDescriptor, EntityId, EntityKind, LinkReason};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Unique identifier for an Assistant profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssistantId(pub String);

impl fmt::Display for AssistantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for AssistantId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AssistantId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Enabled Assistant capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AssistantCapabilitySet {
    enabled: BTreeSet<String>,
}

impl AssistantCapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_enabled(capabilities: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut set = Self::new();
        for capability in capabilities {
            set.enable(capability);
        }
        set
    }

    pub fn enable(&mut self, capability: impl Into<String>) {
        self.enabled.insert(capability.into());
    }

    pub fn disable(&mut self, capability: &str) {
        self.enabled.remove(capability);
    }

    pub fn is_enabled(&self, capability: &str) -> bool {
        self.enabled.contains(capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.enabled.iter()
    }
}

/// Assistant profile owned by a human user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantProfile {
    pub id: AssistantId,
    pub display_name: String,
    pub owner_user_id: ParticipantId,
    pub default_model: Option<String>,
    pub capability_set: AssistantCapabilitySet,
}

impl AssistantProfile {
    pub fn new(
        id: impl Into<AssistantId>,
        display_name: impl Into<String>,
        owner_user_id: ParticipantId,
    ) -> Result<Self, AssistantCoreError> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() {
            return Err(AssistantCoreError::EmptyDisplayName);
        }

        Ok(Self {
            id: id.into(),
            display_name,
            owner_user_id,
            default_model: None,
            capability_set: AssistantCapabilitySet::new(),
        })
    }

    pub fn as_participant(&self) -> Participant {
        Participant {
            id: ParticipantId::from(self.id.0.clone()),
            kind: ParticipantKind::Agent,
            display_name: self.display_name.clone(),
        }
    }
}

/// User-level Assistant preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantPreference {
    pub key: String,
    pub value: String,
}

/// Runtime configuration for Assistant sessions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AssistantRuntimeConfig {
    pub model: Option<String>,
    pub policy_ref: Option<String>,
    pub max_context_messages: Option<usize>,
}

/// Main conversation metadata for a user's Assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMainConversation {
    pub assistant_id: AssistantId,
    pub conversation_id: ConversationId,
    pub owner_user_id: ParticipantId,
}

/// Resolved context needed to run the Assistant in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantSessionContext {
    pub profile: AssistantProfile,
    pub conversation_id: ConversationId,
    pub runtime_config: AssistantRuntimeConfig,
    pub preferences: Vec<AssistantPreference>,
}

/// Minimal spec for creating Assistant-mediated conversations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantConversationSpec {
    pub kind: ConversationKind,
    pub title: Option<String>,
    pub participants: Vec<Participant>,
    pub linked_entity_kinds: Vec<EntityKind>,
}

/// Helper service for constructing Assistant foreground conversation specs.
#[derive(Debug, Clone, Default)]
pub struct AssistantConversationService;

impl AssistantConversationService {
    pub fn main_conversation_spec(
        profile: &AssistantProfile,
        owner: Participant,
    ) -> Result<AssistantConversationSpec, AssistantCoreError> {
        if owner.id != profile.owner_user_id {
            return Err(AssistantCoreError::OwnerParticipantMismatch {
                expected: profile.owner_user_id.clone(),
                actual: owner.id,
            });
        }
        if owner.kind != ParticipantKind::Human {
            return Err(AssistantCoreError::OwnerMustBeHuman(owner.id));
        }

        Ok(AssistantConversationSpec {
            kind: ConversationKind::Direct,
            title: Some(profile.display_name.clone()),
            participants: vec![owner, profile.as_participant()],
            linked_entity_kinds: vec![],
        })
    }

    pub fn group_conversation_spec(
        profile: &AssistantProfile,
        humans: Vec<Participant>,
        title: Option<String>,
    ) -> Result<AssistantConversationSpec, AssistantCoreError> {
        if humans.is_empty() {
            return Err(AssistantCoreError::AtLeastOneHumanRequired);
        }
        for participant in &humans {
            if participant.kind != ParticipantKind::Human {
                return Err(AssistantCoreError::ForegroundEntityParticipantRejected {
                    participant_id: participant.id.clone(),
                    kind: format!("{:?}", participant.kind),
                });
            }
        }

        let mut participants = humans;
        participants.push(profile.as_participant());
        Ok(AssistantConversationSpec {
            kind: ConversationKind::Group,
            title,
            participants,
            linked_entity_kinds: vec![],
        })
    }

    pub fn with_linked_entity_kinds(
        mut spec: AssistantConversationSpec,
        linked_entity_kinds: Vec<EntityKind>,
    ) -> AssistantConversationSpec {
        spec.linked_entity_kinds = linked_entity_kinds;
        spec
    }

    pub async fn create_main_conversation(
        kernel: &ConversationKernel,
        profile: &AssistantProfile,
        owner: Participant,
        linked_entities: Vec<EntityDescriptor>,
    ) -> anyhow::Result<AssistantMainConversation> {
        let spec = Self::main_conversation_spec(profile, owner)?;
        let conversation_id = Self::create_conversation_from_spec(
            kernel,
            profile,
            spec,
            linked_entities,
            Some(profile.owner_user_id.clone()),
        )
        .await?;

        Ok(AssistantMainConversation {
            assistant_id: profile.id.clone(),
            conversation_id,
            owner_user_id: profile.owner_user_id.clone(),
        })
    }

    pub async fn create_group_conversation(
        kernel: &ConversationKernel,
        profile: &AssistantProfile,
        humans: Vec<Participant>,
        title: Option<String>,
        linked_entities: Vec<EntityDescriptor>,
        actor_id: Option<ParticipantId>,
    ) -> anyhow::Result<ConversationId> {
        let spec = Self::group_conversation_spec(profile, humans, title)?;
        Self::create_conversation_from_spec(kernel, profile, spec, linked_entities, actor_id).await
    }

    async fn create_conversation_from_spec(
        kernel: &ConversationKernel,
        profile: &AssistantProfile,
        spec: AssistantConversationSpec,
        linked_entities: Vec<EntityDescriptor>,
        actor_id: Option<ParticipantId>,
    ) -> anyhow::Result<ConversationId> {
        let conversation_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: spec.kind,
                title: spec.title,
                participants: spec.participants,
                actor_id: actor_id.clone(),
            })
            .await?;

        for entity in linked_entities {
            kernel
                .link_entity(LinkEntityCommand {
                    conversation_id: conversation_id.clone(),
                    entity,
                    reason: LinkReason::SystemDefault,
                    linked_by: actor_id
                        .clone()
                        .or_else(|| Some(profile.owner_user_id.clone())),
                })
                .await?;
        }

        Ok(conversation_id)
    }

    pub fn default_entity_descriptor(kind: EntityKind) -> EntityDescriptor {
        let slug = format!("{}", EntityKindName(&kind));
        EntityDescriptor {
            id: EntityId::from(slug.clone()),
            kind,
            display_name: slug
                .split('-')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
            capabilities: vec![EntityCapability::new("metadata")],
            default_policy_ref: None,
        }
    }
}

struct EntityKindName<'a>(&'a EntityKind);

impl fmt::Display for EntityKindName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            EntityKind::Browser => "browser",
            EntityKind::Mail => "mail",
            EntityKind::Knowledge => "knowledge",
            EntityKind::Reminder => "reminder",
            EntityKind::Person => "person",
            EntityKind::Device => "device",
            EntityKind::Plugin => "plugin",
            EntityKind::ExternalService => "external-service",
            EntityKind::System => "system",
        };
        f.write_str(name)
    }
}

/// Simple preference map helper for deterministic preference resolution.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AssistantPreferenceSet {
    values: BTreeMap<String, String>,
}

impl AssistantPreferenceSet {
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// Assistant core domain errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssistantCoreError {
    #[error("assistant display name must not be empty")]
    EmptyDisplayName,

    #[error("owner participant mismatch: expected {expected}, got {actual}")]
    OwnerParticipantMismatch {
        expected: ParticipantId,
        actual: ParticipantId,
    },

    #[error("owner participant must be human: {0}")]
    OwnerMustBeHuman(ParticipantId),

    #[error("at least one human participant is required")]
    AtLeastOneHumanRequired,

    #[error("foreground entity participant rejected: {participant_id} has kind {kind}")]
    ForegroundEntityParticipantRejected {
        participant_id: ParticipantId,
        kind: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn human(id: &str, name: &str) -> Participant {
        Participant {
            id: ParticipantId::from(id),
            kind: ParticipantKind::Human,
            display_name: name.to_string(),
        }
    }

    fn integration(id: &str, name: &str) -> Participant {
        Participant {
            id: ParticipantId::from(id),
            kind: ParticipantKind::Integration,
            display_name: name.to_string(),
        }
    }

    fn profile() -> AssistantProfile {
        let mut profile =
            AssistantProfile::new("assistant-main", "Assistant", ParticipantId::from("u1"))
                .unwrap();
        profile.default_model = Some("test-model".to_string());
        profile.capability_set.enable("conversation");
        profile.capability_set.enable("linked_entities");
        profile
    }

    #[test]
    fn assistant_id_display_and_serde_roundtrip() {
        let id = AssistantId::from("assistant-main");
        assert_eq!(id.to_string(), "assistant-main");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: AssistantId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn assistant_profile_serde_roundtrip() {
        let profile = profile();

        let json = serde_json::to_string_pretty(&profile).unwrap();
        let decoded: AssistantProfile = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, profile);
    }

    #[test]
    fn capability_set_supports_enable_and_disable() {
        let mut capabilities = AssistantCapabilitySet::new();

        capabilities.enable("browser");
        assert!(capabilities.is_enabled("browser"));

        capabilities.disable("browser");
        assert!(!capabilities.is_enabled("browser"));
    }

    #[test]
    fn assistant_profile_requires_non_empty_display_name() {
        let result = AssistantProfile::new("assistant-main", "   ", ParticipantId::from("u1"));

        assert_eq!(result.unwrap_err(), AssistantCoreError::EmptyDisplayName);
    }

    #[test]
    fn assistant_profile_converts_to_agent_participant() {
        let profile = profile();
        let participant = profile.as_participant();

        assert_eq!(participant.id, ParticipantId::from("assistant-main"));
        assert_eq!(participant.kind, ParticipantKind::Agent);
        assert_eq!(participant.display_name, "Assistant");
    }

    #[test]
    fn create_main_assistant_conversation_includes_exactly_human_and_assistant() {
        let profile = profile();
        let owner = human("u1", "诗闻");

        let spec = AssistantConversationService::main_conversation_spec(&profile, owner).unwrap();

        assert_eq!(spec.kind, ConversationKind::Direct);
        assert_eq!(spec.participants.len(), 2);
        assert_eq!(spec.participants[0].kind, ParticipantKind::Human);
        assert_eq!(spec.participants[1].kind, ParticipantKind::Agent);
        assert_eq!(
            spec.participants[1].id,
            ParticipantId::from("assistant-main")
        );
    }

    #[test]
    fn create_group_assistant_conversation_includes_humans_and_assistant() {
        let profile = profile();
        let spec = AssistantConversationService::group_conversation_spec(
            &profile,
            vec![human("u1", "诗闻"), human("u2", "Teammate")],
            Some("Group".to_string()),
        )
        .unwrap();

        assert_eq!(spec.kind, ConversationKind::Group);
        assert_eq!(spec.participants.len(), 3);
        assert_eq!(spec.participants[0].kind, ParticipantKind::Human);
        assert_eq!(spec.participants[1].kind, ParticipantKind::Human);
        assert_eq!(spec.participants[2].kind, ParticipantKind::Agent);
    }

    #[test]
    fn assistant_main_conversation_rejects_non_human_owner() {
        let profile = profile();
        let owner = integration("browser-main", "Browser");

        let result = AssistantConversationService::main_conversation_spec(&profile, owner);

        assert!(matches!(
            result.unwrap_err(),
            AssistantCoreError::OwnerParticipantMismatch { .. }
                | AssistantCoreError::OwnerMustBeHuman(_)
        ));
    }

    #[test]
    fn assistant_group_conversation_rejects_browser_as_foreground_participant() {
        let profile = profile();
        let result = AssistantConversationService::group_conversation_spec(
            &profile,
            vec![human("u1", "诗闻"), integration("browser-main", "Browser")],
            Some("Bad group".to_string()),
        );

        assert!(matches!(
            result.unwrap_err(),
            AssistantCoreError::ForegroundEntityParticipantRejected { .. }
        ));
    }

    #[test]
    fn browser_can_be_modeled_as_linked_entity_kind_not_participant() {
        let profile = profile();
        let owner = human("u1", "诗闻");
        let spec = AssistantConversationService::main_conversation_spec(&profile, owner).unwrap();
        let spec = AssistantConversationService::with_linked_entity_kinds(
            spec,
            vec![EntityKind::Browser, EntityKind::Knowledge],
        );

        assert_eq!(spec.participants.len(), 2);
        assert_eq!(
            spec.linked_entity_kinds,
            vec![EntityKind::Browser, EntityKind::Knowledge]
        );
        assert!(
            !spec
                .participants
                .iter()
                .any(|participant| participant.id == ParticipantId::from("browser-main"))
        );
    }

    #[test]
    fn assistant_session_context_serde_roundtrip() {
        let context = AssistantSessionContext {
            profile: profile(),
            conversation_id: ConversationId::from("conv-001"),
            runtime_config: AssistantRuntimeConfig {
                model: Some("test-model".to_string()),
                policy_ref: Some("policy/default".to_string()),
                max_context_messages: Some(20),
            },
            preferences: vec![AssistantPreference {
                key: "tone".to_string(),
                value: "concise".to_string(),
            }],
        };

        let json = serde_json::to_string_pretty(&context).unwrap();
        let decoded: AssistantSessionContext = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, context);
    }

    #[test]
    fn preference_set_resolves_values() {
        let mut preferences = AssistantPreferenceSet::default();
        preferences.set("tone", "concise");

        assert_eq!(preferences.get("tone"), Some("concise"));
        assert_eq!(preferences.get("missing"), None);
    }
}

#[cfg(test)]
mod kernel_integration_tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use conversation_core::{MessageContent, Visibility};
    use conversation_journal::MemoryConversationJournal;
    use conversation_kernel::{AppendMessageCommand, Clock, IdGenerator};
    use std::sync::Arc;

    struct SequentialIdGenerator {
        counter: std::sync::Mutex<u64>,
    }

    impl SequentialIdGenerator {
        fn new() -> Self {
            Self {
                counter: std::sync::Mutex::new(0),
            }
        }
    }

    impl IdGenerator for SequentialIdGenerator {
        fn new_id(&self) -> String {
            let mut c = self.counter.lock().unwrap();
            *c += 1;
            format!("id-{}", c)
        }
    }

    struct FixedClock {
        time: DateTime<Utc>,
    }

    impl FixedClock {
        fn new(time: DateTime<Utc>) -> Self {
            Self { time }
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.time
        }
    }

    fn kernel() -> ConversationKernel {
        let journal = Arc::new(MemoryConversationJournal::new());
        let id_gen = Arc::new(SequentialIdGenerator::new());
        let clock = Arc::new(FixedClock::new("2026-05-24T13:00:00Z".parse().unwrap()));
        ConversationKernel::with_generators(journal, id_gen, clock)
    }

    fn human(id: &str, name: &str) -> Participant {
        Participant {
            id: ParticipantId::from(id),
            kind: ParticipantKind::Human,
            display_name: name.to_string(),
        }
    }

    fn profile() -> AssistantProfile {
        AssistantProfile::new("assistant-main", "Assistant", ParticipantId::from("u1")).unwrap()
    }

    fn browser_entity() -> EntityDescriptor {
        EntityDescriptor {
            id: EntityId::from("browser-main"),
            kind: EntityKind::Browser,
            display_name: "Browser".to_string(),
            capabilities: vec![EntityCapability::new("read_page")],
            default_policy_ref: None,
        }
    }

    #[tokio::test]
    async fn create_main_assistant_conversation_through_kernel() {
        let kernel = kernel();
        let profile = profile();

        let main = AssistantConversationService::create_main_conversation(
            &kernel,
            &profile,
            human("u1", "诗闻"),
            vec![],
        )
        .await
        .unwrap();

        let state = kernel.load_state(&main.conversation_id).await.unwrap();
        assert_eq!(main.assistant_id, AssistantId::from("assistant-main"));
        assert_eq!(main.owner_user_id, ParticipantId::from("u1"));
        assert_eq!(state.participants.len(), 2);
        assert!(state.participants.contains_key(&ParticipantId::from("u1")));
        assert!(
            state
                .participants
                .contains_key(&ParticipantId::from("assistant-main"))
        );
    }

    #[tokio::test]
    async fn create_main_conversation_links_browser_without_participant() {
        let kernel = kernel();
        let profile = profile();

        let main = AssistantConversationService::create_main_conversation(
            &kernel,
            &profile,
            human("u1", "诗闻"),
            vec![browser_entity()],
        )
        .await
        .unwrap();

        let state = kernel.load_state(&main.conversation_id).await.unwrap();
        assert!(
            state
                .linked_entities
                .contains_key(&EntityId::from("browser-main"))
        );
        assert!(
            !state
                .participants
                .contains_key(&ParticipantId::from("browser-main"))
        );
        assert_eq!(state.participants.len(), 2);
    }

    #[tokio::test]
    async fn assistant_conversation_can_immediately_append_user_message() {
        let kernel = kernel();
        let profile = profile();
        let main = AssistantConversationService::create_main_conversation(
            &kernel,
            &profile,
            human("u1", "诗闻"),
            vec![browser_entity()],
        )
        .await
        .unwrap();

        let message_id = kernel
            .append_message(AppendMessageCommand {
                conversation_id: main.conversation_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "总结这个网页".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let state = kernel.load_state(&main.conversation_id).await.unwrap();
        assert!(state.messages_by_id.contains_key(&message_id));
        assert!(
            state
                .linked_entities
                .contains_key(&EntityId::from("browser-main"))
        );
    }

    #[tokio::test]
    async fn create_group_assistant_conversation_through_kernel() {
        let kernel = kernel();
        let profile = profile();

        let conversation_id = AssistantConversationService::create_group_conversation(
            &kernel,
            &profile,
            vec![human("u1", "诗闻"), human("u2", "Teammate")],
            Some("Group".to_string()),
            vec![],
            Some(ParticipantId::from("u1")),
        )
        .await
        .unwrap();

        let state = kernel.load_state(&conversation_id).await.unwrap();
        assert_eq!(state.participants.len(), 3);
        assert!(
            state
                .participants
                .contains_key(&ParticipantId::from("assistant-main"))
        );
    }
}
