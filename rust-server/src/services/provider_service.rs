//! Provider selection and management service.
//!
//! This module implements weighted round-robin selection of LLM providers
//! with thread-safe state management.

use crate::api::models::{is_pattern, Provider};
use crate::core::config::AppConfig;
use crate::core::error_types::ProviderEjectionReason;
use crate::core::metrics::{get_metrics, init_metrics};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CIRCUIT_CLOSED: &str = "closed";
const CIRCUIT_OPEN: &str = "open";
const CIRCUIT_HALF_OPEN: &str = "half_open";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    fn as_label(&self) -> &'static str {
        match self {
            Self::Closed => CIRCUIT_CLOSED,
            Self::Open => CIRCUIT_OPEN,
            Self::HalfOpen => CIRCUIT_HALF_OPEN,
        }
    }
}

#[derive(Debug, Clone)]
struct ProviderRuntimeState {
    multiplier: f64,
    circuit_state: CircuitState,
    cooldown_until: Option<Instant>,
    open_until: Option<Instant>,
    recovery_started_at: Option<Instant>,
    half_open_successes: u32,
    half_open_in_flight: u32,
    consecutive_429: u32,
    consecutive_5xx: u32,
    consecutive_transport: u32,
    ejection_count: u32,
}

impl ProviderRuntimeState {
    fn new() -> Self {
        Self {
            multiplier: 1.0,
            circuit_state: CircuitState::Closed,
            cooldown_until: None,
            open_until: None,
            recovery_started_at: None,
            half_open_successes: 0,
            half_open_in_flight: 0,
            consecutive_429: 0,
            consecutive_5xx: 0,
            consecutive_transport: 0,
            ejection_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct AdaptiveRoutingConfig {
    enabled: bool,
    min_multiplier: f64,
    half_open_weight_factor: f64,
    success_recovery_step: f64,
    slow_start_window: Duration,
    consecutive_429_threshold: u32,
    consecutive_5xx_threshold: u32,
    consecutive_transport_threshold: u32,
    half_open_success_threshold: u32,
    half_open_max_probes: u32,
    base_429_cooldown: Duration,
    max_429_cooldown: Duration,
    base_open_duration: Duration,
    max_open_duration: Duration,
}

impl AdaptiveRoutingConfig {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            min_multiplier: env_f64("ADAPTIVE_MIN_MULTIPLIER", 0.05),
            half_open_weight_factor: env_f64("ADAPTIVE_HALF_OPEN_WEIGHT_FACTOR", 0.2),
            success_recovery_step: env_f64("ADAPTIVE_SUCCESS_RECOVERY_STEP", 0.1),
            slow_start_window: Duration::from_secs(env_u64("ADAPTIVE_SLOW_START_WINDOW_SECS", 60)),
            consecutive_429_threshold: env_u32("ADAPTIVE_429_THRESHOLD", 3),
            consecutive_5xx_threshold: env_u32("ADAPTIVE_5XX_THRESHOLD", 5),
            consecutive_transport_threshold: env_u32("ADAPTIVE_TRANSPORT_THRESHOLD", 4),
            half_open_success_threshold: env_u32("ADAPTIVE_HALF_OPEN_SUCCESS_THRESHOLD", 2),
            half_open_max_probes: env_u32("ADAPTIVE_HALF_OPEN_MAX_PROBES", 1),
            base_429_cooldown: Duration::from_secs(env_u64("ADAPTIVE_BASE_429_COOLDOWN_SECS", 15)),
            max_429_cooldown: Duration::from_secs(env_u64("ADAPTIVE_MAX_429_COOLDOWN_SECS", 300)),
            base_open_duration: Duration::from_secs(env_u64(
                "ADAPTIVE_BASE_OPEN_DURATION_SECS",
                30,
            )),
            max_open_duration: Duration::from_secs(env_u64("ADAPTIVE_MAX_OPEN_DURATION_SECS", 300)),
        }
    }
}

/// Service for managing and selecting LLM providers.
///
/// Uses weighted random selection to distribute requests across providers
/// based on their configured weights.
#[derive(Clone)]
pub struct ProviderService {
    providers: Arc<Vec<Provider>>,
    weights: Arc<Vec<u32>>,
    weighted_index: Arc<WeightedIndex<u32>>,
    runtime_states: Arc<DashMap<String, ProviderRuntimeState>>,
    adaptive_config: Arc<AdaptiveRoutingConfig>,
}

impl ProviderService {
    /// Create a new provider service from configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration containing provider definitions
    ///
    /// # Panics
    ///
    /// Panics if the weighted index cannot be created (e.g., all weights are zero)
    ///
    /// # Examples
    ///
    /// ```
    /// use llm_proxy_rust::core::config::{AppConfig, ProviderConfig, ServerConfig};
    /// use llm_proxy_rust::services::ProviderService;
    /// use std::collections::HashMap;
    ///
    /// let config = AppConfig {
    ///     providers: vec![ProviderConfig {
    ///         name: "test".to_string(),
    ///         api_base: "http://localhost".to_string(),
    ///         api_key: "key".to_string(),
    ///         weight: 1,
    ///         model_mapping: HashMap::new(),
    ///         provider_type: "openai".to_string(),
    ///         provider_params: HashMap::new(),
    ///     }],
    ///     server: ServerConfig::default(),
    ///     verify_ssl: true,
    ///     request_timeout_secs: 300,
    ///     ttft_timeout_secs: None,
    ///     credentials: vec![],
    ///     provider_suffix: None,
    ///     min_tokens_limit: 1,
    ///     max_tokens_limit: 128000,
    /// };
    /// let service = ProviderService::new(config);
    /// ```
    pub fn new(config: AppConfig) -> Self {
        Self::new_with_adaptive(config, adaptive_enabled_from_env())
    }

    pub fn new_with_adaptive(config: AppConfig, adaptive_enabled: bool) -> Self {
        let providers: Vec<Provider> = config
            .providers
            .into_iter()
            .map(|p| Provider {
                name: p.name,
                api_base: p.api_base,
                api_key: p.api_key,
                weight: p.weight,
                model_mapping: p.model_mapping,
                provider_type: p.provider_type,
                provider_params: p.provider_params,
            })
            .collect();

        let weights: Vec<u32> = providers.iter().map(|p| p.weight).collect();
        let weighted_index = WeightedIndex::new(&weights).expect("Failed to create weighted index");
        let runtime_states = DashMap::new();

        for provider in &providers {
            runtime_states.insert(provider.name.clone(), ProviderRuntimeState::new());
        }

        let adaptive_config = AdaptiveRoutingConfig::new(adaptive_enabled);
        let service = Self {
            providers: Arc::new(providers),
            weights: Arc::new(weights),
            weighted_index: Arc::new(weighted_index),
            runtime_states: Arc::new(runtime_states),
            adaptive_config: Arc::new(adaptive_config),
        };

        if service.adaptive_config.enabled {
            init_metrics();
            for provider in service.providers.iter() {
                let static_weight = provider.weight as f64;
                service.update_runtime_metrics(
                    provider.name.as_str(),
                    static_weight,
                    CircuitState::Closed,
                );
            }
        }

        service
    }

