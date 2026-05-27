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
use agentos_observability::{
    InMemoryObservabilitySink, MetricSample, ObservabilityEventKind, TraceEvent,
};
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
        usage: Option<ModelUsage>,
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

/// Optional observability wiring for the tool loop.
pub struct ToolLoopObservability<'a> {
    sink: &'a mut InMemoryObservabilitySink,
}

impl<'a> ToolLoopObservability<'a> {
    pub fn new(sink: &'a mut InMemoryObservabilitySink) -> Self {
        Self { sink }
    }

    fn trace(
        &mut self,
        run_id: &str,
        conversation_id: &str,
        operation: &str,
        turn: u32,
        attributes: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) {
        let mut event = TraceEvent::new(
            format!("tool-loop:{run_id}:{operation}:{turn}"),
            ObservabilityEventKind::ToolLoop,
            "agent-runtime.tool-loop",
            run_id,
            chrono::Utc::now(),
        )
        .with_operation(operation)
        .with_attribute("conversation_id", conversation_id.to_string())
        .with_attribute("turn", i64::from(turn));
        for (key, value) in attributes {
            event = event.with_attribute(key, value);
        }
        self.sink.record_trace(event);
    }

    fn counter(&mut self, name: &str, value: f64, run_id: &str, conversation_id: &str) {
        self.sink.record_metric(
            MetricSample::counter(name, value)
                .with_label("run_id", run_id)
                .with_label("conversation_id", conversation_id),
        );
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
    pub observability: Option<ToolLoopObservability<'a>>,
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
        let mut aggregated_usage = ModelUsage::default();
        let mut current_request = req.initial_request;
        let mut observability = req.observability;
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
                if let Some(obs) = observability.as_mut() {
                    obs.trace(
                        req.run_id,
                        req.conversation_id,
                        "tool_loop.cancelled",
                        turns_used,
                        [("reason".to_string(), serde_json::json!("run cancelled"))],
                    );
                    obs.counter("tool_loop.cancelled", 1.0, req.run_id, req.conversation_id);
                }
                return ToolLoopOutcome::Cancelled {
                    reason: "run cancelled".to_string(),
                    turns_used,
                };
            }
            turns_used += 1;

            if turns_used > req.config.max_turns {
                if let Some(obs) = observability.as_mut() {
                    obs.trace(
                        req.run_id,
                        req.conversation_id,
                        "tool_loop.max_turns_reached",
                        turns_used - 1,
                        [(
                            "max_turns".to_string(),
                            serde_json::json!(req.config.max_turns),
                        )],
                    );
                    obs.counter(
                        "tool_loop.max_turns_reached",
                        1.0,
                        req.run_id,
                        req.conversation_id,
                    );
                }
                return ToolLoopOutcome::MaxTurnsReached {
                    last_tool_calls: vec![],
                    turns_used: turns_used - 1,
                };
            }

