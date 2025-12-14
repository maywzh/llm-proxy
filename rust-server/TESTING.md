# Testing Guide for LLM Proxy Rust Server

This document provides comprehensive information about the test suite for the Rust implementation of the LLM proxy server.

## Table of Contents

- [Overview](#overview)
- [Test Structure](#test-structure)
- [Running Tests](#running-tests)
- [Test Coverage](#test-coverage)
- [Benchmarking](#benchmarking)
- [Writing Tests](#writing-tests)
- [CI/CD Integration](#cicd-integration)

## Overview

The test suite is designed to ensure high code quality and reliability through multiple testing strategies:

- **Unit Tests**: Test individual functions and modules in isolation
- **Integration Tests**: Test end-to-end API functionality
- **Property-Based Tests**: Verify algorithmic properties with random inputs
- **Mock Tests**: Test external API interactions without real HTTP calls
- **Benchmarks**: Measure performance of critical code paths

### Test Coverage Goals

- **Target**: 80%+ code coverage
- **Critical Paths**: 100% coverage for core business logic
- **Error Handling**: Comprehensive error scenario testing

## Test Structure

```
rust-server/
├── src/
│   ├── core/
│   │   ├── config.rs          # Unit tests inline
│   │   ├── error.rs           # Unit tests inline
│   │   ├── metrics.rs         # Unit tests inline
│   │   └── middleware.rs      # Unit tests inline
│   ├── services/
│   │   └── provider_service.rs # Unit tests inline
│   └── api/
│       ├── models.rs          # Unit tests inline
│       ├── streaming.rs       # Unit tests inline
│       └── handlers.rs        # (tested via integration tests)
├── tests/
│   ├── integration_test.rs    # End-to-end API tests
│   ├── property_tests.rs      # Property-based tests
│   └── mock_tests.rs          # HTTP mocking tests
├── benches/
│   ├── provider_selection.rs  # Provider algorithm benchmarks
│   └── request_handling.rs    # Request processing benchmarks
└── scripts/
    ├── run_tests.sh           # Run all tests
    ├── coverage.sh            # Generate coverage report
    └── bench.sh               # Run benchmarks
```

## Running Tests

### Quick Start

Run all tests:
```bash
cargo test
```

Run with output:
```bash
cargo test -- --nocapture
```

### Specific Test Types

**Unit Tests Only**:
```bash
cargo test --lib
```

**Integration Tests**:
```bash
cargo test --test integration_test
```

**Property-Based Tests**:
```bash
cargo test --test property_tests
```

**Mock Tests**:
```bash
cargo test --test mock_tests
```

**Doc Tests**:
```bash
cargo test --doc
```

### Using Test Scripts

**Run all tests**:
```bash
./scripts/run_tests.sh
```

**Generate coverage report**:
```bash
./scripts/coverage.sh
```

**Run benchmarks**:
```bash
./scripts/bench.sh
```

### Test Filtering

Run tests matching a pattern:
```bash
cargo test provider
```

Run a specific test:
```bash
cargo test test_provider_service_creation
```

Run tests in a specific module:
```bash
cargo test core::config
```

## Test Coverage

### Generating Coverage Reports

Install cargo-tarpaulin:
```bash
cargo install cargo-tarpaulin
```

Generate HTML coverage report:
```bash
cargo tarpaulin --out Html --output-dir coverage
```

Generate XML coverage report (for CI):
```bash
cargo tarpaulin --out Xml --output-dir coverage
```

View HTML report:
```bash
open coverage/tarpaulin-report.html  # macOS
xdg-open coverage/tarpaulin-report.html  # Linux
```

### Coverage Configuration

The coverage tool is configured to:
- Exclude test files from coverage calculation
- Exclude benchmark files
- Set timeout to 300 seconds for long-running tests
- Generate both HTML and XML reports

## Benchmarking

### Running Benchmarks

Run all benchmarks:
```bash
cargo bench
```

Run specific benchmark:
```bash
cargo bench --bench provider_selection
```

### Benchmark Categories

**Provider Selection Benchmarks** (`benches/provider_selection.rs`):
- Single provider selection performance
- Concurrent provider selection
- Provider list retrieval
- Model list generation
- Weighted distribution accuracy
- Service creation overhead

**Request Handling Benchmarks** (`benches/request_handling.rs`):
- Request serialization/deserialization
- Configuration loading
- Model mapping lookups
- Message creation
- JSON manipulation

### Viewing Benchmark Results

Results are saved to `target/criterion/`:
```bash
open target/criterion/report/index.html  # macOS
xdg-open target/criterion/report/index.html  # Linux
```

## Writing Tests

### Unit Test Guidelines

Place unit tests in the same file as the code being tested:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name() {
        // Arrange
        let input = create_test_input();
        
        // Act
        let result = function_under_test(input);
        
        // Assert
        assert_eq!(result, expected_value);
    }
}
```

### Integration Test Guidelines

Create integration tests in `tests/` directory:

```rust
#[tokio::test]
async fn test_api_endpoint() {
    let app = create_test_app();
    
    let response = app
        .oneshot(Request::builder()
            .uri("/endpoint")
            .body(Body::empty())
            .unwrap())
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
}
```

### Property-Based Test Guidelines

Use proptest for property-based testing:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_test_name(input in strategy()) {
        // Test property that should hold for all inputs
        prop_assert!(property_holds(input));
    }
}
```

### Mock Test Guidelines

Use wiremock for HTTP mocking:

```rust
#[tokio::test]
async fn test_with_mock() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("POST"))
        .and(path("/endpoint"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(json!({"result": "success"})))
        .mount(&mock_server)
        .await;
    
    // Test code that calls the mocked endpoint
}
```

### Test Best Practices

1. **Arrange-Act-Assert**: Structure tests clearly
2. **One Assertion Per Test**: Focus on single behavior
3. **Descriptive Names**: Use `test_<what>_<when>_<expected>`
4. **Independent Tests**: No shared state between tests
5. **Fast Tests**: Keep unit tests under 100ms
6. **Cleanup**: Use `Drop` or defer cleanup in tests
7. **Error Messages**: Provide context in assertions

### Test Utilities

**Temporary Files**:
```rust
use tempfile::NamedTempFile;

let temp_file = NamedTempFile::new().unwrap();
```

**Serial Tests** (for tests that can't run in parallel):
```rust
use serial_test::serial;

#[test]
#[serial]
fn test_with_shared_resource() {
    // Test code
}
```

**Pretty Assertions**:
```rust
use pretty_assertions::assert_eq;

assert_eq!(actual, expected);  // Shows diff on failure
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      
      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          
      - name: Run tests
        run: cargo test --all-features
        
      - name: Generate coverage
        run: |
          cargo install cargo-tarpaulin
          cargo tarpaulin --out Xml
          
      - name: Upload coverage
        uses: codecov/codecov-action@v2
```

### Pre-commit Hooks

Add to `.git/hooks/pre-commit`:
```bash
#!/bin/bash
cargo test --quiet
if [ $? -ne 0 ]; then
    echo "Tests failed. Commit aborted."
    exit 1
fi
```

## Troubleshooting

### Common Issues

**Tests timing out**:
```bash
cargo test -- --test-threads=1  # Run tests serially
```

**Flaky tests**:
- Check for race conditions
- Use `serial_test` for shared resources
- Increase timeouts for async tests

**Coverage not generating**:
```bash
# Clean and rebuild
cargo clean
cargo build
cargo tarpaulin
```

**Benchmark variance**:
- Close other applications
- Run multiple times and compare
- Use `--sample-size` to increase samples

## Additional Resources

- [Rust Testing Documentation](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Proptest Documentation](https://docs.rs/proptest/)
- [Criterion Documentation](https://docs.rs/criterion/)
- [Wiremock Documentation](https://docs.rs/wiremock/)
- [Cargo-tarpaulin Documentation](https://github.com/xd009642/tarpaulin)

## Test Metrics

Current test statistics:
- **Total Tests**: 150+
- **Unit Tests**: 100+
- **Integration Tests**: 20+
- **Property Tests**: 15+
- **Mock Tests**: 10+
- **Benchmarks**: 12+

Target metrics:
- **Code Coverage**: 80%+
- **Test Execution Time**: <30s
- **Benchmark Stability**: <5% variance