    /// Get the next provider using weighted random selection.
    ///
    /// This method is thread-safe and can be called concurrently.
    /// Returns a clone of the provider to avoid holding locks.
    ///
    /// # Arguments
    ///
    /// * `model` - Optional model name to filter providers that support it
    ///
    /// # Returns
    ///
    /// Selected provider
    ///
    /// # Errors
    ///
    /// Returns error if model is specified but no provider supports it
    pub fn get_next_provider(&self, model: Option<&str>) -> Result<Provider, String> {
        if self.adaptive_config.enabled {
            return self.get_next_provider_adaptive(model);
        }

        let mut rng = thread_rng();

        let Some(model_name) = model else {
            let index = self.weighted_index.sample(&mut rng);
            return Ok(self.providers[index].clone());
        };

        let mut available_providers = Vec::new();
        let mut available_weights = Vec::new();

        for (provider, &weight) in self.providers.iter().zip(self.weights.iter()) {
            if provider.supports_model(model_name) {
                available_providers.push(provider.clone());
                available_weights.push(weight);
            }
        }

        if available_providers.is_empty() {
            return Err(format!("No provider supports model: {}", model_name));
        }

        let weighted_index = WeightedIndex::new(&available_weights)
            .map_err(|e| format!("Failed to create weighted index: {}", e))?;

        let index = weighted_index.sample(&mut rng);
        Ok(available_providers[index].clone())
    }

    pub fn report_http_status(
        &self,
        provider_name: &str,
        status_code: u16,
        retry_after: Option<&str>,
    ) {
        if !self.adaptive_config.enabled {
            return;
        }

        if status_code < 400 {
            self.report_success(provider_name);
            return;
        }

        if status_code == 429 {
            self.on_provider_429(provider_name, parse_retry_after_seconds(retry_after));
            return;
        }

        if status_code >= 500 {
            self.on_provider_5xx(provider_name);
            return;
        }

        // 4xx errors: only 408 (Request Timeout) indicates provider-side pressure;
        // other 4xx (401/403/404 etc.) are client/config errors and should not
        // penalize the provider's weight or interrupt HalfOpen recovery.
        if let Some(mut state) = self.runtime_states.get_mut(provider_name) {
            if state.circuit_state == CircuitState::HalfOpen {
                state.half_open_in_flight = state.half_open_in_flight.saturating_sub(1);
            }
            state.consecutive_429 = 0;
            state.consecutive_5xx = 0;
            state.consecutive_transport = 0;

            if status_code == 408 {
                state.multiplier =
                    (state.multiplier * 0.9).max(self.adaptive_config.min_multiplier);
                state.half_open_successes = 0;
            }

            let multiplier = state.multiplier;
            let circuit_state = state.circuit_state;
            drop(state);
            self.update_runtime_metrics(provider_name, multiplier, circuit_state);
        }
    }

    pub fn report_transport_error(&self, provider_name: &str) {
        if !self.adaptive_config.enabled {
            return;
        }

        let now = Instant::now();
        if let Some(mut state) = self.runtime_states.get_mut(provider_name) {
            state.consecutive_429 = 0;
            state.consecutive_5xx = 0;
            state.consecutive_transport = state.consecutive_transport.saturating_add(1);
            state.multiplier = (state.multiplier * 0.7).max(self.adaptive_config.min_multiplier);
            state.half_open_successes = 0;

            if state.circuit_state == CircuitState::HalfOpen {
                state.half_open_in_flight = state.half_open_in_flight.saturating_sub(1);
            }

            // In HalfOpen, a single failure immediately re-opens the circuit
            let should_open = state.circuit_state == CircuitState::HalfOpen
                || state.consecutive_transport
                    >= self.adaptive_config.consecutive_transport_threshold;

            if should_open {
                let open_duration = next_backoff(
                    state.ejection_count,
                    self.adaptive_config.base_open_duration,
                    self.adaptive_config.max_open_duration,
                );
                self.open_circuit(
                    provider_name,
                    &mut state,
                    now,
                    ProviderEjectionReason::Transport,
                    open_duration,
                );
                return;
            }

            let multiplier = state.multiplier;
            let circuit_state = state.circuit_state;
            drop(state);
            self.update_runtime_metrics(provider_name, multiplier, circuit_state);
        }
    }

    pub fn report_success(&self, provider_name: &str) {
        if !self.adaptive_config.enabled {
            return;
        }

        let now = Instant::now();
        if let Some(mut state) = self.runtime_states.get_mut(provider_name) {
            self.promote_open_to_half_open_if_needed(&mut state, now);
            state.consecutive_429 = 0;
            state.consecutive_5xx = 0;
            state.consecutive_transport = 0;
            state.cooldown_until = None;

            if state.circuit_state == CircuitState::HalfOpen {
                state.half_open_in_flight = state.half_open_in_flight.saturating_sub(1);
                state.half_open_successes = state.half_open_successes.saturating_add(1);
                if state.half_open_successes >= self.adaptive_config.half_open_success_threshold {
                    state.circuit_state = CircuitState::Closed;
                    state.half_open_successes = 0;
                    state.half_open_in_flight = 0;
                    state.ejection_count = 0;
                    state.recovery_started_at = Some(now);
                    state.multiplier = state.multiplier.max(0.3);
                }
            } else {
                state.multiplier =
                    (state.multiplier + self.adaptive_config.success_recovery_step).min(1.0);
            }

            let multiplier = state.multiplier;
            let circuit_state = state.circuit_state;
            drop(state);
            self.update_runtime_metrics(provider_name, multiplier, circuit_state);
        }
    }

    pub fn adaptive_enabled(&self) -> bool {
        self.adaptive_config.enabled
    }

    /// Get all configured providers.
    ///
    /// Returns a vector of provider clones.
    pub fn get_all_providers(&self) -> Vec<Provider> {
        (*self.providers).clone()
    }

    /// Get provider weights.
    ///
    /// Returns a vector of weights corresponding to each provider.
    pub fn get_provider_weights(&self) -> Vec<u32> {
        (*self.weights).clone()
    }

    /// Get all unique model names across all providers.
    ///
    /// Returns a set of model identifiers that can be requested.
    /// Note: Wildcard/regex patterns are filtered out from the result.
    /// Only exact model names are returned for /v1/models compatibility.
    pub fn get_all_models(&self) -> HashSet<String> {
        let mut models = HashSet::new();
        for provider in self.providers.iter() {
            for model in provider.model_mapping.keys() {
                // Filter out wildcard/regex patterns
                if !is_pattern(model) {
                    models.insert(model.clone());
                }
            }
        }
        models
    }

    /// Log provider configuration at startup.
    ///
    /// Outputs provider names, weights, and selection probabilities.
    pub fn log_providers(&self) {
        let total_weight: u32 = self.weights.iter().sum();
        tracing::info!(
            "Starting LLM API Proxy with {} providers",
            self.providers.len()
        );
        for (i, provider) in self.providers.iter().enumerate() {
            let weight = self.weights[i];
            let probability = (weight as f64 / total_weight as f64) * 100.0;
            tracing::info!(
                "  - {}: weight={} ({:.1}%)",
                provider.name,
                weight,
                probability
            );
        }

        if self.adaptive_config.enabled {
            tracing::info!("Adaptive provider routing is enabled");
        }
    }

