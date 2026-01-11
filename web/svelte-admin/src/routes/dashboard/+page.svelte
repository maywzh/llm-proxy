<script lang="ts">
  import {
    LayoutDashboard,
    ExternalLink,
    RefreshCw,
    Maximize2,
    X,
  } from 'lucide-svelte';
  import { PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL } from '$env/static/public';

  let isLoading = $state(true);
  let isFullscreen = $state(false);

  const publicDashboardUrl = PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL || '';

  function handleIframeLoad() {
    isLoading = false;
  }

  function handleRefresh() {
    isLoading = true;
    const iframe = document.getElementById(
      'grafana-iframe'
    ) as HTMLIFrameElement;
    if (iframe) {
      iframe.src = iframe.src;
    }
  }

  function handleOpenExternal() {
    window.open(publicDashboardUrl, '_blank');
  }

  function toggleFullscreen() {
    isFullscreen = !isFullscreen;
  }
</script>

<svelte:head>
  <title>Dashboard - LLM Proxy Admin</title>
</svelte:head>

<div
  class="space-y-4 {isFullscreen
    ? 'fixed inset-0 z-50 bg-white dark:bg-gray-900 p-4'
    : ''}"
>
  <div class="flex items-center justify-between">
    <div class="flex items-center space-x-3">
      <LayoutDashboard class="w-6 h-6 text-primary-600" />
      <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
        Dashboard
      </h1>
    </div>

    {#if publicDashboardUrl}
      <div class="flex items-center space-x-2">
        <button
          onclick={handleRefresh}
          class="btn btn-secondary text-sm flex items-center space-x-2"
          title="Refresh Dashboard"
        >
          <RefreshCw class="w-4 h-4 {isLoading ? 'animate-spin' : ''}" />
          <span class="hidden sm:inline">Refresh</span>
        </button>

        <button
          onclick={toggleFullscreen}
          class="btn btn-secondary text-sm flex items-center space-x-2"
          title={isFullscreen ? 'Exit Fullscreen' : 'Fullscreen'}
        >
          {#if isFullscreen}
            <X class="w-4 h-4" />
          {:else}
            <Maximize2 class="w-4 h-4" />
          {/if}
        </button>

        <button
          onclick={handleOpenExternal}
          class="btn btn-secondary text-sm flex items-center space-x-2"
          title="Open in New Tab"
        >
          <ExternalLink class="w-4 h-4" />
          <span class="hidden sm:inline">Open</span>
        </button>
      </div>
    {/if}
  </div>

  {#if !publicDashboardUrl}
    <div class="card p-8 text-center">
      <LayoutDashboard class="w-16 h-16 text-gray-300 mx-auto mb-4" />
      <h2 class="text-xl font-semibold text-gray-700 dark:text-gray-300 mb-2">
        Grafana Dashboard Not Configured
      </h2>
      <p class="text-gray-500 dark:text-gray-400 mb-4">
        Please set the <code
          class="bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded"
          >PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL</code
        > environment variable.
      </p>
      <a
        href="https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/"
        target="_blank"
        rel="noopener noreferrer"
        class="text-primary-600 hover:text-primary-700 inline-flex items-center"
      >
        Learn how to create a Public Dashboard
        <ExternalLink class="w-4 h-4 ml-1" />
      </a>
    </div>
  {:else}
    <div class="card p-0 overflow-hidden relative">
      {#if isLoading}
        <div
          class="absolute inset-0 flex items-center justify-center bg-gray-50 dark:bg-gray-800 z-10"
        >
          <div class="text-center">
            <div
              class="animate-spin rounded-full h-12 w-12 border-b-2 border-primary-600 mx-auto"
            ></div>
            <p class="mt-4 text-gray-600 dark:text-gray-400">
              Loading dashboard...
            </p>
          </div>
        </div>
      {/if}

      <iframe
        id="grafana-iframe"
        src={publicDashboardUrl}
        title="Grafana Dashboard"
        class="w-full border-0"
        style="height: {isFullscreen
          ? 'calc(100vh - 120px)'
          : 'calc(100vh - 200px)'}; min-height: 600px;"
        onload={handleIframeLoad}
        allow="fullscreen"
      ></iframe>
    </div>
  {/if}
</div>
