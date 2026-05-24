//! Conversation policies — pluggable rules for triggering agent runs.
//!
//! Policies determine when a new message should trigger an agent run.
//! The first version uses simple keyword rules; future versions can
//! plug in a local model (e.g. Qwen) or a more sophisticated classifier.

use crate::state::ConversationState;
use conversation_core::*;
use serde::{Deserialize, Serialize};

/// Why a policy decided to trigger an agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunReason {
    /// The user explicitly mentioned the assistant (e.g. "@assistant").
    ExplicitMention,
    /// The user made a help request (e.g. "帮我", "请").
    HelpRequest,
    /// A complex concept was detected.
    ComplexConcept,
    /// A historical reference was detected.
    HistoricalReference,
    /// The assistant proactively decided to suggest.
    Proactive,
}

/// Trait for conversation policies.
///
/// Implementors decide whether a new message should trigger an agent run.
pub trait ConversationPolicy: Send + Sync {
    /// Evaluate whether the given message should trigger an agent run.
    ///
    /// Returns `Some(reason)` if an agent run should be triggered, `None` otherwise.
    fn should_request_agent_run(
        &self,
        state: &ConversationState,
        message: &Message,
    ) -> Option<AgentRunReason>;
}

/// A rule-based policy that triggers on keywords.
///
/// First version: no model, just pattern matching.
pub struct RuleBasedPolicy;

impl ConversationPolicy for RuleBasedPolicy {
    fn should_request_agent_run(
        &self,
        _state: &ConversationState,
        message: &Message,
    ) -> Option<AgentRunReason> {
        match &message.content {
            MessageContent::Text { text } => {
                let text_lower = text.to_lowercase();

                // Explicit mention takes priority.
                if text_lower.contains("@assistant") || text_lower.contains("@ai") {
                    return Some(AgentRunReason::ExplicitMention);
                }

                // Help request patterns.
                if text.starts_with("帮我")
                    || text.starts_with("请帮")
                    || text.starts_with("帮个忙")
                    || text_lower.starts_with("help")
                    || text_lower.starts_with("please")
                {
                    return Some(AgentRunReason::HelpRequest);
                }

                // Instruction patterns.
                if text.starts_with("总结")
                    || text.starts_with("分析")
                    || text.starts_with("解释")
                    || text.starts_with("设计")
                    || text.starts_with("比较")
                    || text_lower.starts_with("summarize")
                    || text_lower.starts_with("analyze")
                    || text_lower.starts_with("explain")
                {
                    return Some(AgentRunReason::HelpRequest);
                }

                None
            }
            // Non-text messages don't trigger agent runs in the rule-based policy.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ConversationState;
    use chrono::Utc;

    fn make_text_message(text: &str) -> Message {
        Message {
            id: MessageId::from("msg-test"),
            conversation_id: ConversationId::from("conv-1"),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: text.to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: Utc::now(),
            edited_at: None,
        }
    }

    #[test]
    fn explicit_mention_assistant_alias() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();
        let msg = make_text_message("@assistant 帮我总结一下");

        let reason = policy.should_request_agent_run(&state, &msg);
        assert_eq!(reason, Some(AgentRunReason::ExplicitMention));
    }

    #[test]
    fn explicit_mention_english() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();
        let msg = make_text_message("@assistant what do you think?");

        let reason = policy.should_request_agent_run(&state, &msg);
        assert_eq!(reason, Some(AgentRunReason::ExplicitMention));
    }

    #[test]
    fn explicit_mention_at_ai() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();
        let msg = make_text_message("@AI 请解释一下");

        let reason = policy.should_request_agent_run(&state, &msg);
        assert_eq!(reason, Some(AgentRunReason::ExplicitMention));
    }

    #[test]
    fn help_request_chinese() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("帮我设计一下")),
            Some(AgentRunReason::HelpRequest)
        );

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("请帮我看一下")),
            Some(AgentRunReason::HelpRequest)
        );
    }

    #[test]
    fn help_request_english() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("help me with this")),
            Some(AgentRunReason::HelpRequest)
        );
    }

    #[test]
    fn instruction_patterns() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("总结一下今天的会议")),
            Some(AgentRunReason::HelpRequest)
        );

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("分析这个系统的瓶颈")),
            Some(AgentRunReason::HelpRequest)
        );

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("summarize this")),
            Some(AgentRunReason::HelpRequest)
        );
    }

    #[test]
    fn normal_message_does_not_trigger() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("今天天气不错")),
            None
        );

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("I'm going to the store")),
            None
        );

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("好的")),
            None
        );

        assert_eq!(
            policy.should_request_agent_run(&state, &make_text_message("哈哈哈")),
            None
        );
    }

    #[test]
    fn system_notice_does_not_trigger() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();
        let msg = Message {
            id: MessageId::from("msg-sys"),
            conversation_id: ConversationId::from("conv-1"),
            sender_id: ParticipantId::from("system"),
            content: MessageContent::SystemNotice {
                text: "User joined".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: Utc::now(),
            edited_at: None,
        };

        assert_eq!(policy.should_request_agent_run(&state, &msg), None);
    }

    #[test]
    fn explicit_mention_takes_priority_over_help() {
        let policy = RuleBasedPolicy;
        let state = ConversationState::default();
        let msg = make_text_message("@assistant 帮我分析一下");

        // Should be ExplicitMention, not HelpRequest
        assert_eq!(
            policy.should_request_agent_run(&state, &msg),
            Some(AgentRunReason::ExplicitMention)
        );
    }
}
