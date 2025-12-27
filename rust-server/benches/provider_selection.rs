//! Benchmarks for provider selection algorithm.
//!
//! Run with: cargo bench --bench provider_selection

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_proxy_rust::{
    core::config::{AppConfig, ProviderConfig, ServerConfig},
    services::ProviderService,
};
use std::collections::HashMap;

fn create_config_with_providers(count: usize) -> AppConfig {
    let providers: Vec<ProviderConfig> = (0..count)
        .map(|i| ProviderConfig {
            name: format!("Provider{}", i),
            api_base: format!("http://localhost:{}", 8000 + i),
            api_key: format!("key{}", i),
            weight: (i % 10 + 1) as u32,
            model_mapping: HashMap::new(),
        })
        .collect();

    AppConfig {
        providers,
        server: ServerConfig::default(),
        verify_ssl: true,
        master_keys: vec![],
    }
}

fn bench_provider_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_selection");

    for provider_count in [2, 5, 10, 20, 50].iter() {
        let config = create_config_with_providers(*provider_count);
        let service = ProviderService::new(config);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(provider_count),
            provider_count,
            |b, _| {
                b.iter(|| {
                    black_box(
                        service
                            .get_next_provider(None)
                            .expect("get_next_provider failed"),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_provider_selection_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_selection_concurrent");

    let config = create_config_with_providers(10);
    let service = std::sync::Arc::new(ProviderService::new(config));

    for thread_count in [1, 2, 4, 8].iter() {
        group.throughput(Throughput::Elements(*thread_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let mut handles = vec![];
                    for _ in 0..thread_count {
                        let service_clone = std::sync::Arc::clone(&service);
                        let handle = std::thread::spawn(move || {
                            black_box(
                                service_clone
                                    .get_next_provider(None)
                                    .expect("get_next_provider failed"),
                            );
                        });
                        handles.push(handle);
                    }
                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_get_all_providers(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_all_providers");

    for provider_count in [2, 5, 10, 20, 50].iter() {
        let config = create_config_with_providers(*provider_count);
        let service = ProviderService::new(config);

        group.throughput(Throughput::Elements(*provider_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(provider_count),
            provider_count,
            |b, _| {
                b.iter(|| {
                    black_box(service.get_all_providers());
                });
            },
        );
    }

    group.finish();
}

fn bench_get_all_models(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_all_models");

    for provider_count in [2, 5, 10, 20].iter() {
        let mut config = create_config_with_providers(*provider_count);

        // Add model mappings
        for (i, provider) in config.providers.iter_mut().enumerate() {
            for j in 0..5 {
                provider
                    .model_mapping
                    .insert(format!("model-{}-{}", i, j), format!("mapped-{}-{}", i, j));
            }
        }

        let service = ProviderService::new(config);

        group.throughput(Throughput::Elements(*provider_count as u64 * 5));
        group.bench_with_input(
            BenchmarkId::from_parameter(provider_count),
            provider_count,
            |b, _| {
                b.iter(|| {
                    black_box(service.get_all_models());
                });
            },
        );
    }

    group.finish();
}

fn bench_weighted_distribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("weighted_distribution");

    let config = create_config_with_providers(10);
    let service = ProviderService::new(config);

    group.throughput(Throughput::Elements(1000));
    group.bench_function("1000_selections", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                black_box(
                    service
                        .get_next_provider(None)
                        .expect("get_next_provider failed"),
                );
            }
        });
    });

    group.finish();
}

fn bench_service_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("service_creation");

    for provider_count in [2, 5, 10, 20].iter() {
        let config = create_config_with_providers(*provider_count);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(provider_count),
            &config,
            |b, config| {
                b.iter(|| {
                    black_box(ProviderService::new(config.clone()));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_provider_selection,
    bench_provider_selection_concurrent,
    bench_get_all_providers,
    bench_get_all_models,
    bench_weighted_distribution,
    bench_service_creation,
);

criterion_main!(benches);
