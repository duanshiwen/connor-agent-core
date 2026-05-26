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

mod action_store;
mod approval_queue;
mod retry_policy;
mod run_queue;
mod run_store;
mod tool_loop_checkpoint;

pub use approval_queue::{
    ApprovalDecision, ApprovalDecisionKind, ApprovalQueue, ApprovalQueueError, ApprovalQueueResult,
    ApprovalRecord, ApprovalRequest, ApprovalStatus, JsonlApprovalQueue, MemoryApprovalQueue,
};

pub use action_store::{
    ActionRecord, ActionStore, ActionStoreError, ActionStoreResult, JsonlActionStore,
    MemoryActionStore,
};
pub use retry_policy::{
    DefaultRetryPolicy, RetryBackoffConfig, RetryDecision, RetryErrorClass, RetryPolicy,
    classify_error_message,
};
pub use run_queue::{AgentRunLease, AgentRunQueue, AgentRunQueueError, AgentRunQueueResult};
pub use run_store::{
    AgentRunRecord, AgentRunStore, AgentRunStoreError, AgentRunStoreResult, DurableAgentRunStatus,
    JsonlAgentRunStore, MemoryAgentRunStore,
};

pub use tool_loop_checkpoint::{
    MemoryToolLoopCheckpointStore, ToolLoopCheckpoint, ToolLoopCheckpointError,
    ToolLoopCheckpointKind, ToolLoopCheckpointResult, ToolLoopCheckpointStore, ToolLoopResumePlan,
    ToolResultCheckpoint,
};

use action_core::{ActionId, ActionKind, ActionRequest};
use action_runtime::{ActionRuntime, ActionRuntimeOutcome, ProcessActionRequest};
use anyhow::{Context, Result};
use conversation_core::*;
use conversation_kernel::{ConversationKernel, ConversationState};
use entity_core::EntityDescriptor;
use model_adapter::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

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
    /// Participants indexed by ID for deterministic role resolution.
    pub participants: HashMap<ParticipantId, Participant>,
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
            participants: state.participants.clone().into_iter().collect(),
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
            let role = context
                .participants
                .get(&msg.sender_id)
                .map(|participant| match participant.kind {
                    ParticipantKind::Agent => ModelRole::Assistant,
                    ParticipantKind::Human
                    | ParticipantKind::System
                    | ParticipantKind::Integration => ModelRole::User,
                })
                .unwrap_or(ModelRole::User);

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
        model_output: &ModelOutput,
    ) -> Option<AgentActionProposal>;
}

/// Detector that never proposes actions. Preserves text-only behavior.
#[derive(Debug, Default)]
pub struct NoopActionProposalDetector;

impl ActionProposalDetector for NoopActionProposalDetector {
    fn detect(
        &self,
        _context: &AgentContext,
        _model_output: &ModelOutput,
    ) -> Option<AgentActionProposal> {
        None
    }
}

struct ParsedActionMarker<'a> {
    kind: &'a str,
    input_text: &'a str,
}

fn parse_action_marker(text: &str) -> Option<ParsedActionMarker<'_>> {
    let marker = "ACTION ";
    let start = text.find(marker)? + marker.len();
    let rest = text[start..].trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let kind = parts.next()?.trim();
    if kind.is_empty() {
        return None;
    }
    Some(ParsedActionMarker {
        kind,
        input_text: parts.next().unwrap_or("{}").trim(),
    })
}

fn action_id_for_detected_action(run_id: &str, kind: &str) -> ActionId {
    let kind_slug = kind
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    ActionId::from(format!("action-{run_id}-{kind_slug}"))
}

fn proposal_from_parts(run_id: &str, kind: &str, input: serde_json::Value) -> AgentActionProposal {
    AgentActionProposal {
        action_id: action_id_for_detected_action(run_id, kind),
        action_kind: ActionKind::from(kind),
        input,
        summary: format!("Proposed action {kind}"),
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
        context: &AgentContext,
        model_output: &ModelOutput,
    ) -> Option<AgentActionProposal> {
        let text = model_output.text().unwrap_or("");
        let parsed = parse_action_marker(text)?;
        let input = if parsed.input_text.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(parsed.input_text).unwrap_or_else(|_| {
                serde_json::json!({
                    "raw": parsed.input_text,
                })
            })
        };
        Some(proposal_from_parts(&context.run_id, parsed.kind, input))
    }
}

/// Registry-gated detector for deterministic structured action proposal tests.
///
/// Unlike `KeywordActionProposalDetector`, this detector only accepts registered
/// action kinds and rejects malformed JSON input.
#[derive(Debug)]
pub struct RegistryActionProposalDetector<'a> {
    registry: &'a action_core::ActionRegistry,
}

impl<'a> RegistryActionProposalDetector<'a> {
    pub fn new(registry: &'a action_core::ActionRegistry) -> Self {
        Self { registry }
    }
}

