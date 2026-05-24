//! The Conversation Kernel — command processing and event production.

use crate::commands::*;
use crate::projector::ConversationProjector;
use crate::state::ConversationState;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::ConversationJournal;
use std::sync::Arc;
use uuid::Uuid;

/// Trait for generating unique IDs. Testable via injection.
pub trait IdGenerator: Send + Sync {
    fn new_id(&self) -> String;
}

/// Default ID generator using UUID v4.
pub struct UuidGenerator;

impl IdGenerator for UuidGenerator {
    fn new_id(&self) -> String {
        Uuid::new_v4().to_string()
    }
}

/// Trait for getting the current time. Testable via injection.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Default clock using system time.
pub struct UtcClock;

impl Clock for UtcClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// The Conversation Kernel.
///
/// Provides validated command processing for conversations.
/// All state changes are represented as events and persisted to the journal.
#[derive(Clone)]
pub struct ConversationKernel {
    journal: Arc<dyn ConversationJournal>,
    id_gen: Arc<dyn IdGenerator>,
    clock: Arc<dyn Clock>,
}

impl ConversationKernel {
    /// Create a new kernel with default generators (UUID + UtcClock).
    pub fn new(journal: Arc<dyn ConversationJournal>) -> Self {
        Self {
            journal,
            id_gen: Arc::new(UuidGenerator),
            clock: Arc::new(UtcClock),
        }
    }

