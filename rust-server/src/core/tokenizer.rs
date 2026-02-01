//! Tokenizer Selection Module
//!
//! This module provides a unified interface for token counting across different
//! LLM providers. It supports:
//! - tiktoken (OpenAI models, default fallback)
//! - HuggingFace tokenizer (Claude models with -bedrock/-vertex suffix)
//!
//! Claude tokenizer is embedded in the binary for offline usage.

use once_cell::sync::Lazy;
use std::sync::Arc;
use tokenizers::Tokenizer as HfTokenizer;
use tracing::{debug, warn};

/// Embedded Anthropic Claude tokenizer JSON (from litellm)
/// This allows Claude tokenization without network access
const EMBEDDED_CLAUDE_TOKENIZER_JSON: &str = include_str!("tokenizers/anthropic_tokenizer.json");

/// Special marker for embedded Claude tokenizer
const CLAUDE_EMBEDDED_MARKER: &str = "__embedded_claude__";

/// Tokenizer type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerType {
    /// OpenAI tiktoken tokenizer
    Tiktoken,
    /// HuggingFace tokenizer (embedded Claude)
    HuggingFace,
}

/// Tokenizer selection result
#[derive(Debug)]
pub struct TokenizerSelection {
    /// Type of tokenizer to use
    pub tokenizer_type: TokenizerType,
    /// HuggingFace repo name (if applicable)
    pub hf_repo: Option<String>,
    /// Model name for tiktoken (if applicable)
    pub tiktoken_model: Option<String>,
}

/// Cached embedded Claude tokenizer (loaded once on first use)
static EMBEDDED_CLAUDE_TOKENIZER: Lazy<Option<Arc<HfTokenizer>>> = Lazy::new(|| {
    debug!("Loading embedded Claude tokenizer from binary");
    match HfTokenizer::from_bytes(EMBEDDED_CLAUDE_TOKENIZER_JSON.as_bytes()) {
        Ok(tokenizer) => {
            debug!("Successfully loaded embedded Claude tokenizer");
            Some(Arc::new(tokenizer))
        }
        Err(e) => {
            warn!(
                "Failed to load embedded Claude tokenizer: {}. Falling back to tiktoken.",
                e
            );
            None
        }
    }
});

/// Select the appropriate tokenizer for a given model
///
/// # Arguments
/// * `model` - The model name (e.g., "claude-3-opus", "gpt-4")
///
/// # Returns
/// A `TokenizerSelection` indicating which tokenizer to use
///
/// # Claude Model Handling
/// - All Claude models (containing "claude") use embedded Claude tokenizer
/// - All other models use tiktoken as fallback
pub fn select_tokenizer(model: &str) -> TokenizerSelection {
    let model_lower = model.to_lowercase();

    // All Claude models use Anthropic's official tokenizer (embedded)
    if model_lower.contains("claude") {
        return TokenizerSelection {
            tokenizer_type: TokenizerType::HuggingFace,
            hf_repo: Some(CLAUDE_EMBEDDED_MARKER.to_string()),
            tiktoken_model: None,
        };
    }

    // Default to tiktoken for all other models (OpenAI, etc.)
    TokenizerSelection {
        tokenizer_type: TokenizerType::Tiktoken,
        hf_repo: None,
        tiktoken_model: Some(normalize_tiktoken_model(model)),
    }
}

/// Normalize model name for tiktoken
fn normalize_tiktoken_model(model: &str) -> String {
    if model.contains("gpt-35") {
        model.replace("-35", "-3.5")
    } else if model.starts_with("gpt-") || model.starts_with("o1") || model.starts_with("o3") {
        model.to_string()
    } else {
        // Default to gpt-3.5-turbo encoding for unknown models
        "gpt-3.5-turbo".to_string()
    }
}

/// Get the embedded Claude tokenizer
///
/// # Arguments
/// * `repo` - The HuggingFace repository name (must be `__embedded_claude__`)
///
/// # Returns
/// An `Option<Arc<HfTokenizer>>` if the tokenizer was loaded successfully
pub fn get_hf_tokenizer(repo: &str) -> Option<Arc<HfTokenizer>> {
    if repo == CLAUDE_EMBEDDED_MARKER {
        EMBEDDED_CLAUDE_TOKENIZER.clone()
    } else {
        warn!(
            "Unknown tokenizer repo: {}. Only embedded Claude tokenizer is supported.",
            repo
        );
        None
    }
}

