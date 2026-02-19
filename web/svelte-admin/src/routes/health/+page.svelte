<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { auth } from '$lib/stores';
  import type { HealthCheckResponse, HealthStatus } from '$lib/types';
  import {
    RefreshCw,
    RotateCw,
    Loader2,
    AlertCircle,
    X,
    ChevronDown,
    ChevronUp,
    CheckCircle2,
    XCircle,
    MinusCircle,
    CircleDashed,
    Clock,
    Zap,
    Server,
    ShieldCheck,
    ShieldAlert,
    HeartPulse,
    Eye,
    EyeOff,
  } from 'lucide-svelte';
  import ProviderIcon from '$lib/components/ProviderIcon.svelte';

  const HEALTH_CACHE_KEY = 'llm-proxy-health-check-cache';

  interface HealthCheckCache {
    timestamp: string;
    data: HealthCheckResponse;
  }

  let healthData = $state<HealthCheckResponse | null>(null);
  let lastCheckTime = $state<string | null>(null);
  let checking = $state(false);
  let reloading = $state(false);
  let checkingProviders: Set<number> = new SvelteSet();
  let checkingModels: Set<string> = new SvelteSet();
  let error = $state<string | null>(null);
  let expandedProviders: Set<number> = new SvelteSet();
  let showDisabled = $state(false);

  onMount(() => {
    const cached = localStorage.getItem(HEALTH_CACHE_KEY);
    if (cached) {
      try {
        const { timestamp, data }: HealthCheckCache = JSON.parse(cached);
        healthData = data;
        lastCheckTime = timestamp;
        if (data.providers) {
          expandedProviders = new SvelteSet(
            data.providers
              .filter(p => p.status === 'unhealthy')
              .map(p => p.provider_id)
          );
        }
      } catch {
        localStorage.removeItem(HEALTH_CACHE_KEY);
      }
    }
  });

  async function handleReloadProviders() {
    const client = auth.apiClient;
    if (!client) return;

    reloading = true;
    error = null;

    try {
      const providersResponse = await client.listProviders();
      const filteredProviders = showDisabled
        ? providersResponse.providers
        : providersResponse.providers.filter(p => p.is_enabled);
      const updatedProviders: import('$lib/types').ProviderHealthStatus[] =
        filteredProviders.map(p => {
          const existing = healthData?.providers.find(
            ep => ep.provider_id === p.id
          );
          return (
            existing || {
              provider_id: p.id,
              provider_key: p.provider_key,
              provider_type: p.provider_type,
              status: (p.is_enabled
                ? 'unknown'
                : 'disabled') as import('$lib/types').HealthStatus,
              models: [],
              avg_response_time_ms: null,
              checked_at: new Date().toISOString(),
            }
          );
        });

      const updatedHealthData: HealthCheckResponse = {
        providers: updatedProviders,
        total_providers: updatedProviders.length,
        healthy_providers: updatedProviders.filter(p => p.status === 'healthy')
          .length,
        unhealthy_providers: updatedProviders.filter(
          p => p.status === 'unhealthy'
        ).length,
      };

      healthData = updatedHealthData;

      const cache: HealthCheckCache = {
        timestamp: lastCheckTime || new Date().toISOString(),
        data: updatedHealthData,
      };
      localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to reload providers';
    } finally {
      reloading = false;
    }
  }

  async function handleCheckHealth() {
    const client = auth.apiClient;
    if (!client) return;

    checking = true;
    error = null;

    try {
      const providersResponse = await client.listProviders();
      const allProviders = providersResponse.providers;
      const displayProviders = showDisabled
        ? allProviders
        : allProviders.filter(p => p.is_enabled);
      const providerIds = allProviders.filter(p => p.is_enabled).map(p => p.id);
      const initialProviders: import('$lib/types').ProviderHealthStatus[] =
        displayProviders.map(p => {
          const existing = healthData?.providers.find(
            ep => ep.provider_id === p.id
          );
          return (
            existing || {
              provider_id: p.id,
              provider_key: p.provider_key,
              provider_type: p.provider_type,
              status: (p.is_enabled
                ? 'unknown'
                : 'disabled') as import('$lib/types').HealthStatus,
              models: [],
              avg_response_time_ms: null,
              checked_at: new Date().toISOString(),
            }
          );
        });
      healthData = {
        providers: initialProviders,
        total_providers: initialProviders.length,
        healthy_providers: initialProviders.filter(p => p.status === 'healthy')
          .length,
        unhealthy_providers: initialProviders.filter(
          p => p.status === 'unhealthy'
        ).length,
      };

      const checkPromises = providerIds.map(async id => {
        checkingProviders.add(id);
        checkingProviders = checkingProviders;
        try {
          const response = await client.checkProviderHealth(id, {
            max_concurrent: 2,
            timeout_secs: 30,
          });

          if (healthData) {
            const updatedProviders = healthData.providers.map(p => {
              if (p.provider_id === id) {
                return {
                  provider_id: response.provider_id,
                  provider_key: response.provider_key,
                  provider_type: response.provider_type,
                  status: response.status,
                  models: response.models,
                  avg_response_time_ms:
                    response.models.reduce(
                      (sum: number, m) => sum + (m.response_time_ms || 0),
                      0
                    ) / response.models.length || null,
                  checked_at: response.checked_at,
                };
              }
              return p;
            });

            const updatedHealthData = {
              ...healthData,
              providers: updatedProviders,
              healthy_providers: updatedProviders.filter(
                p => p.status === 'healthy'
              ).length,
              unhealthy_providers: updatedProviders.filter(
                p => p.status === 'unhealthy'
              ).length,
            };

            healthData = updatedHealthData;

            const cache: HealthCheckCache = {
              timestamp: new Date().toISOString(),
              data: updatedHealthData,
            };
            localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

            if (response.status === 'unhealthy') {
              expandedProviders.add(id);
              expandedProviders = expandedProviders;
            }
          }
        } catch {
          // Provider check failed silently
        } finally {
          checkingProviders.delete(id);
          checkingProviders = checkingProviders;
        }
      });

      await Promise.all(checkPromises);

      const timestamp = new Date().toISOString();
      lastCheckTime = timestamp;
    } catch (err) {
      error =
        err instanceof Error ? err.message : 'Failed to check health status';
    } finally {
      checking = false;
    }
  }

  async function handleCheckProviderHealth(providerId: number) {
    const client = auth.apiClient;
    if (!client) return;

    checkingProviders.add(providerId);
    checkingProviders = checkingProviders;
    error = null;

    try {
      const response = await client.checkProviderHealth(providerId, {
        max_concurrent: 2,
        timeout_secs: 30,
      });

      if (healthData) {
        const updatedProviders = healthData.providers.map(p => {
          if (p.provider_id === providerId) {
            return {
              provider_id: response.provider_id,
              provider_key: response.provider_key,
              provider_type: response.provider_type,
              status: response.status,
              models: response.models,
              avg_response_time_ms:
                response.models.reduce(
                  (sum: number, m) => sum + (m.response_time_ms || 0),
                  0
                ) / response.models.length || null,
              checked_at: response.checked_at,
            };
          }
          return p;
        });

        const updatedHealthData = {
          ...healthData,
          providers: updatedProviders,
          healthy_providers: updatedProviders.filter(
            p => p.status === 'healthy'
          ).length,
          unhealthy_providers: updatedProviders.filter(
            p => p.status === 'unhealthy'
          ).length,
        };

        healthData = updatedHealthData;

        const cache: HealthCheckCache = {
          timestamp: lastCheckTime || new Date().toISOString(),
          data: updatedHealthData,
        };
        localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

        if (response.status === 'unhealthy') {
          expandedProviders.add(providerId);
          expandedProviders = expandedProviders;
        }
      }
    } catch (err) {
      error =
        err instanceof Error
          ? err.message
          : 'Failed to check provider health status';
    } finally {
      checkingProviders.delete(providerId);
      checkingProviders = checkingProviders;
    }
  }

  async function handleCheckModelHealth(providerId: number, modelName: string) {
    const client = auth.apiClient;
    if (!client) return;

    const modelKey = `${providerId}-${modelName}`;
    checkingModels.add(modelKey);
    checkingModels = checkingModels;
    error = null;

    try {
      const response = await client.checkProviderHealth(providerId, {
        models: [modelName],
        max_concurrent: 1,
        timeout_secs: 30,
      });

      if (healthData && response.models.length > 0) {
        const modelResult = response.models[0];
        const updatedProviders = healthData.providers.map(p => {
          if (p.provider_id === providerId) {
            const updatedModels = p.models.map(m =>
              m.model === modelName ? modelResult : m
            );
            return { ...p, models: updatedModels };
          }
          return p;
        });

        healthData = {
          ...healthData,
          providers: updatedProviders,
        };

        const cache: HealthCheckCache = {
          timestamp: lastCheckTime || new Date().toISOString(),
          data: healthData,
        };
        localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));
      }
    } catch (err) {
      error =
        err instanceof Error ? err.message : 'Failed to check model health';
    } finally {
      checkingModels.delete(modelKey);
      checkingModels = checkingModels;
    }
  }

  function toggleProvider(providerId: number) {
    if (expandedProviders.has(providerId)) {
      expandedProviders.delete(providerId);
    } else {
      expandedProviders.add(providerId);
    }
    expandedProviders = expandedProviders;
  }

  function getCardBorderClass(status: HealthStatus) {
    switch (status) {
      case 'healthy':
        return 'border-l-4 border-l-emerald-500';
      case 'unhealthy':
        return 'border-l-4 border-l-red-500';
      case 'disabled':
        return 'border-l-4 border-l-gray-300 dark:border-l-gray-600';
      default:
        return 'border-l-4 border-l-gray-300 dark:border-l-gray-600';
    }
  }

  function formatTimestamp(timestamp: string) {
    try {
      const date = new Date(timestamp);
      return date.toLocaleString();
    } catch {
      return timestamp;
    }
  }

  function formatResponseTime(ms: number | null) {
    if (ms === null) return 'N/A';
    if (ms < 1000) return `${Math.round(ms)}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  }

  function getResponseTimeColor(ms: number | null) {
    if (ms === null) return 'text-gray-400';
    if (ms < 500) return 'text-emerald-600 dark:text-emerald-400';
    if (ms < 2000) return 'text-amber-600 dark:text-amber-400';
    return 'text-red-600 dark:text-red-400';
  }

  function formatRelativeTime(timestamp: string) {
    try {
      const date = new Date(timestamp);
      const now = new Date();
      const diffMs = now.getTime() - date.getTime();
      const diffMins = Math.floor(diffMs / 60000);
      const diffHours = Math.floor(diffMins / 60);
      const diffDays = Math.floor(diffHours / 24);

      if (diffMins < 1) return 'just now';
      if (diffMins < 60)
        return `${diffMins} minute${diffMins !== 1 ? 's' : ''} ago`;
      if (diffHours < 24)
        return `${diffHours} hour${diffHours !== 1 ? 's' : ''} ago`;
      return `${diffDays} day${diffDays !== 1 ? 's' : ''} ago`;
    } catch {
      return timestamp;
    }
  }
</script>

<svelte:head>
  <title>Health Check - HEN Admin</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex justify-between items-center">
    <div class="flex items-center gap-3">
      <div class="p-2 bg-primary-100 dark:bg-primary-600/20 rounded-lg">
        <HeartPulse class="w-6 h-6 text-primary-600 dark:text-primary-400" />
      </div>
      <div>
        <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
          Health Check
        </h1>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Monitor provider and model health status
        </p>
      </div>
    </div>
    <div class="flex items-center gap-3">
      <div
        class="flex items-center gap-2 cursor-pointer text-sm text-gray-600 dark:text-gray-400 select-none"
      >
        <button
          onclick={() => (showDisabled = !showDisabled)}
          aria-label="Toggle show disabled providers"
          class="relative inline-flex h-5 w-9 items-center rounded-full transition-colors {showDisabled
            ? 'bg-primary-600'
            : 'bg-gray-300 dark:bg-gray-600'}"
        >
          <span
            class="inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform {showDisabled
              ? 'translate-x-4.5'
              : 'translate-x-0.75'}"
          ></span>
        </button>
        {#if showDisabled}
          <Eye class="w-3.5 h-3.5" />
        {:else}
          <EyeOff class="w-3.5 h-3.5" />
        {/if}
        <span>Show Disabled</span>
      </div>
      <button
        onclick={handleReloadProviders}
        disabled={reloading || checking}
        class="btn btn-secondary flex items-center gap-2"
      >
        <RotateCw class="w-4 h-4 {reloading ? 'animate-spin' : ''}" />
        <span>Reload</span>
      </button>
      <button
        onclick={handleCheckHealth}
        disabled={checking}
        class="btn btn-primary flex items-center gap-2"
      >
        <RefreshCw class="w-4 h-4 {checking ? 'animate-spin' : ''}" />
        <span>{checking ? 'Checking...' : 'Check All'}</span>
      </button>
    </div>
  </div>

  <!-- Error Display -->
  {#if error}
    <div class="alert-error">
      <div class="flex">
        <div class="shrink-0">
          <AlertCircle class="h-5 w-5 text-red-400" />
        </div>
        <div class="ml-3">
          <p class="text-sm text-red-700">{error}</p>
        </div>
        <div class="ml-auto pl-3">
          <button
            onclick={() => (error = null)}
            class="text-red-400 hover:text-red-600"
          >
            <X class="h-5 w-5" />
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Last Check Time -->
  {#if lastCheckTime}
    <div
      class="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 px-1"
    >
      <Clock class="w-3.5 h-3.5" />
      <span>Last checked: {formatRelativeTime(lastCheckTime)}</span>
      <span class="text-gray-300 dark:text-gray-600">|</span>
      <span class="text-xs">{formatTimestamp(lastCheckTime)}</span>
    </div>
  {/if}

  <!-- Statistics Cards -->
  {#if healthData}
    <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
      <div class="card overflow-hidden">
        <div class="card-body relative">
          <div
            class="absolute top-0 right-0 w-20 h-20 bg-blue-50 dark:bg-blue-900/10 rounded-bl-full"
          ></div>
          <div class="flex items-center justify-between relative">
            <div>
              <p
                class="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1"
              >
                Total Providers
              </p>
              <p class="text-3xl font-bold text-gray-900 dark:text-gray-100">
                {healthData.total_providers}
              </p>
            </div>
            <div class="p-3 bg-blue-100 dark:bg-blue-900/30 rounded-xl">
              <Server class="w-6 h-6 text-blue-600 dark:text-blue-400" />
            </div>
          </div>
        </div>
      </div>

      <div class="card overflow-hidden">
        <div class="card-body relative">
          <div
            class="absolute top-0 right-0 w-20 h-20 bg-emerald-50 dark:bg-emerald-900/10 rounded-bl-full"
          ></div>
          <div class="flex items-center justify-between relative">
            <div>
              <p
                class="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1"
              >
                Healthy
              </p>
              <p
                class="text-3xl font-bold text-emerald-600 dark:text-emerald-400"
              >
                {healthData.healthy_providers}
              </p>
            </div>
            <div class="p-3 bg-emerald-100 dark:bg-emerald-900/30 rounded-xl">
              <ShieldCheck
                class="w-6 h-6 text-emerald-600 dark:text-emerald-400"
              />
            </div>
          </div>
        </div>
      </div>

      <div class="card overflow-hidden">
        <div class="card-body relative">
          <div
            class="absolute top-0 right-0 w-20 h-20 bg-red-50 dark:bg-red-900/10 rounded-bl-full"
          ></div>
          <div class="flex items-center justify-between relative">
            <div>
              <p
                class="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1"
              >
                Unhealthy
              </p>
              <p class="text-3xl font-bold text-red-600 dark:text-red-400">
                {healthData.unhealthy_providers}
              </p>
            </div>
            <div class="p-3 bg-red-100 dark:bg-red-900/30 rounded-xl">
              <ShieldAlert class="w-6 h-6 text-red-600 dark:text-red-400" />
            </div>
          </div>
        </div>
      </div>
    </div>
  {/if}

  <!-- Providers Grid -->
  <div class="space-y-4">
    <div class="flex justify-between items-center">
      <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
        Provider Health Status
        {#if healthData}
          <span
            class="ml-2 text-sm font-normal text-gray-500 dark:text-gray-400"
          >
            ({healthData.providers.length})
          </span>
        {/if}
      </h2>
    </div>

    {#if !healthData}
      <div class="card">
        <div class="card-body text-center py-16">
          <div
            class="inline-flex p-4 bg-gray-100 dark:bg-gray-800 rounded-full mb-4"
          >
            <HeartPulse class="w-10 h-10 text-gray-400" />
          </div>
          <p class="text-gray-700 dark:text-gray-300 font-medium mb-1">
            No health check data available
          </p>
          <p class="text-sm text-gray-500 dark:text-gray-400 mb-6">
            Click "Check All" to start monitoring your providers
          </p>
          <button
            onclick={handleCheckHealth}
            disabled={checking}
            class="btn btn-primary"
          >
            {#if checking}
              <Loader2 class="w-4 h-4 animate-spin mr-2" />
              Checking...
            {:else}
              Check All
            {/if}
          </button>
        </div>
      </div>
    {:else if healthData.providers.length === 0}
      <div class="card">
        <div
          class="card-body text-center py-12 text-gray-500 dark:text-gray-400"
        >
          No providers configured yet.
        </div>
      </div>
    {:else}
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {#each healthData.providers as provider (provider.provider_id)}
          <div
            class="card hover:shadow-lg transition-all duration-200 {getCardBorderClass(
              provider.status
            )}"
          >
            <div class="card-body">
              <!-- Provider Header -->
              <div class="flex items-start justify-between mb-3">
                <div class="flex items-center gap-3 flex-1 min-w-0">
                  <div
                    class="shrink-0 p-1.5 bg-gray-50 dark:bg-gray-700/50 rounded-lg"
                  >
                    <ProviderIcon
                      providerKey={provider.provider_key}
                      providerType={provider.provider_type}
                      class="w-6 h-6"
                    />
                  </div>
                  <div class="flex-1 min-w-0">
                    <h3
                      class="text-base font-semibold text-gray-900 dark:text-gray-100 truncate"
                    >
                      {provider.provider_key}
                    </h3>
                    {#if provider.status === 'healthy'}
                      <span
                        class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400"
                      >
                        <span
                          class="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse"
                        ></span>
                        healthy
                      </span>
                    {:else if provider.status === 'unhealthy'}
                      <span
                        class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-red-50 text-red-700 dark:bg-red-900/30 dark:text-red-400"
                      >
                        <span class="w-1.5 h-1.5 rounded-full bg-red-500"
                        ></span>
                        unhealthy
                      </span>
                    {:else if provider.status === 'disabled'}
                      <span
                        class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400"
                      >
                        <span class="w-1.5 h-1.5 rounded-full bg-gray-400"
                        ></span>
                        disabled
                      </span>
                    {:else}
                      <span
                        class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400"
                      >
                        <span class="w-1.5 h-1.5 rounded-full bg-gray-400"
                        ></span>
                        unknown
                      </span>
                    {/if}
                  </div>
                </div>
                {#if checkingProviders.has(provider.provider_id)}
                  <Loader2
                    class="w-4 h-4 animate-spin text-blue-500 shrink-0"
                  />
                {/if}
              </div>

              <!-- Provider Stats -->
              <div
                class="grid grid-cols-2 gap-3 mb-3 py-3 border-y border-gray-100 dark:border-gray-700"
              >
                <div>
                  <p class="text-xs text-gray-500 dark:text-gray-400 mb-0.5">
                    Models
                  </p>
                  <p
                    class="text-sm font-semibold text-gray-900 dark:text-gray-100"
                  >
                    {provider.models.length}
                  </p>
                </div>
                <div>
                  <p class="text-xs text-gray-500 dark:text-gray-400 mb-0.5">
                    Avg Response
                  </p>
                  <p
                    class="text-sm font-semibold flex items-center gap-1 {getResponseTimeColor(
                      provider.avg_response_time_ms
                    )}"
                  >
                    <Zap class="w-3 h-3" />
                    {formatResponseTime(provider.avg_response_time_ms)}
                  </p>
                </div>
              </div>

              <!-- Action Buttons -->
              <div class="flex gap-2 mb-3">
                <button
                  onclick={e => {
                    e.stopPropagation();
                    handleCheckProviderHealth(provider.provider_id);
                  }}
                  disabled={checkingProviders.has(provider.provider_id)}
                  class="btn btn-sm btn-secondary flex-1 flex items-center justify-center gap-1.5"
                  title="Check this provider's health"
                >
                  <RefreshCw
                    class="w-3.5 h-3.5 {checkingProviders.has(
                      provider.provider_id
                    )
                      ? 'animate-spin'
                      : ''}"
                  />
                  <span>Check</span>
                </button>
                <button
                  onclick={() => toggleProvider(provider.provider_id)}
                  class="btn btn-sm btn-ghost flex items-center justify-center gap-1"
                >
                  <span class="text-xs">
                    {expandedProviders.has(provider.provider_id)
                      ? 'Hide'
                      : 'Details'}
                  </span>
                  {#if expandedProviders.has(provider.provider_id)}
                    <ChevronUp class="w-3.5 h-3.5" />
                  {:else}
                    <ChevronDown class="w-3.5 h-3.5" />
                  {/if}
                </button>
              </div>

              <!-- Last Checked -->
              <div
                class="flex items-center justify-center gap-1 text-xs text-gray-400 dark:text-gray-500"
              >
                <Clock class="w-3 h-3" />
                <span>{formatTimestamp(provider.checked_at)}</span>
              </div>

              <!-- Model Details (Expanded) -->
              {#if expandedProviders.has(provider.provider_id)}
                <div
                  class="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700 space-y-2"
                >
                  <h4
                    class="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-2"
                  >
                    Model Results
                  </h4>
                  {#if checkingProviders.has(provider.provider_id)}
                    <div
                      class="flex items-center justify-center p-6 bg-gray-50 dark:bg-gray-900 rounded-lg"
                    >
                      <Loader2
                        class="w-5 h-5 animate-spin text-blue-500 mr-2"
                      />
                      <span class="text-sm text-gray-600 dark:text-gray-400">
                        Checking...
                      </span>
                    </div>
                  {:else}
                    <div class="space-y-1.5">
                      {#each provider.models as model, idx (idx)}
                        <div
                          class="flex items-center justify-between p-2.5 bg-gray-50 dark:bg-gray-900 rounded-lg group"
                        >
                          <div class="flex items-center gap-2 min-w-0 flex-1">
                            {#if model.status === 'healthy'}
                              <CheckCircle2
                                class="w-4 h-4 text-emerald-500 shrink-0"
                              />
                            {:else if model.status === 'unhealthy'}
                              <XCircle class="w-4 h-4 text-red-500 shrink-0" />
                            {:else if model.status === 'disabled'}
                              <MinusCircle
                                class="w-4 h-4 text-gray-400 shrink-0"
                              />
                            {:else}
                              <CircleDashed
                                class="w-4 h-4 text-gray-400 shrink-0"
                              />
                            {/if}
                            <span
                              class="text-sm text-gray-900 dark:text-gray-100 truncate"
                            >
                              {model.model}
                            </span>
                          </div>
                          <div class="flex items-center gap-2 shrink-0">
                            {#if model.response_time_ms !== null}
                              <span
                                class="text-xs font-mono {getResponseTimeColor(
                                  model.response_time_ms
                                )}"
                              >
                                {formatResponseTime(model.response_time_ms)}
                              </span>
                            {/if}
                            <button
                              onclick={e => {
                                e.stopPropagation();
                                handleCheckModelHealth(
                                  provider.provider_id,
                                  model.model
                                );
                              }}
                              disabled={checkingModels.has(
                                `${provider.provider_id}-${model.model}`
                              )}
                              class="opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:bg-gray-200 dark:hover:bg-gray-700 rounded cursor-pointer"
                              title="Check {model.model}"
                            >
                              <RefreshCw
                                class="w-3 h-3 text-gray-500 {checkingModels.has(
                                  `${provider.provider_id}-${model.model}`
                                )
                                  ? 'animate-spin'
                                  : ''}"
                              />
                            </button>
                          </div>
                        </div>
                        {#if model.error}
                          <p
                            class="text-xs text-red-600 dark:text-red-400 pl-6"
                          >
                            {model.error}
                          </p>
                        {/if}
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
