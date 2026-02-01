//! Integration tests for tokenizer support
//!
//! These tests verify the tokenizer selection and token counting functionality
//! for different model types (OpenAI, Claude)

use llm_proxy_rust::core::tokenizer::{
    count_tokens_hf, get_hf_tokenizer, get_tokenizer_info, select_tokenizer, TokenizerType,
};

#[test]
fn test_tokenizer_selection_openai_models() {
    // OpenAI models should use tiktoken
    let models = vec![
        "gpt-4",
        "gpt-4-turbo",
        "gpt-4o",
        "gpt-3.5-turbo",
        "gpt-35-turbo",
        "o1-preview",
        "o1-mini",
        "o3-mini",
    ];

    for model in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type,
            TokenizerType::Tiktoken,
            "Model {} should use tiktoken",
            model
        );
        assert!(
            selection.hf_repo.is_none(),
            "Model {} should not have HF repo",
            model
        );
    }
}

#[test]
fn test_tokenizer_selection_claude_models() {
    // All Claude models should use embedded HuggingFace tokenizer
    let models = vec![
        "claude-3-opus",
        "claude-3-sonnet",
        "claude-3-haiku",
        "claude-3-5-sonnet",
        "claude-opus-4-5",
    ];

    for model in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type,
            TokenizerType::HuggingFace,
            "Model {} should use HuggingFace",
            model
        );
        assert_eq!(
            selection.hf_repo,
            Some("__embedded_claude__".to_string()),
            "Model {} should use embedded Claude tokenizer",
            model
        );
    }
}

#[test]
fn test_tokenizer_selection_claude_bedrock_models() {
    // Claude models with -bedrock suffix should use HuggingFace tokenizer (embedded)
    let models = vec![
        "claude-3-opus-bedrock",
        "claude-3-sonnet-bedrock",
        "claude-3-haiku-bedrock",
        "claude-3-5-sonnet-bedrock",
        "claude-opus-4-5-bedrock",
    ];

    for model in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type,
            TokenizerType::HuggingFace,
            "Model {} should use HuggingFace",
            model
        );
        assert_eq!(
            selection.hf_repo,
            Some("__embedded_claude__".to_string()),
            "Model {} should use embedded Claude tokenizer",
            model
        );
    }
}

#[test]
fn test_tokenizer_selection_claude_vertex_models() {
    // Claude models with -vertex suffix should use HuggingFace tokenizer (embedded)
    let models = vec![
        "claude-3-opus-vertex",
        "claude-3-sonnet-vertex",
        "claude-3-haiku-vertex",
        "claude-3-5-sonnet-vertex",
        "claude-opus-4-5-vertex",
    ];

    for model in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type,
            TokenizerType::HuggingFace,
            "Model {} should use HuggingFace",
            model
        );
        assert_eq!(
            selection.hf_repo,
            Some("__embedded_claude__".to_string()),
            "Model {} should use embedded Claude tokenizer",
            model
        );
    }
}

#[test]
fn test_tokenizer_selection_claude_case_insensitive() {
    // Claude model matching should be case-insensitive
    let models = vec![
        ("CLAUDE-3-OPUS", TokenizerType::HuggingFace),
        ("Claude-3-Sonnet", TokenizerType::HuggingFace),
        ("claude-3-haiku", TokenizerType::HuggingFace),
        ("CLAUDE-3-OPUS-BEDROCK", TokenizerType::HuggingFace),
        ("Claude-3-Sonnet-Bedrock", TokenizerType::HuggingFace),
        ("claude-3-haiku-VERTEX", TokenizerType::HuggingFace),
        ("Claude-3-5-Sonnet-Vertex", TokenizerType::HuggingFace),
    ];

    for (model, expected_type) in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type, expected_type,
            "Model {} should use {:?}",
            model, expected_type
        );
    }
}

#[test]
fn test_tokenizer_selection_unknown_models() {
    // Unknown models should fall back to tiktoken with gpt-3.5-turbo encoding
    let models = vec!["unknown-model", "custom-model", "my-fine-tuned-model"];

    for model in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type,
            TokenizerType::Tiktoken,
            "Model {} should use tiktoken",
            model
        );
        assert_eq!(
            selection.tiktoken_model,
            Some("gpt-3.5-turbo".to_string()),
            "Model {} should use gpt-3.5-turbo encoding",
            model
        );
    }
}

#[test]
fn test_tokenizer_info() {
    // Test tokenizer info for different models
    let info = get_tokenizer_info("gpt-4");
    assert!(
        info.contains("tiktoken"),
        "GPT-4 info should mention tiktoken"
    );

    let info = get_tokenizer_info("claude-3-opus");
    assert!(
        info.contains("HuggingFace"),
        "Claude info should mention HuggingFace"
    );
    assert!(
        info.contains("__embedded_claude__"),
        "Claude info should mention embedded tokenizer"
    );
}

#[test]
fn test_case_insensitive_model_matching() {
    // Model matching should be case-insensitive
    let models = vec![
        ("GPT-4", TokenizerType::Tiktoken),
        ("gpt-4", TokenizerType::Tiktoken),
        ("CLAUDE-3-OPUS", TokenizerType::HuggingFace),
        ("claude-3-opus", TokenizerType::HuggingFace),
        ("CLAUDE-3-OPUS-BEDROCK", TokenizerType::HuggingFace),
        ("claude-3-opus-bedrock", TokenizerType::HuggingFace),
    ];

    for (model, expected_type) in models {
        let selection = select_tokenizer(model);
        assert_eq!(
            selection.tokenizer_type, expected_type,
            "Model {} should use {:?}",
            model, expected_type
        );
    }
}

// Test embedded Claude tokenizer loading and counting (no network required)
#[test]
fn test_embedded_claude_tokenizer_loading_and_counting() {
    // This test uses the embedded Claude tokenizer, no network required
    let repo = "__embedded_claude__";
    let tokenizer = get_hf_tokenizer(repo);

    assert!(
        tokenizer.is_some(),
        "Embedded Claude tokenizer should load successfully"
    );

    if let Some(tokenizer) = tokenizer {
        let text = "Hello, world! This is a test.";
        let count = count_tokens_hf(text, &tokenizer);
        assert!(count > 0, "Token count should be greater than 0");
        println!(
            "Embedded Claude tokenizer - Token count for '{}': {}",
            text, count
        );

        // Test with longer text
        let long_text = "The quick brown fox jumps over the lazy dog. This is a longer sentence to test the tokenizer with more content.";
        let long_count = count_tokens_hf(long_text, &tokenizer);
        assert!(
            long_count > count,
            "Longer text should have more tokens: {} vs {}",
            long_count,
            count
        );
        println!(
            "Embedded Claude tokenizer - Token count for longer text: {}",
            long_count
        );
    }
}

#[test]
fn test_unknown_hf_repo_returns_none() {
    // Unknown HF repo should return None
    let tokenizer = get_hf_tokenizer("unknown-repo");
    assert!(tokenizer.is_none(), "Unknown repo should return None");
}