    /// Create a new kernel with custom generators (for testing).
    pub fn with_generators(
        journal: Arc<dyn ConversationJournal>,
        id_gen: Arc<dyn IdGenerator>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            journal,
            id_gen,
            clock,
        }
    }

    /// Load all events for a conversation.
    pub async fn load_events(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Vec<ConversationEventEnvelope>> {
        self.journal.load(conversation_id).await
    }

    /// Load and project the current state of a conversation.
    pub async fn load_state(&self, conversation_id: &ConversationId) -> Result<ConversationState> {
        let events = self.load_events(conversation_id).await?;
        ConversationProjector::project(&events)
    }

    /// Create a new conversation.
    ///
    /// Produces: `ConversationCreated` + one `ParticipantAdded` per participant.
    pub async fn create_conversation(
        &self,
        cmd: CreateConversationCommand,
    ) -> Result<ConversationId> {
        if cmd.participants.is_empty() {
            bail!("at least one participant is required");
        }

        let conversation_id = ConversationId(self.id_gen.new_id());
        let now = self.clock.now();

        let session = ConversationSession {
            id: conversation_id.clone(),
            kind: cmd.kind,
            title: cmd.title,
            participants: cmd.participants.iter().map(|p| p.id.clone()).collect(),
            created_at: now,
            updated_at: now,
            status: ConversationStatus::Active,
        };

        // Append ConversationCreated event.
        self.append_envelope(
            &conversation_id,
            cmd.actor_id.clone(),
            now,
            ConversationEvent::ConversationCreated { session },
        )
        .await?;

        // Append ParticipantAdded events for each participant.
        for participant in cmd.participants {
            self.append_envelope(
                &conversation_id,
                cmd.actor_id.clone(),
                now,
                ConversationEvent::ParticipantAdded { participant },
            )
            .await?;
        }

        Ok(conversation_id)
    }

    /// Append a message to a conversation.
    ///
    /// Validates: conversation exists, sender is a participant.
    /// Produces: `MessageAppended`.
    pub async fn append_message(&self, cmd: AppendMessageCommand) -> Result<MessageId> {
        // Load state to validate.
        let state = self.load_state(&cmd.conversation_id).await?;

        let session = state
            .session
            .as_ref()
            .with_context(|| format!("conversation not found: {}", cmd.conversation_id))?;

        if session.status != ConversationStatus::Active {
            bail!("conversation is not active: {}", cmd.conversation_id);
        }

        let sender = state
            .participants
            .get(&cmd.sender_id)
            .with_context(|| format!("sender is not a participant: {}", cmd.sender_id))?;

        if !sender.kind.is_foreground() {
            bail!(
                "sender is not a foreground participant: {} (kind: {:?})",
                cmd.sender_id,
                sender.kind
            );
        }

        if let Some(reply_to) = &cmd.reply_to {
            let replied_message = state
                .messages_by_id
                .get(reply_to)
                .with_context(|| format!("reply_to message not found: {reply_to}"))?;
            if replied_message.conversation_id != cmd.conversation_id {
                bail!("reply_to message belongs to a different conversation: {reply_to}");
            }
        }

        if let Some(thread_id) = &cmd.thread_id
            && !state.threads.contains_key(thread_id)
        {
            bail!("thread not found: {thread_id}");
        }

        Self::validate_visibility(&state, &cmd.visibility)?;

        let message_id = MessageId(self.id_gen.new_id());
        let now = self.clock.now();

        let message = Message {
            id: message_id.clone(),
            conversation_id: cmd.conversation_id.clone(),
            sender_id: cmd.sender_id,
            content: cmd.content,
            reply_to: cmd.reply_to,
            thread_id: cmd.thread_id,
            visibility: cmd.visibility,
            created_at: now,
            edited_at: None,
        };

        self.append_envelope(
            &cmd.conversation_id,
            Some(message.sender_id.clone()),
            now,
            ConversationEvent::MessageAppended { message },
        )
        .await?;

        Ok(message_id)
    }

    /// Edit an existing message.
    ///
    /// Validates: conversation exists, message exists, editor is a participant.
    /// Produces: `MessageEdited` with a deterministic edit timestamp.
    pub async fn edit_message(&self, cmd: EditMessageCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;

        if state.session.is_none() {
            bail!("conversation not found: {}", cmd.conversation_id);
        }

        if !state.messages_by_id.contains_key(&cmd.message_id) {
            bail!("message not found: {}", cmd.message_id);
        }

        if !state.participants.contains_key(&cmd.edited_by) {
            bail!("edited_by is not a participant: {}", cmd.edited_by);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.edited_by),
            now,
            ConversationEvent::MessageEdited {
                message_id: cmd.message_id,
                new_content: cmd.new_content,
                edited_at: now,
            },
        )
        .await
    }

    /// Create a private assistant suggestion for a specific user.
    ///
    /// Validates: conversation exists, target user is a participant.
    /// Produces: `AssistantSuggestionCreated` with `Visibility::PrivateToUser`.
    pub async fn create_assistant_suggestion(
        &self,
        cmd: CreateAssistantSuggestionCommand,
    ) -> Result<MessageId> {
        let state = self.load_state(&cmd.conversation_id).await?;

        let session = state
            .session
            .as_ref()
            .with_context(|| format!("conversation not found: {}", cmd.conversation_id))?;

        if session.status != ConversationStatus::Active {
            bail!("conversation is not active: {}", cmd.conversation_id);
        }

        if !state.participants.contains_key(&cmd.target_user_id) {
            bail!("target user is not a participant: {}", cmd.target_user_id);
        }

        // Find an agent participant as the sender.
        let agent_sender = state
            .participants
            .values()
            .find(|p| p.kind == ParticipantKind::Agent)
            .context("no agent participant found in conversation")?;

        let message_id = MessageId(self.id_gen.new_id());
        let now = self.clock.now();

        let suggestion_message = Message {
            id: message_id.clone(),
            conversation_id: cmd.conversation_id.clone(),
            sender_id: agent_sender.id.clone(),
            content: MessageContent::AgentSuggestion {
                text: cmd.text,
                actions: cmd.actions,
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::PrivateToUser {
                user_id: cmd.target_user_id,
            },
            created_at: now,
            edited_at: None,
        };

        self.append_envelope(
            &cmd.conversation_id,
            Some(agent_sender.id.clone()),
            now,
            ConversationEvent::AssistantSuggestionCreated {
                suggestion_message,
                trigger: cmd.trigger,
            },
        )
        .await?;

        Ok(message_id)
    }

    /// Link an entity to a conversation.
    ///
    /// Validates: conversation exists and optional actor is a participant.
    /// Produces: `EntityLinkedToConversation`.
    pub async fn link_entity(&self, cmd: LinkEntityCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_conversation_exists(&state, &cmd.conversation_id)?;
        Self::validate_optional_actor(&state, cmd.linked_by.as_ref(), "linked_by")?;

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            cmd.linked_by,
            now,
            ConversationEvent::EntityLinkedToConversation {
                entity: cmd.entity,
                reason: cmd.reason,
            },
        )
        .await
    }

    /// Unlink an entity from a conversation.
    ///
    /// Validates: conversation exists, entity is linked, optional actor is a participant.
    /// Produces: `EntityUnlinkedFromConversation`.
    pub async fn unlink_entity(&self, cmd: UnlinkEntityCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_conversation_exists(&state, &cmd.conversation_id)?;
        Self::validate_optional_actor(&state, cmd.unlinked_by.as_ref(), "unlinked_by")?;

        if !state.linked_entities.contains_key(&cmd.entity_id) {
            bail!("linked entity not found: {}", cmd.entity_id);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            cmd.unlinked_by,
            now,
            ConversationEvent::EntityUnlinkedFromConversation {
                entity_id: cmd.entity_id,
                reason: cmd.reason,
            },
        )
        .await
    }

    /// Record metadata about an entity state observation.
    ///
    /// Validates: conversation exists, entity is linked, optional actor is a participant.
    /// Produces: `EntityStateObserved`.
    pub async fn observe_entity_state(&self, cmd: ObserveEntityStateCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_conversation_exists(&state, &cmd.conversation_id)?;
        Self::validate_optional_actor(&state, cmd.observed_by.as_ref(), "observed_by")?;

        if !state.linked_entities.contains_key(&cmd.entity_id) {
            bail!("linked entity not found: {}", cmd.entity_id);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            cmd.observed_by,
            now,
            ConversationEvent::EntityStateObserved {
                entity_id: cmd.entity_id,
                state_ref: cmd.state_ref,
            },
        )
        .await
    }

    /// Record metadata about an entity query.
    ///
    /// Validates: conversation exists, entity is linked, optional actor is a participant.
    /// Produces: `EntityQueried`.
    pub async fn query_entity(&self, cmd: QueryEntityCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_conversation_exists(&state, &cmd.conversation_id)?;
        Self::validate_optional_actor(&state, cmd.queried_by.as_ref(), "queried_by")?;

        if !state.linked_entities.contains_key(&cmd.entity_id) {
            bail!("linked entity not found: {}", cmd.entity_id);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            cmd.queried_by,
            now,
            ConversationEvent::EntityQueried {
                entity_id: cmd.entity_id,
                query: cmd.query,
                result_ref: cmd.result_ref,
            },
        )
        .await
    }

    /// Request an agent run. Builds a conversation slice and emits boundary events.
    ///
    /// Produces: `ContextSliceBuilt` + `AgentRunRequested`.
    pub async fn request_agent_run(&self, cmd: RequestAgentRunCommand) -> Result<String> {
        let state = self.load_state(&cmd.conversation_id).await?;

        if state.session.is_none() {
            bail!("conversation not found: {}", cmd.conversation_id);
        }

        if !state.messages_by_id.contains_key(&cmd.trigger_message_id) {
            bail!("trigger message not found: {}", cmd.trigger_message_id);
        }

        let run_id = self.id_gen.new_id();
        let slice_id = self.id_gen.new_id();
        let now = self.clock.now();

        // Build a simple recent-window slice (first 20 messages or all).
        let max_messages = 20;
        let slice_messages: Vec<Message> = if state.messages.len() > max_messages {
            state.messages[state.messages.len() - max_messages..].to_vec()
        } else {
            state.messages.clone()
        };

        let message_ids: Vec<MessageId> = slice_messages.iter().map(|m| m.id.clone()).collect();

        // Emit ContextSliceBuilt.
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.requested_by.clone()),
            now,
            ConversationEvent::ContextSliceBuilt {
                slice_id: slice_id.clone(),
                trigger_message_id: cmd.trigger_message_id.clone(),
                message_ids,
            },
        )
        .await?;

        // Emit AgentRunRequested.
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.requested_by.clone()),
            now,
            ConversationEvent::AgentRunRequested {
                run_id: run_id.clone(),
                trigger_message_id: cmd.trigger_message_id,
                context_slice_id: slice_id,
            },
        )
        .await?;

        Ok(run_id)
    }

    /// Mark an agent run as started.
    ///
    /// Validates: conversation exists, run exists, run is not terminal.
    /// Produces: `AgentRunStarted`.
    pub async fn start_agent_run(&self, cmd: StartAgentRunCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_agent_run_transition(&state, &cmd.conversation_id, &cmd.run_id)?;

        if !state.participants.contains_key(&cmd.started_by) {
            bail!("started_by is not a participant: {}", cmd.started_by);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.started_by),
            now,
            ConversationEvent::AgentRunStarted { run_id: cmd.run_id },
        )
        .await
    }

    /// Mark an agent run as completed.
    ///
    /// Validates: conversation exists, output message exists, run not already terminal.
    /// Produces: `AgentRunCompleted`.
    pub async fn complete_agent_run(&self, cmd: CompleteAgentRunCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_agent_run_transition(&state, &cmd.conversation_id, &cmd.run_id)?;

        if !state.messages_by_id.contains_key(&cmd.output_message_id) {
            bail!("output message not found: {}", cmd.output_message_id);
        }

        if !state.participants.contains_key(&cmd.completed_by) {
            bail!("completed_by is not a participant: {}", cmd.completed_by);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.completed_by),
            now,
            ConversationEvent::AgentRunCompleted {
                run_id: cmd.run_id,
                output_message_id: cmd.output_message_id,
            },
        )
        .await
    }

    /// Mark an agent run as failed.
    ///
    /// Validates: conversation exists, run exists, run is not terminal.
    /// Produces: `AgentRunFailed`.
    pub async fn fail_agent_run(&self, cmd: FailAgentRunCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_agent_run_transition(&state, &cmd.conversation_id, &cmd.run_id)?;

        if !state.participants.contains_key(&cmd.failed_by) {
            bail!("failed_by is not a participant: {}", cmd.failed_by);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.failed_by),
            now,
            ConversationEvent::AgentRunFailed {
                run_id: cmd.run_id,
                error_code: cmd.error_code,
                error_message: cmd.error_message,
            },
        )
        .await
    }

    /// Mark an agent run as cancelled.
    ///
    /// Validates: conversation exists, run exists, run is not terminal.
    /// Produces: `AgentRunCancelled`.
    pub async fn cancel_agent_run(&self, cmd: CancelAgentRunCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_agent_run_transition(&state, &cmd.conversation_id, &cmd.run_id)?;

        if !state.participants.contains_key(&cmd.cancelled_by) {
            bail!("cancelled_by is not a participant: {}", cmd.cancelled_by);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            Some(cmd.cancelled_by),
            now,
            ConversationEvent::AgentRunCancelled {
                run_id: cmd.run_id,
                reason: cmd.reason,
            },
        )
        .await
    }

    /// Mark an agent run as timed out.
    ///
    /// Validates: conversation exists, run exists, run is not terminal.
    /// Produces: `AgentRunTimedOut`.
    pub async fn timeout_agent_run(&self, cmd: TimeoutAgentRunCommand) -> Result<()> {
        let state = self.load_state(&cmd.conversation_id).await?;
        self.validate_agent_run_transition(&state, &cmd.conversation_id, &cmd.run_id)?;

        if let Some(actor) = &cmd.timed_out_by
            && !state.participants.contains_key(actor)
        {
            bail!("timed_out_by is not a participant: {}", actor);
        }

        let now = self.clock.now();
        self.append_envelope(
            &cmd.conversation_id,
            cmd.timed_out_by,
            now,
            ConversationEvent::AgentRunTimedOut { run_id: cmd.run_id },
        )
        .await
    }

    fn validate_conversation_exists(
        &self,
        state: &ConversationState,
        conversation_id: &ConversationId,
    ) -> Result<()> {
        if state.session.is_none() {
            bail!("conversation not found: {}", conversation_id);
        }
        Ok(())
    }

    fn validate_optional_actor(
        state: &ConversationState,
        actor_id: Option<&ParticipantId>,
        field_name: &str,
    ) -> Result<()> {
        if let Some(actor_id) = actor_id
            && !state.participants.contains_key(actor_id)
        {
            bail!("{field_name} is not a participant: {actor_id}");
        }
        Ok(())
    }

    fn validate_visibility(state: &ConversationState, visibility: &Visibility) -> Result<()> {
        match visibility {
            Visibility::Conversation => Ok(()),
            Visibility::PrivateToUser { user_id } => {
                if !state.participants.contains_key(user_id) {
                    bail!("visibility user is not a participant: {user_id}");
                }
                Ok(())
            }
            Visibility::AgentOnly => {
                if !state
                    .participants
                    .values()
                    .any(|participant| participant.kind == ParticipantKind::Agent)
                {
                    bail!("agent-only visibility requires an agent participant");
                }
                Ok(())
            }
            Visibility::Participants { participant_ids } => {
                for participant_id in participant_ids {
                    if !state.participants.contains_key(participant_id) {
                        bail!("visibility participant is not a participant: {participant_id}");
                    }
                }
                Ok(())
            }
        }
    }

    fn validate_agent_run_transition(
        &self,
        state: &ConversationState,
        conversation_id: &ConversationId,
        run_id: &str,
    ) -> Result<()> {
        if state.session.is_none() {
            bail!("conversation not found: {}", conversation_id);
        }

        let run = state
            .agent_runs
            .get(run_id)
            .with_context(|| format!("agent run not found: {run_id}"))?;

        match run.status {
            AgentRunStatus::Completed
            | AgentRunStatus::Failed
            | AgentRunStatus::Cancelled
            | AgentRunStatus::TimedOut => {
                bail!("agent run already terminal: {run_id}");
            }
            AgentRunStatus::Requested | AgentRunStatus::Started => Ok(()),
        }
    }

    /// Helper to create and append an event envelope.
    async fn append_envelope(
        &self,
        conversation_id: &ConversationId,
        actor_id: Option<ParticipantId>,
        occurred_at: DateTime<Utc>,
        event: ConversationEvent,
    ) -> Result<()> {
        let envelope = ConversationEventEnvelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            event_id: EventId(self.id_gen.new_id()),
            conversation_id: conversation_id.clone(),
            occurred_at,
            actor_id,
            event,
        };
        self.journal.append(envelope).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conversation_journal::MemoryConversationJournal;

    /// A deterministic ID generator for testing.
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

    /// A fixed clock for testing.
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

    fn test_kernel() -> ConversationKernel {
        let journal = Arc::new(MemoryConversationJournal::new());
        let id_gen = Arc::new(SequentialIdGenerator::new());
        let clock = Arc::new(FixedClock::new("2026-01-01T00:00:00Z".parse().unwrap()));
        ConversationKernel::with_generators(journal, id_gen, clock)
    }

    fn human(id: &str, name: &str) -> Participant {
        Participant {
            id: ParticipantId::from(id),
            kind: ParticipantKind::Human,
            display_name: name.to_string(),
        }
    }

    fn agent(id: &str, name: &str) -> Participant {
        Participant {
            id: ParticipantId::from(id),
            kind: ParticipantKind::Agent,
            display_name: name.to_string(),
        }
    }

    // --- create_conversation tests ---

    #[tokio::test]
    async fn create_conversation_produces_events() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let events = kernel.load_events(&conv_id).await.unwrap();
        // 1 ConversationCreated + 2 ParticipantAdded
        assert_eq!(events.len(), 3);

        assert!(matches!(
            &events[0].event,
            ConversationEvent::ConversationCreated { .. }
        ));
        assert!(matches!(
            &events[1].event,
            ConversationEvent::ParticipantAdded { .. }
        ));
        assert!(matches!(
            &events[2].event,
            ConversationEvent::ParticipantAdded { .. }
        ));
    }

    #[tokio::test]
    async fn create_conversation_with_title() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Group,
                title: Some("Design Discussion".to_string()),
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let state = kernel.load_state(&conv_id).await.unwrap();
        let session = state.session.unwrap();
        assert_eq!(session.title, Some("Design Discussion".to_string()));
        assert_eq!(session.kind, ConversationKind::Group);
    }

    #[tokio::test]
    async fn create_conversation_rejects_empty_participants() {
        let kernel = test_kernel();

        let result = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![],
                actor_id: None,
            })
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("at least one participant")
        );
    }

    // --- append_message tests ---

    #[tokio::test]
    async fn append_message_produces_event() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let msg_id = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "Hello!".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].id, msg_id);

        match &state.messages[0].content {
            MessageContent::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("expected Text content"),
        }
    }

    #[tokio::test]
    async fn append_message_rejects_unknown_sender() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let result = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id,
                sender_id: ParticipantId::from("ghost"),
                content: MessageContent::Text {
                    text: "I don't exist".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not a participant")
        );
    }

    #[tokio::test]
    async fn append_message_rejects_nonexistent_conversation() {
        let kernel = test_kernel();

        let result = kernel
            .append_message(AppendMessageCommand {
                conversation_id: ConversationId::from("nonexistent"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "hello".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("conversation not found")
        );
    }

    #[tokio::test]
    async fn append_multiple_messages_preserves_order() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "first".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("a1"),
                content: MessageContent::Text {
                    text: "second".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "third".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_eq!(state.messages.len(), 3);
        match &state.messages[0].content {
            MessageContent::Text { text } => assert_eq!(text, "first"),
            _ => panic!(),
        }
        match &state.messages[2].content {
            MessageContent::Text { text } => assert_eq!(text, "third"),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn append_message_rejects_system_sender() {
        let kernel = test_kernel();

        // Create conversation with Human + Agent + System.
        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Group,
                title: None,
                participants: vec![
                    human("u1", "诗闻"),
                    agent("a1", "小助理"),
                    Participant {
                        id: ParticipantId::from("sys1"),
                        kind: ParticipantKind::System,
                        display_name: "System".to_string(),
                    },
                ],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // System participant tries to send a regular message.
        let result = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id,
                sender_id: ParticipantId::from("sys1"),
                content: MessageContent::Text {
                    text: "I am system".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not a foreground participant")
        );
    }

    // --- create_assistant_suggestion tests ---

    #[tokio::test]
    async fn assistant_suggestion_is_private_to_user() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Group,
                title: Some("Team Chat".to_string()),
                participants: vec![
                    human("u1", "诗闻"),
                    human("u2", "Other"),
                    agent("a1", "小助理"),
                ],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let msg_id = kernel
            .create_assistant_suggestion(CreateAssistantSuggestionCommand {
                conversation_id: conv_id.clone(),
                target_user_id: ParticipantId::from("u1"),
                text: "这句话可能有讽刺意味".to_string(),
                actions: vec![],
                trigger: SuggestionTrigger::Mention,
            })
            .await
            .unwrap();

        let events = kernel.load_events(&conv_id).await.unwrap();
        let suggestion_event = events
            .iter()
            .find(|e| {
                matches!(
                    &e.event,
                    ConversationEvent::AssistantSuggestionCreated { .. }
                )
            })
            .expect("suggestion event not found");

        match &suggestion_event.event {
            ConversationEvent::AssistantSuggestionCreated {
                suggestion_message, ..
            } => {
                assert_eq!(suggestion_message.id, msg_id);
                assert!(matches!(
                    &suggestion_message.visibility,
                    Visibility::PrivateToUser { user_id }
                        if *user_id == ParticipantId::from("u1")
                ));
            }
            _ => panic!("wrong event type"),
        }
    }

    #[tokio::test]
    async fn assistant_suggestion_rejects_unknown_target() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::Group,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let result = kernel
            .create_assistant_suggestion(CreateAssistantSuggestionCommand {
                conversation_id: conv_id,
                target_user_id: ParticipantId::from("nonexistent"),
                text: "hello".to_string(),
                actions: vec![],
                trigger: SuggestionTrigger::Proactive,
            })
            .await;

        assert!(result.is_err());
    }

    // --- request_agent_run tests ---

    #[tokio::test]
    async fn request_agent_run_produces_boundary_events() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: Some("Research Task".to_string()),
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // Add some messages.
        let msg1 = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "帮我分析这个系统".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let msg2 = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("a1"),
                content: MessageContent::Text {
                    text: "好的，让我来分析".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let run_id = kernel
            .request_agent_run(RequestAgentRunCommand {
                conversation_id: conv_id.clone(),
                trigger_message_id: msg1.clone(),
                requested_by: ParticipantId::from("u1"),
            })
            .await
            .unwrap();

        assert!(!run_id.is_empty());

        let events = kernel.load_events(&conv_id).await.unwrap();
        let last_two: Vec<_> = events.iter().rev().take(2).collect();

        // Second-to-last: ContextSliceBuilt
        assert!(matches!(
            &last_two[1].event,
            ConversationEvent::ContextSliceBuilt { .. }
        ));

        // Last: AgentRunRequested
        assert!(matches!(
            &last_two[0].event,
            ConversationEvent::AgentRunRequested { .. }
        ));
    }

    // --- Full integration test ---

    #[tokio::test]
    async fn full_conversation_lifecycle() {
        let kernel = test_kernel();

        // 1. Create conversation
        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: Some("Design Kernel".to_string()),
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // 2. User sends a message
        let user_msg = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "帮我设计 Conversation Kernel".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        // 3. Agent replies
        kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("a1"),
                content: MessageContent::Text {
                    text: "好的，我来帮你设计".to_string(),
                },
                reply_to: Some(user_msg.clone()),
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        // 4. Verify state
        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.participants.len(), 2);
        assert!(state.session.is_some());
        assert_eq!(
            state.session.as_ref().unwrap().kind,
            ConversationKind::AgentTask
        );

        // 5. Request agent run
        let run_id = kernel
            .request_agent_run(RequestAgentRunCommand {
                conversation_id: conv_id.clone(),
                trigger_message_id: user_msg,
                requested_by: ParticipantId::from("u1"),
            })
            .await
            .unwrap();

        assert!(!run_id.is_empty());

        // 6. Verify all events
        let events = kernel.load_events(&conv_id).await.unwrap();
        // 1 Created + 2 ParticipantAdded + 2 MessageAppended + 1 SliceBuilt + 1 RunRequested = 7
        assert_eq!(events.len(), 7);
    }
}
