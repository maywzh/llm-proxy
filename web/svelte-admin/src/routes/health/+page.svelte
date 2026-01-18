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
      const response = await client.checkProvidersHealth({
        timeout_secs: 30,
        max_concurrent: 2,
      });

      const timestamp = new Date().toISOString();
      healthData = response;
      lastCheckTime = timestamp;

      // Save to cache
      const cache: HealthCheckCache = { timestamp, data: response };
      localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

      // Auto-expand unhealthy providers
      if (response.providers) {
        expandedProviders = new SvelteSet(
          response.providers
            .filter(p => p.status === 'unhealthy')
            .map(p => p.provider_id)
        );
      }
    } catch (err) {
      error =
        err instanceof Error ? err.message : 'Failed to check health status';
    } finally {
      checking = false;
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

  <!-- Providers List -->
  <div class="card">
    <div class="card-header flex justify-between items-center">
      <h2 class="card-title">
        Provider Health Status
        {#if healthData}
          ({healthData.providers.length})
        {/if}
      </h2>
    </div>

    <div class="card-body p-0">
      {#if !healthData}
        <div class="text-center py-12 text-gray-500 dark:text-gray-400">
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
      {:else if healthData.providers.length === 0}
        <div class="text-center py-12 text-gray-500 dark:text-gray-400">
          No providers configured yet.
        </div>
      {:else}
        <div class="divide-y divide-gray-200 dark:divide-gray-700">
          {#each healthData.providers as provider (provider.provider_id)}
            <div
              class="p-4 hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors"
            >
              <!-- Provider Header -->
              <div
                class="flex items-center justify-between cursor-pointer"
                onclick={() => toggleProvider(provider.provider_id)}
                onkeydown={e =>
                  e.key === 'Enter' && toggleProvider(provider.provider_id)}
                role="button"
                tabindex="0"
              >
                <div class="flex items-center space-x-4 flex-1">
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
                    <div class="flex items-center space-x-3">
                      <h3
                        class="text-lg font-semibold text-gray-900 dark:text-gray-100"
                      >
                        {provider.provider_key}
                      </h3>
                      <span class={getStatusBadgeClass(provider.status)}>
                        {provider.status}
                      </span>
                    </div>
                    <div
                      class="flex items-center space-x-4 mt-1 text-sm text-gray-600 dark:text-gray-400"
                    >
                      <span>ID: {provider.provider_id}</span>
                      <span>•</span>
                      <span>
                        {provider.models.length} model{provider.models
                          .length !== 1
                          ? 's'
                          : ''} tested
                      </span>
                      {#if provider.avg_response_time_ms !== null}
                        <span>•</span>
                        <span>
                          Avg: {formatResponseTime(
                            provider.avg_response_time_ms
                          )}
                        </span>
                      {/if}
                    </div>
                  </div>
                </div>
                <div class="flex items-center space-x-4">
                  <div
                    class="text-right text-sm text-gray-500 dark:text-gray-400"
                  >
                    <p>Last checked:</p>
                    <p>{formatTimestamp(provider.checked_at)}</p>
                  </div>
                  <button class="btn-icon">
                    {#if expandedProviders.has(provider.provider_id)}
                      <ChevronUp class="w-5 h-5" />
                    {:else}
                      <ChevronDown class="w-5 h-5" />
                    {/if}
                  </button>
                </div>
              </div>

              <!-- Model Details (Expanded) -->
              {#if expandedProviders.has(provider.provider_id)}
                <div class="mt-4 ml-9 space-y-2">
                  <h4
                    class="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2"
                  >
                    Model Test Results:
                  </h4>
                  <div class="space-y-2">
                    {#each provider.models as model, idx (idx)}
                      <div
                        class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-900 rounded-lg"
                      >
                        <div class="flex items-center space-x-3">
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
                          <div>
                            <p
                              class="font-medium text-gray-900 dark:text-gray-100"
                            >
                              {model.model}
                            </p>
                            {#if model.error}
                              <p
                                class="text-sm text-red-600 dark:text-red-400 mt-1"
                              >
                                {model.error}
                              </p>
                            {/if}
                          </div>
                        </div>
                        <div class="flex items-center space-x-4">
                          <span class={getStatusBadgeClass(model.status)}>
                            {model.status}
                          </span>
                          {#if model.response_time_ms !== null}
                            <span
                              class="text-sm text-gray-600 dark:text-gray-400"
                            >
                              {formatResponseTime(model.response_time_ms)}
                            </span>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