impl ActionProposalDetector for RegistryActionProposalDetector<'_> {
    fn detect(
        &self,
        context: &AgentContext,
        model_output: &ModelOutput,
    ) -> Option<AgentActionProposal> {
        let text = model_output.text().unwrap_or("");
        let parsed = parse_action_marker(text)?;
        let action_kind = ActionKind::from(parsed.kind);
        self.registry.get(&action_kind)?;
        let input = if parsed.input_text.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(parsed.input_text).ok()?
        };
        Some(AgentActionProposal {
            action_id: action_id_for_detected_action(&context.run_id, parsed.kind),
            action_kind,
            input,
            summary: format!("Proposed action {}", parsed.kind),
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
                    text: response.text().unwrap_or("").to_string(),
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
            response_text: response.text().unwrap_or("").to_string(),
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
                response.text().unwrap_or(""),
                summarize_action_outcome(outcome)
            ),
            None => response.text().unwrap_or("").to_string(),
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

// ────────────────────────────────────────────────────────────────────────────
// PR 59: AgentToolLoop — LLM Tool Loop Runtime
// ────────────────────────────────────────────────────────────────────────────

/// Maps a model-returned `ToolCall` to an `ActionRequest` for execution.
pub trait ToolCallMapper: Send + Sync {
    fn map_to_action_request(
        &self,
        tool_call: &ToolCall,
        run_id: &str,
        conversation_id: &str,
    ) -> Result<ActionRequest>;
}

/// Configuration for the tool loop.
#[derive(Debug, Clone)]
pub struct ToolLoopConfig {
    /// Maximum number of model-call turns before stopping.
    pub max_turns: u32,
}

impl Default for ToolLoopConfig {
    fn default() -> Self {
        Self { max_turns: 10 }
    }
}

/// Result of a tool loop execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolLoopOutcome {
    Completed {
        response_text: String,
        turns_used: u32,
        tool_calls_made: u32,
    },
    MaxTurnsReached {
        last_tool_calls: Vec<ToolCall>,
        turns_used: u32,
    },
    Cancelled {
        reason: String,
        turns_used: u32,
    },
    TimedOut {
        operation: TimeoutOperation,
        timeout_ms: u64,
        turns_used: u32,
    },
    Failed {
        error: String,
        turns_used: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutOperation {
    ModelCall,
    Action,
    BrowserOperation,
}

#[derive(Debug, Clone, Default)]
pub struct RunCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl RunCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolLoopExecutionControls {
    pub cancellation_token: Option<RunCancellationToken>,
    pub model_call_timeout: Option<Duration>,
    pub action_timeout: Option<Duration>,
    pub browser_operation_timeout: Option<Duration>,
}

impl ToolLoopExecutionControls {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cancellation_token(mut self, token: RunCancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    pub fn with_model_call_timeout(mut self, timeout: Duration) -> Self {
        self.model_call_timeout = Some(timeout);
        self
    }

    pub fn with_action_timeout(mut self, timeout: Duration) -> Self {
        self.action_timeout = Some(timeout);
        self
    }

    pub fn with_browser_operation_timeout(mut self, timeout: Duration) -> Self {
        self.browser_operation_timeout = Some(timeout);
        self
    }

    pub fn cancel(&self) {
        if let Some(token) = &self.cancellation_token {
            token.cancel();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(RunCancellationToken::is_cancelled)
    }
}

/// Request to run the tool loop.
pub struct ToolLoopRequest<'a> {
    pub adapter: &'a dyn ToolCallingModelAdapter,
    pub action_runtime: &'a ActionRuntime<'a>,
    pub mapper: &'a dyn ToolCallMapper,
    pub config: &'a ToolLoopConfig,
    pub initial_request: ModelRequest,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: ToolChoice,
    pub run_id: &'a str,
    pub conversation_id: &'a str,
}

/// Orchestrates the LLM tool loop.
pub struct AgentToolLoop;

impl AgentToolLoop {
    pub async fn run(req: ToolLoopRequest<'_>) -> ToolLoopOutcome {
        Self::run_inner(req, None, ToolLoopExecutionControls::default()).await
    }

    pub async fn run_with_checkpoints(
        req: ToolLoopRequest<'_>,
        checkpoint_store: &dyn ToolLoopCheckpointStore,
    ) -> ToolLoopOutcome {
        Self::run_inner(
            req,
            Some(checkpoint_store),
            ToolLoopExecutionControls::default(),
        )
        .await
    }

    pub async fn run_with_controls(
        req: ToolLoopRequest<'_>,
        controls: ToolLoopExecutionControls,
    ) -> ToolLoopOutcome {
        Self::run_inner(req, None, controls).await
    }

    pub async fn run_with_checkpoints_and_controls(
        req: ToolLoopRequest<'_>,
        checkpoint_store: &dyn ToolLoopCheckpointStore,
        controls: ToolLoopExecutionControls,
    ) -> ToolLoopOutcome {
        Self::run_inner(req, Some(checkpoint_store), controls).await
    }

    async fn run_inner(
        req: ToolLoopRequest<'_>,
        checkpoint_store: Option<&dyn ToolLoopCheckpointStore>,
        controls: ToolLoopExecutionControls,
    ) -> ToolLoopOutcome {
        let mut turns_used: u32 = 0;
        let mut tool_calls_made: u32 = 0;
        let mut current_request = req.initial_request;
        let resume_plan = if let Some(store) = checkpoint_store {
            match store.list(req.run_id).await {
                Ok(checkpoints) => ToolLoopResumePlan::from_checkpoints(checkpoints),
                Err(e) => {
                    return ToolLoopOutcome::Failed {
                        error: format!("checkpoint load failed: {e}"),
                        turns_used: 0,
                    };
                }
            }
        } else {
            ToolLoopResumePlan::default()
        };

        loop {
            if controls.is_cancelled() {
                return ToolLoopOutcome::Cancelled {
                    reason: "run cancelled".to_string(),
                    turns_used,
                };
            }
            turns_used += 1;

            if turns_used > req.config.max_turns {
                return ToolLoopOutcome::MaxTurnsReached {
                    last_tool_calls: vec![],
                    turns_used: turns_used - 1,
                };
            }

            if let Some(store) = checkpoint_store
                && let Err(e) = store
                    .append(ToolLoopCheckpoint::before_model_call(
                        req.run_id, turns_used,
                    ))
                    .await
            {
                return ToolLoopOutcome::Failed {
                    error: format!("checkpoint write failed: {e}"),
                    turns_used: turns_used - 1,
                };
            }

            let model_call = req.adapter.complete_with_tools(
                current_request.clone(),
                req.tools.clone(),
                req.tool_choice.clone(),
            );
            let output = match with_optional_timeout(model_call, controls.model_call_timeout).await
            {
                Ok(Ok(o)) => o,
                Err(timeout) => {
                    return ToolLoopOutcome::TimedOut {
                        operation: TimeoutOperation::ModelCall,
                        timeout_ms: duration_millis(timeout),
                        turns_used: turns_used - 1,
                    };
                }
                Ok(Err(e)) => {
                    return ToolLoopOutcome::Failed {
                        error: format!("model call failed: {e}"),
                        turns_used: turns_used - 1,
                    };
                }
            };

            if let Some(store) = checkpoint_store
                && let Err(e) = store
                    .append(ToolLoopCheckpoint::after_model_call(req.run_id, turns_used))
                    .await
            {
                return ToolLoopOutcome::Failed {
                    error: format!("checkpoint write failed: {e}"),
                    turns_used: turns_used - 1,
                };
            }

            match output {
                ModelOutput::Text { text, .. } => {
                    return ToolLoopOutcome::Completed {
                        response_text: text,
                        turns_used: turns_used - 1,
                        tool_calls_made,
                    };
                }
                ModelOutput::ToolCalls {
                    content,
                    tool_calls,
                    ..
                } => {
                    let last_tool_calls = tool_calls.clone();
                    let mut tool_results: Vec<(String, String)> = Vec::new();

                    for tc in &tool_calls {
                        if controls.is_cancelled() {
                            return ToolLoopOutcome::Cancelled {
                                reason: "run cancelled".to_string(),
                                turns_used,
                            };
                        }
                        if resume_plan.should_skip_tool_call(&tc.id)
                            && let Some(result) = resume_plan.completed_tool_result(&tc.id)
                        {
                            tool_results.push((tc.id.clone(), result.to_string()));
                            continue;
                        }

                        let action_request = match req.mapper.map_to_action_request(
                            tc,
                            req.run_id,
                            req.conversation_id,
                        ) {
                            Ok(ar) => ar,
                            Err(e) => {
                                tool_results.push((tc.id.clone(), format!("error: {e}")));
                                continue;
                            }
                        };
                        tool_calls_made += 1;
                        let action_id = action_request.action_id.to_string();
                        let read_only = is_read_only_tool_action(&action_request.action_kind.0);

                        let action_timeout =
                            action_timeout_for(&controls, &action_request.action_kind.0);
                        let conversation_id = ConversationId::from(req.conversation_id);
                        let action = req.action_runtime.process(ProcessActionRequest {
                            conversation_id: &conversation_id,
                            action_request,
                            requested_by: Some(ParticipantId::from("agent")),
                            runtime_actor: Some(ParticipantId::from("tool_loop")),
                        });
                        let outcome = match with_optional_timeout(action, action_timeout).await {
                            Ok(Ok(o)) => o,
                            Err(timeout) => {
                                return ToolLoopOutcome::TimedOut {
                                    operation: if is_browser_tool_action(tc.name.as_str()) {
                                        TimeoutOperation::BrowserOperation
                                    } else {
                                        TimeoutOperation::Action
                                    },
                                    timeout_ms: duration_millis(timeout),
                                    turns_used,
                                };
                            }
                            Ok(Err(e)) => {
                                tool_results.push((tc.id.clone(), format!("error: {e}")));
                                continue;
                            }
                        };

                        let result_text = match outcome {
                            ActionRuntimeOutcome::Completed { result, .. } => result.summary,
                            ActionRuntimeOutcome::Denied { reason, .. } => {
                                format!("denied: {reason}")
                            }
                            ActionRuntimeOutcome::ApprovalRequired { action_id, .. } => {
                                format!("approval_required: {action_id}")
                            }
                            ActionRuntimeOutcome::Failed {
                                action_id,
                                error_message,
                                ..
                            } => format!("failed ({action_id}): {error_message}"),
                        };

                        if let Some(store) = checkpoint_store
                            && let Err(e) = store
                                .append(ToolLoopCheckpoint::tool_result(
                                    req.run_id,
                                    turns_used,
                                    ToolResultCheckpoint {
                                        tool_call_id: tc.id.clone(),
                                        action_id,
                                        result_text: result_text.clone(),
                                        read_only,
                                    },
                                ))
                                .await
                        {
                            return ToolLoopOutcome::Failed {
                                error: format!("checkpoint write failed: {e}"),
                                turns_used: turns_used - 1,
                            };
                        }
                        tool_results.push((tc.id.clone(), result_text));
                    }

                    let mut messages = current_request.messages;
                    let mut assistant_content = content.unwrap_or_default();
                    if assistant_content.is_empty() && !tool_calls.is_empty() {
                        assistant_content = "calling tools".to_string();
                    }
                    messages.push(ModelMessage {
                        role: ModelRole::Assistant,
                        text: assistant_content,
                    });

                    for (tool_call_id, result) in &tool_results {
                        messages.push(ModelMessage {
                            role: ModelRole::Tool,
                            text: format!("tool_call_id: {tool_call_id}\nresult: {result}"),
                        });
                    }

                    current_request = ModelRequest {
                        model_id: current_request.model_id.clone(),
                        messages,
                        max_output_tokens: current_request.max_output_tokens,
                        temperature_millis: current_request.temperature_millis,
                        metadata: current_request.metadata.clone(),
                    };

                    if turns_used >= req.config.max_turns {
                        return ToolLoopOutcome::MaxTurnsReached {
                            last_tool_calls,
                            turns_used,
                        };
                    }
                }
            }
        }
    }
}

async fn with_optional_timeout<F, T>(future: F, timeout: Option<Duration>) -> Result<T, Duration>
where
    F: std::future::Future<Output = T>,
{
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| timeout)
    } else {
        Ok(future.await)
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn action_timeout_for(controls: &ToolLoopExecutionControls, action_kind: &str) -> Option<Duration> {
    if is_browser_tool_action(action_kind) {
        controls
            .browser_operation_timeout
            .or(controls.action_timeout)
    } else {
        controls.action_timeout
    }
}

fn is_browser_tool_action(action_kind: &str) -> bool {
    action_kind.starts_with("browser.")
}

fn is_read_only_tool_action(action_kind: &str) -> bool {
    action_kind.contains(".search")
        || action_kind.contains(".read")
        || action_kind.contains(".list")
        || action_kind.contains(".get")
        || action_kind.contains(".fetch")
}

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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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
            participants: HashMap::from([
                (ParticipantId::from("u1"), human("u1", "Test User")),
                (
                    ParticipantId::from("a1"),
                    agent_participant("a1", "Assistant"),
                ),
            ]),
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
            participants: HashMap::from([(ParticipantId::from("u1"), human("u1", "Test User"))]),
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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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
            ) -> Result<ModelOutput, ModelAdapterError> {
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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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
        async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
            Ok(ModelOutput::Text {
                text: self.text.clone(),
                usage: None,
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
                participants: vec![
                    human("u1", "Test User"),
                    agent_participant("a1", "Assistant"),
                ],
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

    #[derive(Debug)]
    struct FailingActionExecutor;

    #[async_trait]
    impl action_core::ActionExecutor for FailingActionExecutor {
        async fn execute(
            &self,
            _request: &action_core::ActionRequest,
        ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
            Err(action_core::ActionExecutorError::ExecutionFailed(
                "intentional failure".to_string(),
            ))
        }
    }

    async fn process_static_action_response_with_executor(
        response_text: &str,
        executor: &dyn action_core::ActionExecutor,
        detector: &dyn ActionProposalDetector,
    ) -> (
        AgentRunWithActionsOutcome,
        ConversationKernel,
        ConversationId,
        String,
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
        let audit = audit_log::MemoryAuditSink::new();
        let action_runtime = action_runtime::ActionRuntime {
            kernel: &kernel,
            registry: &registry,
            policy: &policy,
            executor,
            audit_log: &audit,
            artifact_resolver: None,
        };

        let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
            kernel: &kernel,
            adapter: &adapter,
            context_builder: &context_builder,
            config: &config,
            conversation_id: &conv_id,
            run_id: &run_id,
            trigger_message_id: &user_msg,
            agent_participant_id: &ParticipantId::from("a1"),
            detector,
            action_runtime: &action_runtime,
        })
        .await
        .unwrap();

        (outcome, kernel, conv_id, run_id, audit)
    }

    async fn process_static_action_response(
        response_text: &str,
    ) -> (
        AgentRunWithActionsOutcome,
        ConversationKernel,
        ConversationId,
        String,
        audit_log::MemoryAuditSink,
    ) {
        let executor = action_core::FakeActionExecutor::new("from agent runtime");
        let detector = KeywordActionProposalDetector;
        process_static_action_response_with_executor(response_text, &executor, &detector).await
    }

    fn assert_agent_run_completed(
        state: &ConversationState,
        run_id: &str,
        output_message_id: &MessageId,
    ) {
        let run = state.agent_runs.get(run_id).unwrap();
        assert_eq!(run.status, AgentRunStatus::Completed);
        assert_eq!(run.output_message_id.as_ref(), Some(output_message_id));
    }

    fn knowledge_draft(
        title: &str,
        content_markdown: &str,
    ) -> knowledge_entity::KnowledgeEntryDraft {
        knowledge_entity::KnowledgeEntryDraft::new(title, content_markdown, chrono::Utc::now())
            .with_tags(vec!["agent-os".to_string()])
    }

    async fn process_static_knowledge_action_response(
        response_text: &str,
        repository: std::sync::Arc<knowledge_entity::MemoryKnowledgeRepository>,
    ) -> (
        AgentRunWithActionsOutcome,
        ConversationKernel,
        ConversationId,
        String,
        audit_log::MemoryAuditSink,
    ) {
        let kernel = test_kernel();
        let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
        let adapter = StaticAdapter {
            text: response_text.to_string(),
        };
        let context_builder = AgentContextBuilder::new(50);
        let config = AgentRuntimeConfig::default();
        let mut registry = action_core::ActionRegistry::new();
        knowledge_entity::register_knowledge_action_schemas(&mut registry).unwrap();
        let policy = capability_policy::CapabilityPolicy::default_safe();
        let executor = knowledge_entity::KnowledgeActionExecutor::new(repository);
        let audit = audit_log::MemoryAuditSink::new();
        let action_runtime = action_runtime::ActionRuntime {
            kernel: &kernel,
            registry: &registry,
            policy: &policy,
            executor: &executor,
            audit_log: &audit,
            artifact_resolver: None,
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

        (outcome, kernel, conv_id, run_id, audit)
    }

    #[tokio::test]
    async fn agent_runtime_executes_read_only_action_from_fake_proposal() {
        let (outcome, kernel, conv_id, run_id, audit) = process_static_action_response(
            "I will search. ACTION knowledge.search {\"query\":\"agent os\"}",
        )
        .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome,
                response_text,
                output_message_id,
                ..
            } => {
                assert!(matches!(
                    action_outcome,
                    action_runtime::ActionRuntimeOutcome::Completed { .. }
                ));
                assert!(response_text.contains("[Action outcome]"));
                output_message_id
            }
            _ => panic!("expected completed with action"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
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
        let (outcome, kernel, conv_id, run_id, audit) = process_static_action_response(
            "I will save. ACTION knowledge.save_entry {\"title\":\"AgentOS\"}",
        )
        .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. },
                output_message_id,
                ..
            } => output_message_id,
            _ => panic!("expected approval required action outcome"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
        assert_eq!(
            audit.list().await.unwrap()[0].result_status,
            "approval_required"
        );
    }

    #[tokio::test]
    async fn agent_runtime_reports_denied_action_without_execution() {
        let (outcome, kernel, conv_id, run_id, audit) =
            process_static_action_response("I will send. ACTION mail.send {\"to\":\"x@y.z\"}")
                .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::Denied { .. },
                output_message_id,
                ..
            } => output_message_id,
            _ => panic!("expected denied action outcome"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Denied);
        assert_eq!(audit.list().await.unwrap()[0].result_status, "denied");
    }

    #[tokio::test]
    async fn agent_runtime_executes_real_knowledge_search_proposal() {
        let repository = std::sync::Arc::new(knowledge_entity::MemoryKnowledgeRepository::new());
        knowledge_entity::KnowledgeRepository::save_draft(
            repository.as_ref(),
            knowledge_draft("AgentOS Notes", "foundation content"),
        )
        .await
        .unwrap();
        let (outcome, kernel, conv_id, run_id, audit) = process_static_knowledge_action_response(
            "I will search. ACTION knowledge.search {\"query\":{\"text\":\"agentos\",\"tags\":[],\"limit\":10}}",
            repository,
        )
        .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::Completed { result, .. },
                response_text,
                output_message_id,
                ..
            } => {
                assert!(response_text.contains("[Action outcome]"));
                let action_core::ActionResultPayload::Json(value) = result.payload else {
                    panic!("expected json payload");
                };
                let results: Vec<knowledge_entity::KnowledgeSearchResult> =
                    serde_json::from_value(value).unwrap();
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].entry.title, "AgentOS Notes");
                output_message_id
            }
            _ => panic!("expected completed knowledge action outcome"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Completed);
        assert_eq!(
            action.action_kind,
            action_core::ActionKind::from("knowledge.search")
        );
        let events = audit.list().await.unwrap();
        assert_eq!(events[0].policy_decision, "allow");
        assert_eq!(events[0].result_status, "completed");
    }

    #[tokio::test]
    async fn agent_runtime_reports_real_knowledge_create_draft_approval_required() {
        let repository = std::sync::Arc::new(knowledge_entity::MemoryKnowledgeRepository::new());
        let repository_for_assert = repository.clone();
        let (outcome, kernel, conv_id, run_id, audit) = process_static_knowledge_action_response(
            "I will draft. ACTION knowledge.create_draft {\"title\":\"AgentOS Notes\",\"content_markdown\":\"draft content\",\"source_uri\":null,\"source_artifact_id\":null,\"source_asset_id\":null,\"tags\":[\"agent-os\"],\"metadata\":{},\"created_at\":\"2026-05-24T12:00:00Z\"}",
            repository,
        )
        .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. },
                response_text,
                output_message_id,
                ..
            } => {
                assert!(response_text.contains("[Action outcome]"));
                output_message_id
            }
            _ => panic!("expected approval required knowledge action outcome"),
        };

        assert!(
            knowledge_entity::KnowledgeRepository::list_entries(repository_for_assert.as_ref())
                .await
                .unwrap()
                .is_empty()
        );
        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
        let events = audit.list().await.unwrap();
        assert_eq!(events[0].policy_decision, "ask");
        assert_eq!(events[0].result_status, "approval_required");
    }

    #[tokio::test]
    async fn agent_runtime_denies_real_knowledge_save_entry_by_default_safe_policy() {
        let repository = std::sync::Arc::new(knowledge_entity::MemoryKnowledgeRepository::new());
        let repository_for_assert = repository.clone();
        let (outcome, kernel, conv_id, run_id, audit) = process_static_knowledge_action_response(
            "I will save. ACTION knowledge.save_entry {\"draft\":{\"title\":\"AgentOS Notes\",\"content_markdown\":\"saved content\",\"source_uri\":null,\"source_artifact_id\":null,\"source_asset_id\":null,\"tags\":[\"agent-os\"],\"metadata\":{},\"created_at\":\"2026-05-24T12:00:00Z\"}}",
            repository,
        )
        .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::Denied { .. },
                response_text,
                output_message_id,
                ..
            } => {
                assert!(response_text.contains("[Action outcome]"));
                output_message_id
            }
            _ => panic!("expected denied knowledge action outcome"),
        };

        assert!(
            knowledge_entity::KnowledgeRepository::list_entries(repository_for_assert.as_ref())
                .await
                .unwrap()
                .is_empty()
        );
        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Denied);
        let events = audit.list().await.unwrap();
        assert_eq!(events[0].policy_decision, "deny");
        assert_eq!(events[0].result_status, "denied");
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
            artifact_resolver: None,
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

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedText {
                output_message_id, ..
            } => output_message_id,
            _ => panic!("expected text-only outcome"),
        };
        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        assert!(state.actions.is_empty());
        assert!(audit.list().await.unwrap().is_empty());
    }

    fn detector_context(run_id: &str) -> AgentContext {
        let trigger_message = Message {
            id: MessageId::from("msg-detector"),
            conversation_id: ConversationId::from("conv-detector"),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "detect".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        };
        AgentContext {
            conversation_id: ConversationId::from("conv-detector"),
            run_id: run_id.to_string(),
            trigger_message: trigger_message.clone(),
            messages: vec![trigger_message],
            participants: HashMap::from([(ParticipantId::from("u1"), human("u1", "Test User"))]),
            linked_entities: Vec::new(),
            model_id: ModelId::from("test-model"),
            system_prompt: None,
        }
    }

    fn detector_response(text: &str) -> ModelOutput {
        ModelOutput::Text {
            text: text.to_string(),
            usage: None,
        }
    }

    #[test]
    fn registry_detector_accepts_registered_action_with_json_input() {
        let registry = action_registry();
        let detector = RegistryActionProposalDetector::new(&registry);
        let proposal = detector
            .detect(
                &detector_context("run-typed"),
                &detector_response("Ready. ACTION knowledge.search {\"query\":\"agent os\"}"),
            )
            .unwrap();

        assert_eq!(
            proposal.action_id,
            action_core::ActionId::from("action-run-typed-knowledge-search")
        );
        assert_eq!(
            proposal.action_kind,
            action_core::ActionKind::from("knowledge.search")
        );
        assert_eq!(proposal.input, serde_json::json!({"query":"agent os"}));
    }

    #[test]
    fn registry_detector_rejects_unknown_action_kind() {
        let registry = action_registry();
        let detector = RegistryActionProposalDetector::new(&registry);
        assert!(
            detector
                .detect(
                    &detector_context("run-typed"),
                    &detector_response("ACTION unknown.action {\"x\":1}"),
                )
                .is_none()
        );
    }

    #[test]
    fn registry_detector_rejects_malformed_json() {
        let registry = action_registry();
        let detector = RegistryActionProposalDetector::new(&registry);
        assert!(
            detector
                .detect(
                    &detector_context("run-typed"),
                    &detector_response("ACTION knowledge.search not-json"),
                )
                .is_none()
        );
    }

    #[test]
    fn registry_detector_defaults_empty_input_to_empty_object() {
        let registry = action_registry();
        let detector = RegistryActionProposalDetector::new(&registry);
        let proposal = detector
            .detect(
                &detector_context("run-typed"),
                &detector_response("ACTION knowledge.search"),
            )
            .unwrap();
        assert_eq!(proposal.input, serde_json::json!({}));
    }

    #[tokio::test]
    async fn registry_detector_preserves_text_only_outcome_for_unknown_action() {
        let kernel = test_kernel();
        let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
        let adapter = StaticAdapter {
            text: "I will act. ACTION unknown.action {\"x\":1}".to_string(),
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
            artifact_resolver: None,
        };
        let detector = RegistryActionProposalDetector::new(&registry);

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

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedText {
                output_message_id, ..
            } => output_message_id,
            _ => panic!("expected text-only outcome"),
        };
        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        assert!(state.actions.is_empty());
        assert!(audit.list().await.unwrap().is_empty());
    }

    #[test]
    fn keyword_detector_keeps_raw_fallback_for_malformed_json() {
        let detector = KeywordActionProposalDetector;
        let proposal = detector
            .detect(
                &detector_context("run-keyword"),
                &detector_response("ACTION unknown.action not-json"),
            )
            .unwrap();

        assert_eq!(
            proposal.action_kind,
            action_core::ActionKind::from("unknown.action")
        );
        assert_eq!(proposal.input, serde_json::json!({"raw":"not-json"}));
    }

    #[tokio::test]
    async fn agent_runtime_reports_failed_action_and_completes_run() {
        let executor = FailingActionExecutor;
        let detector = KeywordActionProposalDetector;
        let (outcome, kernel, conv_id, run_id, audit) =
            process_static_action_response_with_executor(
                "I will search. ACTION knowledge.search {\"query\":\"agent os\"}",
                &executor,
                &detector,
            )
            .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome: action_runtime::ActionRuntimeOutcome::Failed { .. },
                output_message_id,
                ..
            } => output_message_id,
            _ => panic!("expected failed action outcome"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Failed);
        assert_eq!(audit.list().await.unwrap()[0].result_status, "failed");
    }

    // ---- PR 37: Browser AgentRuntime proposal integration ----

    async fn process_static_browser_action_response(
        response_text: &str,
    ) -> (
        AgentRunWithActionsOutcome,
        ConversationKernel,
        ConversationId,
        String,
        audit_log::MemoryAuditSink,
    ) {
        let kernel = test_kernel();
        let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
        let adapter = StaticAdapter {
            text: response_text.to_string(),
        };
        let context_builder = AgentContextBuilder::new(50);
        let config = AgentRuntimeConfig::default();
        let mut registry = action_core::ActionRegistry::new();
        browser_entity::register_browser_action_schemas(&mut registry).unwrap();
        let policy = capability_policy::CapabilityPolicy::default_safe();
        let executor = browser_entity::FakeBrowserExecutor::new(chrono::Utc::now());
        let audit = audit_log::MemoryAuditSink::new();
        let action_runtime = action_runtime::ActionRuntime {
            kernel: &kernel,
            registry: &registry,
            policy: &policy,
            executor: &executor,
            audit_log: &audit,
            artifact_resolver: None,
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

        (outcome, kernel, conv_id, run_id, audit)
    }

    #[tokio::test]
    async fn agent_runtime_executes_browser_extract_content_proposal() {
        let (outcome, kernel, conv_id, run_id, audit) = process_static_browser_action_response(
            "I will extract. ACTION browser.extract_content {\"url\":\"https://example.com\"}",
        )
        .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome,
                output_message_id,
                ..
            } => {
                assert!(matches!(
                    action_outcome,
                    action_runtime::ActionRuntimeOutcome::Completed { .. }
                ));
                output_message_id
            }
            _ => panic!("expected completed with action"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::Completed);
        assert_eq!(audit.list().await.unwrap()[0].policy_decision, "allow");
        assert_eq!(audit.list().await.unwrap()[0].result_status, "completed");
    }

    #[tokio::test]
    async fn agent_runtime_reports_browser_open_url_approval_required() {
        let (outcome, kernel, conv_id, run_id, audit) =
            process_static_browser_action_response(
                "I will open. ACTION browser.open_url {\"url\":\"https://example.com\",\"take_snapshot\":true}",
            )
            .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome,
                output_message_id,
                ..
            } => {
                assert!(matches!(
                    action_outcome,
                    action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. }
                ));
                output_message_id
            }
            _ => panic!("expected completed with action"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
        assert_eq!(audit.list().await.unwrap()[0].policy_decision, "ask");
        assert_eq!(
            audit.list().await.unwrap()[0].result_status,
            "approval_required"
        );
    }

    #[tokio::test]
    async fn agent_runtime_reports_browser_capture_snapshot_approval_required() {
        let (outcome, kernel, conv_id, run_id, audit) =
            process_static_browser_action_response(
                "I will capture. ACTION browser.capture_snapshot {\"url\":\"https://example.com\",\"include_html\":false}",
            )
            .await;

        let output_message_id = match outcome {
            AgentRunWithActionsOutcome::CompletedWithAction {
                action_outcome,
                output_message_id,
                ..
            } => {
                assert!(matches!(
                    action_outcome,
                    action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. }
                ));
                output_message_id
            }
            _ => panic!("expected completed with action"),
        };

        let state = kernel.load_state(&conv_id).await.unwrap();
        assert_agent_run_completed(&state, &run_id, &output_message_id);
        let action = state.actions.values().next().unwrap();
        assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
        assert_eq!(audit.list().await.unwrap()[0].policy_decision, "ask");
        assert_eq!(
            audit.list().await.unwrap()[0].result_status,
            "approval_required"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // PR 59: AgentToolLoop tests
    // ─────────────────────────────────────────────────────────────────────

    use std::sync::atomic::{AtomicU32, Ordering};

    struct FakeToolAdapter {
        responses: tokio::sync::Mutex<Vec<ModelOutput>>,
        call_count: AtomicU32,
    }

    impl FakeToolAdapter {
        fn new(responses: Vec<ModelOutput>) -> Self {
            Self {
                responses: tokio::sync::Mutex::new(responses),
                call_count: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl ModelAdapter for FakeToolAdapter {
        async fn complete(
            &self,
            _request: ModelRequest,
        ) -> std::result::Result<ModelOutput, ModelAdapterError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let mut r = self.responses.lock().await;
            if r.is_empty() {
                return Err(ModelAdapterError::ExecutorFailed("empty".into()));
            }
            Ok(r.remove(0))
        }
    }

    #[async_trait]
    impl ToolCallingModelAdapter for FakeToolAdapter {
        async fn complete_with_tools(
            &self,
            _request: ModelRequest,
            _tools: Vec<ToolDefinition>,
            _choice: ToolChoice,
        ) -> std::result::Result<ModelOutput, ModelAdapterError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let mut r = self.responses.lock().await;
            if r.is_empty() {
                return Err(ModelAdapterError::ExecutorFailed("empty".into()));
            }
            Ok(r.remove(0))
        }
    }

    struct TestMapper;
    impl ToolCallMapper for TestMapper {
        fn map_to_action_request(
            &self,
            tc: &ToolCall,
            run_id: &str,
            conv_id: &str,
        ) -> Result<ActionRequest> {
            Ok(ActionRequest {
                action_id: ActionId(format!("{run_id}-{}", tc.id)),
                action_kind: ActionKind(tc.name.clone()),
                input: tc.arguments.clone(),
                requested_by: "agent".to_string(),
                conversation_id: Some(conv_id.to_string()),
                message_id: None,
                requested_at: chrono::Utc::now(),
            })
        }
    }

    macro_rules! with_tool_runtime {
        ($k:ident, $rt:ident, $body:block) => {{
            let $k = test_kernel();
            let registry = action_registry();
            let executor = action_core::FakeActionExecutor::new("ok");
            let audit = audit_log::MemoryAuditSink::new();
            let policy = capability_policy::CapabilityPolicy::default_safe();
            let $rt = ActionRuntime {
                kernel: &$k,
                registry: &registry,
                policy: &policy,
                executor: &executor,
                audit_log: &audit,
                artifact_resolver: None,
            };
            $body
        }};
    }

    #[tokio::test]
    async fn tool_loop_text_only_completes() {
        let adapter = FakeToolAdapter::new(vec![ModelOutput::Text {
            text: "Hello!".to_string(),
            usage: Some(ModelUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }]);
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run(ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig::default(),
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("hi")]),
                tools: vec![],
                tool_choice: ToolChoice::Auto,
                run_id: "run-1",
                conversation_id: "conv-1",
            })
            .await;
            assert!(matches!(outcome, ToolLoopOutcome::Completed {
                response_text, turns_used: 0, tool_calls_made: 0
            } if response_text == "Hello!"));
        });
    }

    #[tokio::test]
    async fn tool_loop_executes_and_returns_text() {
        let adapter = FakeToolAdapter::new(vec![
            ModelOutput::ToolCalls {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "call_001".into(),
                    name: "knowledge.search".into(),
                    arguments: serde_json::json!({"q": "test"}),
                    raw_arguments: r#"{"q":"test"}"#.into(),
                }],
                usage: None,
            },
            ModelOutput::Text {
                text: "Found it!".to_string(),
                usage: None,
            },
        ]);
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run(ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig::default(),
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
                tools: vec![ToolDefinition {
                    name: "knowledge.search".into(),
                    description: "Search".into(),
                    input_schema: serde_json::json!({}),
                }],
                tool_choice: ToolChoice::Auto,
                run_id: "run-2",
                conversation_id: "conv-2",
            })
            .await;
            assert!(matches!(outcome, ToolLoopOutcome::Completed {
                response_text, turns_used: 1, tool_calls_made: 1
            } if response_text == "Found it!"));
        });
    }

    #[tokio::test]
    async fn tool_loop_checkpoint_resume_skips_completed_read_only_tool_result() {
        let checkpoint_store = MemoryToolLoopCheckpointStore::new();
        checkpoint_store
            .append(ToolLoopCheckpoint::tool_result(
                "run-resume",
                1,
                ToolResultCheckpoint {
                    tool_call_id: "call_001".to_string(),
                    action_id: "run-resume-call_001".to_string(),
                    result_text: "cached result".to_string(),
                    read_only: true,
                },
            ))
            .await
            .unwrap();
        let adapter = FakeToolAdapter::new(vec![
            ModelOutput::ToolCalls {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "call_001".into(),
                    name: "knowledge.search".into(),
                    arguments: serde_json::json!({"q": "test"}),
                    raw_arguments: r#"{"q":"test"}"#.into(),
                }],
                usage: None,
            },
            ModelOutput::Text {
                text: "Used cached result".to_string(),
                usage: None,
            },
        ]);
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run_with_checkpoints(
                ToolLoopRequest {
                    adapter: &adapter,
                    action_runtime: &rt,
                    mapper: &TestMapper,
                    config: &ToolLoopConfig::default(),
                    initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
                    tools: vec![],
                    tool_choice: ToolChoice::Auto,
                    run_id: "run-resume",
                    conversation_id: "conv-resume",
                },
                &checkpoint_store,
            )
            .await;
            assert!(matches!(outcome, ToolLoopOutcome::Completed {
                response_text, turns_used: 1, tool_calls_made: 0
            } if response_text == "Used cached result"));
        });
    }

    #[tokio::test]
    async fn tool_loop_cancelled_run_does_not_execute_followup_tool() {
        let controls =
            ToolLoopExecutionControls::new().with_cancellation_token(RunCancellationToken::new());
        controls.cancel();
        let adapter = FakeToolAdapter::new(vec![ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_cancelled".into(),
                name: "knowledge.search".into(),
                arguments: serde_json::json!({"q": "skip"}),
                raw_arguments: r#"{"q":"skip"}"#.into(),
            }],
            usage: None,
        }]);
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run_with_controls(
                ToolLoopRequest {
                    adapter: &adapter,
                    action_runtime: &rt,
                    mapper: &TestMapper,
                    config: &ToolLoopConfig::default(),
                    initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
                    tools: vec![],
                    tool_choice: ToolChoice::Auto,
                    run_id: "run-cancelled",
                    conversation_id: "conv-cancelled",
                },
                controls,
            )
            .await;
            assert!(matches!(
                outcome,
                ToolLoopOutcome::Cancelled { turns_used: 0, .. }
            ));
        });
    }

    #[tokio::test]
    async fn tool_loop_model_timeout_returns_typed_outcome() {
        struct SlowToolAdapter;
        #[async_trait]
        impl ModelAdapter for SlowToolAdapter {
            async fn complete(
                &self,
                _request: ModelRequest,
            ) -> std::result::Result<ModelOutput, ModelAdapterError> {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(ModelOutput::Text {
                    text: "late".into(),
                    usage: None,
                })
            }
        }
        #[async_trait]
        impl ToolCallingModelAdapter for SlowToolAdapter {
            async fn complete_with_tools(
                &self,
                _request: ModelRequest,
                _tools: Vec<ToolDefinition>,
                _choice: ToolChoice,
            ) -> std::result::Result<ModelOutput, ModelAdapterError> {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(ModelOutput::Text {
                    text: "late".into(),
                    usage: None,
                })
            }
        }
        let adapter = SlowToolAdapter;
        let controls = ToolLoopExecutionControls::new()
            .with_model_call_timeout(std::time::Duration::from_millis(1));
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run_with_controls(
                ToolLoopRequest {
                    adapter: &adapter,
                    action_runtime: &rt,
                    mapper: &TestMapper,
                    config: &ToolLoopConfig::default(),
                    initial_request: ModelRequest::new("test", vec![ModelMessage::user("timeout")]),
                    tools: vec![],
                    tool_choice: ToolChoice::Auto,
                    run_id: "run-timeout",
                    conversation_id: "conv-timeout",
                },
                controls,
            )
            .await;
            assert!(
                matches!(outcome, ToolLoopOutcome::TimedOut { operation, turns_used: 0, .. } if operation == TimeoutOperation::ModelCall)
            );
        });
    }

    #[tokio::test]
    async fn tool_loop_respects_max_turns() {
        let adapter = FakeToolAdapter::new(vec![
            ModelOutput::ToolCalls {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "knowledge.search".into(),
                    arguments: serde_json::json!({}),
                    raw_arguments: "{}".into(),
                }],
                usage: None,
            },
            ModelOutput::ToolCalls {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "c2".into(),
                    name: "knowledge.search".into(),
                    arguments: serde_json::json!({}),
                    raw_arguments: "{}".into(),
                }],
                usage: None,
            },
        ]);
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run(ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig { max_turns: 2 },
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("loop")]),
                tools: vec![],
                tool_choice: ToolChoice::Auto,
                run_id: "run-3",
                conversation_id: "conv-3",
            })
            .await;
            assert!(matches!(
                outcome,
                ToolLoopOutcome::MaxTurnsReached { turns_used: 2, .. }
            ));
        });
    }

    #[tokio::test]
    async fn tool_loop_handles_model_error() {
        let adapter = FakeToolAdapter::new(vec![]);
        with_tool_runtime!(_k, rt, {
            let outcome = AgentToolLoop::run(ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig::default(),
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("fail")]),
                tools: vec![],
                tool_choice: ToolChoice::Auto,
                run_id: "run-4",
                conversation_id: "conv-4",
            })
            .await;
            assert!(
                matches!(outcome, ToolLoopOutcome::Failed { error, turns_used: 0 }
                if error.contains("empty"))
            );
        });
    }
}