    fn get_next_provider_adaptive(&self, model: Option<&str>) -> Result<Provider, String> {
        let now = Instant::now();
        let mut eligible_providers = Vec::new();
        let mut eligible_weights = Vec::new();
        let mut fallback_candidates = Vec::new();

        for (provider, &weight) in self.providers.iter().zip(self.weights.iter()) {
            if model.is_some_and(|model_name| !provider.supports_model(model_name)) {
                continue;
            }

            let mut effective_weight = weight as f64;
            let mut eligible = true;

            if let Some(mut state) = self.runtime_states.get_mut(provider.name.as_str()) {
                // All mutations and reads happen in a single short scope.
                let promoted = self.promote_open_to_half_open_if_needed(&mut state, now);

                if let Some(cooldown_until) = state.cooldown_until {
                    if now < cooldown_until {
                        eligible = false;
                    } else {
                        state.cooldown_until = None;
                    }
                }

                if state.circuit_state == CircuitState::Open {
                    eligible = false;
                }

                // Limit concurrent probes to HalfOpen providers
                if state.circuit_state == CircuitState::HalfOpen
                    && state.half_open_in_flight >= self.adaptive_config.half_open_max_probes
                {
                    eligible = false;
                }

                let state_factor = if state.circuit_state == CircuitState::HalfOpen {
                    self.adaptive_config.half_open_weight_factor
                } else {
                    1.0
                };

                let slow_start_factor = self.get_slow_start_factor(&mut state, now);
                effective_weight *= state.multiplier * state_factor * slow_start_factor;
                effective_weight = effective_weight.max(self.adaptive_config.min_multiplier);

                // Snapshot values then release lock before metrics I/O
                let multiplier = state.multiplier;
                let circuit_state = state.circuit_state;
                drop(state);

                // Metrics update happens outside the lock
                if promoted {
                    self.update_runtime_metrics(provider.name.as_str(), multiplier, circuit_state);
                }
            }

            fallback_candidates.push((provider.clone(), effective_weight));

            if eligible {
                let scaled = (effective_weight * 1000.0).round() as u32;
                eligible_providers.push(provider.clone());
                eligible_weights.push(scaled.max(1));
            }
        }

        if eligible_providers.is_empty() {
            return self.select_probe_fallback(model, fallback_candidates);
        }

        let weighted_index = WeightedIndex::new(&eligible_weights)
            .map_err(|e| format!("Failed to create adaptive weighted index: {}", e))?;
        let mut rng = thread_rng();
        let index = weighted_index.sample(&mut rng);
        let selected = &eligible_providers[index];

        // Track in-flight probe for HalfOpen providers
        if let Some(mut state) = self.runtime_states.get_mut(selected.name.as_str()) {
            if state.circuit_state == CircuitState::HalfOpen {
                state.half_open_in_flight = state.half_open_in_flight.saturating_add(1);
            }
        }

        Ok(selected.clone())
    }

    fn select_probe_fallback(
        &self,
        model: Option<&str>,
        fallback_candidates: Vec<(Provider, f64)>,
    ) -> Result<Provider, String> {
        if fallback_candidates.is_empty() {
            if let Some(model_name) = model {
                return Err(format!("No provider supports model: {}", model_name));
            }
            return Err("No provider configured".to_string());
        }

        let selected = fallback_candidates
            .into_iter()
            .max_by(|(_, lhs_weight), (_, rhs_weight)| lhs_weight.total_cmp(rhs_weight))
            .map(|(provider, _)| provider)
            .ok_or_else(|| "No provider available".to_string())?;

        tracing::warn!(
            provider = %selected.name,
            model = %model.unwrap_or("*"),
            "All providers are temporarily degraded; selecting probe provider"
        );

        Ok(selected)
    }

    fn on_provider_429(&self, provider_name: &str, retry_after_secs: Option<u64>) {
        let now = Instant::now();
        if let Some(mut state) = self.runtime_states.get_mut(provider_name) {
            state.consecutive_429 = state.consecutive_429.saturating_add(1);
            state.consecutive_5xx = 0;
            state.consecutive_transport = 0;
            state.half_open_successes = 0;
            state.multiplier = (state.multiplier * 0.5).max(self.adaptive_config.min_multiplier);

            let fallback_cooldown = next_backoff(
                state.consecutive_429.saturating_sub(1),
                self.adaptive_config.base_429_cooldown,
                self.adaptive_config.max_429_cooldown,
            );
            let retry_after_cooldown = retry_after_secs.map(Duration::from_secs);
            let cooldown = retry_after_cooldown
                .unwrap_or(fallback_cooldown)
                .min(self.adaptive_config.max_429_cooldown);
            state.cooldown_until = Some(now + cooldown);

            if state.circuit_state == CircuitState::HalfOpen {
                state.half_open_in_flight = state.half_open_in_flight.saturating_sub(1);
            }

            // In HalfOpen, a single failure immediately re-opens the circuit
            let should_open = state.circuit_state == CircuitState::HalfOpen
                || state.consecutive_429 >= self.adaptive_config.consecutive_429_threshold;

            if should_open {
                self.open_circuit(
                    provider_name,
                    &mut state,
                    now,
                    ProviderEjectionReason::RateLimit429,
                    cooldown,
                );
                return;
            }

            let multiplier = state.multiplier;
            let circuit_state = state.circuit_state;
            drop(state);
            self.update_runtime_metrics(provider_name, multiplier, circuit_state);
        }
    }

    fn on_provider_5xx(&self, provider_name: &str) {
        let now = Instant::now();
        if let Some(mut state) = self.runtime_states.get_mut(provider_name) {
            state.consecutive_429 = 0;
            state.consecutive_transport = 0;
            state.consecutive_5xx = state.consecutive_5xx.saturating_add(1);
            state.half_open_successes = 0;
            state.multiplier = (state.multiplier * 0.7).max(self.adaptive_config.min_multiplier);

            if state.circuit_state == CircuitState::HalfOpen {
                state.half_open_in_flight = state.half_open_in_flight.saturating_sub(1);
            }

            // In HalfOpen, a single failure immediately re-opens the circuit
            let should_open = state.circuit_state == CircuitState::HalfOpen
                || state.consecutive_5xx >= self.adaptive_config.consecutive_5xx_threshold;

            if should_open {
                let open_duration = next_backoff(
                    state.ejection_count,
                    self.adaptive_config.base_open_duration,
                    self.adaptive_config.max_open_duration,
                );
                self.open_circuit(
                    provider_name,
                    &mut state,
                    now,
                    ProviderEjectionReason::Server5xx,
                    open_duration,
                );
                return;
            }

            let multiplier = state.multiplier;
            let circuit_state = state.circuit_state;
            drop(state);
            self.update_runtime_metrics(provider_name, multiplier, circuit_state);
        }
    }

    fn open_circuit(
        &self,
        provider_name: &str,
        state: &mut ProviderRuntimeState,
        now: Instant,
        reason: ProviderEjectionReason,
        open_duration: Duration,
    ) {
        state.circuit_state = CircuitState::Open;
        state.half_open_successes = 0;
        state.half_open_in_flight = 0;
        state.ejection_count = state.ejection_count.saturating_add(1);
        state.open_until = Some(now + open_duration);
        state.recovery_started_at = None;

        let metrics = get_metrics();
        metrics
            .provider_ejections_total
            .with_label_values(&[provider_name, reason.as_str()])
            .inc();

        self.update_runtime_metrics(provider_name, 0.0, state.circuit_state);
    }

