<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { auth } from '$lib/stores';
  import { debounce } from '$lib/debounce';
  import TableSkeleton from '$lib/components/TableSkeleton.svelte';
  import {
    Search,
    ChevronLeft,
    ChevronRight,
    Download,
    X,
    AlertCircle,
    Clock,
    Zap,
    Hash,
    Filter,
    Trash2,
    RefreshCw,
  } from 'lucide-svelte';
  import type {
    RequestLog,
    RequestLogDetail,
    RequestLogFilters,
    RequestLogStats,
    ErrorLog,
    ErrorLogDetail,
    ErrorLogFilters,
  } from '$lib/types';

  const PAGE_SIZE = 50;

  function formatDuration(ms: number | null): string {
    if (ms === null || ms === undefined) return '-';
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  }

  function formatTimestamp(iso: string): string {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  }

  function formatNumber(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return n.toString();
  }

  function statusBadge(code: number | null): string {
    if (code === null) return 'badge';
    if (code >= 200 && code < 300) return 'badge badge-success';
    if (code >= 400 && code < 500) return 'badge badge-warning';
    return 'badge badge-danger';
  }

  function categoryBadge(cat: string): string {
    if (cat.includes('timeout')) return 'badge badge-warning';
    if (cat.includes('auth')) return 'badge badge-danger';
    return 'badge badge-danger';
  }

  function tryFormatJson(str: string | null): string {
    if (!str) return '';
    try {
      return JSON.stringify(JSON.parse(str), null, 2);
    } catch {
      return str;
    }
  }

  function sanitizeHeaders(headersJson: string | null): string {
    if (!headersJson) return '';
    try {
      const headers = JSON.parse(headersJson);
      const sensitiveKeys = [
        'authorization',
        'cookie',
        'x-api-key',
        'x-admin-key',
        'proxy-authorization',
      ];
      const sanitized = { ...headers };
      for (const key of Object.keys(sanitized)) {
        if (sensitiveKeys.includes(key.toLowerCase())) {
          sanitized[key] = '[REDACTED]';
        }
      }
      return JSON.stringify(sanitized, null, 2);
    } catch {
      return headersJson;
    }
  }

  function extractFromHeaders(
    headersJson: string | null | undefined,
    key: string
  ): string | null {
    if (!headersJson) return null;
    try {
      const h = JSON.parse(headersJson);
      return h[key] || h[key.toLowerCase()] || null;
    } catch {
      return null;
    }
  }

  function extractClientIp(
    headersJson: string | null | undefined
  ): string | null {
    const forwarded = extractFromHeaders(headersJson, 'x-forwarded-for');
    if (forwarded) return forwarded.split(',')[0].trim();
    return extractFromHeaders(headersJson, 'x-real-ip');
  }

  type TabKey = 'requests' | 'errors';
  let activeTab = $state<TabKey>('requests');

  // =============================================================================
  // Request Logs
  // =============================================================================
  let reqLogs = $state<RequestLog[]>([]);
  let reqStats = $state<RequestLogStats | null>(null);
  let reqLoading = $state(false);
  let reqError = $state<string | null>(null);
  let reqPage = $state(1);
  let reqTotalPages = $state(1);
  let reqTotal = $state(0);

  let reqSearchTerm = $state('');
  let reqDebouncedSearch = $state('');
  let reqProviderFilter = $state('');
  let reqModelFilter = $state('');
  let reqStatusFilter = $state('');
  let reqStreamingFilter = $state('');
  let reqErrorOnly = $state(false);
  let reqShowFilters = $state(false);

  let reqSelectedLog = $state<RequestLogDetail | null>(null);
  let reqDetailLoading = $state(false);
  let reqSelectedIds = new SvelteSet<number>();
  let reqDeleting = $state(false);
  let reqShowDeleteConfirm = $state(false);

  const debouncedSetReqSearch = debounce((val: string) => {
    reqDebouncedSearch = val;
  }, 300);
  $effect(() => {
    debouncedSetReqSearch(reqSearchTerm);
  });

  // Reset page on filter change
  $effect(() => {
    void reqDebouncedSearch;
    void reqProviderFilter;
    void reqModelFilter;
    void reqStatusFilter;
    void reqStreamingFilter;
    void reqErrorOnly;
    untrack(() => {
      reqPage = 1;
    });
  });

  function buildReqFilters(): RequestLogFilters {
    const f: RequestLogFilters = {};
    if (reqDebouncedSearch) f.request_id = reqDebouncedSearch;
    if (reqProviderFilter) f.provider_name = reqProviderFilter;
    if (reqModelFilter) f.model = reqModelFilter;
    if (reqStatusFilter) f.status_code = Number(reqStatusFilter);
    if (reqStreamingFilter === 'true') f.is_streaming = true;
    if (reqStreamingFilter === 'false') f.is_streaming = false;
    if (reqErrorOnly) f.error_only = true;
    f.sort_by = 'timestamp';
    f.sort_order = 'desc';
    return f;
  }

  async function loadReqLogs() {
    const client = auth.apiClient;
    if (!client) return;
    reqLoading = true;
    reqError = null;
    try {
      const res = await client.listLogs(reqPage, PAGE_SIZE, buildReqFilters());
      reqLogs = res.items;
      reqTotal = res.total;
      reqTotalPages = res.total_pages;
    } catch (err) {
      reqError = err instanceof Error ? err.message : 'Failed to load logs';
    } finally {
      reqLoading = false;
    }
  }

  async function loadReqStats() {
    const client = auth.apiClient;
    if (!client) return;
    try {
      reqStats = await client.getLogStats();
    } catch {
      /* non-critical */
    }
  }

  $effect(() => {
    if (activeTab !== 'requests') return;
    void reqPage;
    void reqDebouncedSearch;
    void reqProviderFilter;
    void reqModelFilter;
    void reqStatusFilter;
    void reqStreamingFilter;
    void reqErrorOnly;
    loadReqLogs();
  });

  async function handleReqRowClick(log: RequestLog) {
    const client = auth.apiClient;
    if (!client) return;
    reqDetailLoading = true;
    try {
      reqSelectedLog = await client.getLog(log.id);
    } catch {
      reqSelectedLog = null;
    } finally {
      reqDetailLoading = false;
    }
  }

  function handleReqExport() {
    const headers = [
      'timestamp',
      'request_id',
      'model_requested',
      'provider_name',
      'status_code',
      'input_tokens',
      'output_tokens',
      'total_duration_ms',
      'ttft_ms',
      'is_streaming',
      'error_category',
    ];
    const csv = [
      headers.join(','),
      ...reqLogs.map(log =>
        headers
          .map(h =>
            JSON.stringify((log as unknown as Record<string, unknown>)[h] ?? '')
          )
          .join(',')
      ),
    ].join('\n');
    const blob = new Blob([csv], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `request-logs-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function clearReqFilters() {
    reqSearchTerm = '';
    reqProviderFilter = '';
    reqModelFilter = '';
    reqStatusFilter = '';
    reqStreamingFilter = '';
    reqErrorOnly = false;
  }

  function toggleReqSelect(id: number) {
    if (reqSelectedIds.has(id)) reqSelectedIds.delete(id);
    else reqSelectedIds.add(id);
  }
  function toggleReqSelectAll() {
    if (reqSelectedIds.size === reqLogs.length) {
      reqSelectedIds.clear();
    } else {
      reqSelectedIds.clear();
      reqLogs.forEach(l => reqSelectedIds.add(l.id));
    }
  }

  async function handleReqBatchDelete() {
    const client = auth.apiClient;
    if (!client || reqSelectedIds.size === 0) return;
    reqDeleting = true;
    try {
      await client.batchDeleteLogs(Array.from(reqSelectedIds));
      reqSelectedIds.clear();
      reqShowDeleteConfirm = false;
      await loadReqLogs();
      await loadReqStats();
    } catch (err) {
      reqError = err instanceof Error ? err.message : 'Failed to delete logs';
    } finally {
      reqDeleting = false;
    }
  }

  const reqHasActiveFilters = $derived(
    reqSearchTerm ||
      reqProviderFilter ||
      reqModelFilter ||
      reqStatusFilter ||
      reqStreamingFilter ||
      reqErrorOnly
  );

  // =============================================================================
  // Error Logs
  // =============================================================================
  let errLogs = $state<ErrorLog[]>([]);
  let errLoading = $state(false);
  let errError = $state<string | null>(null);
  let errPage = $state(1);
  let errTotalPages = $state(1);
  let errTotal = $state(0);

  let errSearchTerm = $state('');
  let errDebouncedSearch = $state('');
  let errProviderFilter = $state('');
  let errCategoryFilter = $state('');
  let errShowFilters = $state(false);

  let errSelectedLog = $state<ErrorLogDetail | null>(null);
  let errDetailLoading = $state(false);
  let errSelectedIds = new SvelteSet<number>();
  let errDeleting = $state(false);
  let errShowDeleteConfirm = $state(false);

  const debouncedSetErrSearch = debounce((val: string) => {
    errDebouncedSearch = val;
  }, 300);
  $effect(() => {
    debouncedSetErrSearch(errSearchTerm);
  });

  $effect(() => {
    void errDebouncedSearch;
    void errProviderFilter;
    void errCategoryFilter;
    untrack(() => {
      errPage = 1;
    });
  });

  function buildErrFilters(): ErrorLogFilters {
    const f: ErrorLogFilters = {};
    if (errDebouncedSearch) f.request_id = errDebouncedSearch;
    if (errProviderFilter) f.provider_name = errProviderFilter;
    if (errCategoryFilter) f.error_category = errCategoryFilter;
    f.sort_by = 'timestamp';
    f.sort_order = 'desc';
    return f;
  }

  async function loadErrLogs() {
    const client = auth.apiClient;
    if (!client) return;
    errLoading = true;
    errError = null;
    try {
      const res = await client.listErrorLogs(
        errPage,
        PAGE_SIZE,
        buildErrFilters()
      );
      errLogs = res.items;
      errTotal = res.total;
      errTotalPages = res.total_pages;
    } catch (err) {
      errError =
        err instanceof Error ? err.message : 'Failed to load error logs';
    } finally {
      errLoading = false;
    }
  }

  $effect(() => {
    if (activeTab !== 'errors') return;
    void errPage;
    void errDebouncedSearch;
    void errProviderFilter;
    void errCategoryFilter;
    loadErrLogs();
  });

  async function handleErrRowClick(log: ErrorLog) {
    const client = auth.apiClient;
    if (!client) return;
    errDetailLoading = true;
    try {
      errSelectedLog = await client.getErrorLog(log.id);
    } catch {
      errSelectedLog = null;
    } finally {
      errDetailLoading = false;
    }
  }

  function clearErrFilters() {
    errSearchTerm = '';
    errProviderFilter = '';
    errCategoryFilter = '';
  }

  function toggleErrSelect(id: number) {
    if (errSelectedIds.has(id)) errSelectedIds.delete(id);
    else errSelectedIds.add(id);
  }
  function toggleErrSelectAll() {
    if (errSelectedIds.size === errLogs.length) {
      errSelectedIds.clear();
    } else {
      errSelectedIds.clear();
      errLogs.forEach(l => errSelectedIds.add(l.id));
    }
  }

  async function handleErrBatchDelete() {
    const client = auth.apiClient;
    if (!client || errSelectedIds.size === 0) return;
    errDeleting = true;
    try {
      await client.batchDeleteErrorLogs(Array.from(errSelectedIds));
      errSelectedIds.clear();
      errShowDeleteConfirm = false;
      await loadErrLogs();
    } catch (err) {
      errError =
        err instanceof Error ? err.message : 'Failed to delete error logs';
    } finally {
      errDeleting = false;
    }
  }

  const errHasActiveFilters = $derived(
    errSearchTerm || errProviderFilter || errCategoryFilter
  );

  onMount(() => {
    loadReqStats();
  });

  onDestroy(() => {
    debouncedSetReqSearch.cancel();
    debouncedSetErrSearch.cancel();
  });
</script>

<svelte:head>
  <title>Logs - HEN Admin</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header + Tabs -->
  <div>
    <h1 class="text-2xl font-semibold text-gray-900 dark:text-white">Logs</h1>
    <div class="mt-4 border-b border-gray-200 dark:border-gray-700">
      <nav class="flex space-x-8">
        <button
          onclick={() => {
            activeTab = 'requests';
          }}
          class="pb-3 text-sm font-medium border-b-2 transition-colors {activeTab ===
          'requests'
            ? 'border-blue-500 text-blue-600 dark:text-blue-400'
            : 'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300'}"
          >Request Logs</button
        >
        <button
          onclick={() => {
            activeTab = 'errors';
          }}
          class="pb-3 text-sm font-medium border-b-2 transition-colors {activeTab ===
          'errors'
            ? 'border-red-500 text-red-600 dark:text-red-400'
            : 'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300'}"
          >Error Logs</button
        >
      </nav>
    </div>
  </div>

  <!-- ================================================================= -->
  <!-- REQUEST LOGS TAB                                                   -->
  <!-- ================================================================= -->
  {#if activeTab === 'requests'}
    {#if reqStats}
      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <div class="card">
          <div class="card-body flex items-center space-x-3">
            <div class="p-2 bg-blue-100 dark:bg-blue-900/30 rounded-lg">
              <Hash class="w-5 h-5 text-blue-600 dark:text-blue-400" />
            </div>
            <div>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                Total Requests
              </p>
              <p class="text-xl font-semibold text-gray-900 dark:text-white">
                {formatNumber(reqStats.total_requests)}
              </p>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="card-body flex items-center space-x-3">
            <div class="p-2 bg-red-100 dark:bg-red-900/30 rounded-lg">
              <AlertCircle class="w-5 h-5 text-red-600 dark:text-red-400" />
            </div>
            <div>
              <p class="text-sm text-gray-500 dark:text-gray-400">Error Rate</p>
              <p class="text-xl font-semibold text-gray-900 dark:text-white">
                {(reqStats.error_rate * 100).toFixed(1)}%
              </p>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="card-body flex items-center space-x-3">
            <div class="p-2 bg-amber-100 dark:bg-amber-900/30 rounded-lg">
              <Clock class="w-5 h-5 text-amber-600 dark:text-amber-400" />
            </div>
            <div>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                Avg Duration
              </p>
              <p class="text-xl font-semibold text-gray-900 dark:text-white">
                {formatDuration(reqStats.avg_duration_ms)}
              </p>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="card-body flex items-center space-x-3">
            <div class="p-2 bg-emerald-100 dark:bg-emerald-900/30 rounded-lg">
              <Zap class="w-5 h-5 text-emerald-600 dark:text-emerald-400" />
            </div>
            <div>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                Total Tokens
              </p>
              <p class="text-xl font-semibold text-gray-900 dark:text-white">
                {formatNumber(
                  reqStats.total_input_tokens + reqStats.total_output_tokens
                )}
              </p>
            </div>
          </div>
        </div>
      </div>
    {/if}

    <div class="card">
      <div class="card-body space-y-3">
        <div class="flex items-center space-x-2">
          <div class="relative flex-1">
            <Search
              class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400"
            />
            <input
              type="text"
              placeholder="Search by full request ID..."
              bind:value={reqSearchTerm}
              class="input pl-10 w-full"
            />
          </div>
          <button
            onclick={() => {
              reqShowFilters = !reqShowFilters;
            }}
            class="btn {reqShowFilters
              ? 'btn-primary'
              : 'btn-secondary'} text-sm flex items-center"
          >
            <Filter class="w-4 h-4 mr-1" /> Filters
          </button>
          {#if reqHasActiveFilters}
            <button
              onclick={clearReqFilters}
              class="btn btn-secondary text-sm flex items-center"
              ><X class="w-4 h-4 mr-1" /> Clear</button
            >
          {/if}
          <button
            onclick={handleReqExport}
            class="btn btn-secondary text-sm flex items-center"
            disabled={reqLogs.length === 0}
            title="Export current page to CSV"
          >
            <Download class="w-4 h-4 mr-1" /> CSV
          </button>
          <button
            onclick={() => {
              loadReqLogs();
              loadReqStats();
            }}
            class="btn btn-secondary text-sm flex items-center"
          >
            <RefreshCw class="w-4 h-4 mr-1" /> Refresh
          </button>
          {#if reqSelectedIds.size > 0}
            <button
              onclick={() => {
                reqShowDeleteConfirm = true;
              }}
              class="btn btn-danger text-sm flex items-center"
            >
              <Trash2 class="w-4 h-4 mr-1" /> Delete ({reqSelectedIds.size})
            </button>
          {/if}
        </div>
        {#if reqShowFilters}
          <div
            class="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2 border-t border-gray-200 dark:border-gray-700"
          >
            <div>
              <label class="label text-xs">Provider</label><input
                type="text"
                placeholder="Provider name"
                bind:value={reqProviderFilter}
                class="input text-sm"
              />
            </div>
            <div>
              <label class="label text-xs">Model</label><input
                type="text"
                placeholder="Model name"
                bind:value={reqModelFilter}
                class="input text-sm"
              />
            </div>
            <div>
              <label class="label text-xs">Status Code</label>
              <select bind:value={reqStatusFilter} class="input text-sm">
                <option value="">All</option>
                <option value="200">200 OK</option>
                <option value="400">400 Bad Request</option>
                <option value="401">401 Unauthorized</option>
                <option value="429">429 Rate Limited</option>
                <option value="500">500 Server Error</option>
                <option value="502">502 Bad Gateway</option>
                <option value="504">504 Timeout</option>
              </select>
            </div>
            <div>
              <label class="label text-xs">Streaming</label>
              <select bind:value={reqStreamingFilter} class="input text-sm">
                <option value="">All</option>
                <option value="true">Streaming</option>
                <option value="false">Non-streaming</option>
              </select>
            </div>
            <div class="col-span-2 sm:col-span-4 flex items-center">
              <label class="flex items-center space-x-2 cursor-pointer">
                <input
                  type="checkbox"
                  bind:checked={reqErrorOnly}
                  class="rounded border-gray-300 text-blue-600"
                />
                <span class="text-sm text-gray-700 dark:text-gray-300"
                  >Errors only</span
                >
              </label>
            </div>
          </div>
        {/if}
      </div>
    </div>

    {#if reqError}
      <div class="alert-error flex items-center space-x-2">
        <AlertCircle class="w-5 h-5" /><span>{reqError}</span>
      </div>
    {/if}

    <div class="table-container">
      {#if reqLoading}
        <TableSkeleton rows={10} columns={12} />
      {:else}
        <table class="table">
          <thead
            ><tr>
              <th class="w-8"
                ><input
                  type="checkbox"
                  checked={reqLogs.length > 0 &&
                    reqSelectedIds.size === reqLogs.length}
                  onchange={toggleReqSelectAll}
                  class="rounded border-gray-300 text-blue-600"
                /></th
              >
              <th>Time</th><th>Request ID</th><th>Model</th><th>Provider</th><th
                >Credential</th
              ><th>Client</th><th>Status</th><th>Stream</th><th>Tokens</th><th
                >Duration</th
              ><th>TTFT</th>
            </tr></thead
          >
          <tbody>
            {#if reqLogs.length === 0}
              <tr
                ><td
                  colspan="12"
                  class="text-center py-8 text-gray-500 dark:text-gray-400"
                  >No logs found</td
                ></tr
              >
            {:else}
              {#each reqLogs as log (log.id)}
                <tr
                  onclick={() => handleReqRowClick(log)}
                  class="cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800"
                >
                  <td class="w-8" onclick={e => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      checked={reqSelectedIds.has(log.id)}
                      onchange={() => toggleReqSelect(log.id)}
                      class="rounded border-gray-300 text-blue-600"
                    />
                  </td>
                  <td
                    class="whitespace-nowrap text-sm text-gray-500 dark:text-gray-400"
                    >{formatTimestamp(log.timestamp)}</td
                  >
                  <td
                    class="font-mono text-xs max-w-[120px] truncate"
                    title={log.request_id}>{log.request_id.slice(0, 12)}...</td
                  >
                  <td
                    class="text-sm max-w-[150px] truncate"
                    title={log.model_requested ?? ''}
                    >{log.model_requested || '-'}</td
                  >
                  <td class="text-sm">{log.provider_name || '-'}</td>
                  <td class="text-sm">{log.credential_name || '-'}</td>
                  <td class="text-sm">{log.client || '-'}</td>
                  <td
                    ><span class={statusBadge(log.status_code)}
                      >{log.status_code ?? '-'}</span
                    ></td
                  >
                  <td class="text-sm text-gray-500"
                    >{log.is_streaming ? 'true' : 'false'}</td
                  >
                  <td
                    class="text-sm tabular-nums"
                    title="In: {log.input_tokens} / Out: {log.output_tokens}"
                    >{formatNumber(log.input_tokens)}/{formatNumber(
                      log.output_tokens
                    )}</td
                  >
                  <td class="text-sm tabular-nums"
                    >{formatDuration(log.total_duration_ms)}</td
                  >
                  <td class="text-sm tabular-nums"
                    >{log.ttft_ms !== null
                      ? formatDuration(log.ttft_ms)
                      : '-'}</td
                  >
                </tr>
              {/each}
            {/if}
          </tbody>
        </table>
      {/if}
    </div>

    {#if reqTotal > 0}
      <div class="flex items-center justify-between px-1">
        <span class="text-sm text-gray-500 dark:text-gray-400"
          >{((reqPage - 1) * PAGE_SIZE + 1).toLocaleString()} – {Math.min(
            reqPage * PAGE_SIZE,
            reqTotal
          ).toLocaleString()} of {reqTotal.toLocaleString()}</span
        >
        <div class="flex items-center space-x-2">
          <button
            onclick={() => {
              reqPage = Math.max(1, reqPage - 1);
            }}
            disabled={reqPage === 1}
            class="btn btn-secondary text-sm"
            ><ChevronLeft class="w-4 h-4" /></button
          >
          <span class="text-sm text-gray-700 dark:text-gray-300"
            >Page {reqPage} of {reqTotalPages}</span
          >
          <button
            onclick={() => {
              reqPage = Math.min(reqTotalPages, reqPage + 1);
            }}
            disabled={reqPage === reqTotalPages}
            class="btn btn-secondary text-sm"
            ><ChevronRight class="w-4 h-4" /></button
          >
        </div>
      </div>
    {/if}

    <!-- Request Detail Modal -->
    {#if reqSelectedLog || reqDetailLoading}
      <div
        class="modal-overlay"
        onclick={() => {
          if (!reqDetailLoading) reqSelectedLog = null;
        }}
        role="presentation"
        tabindex="-1"
        onkeydown={e => {
          if (e.key === 'Escape') reqSelectedLog = null;
        }}
      >
        <div
          class="modal max-w-3xl w-full max-h-[90vh] overflow-y-auto animate-modal-enter"
          onclick={e => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label="Request Details"
          tabindex="-1"
          onkeydown={e => e.stopPropagation()}
        >
          <div class="modal-header">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
              Request Details
            </h3>
            <button
              onclick={() => {
                reqSelectedLog = null;
              }}
              class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
              ><X class="w-5 h-5" /></button
            >
          </div>
          {#if reqDetailLoading}
            <div class="p-6 text-center text-gray-500">Loading...</div>
          {:else if reqSelectedLog}
            <div class="modal-body space-y-4">
              <div class="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >Request ID</span
                  >
                  <p class="font-mono text-xs break-all">
                    {reqSelectedLog.request_id}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Timestamp</span
                  >
                  <p>{new Date(reqSelectedLog.timestamp).toLocaleString()}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Model</span>
                  <p>
                    {reqSelectedLog.model_requested}{#if reqSelectedLog.model_mapped && reqSelectedLog.model_mapped !== reqSelectedLog.model_requested}<span
                        class="text-gray-400"
                      >
                        &rarr; {reqSelectedLog.model_mapped}</span
                      >{/if}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Provider</span>
                  <p>
                    {reqSelectedLog.provider_name}
                    {#if reqSelectedLog.provider_type}<span
                        class="badge badge-info text-xs"
                        >{reqSelectedLog.provider_type}</span
                      >{/if}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >Credential</span
                  >
                  <p>{reqSelectedLog.credential_name || '-'}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Client</span>
                  <p>{reqSelectedLog.client || '-'}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Client IP</span
                  >
                  <p class="font-mono text-xs">
                    {extractClientIp(reqSelectedLog.request_headers) || '-'}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >User-Agent</span
                  >
                  <p class="text-xs break-all">
                    {extractFromHeaders(
                      reqSelectedLog.request_headers,
                      'user-agent'
                    ) || '-'}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Status</span>
                  <p>
                    <span class={statusBadge(reqSelectedLog.status_code)}
                      >{reqSelectedLog.status_code}</span
                    >{#if reqSelectedLog.error_category}<span
                        class="ml-2 text-red-600 dark:text-red-400 text-xs"
                        >{reqSelectedLog.error_category}</span
                      >{/if}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Protocol</span>
                  <p>
                    {reqSelectedLog.client_protocol || '?'} &rarr; {reqSelectedLog.provider_protocol ||
                      '?'}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Streaming</span
                  >
                  <p>{reqSelectedLog.is_streaming ? 'Yes' : 'No'}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Tokens</span>
                  <p>
                    In: {reqSelectedLog.input_tokens.toLocaleString()} / Out: {reqSelectedLog.output_tokens.toLocaleString()}
                    / Total: {reqSelectedLog.total_tokens.toLocaleString()}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >Duration / TTFT</span
                  >
                  <p>
                    {formatDuration(
                      reqSelectedLog.total_duration_ms
                    )}{#if reqSelectedLog.ttft_ms !== null}<span
                        class="text-gray-400"
                      >
                        (TTFT: {formatDuration(reqSelectedLog.ttft_ms)})</span
                      >{/if}
                  </p>
                </div>
              </div>
              {#if reqSelectedLog.error_message}
                <div>
                  <h4
                    class="text-sm font-medium text-red-600 dark:text-red-400 mb-1"
                  >
                    Error Message
                  </h4>
                  <pre
                    class="bg-red-50 dark:bg-red-900/20 p-3 rounded text-sm overflow-x-auto text-red-800 dark:text-red-300">{reqSelectedLog.error_message}</pre>
                </div>
              {/if}
              {#if reqSelectedLog.request_headers}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Request Headers
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-48 overflow-y-auto">{sanitizeHeaders(
                      reqSelectedLog.request_headers
                    )}</pre>
                </div>
              {/if}
              {#if reqSelectedLog.request_body}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Request Body
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">{tryFormatJson(
                      reqSelectedLog.request_body
                    )}</pre>
                </div>
              {/if}
              {#if reqSelectedLog.response_body}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Response Body
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">{tryFormatJson(
                      reqSelectedLog.response_body
                    )}</pre>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}

    {#if reqShowDeleteConfirm}
      <div
        class="modal-overlay"
        onclick={() => {
          reqShowDeleteConfirm = false;
        }}
        role="presentation"
        tabindex="-1"
        onkeydown={e => {
          if (e.key === 'Escape') reqShowDeleteConfirm = false;
        }}
      >
        <div
          class="modal"
          onclick={e => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label="Delete Request Logs"
          tabindex="-1"
          onkeydown={e => e.stopPropagation()}
        >
          <div class="modal-header">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Delete Request Logs
            </h3>
            <button
              onclick={() => {
                reqShowDeleteConfirm = false;
              }}
              class="btn-icon"><X class="w-5 h-5" /></button
            >
          </div>
          <div class="modal-body">
            <p class="text-sm text-gray-600 dark:text-gray-400">
              Are you sure you want to delete <strong
                >{reqSelectedIds.size}</strong
              > selected log(s)? This action cannot be undone.
            </p>
          </div>
          <div class="modal-footer">
            <button
              onclick={() => {
                reqShowDeleteConfirm = false;
              }}
              class="btn btn-secondary">Cancel</button
            >
            <button
              onclick={handleReqBatchDelete}
              class="btn btn-danger flex items-center space-x-2"
              disabled={reqDeleting}
            >
              {#if reqDeleting}<span
                  class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"
                ></span>{/if}
              <span>Delete</span>
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- ================================================================= -->
    <!-- ERROR LOGS TAB                                                     -->
    <!-- ================================================================= -->
  {:else}
    <div class="card">
      <div class="card-body space-y-3">
        <div class="flex items-center space-x-2">
          <div class="relative flex-1">
            <Search
              class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400"
            />
            <input
              type="text"
              placeholder="Search by full request ID..."
              bind:value={errSearchTerm}
              class="input pl-10 w-full"
            />
          </div>
          <button
            onclick={() => {
              errShowFilters = !errShowFilters;
            }}
            class="btn {errShowFilters
              ? 'btn-primary'
              : 'btn-secondary'} text-sm flex items-center"
          >
            <Filter class="w-4 h-4 mr-1" /> Filters
          </button>
          {#if errHasActiveFilters}
            <button
              onclick={clearErrFilters}
              class="btn btn-secondary text-sm flex items-center"
              ><X class="w-4 h-4 mr-1" /> Clear</button
            >
          {/if}
          <button
            onclick={loadErrLogs}
            class="btn btn-secondary text-sm flex items-center"
          >
            <RefreshCw class="w-4 h-4 mr-1" /> Refresh
          </button>
          {#if errSelectedIds.size > 0}
            <button
              onclick={() => {
                errShowDeleteConfirm = true;
              }}
              class="btn btn-danger text-sm flex items-center"
            >
              <Trash2 class="w-4 h-4 mr-1" /> Delete ({errSelectedIds.size})
            </button>
          {/if}
        </div>
        {#if errShowFilters}
          <div
            class="grid grid-cols-2 sm:grid-cols-3 gap-3 pt-2 border-t border-gray-200 dark:border-gray-700"
          >
            <div>
              <label class="label text-xs">Provider</label><input
                type="text"
                placeholder="Provider name"
                bind:value={errProviderFilter}
                class="input text-sm"
              />
            </div>
            <div>
              <label class="label text-xs">Error Category</label><input
                type="text"
                placeholder="e.g. timeout, auth_error"
                bind:value={errCategoryFilter}
                class="input text-sm"
              />
            </div>
          </div>
        {/if}
      </div>
    </div>

    {#if errError}
      <div class="alert-error flex items-center space-x-2">
        <AlertCircle class="w-5 h-5" /><span>{errError}</span>
      </div>
    {/if}

    <div class="table-container">
      {#if errLoading}
        <TableSkeleton rows={10} columns={9} />
      {:else}
        <table class="table">
          <thead
            ><tr>
              <th class="w-8"
                ><input
                  type="checkbox"
                  checked={errLogs.length > 0 &&
                    errSelectedIds.size === errLogs.length}
                  onchange={toggleErrSelectAll}
                  class="rounded border-gray-300 text-blue-600"
                /></th
              >
              <th>Time</th><th>Request ID</th><th>Category</th><th>Provider</th
              ><th>Credential</th><th>Model</th><th>Message</th><th>Duration</th
              >
            </tr></thead
          >
          <tbody>
            {#if errLogs.length === 0}
              <tr
                ><td
                  colspan="9"
                  class="text-center py-8 text-gray-500 dark:text-gray-400"
                  >No error logs found</td
                ></tr
              >
            {:else}
              {#each errLogs as log (log.id)}
                <tr
                  onclick={() => handleErrRowClick(log)}
                  class="cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800"
                >
                  <td class="w-8" onclick={e => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      checked={errSelectedIds.has(log.id)}
                      onchange={() => toggleErrSelect(log.id)}
                      class="rounded border-gray-300 text-blue-600"
                    />
                  </td>
                  <td
                    class="whitespace-nowrap text-sm text-gray-500 dark:text-gray-400"
                    >{formatTimestamp(log.timestamp)}</td
                  >
                  <td
                    class="font-mono text-xs max-w-[120px] truncate"
                    title={log.request_id ?? ''}
                    >{log.request_id
                      ? `${log.request_id.slice(0, 12)}...`
                      : '-'}</td
                  >
                  <td
                    ><span class={categoryBadge(log.error_category)}
                      >{log.error_category}</span
                    ></td
                  >
                  <td class="text-sm">{log.provider_name || '-'}</td>
                  <td class="text-sm">{log.credential_name || '-'}</td>
                  <td
                    class="text-sm max-w-[150px] truncate"
                    title={log.model_requested ?? ''}
                    >{log.model_requested || '-'}</td
                  >
                  <td
                    class="text-sm max-w-[250px] truncate text-red-600 dark:text-red-400"
                    title={log.error_message ?? ''}
                    >{log.error_message || '-'}</td
                  >
                  <td class="text-sm tabular-nums"
                    >{formatDuration(log.total_duration_ms)}</td
                  >
                </tr>
              {/each}
            {/if}
          </tbody>
        </table>
      {/if}
    </div>

    {#if errTotal > 0}
      <div class="flex items-center justify-between px-1">
        <span class="text-sm text-gray-500 dark:text-gray-400"
          >{((errPage - 1) * PAGE_SIZE + 1).toLocaleString()} – {Math.min(
            errPage * PAGE_SIZE,
            errTotal
          ).toLocaleString()} of {errTotal.toLocaleString()}</span
        >
        <div class="flex items-center space-x-2">
          <button
            onclick={() => {
              errPage = Math.max(1, errPage - 1);
            }}
            disabled={errPage === 1}
            class="btn btn-secondary text-sm"
            ><ChevronLeft class="w-4 h-4" /></button
          >
          <span class="text-sm text-gray-700 dark:text-gray-300"
            >Page {errPage} of {errTotalPages}</span
          >
          <button
            onclick={() => {
              errPage = Math.min(errTotalPages, errPage + 1);
            }}
            disabled={errPage === errTotalPages}
            class="btn btn-secondary text-sm"
            ><ChevronRight class="w-4 h-4" /></button
          >
        </div>
      </div>
    {/if}

    <!-- Error Detail Modal -->
    {#if errSelectedLog || errDetailLoading}
      <div
        class="modal-overlay"
        onclick={() => {
          if (!errDetailLoading) errSelectedLog = null;
        }}
        role="presentation"
        tabindex="-1"
        onkeydown={e => {
          if (e.key === 'Escape') errSelectedLog = null;
        }}
      >
        <div
          class="modal max-w-3xl w-full max-h-[90vh] overflow-y-auto animate-modal-enter"
          onclick={e => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label="Error Details"
          tabindex="-1"
          onkeydown={e => e.stopPropagation()}
        >
          <div class="modal-header">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
              Error Details
            </h3>
            <button
              onclick={() => {
                errSelectedLog = null;
              }}
              class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
              ><X class="w-5 h-5" /></button
            >
          </div>
          {#if errDetailLoading}
            <div class="p-6 text-center text-gray-500">Loading...</div>
          {:else if errSelectedLog}
            <div class="modal-body space-y-4">
              <div class="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >Request ID</span
                  >
                  <p class="font-mono text-xs break-all">
                    {errSelectedLog.request_id || '-'}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Timestamp</span
                  >
                  <p>{new Date(errSelectedLog.timestamp).toLocaleString()}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Category</span>
                  <p>
                    <span class={categoryBadge(errSelectedLog.error_category)}
                      >{errSelectedLog.error_category}</span
                    >
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >Error Code</span
                  >
                  <p>{errSelectedLog.error_code ?? '-'}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Provider</span>
                  <p>{errSelectedLog.provider_name || '-'}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Model</span>
                  <p>
                    {errSelectedLog.model_requested ||
                      '-'}{#if errSelectedLog.model_mapped && errSelectedLog.model_mapped !== errSelectedLog.model_requested}<span
                        class="text-gray-400"
                      >
                        &rarr; {errSelectedLog.model_mapped}</span
                      >{/if}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400"
                    >Credential</span
                  >
                  <p>{errSelectedLog.credential_name || '-'}</p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Protocol</span>
                  <p>
                    {errSelectedLog.client_protocol || '?'} &rarr; {errSelectedLog.provider_protocol ||
                      '?'}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Streaming</span
                  >
                  <p>
                    {errSelectedLog.is_streaming === null
                      ? '-'
                      : errSelectedLog.is_streaming
                        ? 'Yes'
                        : 'No'}
                  </p>
                </div>
                <div>
                  <span class="text-gray-500 dark:text-gray-400">Duration</span>
                  <p>{formatDuration(errSelectedLog.total_duration_ms)}</p>
                </div>
              </div>
              {#if errSelectedLog.error_message}
                <div>
                  <h4
                    class="text-sm font-medium text-red-600 dark:text-red-400 mb-1"
                  >
                    Error Message
                  </h4>
                  <pre
                    class="bg-red-50 dark:bg-red-900/20 p-3 rounded text-sm overflow-x-auto text-red-800 dark:text-red-300 whitespace-pre-wrap">{errSelectedLog.error_message}</pre>
                </div>
              {/if}
              {#if errSelectedLog.request_body}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Request Body
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">{tryFormatJson(
                      errSelectedLog.request_body
                    )}</pre>
                </div>
              {/if}
              {#if errSelectedLog.response_body}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Response Body
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">{tryFormatJson(
                      errSelectedLog.response_body
                    )}</pre>
                </div>
              {/if}
              {#if errSelectedLog.provider_request_body}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Provider Request Body
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">{tryFormatJson(
                      errSelectedLog.provider_request_body
                    )}</pre>
                </div>
              {/if}
              {#if errSelectedLog.provider_request_headers}
                <div>
                  <h4
                    class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
                  >
                    Provider Request Headers
                  </h4>
                  <pre
                    class="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-48 overflow-y-auto">{tryFormatJson(
                      errSelectedLog.provider_request_headers
                    )}</pre>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}

    {#if errShowDeleteConfirm}
      <div
        class="modal-overlay"
        onclick={() => {
          errShowDeleteConfirm = false;
        }}
        role="presentation"
        tabindex="-1"
        onkeydown={e => {
          if (e.key === 'Escape') errShowDeleteConfirm = false;
        }}
      >
        <div
          class="modal"
          onclick={e => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label="Delete Error Logs"
          tabindex="-1"
          onkeydown={e => e.stopPropagation()}
        >
          <div class="modal-header">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Delete Error Logs
            </h3>
            <button
              onclick={() => {
                errShowDeleteConfirm = false;
              }}
              class="btn-icon"><X class="w-5 h-5" /></button
            >
          </div>
          <div class="modal-body">
            <p class="text-sm text-gray-600 dark:text-gray-400">
              Are you sure you want to delete <strong
                >{errSelectedIds.size}</strong
              > selected error log(s)? This action cannot be undone.
            </p>
          </div>
          <div class="modal-footer">
            <button
              onclick={() => {
                errShowDeleteConfirm = false;
              }}
              class="btn btn-secondary">Cancel</button
            >
            <button
              onclick={handleErrBatchDelete}
              class="btn btn-danger flex items-center space-x-2"
              disabled={errDeleting}
            >
              {#if errDeleting}<span
                  class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"
                ></span>{/if}
              <span>Delete</span>
            </button>
          </div>
        </div>
      </div>
    {/if}
  {/if}
</div>
