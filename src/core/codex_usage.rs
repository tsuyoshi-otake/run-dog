//! Normalization of Codex's per-turn `last_token_usage` fields.

use super::TokenUsage;

/// Last-turn token fields from a Codex `token_count` event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodexTokenTotals {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

impl CodexTokenTotals {
    #[must_use]
    pub fn to_usage(self) -> TokenUsage {
        let cached = self.cached.min(self.input);
        TokenUsage {
            input: self.input - cached,
            cached_input: cached,
            output: self.output,
            ..TokenUsage::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CodexTokenTotals;
    use crate::core::TokenUsage;

    #[test]
    fn component_last_turn_usage_caps_cached_input_at_raw_input() {
        assert_eq!(
            CodexTokenTotals {
                input: 10,
                cached: 12,
                output: 3,
            }
            .to_usage(),
            TokenUsage {
                input: 0,
                cached_input: 10,
                output: 3,
                ..TokenUsage::default()
            }
        );
    }
}
