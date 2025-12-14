//! Benchmarks for request handling performance.
//!
//! Run with: cargo bench --bench request_handling

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use llm_proxy_rust::{
    api::models::{ChatCompletionRequest, Message},
    core::config::{AppConfig, ProviderConfig, ServerConfig},
};
use std::collections::HashMap;

fn create_test_request(message_count: usize) -> ChatCompletionRequest {
    let messages: Vec<Message> = (0..message_count)
        .map(|i| Message {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: format!("Message content {}", i),
        })
        .collect();

    ChatCompletionRequest {
        model: "gpt-4".to_string(),
        messages,
        temperature: Some(0.7),
        max_tokens: Some(100),
        stream: Some(false),
        extra: HashMap::new(),
    }
}

fn bench_request_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_serialization");

    for message_count in [1, 5, 10, 20].iter() {
        let request = create_test_request(*message_count);

        group.throughput(Throughput::Elements(*message_count as u64));
        group.bench_function(format!("{}_messages", message_count), |b| {
            b.iter(|| {
                black_box(serde_json::to_string(&request).unwrap());
            });
        });
    }

    group.finish();
}

fn bench_request_deserialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_deserialization");

    for message_count in [1, 5, 10, 20].iter() {
        let request = create_test_request(*message_count);
        let json = serde_json::to_string(&request).unwrap();

        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_function(format!("{}_messages", message_count), |b| {
            b.iter(|| {
                black_box(serde_json::from_str::<ChatCompletionRequest>(&json).unwrap());
            });
        });
    }

    group.finish();
}

fn bench_config_loading(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_loading");

    let config_yaml = r#"
providers:
  - name: Provider1
    api_base: http://localhost:8000
    api_key: key1
    weight: 2
    model_mapping:
      gpt-4: test-gpt-4
      gpt-3.5-turbo: test-gpt-3.5

  - name: Provider2
    api_base: http://localhost:8001
    api_key: key2
    weight: 1
    model_mapping:
      claude-3: test-claude-3

server:
  host: 0.0.0.0
  port: 18000

verify_ssl: true
"#;

    group.bench_function("parse_yaml", |b| {
        b.iter(|| {
            black_box(serde_yaml::from_str::<AppConfig>(config_yaml).unwrap());
        });
    });

    group.finish();
}

fn bench_model_mapping_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("model_mapping_lookup");

    let mut model_mapping = HashMap::new();
    for i in 0..100 {
        model_mapping.insert(format!("model-{}", i), format!("mapped-{}", i));
    }

    group.bench_function("existing_key", |b| {
        b.iter(|| {
            black_box(model_mapping.get("model-50"));
        });
    });

    group.bench_function("missing_key", |b| {
        b.iter(|| {
            black_box(model_mapping.get("nonexistent"));
        });
    });

    group.finish();
}

fn bench_message_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_creation");

    group.bench_function("simple_message", |b| {
        b.iter(|| {
            black_box(Message {
                role: "user".to_string(),
                content: "Hello, world!".to_string(),
            });
        });
    });

    group.bench_function("long_message", |b| {
        let long_content = "a".repeat(1000);
        b.iter(|| {
            black_box(Message {
                role: "user".to_string(),
                content: long_content.clone(),
            });
        });
    });

    group.finish();
}

fn bench_json_value_manipulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_value_manipulation");

    let json_obj = serde_json::json!({
        "id": "test-123",
        "model": "gpt-4",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                }
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    });

    group.bench_function("clone_value", |b| {
        b.iter(|| {
            black_box(json_obj.clone());
        });
    });

    group.bench_function("get_nested_field", |b| {
        b.iter(|| {
            black_box(json_obj["choices"][0]["message"]["content"].as_str());
        });
    });

    group.bench_function("modify_field", |b| {
        b.iter(|| {
            let mut obj = json_obj.clone();
            if let Some(obj_map) = obj.as_object_mut() {
                obj_map.insert("model".to_string(), serde_json::json!("new-model"));
            }
            black_box(obj);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_request_serialization,
    bench_request_deserialization,
    bench_config_loading,
    bench_model_mapping_lookup,
    bench_message_creation,
    bench_json_value_manipulation,
);

criterion_main!(benches);