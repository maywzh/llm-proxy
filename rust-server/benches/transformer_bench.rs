//! Benchmarks for the transformer module.
//!
//! Run with: cargo bench --bench transformer_bench
//!
//! These benchmarks measure the performance of protocol transformations
//! between OpenAI, Anthropic, and Response API formats.

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_proxy_rust::transformer::{
    anthropic::AnthropicTransformer, openai::OpenAITransformer,
    response_api::ResponseApiTransformer, Protocol, Transformer, UnifiedMessage, UnifiedRequest,
    UnifiedResponse, UnifiedUsage,
};
use serde_json::json;

// ============================================================================
// Request Transformation Benchmarks
// ============================================================================

fn bench_openai_request_out(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();
    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello, how are you?"}
        ],
        "temperature": 0.7,
        "max_tokens": 1000
    });

    c.bench_function("openai_request_out", |b| {
        b.iter(|| transformer.transform_request_out(black_box(request.clone())))
    });
}

fn bench_openai_request_in(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();
    let unified = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello!")])
        .with_system("You are a helpful assistant.")
        .with_max_tokens(1000);

    c.bench_function("openai_request_in", |b| {
        b.iter(|| transformer.transform_request_in(black_box(&unified)))
    });
}

fn bench_anthropic_request_out(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();
    let request = json!({
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "system": "You are a helpful assistant.",
        "messages": [
            {"role": "user", "content": "Hello, how are you?"}
        ],
        "temperature": 0.7
    });

    c.bench_function("anthropic_request_out", |b| {
        b.iter(|| transformer.transform_request_out(black_box(request.clone())))
    });
}

fn bench_anthropic_request_in(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();
    let unified = UnifiedRequest::new("claude-3-opus", vec![UnifiedMessage::user("Hello!")])
        .with_system("You are a helpful assistant.")
        .with_max_tokens(1024);

    c.bench_function("anthropic_request_in", |b| {
        b.iter(|| transformer.transform_request_in(black_box(&unified)))
    });
}

fn bench_response_api_request_out(c: &mut Criterion) {
    let transformer = ResponseApiTransformer::new();
    let request = json!({
        "model": "gpt-4",
        "instructions": "You are a helpful assistant.",
        "input": "Hello, how are you?",
        "max_output_tokens": 1000
    });

    c.bench_function("response_api_request_out", |b| {
        b.iter(|| transformer.transform_request_out(black_box(request.clone())))
    });
}

fn bench_response_api_request_in(c: &mut Criterion) {
    let transformer = ResponseApiTransformer::new();
    let unified = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello!")])
        .with_system("You are a helpful assistant.")
        .with_max_tokens(1000);

    c.bench_function("response_api_request_in", |b| {
        b.iter(|| transformer.transform_request_in(black_box(&unified)))
    });
}

// ============================================================================
// Response Transformation Benchmarks
// ============================================================================

fn bench_openai_response_in(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();
    let response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1234567890,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! I'm doing well, thank you for asking."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 15,
            "total_tokens": 35
        }
    });

    c.bench_function("openai_response_in", |b| {
        b.iter(|| transformer.transform_response_in(black_box(response.clone()), "gpt-4"))
    });
}

fn bench_openai_response_out(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();
    let unified = UnifiedResponse::text(
        "chatcmpl-123",
        "gpt-4",
        "Hello! I'm doing well, thank you for asking.",
        UnifiedUsage::new(20, 15),
    );

    c.bench_function("openai_response_out", |b| {
        b.iter(|| {
            transformer.transform_response_out(black_box(&unified), black_box(Protocol::OpenAI))
        })
    });
}

fn bench_anthropic_response_in(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();
    let response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello! I'm doing well, thank you for asking."}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 20,
            "output_tokens": 15
        }
    });

    c.bench_function("anthropic_response_in", |b| {
        b.iter(|| transformer.transform_response_in(black_box(response.clone()), "claude-3-opus"))
    });
}

fn bench_anthropic_response_out(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();
    let unified = UnifiedResponse::text(
        "msg_123",
        "claude-3-opus",
        "Hello! I'm doing well, thank you for asking.",
        UnifiedUsage::new(20, 15),
    );

    c.bench_function("anthropic_response_out", |b| {
        b.iter(|| {
            transformer.transform_response_out(black_box(&unified), black_box(Protocol::Anthropic))
        })
    });
}

// ============================================================================
// Streaming Transformation Benchmarks
// ============================================================================

fn bench_openai_stream_chunk_in(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();
    let chunk = Bytes::from(
        r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

"#,
    );

    c.bench_function("openai_stream_chunk_in", |b| {
        b.iter(|| transformer.transform_stream_chunk_in(black_box(&chunk)))
    });
}

fn bench_anthropic_stream_chunk_in(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();
    let chunk = Bytes::from(
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

"#,
    );

    c.bench_function("anthropic_stream_chunk_in", |b| {
        b.iter(|| transformer.transform_stream_chunk_in(black_box(&chunk)))
    });
}

// ============================================================================
// Cross-Protocol Transformation Benchmarks
// ============================================================================

