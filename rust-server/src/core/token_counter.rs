//! Outbound Token Counter
//!
//! Unified token counting component for calculating and injecting usage
//! at the outbound stage (when sending response to client).

use crate::api::streaming::count_tokens;
use crate::transformer::unified::UnifiedUsage;

/// Outbound Token Counter for unified token calculation
///
/// This component is responsible for:
/// 1. Accumulating output content during streaming
/// 2. Calculating output_tokens at stream end
/// 3. Providing final usage with fallback calculation
#[derive(Debug, Clone)]
pub struct OutboundTokenCounter {
    /// Model name for token calculation
    model: String,
    /// Pre-calculated input tokens (from inbound)
    input_tokens: i32,
    /// Accumulated output content for token calculation
    output_content: String,
    /// Provider-reported usage (if available)
    provider_usage: Option<UnifiedUsage>,
}

impl OutboundTokenCounter {
    /// Create a new OutboundTokenCounter with pre-calculated input tokens
    pub fn new(model: impl Into<String>, input_tokens: i32) -> Self {
        Self {
            model: model.into(),
            input_tokens,
            output_content: String::new(),
            provider_usage: None,
        }
    }

    /// Create a new OutboundTokenCounter without pre-calculated input tokens
    /// Input tokens will be calculated from messages at finalize time
    pub fn new_lazy(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input_tokens: 0,
            output_content: String::new(),
            provider_usage: None,
        }
    }

    /// Accumulate output content for token calculation
    pub fn accumulate_content(&mut self, content: &str) {
        self.output_content.push_str(content);
    }

    /// Set provider-reported usage
    pub fn set_provider_usage(&mut self, usage: UnifiedUsage) {
        self.provider_usage = Some(usage);
    }

    /// Update provider usage if it has meaningful values
    pub fn update_provider_usage(&mut self, usage: &UnifiedUsage) {
        if usage.input_tokens > 0 || usage.output_tokens > 0 {
            self.provider_usage = Some(usage.clone());
        }
    }

    /// Calculate output tokens from accumulated content
    pub fn calculate_output_tokens(&self) -> i32 {
        if self.output_content.is_empty() {
            return 0;
        }
        count_tokens(&self.output_content, &self.model) as i32
    }

    /// Get final usage with fallback calculation
    ///
    /// Priority:
    /// 1. Provider usage (if meaningful - non-zero)
    /// 2. Calculated usage (input_tokens from inbound + output_tokens from accumulated content)
    pub fn finalize(&self) -> UnifiedUsage {
        // Check if provider usage is meaningful
        if let Some(ref usage) = self.provider_usage {
            if usage.input_tokens > 0 || usage.output_tokens > 0 {
                return usage.clone();
            }
        }

        // Calculate fallback usage
        let output_tokens = self.calculate_output_tokens();

        UnifiedUsage {
            input_tokens: self.input_tokens,
            output_tokens,
            ..Default::default()
        }
    }

    /// Get final usage with custom input tokens (for lazy calculation)
    pub fn finalize_with_input(&self, input_tokens: i32) -> UnifiedUsage {
        // Check if provider usage is meaningful
        if let Some(ref usage) = self.provider_usage {
            if usage.input_tokens > 0 || usage.output_tokens > 0 {
                return usage.clone();
            }
        }

        // Calculate fallback usage
        let output_tokens = self.calculate_output_tokens();

        UnifiedUsage {
            input_tokens,
            output_tokens,
            ..Default::default()
        }
    }

    /// Get the model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the accumulated output content
    pub fn output_content(&self) -> &str {
        &self.output_content
    }

    /// Get the pre-calculated input tokens
    pub fn input_tokens(&self) -> i32 {
        self.input_tokens
    }

    /// Check if provider usage has been set
    pub fn has_provider_usage(&self) -> bool {
        self.provider_usage.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_with_input_tokens() {
        let counter = OutboundTokenCounter::new("gpt-4", 100);
        assert_eq!(counter.model(), "gpt-4");
        assert_eq!(counter.input_tokens(), 100);
        assert!(counter.output_content().is_empty());
    }

    #[test]
    fn test_new_lazy() {
        let counter = OutboundTokenCounter::new_lazy("gpt-4");
        assert_eq!(counter.model(), "gpt-4");
        assert_eq!(counter.input_tokens(), 0);
        assert!(counter.output_content().is_empty());
    }

    #[test]
    fn test_accumulate_content() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.accumulate_content("Hello ");
        counter.accumulate_content("World!");
        assert_eq!(counter.output_content(), "Hello World!");
    }

    #[test]
    fn test_finalize_with_provider_usage() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.accumulate_content("Hello World!");
        counter.set_provider_usage(UnifiedUsage {
            input_tokens: 50,
            output_tokens: 10,
            ..Default::default()
        });

        let usage = counter.finalize();
        assert_eq!(usage.input_tokens, 50);
        assert_eq!(usage.output_tokens, 10);
    }

    #[test]
    fn test_finalize_with_zero_provider_usage_uses_fallback() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.accumulate_content("Hello World!");
        counter.set_provider_usage(UnifiedUsage {
            input_tokens: 0,
            output_tokens: 0,
            ..Default::default()
        });

        let usage = counter.finalize();
        assert_eq!(usage.input_tokens, 100);
        assert!(usage.output_tokens > 0); // Should be calculated from content
    }

    #[test]
    fn test_finalize_without_provider_usage() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.accumulate_content("Hello World!");

        let usage = counter.finalize();
        assert_eq!(usage.input_tokens, 100);
        assert!(usage.output_tokens > 0); // Should be calculated from content
    }

    #[test]
    fn test_finalize_with_input() {
        let mut counter = OutboundTokenCounter::new_lazy("gpt-4");
        counter.accumulate_content("Hello World!");

        let usage = counter.finalize_with_input(200);
        assert_eq!(usage.input_tokens, 200);
        assert!(usage.output_tokens > 0);
    }

    #[test]
    fn test_update_provider_usage_ignores_zero() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.update_provider_usage(&UnifiedUsage {
            input_tokens: 0,
            output_tokens: 0,
            ..Default::default()
        });

        assert!(!counter.has_provider_usage());
    }

    #[test]
    fn test_update_provider_usage_accepts_nonzero() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.update_provider_usage(&UnifiedUsage {
            input_tokens: 50,
            output_tokens: 10,
            ..Default::default()
        });

        assert!(counter.has_provider_usage());
        let usage = counter.finalize();
        assert_eq!(usage.input_tokens, 50);
    }

    #[test]
    fn test_calculate_output_tokens_empty() {
        let counter = OutboundTokenCounter::new("gpt-4", 100);
        assert_eq!(counter.calculate_output_tokens(), 0);
    }

    #[test]
    fn test_calculate_output_tokens_with_content() {
        let mut counter = OutboundTokenCounter::new("gpt-4", 100);
        counter.accumulate_content("Hello World!");
        assert!(counter.calculate_output_tokens() > 0);
    }
}
