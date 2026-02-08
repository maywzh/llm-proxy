<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { auth } from '$lib/stores';
  import type { HealthCheckResponse, HealthStatus } from '$lib/types';
  import {
    Activity,
    RefreshCw,
    Loader2,
    AlertCircle,
    X,
    ChevronDown,
    ChevronUp,
    Check,
    XCircle,
    MinusCircle,
    HelpCircle,
    Clock,
  } from 'lucide-svelte';

  const HEALTH_CACHE_KEY = 'llm-proxy-health-check-cache';

  interface HealthCheckCache {
    timestamp: string;
    data: HealthCheckResponse;
  }

  let healthData = $state<HealthCheckResponse | null>(null);
  let lastCheckTime = $state<string | null>(null);
  let checking = $state(false);
  let checkingProviders: Set<number> = new SvelteSet();
  let error = $state<string | null>(null);
  let expandedProviders: Set<number> = new SvelteSet();

  // Load cached data on mount
  onMount(() => {
    const cached = localStorage.getItem(HEALTH_CACHE_KEY);
    if (cached) {
      try {
        const { timestamp, data }: HealthCheckCache = JSON.parse(cached);
        healthData = data;
        lastCheckTime = timestamp;
        // Auto-expand unhealthy providers
        if (data.providers) {
          expandedProviders = new SvelteSet(
            data.providers
              .filter(p => p.status === 'unhealthy')
              .map(p => p.provider_id)
          );
        }
      } catch {
        // Failed to load cached health data
        localStorage.removeItem(HEALTH_CACHE_KEY);
      }
    }
  });

  async function handleCheckHealth() {
    const client = auth.apiClient;
    if (!client) return;

    checking = true;
    error = null;

    try {
      // Always fetch fresh provider list to pick up newly added providers
      const providersResponse = await client.listProviders();
      const providerIds = providersResponse.providers.map(p => p.id);
      // Initialize health data, preserving existing results for known providers
      const initialProviders: import('$lib/types').ProviderHealthStatus[] =
        providersResponse.providers.map(p => {
          const existing = healthData?.providers.find(
            ep => ep.provider_id === p.id
          );
          return (
            existing || {
              provider_id: p.id,
              provider_key: p.provider_key,
              status: 'unknown' as import('$lib/types').HealthStatus,
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

      // Check all providers in parallel, updating each as results come in
      const checkPromises = providerIds.map(async id => {
        checkingProviders.add(id);
        checkingProviders = checkingProviders;
        try {
          const response = await client.checkProviderHealth(id, {
            max_concurrent: 2,
            timeout_secs: 30,
          });

          // Update this specific provider in healthData
          if (healthData) {
            const updatedProviders = healthData.providers.map(p => {
              if (p.provider_id === id) {
                return {
                  provider_id: response.provider_id,
                  provider_key: response.provider_key,
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

            // Update cache
            const cache: HealthCheckCache = {
              timestamp: new Date().toISOString(),
              data: updatedHealthData,
            };
            localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

            // Auto-expand if unhealthy
            if (response.status === 'unhealthy') {
              expandedProviders.add(id);
              expandedProviders = expandedProviders;
            }
          }
        } catch {
          // Error checking provider - silently continue with other providers
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

      // Update the provider in healthData
      if (healthData) {
        const updatedProviders = healthData.providers.map(p => {
          if (p.provider_id === providerId) {
            // Convert CheckProviderHealthResponse to ProviderHealthStatus format
            return {
              provider_id: response.provider_id,
              provider_key: response.provider_key,
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

        // Update cache
        const cache: HealthCheckCache = {
          timestamp: lastCheckTime || new Date().toISOString(),
          data: updatedHealthData,
        };
        localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

        // Auto-expand if unhealthy
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

  function toggleProvider(providerId: number) {
    if (expandedProviders.has(providerId)) {
      expandedProviders.delete(providerId);
    } else {
      expandedProviders.add(providerId);
    }
    expandedProviders = expandedProviders;
  }

  function getStatusIcon(status: HealthStatus) {
    switch (status) {
      case 'healthy':
        return Check;
      case 'unhealthy':
        return XCircle;
      case 'disabled':
        return MinusCircle;
      default:
        return HelpCircle;
    }
  }

  function getStatusBadgeClass(status: HealthStatus) {
    switch (status) {
      case 'healthy':
        return 'badge badge-success';
      case 'unhealthy':
        return 'badge badge-danger';
      case 'disabled':
        return 'badge badge-secondary';
      default:
        return 'badge badge-secondary';
    }
  }

  function getStatusIconClass(status: HealthStatus) {
    switch (status) {
      case 'healthy':
        return 'text-green-500';
      case 'unhealthy':
        return 'text-red-500';
      case 'disabled':
        return 'text-gray-400';
      default:
        return 'text-gray-400';
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
    return `${ms}ms`;
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
  <title>Health Check - LLM Proxy Admin</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex justify-between items-center">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
        Health Check
      </h1>
      <p class="text-gray-600 dark:text-gray-400">
        Monitor provider and model health status
      </p>
    </div>
    <button
      onclick={handleCheckHealth}
      disabled={checking}
      class="btn btn-primary flex items-center space-x-2"
    >
      <RefreshCw class="w-5 h-5 {checking ? 'animate-spin' : ''}" />
      <span>Check Health</span>
    </button>
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
    <div class="card">
      <div class="card-body py-3">
        <div
          class="flex items-center space-x-2 text-sm text-gray-600 dark:text-gray-400"
        >
          <Clock class="w-4 h-4" />
          <span>Last checked: {formatRelativeTime(lastCheckTime)}</span>
          <span class="text-gray-400">•</span>
          <span class="text-xs">{formatTimestamp(lastCheckTime)}</span>
        </div>
      </div>
    </div>
  {/if}

  <!-- Statistics Cards -->
  {#if healthData}
    <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
      <div class="card">
        <div class="card-body">
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm text-gray-600 dark:text-gray-400">
                Total Providers
              </p>
              <p class="text-2xl font-bold text-gray-900 dark:text-gray-100">
                {healthData.total_providers}
              </p>
            </div>
            <Activity class="w-8 h-8 text-blue-500" />
          </div>
        </div>
      </div>

      <div class="card">
        <div class="card-body">
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm text-gray-600 dark:text-gray-400">Healthy</p>
              <p class="text-2xl font-bold text-green-600">
                {healthData.healthy_providers}
              </p>
            </div>
            <Check class="w-8 h-8 text-green-500" />
          </div>
        </div>
      </div>

      <div class="card">
        <div class="card-body">
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm text-gray-600 dark:text-gray-400">Unhealthy</p>
              <p class="text-2xl font-bold text-red-600">
                {healthData.unhealthy_providers}
              </p>
            </div>
            <XCircle class="w-8 h-8 text-red-500" />
          </div>
        </div>
      </div>
    </div>
  {/if}

  <!-- Providers Grid -->
  <div class="space-y-4">
    <div class="flex justify-between items-center">
      <h2 class="text-xl font-bold text-gray-900 dark:text-gray-100">
        Provider Health Status
        {#if healthData}
          ({healthData.providers.length})
        {/if}
      </h2>
    </div>

    {#if !healthData}
      <div class="card">
        <div
          class="card-body text-center py-12 text-gray-500 dark:text-gray-400"
        >
          <Activity class="w-12 h-12 mx-auto mb-4 text-gray-400" />
          <p class="mb-2">No health check data available</p>
          <p class="text-sm mb-4">Click "Check Health" to start monitoring</p>
          <button
            onclick={handleCheckHealth}
            disabled={checking}
            class="btn btn-primary"
          >
            {#if checking}
              <Loader2 class="w-5 h-5 animate-spin mr-2" />
              Checking...
            {:else}
              Check Health
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
          <div class="card hover:shadow-lg transition-shadow">
            <div class="card-body">
              <!-- Provider Header -->
              <div class="flex items-start justify-between mb-4">
                <div class="flex items-center space-x-3 flex-1 min-w-0">
                  <div class="shrink-0">
                    {#if getStatusIcon(provider.status) === Check}
                      <Check
                        class="w-5 h-5 {getStatusIconClass(provider.status)}"
                      />
                    {:else if getStatusIcon(provider.status) === XCircle}
                      <XCircle
                        class="w-5 h-5 {getStatusIconClass(provider.status)}"
                      />
                    {:else if getStatusIcon(provider.status) === MinusCircle}
                      <MinusCircle
                        class="w-5 h-5 {getStatusIconClass(provider.status)}"
                      />
                    {:else}
                      <HelpCircle
                        class="w-5 h-5 {getStatusIconClass(provider.status)}"
                      />
                    {/if}
                  </div>
                  <div class="flex-1 min-w-0">
                    <h3
                      class="text-lg font-semibold text-gray-900 dark:text-gray-100 truncate"
                    >
                      {provider.provider_key}
                    </h3>
                    <span class={getStatusBadgeClass(provider.status)}>
                      {provider.status}
                    </span>
                  </div>
                </div>
              </div>

              <!-- Provider Stats -->
              <div class="space-y-2 mb-4">
                <div class="flex items-center justify-between text-sm">
                  <span class="text-gray-600 dark:text-gray-400">
                    Models Tested
                  </span>
                  <span class="font-medium text-gray-900 dark:text-gray-100">
                    {provider.models.length}
                  </span>
                </div>
                {#if provider.avg_response_time_ms !== null}
                  <div class="flex items-center justify-between text-sm">
                    <span class="text-gray-600 dark:text-gray-400">
                      Avg Response
                    </span>
                    <span class="font-medium text-gray-900 dark:text-gray-100">
                      {formatResponseTime(provider.avg_response_time_ms)}
                    </span>
                  </div>
                {/if}
              </div>

              <!-- Check Button -->
              <button
                onclick={e => {
                  e.stopPropagation();
                  handleCheckProviderHealth(provider.provider_id);
                }}
                disabled={checkingProviders.has(provider.provider_id)}
                class="btn btn-sm btn-secondary w-full flex items-center justify-center space-x-2 mb-3"
                title="Check this provider's health"
              >
                <RefreshCw
                  class="w-4 h-4 {checkingProviders.has(provider.provider_id)
                    ? 'animate-spin'
                    : ''}"
                />
                <span>Check</span>
              </button>

              <!-- Last Checked -->
              <div
                class="text-xs text-gray-500 dark:text-gray-400 text-center border-t border-gray-200 dark:border-gray-700 pt-3"
              >
                <div class="flex items-center justify-center space-x-1">
                  <Clock class="w-3 h-3" />
                  <span>{formatTimestamp(provider.checked_at)}</span>
                </div>
              </div>

              <!-- Expand/Collapse Button -->
              <button
                onclick={() => toggleProvider(provider.provider_id)}
                class="btn btn-sm btn-ghost w-full flex items-center justify-center space-x-2 mt-2"
              >
                <span class="text-sm">
                  {expandedProviders.has(provider.provider_id)
                    ? 'Hide Details'
                    : 'Show Details'}
                </span>
                {#if expandedProviders.has(provider.provider_id)}
                  <ChevronUp class="w-4 h-4" />
                {:else}
                  <ChevronDown class="w-4 h-4" />
                {/if}
              </button>

              <!-- Model Details (Expanded) -->
              {#if expandedProviders.has(provider.provider_id)}
                <div
                  class="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700 space-y-2"
                >
                  <h4
                    class="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2"
                  >
                    Model Test Results:
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
                    <div class="space-y-2">
                      {#each provider.models as model, idx (idx)}
                        <div class="p-3 bg-gray-50 dark:bg-gray-900 rounded-lg">
                          <div class="flex items-center justify-between mb-1">
                            <div class="flex items-center space-x-2">
                              <div class="shrink-0">
                                {#if getStatusIcon(model.status) === Check}
                                  <Check
                                    class="w-5 h-5 {getStatusIconClass(
                                      model.status
                                    )}"
                                  />
                                {:else if getStatusIcon(model.status) === XCircle}
                                  <XCircle
                                    class="w-5 h-5 {getStatusIconClass(
                                      model.status
                                    )}"
                                  />
                                {:else if getStatusIcon(model.status) === MinusCircle}
                                  <MinusCircle
                                    class="w-5 h-5 {getStatusIconClass(
                                      model.status
                                    )}"
                                  />
                                {:else}
                                  <HelpCircle
                                    class="w-5 h-5 {getStatusIconClass(
                                      model.status
                                    )}"
                                  />
                                {/if}
                              </div>
                              <p
                                class="font-medium text-sm text-gray-900 dark:text-gray-100 truncate"
                              >
                                {model.model}
                              </p>
                            </div>
                            <span
                              class="{getStatusBadgeClass(
                                model.status
                              )} text-xs"
                            >
                              {model.status}
                            </span>
                          </div>
                          {#if model.response_time_ms !== null}
                            <div
                              class="text-xs text-gray-600 dark:text-gray-400"
                            >
                              {formatResponseTime(model.response_time_ms)}
                            </div>
                          {/if}
                          {#if model.error}
                            <p
                              class="text-xs text-red-600 dark:text-red-400 mt-1"
                            >
                              {model.error}
                            </p>
                          {/if}
                        </div>
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
