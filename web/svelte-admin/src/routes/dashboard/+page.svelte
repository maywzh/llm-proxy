<script lang="ts">
  import {
    LayoutDashboard,
    ExternalLink,
    RefreshCw,
    Maximize2,
    X,
  } from 'lucide-svelte';
  import { PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL } from '$env/static/public';
  import DashboardSkeleton from '$lib/components/DashboardSkeleton.svelte';

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
      const currentSrc = iframe.src;
      iframe.src = '';
      // Use setTimeout to ensure the src is cleared before reassigning
      setTimeout(() => {
        iframe.src = currentSrc;
      }, 0);
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
  <title>Dashboard - HEN Admin</title>
</svelte:head>

{#if !publicDashboardUrl}
  <div class="space-y-6">
    <div class="flex items-center space-x-3">
      <LayoutDashboard class="w-6 h-6 text-primary-600" />
      <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
        Dashboard
      </h1>
    </div>

    <div class="card p-8">
      <div class="text-center mb-6">
        <LayoutDashboard class="w-16 h-16 text-gray-300 mx-auto mb-4" />
        <h2 class="text-xl font-semibold text-gray-700 dark:text-gray-300 mb-2">
          Grafana Dashboard Not Configured
        </h2>
        <p class="text-gray-500 dark:text-gray-400">
          Follow the steps below to configure your Grafana public dashboard.
        </p>
      </div>

      <div class="bg-gray-50 dark:bg-gray-800 rounded-lg p-6 mb-6">
        <h3
          class="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-4 uppercase tracking-wide"
        >
          Setup Guide
        </h3>
        <ol class="space-y-3 text-gray-600 dark:text-gray-400">
          <li class="flex items-start">
            <span
              class="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3"
            >
              1
            </span>
            <span>Create or open a dashboard in Grafana</span>
          </li>
          <li class="flex items-start">
            <span
              class="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3"
            >
              2
            </span>
            <span>Click the share button and select "Public Dashboard"</span>
          </li>
          <li class="flex items-start">
            <span
              class="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3"
            >
              3
            </span>
            <span>Enable public access and copy the generated URL</span>
          </li>
          <li class="flex items-start">
            <span
              class="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3"
            >
              4
            </span>
            <span>
              Set the
              <code
                class="bg-gray-200 dark:bg-gray-700 px-2 py-0.5 rounded text-sm"
              >
                PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL
              </code>
              environment variable
            </span>
          </li>
          <li class="flex items-start">
            <span
              class="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3"
            >
              5
            </span>
            <span>Restart the application to apply the changes</span>
          </li>
        </ol>
      </div>

      <div class="text-center">
        <a
          href="https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/"
          target="_blank"
          rel="noopener noreferrer"
          class="text-primary-600 hover:text-primary-700 inline-flex items-center"
        >
          Learn more about Public Dashboards
          <ExternalLink class="w-4 h-4 ml-1" />
        </a>
      </div>
    </div>
  </div>
{:else}
  <div
    class="space-y-4 transition-all duration-300 {isFullscreen
      ? 'fixed inset-0 z-50 bg-white dark:bg-gray-900 p-4 animate-fade-in'
      : ''}"
  >
    <div class="flex items-center justify-between">
      <div class="flex items-center space-x-3">
        <LayoutDashboard class="w-6 h-6 text-primary-600" />
        <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
          Dashboard
        </h1>
      </div>

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
    </div>

    <div class="card p-0 overflow-hidden relative">
      {#if isLoading}
        <div
          class="absolute inset-0 flex items-center justify-center bg-gray-50 dark:bg-gray-800 z-10 animate-fade-in"
        >
          <div class="w-full max-w-3xl px-8">
            <DashboardSkeleton />
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
  </div>
{/if}
