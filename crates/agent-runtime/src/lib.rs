//! # Agent Runtime
//!
//! Bridges the conversation kernel with the model adapter to process agent runs.
//!
//! ## Flow
//!
//! ```text
//! AgentRunRequested
//!   → AgentRunProcessor::process()
//!     → AgentContextBuilder::build(state, messages)
//!     → PromptRenderer::render(context)
//!     → ModelAdapter::complete(request)
//!     → kernel.append_message(assistant_response)
//!     → kernel.complete_agent_run(run_id)
//! ```

use action_core::{ActionId, ActionKind, ActionRequest};
use action_runtime::{ActionRuntime, ActionRuntimeOutcome, ProcessActionRequest};
use anyhow::{Context, Result};
use conversation_core::*;
use conversation_kernel::{ConversationKernel, ConversationState};
use entity_core::EntityDescriptor;
use model_adapter::*;
use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────────────
// Configuration
// ────────────────────────────────────────────────────────────────────────────

/// Configuration for the agent runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimeConfig {
    /// Default model to use when no model is specified in the run request.
    pub default_model_id: ModelId,
    /// Maximum number of context messages to include in a prompt.
    pub max_context_messages: usize,
    /// Optional system prompt prepended to every model request.
    pub system_prompt: Option<String>,
}

