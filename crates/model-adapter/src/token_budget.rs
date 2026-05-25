use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────────────
// Token Estimator
// ────────────────────────────────────────────────────────────────────────────

/// Estimates token count for a given text.
pub trait TokenEstimator: Send + Sync {
    fn estimate(&self, text: &str) -> u32;
}

/// Simple char-based estimator: tokens ≈ chars / 4.
pub struct SimpleTokenEstimator;

impl TokenEstimator for SimpleTokenEstimator {
    fn estimate(&self, text: &str) -> u32 {
        (text.len() as u32).div_ceil(4)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Token Budget Config
// ────────────────────────────────────────────────────────────────────────────

/// Configuration for token budget gates.
#[derive(Debug, Clone)]
pub struct TokenBudgetConfig {
    /// Max tokens for a single tool result before gating.
    pub tool_result_inline_limit: u32,
    /// Max total tokens for tool schemas.
    pub tool_schema_budget: u32,
    /// Max tokens for conversation history.
    pub history_budget: u32,
    /// Max total context tokens (history + tool results + system).
    pub total_context_budget: u32,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            tool_result_inline_limit: 512,
            tool_schema_budget: 4096,
            history_budget: 32768,
            total_context_budget: 100_000,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Aggregated Usage
// ────────────────────────────────────────────────────────────────────────────

/// Tracks aggregated token usage across turns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AggregatedUsage {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub turns: u32,
}

impl AggregatedUsage {
    pub fn record(&mut self, input: u64, output: u64) {
        self.total_input_tokens += input;
        self.total_output_tokens += output;
        self.turns += 1;
    }

    pub fn total(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tool Result Gate
// ────────────────────────────────────────────────────────────────────────────

/// Decision for how to handle a tool result in the next turn.
#[derive(Debug, Clone)]
pub enum ToolResultDecision {
    /// Include the full result inline.
    Inline { content: String },
    /// Truncate to fit within budget.
    Truncated {
        content: String,
        original_tokens: u32,
    },
    /// Use an artifact reference instead of inline content.
    ArtifactRef { ref_id: String, summary: String },
}

/// Gates tool results based on token budget.
pub struct ToolResultGate<'a> {
    config: &'a TokenBudgetConfig,
    estimator: &'a dyn TokenEstimator,
}

impl<'a> ToolResultGate<'a> {
    pub fn new(config: &'a TokenBudgetConfig, estimator: &'a dyn TokenEstimator) -> Self {
        Self { config, estimator }
    }

    /// Decide how to include a tool result.
    pub fn decide(&self, result: &str) -> ToolResultDecision {
        let tokens = self.estimator.estimate(result);
        if tokens <= self.config.tool_result_inline_limit {
            ToolResultDecision::Inline {
                content: result.to_string(),
            }
        } else {
            // Truncate to fit within limit
            let limit_chars = (self.config.tool_result_inline_limit * 4) as usize;
            let truncated = if result.len() > limit_chars {
                format!(
                    "{}...(truncated, {} tokens)",
                    &result[..limit_chars],
                    tokens
                )
            } else {
                result.to_string()
            };
            ToolResultDecision::Truncated {
                content: truncated,
                original_tokens: tokens,
            }
        }
    }

    /// Gate a list of tool results, returning decisions.
    pub fn gate_results(&self, results: &[(&str, &str)]) -> Vec<(String, ToolResultDecision)> {
        results
            .iter()
            .map(|(id, content)| (id.to_string(), self.decide(content)))
            .collect()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tool Schema Selector
// ────────────────────────────────────────────────────────────────────────────

/// Selects which tool schemas to include based on budget.
pub struct ToolSchemaSelector<'a> {
    config: &'a TokenBudgetConfig,
    estimator: &'a dyn TokenEstimator,
}

impl<'a> ToolSchemaSelector<'a> {
    pub fn new(config: &'a TokenBudgetConfig, estimator: &'a dyn TokenEstimator) -> Self {
        Self { config, estimator }
    }

    /// Select tools that fit within the schema budget.
    /// Returns (selected tools, total tokens used).
    pub fn select(
        &self,
        tools: &[(String, String, String)],
    ) -> (Vec<(String, String, String)>, u32) {
        let mut selected = Vec::new();
        let mut total_tokens = 0u32;

        for (name, desc, schema) in tools {
            let text = format!("{name} {desc} {schema}");
            let tokens = self.estimator.estimate(&text);
            if total_tokens + tokens <= self.config.tool_schema_budget {
                selected.push((name.clone(), desc.clone(), schema.clone()));
                total_tokens += tokens;
            }
        }

        (selected, total_tokens)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Context Dedupe Report
// ────────────────────────────────────────────────────────────────────────────

/// Report from context deduplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextDedupeReport {
    pub messages_before: usize,
    pub messages_after: usize,
    pub tokens_saved: u32,
}

/// Deduplicates context messages.
pub struct ContextDedupe;

impl ContextDedupe {
    /// Deduplicate consecutive identical messages.
    pub fn deduplicate(
        messages: &[(String, String)],
        estimator: &dyn TokenEstimator,
    ) -> (Vec<(String, String)>, ContextDedupeReport) {
        let mut result = Vec::new();
        let mut tokens_saved = 0u32;

        for (role, text) in messages {
            if let Some((_, prev_text)) = result.last()
                && prev_text == text
            {
                tokens_saved += estimator.estimate(text);
                continue;
            }
            result.push((role.clone(), text.clone()));
        }

        let messages_after = result.len();

        (
            result,
            ContextDedupeReport {
                messages_before: messages.len(),
                messages_after,
                tokens_saved,
            },
        )
    }
}

// ────────────────────────────────────────────────────────────────────────────
// History Compaction
// ────────────────────────────────────────────────────────────────────────────

/// Result of history compaction.
#[derive(Debug, Clone)]
pub struct CompactedHistory {
    pub messages: Vec<(String, String)>,
    pub removed_count: usize,
    pub tokens_removed: u32,
}

/// Compacts history to fit within budget.
pub struct HistoryCompaction<'a> {
    config: &'a TokenBudgetConfig,
    estimator: &'a dyn TokenEstimator,
}

impl<'a> HistoryCompaction<'a> {
    pub fn new(config: &'a TokenBudgetConfig, estimator: &'a dyn TokenEstimator) -> Self {
        Self { config, estimator }
    }

    /// Compact history by removing oldest messages until within budget.
    pub fn compact(&self, messages: &[(String, String)]) -> CompactedHistory {
        let total_tokens: u32 = messages
            .iter()
            .map(|(_, t)| self.estimator.estimate(t))
            .sum();

        if total_tokens <= self.config.history_budget {
            return CompactedHistory {
                messages: messages.to_vec(),
                removed_count: 0,
                tokens_removed: 0,
            };
        }

        // Remove oldest messages first (keep recent context)
        let mut current_tokens = total_tokens;
        let mut remove_count = 0;

        for (_, text) in messages {
            if current_tokens <= self.config.history_budget {
                break;
            }
            let tokens = self.estimator.estimate(text);
            current_tokens = current_tokens.saturating_sub(tokens);
            remove_count += 1;
        }

        let removed_tokens: u32 = messages[..remove_count]
            .iter()
            .map(|(_, t)| self.estimator.estimate(t))
            .sum();

        CompactedHistory {
            messages: messages[remove_count..].to_vec(),
            removed_count: remove_count,
            tokens_removed: removed_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TokenEstimator ──

    #[test]
    fn simple_estimator_basic() {
        let est = SimpleTokenEstimator;
        // 4 chars ≈ 1 token
        assert_eq!(est.estimate("test"), 1);
        assert_eq!(est.estimate("hello world"), 3); // 11 chars / 4 = 3 (rounded up)
        assert_eq!(est.estimate(""), 0);
    }

    #[test]
    fn simple_estimator_long_text() {
        let est = SimpleTokenEstimator;
        let text = "a".repeat(1000);
        assert_eq!(est.estimate(&text), 250);
    }

    // ── AggregatedUsage ──

    #[test]
    fn usage_aggregation() {
        let mut usage = AggregatedUsage::default();
        usage.record(100, 50);
        usage.record(200, 100);
        assert_eq!(usage.total_input_tokens, 300);
        assert_eq!(usage.total_output_tokens, 150);
        assert_eq!(usage.turns, 2);
        assert_eq!(usage.total(), 450);
    }

    // ── ToolResultGate ──

    #[test]
    fn small_result_inlined() {
        let config = TokenBudgetConfig {
            tool_result_inline_limit: 100,
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let gate = ToolResultGate::new(&config, &est);

        match gate.decide("short result") {
            ToolResultDecision::Inline { content } => assert_eq!(content, "short result"),
            _ => panic!("expected inline"),
        }
    }

    #[test]
    fn large_result_truncated() {
        let config = TokenBudgetConfig {
            tool_result_inline_limit: 10, // 10 tokens ≈ 40 chars
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let gate = ToolResultGate::new(&config, &est);

        let long_result = "x".repeat(200); // 50 tokens
        match gate.decide(&long_result) {
            ToolResultDecision::Truncated {
                content,
                original_tokens,
            } => {
                assert!(content.len() < long_result.len());
                assert_eq!(original_tokens, 50);
            }
            _ => panic!("expected truncated"),
        }
    }

    #[test]
    fn gate_results_batch() {
        let config = TokenBudgetConfig::default();
        let est = SimpleTokenEstimator;
        let gate = ToolResultGate::new(&config, &est);

        let results = vec![("r1", "short"), ("r2", "a much longer result here")];
        let decisions = gate.gate_results(&results);
        assert_eq!(decisions.len(), 2);
    }

    // ── ToolSchemaSelector ──

    #[test]
    fn selector_within_budget() {
        let config = TokenBudgetConfig {
            tool_schema_budget: 1000,
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let selector = ToolSchemaSelector::new(&config, &est);

        let tools = vec![
            ("tool1".into(), "desc1".into(), "{}".into()),
            ("tool2".into(), "desc2".into(), "{}".into()),
        ];
        let (selected, tokens) = selector.select(&tools);
        assert_eq!(selected.len(), 2);
        assert!(tokens <= 1000);
    }

    #[test]
    fn selector_respects_budget() {
        let config = TokenBudgetConfig {
            tool_schema_budget: 2, // very small budget
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let selector = ToolSchemaSelector::new(&config, &est);

        let tools = vec![
            ("tool1".into(), "description1".into(), "{}".into()),
            ("tool2".into(), "description2".into(), "{}".into()),
        ];
        let (selected, _) = selector.select(&tools);
        assert!(selected.len() < 2);
    }

    // ── ContextDedupe ──

    #[test]
    fn dedupe_removes_duplicates() {
        let est = SimpleTokenEstimator;
        let messages = vec![
            ("user".into(), "hello".into()),
            ("user".into(), "hello".into()), // consecutive duplicate
            ("assistant".into(), "hi".into()),
        ];
        let (deduped, report) = ContextDedupe::deduplicate(&messages, &est);
        assert_eq!(deduped.len(), 2);
        assert_eq!(report.messages_before, 3);
        assert_eq!(report.messages_after, 2);
        assert!(report.tokens_saved > 0);
    }

    #[test]
    fn dedupe_no_duplicates() {
        let est = SimpleTokenEstimator;
        let messages = vec![
            ("user".into(), "hello".into()),
            ("assistant".into(), "hi".into()),
        ];
        let (deduped, report) = ContextDedupe::deduplicate(&messages, &est);
        assert_eq!(deduped.len(), 2);
        assert_eq!(report.tokens_saved, 0);
    }

    // ── HistoryCompaction ──

    #[test]
    fn compaction_within_budget() {
        let config = TokenBudgetConfig {
            history_budget: 10000,
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let compaction = HistoryCompaction::new(&config, &est);

        let messages = vec![
            ("user".into(), "hello".into()),
            ("assistant".into(), "hi".into()),
        ];
        let result = compaction.compact(&messages);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.removed_count, 0);
    }

    #[test]
    fn compaction_removes_oldest() {
        let config = TokenBudgetConfig {
            history_budget: 10, // 10 tokens ≈ 40 chars
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let compaction = HistoryCompaction::new(&config, &est);

        let messages = vec![
            ("user".into(), "first message from user".into()), // ~6 tokens
            ("assistant".into(), "first response from assistant".into()), // ~7 tokens
            ("user".into(), "second message".into()),          // ~4 tokens
        ];
        let result = compaction.compact(&messages);
        // Total is ~17 tokens, budget is 10
        // Remove oldest: first (6 tokens) -> remaining 11
        // Remove oldest: second (7 tokens) -> remaining 4
        // Now within budget
        assert!(result.removed_count > 0);
        assert!(result.tokens_removed > 0);
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn tool_result_decision_artifact_ref() {
        let config = TokenBudgetConfig {
            tool_result_inline_limit: 10,
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let gate = ToolResultGate::new(&config, &est);

        let long = "x".repeat(200);
        match gate.decide(&long) {
            ToolResultDecision::Truncated {
                original_tokens, ..
            } => {
                assert!(original_tokens > 10);
            }
            _ => panic!("expected truncated for long content"),
        }
    }

    #[test]
    fn usage_serialization_roundtrip() {
        let usage = AggregatedUsage {
            total_input_tokens: 1000,
            total_output_tokens: 500,
            turns: 3,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: AggregatedUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_input_tokens, 1000);
        assert_eq!(back.turns, 3);
    }

    #[test]
    fn compacted_history_preserves_order() {
        let config = TokenBudgetConfig {
            history_budget: 10000,
            ..Default::default()
        };
        let est = SimpleTokenEstimator;
        let compaction = HistoryCompaction::new(&config, &est);

        let messages = vec![
            ("user".into(), "a".into()),
            ("assistant".into(), "b".into()),
            ("user".into(), "c".into()),
            ("assistant".into(), "d".into()),
        ];
        let result = compaction.compact(&messages);
        assert_eq!(result.messages[0].1, "a");
        assert_eq!(result.messages[3].1, "d");
    }
}
