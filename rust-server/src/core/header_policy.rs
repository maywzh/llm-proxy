use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnthropicBetaPolicy {
    Drop,
    Passthrough,
    Allowlist,
}

fn is_anthropic_provider(provider_type: &str) -> bool {
    provider_type.eq_ignore_ascii_case("anthropic") || provider_type.eq_ignore_ascii_case("claude")
}

fn is_gcp_vertex_provider(provider_type: &str) -> bool {
    provider_type.eq_ignore_ascii_case("gcp-vertex")
        || provider_type.eq_ignore_ascii_case("gcp_vertex")
        || provider_type.eq_ignore_ascii_case("vertex")
}

fn parse_policy(
    provider_params: &HashMap<String, Value>,
    default_policy: AnthropicBetaPolicy,
) -> AnthropicBetaPolicy {
    let policy = provider_params
        .get("anthropic_beta_policy")
        .and_then(|v| v.as_str())
        .map(|v| v.to_ascii_lowercase());

    match policy.as_deref() {
        Some("passthrough") => AnthropicBetaPolicy::Passthrough,
        Some("allowlist") => AnthropicBetaPolicy::Allowlist,
        Some("drop") => AnthropicBetaPolicy::Drop,
        _ => default_policy,
    }
}

fn parse_allowlist(provider_params: &HashMap<String, Value>) -> Vec<String> {
    let value = match provider_params.get("anthropic_beta_allowlist") {
        Some(value) => value,
        None => return Vec::new(),
    };

    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
            .collect(),
        Value::String(items) => items
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub fn sanitize_anthropic_beta_header(
    provider_type: &str,
    provider_params: &HashMap<String, Value>,
    header_value: Option<&str>,
) -> Option<String> {
    let header_value = header_value.map(|v| v.trim()).filter(|v| !v.is_empty())?;
    if !is_anthropic_provider(provider_type) && !is_gcp_vertex_provider(provider_type) {
        return None;
    }

    let policy = parse_policy(provider_params, AnthropicBetaPolicy::Drop);
    match policy {
        AnthropicBetaPolicy::Drop => None,
        AnthropicBetaPolicy::Passthrough => Some(header_value.to_string()),
        AnthropicBetaPolicy::Allowlist => {
            let allowlist = parse_allowlist(provider_params);
            if allowlist.is_empty() {
                return None;
            }
            let filtered: Vec<&str> = header_value
                .split(',')
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .filter(|v| allowlist.iter().any(|allowed| allowed == v))
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(filtered.join(","))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_sanitize_anthropic_beta_drop_default() {
        let params = HashMap::new();
        let result =
            sanitize_anthropic_beta_header("anthropic", &params, Some("feature-a,feature-b"));
        assert!(result.is_none());
    }

    #[test]
    fn test_sanitize_anthropic_beta_passthrough() {
        let mut params = HashMap::new();
        params.insert("anthropic_beta_policy".to_string(), json!("passthrough"));
        let result =
            sanitize_anthropic_beta_header("anthropic", &params, Some("feature-a, feature-b"));
        assert_eq!(result, Some("feature-a, feature-b".to_string()));
    }

    #[test]
    fn test_sanitize_anthropic_beta_allowlist() {
        let mut params = HashMap::new();
        params.insert("anthropic_beta_policy".to_string(), json!("allowlist"));
        params.insert(
            "anthropic_beta_allowlist".to_string(),
            json!(["feature-a", "feature-c"]),
        );
        let result =
            sanitize_anthropic_beta_header("anthropic", &params, Some("feature-a,feature-b"));
        assert_eq!(result, Some("feature-a".to_string()));
    }

    #[test]
    fn test_sanitize_anthropic_beta_non_anthropic() {
        let params = HashMap::new();
        let result = sanitize_anthropic_beta_header("openai", &params, Some("feature-a"));
        assert!(result.is_none());
    }
}