            if let Some(obs) = observability.as_mut() {
                obs.trace(
                    req.run_id,
                    req.conversation_id,
                    "tool_loop.turn_started",
                    turns_used,
                    [(
                        "request_messages".to_string(),
                        serde_json::json!(current_request.messages.len()),
                    )],
                );
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
                Ok(Ok(o)) => {
                    if let Some(obs) = observability.as_mut() {
                        obs.trace(
                            req.run_id,
                            req.conversation_id,
                            "tool_loop.model_call_completed",
                            turns_used,
                            [],
                        );
                        obs.counter(
                            "tool_loop.model_call.completed",
                            1.0,
                            req.run_id,
                            req.conversation_id,
                        );
                    }
                    o
                }
                Err(timeout) => {
                    if let Some(obs) = observability.as_mut() {
                        obs.trace(
                            req.run_id,
                            req.conversation_id,
                            "tool_loop.model_call_timed_out",
                            turns_used - 1,
                            [(
                                "timeout_ms".to_string(),
                                serde_json::json!(duration_millis(timeout)),
                            )],
                        );
                        obs.counter(
                            "tool_loop.model_call.timed_out",
                            1.0,
                            req.run_id,
                            req.conversation_id,
                        );
                    }
                    return ToolLoopOutcome::TimedOut {
                        operation: TimeoutOperation::ModelCall,
                        timeout_ms: duration_millis(timeout),
                        turns_used: turns_used - 1,
                    };
                }
                Ok(Err(e)) => {
                    if let Some(obs) = observability.as_mut() {
                        obs.trace(
                            req.run_id,
                            req.conversation_id,
                            "tool_loop.model_call_failed",
                            turns_used - 1,
                            [("error".to_string(), serde_json::json!(e.to_string()))],
                        );
                        obs.counter(
                            "tool_loop.model_call.failed",
                            1.0,
                            req.run_id,
                            req.conversation_id,
                        );
                    }
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

            if let Some(usage) = output.usage() {
                aggregated_usage.input_tokens = aggregated_usage
                    .input_tokens
                    .saturating_add(usage.input_tokens);
                aggregated_usage.output_tokens = aggregated_usage
                    .output_tokens
                    .saturating_add(usage.output_tokens);
            }

            match output {
                ModelOutput::Text { text, .. } => {
                    if let Some(obs) = observability.as_mut() {
                        obs.trace(
                            req.run_id,
                            req.conversation_id,
                            "tool_loop.completed",
                            turns_used - 1,
                            [
                                (
                                    "tool_calls_made".to_string(),
                                    serde_json::json!(tool_calls_made),
                                ),
                                (
                                    "input_tokens".to_string(),
                                    serde_json::json!(aggregated_usage.input_tokens),
                                ),
                                (
                                    "output_tokens".to_string(),
                                    serde_json::json!(aggregated_usage.output_tokens),
                                ),
                            ],
                        );
                        obs.counter("tool_loop.completed", 1.0, req.run_id, req.conversation_id);
                        obs.counter(
                            "tool_loop.tool_calls_made",
                            f64::from(tool_calls_made),
                            req.run_id,
                            req.conversation_id,
                        );
                    }
                    return ToolLoopOutcome::Completed {
                        response_text: text,
                        turns_used: turns_used - 1,
                        tool_calls_made,
                        usage: Some(aggregated_usage),
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
                                if let Some(obs) = observability.as_mut() {
                                    obs.trace(
                                        req.run_id,
                                        req.conversation_id,
                                        "tool_loop.tool_mapping_failed",
                                        turns_used,
                                        [
                                            (
                                                "tool_call_id".to_string(),
                                                serde_json::json!(tc.id.clone()),
                                            ),
                                            (
                                                "tool_name".to_string(),
                                                serde_json::json!(tc.name.clone()),
                                            ),
                                            ("error".to_string(), serde_json::json!(e.to_string())),
                                        ],
                                    );
                                    obs.counter(
                                        "tool_loop.tool_mapping.failed",
                                        1.0,
                                        req.run_id,
                                        req.conversation_id,
                                    );
                                }
                                tool_results.push((tc.id.clone(), format!("error: {e}")));
                                continue;
                            }
                        };
                        tool_calls_made += 1;
                        let action_id = action_request.action_id.to_string();
                        let action_kind = action_request.action_kind.0.clone();
                        let read_only = is_read_only_tool_action(&action_kind);
                        if let Some(obs) = observability.as_mut() {
                            obs.trace(
                                req.run_id,
                                req.conversation_id,
                                "tool_loop.action_started",
                                turns_used,
                                [
                                    ("tool_call_id".to_string(), serde_json::json!(tc.id.clone())),
                                    ("tool_name".to_string(), serde_json::json!(tc.name.clone())),
                                    (
                                        "action_id".to_string(),
                                        serde_json::json!(action_id.clone()),
                                    ),
                                    (
                                        "action_kind".to_string(),
                                        serde_json::json!(action_kind.clone()),
                                    ),
                                    ("read_only".to_string(), serde_json::json!(read_only)),
                                ],
                            );
                            obs.counter(
                                "tool_loop.action.started",
                                1.0,
                                req.run_id,
                                req.conversation_id,
                            );
                        }

                        let action_timeout = action_timeout_for(&controls, &action_kind);
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
                                if let Some(obs) = observability.as_mut() {
                                    obs.trace(
                                        req.run_id,
                                        req.conversation_id,
                                        "tool_loop.action_timed_out",
                                        turns_used,
                                        [
                                            (
                                                "action_id".to_string(),
                                                serde_json::json!(action_id.clone()),
                                            ),
                                            (
                                                "action_kind".to_string(),
                                                serde_json::json!(action_kind.clone()),
                                            ),
                                            (
                                                "timeout_ms".to_string(),
                                                serde_json::json!(duration_millis(timeout)),
                                            ),
                                        ],
                                    );
                                    obs.counter(
                                        "tool_loop.action.timed_out",
                                        1.0,
                                        req.run_id,
                                        req.conversation_id,
                                    );
                                }
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
                                if let Some(obs) = observability.as_mut() {
                                    obs.trace(
                                        req.run_id,
                                        req.conversation_id,
                                        "tool_loop.action_failed",
                                        turns_used,
                                        [
                                            (
                                                "action_id".to_string(),
                                                serde_json::json!(action_id.clone()),
                                            ),
                                            (
                                                "action_kind".to_string(),
                                                serde_json::json!(action_kind.clone()),
                                            ),
                                            ("error".to_string(), serde_json::json!(e.to_string())),
                                        ],
                                    );
                                    obs.counter(
                                        "tool_loop.action.failed",
                                        1.0,
                                        req.run_id,
                                        req.conversation_id,
                                    );
                                }
                                tool_results.push((tc.id.clone(), format!("error: {e}")));
                                continue;
                            }
                        };

                        let outcome_kind = action_outcome_kind(&outcome).to_string();
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
                        if let Some(obs) = observability.as_mut() {
                            obs.trace(
                                req.run_id,
                                req.conversation_id,
                                "tool_loop.action_completed",
                                turns_used,
                                [
                                    (
                                        "action_id".to_string(),
                                        serde_json::json!(action_id.clone()),
                                    ),
                                    (
                                        "action_kind".to_string(),
                                        serde_json::json!(action_kind.clone()),
                                    ),
                                    (
                                        "outcome".to_string(),
                                        serde_json::json!(outcome_kind.clone()),
                                    ),
                                ],
                            );
                            obs.counter(
                                &format!("tool_loop.action.{outcome_kind}"),
                                1.0,
                                req.run_id,
                                req.conversation_id,
                            );
                        }

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

fn action_outcome_kind(outcome: &ActionRuntimeOutcome) -> &'static str {
    match outcome {
        ActionRuntimeOutcome::Completed { .. } => "completed",
        ActionRuntimeOutcome::ApprovalRequired { .. } => "approval_required",
        ActionRuntimeOutcome::Denied { .. } => "denied",
        ActionRuntimeOutcome::Failed { .. } => "failed",
    }
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
mod tests;
