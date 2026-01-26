# Transformer Pipeline Benchmarks

This document describes the benchmark suite for the transformer pipeline and provides performance analysis.

## Overview

The transformer pipeline supports two modes of operation:

1. **Bypass Mode**: When client and provider use the same protocol (e.g., OpenAI → OpenAI), the pipeline performs minimal transformation (only model name mapping).

2. **Full Transformation Mode**: When protocols differ (e.g., OpenAI → Anthropic), the pipeline performs complete protocol conversion through the Unified Internal Format (UIF).

## Running Benchmarks

```bash
# Run all benchmarks
cd rust-server && cargo bench

# Run specific benchmark group
cargo bench --bench transformer_bench -- request_transformation

# Run with custom sample size (faster, less accurate)
cargo bench --bench transformer_bench -- --sample-size 10

# View HTML reports
open target/criterion/report/index.html
```

## Benchmark Results

Results collected on Apple Silicon (M-series) with `--sample-size 10`:

### Request Transformation

| Benchmark | Time | Notes |
|-----------|------|-------|
| `bypass_openai_to_openai` | ~1.1 µs | Same protocol, bypass mode |
| `bypass_anthropic_to_anthropic` | ~1.1 µs | Same protocol, bypass mode |
| `transform_openai_to_anthropic` | ~3.8 µs | Cross-protocol transformation |
| `transform_anthropic_to_openai` | ~4.2 µs | Cross-protocol transformation |
| `transform_with_features` | ~3.9 µs | Same protocol with feature transformers |

**Key Finding**: Bypass mode is **~3.5x faster** than full transformation.

### Response Transformation

| Benchmark | Time | Notes |
|-----------|------|-------|
| `bypass_response_openai` | ~0.8 µs | Same protocol, bypass mode |
| `transform_response_anthropic_to_openai` | ~2.0 µs | Cross-protocol transformation |
| `transform_response_openai_to_anthropic` | ~2.1 µs | Cross-protocol transformation |

**Key Finding**: Response bypass is **~2.5x faster** than full transformation.

### Streaming Performance

| Benchmark | Time | Notes |
|-----------|------|-------|
| `stream_chunk_parse_openai` | ~445 ns | Parse OpenAI SSE chunk |
| `stream_chunk_parse_anthropic` | ~365 ns | Parse Anthropic SSE chunk |
| `stream_chunk_output_openai` | ~1.0 µs | Format chunk for OpenAI |
| `stream_chunk_output_anthropic` | ~0.9 µs | Format chunk for Anthropic |
| `stream_chunk_cross_protocol` | ~1.4 µs | Parse + transform + output |
| `stream_accumulator` (100 chunks) | ~3.0 µs | Accumulate streaming chunks |

**Key Finding**: Streaming chunk processing is highly efficient at **~30 ns per chunk** for accumulation.

### Payload Size Impact

| Payload Size | Bypass Request | Transform Request | Bypass Response | Transform Response |
|--------------|----------------|-------------------|-----------------|-------------------|
| 100 bytes | 394 ns | 1.27 µs | 830 ns | 1.96 µs |
| 1 KB | 428 ns | 1.32 µs | 813 ns | 1.96 µs |
| 10 KB | 546 ns | 1.98 µs | 890 ns | 2.46 µs |
| 100 KB | 2.39 µs | 27.3 µs | 2.57 µs | 26.2 µs |

**Key Finding**:
- Bypass mode scales excellently with payload size (sub-linear)
- Full transformation shows linear scaling with payload size
- At 100KB, bypass is **~11x faster** than full transformation

### Throughput (Payload Size Benchmarks)

| Payload Size | Bypass Throughput | Transform Throughput |
|--------------|-------------------|---------------------|
| 100 bytes | 242 MiB/s | 75 MiB/s |
| 1 KB | 2.2 GiB/s | 741 MiB/s |
| 10 KB | 17.5 GiB/s | 4.8 GiB/s |
| 100 KB | 40 GiB/s | 3.5 GiB/s |

### Message Count Impact

| Messages | Bypass | Transform | Ratio |
|----------|--------|-----------|-------|
| 1 | 397 ns | 1.25 µs | 3.1x |
| 5 | 931 ns | 3.29 µs | 3.5x |
| 10 | 1.61 µs | 5.80 µs | 3.6x |
| 20 | 2.93 µs | 10.6 µs | 3.6x |
| 50 | 7.18 µs | 25.5 µs | 3.5x |

**Key Finding**: Both modes scale linearly with message count, maintaining ~3.5x performance advantage for bypass.

### Individual Transformer Operations

| Operation | OpenAI | Anthropic |
|-----------|--------|-----------|
| `transform_request_out` | 2.25 µs | 2.24 µs |
| `transform_request_in` | 1.61 µs | 1.54 µs |
| `transform_response_in` | 0.99 µs | 0.88 µs |
| `transform_response_out` | 1.03 µs | 0.92 µs |

**Key Finding**: OpenAI and Anthropic transformers have comparable performance.

### Pipeline Overhead

| Operation | Time |
|-----------|------|
| Registry creation | 144 ns |
| Pipeline creation | 4.2 ns |
| Context creation | 63 ns |
| Protocol detection (OpenAI) | 44-67 ns |
| Protocol detection (Anthropic) | 18-33 ns |

**Key Finding**: Pipeline overhead is negligible (~200 ns total).

## Performance Recommendations

### 1. Use Bypass Mode When Possible

Configure providers with matching protocols to leverage bypass mode:

```yaml
# Optimal: OpenAI client → OpenAI provider (bypass)
providers:
  - provider_key: openai-main
    provider_type: openai
    api_base: https://api.openai.com/v1
```

### 2. Avoid Feature Transformers for Same-Protocol

Feature transformers disable bypass mode. Only use them when necessary:

```rust
// This disables bypass even for same-protocol
let pipeline = TransformPipeline::with_features(registry, features);
```

### 3. Consider Payload Size

For very large payloads (>10KB), the performance difference between bypass and full transformation becomes significant:

- **Bypass**: ~40 GiB/s throughput
- **Transform**: ~3.5 GiB/s throughput

### 4. Streaming is Efficient

Cross-protocol streaming adds minimal overhead (~1.4 µs per chunk). For typical streaming responses with hundreds of chunks, total transformation overhead is <1ms.

## Benchmark Methodology

- **Framework**: Criterion.rs v0.5
- **Sample Size**: 10-100 samples per benchmark
- **Warm-up**: 3 seconds
- **Measurement**: Statistical analysis with outlier detection
- **Profile**: Release mode with LTO enabled

## Benchmark Categories

1. **request_transformation**: End-to-end request transformation
2. **response_transformation**: End-to-end response transformation
3. **streaming**: SSE chunk parsing and formatting
4. **payload_sizes**: Impact of payload size on performance
5. **message_counts**: Impact of message count on performance
6. **individual_transformers**: Per-transformer operation benchmarks
7. **overhead**: Pipeline infrastructure overhead

## HTML Reports

After running benchmarks, detailed HTML reports are available at:

```
target/criterion/report/index.html
```

Reports include:
- Statistical analysis
- Performance regression detection
- Violin plots and histograms
- Comparison with previous runs

---

**Last Updated**: 2026-01-22