    /// Returns `true` if the state was actually promoted from Open to HalfOpen.
    fn promote_open_to_half_open_if_needed(
        &self,
        state: &mut ProviderRuntimeState,
        now: Instant,
    ) -> bool {
        if state.circuit_state == CircuitState::Open {
            if let Some(open_until) = state.open_until {
                if now >= open_until {
                    state.circuit_state = CircuitState::HalfOpen;
                    state.open_until = None;
                    state.cooldown_until = None;
                    state.half_open_successes = 0;
                    state.half_open_in_flight = 0;
                    state.recovery_started_at = Some(now);
                    state.multiplier = state
                        .multiplier
                        .max(self.adaptive_config.half_open_weight_factor);
                    return true;
                }
            }
        }
        false
    }

    fn get_slow_start_factor(&self, state: &mut ProviderRuntimeState, now: Instant) -> f64 {
        if state.circuit_state != CircuitState::Closed {
            return 1.0;
        }

        let Some(started_at) = state.recovery_started_at else {
            return 1.0;
        };

        let elapsed = now.saturating_duration_since(started_at);
        if elapsed >= self.adaptive_config.slow_start_window {
            state.recovery_started_at = None;
            return 1.0;
        }

        let progress = elapsed.as_secs_f64() / self.adaptive_config.slow_start_window.as_secs_f64();
        progress.max(self.adaptive_config.half_open_weight_factor)
    }