/// Count tokens using HuggingFace tokenizer
///
/// # Arguments
/// * `text` - The text to tokenize
/// * `tokenizer` - The HuggingFace tokenizer
///
/// # Returns
/// The number of tokens
pub fn count_tokens_hf(text: &str, tokenizer: &HfTokenizer) -> usize {
    match tokenizer.encode(text, false) {
        Ok(encoding) => encoding.get_ids().len(),
        Err(e) => {
            warn!("HuggingFace tokenization failed: {}. Returning 0.", e);
            0
        }
    }
}

/// Get tokenizer info for a model (useful for debugging/logging)
pub fn get_tokenizer_info(model: &str) -> String {
    let selection = select_tokenizer(model);
    match selection.tokenizer_type {
        TokenizerType::HuggingFace => {
            format!(
                "HuggingFace ({})",
                selection.hf_repo.unwrap_or_else(|| "unknown".to_string())
            )
        }
        TokenizerType::Tiktoken => {
            format!(
                "tiktoken ({})",
                selection
                    .tiktoken_model
                    .unwrap_or_else(|| "default".to_string())
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_tokenizer_openai() {
        let selection = select_tokenizer("gpt-4");
        assert_eq!(selection.tokenizer_type, TokenizerType::Tiktoken);
        assert!(selection.hf_repo.is_none());
        assert_eq!(selection.tiktoken_model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_select_tokenizer_gpt35() {
        let selection = select_tokenizer("gpt-35-turbo");
        assert_eq!(selection.tokenizer_type, TokenizerType::Tiktoken);
        assert_eq!(selection.tiktoken_model, Some("gpt-3.5-turbo".to_string()));
    }

    #[test]
    fn test_select_tokenizer_unknown() {
        let selection = select_tokenizer("unknown-model");
        assert_eq!(selection.tokenizer_type, TokenizerType::Tiktoken);
        assert_eq!(selection.tiktoken_model, Some("gpt-3.5-turbo".to_string()));
    }

    #[test]
    fn test_select_tokenizer_claude() {
        // All Claude models should use embedded HuggingFace tokenizer
        let selection = select_tokenizer("claude-3-opus");
        assert_eq!(selection.tokenizer_type, TokenizerType::HuggingFace);
        assert_eq!(selection.hf_repo, Some("__embedded_claude__".to_string()));
    }

    #[test]
    fn test_select_tokenizer_claude_bedrock() {
        // Claude models with -bedrock suffix should use embedded HuggingFace tokenizer
        let selection = select_tokenizer("claude-3-opus-bedrock");
        assert_eq!(selection.tokenizer_type, TokenizerType::HuggingFace);
        assert_eq!(selection.hf_repo, Some("__embedded_claude__".to_string()));
    }

    #[test]
    fn test_select_tokenizer_claude_vertex() {
        // Claude models with -vertex suffix should use embedded HuggingFace tokenizer
        let selection = select_tokenizer("claude-3-sonnet-vertex");
        assert_eq!(selection.tokenizer_type, TokenizerType::HuggingFace);
        assert_eq!(selection.hf_repo, Some("__embedded_claude__".to_string()));
    }

    #[test]
    fn test_normalize_tiktoken_model() {
        assert_eq!(normalize_tiktoken_model("gpt-35-turbo"), "gpt-3.5-turbo");
        assert_eq!(normalize_tiktoken_model("gpt-4"), "gpt-4");
        assert_eq!(normalize_tiktoken_model("o1-preview"), "o1-preview");
        assert_eq!(normalize_tiktoken_model("unknown"), "gpt-3.5-turbo");
    }

    #[test]
    fn test_get_tokenizer_info() {
        let info = get_tokenizer_info("gpt-4");
        assert!(info.contains("tiktoken"));

        let info = get_tokenizer_info("claude-3-opus");
        assert!(info.contains("HuggingFace"));
    }
}