fn bench_cross_protocol_openai_to_anthropic(c: &mut Criterion) {
    let openai = OpenAITransformer::new();
    let anthropic = AnthropicTransformer::new();

    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello!"}
        ],
        "temperature": 0.7,
        "max_tokens": 1000
    });

    c.bench_function("cross_protocol_openai_to_anthropic", |b| {
        b.iter(|| {
            let unified = openai
                .transform_request_out(black_box(request.clone()))
                .unwrap();
            anthropic.transform_request_in(black_box(&unified))
        })
    });
}

fn bench_cross_protocol_anthropic_to_openai(c: &mut Criterion) {
    let anthropic = AnthropicTransformer::new();
    let openai = OpenAITransformer::new();

    let request = json!({
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "system": "You are a helpful assistant.",
        "messages": [
            {"role": "user", "content": "Hello!"}
        ],
        "temperature": 0.7
    });

    c.bench_function("cross_protocol_anthropic_to_openai", |b| {
        b.iter(|| {
            let unified = anthropic
                .transform_request_out(black_box(request.clone()))
                .unwrap();
            openai.transform_request_in(black_box(&unified))
        })
    });
}

// ============================================================================
// Message Size Scaling Benchmarks
// ============================================================================

fn bench_openai_request_scaling(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();

    let mut group = c.benchmark_group("openai_request_scaling");

    for message_count in [1, 5, 10, 20, 50].iter() {
        let messages: Vec<_> = (0..*message_count)
            .map(|i| {
                if i % 2 == 0 {
                    json!({"role": "user", "content": format!("Message {}", i)})
                } else {
                    json!({"role": "assistant", "content": format!("Response {}", i)})
                }
            })
            .collect();

        let request = json!({
            "model": "gpt-4",
            "messages": messages,
            "temperature": 0.7,
            "max_tokens": 1000
        });

        group.throughput(Throughput::Elements(*message_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(message_count),
            &request,
            |b, req| b.iter(|| transformer.transform_request_out(black_box(req.clone()))),
        );
    }

    group.finish();
}

fn bench_anthropic_request_scaling(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();

    let mut group = c.benchmark_group("anthropic_request_scaling");

    for message_count in [1, 5, 10, 20, 50].iter() {
        let messages: Vec<_> = (0..*message_count)
            .map(|i| {
                if i % 2 == 0 {
                    json!({"role": "user", "content": format!("Message {}", i)})
                } else {
                    json!({"role": "assistant", "content": format!("Response {}", i)})
                }
            })
            .collect();

        let request = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": messages,
            "temperature": 0.7
        });

        group.throughput(Throughput::Elements(*message_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(message_count),
            &request,
            |b, req| b.iter(|| transformer.transform_request_out(black_box(req.clone()))),
        );
    }

    group.finish();
}

// ============================================================================
// Tool Use Transformation Benchmarks
// ============================================================================

fn bench_openai_request_with_tools(c: &mut Criterion) {
    let transformer = OpenAITransformer::new();
    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "What's the weather in Tokyo?"}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the current weather in a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string", "description": "The city name"},
                        "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                    },
                    "required": ["location"]
                }
            }
        }],
        "tool_choice": "auto"
    });

    c.bench_function("openai_request_with_tools", |b| {
        b.iter(|| transformer.transform_request_out(black_box(request.clone())))
    });
}

fn bench_anthropic_request_with_tools(c: &mut Criterion) {
    let transformer = AnthropicTransformer::new();
    let request = json!({
        "model": "claude-3-opus",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "What's the weather in Tokyo?"}
        ],
        "tools": [{
            "name": "get_weather",
            "description": "Get the current weather in a location",
            "input_schema": {
                "type": "object",
                "properties": {
                    "location": {"type": "string", "description": "The city name"},
                    "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                },
                "required": ["location"]
            }
        }],
        "tool_choice": {"type": "auto"}
    });

    c.bench_function("anthropic_request_with_tools", |b| {
        b.iter(|| transformer.transform_request_out(black_box(request.clone())))
    });
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(
    request_benches,
    bench_openai_request_out,
    bench_openai_request_in,
    bench_anthropic_request_out,
    bench_anthropic_request_in,
    bench_response_api_request_out,
    bench_response_api_request_in,
);

criterion_group!(
    response_benches,
    bench_openai_response_in,
    bench_openai_response_out,
    bench_anthropic_response_in,
    bench_anthropic_response_out,
);

criterion_group!(
    streaming_benches,
    bench_openai_stream_chunk_in,
    bench_anthropic_stream_chunk_in,
);

criterion_group!(
    cross_protocol_benches,
    bench_cross_protocol_openai_to_anthropic,
    bench_cross_protocol_anthropic_to_openai,
);

criterion_group!(
    scaling_benches,
    bench_openai_request_scaling,
    bench_anthropic_request_scaling,
);

criterion_group!(
    tool_benches,
    bench_openai_request_with_tools,
    bench_anthropic_request_with_tools,
);

criterion_main!(
    request_benches,
    response_benches,
    streaming_benches,
    cross_protocol_benches,
    scaling_benches,
    tool_benches,
);