impl Default for AgentRuntimeConfig {
    fn default() -> Self {
        Self {
            default_model_id: ModelId::from("fake/default"),
            max_context_messages: 50,
            system_prompt: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Context Builder
// ────────────────────────────────────────────────────────────────────────────

/// Resolved context ready for prompt rendering.
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// The conversation ID.
    pub conversation_id: ConversationId,
    /// The agent run ID.
    pub run_id: String,
    /// The trigger message that initiated this run.
    pub trigger_message: Message,
    /// Ordered messages to include in the prompt (may be truncated).
    pub messages: Vec<Message>,
    /// Linked entity descriptors available for this conversation.
    pub linked_entities: Vec<EntityDescriptor>,
    /// The model to use for this run.
    pub model_id: ModelId,
    /// Optional system prompt.
    pub system_prompt: Option<String>,
}

/// Builds an `AgentContext` from a projected conversation state.
pub struct AgentContextBuilder {
    max_context_messages: usize,
}

impl AgentContextBuilder {
    pub fn new(max_context_messages: usize) -> Self {
        Self {
            max_context_messages,
        }
    }

    /// Build context from conversation state, selecting relevant messages.
    pub fn build(
        &self,
        state: &ConversationState,
        run_id: &str,
        trigger_message_id: &MessageId,
        config: &AgentRuntimeConfig,
    ) -> Result<AgentContext> {
        let session = state
            .session
            .as_ref()
            .context("conversation has no session")?;

        // Find the trigger message.
        let trigger_message = state
            .messages_by_id
            .get(trigger_message_id)
            .context("trigger message not found")?
            .clone();

        // Select messages: take up to max_context_messages ending at trigger.
        let trigger_index = state
            .messages
            .iter()
            .position(|m| m.id == *trigger_message_id)
            .context("trigger message not in ordered list")?;

        let start = trigger_index.saturating_sub(self.max_context_messages - 1);
        let messages = state.messages[start..=trigger_index].to_vec();

        // Collect linked entities.
        let linked_entities: Vec<EntityDescriptor> =
            state.linked_entities.values().cloned().collect();

        Ok(AgentContext {
            conversation_id: session.id.clone(),
            run_id: run_id.to_string(),
            trigger_message,
            messages,
            linked_entities,
            model_id: config.default_model_id.clone(),
            system_prompt: config.system_prompt.clone(),
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Prompt Renderer
// ────────────────────────────────────────────────────────────────────────────

/// Renders an `AgentContext` into a `ModelRequest` for the model adapter.
pub struct PromptRenderer;

impl PromptRenderer {
    /// Render context into a model request.
    pub fn render(context: &AgentContext) -> ModelRequest {
        let mut messages = Vec::new();

        // Prepend system prompt if configured.
        if let Some(system_prompt) = &context.system_prompt {
            messages.push(ModelMessage::system(system_prompt));
        }

        // Convert conversation messages to model messages.
        for msg in &context.messages {
            let role = match &msg.sender_id {
                id if id.0.starts_with("agent") || id.0.starts_with("a") => {
                    // Heuristic: sender IDs starting with "agent" or "a" are assistant.
                    // In production, this should be resolved via participant lookup.
                    ModelRole::Assistant
                }
                _ => ModelRole::User,
            };

            let text = match &msg.content {
                MessageContent::Text { text } => text.clone(),
                MessageContent::SystemNotice { text, .. } => format!("[System] {text}"),
                MessageContent::AgentSuggestion { text, .. } => {
                    format!("[Suggestion] {text}")
                }
            };

            messages.push(ModelMessage { role, text });
        }

        ModelRequest::new(context.model_id.clone(), messages)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Action Proposal Detection
// ────────────────────────────────────────────────────────────────────────────

/// Deterministic action proposal extracted from model output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentActionProposal {
    pub action_id: ActionId,
    pub action_kind: ActionKind,
    pub input: serde_json::Value,
    pub summary: String,
}

/// Detects whether a model response proposes an action.
pub trait ActionProposalDetector: Send + Sync {
    fn detect(
        &self,
        context: &AgentContext,
        model_response: &ModelResponse,
    ) -> Option<AgentActionProposal>;
}

/// Detector that never proposes actions. Preserves text-only behavior.
#[derive(Debug, Default)]
pub struct NoopActionProposalDetector;

impl ActionProposalDetector for NoopActionProposalDetector {
    fn detect(
        &self,
        _context: &AgentContext,
        _model_response: &ModelResponse,
    ) -> Option<AgentActionProposal> {
        None
    }
}

/// Deterministic fake detector for tests and early action integration.
///
/// It recognizes response markers like:
///
/// ```text
/// ACTION knowledge.search {"query":"agent os"}
/// ACTION knowledge.save_entry {"title":"AgentOS"}
/// ACTION mail.send {"to":"user@example.com"}
/// ```
#[derive(Debug, Default)]
pub struct KeywordActionProposalDetector;

impl ActionProposalDetector for KeywordActionProposalDetector {
    fn detect(
        &self,
        _context: &AgentContext,
        model_response: &ModelResponse,
    ) -> Option<AgentActionProposal> {
        let marker = "ACTION ";
        let start = model_response.text.find(marker)? + marker.len();
        let rest = model_response.text[start..].trim();
        let mut parts = rest.splitn(2, char::is_whitespace);
        let kind = parts.next()?.trim();
        if kind.is_empty() {
            return None;
        }
        let input_text = parts.next().unwrap_or("{}").trim();
        let input = if input_text.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(input_text).unwrap_or_else(|_| {
                serde_json::json!({
                    "raw": input_text,
                })
            })
        };
        Some(AgentActionProposal {
            action_id: ActionId::from(format!("action-{}", uuid::Uuid::new_v4())),
            action_kind: ActionKind::from(kind),
            input,
            summary: format!("Proposed action {kind}"),
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Agent Run Processor
// ────────────────────────────────────────────────────────────────────────────

/// Result of processing a single agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRunOutcome {
    /// Run completed successfully.
    Completed {
        run_id: String,
        output_message_id: MessageId,
        response_text: String,
    },
    /// Run failed.
    Failed {
        run_id: String,
        error_code: String,
        error_message: String,
    },
}

/// Result of processing a run with optional action proposal handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRunWithActionsOutcome {
    /// No action was detected; normal text path completed.
    CompletedText {
        run_id: String,
        output_message_id: MessageId,
        response_text: String,
    },
    /// An action was detected and processed through action-runtime.
    CompletedWithAction {
        run_id: String,
        output_message_id: MessageId,
        response_text: String,
        action_outcome: ActionRuntimeOutcome,
    },
    /// Run failed.
    Failed {
        run_id: String,
        error_code: String,
        error_message: String,
    },
}

/// Request to process a run with optional action integration.
pub struct ProcessRunWithActionsRequest<'a> {
    pub kernel: &'a ConversationKernel,
    pub adapter: &'a dyn ModelAdapter,
    pub context_builder: &'a AgentContextBuilder,
    pub config: &'a AgentRuntimeConfig,
    pub conversation_id: &'a ConversationId,
    pub run_id: &'a str,
    pub trigger_message_id: &'a MessageId,
    pub agent_participant_id: &'a ParticipantId,
    pub detector: &'a dyn ActionProposalDetector,
    pub action_runtime: &'a ActionRuntime<'a>,
}

/// Request to process a single agent run.
pub struct ProcessRunRequest<'a> {
    pub kernel: &'a ConversationKernel,
    pub adapter: &'a dyn ModelAdapter,
    pub context_builder: &'a AgentContextBuilder,
    pub config: &'a AgentRuntimeConfig,
    pub conversation_id: &'a ConversationId,
    pub run_id: &'a str,
    pub trigger_message_id: &'a MessageId,
    pub agent_participant_id: &'a ParticipantId,
}

/// Processes a single agent run: build context → call model → write response.
pub struct AgentRunProcessor;

impl AgentRunProcessor {
    /// Process a pending agent run.
    ///
    /// 1. Start the run (emit AgentRunStarted)
    /// 2. Build context from conversation state
    /// 3. Render prompt
    /// 4. Call model adapter
    /// 5. Append assistant message
    /// 6. Complete the run (emit AgentRunCompleted)
    pub async fn process(req: ProcessRunRequest<'_>) -> Result<AgentRunOutcome> {
        let ProcessRunRequest {
            kernel,
            adapter,
            context_builder,
            config,
            conversation_id,
            run_id,
            trigger_message_id,
            agent_participant_id,
        } = req;
        // Step 1: Mark run as started.
        kernel
            .start_agent_run(conversation_kernel::StartAgentRunCommand {
                conversation_id: conversation_id.clone(),
                run_id: run_id.to_string(),
                started_by: agent_participant_id.clone(),
            })
            .await?;

        // Step 2: Load state and build context.
        let state = kernel.load_state(conversation_id).await?;
        let context = match context_builder.build(&state, run_id, trigger_message_id, config) {
            Ok(ctx) => ctx,
            Err(e) => {
                // Context build failed — fail the run.
                kernel
                    .fail_agent_run(conversation_kernel::FailAgentRunCommand {
                        conversation_id: conversation_id.clone(),
                        run_id: run_id.to_string(),
                        error_code: "context_build_failed".to_string(),
                        error_message: e.to_string(),
                        failed_by: agent_participant_id.clone(),
                    })
                    .await?;
                return Ok(AgentRunOutcome::Failed {
                    run_id: run_id.to_string(),
                    error_code: "context_build_failed".to_string(),
                    error_message: e.to_string(),
                });
            }
        };

        // Step 3: Render prompt.
        let request = PromptRenderer::render(&context);

        // Step 4: Call model adapter.
        let response = match adapter.complete(request).await {
            Ok(resp) => resp,
            Err(e) => {
                // Model call failed — fail the run.
                kernel
                    .fail_agent_run(conversation_kernel::FailAgentRunCommand {
                        conversation_id: conversation_id.clone(),
                        run_id: run_id.to_string(),
                        error_code: "model_call_failed".to_string(),
                        error_message: e.to_string(),
                        failed_by: agent_participant_id.clone(),
                    })
                    .await?;
                return Ok(AgentRunOutcome::Failed {
                    run_id: run_id.to_string(),
                    error_code: "model_call_failed".to_string(),
                    error_message: e.to_string(),
                });
            }
        };

        // Step 5: Append assistant message.
        let output_message_id = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conversation_id.clone(),
                sender_id: agent_participant_id.clone(),
                content: MessageContent::Text {
                    text: response.text.clone(),
                },
                reply_to: Some(trigger_message_id.clone()),
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await?;

        // Step 6: Complete the run.
        kernel
            .complete_agent_run(conversation_kernel::CompleteAgentRunCommand {
                conversation_id: conversation_id.clone(),
                run_id: run_id.to_string(),
                output_message_id: output_message_id.clone(),
                completed_by: agent_participant_id.clone(),
            })
            .await?;

        Ok(AgentRunOutcome::Completed {
            run_id: run_id.to_string(),
            output_message_id,
            response_text: response.text,
        })
    }

    /// Process a pending run and route deterministic action proposals through action-runtime.
    pub async fn process_with_actions(
        req: ProcessRunWithActionsRequest<'_>,
    ) -> Result<AgentRunWithActionsOutcome> {
        let ProcessRunWithActionsRequest {
            kernel,
            adapter,
            context_builder,
            config,
            conversation_id,
            run_id,
            trigger_message_id,
            agent_participant_id,
            detector,
            action_runtime,
        } = req;

        kernel
            .start_agent_run(conversation_kernel::StartAgentRunCommand {
                conversation_id: conversation_id.clone(),
                run_id: run_id.to_string(),
                started_by: agent_participant_id.clone(),
            })
            .await?;

        let state = kernel.load_state(conversation_id).await?;
        let context = match context_builder.build(&state, run_id, trigger_message_id, config) {
            Ok(ctx) => ctx,
            Err(e) => {
                kernel
                    .fail_agent_run(conversation_kernel::FailAgentRunCommand {
                        conversation_id: conversation_id.clone(),
                        run_id: run_id.to_string(),
                        error_code: "context_build_failed".to_string(),
                        error_message: e.to_string(),
                        failed_by: agent_participant_id.clone(),
                    })
                    .await?;
                return Ok(AgentRunWithActionsOutcome::Failed {
                    run_id: run_id.to_string(),
                    error_code: "context_build_failed".to_string(),
                    error_message: e.to_string(),
                });
            }
        };

        let request = PromptRenderer::render(&context);
        let response = match adapter.complete(request).await {
            Ok(resp) => resp,
            Err(e) => {
                kernel
                    .fail_agent_run(conversation_kernel::FailAgentRunCommand {
                        conversation_id: conversation_id.clone(),
                        run_id: run_id.to_string(),
                        error_code: "model_call_failed".to_string(),
                        error_message: e.to_string(),
                        failed_by: agent_participant_id.clone(),
                    })
                    .await?;
                return Ok(AgentRunWithActionsOutcome::Failed {
                    run_id: run_id.to_string(),
                    error_code: "model_call_failed".to_string(),
                    error_message: e.to_string(),
                });
            }
        };

        let action_outcome = if let Some(proposal) = detector.detect(&context, &response) {
            let action_request = ActionRequest {
                action_id: proposal.action_id,
                action_kind: proposal.action_kind,
                input: proposal.input,
                requested_by: agent_participant_id.to_string(),
                conversation_id: Some(conversation_id.to_string()),
                message_id: Some(trigger_message_id.to_string()),
                requested_at: chrono::Utc::now(),
            };
            Some(
                action_runtime
                    .process(ProcessActionRequest {
                        conversation_id,
                        action_request,
                        requested_by: Some(agent_participant_id.clone()),
                        runtime_actor: Some(agent_participant_id.clone()),
                    })
                    .await?,
            )
        } else {
            None
        };

        let response_text = match &action_outcome {
            Some(outcome) => format!(
                "{}

[Action outcome] {}",
                response.text,
                summarize_action_outcome(outcome)
            ),
            None => response.text.clone(),
        };

        let output_message_id = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conversation_id.clone(),
                sender_id: agent_participant_id.clone(),
                content: MessageContent::Text {
                    text: response_text.clone(),
                },
                reply_to: Some(trigger_message_id.clone()),
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await?;

        kernel
            .complete_agent_run(conversation_kernel::CompleteAgentRunCommand {
                conversation_id: conversation_id.clone(),
                run_id: run_id.to_string(),
                output_message_id: output_message_id.clone(),
                completed_by: agent_participant_id.clone(),
            })
            .await?;

        if let Some(action_outcome) = action_outcome {
            Ok(AgentRunWithActionsOutcome::CompletedWithAction {
                run_id: run_id.to_string(),
                output_message_id,
                response_text,
                action_outcome,
            })
        } else {
            Ok(AgentRunWithActionsOutcome::CompletedText {
                run_id: run_id.to_string(),
                output_message_id,
                response_text,
            })
        }
    }
}

fn summarize_action_outcome(outcome: &ActionRuntimeOutcome) -> String {
    match outcome {
        ActionRuntimeOutcome::Completed { action_id, result } => {
            format!("action {action_id} completed: {}", result.summary)
        }
        ActionRuntimeOutcome::ApprovalRequired { action_id, reason } => {
            format!("action {action_id} requires approval: {reason}")
        }
        ActionRuntimeOutcome::Denied { action_id, reason } => {
            format!("action {action_id} denied: {reason}")
        }
        ActionRuntimeOutcome::Failed {
            action_id,
            error_message,
        } => format!("action {action_id} failed: {error_message}"),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Agent Runtime — high-level entry point
// ────────────────────────────────────────────────────────────────────────────

/// High-level agent runtime that processes all pending runs for a conversation.
pub struct AgentRuntime {
    kernel: ConversationKernel,
    adapter: Box<dyn ModelAdapter>,
    context_builder: AgentContextBuilder,
    config: AgentRuntimeConfig,
    agent_participant_id: ParticipantId,
}

impl AgentRuntime {
    pub fn new(
        kernel: ConversationKernel,
        adapter: Box<dyn ModelAdapter>,
        config: AgentRuntimeConfig,
        agent_participant_id: ParticipantId,
    ) -> Self {
        let context_builder = AgentContextBuilder::new(config.max_context_messages);
        Self {
            kernel,
            adapter,
            context_builder,
            config,
            agent_participant_id,
        }
    }

    /// Process a single agent run. Returns the outcome.
    pub async fn process_run(
        &self,
        conversation_id: &ConversationId,
        run_id: &str,
        trigger_message_id: &MessageId,
    ) -> Result<AgentRunOutcome> {
        AgentRunProcessor::process(ProcessRunRequest {
            kernel: &self.kernel,
            adapter: self.adapter.as_ref(),
            context_builder: &self.context_builder,
            config: &self.config,
            conversation_id,
            run_id,
            trigger_message_id,
            agent_participant_id: &self.agent_participant_id,
        })
        .await
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use audit_log::AuditLog;
    use conversation_journal::MemoryConversationJournal;
    use conversation_kernel::{Clock, IdGenerator};
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
        time: chrono::DateTime<chrono::Utc>,
    }

    impl FixedClock {
        fn new(time: chrono::DateTime<chrono::Utc>) -> Self {
            Self { time }
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
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

    fn agent_participant(id: &str, name: &str) -> Participant {
        Participant {
            id: ParticipantId::from(id),
            kind: ParticipantKind::Agent,
            display_name: name.to_string(),
        }
    }

    // ── Context Builder tests ──────────────────────────────────────────

    #[tokio::test]
    async fn context_builder_selects_messages_up_to_trigger() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // Append 5 messages.
        let mut msg_ids = Vec::new();
        for i in 0..5 {
            let sender = if i % 2 == 0 { "u1" } else { "a1" };
            let id = kernel
                .append_message(conversation_kernel::AppendMessageCommand {
                    conversation_id: conv_id.clone(),
                    sender_id: ParticipantId::from(sender),
                    content: MessageContent::Text {
                        text: format!("message {i}"),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                })
                .await
                .unwrap();
            msg_ids.push(id);
        }

        let state = kernel.load_state(&conv_id).await.unwrap();
        let builder = AgentContextBuilder::new(10);
        let config = AgentRuntimeConfig::default();

        let context = builder
            .build(&state, "run-1", &msg_ids[3], &config)
            .unwrap();

        // Should include messages 0..=3 (4 messages), respecting max_context_messages.
        assert_eq!(context.messages.len(), 4);
        assert_eq!(context.trigger_message.id, msg_ids[3]);
        assert_eq!(context.conversation_id, conv_id);
    }

    #[tokio::test]
    async fn context_builder_truncates_to_max_messages() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // Append 10 messages.
        let mut msg_ids = Vec::new();
        for i in 0..10 {
            let sender = if i % 2 == 0 { "u1" } else { "a1" };
            let id = kernel
                .append_message(conversation_kernel::AppendMessageCommand {
                    conversation_id: conv_id.clone(),
                    sender_id: ParticipantId::from(sender),
                    content: MessageContent::Text {
                        text: format!("message {i}"),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                })
                .await
                .unwrap();
            msg_ids.push(id);
        }

        let state = kernel.load_state(&conv_id).await.unwrap();
        let builder = AgentContextBuilder::new(3); // Only 3 messages max.
        let config = AgentRuntimeConfig::default();

        let context = builder
            .build(&state, "run-1", &msg_ids[9], &config)
            .unwrap();

        // Should include only the last 3 messages (7, 8, 9).
        assert_eq!(context.messages.len(), 3);
    }

    #[tokio::test]
    async fn context_builder_fails_for_missing_trigger() {
        let kernel = test_kernel();

        let conv_id = kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let state = kernel.load_state(&conv_id).await.unwrap();
        let builder = AgentContextBuilder::new(10);
        let config = AgentRuntimeConfig::default();

        let result = builder.build(&state, "run-1", &MessageId::from("nonexistent"), &config);
        assert!(result.is_err());
    }

    // ── Prompt Renderer tests ──────────────────────────────────────────

    #[tokio::test]
    async fn prompt_renderer_converts_messages_to_model_roles() {
        let context = AgentContext {
            conversation_id: ConversationId::from("conv-1"),
            run_id: "run-1".to_string(),
            trigger_message: Message {
                id: MessageId::from("msg-1"),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "hello".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
                created_at: chrono::Utc::now(),
                edited_at: None,
            },
            messages: vec![
                Message {
                    id: MessageId::from("msg-1"),
                    conversation_id: ConversationId::from("conv-1"),
                    sender_id: ParticipantId::from("u1"),
                    content: MessageContent::Text {
                        text: "帮我总结".to_string(),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                    created_at: chrono::Utc::now(),
                    edited_at: None,
                },
                Message {
                    id: MessageId::from("msg-2"),
                    conversation_id: ConversationId::from("conv-1"),
                    sender_id: ParticipantId::from("a1"),
                    content: MessageContent::Text {
                        text: "好的".to_string(),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                    created_at: chrono::Utc::now(),
                    edited_at: None,
                },
            ],
            linked_entities: vec![],
            model_id: ModelId::from("test-model"),
            system_prompt: Some("You are helpful.".to_string()),
        };

        let request = PromptRenderer::render(&context);

        assert_eq!(request.model_id, ModelId::from("test-model"));
        // System prompt + 2 messages = 3.
        assert_eq!(request.messages.len(), 3);
        assert_eq!(request.messages[0].role, ModelRole::System);
        assert_eq!(request.messages[0].text, "You are helpful.");
        assert_eq!(request.messages[1].role, ModelRole::User);
        assert_eq!(request.messages[1].text, "帮我总结");
        assert_eq!(request.messages[2].role, ModelRole::Assistant);
        assert_eq!(request.messages[2].text, "好的");
    }

    #[tokio::test]
    async fn prompt_renderer_no_system_prompt() {
        let context = AgentContext {
            conversation_id: ConversationId::from("conv-1"),
            run_id: "run-1".to_string(),
            trigger_message: Message {
                id: MessageId::from("msg-1"),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "hello".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
                created_at: chrono::Utc::now(),
                edited_at: None,
            },
            messages: vec![Message {
                id: MessageId::from("msg-1"),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "hello".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
                created_at: chrono::Utc::now(),
                edited_at: None,
            }],
            linked_entities: vec![],
            model_id: ModelId::from("test-model"),
            system_prompt: None,
        };

        let request = PromptRenderer::render(&context);
        // No system prompt, just 1 message.
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, ModelRole::User);
    }

    // ── AgentRunProcessor E2E tests ────────────────────────────────────

    #[tokio::test]
    async fn e2e_user_message_to_assistant_response() {
        let kernel = test_kernel();
        let adapter = FakeModelAdapter::new("Assistant reply");

        // Create conversation.
        let conv_id = kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: Some("Test Task".to_string()),
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // User sends a message.
        let user_msg = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "帮我总结这段话".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        // Request agent run.
        let run_id = kernel
            .request_agent_run(conversation_kernel::RequestAgentRunCommand {
                conversation_id: conv_id.clone(),
                trigger_message_id: user_msg.clone(),
                requested_by: ParticipantId::from("u1"),
            })
            .await
            .unwrap();

        // Process the run.
        let context_builder = AgentContextBuilder::new(50);
        let config = AgentRuntimeConfig::default();

        let outcome = AgentRunProcessor::process(ProcessRunRequest {
            kernel: &kernel,
            adapter: &adapter,
            context_builder: &context_builder,
            config: &config,
            conversation_id: &conv_id,
            run_id: &run_id,
            trigger_message_id: &user_msg,
            agent_participant_id: &ParticipantId::from("a1"),
        })
        .await
        .unwrap();

        // Verify outcome.
        match &outcome {
            AgentRunOutcome::Completed {
                run_id: rid,
                response_text,
                ..
            } => {
                assert_eq!(rid, &run_id);
                assert!(response_text.contains("Assistant reply"));
                assert!(response_text.contains("帮我总结这段话"));
            }
            AgentRunOutcome::Failed { .. } => panic!("expected completion"),
        }

        // Verify state.
        let state = kernel.load_state(&conv_id).await.unwrap();
        // 2 participants + user message + assistant message = 4 messages? No.
        // Messages: user_msg + assistant_msg = 2
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[1].sender_id, ParticipantId::from("a1"));

        // Verify agent run is completed.
        let run_state = state.agent_runs.get(&run_id).unwrap();
        assert!(matches!(run_state.status, AgentRunStatus::Completed { .. }));
    }

    #[tokio::test]
    async fn e2e_model_failure_records_failed_run() {
        let kernel = test_kernel();
        // Use an adapter that rejects empty requests.
        let _adapter = FakeModelAdapter::default();

        let conv_id = kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: None,
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        // Request agent run without any messages (empty context → model fails).
        // First append a message so request_agent_run has a trigger.
        let user_msg = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "hello".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let run_id = kernel
            .request_agent_run(conversation_kernel::RequestAgentRunCommand {
                conversation_id: conv_id.clone(),
                trigger_message_id: user_msg.clone(),
                requested_by: ParticipantId::from("u1"),
            })
            .await
            .unwrap();

        // Use a custom adapter that always fails.
        struct FailingAdapter;
        #[async_trait]
        impl ModelAdapter for FailingAdapter {
            async fn complete(
                &self,
                _request: ModelRequest,
            ) -> Result<ModelResponse, ModelAdapterError> {
                Err(ModelAdapterError::ExecutorFailed(
                    "simulated failure".to_string(),
                ))
            }
        }

        let context_builder = AgentContextBuilder::new(50);
        let config = AgentRuntimeConfig::default();

        let outcome = AgentRunProcessor::process(ProcessRunRequest {
            kernel: &kernel,
            adapter: &FailingAdapter,
            context_builder: &context_builder,
            config: &config,
            conversation_id: &conv_id,
            run_id: &run_id,
            trigger_message_id: &user_msg,
            agent_participant_id: &ParticipantId::from("a1"),
        })
        .await
        .unwrap();

        // Should be a failed outcome.
        match &outcome {
            AgentRunOutcome::Failed {
                error_code,
                error_message,
                ..
            } => {
                assert_eq!(error_code, "model_call_failed");
                assert!(error_message.contains("simulated failure"));
            }
            AgentRunOutcome::Completed { .. } => panic!("expected failure"),
        }

        // Verify agent run is failed in state.
        let state = kernel.load_state(&conv_id).await.unwrap();
        let run_state = state.agent_runs.get(&run_id).unwrap();
        assert!(matches!(run_state.status, AgentRunStatus::Failed { .. }));
    }

    #[tokio::test]
    async fn e2e_agent_runtime_high_level_entry() {
        let kernel = test_kernel();
        let adapter = FakeModelAdapter::new("High-level reply");
        let config = AgentRuntimeConfig {
            system_prompt: Some("You are a concise assistant.".to_string()),
            ..Default::default()
        };

        let runtime = AgentRuntime::new(
            kernel.clone(),
            Box::new(adapter),
            config,
            ParticipantId::from("a1"),
        );

        // Create conversation and message.
        let conv_id = runtime
            .kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: None,
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let user_msg = runtime
            .kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "你好".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();

        let run_id = runtime
            .kernel
            .request_agent_run(conversation_kernel::RequestAgentRunCommand {
                conversation_id: conv_id.clone(),
                trigger_message_id: user_msg.clone(),
                requested_by: ParticipantId::from("u1"),
            })
            .await
            .unwrap();

        let outcome = runtime
            .process_run(&conv_id, &run_id, &user_msg)
            .await
            .unwrap();

        assert!(matches!(outcome, AgentRunOutcome::Completed { .. }));
    }

    // ── AgentRunProcessor action integration tests ───────────────────────

    struct StaticAdapter {
        text: String,
    }

    #[async_trait]
    impl ModelAdapter for StaticAdapter {
        async fn complete(
            &self,
            request: ModelRequest,
        ) -> Result<ModelResponse, ModelAdapterError> {
            Ok(ModelResponse {
                text: self.text.clone(),
                usage: None,
                model_id: request.model_id,
            })
        }
    }

    fn action_registry() -> action_core::ActionRegistry {
        let mut registry = action_core::ActionRegistry::new();
        for (kind, side_effect) in [
            ("knowledge.search", action_core::SideEffectKind::ReadOnly),
            (
                "knowledge.save_entry",
                action_core::SideEffectKind::RuntimeStateMutation,
            ),
            (
                "mail.send",
                action_core::SideEffectKind::ExternalSystemMutation,
            ),
        ] {
            registry
                .register(action_core::ActionSchema {
                    kind: action_core::ActionKind::from(kind),
                    display_name: kind.to_string(),
                    description: kind.to_string(),
                    side_effect,
                    input_schema: None,
                    output_schema: None,
                })
                .unwrap();
        }
        registry
    }

    async fn setup_run(kernel: &ConversationKernel) -> (ConversationId, MessageId, String) {
        let conv_id = kernel
            .create_conversation(conversation_kernel::CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: Some("Action integration".to_string()),
                participants: vec![human("u1", "Test User"), agent_participant("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();
        let user_msg = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "please act".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();
        let run_id = kernel
            .request_agent_run(conversation_kernel::RequestAgentRunCommand {
                conversation_id: conv_id.clone(),
                trigger_message_id: user_msg.clone(),
                requested_by: ParticipantId::from("u1"),
            })
            .await
            .unwrap();
        (conv_id, user_msg, run_id)
    }

    async fn process_static_action_response(
        response_text: &str,
    ) -> (
        AgentRunWithActionsOutcome,
        ConversationKernel,
        ConversationId,
        audit_log::MemoryAuditSink,
    ) {
        let kernel = test_kernel();
        let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
        let adapter = StaticAdapter {
            text: response_text.to_string(),
        };
        let context_builder = AgentContextBuilder::new(50);
        let config = AgentRuntimeConfig::default();
        let registry = action_registry();
        let policy = capability_policy::CapabilityPolicy::default_safe();
        let executor = action_core::FakeActionExecutor::new("from agent runtime");
        let audit = audit_log::MemoryAuditSink::new();
        let action_runtime = action_runtime::ActionRuntime {
            kernel: &kernel,
            registry: &registry,
            policy: &policy,
            executor: &executor,
            audit_log: &audit,
        };
        let detector = KeywordActionProposalDetector;

        let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
            kernel: &kernel,
            adapter: &adapter,
            context_builder: &context_builder,
            config: &config,
            conversation_id: &conv_id,
            run_id: &run_id,
            trigger_message_id: &user_msg,
            agent_participant_id: &ParticipantId::from("a1"),
            detector: &detector,
            action_runtime: &action_runtime,
        })
        .await
        .unwrap();

        (outcome, kernel, conv_id, audit)
    }

    #[tokio::test]
    async fn agent_runtime_executes_read_only_action_from_fake_proposal() {
        let (outcome, kernel, conv_id, audit) = process_static_action_response(
            "I will search. ACTION knowledge.search {\"query\":\"agent os\"}",
        )
        .await;

        match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome,
                response_text,
                ..
            } => {
                assert!(matches!(
                    action_outcome,
                    action_runtime::ActionRuntimeOutcome::Completed { .. }
                ));
                assert!(response_text.contains("[Action outcome]"));
            }
            _ => panic!("expected completed with action"),
        }

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_eq!(state.actions.len(), 1);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Completed);
        assert_eq!(
            action.action_kind,
            action_core::ActionKind::from("knowledge.search")
        );
        assert_eq!(audit.list().await.unwrap()[0].result_status, "completed");
    }

    #[tokio::test]
    async fn agent_runtime_reports_approval_required_for_write_action() {
        let (outcome, kernel, conv_id, audit) = process_static_action_response(
            "I will save. ACTION knowledge.save_entry {\"title\":\"AgentOS\"}",
        )
        .await;

        assert!(matches!(
            outcome,
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. },
                ..
            }
        ));

        let state = kernel.load_state(&conv_id).await.unwrap();
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
        assert_eq!(
            audit.list().await.unwrap()[0].result_status,
            "approval_required"
        );
    }

    #[tokio::test]
    async fn agent_runtime_reports_denied_action_without_execution() {
        let (outcome, kernel, conv_id, audit) =
            process_static_action_response("I will send. ACTION mail.send {\"to\":\"x@y.z\"}")
                .await;

        assert!(matches!(
            outcome,
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::Denied { .. },
                ..
            }
        ));

        let state = kernel.load_state(&conv_id).await.unwrap();
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Denied);
        assert_eq!(audit.list().await.unwrap()[0].result_status, "denied");
    }

    #[tokio::test]
    async fn agent_runtime_noop_detector_preserves_text_only_outcome() {
        let kernel = test_kernel();
        let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
        let adapter = StaticAdapter {
            text: "Plain text response".to_string(),
        };
        let context_builder = AgentContextBuilder::new(50);
        let config = AgentRuntimeConfig::default();
        let registry = action_registry();
        let policy = capability_policy::CapabilityPolicy::default_safe();
        let executor = action_core::FakeActionExecutor::default();
        let audit = audit_log::MemoryAuditSink::new();
        let action_runtime = action_runtime::ActionRuntime {
            kernel: &kernel,
            registry: &registry,
            policy: &policy,
            executor: &executor,
            audit_log: &audit,
        };
        let detector = NoopActionProposalDetector;

        let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
            kernel: &kernel,
            adapter: &adapter,
            context_builder: &context_builder,
            config: &config,
            conversation_id: &conv_id,
            run_id: &run_id,
            trigger_message_id: &user_msg,
            agent_participant_id: &ParticipantId::from("a1"),
            detector: &detector,
            action_runtime: &action_runtime,
        })
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            AgentRunWithActionsOutcome::CompletedText { .. }
        ));
        assert!(
            kernel
                .load_state(&conv_id)
                .await
                .unwrap()
                .actions
                .is_empty()
        );
        assert!(audit.list().await.unwrap().is_empty());
    }
}