    fn update_runtime_metrics(
        &self,
        provider_name: &str,
        effective_weight: f64,
        circuit_state: CircuitState,
    ) {
        let metrics = get_metrics();
        metrics
            .provider_effective_weight
            .with_label_values(&[provider_name])
            .set(effective_weight.max(0.0));

        for state in [CIRCUIT_CLOSED, CIRCUIT_OPEN, CIRCUIT_HALF_OPEN] {
            let value = if state == circuit_state.as_label() {
                1.0
            } else {
                0.0
            };
            metrics
                .provider_circuit_state
                .with_label_values(&[provider_name, state])
                .set(value);
        }

        let health = if circuit_state == CircuitState::Open {
            0.0
        } else {
            1.0
        };
        metrics
            .provider_health
            .with_label_values(&[provider_name])
            .set(health);
    }
}

fn adaptive_enabled_from_env() -> bool {
    std::env::var("ADAPTIVE_ROUTING_ENABLED")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn next_backoff(step: u32, base: Duration, max: Duration) -> Duration {
    let exponent = step.min(10);
    let multiplier = 1_u64 << exponent;
    let seconds = base.as_secs().saturating_mul(multiplier).min(max.as_secs());
    Duration::from_secs(seconds)
}

fn parse_retry_after_seconds(value: Option<&str>) -> Option<u64> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }

    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(seconds);
    }

    let parsed = DateTime::parse_from_rfc2822(raw).ok()?;
    let now = Utc::now();
    if parsed.with_timezone(&Utc) <= now {
        return Some(0);
    }

    let delta = parsed.with_timezone(&Utc) - now;
    delta.num_seconds().try_into().ok()
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{ModelMappingValue, ProviderConfig, ServerConfig};
    use std::collections::HashMap;

    /// Helper to create a simple string model mapping (for backward compatibility in tests)
    fn simple_mapping(entries: &[(&str, &str)]) -> HashMap<String, ModelMappingValue> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), ModelMappingValue::Simple(v.to_string())))
            .collect()
    }

    fn create_test_config() -> AppConfig {
        AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "Provider1".to_string(),
                    api_base: "http://localhost:8000".to_string(),
                    api_key: "key1".to_string(),
                    weight: 2,
                    model_mapping: simple_mapping(&[("model1", "provider1-model1")]),
                    provider_type: "openai".to_string(),
                    provider_params: HashMap::new(),
                },
                ProviderConfig {
                    name: "Provider2".to_string(),
                    api_base: "http://localhost:8001".to_string(),
                    api_key: "key2".to_string(),
                    weight: 1,
                    model_mapping: simple_mapping(&[("model2", "provider2-model2")]),
                    provider_type: "openai".to_string(),
                    provider_params: HashMap::new(),
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        }
    }

    fn create_single_provider_config() -> AppConfig {
        AppConfig {
            providers: vec![ProviderConfig {
                name: "OnlyProvider".to_string(),
                api_base: "http://localhost:8000".to_string(),
                api_key: "key".to_string(),
                weight: 1,
                model_mapping: HashMap::new(),
                provider_type: "openai".to_string(),
                provider_params: HashMap::new(),
            }],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        }
    }

    #[test]
    fn test_provider_service_creation() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        assert_eq!(service.get_all_providers().len(), 2);
        assert_eq!(service.get_provider_weights(), vec![2, 1]);
    }

    #[test]
    fn test_weighted_selection() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        // Test that selection works
        let provider = service.get_next_provider(None).unwrap();
        assert!(provider.name == "Provider1" || provider.name == "Provider2");
    }

    #[test]
    fn test_weighted_selection_distribution() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        // Sample 1000 times to check distribution
        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // Provider1 has weight 2, Provider2 has weight 1
        // So Provider1 should be selected roughly 2x as often
        // Allow for some variance (between 1.5x and 3x)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 3.0, "Ratio was {}", ratio);
    }

    #[test]
    fn test_single_provider_always_selected() {
        let config = create_single_provider_config();
        let service = ProviderService::new(config);

        for _ in 0..100 {
            let provider = service.get_next_provider(None).unwrap();
            assert_eq!(provider.name, "OnlyProvider");
        }
    }

    #[test]
    fn test_get_all_models() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let models = service.get_all_models();
        assert_eq!(models.len(), 2);
        assert!(models.contains("model1"));
        assert!(models.contains("model2"));
    }

    #[test]
    fn test_get_all_models_empty() {
        let config = create_single_provider_config();
        let service = ProviderService::new(config);

        let models = service.get_all_models();
        assert_eq!(models.len(), 0);
    }

    #[test]
    fn test_get_all_models_with_duplicates() {
        let mut config = create_test_config();
        // Add same model to both providers
        config.providers[0].model_mapping.insert(
            "shared-model".to_string(),
            ModelMappingValue::Simple("provider1-shared".to_string()),
        );
        config.providers[1].model_mapping.insert(
            "shared-model".to_string(),
            ModelMappingValue::Simple("provider2-shared".to_string()),
        );

        let service = ProviderService::new(config);
        let models = service.get_all_models();

        // Should only have 3 unique models (model1, model2, shared-model)
        assert_eq!(models.len(), 3);
        assert!(models.contains("shared-model"));
    }

    #[test]
    fn test_provider_cloning() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let provider1 = service.get_next_provider(None).unwrap();
        let provider2 = service.get_next_provider(None).unwrap();

        // Providers should be independent clones
        assert!(provider1.name == "Provider1" || provider1.name == "Provider2");
        assert!(provider2.name == "Provider1" || provider2.name == "Provider2");
    }

    #[test]
    fn test_provider_fields() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let providers = service.get_all_providers();

        for provider in providers {
            assert!(!provider.name.is_empty());
            assert!(!provider.api_base.is_empty());
            assert!(!provider.api_key.is_empty());
            assert!(provider.weight > 0);
        }
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let config = create_test_config();
        let service = Arc::new(ProviderService::new(config));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let service = Arc::clone(&service);
                thread::spawn(move || {
                    let _provider = service.get_next_provider(None).unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let config = create_test_config();
        let service = Arc::new(ProviderService::new(config));

        let handles: Vec<_> = (0..100)
            .map(|_| {
                let service = Arc::clone(&service);
                thread::spawn(move || {
                    for _ in 0..10 {
                        let provider = service.get_next_provider(None).unwrap();
                        assert!(provider.name == "Provider1" || provider.name == "Provider2");
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_equal_weights() {
        let mut config = create_test_config();
        config.providers[0].weight = 1;
        config.providers[1].weight = 1;

        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // With equal weights, distribution should be roughly 50/50
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 0.8 && ratio < 1.2, "Ratio was {}", ratio);
    }

    #[test]
    fn test_high_weight_difference() {
        let mut config = create_test_config();
        config.providers[0].weight = 10;
        config.providers[1].weight = 1;

        let service = ProviderService::new(config);

        let mut provider1_count = 0;

        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            }
        }

        // Provider1 should be selected roughly 90% of the time
        let percentage = (provider1_count as f64 / 1000.0) * 100.0;
        assert!(
            percentage > 80.0 && percentage < 95.0,
            "Percentage was {}",
            percentage
        );
    }

    #[test]
    fn test_model_mapping_preserved() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let providers = service.get_all_providers();

        let provider1 = providers.iter().find(|p| p.name == "Provider1").unwrap();
        assert_eq!(
            provider1
                .model_mapping
                .get("model1")
                .unwrap()
                .mapped_model(),
            "provider1-model1"
        );

        let provider2 = providers.iter().find(|p| p.name == "Provider2").unwrap();
        assert_eq!(
            provider2
                .model_mapping
                .get("model2")
                .unwrap()
                .mapped_model(),
            "provider2-model2"
        );
    }

    #[test]
    fn test_get_next_provider_with_model() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        // Request model1 which only Provider1 has
        for _ in 0..10 {
            let provider = service.get_next_provider(Some("model1")).unwrap();
            assert_eq!(provider.name, "Provider1");
            assert!(provider.model_mapping.contains_key("model1"));
        }
    }

    #[test]
    fn test_get_next_provider_with_nonexistent_model() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        // Request a model that no provider has
        let result = service.get_next_provider(Some("nonexistent-model"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No provider supports model"));
    }

    #[test]
    fn test_get_next_provider_without_model_uses_all_providers() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // Both providers should be selected
        assert!(provider1_count > 0);
        assert!(provider2_count > 0);

        // Should follow weight distribution (2:1 ratio)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 2.5, "Ratio was {}", ratio);
    }

    #[test]
    fn test_get_next_provider_with_shared_model() {
        // Create config where both providers have the same model
        let mut config = create_test_config();
        config.providers[0].model_mapping.insert(
            "shared-model".to_string(),
            ModelMappingValue::Simple("provider1-shared".to_string()),
        );
        config.providers[1].model_mapping.insert(
            "shared-model".to_string(),
            ModelMappingValue::Simple("provider2-shared".to_string()),
        );

        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        // Request the shared model many times
        for _ in 0..1000 {
            let provider = service.get_next_provider(Some("shared-model")).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // Both providers should be selected
        assert!(provider1_count > 0);
        assert!(provider2_count > 0);

        // Should follow weight distribution (2:1 ratio)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 2.5, "Ratio was {}", ratio);
    }

    #[test]
    fn test_provider_selection_with_regex_pattern() {
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "claude-provider".to_string(),
                    api_base: "https://api.claude.com".to_string(),
                    api_key: "key1".to_string(),
                    weight: 1,
                    model_mapping: simple_mapping(&[("claude-opus-4-5-.*", "claude-opus-mapped")]),
                    provider_type: "anthropic".to_string(),
                    provider_params: HashMap::new(),
                },
                ProviderConfig {
                    name: "openai-provider".to_string(),
                    api_base: "https://api.openai.com".to_string(),
                    api_key: "key2".to_string(),
                    weight: 1,
                    model_mapping: simple_mapping(&[("gpt-4", "gpt-4-turbo")]),
                    provider_type: "openai".to_string(),
                    provider_params: HashMap::new(),
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        };

        let service = ProviderService::new(config);

        // Test regex pattern matching - should select claude-provider
        let provider = service
            .get_next_provider(Some("claude-opus-4-5-20240620"))
            .unwrap();
        assert_eq!(provider.name, "claude-provider");
        assert_eq!(
            provider.get_mapped_model("claude-opus-4-5-20240620"),
            "claude-opus-mapped"
        );

        // Test another variant of the pattern
        let provider = service
            .get_next_provider(Some("claude-opus-4-5-latest"))
            .unwrap();
        assert_eq!(provider.name, "claude-provider");
        assert_eq!(
            provider.get_mapped_model("claude-opus-4-5-latest"),
            "claude-opus-mapped"
        );

        // Test exact match - should select openai-provider
        let provider = service.get_next_provider(Some("gpt-4")).unwrap();
        assert_eq!(provider.name, "openai-provider");
        assert_eq!(provider.get_mapped_model("gpt-4"), "gpt-4-turbo");

        // Test non-matching model - should return error
        let result = service.get_next_provider(Some("unknown-model"));
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_selection_with_simple_wildcard() {
        let config = AppConfig {
            providers: vec![ProviderConfig {
                name: "gemini-provider".to_string(),
                api_base: "https://api.gemini.com".to_string(),
                api_key: "key1".to_string(),
                weight: 1,
                model_mapping: simple_mapping(&[("gemini-*", "gemini-pro")]),
                provider_type: "openai".to_string(),
                provider_params: HashMap::new(),
            }],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        };

        let service = ProviderService::new(config);

        // Test simple wildcard matching
        let provider = service.get_next_provider(Some("gemini-pro")).unwrap();
        assert_eq!(provider.name, "gemini-provider");
        assert_eq!(provider.get_mapped_model("gemini-pro"), "gemini-pro");

        let provider = service.get_next_provider(Some("gemini-ultra")).unwrap();
        assert_eq!(provider.name, "gemini-provider");
        assert_eq!(provider.get_mapped_model("gemini-ultra"), "gemini-pro");

        let provider = service.get_next_provider(Some("gemini-1.5-pro")).unwrap();
        assert_eq!(provider.name, "gemini-provider");
        assert_eq!(provider.get_mapped_model("gemini-1.5-pro"), "gemini-pro");
    }

    #[test]
    fn test_exact_match_priority_over_pattern() {
        let config = AppConfig {
            providers: vec![ProviderConfig {
                name: "provider1".to_string(),
                api_base: "https://api1.com".to_string(),
                api_key: "key1".to_string(),
                weight: 1,
                model_mapping: simple_mapping(&[
                    ("claude-.*", "claude-pattern"),
                    ("claude-opus", "claude-opus-exact"),
                ]),
                provider_type: "anthropic".to_string(),
                provider_params: HashMap::new(),
            }],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        };

        let service = ProviderService::new(config);

        // Exact match should take priority
        let provider = service.get_next_provider(Some("claude-opus")).unwrap();
        assert_eq!(
            provider.get_mapped_model("claude-opus"),
            "claude-opus-exact"
        );

        // Pattern should match other claude models
        let provider = service.get_next_provider(Some("claude-sonnet")).unwrap();
        assert_eq!(provider.get_mapped_model("claude-sonnet"), "claude-pattern");
    }

    #[test]
    fn test_multiple_providers_with_patterns_weighted() {
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "provider1".to_string(),
                    api_base: "https://api1.com".to_string(),
                    api_key: "key1".to_string(),
                    weight: 2,
                    model_mapping: simple_mapping(&[("claude-opus-4-5-.*", "provider1-claude")]),
                    provider_type: "anthropic".to_string(),
                    provider_params: HashMap::new(),
                },
                ProviderConfig {
                    name: "provider2".to_string(),
                    api_base: "https://api2.com".to_string(),
                    api_key: "key2".to_string(),
                    weight: 1,
                    model_mapping: simple_mapping(&[("claude-opus-4-5-.*", "provider2-claude")]),
                    provider_type: "anthropic".to_string(),
                    provider_params: HashMap::new(),
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        };

        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        // Both providers should be selected with weighted distribution
        for _ in 0..1000 {
            let provider = service
                .get_next_provider(Some("claude-opus-4-5-20240620"))
                .unwrap();
            if provider.name == "provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        assert!(provider1_count > 0);
        assert!(provider2_count > 0);

        // Should follow weight distribution (2:1 ratio)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 2.5, "Ratio was {}", ratio);
    }

    #[test]
    fn test_get_all_models_filters_patterns() {
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "provider1".to_string(),
                    api_base: "https://api1.com".to_string(),
                    api_key: "key1".to_string(),
                    weight: 1,
                    model_mapping: simple_mapping(&[
                        ("gpt-4", "gpt-4-turbo"),                // Exact match
                        ("claude-opus-4-5-.*", "claude-mapped"), // Regex pattern
                        ("gemini-*", "gemini-pro"),              // Simple wildcard
                    ]),
                    provider_type: "openai".to_string(),
                    provider_params: HashMap::new(),
                },
                ProviderConfig {
                    name: "provider2".to_string(),
                    api_base: "https://api2.com".to_string(),
                    api_key: "key2".to_string(),
                    weight: 1,
                    model_mapping: simple_mapping(&[
                        ("gpt-3.5-turbo", "gpt-3.5-turbo-0125"), // Exact match
                        ("claude-.*", "claude-default"),         // Regex pattern
                    ]),
                    provider_type: "openai".to_string(),
                    provider_params: HashMap::new(),
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            ttft_timeout_secs: None,
            credentials: vec![],
            provider_suffix: None,
            min_tokens_limit: 1,
            max_tokens_limit: 128000,
        };

        let service = ProviderService::new(config);
        let models = service.get_all_models();

        // Only exact matches should be returned
        assert!(models.contains("gpt-4"));
        assert!(models.contains("gpt-3.5-turbo"));
        assert_eq!(models.len(), 2);

        // Patterns should NOT be in the result
        assert!(!models.contains("claude-opus-4-5-.*"));
        assert!(!models.contains("gemini-*"));
        assert!(!models.contains("claude-.*"));
    }

    #[test]
    fn test_adaptive_routing_degrades_on_429_and_recovers() {
        init_metrics();

        let mut config = create_single_provider_config();
        config.providers[0].model_mapping.insert(
            "model1".to_string(),
            ModelMappingValue::Simple("provider-model1".to_string()),
        );

        let service = ProviderService::new_with_adaptive(config, true);
        assert!(service.adaptive_enabled());

        service.report_http_status("OnlyProvider", 429, Some("5"));
        service.report_http_status("OnlyProvider", 429, Some("5"));
        service.report_http_status("OnlyProvider", 429, Some("5"));

        let probe = service.get_next_provider(Some("model1")).unwrap();
        assert_eq!(probe.name, "OnlyProvider");

        service.report_success("OnlyProvider");
        service.report_success("OnlyProvider");

        let selected = service.get_next_provider(Some("model1")).unwrap();
        assert_eq!(selected.name, "OnlyProvider");
    }

    #[test]
    fn test_retry_after_seconds_parsing() {
        assert_eq!(parse_retry_after_seconds(Some("15")), Some(15));
        assert!(parse_retry_after_seconds(Some("Wed, 21 Oct 2015 07:28:00 GMT")).is_some());
        assert_eq!(parse_retry_after_seconds(Some("")), None);
        assert_eq!(parse_retry_after_seconds(Some("invalid")), None);
    }

    // ========================================================================
    // Adaptive Routing: Circuit Breaker Tests
    // ========================================================================

    #[test]
    fn test_5xx_triggers_circuit_open() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Below threshold: should stay Closed
        for _ in 0..4 {
            service.report_http_status("OnlyProvider", 500, None);
        }
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Closed);
            assert_eq!(state.consecutive_5xx, 4);
            assert!(state.multiplier < 1.0);
        }

        // 5th hit crosses threshold (consecutive_5xx_threshold = 5)
        service.report_http_status("OnlyProvider", 502, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
            assert!(state.open_until.is_some());
            assert_eq!(state.ejection_count, 1);
        }
    }

    #[test]
    fn test_transport_error_triggers_circuit_open() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Below threshold: should stay Closed
        for _ in 0..3 {
            service.report_transport_error("OnlyProvider");
        }
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Closed);
            assert_eq!(state.consecutive_transport, 3);
        }

        // 4th hit crosses threshold (consecutive_transport_threshold = 4)
        service.report_transport_error("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
            assert!(state.open_until.is_some());
            assert_eq!(state.ejection_count, 1);
        }
    }

    #[test]
    fn test_open_to_half_open_promotion() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Trigger Open state
        for _ in 0..5 {
            service.report_http_status("OnlyProvider", 500, None);
        }
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
        }

        // Simulate open_until expiry by manually setting it to the past
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
        }

        // Next report_success triggers promote_open_to_half_open_if_needed
        service.report_success("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            // After promotion to HalfOpen + one success, still HalfOpen (need 2 successes)
            assert_eq!(state.circuit_state, CircuitState::HalfOpen);
            assert_eq!(state.half_open_successes, 1);
        }
    }

    #[test]
    fn test_half_open_success_recovers_to_closed() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Trigger Open state
        for _ in 0..5 {
            service.report_http_status("OnlyProvider", 500, None);
        }

        // Force transition to HalfOpen
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
        }

        // First success: promotes Open→HalfOpen, then records 1 success in HalfOpen
        service.report_success("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::HalfOpen);
            assert_eq!(state.half_open_successes, 1);
        }

        // Second success in HalfOpen: crosses half_open_success_threshold (2) → Closed
        service.report_success("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Closed);
            assert_eq!(state.half_open_successes, 0);
            assert!(state.recovery_started_at.is_some()); // slow-start active
            assert!(state.multiplier >= 0.3); // guaranteed minimum after HalfOpen recovery
        }
    }

    #[test]
    fn test_half_open_failure_reopens_circuit() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Trigger Open → force HalfOpen
        for _ in 0..5 {
            service.report_http_status("OnlyProvider", 500, None);
        }
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
            // Manually promote to HalfOpen
            state.circuit_state = CircuitState::HalfOpen;
            state.half_open_successes = 0;
            state.consecutive_5xx = 0;
        }

        // A single 5xx while in HalfOpen should immediately re-open the circuit
        service.report_http_status("OnlyProvider", 500, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
            assert_eq!(state.ejection_count, 2); // second ejection
        }
    }

    #[test]
    fn test_exponential_backoff_cooldown() {
        // Verify next_backoff produces exponential durations
        let base = Duration::from_secs(30);
        let max = Duration::from_secs(300);

        assert_eq!(next_backoff(0, base, max), Duration::from_secs(30)); // 30 * 2^0
        assert_eq!(next_backoff(1, base, max), Duration::from_secs(60)); // 30 * 2^1
        assert_eq!(next_backoff(2, base, max), Duration::from_secs(120)); // 30 * 2^2
        assert_eq!(next_backoff(3, base, max), Duration::from_secs(240)); // 30 * 2^3
        assert_eq!(next_backoff(4, base, max), Duration::from_secs(300)); // capped at max
        assert_eq!(next_backoff(10, base, max), Duration::from_secs(300)); // still capped
    }

    #[test]
    fn test_all_providers_ejected_fallback() {
        init_metrics();

        let mut config = create_test_config();
        // Give both providers the same model so they are both eligible
        config.providers[0].model_mapping.insert(
            "shared".to_string(),
            ModelMappingValue::Simple("p1-shared".to_string()),
        );
        config.providers[1].model_mapping.insert(
            "shared".to_string(),
            ModelMappingValue::Simple("p2-shared".to_string()),
        );
        let service = ProviderService::new_with_adaptive(config, true);

        // Eject Provider1 via 5xx
        for _ in 0..5 {
            service.report_http_status("Provider1", 500, None);
        }
        // Eject Provider2 via transport errors
        for _ in 0..4 {
            service.report_transport_error("Provider2");
        }
        {
            let s1 = service.runtime_states.get("Provider1").unwrap();
            let s2 = service.runtime_states.get("Provider2").unwrap();
            assert_eq!(s1.circuit_state, CircuitState::Open);
            assert_eq!(s2.circuit_state, CircuitState::Open);
        }

        // Despite both being Open, get_next_provider should still return a probe fallback
        let probe = service.get_next_provider(Some("shared")).unwrap();
        assert!(
            probe.name == "Provider1" || probe.name == "Provider2",
            "Expected a fallback probe provider, got: {}",
            probe.name
        );
    }

    #[test]
    fn test_error_counter_isolation() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Mixed errors should reset each other's counters
        service.report_http_status("OnlyProvider", 429, None);
        service.report_http_status("OnlyProvider", 429, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_429, 2);
        }

        // A 5xx should reset 429 counter
        service.report_http_status("OnlyProvider", 500, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_429, 0);
            assert_eq!(state.consecutive_5xx, 1);
        }

        // A transport error should reset 5xx counter
        service.report_transport_error("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_5xx, 0);
            assert_eq!(state.consecutive_transport, 1);
        }

        // A success should reset all counters
        service.report_success("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_429, 0);
            assert_eq!(state.consecutive_5xx, 0);
            assert_eq!(state.consecutive_transport, 0);
        }
    }

    #[test]
    fn test_multiplier_degradation_and_recovery() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Initial multiplier is 1.0
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 1.0).abs() < f64::EPSILON);
        }

        // 429 halves the multiplier: 1.0 → 0.5
        service.report_http_status("OnlyProvider", 429, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 0.5).abs() < f64::EPSILON);
        }

        // 5xx applies 0.7x: 0.5 → 0.35
        service.report_http_status("OnlyProvider", 500, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 0.35).abs() < 0.001);
        }

        // success_recovery_step is 0.1: 0.35 → 0.45
        service.report_success("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 0.45).abs() < 0.001);
        }

        // Multiple successes should eventually cap at 1.0
        for _ in 0..20 {
            service.report_success("OnlyProvider");
        }
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_slow_start_factor() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Set up state with recovery_started_at in the past (30s ago, halfway through 60s window)
        let thirty_secs_ago = Instant::now() - Duration::from_secs(30);
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.circuit_state = CircuitState::Closed;
            state.recovery_started_at = Some(thirty_secs_ago);
        }

        // get_slow_start_factor should return ~0.5 (30/60 progress)
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            let factor = service.get_slow_start_factor(&mut state, Instant::now());
            assert!(factor > 0.4 && factor < 0.6, "Factor was {}", factor);
        }

        // After window expires, factor should be 1.0
        let seventy_secs_ago = Instant::now() - Duration::from_secs(70);
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.recovery_started_at = Some(seventy_secs_ago);
            let factor = service.get_slow_start_factor(&mut state, Instant::now());
            assert!((factor - 1.0).abs() < f64::EPSILON);
            assert!(state.recovery_started_at.is_none()); // should be cleared
        }
    }

    #[test]
    fn test_adaptive_disabled_ignores_reports() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, false);
        assert!(!service.adaptive_enabled());

        // Reports should be no-ops
        service.report_http_status("OnlyProvider", 429, None);
        service.report_http_status("OnlyProvider", 500, None);
        service.report_transport_error("OnlyProvider");
        service.report_success("OnlyProvider");

        // State should remain at defaults
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Closed);
            assert!((state.multiplier - 1.0).abs() < f64::EPSILON);
            assert_eq!(state.consecutive_429, 0);
            assert_eq!(state.consecutive_5xx, 0);
            assert_eq!(state.consecutive_transport, 0);
        }
    }

    #[test]
    fn test_half_open_concurrency_limit() {
        init_metrics();

        let mut config = create_test_config();
        config.providers[0].model_mapping.insert(
            "shared".to_string(),
            ModelMappingValue::Simple("p1-shared".to_string()),
        );
        config.providers[1].model_mapping.insert(
            "shared".to_string(),
            ModelMappingValue::Simple("p2-shared".to_string()),
        );
        let service = ProviderService::new_with_adaptive(config, true);

        // Eject Provider1 via 5xx, keep Provider2 healthy
        for _ in 0..5 {
            service.report_http_status("Provider1", 500, None);
        }
        {
            let state = service.runtime_states.get("Provider1").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
        }

        // Force Provider1 into HalfOpen
        {
            let mut state = service.runtime_states.get_mut("Provider1").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
            state.circuit_state = CircuitState::HalfOpen;
            state.half_open_in_flight = 0;
        }

        // Simulate that the single allowed probe slot is already taken
        {
            let mut state = service.runtime_states.get_mut("Provider1").unwrap();
            state.half_open_in_flight = 1; // at limit (default max_probes=1)
        }

        // With Provider1 at the in-flight limit, all requests should go to Provider2
        for _ in 0..20 {
            let provider = service.get_next_provider(Some("shared")).unwrap();
            assert_eq!(
                provider.name, "Provider2",
                "HalfOpen provider should be excluded when at probe limit"
            );
        }

        // After completing the probe (success), the slot frees up
        service.report_success("Provider1");
        {
            let state = service.runtime_states.get("Provider1").unwrap();
            assert_eq!(state.half_open_in_flight, 0);
        }

        // Now Provider1 should be eligible again as a HalfOpen probe
        let mut saw_provider1 = false;
        for _ in 0..100 {
            let provider = service.get_next_provider(Some("shared")).unwrap();
            if provider.name == "Provider1" {
                saw_provider1 = true;
                break;
            }
        }
        assert!(
            saw_provider1,
            "Provider1 should be selectable after probe slot freed"
        );
    }

    // ========================================================================
    // Adaptive Routing: Additional Edge Case Tests
    // ========================================================================

    #[test]
    fn test_429_with_retry_after_header() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Report 429 with Retry-After: 30 seconds
        service.report_http_status("OnlyProvider", 429, Some("30"));
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_429, 1);
            assert!(state.cooldown_until.is_some());
            assert!((state.multiplier - 0.5).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_429_resets_5xx_and_transport_counters() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        service.report_http_status("OnlyProvider", 500, None);
        service.report_http_status("OnlyProvider", 500, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_5xx, 2);
        }

        // A 429 should reset 5xx counter
        service.report_http_status("OnlyProvider", 429, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.consecutive_5xx, 0);
            assert_eq!(state.consecutive_429, 1);
        }
    }

    #[test]
    fn test_4xx_non_429_non_408_no_penalty() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // 401/403/404 should not degrade multiplier
        service.report_http_status("OnlyProvider", 401, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 1.0).abs() < f64::EPSILON);
            assert_eq!(state.circuit_state, CircuitState::Closed);
        }

        service.report_http_status("OnlyProvider", 404, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert!((state.multiplier - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_408_slight_penalty() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        service.report_http_status("OnlyProvider", 408, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            // 408 applies 0.9x: 1.0 → 0.9
            assert!((state.multiplier - 0.9).abs() < 0.001);
        }
    }

    #[test]
    fn test_200_status_treated_as_success() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Degrade first
        service.report_http_status("OnlyProvider", 429, None);
        let before = {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            state.multiplier
        };

        // 200 through report_http_status triggers report_success internally
        service.report_http_status("OnlyProvider", 200, None);
        let after = {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            state.multiplier
        };

        assert!(after > before, "Success should recover multiplier");
    }

    #[test]
    fn test_half_open_429_immediately_reopens() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Open the circuit
        for _ in 0..5 {
            service.report_http_status("OnlyProvider", 500, None);
        }

        // Force HalfOpen
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
            state.circuit_state = CircuitState::HalfOpen;
            state.half_open_successes = 0;
            state.consecutive_429 = 0;
        }

        // A single 429 in HalfOpen should immediately re-open
        service.report_http_status("OnlyProvider", 429, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
        }
    }

    #[test]
    fn test_half_open_transport_error_immediately_reopens() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Open → force HalfOpen
        for _ in 0..5 {
            service.report_http_status("OnlyProvider", 500, None);
        }
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
            state.circuit_state = CircuitState::HalfOpen;
            state.consecutive_transport = 0;
        }

        // A single transport error in HalfOpen should immediately re-open
        service.report_transport_error("OnlyProvider");
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.circuit_state, CircuitState::Open);
        }
    }

    #[test]
    fn test_ejection_count_increments_per_open() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // First ejection
        for _ in 0..5 {
            service.report_http_status("OnlyProvider", 500, None);
        }
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.ejection_count, 1);
        }

        // Force HalfOpen then re-open
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.open_until = Some(Instant::now() - Duration::from_secs(1));
            state.circuit_state = CircuitState::HalfOpen;
            state.consecutive_5xx = 0;
        }
        service.report_http_status("OnlyProvider", 500, None);
        {
            let state = service.runtime_states.get("OnlyProvider").unwrap();
            assert_eq!(state.ejection_count, 2);
        }
    }

    #[test]
    fn test_report_unknown_provider_is_noop() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Should not panic or crash
        service.report_http_status("NonExistentProvider", 500, None);
        service.report_transport_error("NonExistentProvider");
        service.report_success("NonExistentProvider");
    }

    #[test]
    fn test_multiplier_floor_at_min_multiplier() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Repeatedly degrade — multiplier should never go below min_multiplier (0.05)
        for _ in 0..50 {
            service.report_http_status("OnlyProvider", 429, None);
            // Reset consecutive count to avoid circuit open
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.consecutive_429 = 0;
            state.circuit_state = CircuitState::Closed;
            state.cooldown_until = None;
        }

        let state = service.runtime_states.get("OnlyProvider").unwrap();
        assert!(
            state.multiplier >= 0.05,
            "Multiplier should not go below min_multiplier, got: {}",
            state.multiplier
        );
    }

    #[test]
    fn test_slow_start_factor_early_in_window() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        // Set recovery_started_at to 5 seconds ago (early in 60s window)
        let five_secs_ago = Instant::now() - Duration::from_secs(5);
        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.circuit_state = CircuitState::Closed;
            state.recovery_started_at = Some(five_secs_ago);
        }

        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            let factor = service.get_slow_start_factor(&mut state, Instant::now());
            // ~5/60 ≈ 0.083, but clamped to half_open_weight_factor (0.2) minimum
            assert!(
                (0.2 - 0.01..=0.2).contains(&factor),
                "Factor was {}",
                factor
            );
        }
    }

    #[test]
    fn test_slow_start_not_active_when_no_recovery() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.circuit_state = CircuitState::Closed;
            state.recovery_started_at = None;
            let factor = service.get_slow_start_factor(&mut state, Instant::now());
            assert!((factor - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_slow_start_not_active_in_open_state() {
        init_metrics();
        let config = create_single_provider_config();
        let service = ProviderService::new_with_adaptive(config, true);

        {
            let mut state = service.runtime_states.get_mut("OnlyProvider").unwrap();
            state.circuit_state = CircuitState::Open;
            state.recovery_started_at = Some(Instant::now() - Duration::from_secs(30));
            let factor = service.get_slow_start_factor(&mut state, Instant::now());
            assert!((factor - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_parse_retry_after_rfc2822_past_date() {
        // A past date should return Some(0)
        assert_eq!(
            parse_retry_after_seconds(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            Some(0)
        );
    }

    #[test]
    fn test_parse_retry_after_none() {
        assert_eq!(parse_retry_after_seconds(None), None);
    }

    #[test]
    fn test_parse_retry_after_whitespace() {
        assert_eq!(parse_retry_after_seconds(Some("  ")), None);
        assert_eq!(parse_retry_after_seconds(Some("  10  ")), Some(10));
    }

    #[test]
    fn test_next_backoff_high_step_capped() {
        let base = Duration::from_secs(30);
        let max = Duration::from_secs(300);
        // step=20 should be capped at step=10 internally
        assert_eq!(next_backoff(20, base, max), Duration::from_secs(300));
    }

    #[test]
    fn test_next_backoff_zero_base() {
        let base = Duration::from_secs(0);
        let max = Duration::from_secs(300);
        assert_eq!(next_backoff(0, base, max), Duration::from_secs(0));
        assert_eq!(next_backoff(5, base, max), Duration::from_secs(0));
    }
}
