import { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
import { useDebounce } from '../hooks/useDebounce';
import { TableSkeleton } from '../components/Skeleton';
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
} from 'lucide-react';
import type {
  RequestLog,
  RequestLogDetail,
  RequestLogFilters,
  RequestLogStats,
  ErrorLog,
  ErrorLogDetail,
  ErrorLogFilters,
} from '../types';

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

const PAGE_SIZE = 50;

type TabKey = 'requests' | 'errors';

// =============================================================================
// Request Logs Tab
// =============================================================================

function RequestLogsTab() {
  const { apiClient } = useAuth();
  const [logs, setLogs] = useState<RequestLog[]>([]);
  const [stats, setStats] = useState<RequestLogStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);

  const [searchTerm, setSearchTerm] = useState('');
  const debouncedSearch = useDebounce(searchTerm, 300);
  const [providerFilter, setProviderFilter] = useState('');
  const [modelFilter, setModelFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState('');
  const [streamingFilter, setStreamingFilter] = useState('');
  const [errorOnly, setErrorOnly] = useState(false);
  const [showFilters, setShowFilters] = useState(false);

  const [selectedLog, setSelectedLog] = useState<RequestLogDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [deleting, setDeleting] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const buildFilters = useCallback((): RequestLogFilters => {
    const f: RequestLogFilters = {};
    if (debouncedSearch) f.request_id = debouncedSearch;
    if (providerFilter) f.provider_name = providerFilter;
    if (modelFilter) f.model = modelFilter;
    if (statusFilter) f.status_code = Number(statusFilter);
    if (streamingFilter === 'true') f.is_streaming = true;
    if (streamingFilter === 'false') f.is_streaming = false;
    if (errorOnly) f.error_only = true;
    f.sort_by = 'timestamp';
    f.sort_order = 'desc';
    return f;
  }, [
    debouncedSearch,
    providerFilter,
    modelFilter,
    statusFilter,
    streamingFilter,
    errorOnly,
  ]);

  const loadLogs = useCallback(async () => {
    if (!apiClient) return;
    setLoading(true);
    setError(null);
    try {
      const filters = buildFilters();
      const res = await apiClient.listLogs(page, PAGE_SIZE, filters);
      setLogs(res.items);
      setTotal(res.total);
      setTotalPages(res.total_pages);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load logs');
    } finally {
      setLoading(false);
    }
  }, [apiClient, page, buildFilters]);

  const loadStats = useCallback(async () => {
    if (!apiClient) return;
    try {
      const res = await apiClient.getLogStats();
      setStats(res);
    } catch {
      // Stats are non-critical
    }
  }, [apiClient]);

  useEffect(() => {
    loadLogs();
  }, [loadLogs]);
  useEffect(() => {
    loadStats();
  }, [loadStats]);
  useEffect(() => {
    setPage(1);
  }, [
    debouncedSearch,
    providerFilter,
    modelFilter,
    statusFilter,
    streamingFilter,
    errorOnly,
  ]);

  const handleRowClick = async (log: RequestLog) => {
    if (!apiClient) return;
    setDetailLoading(true);
    try {
      const detail = await apiClient.getLog(log.id);
      setSelectedLog(detail);
    } catch {
      setSelectedLog(null);
    } finally {
      setDetailLoading(false);
    }
  };

  const handleExport = () => {
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
      ...logs.map(log =>
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
  };

  const clearFilters = () => {
    setSearchTerm('');
    setProviderFilter('');
    setModelFilter('');
    setStatusFilter('');
    setStreamingFilter('');
    setErrorOnly(false);
  };

  const toggleSelect = (id: number) => {
    setSelectedIds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleSelectAll = () => {
    if (selectedIds.size === logs.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(logs.map(l => l.id)));
    }
  };

  const handleBatchDelete = async () => {
    if (!apiClient || selectedIds.size === 0) return;
    setDeleting(true);
    try {
      await apiClient.batchDeleteLogs(Array.from(selectedIds));
      setSelectedIds(new Set());
      setShowDeleteConfirm(false);
      await loadLogs();
      await loadStats();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete logs');
    } finally {
      setDeleting(false);
    }
  };

  const hasActiveFilters =
    searchTerm ||
    providerFilter ||
    modelFilter ||
    statusFilter ||
    streamingFilter ||
    errorOnly;

  return (
    <div className="space-y-6">
      {/* Stats Cards */}
      {stats && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          <div className="card">
            <div className="card-body flex items-center space-x-3">
              <div className="p-2 bg-blue-100 dark:bg-blue-900/30 rounded-lg">
                <Hash className="w-5 h-5 text-blue-600 dark:text-blue-400" />
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  Total Requests
                </p>
                <p className="text-xl font-semibold text-gray-900 dark:text-white">
                  {formatNumber(stats.total_requests)}
                </p>
              </div>
            </div>
          </div>
          <div className="card">
            <div className="card-body flex items-center space-x-3">
              <div className="p-2 bg-red-100 dark:bg-red-900/30 rounded-lg">
                <AlertCircle className="w-5 h-5 text-red-600 dark:text-red-400" />
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  Error Rate
                </p>
                <p className="text-xl font-semibold text-gray-900 dark:text-white">
                  {(stats.error_rate * 100).toFixed(1)}%
                </p>
              </div>
            </div>
          </div>
          <div className="card">
            <div className="card-body flex items-center space-x-3">
              <div className="p-2 bg-amber-100 dark:bg-amber-900/30 rounded-lg">
                <Clock className="w-5 h-5 text-amber-600 dark:text-amber-400" />
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  Avg Duration
                </p>
                <p className="text-xl font-semibold text-gray-900 dark:text-white">
                  {formatDuration(stats.avg_duration_ms)}
                </p>
              </div>
            </div>
          </div>
          <div className="card">
            <div className="card-body flex items-center space-x-3">
              <div className="p-2 bg-emerald-100 dark:bg-emerald-900/30 rounded-lg">
                <Zap className="w-5 h-5 text-emerald-600 dark:text-emerald-400" />
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  Total Tokens
                </p>
                <p className="text-xl font-semibold text-gray-900 dark:text-white">
                  {formatNumber(
                    stats.total_input_tokens + stats.total_output_tokens
                  )}
                </p>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Search & Filters */}
      <div className="card">
        <div className="card-body space-y-3">
          <div className="flex items-center space-x-2">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
              <input
                type="text"
                placeholder="Search by full request ID..."
                value={searchTerm}
                onChange={e => setSearchTerm(e.target.value)}
                className="input pl-10 w-full"
              />
            </div>
            <button
              onClick={() => setShowFilters(!showFilters)}
              className={`btn ${showFilters ? 'btn-primary' : 'btn-secondary'} text-sm`}
            >
              <Filter className="w-4 h-4 mr-1" /> Filters
            </button>
            {hasActiveFilters && (
              <button
                onClick={clearFilters}
                className="btn btn-secondary text-sm"
              >
                <X className="w-4 h-4 mr-1" /> Clear
              </button>
            )}
            <button
              onClick={handleExport}
              className="btn btn-secondary text-sm"
              disabled={logs.length === 0}
              title="Export current page to CSV"
            >
              <Download className="w-4 h-4 mr-1" /> CSV
            </button>
            <button
              onClick={() => {
                loadLogs();
                loadStats();
              }}
              className="btn btn-secondary text-sm"
            >
              Refresh
            </button>
            {selectedIds.size > 0 && (
              <button
                onClick={() => setShowDeleteConfirm(true)}
                className="btn btn-danger text-sm"
              >
                <Trash2 className="w-4 h-4 mr-1" /> Delete ({selectedIds.size})
              </button>
            )}
          </div>
          {showFilters && (
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2 border-t border-gray-200 dark:border-gray-700">
              <div>
                <label className="label text-xs">Provider</label>
                <input
                  type="text"
                  placeholder="Provider name"
                  value={providerFilter}
                  onChange={e => setProviderFilter(e.target.value)}
                  className="input text-sm"
                />
              </div>
              <div>
                <label className="label text-xs">Model</label>
                <input
                  type="text"
                  placeholder="Model name"
                  value={modelFilter}
                  onChange={e => setModelFilter(e.target.value)}
                  className="input text-sm"
                />
              </div>
              <div>
                <label className="label text-xs">Status Code</label>
                <select
                  value={statusFilter}
                  onChange={e => setStatusFilter(e.target.value)}
                  className="input text-sm"
                >
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
                <label className="label text-xs">Streaming</label>
                <select
                  value={streamingFilter}
                  onChange={e => setStreamingFilter(e.target.value)}
                  className="input text-sm"
                >
                  <option value="">All</option>
                  <option value="true">Streaming</option>
                  <option value="false">Non-streaming</option>
                </select>
              </div>
              <div className="col-span-2 sm:col-span-4 flex items-center">
                <label className="flex items-center space-x-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={errorOnly}
                    onChange={e => setErrorOnly(e.target.checked)}
                    className="rounded border-gray-300 text-blue-600"
                  />
                  <span className="text-sm text-gray-700 dark:text-gray-300">
                    Errors only
                  </span>
                </label>
              </div>
            </div>
          )}
        </div>
      </div>

      {error && (
        <div className="alert-error">
          <AlertCircle className="w-5 h-5" />
          <span>{error}</span>
        </div>
      )}

      {/* Table */}
      <div className="table-container">
        {loading ? (
          <TableSkeleton rows={10} columns={12} />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th className="w-8">
                  <input
                    type="checkbox"
                    checked={
                      logs.length > 0 && selectedIds.size === logs.length
                    }
                    onChange={toggleSelectAll}
                    className="rounded border-gray-300 text-blue-600"
                  />
                </th>
                <th>Time</th>
                <th>Request ID</th>
                <th>Model</th>
                <th>Provider</th>
                <th>Credential</th>
                <th>Client</th>
                <th>Status</th>
                <th>Stream</th>
                <th>Tokens</th>
                <th>Duration</th>
                <th>TTFT</th>
              </tr>
            </thead>
            <tbody>
              {logs.length === 0 ? (
                <tr>
                  <td
                    colSpan={12}
                    className="text-center py-8 text-gray-500 dark:text-gray-400"
                  >
                    No logs found
                  </td>
                </tr>
              ) : (
                logs.map(log => (
                  <tr
                    key={log.id}
                    onClick={() => handleRowClick(log)}
                    className="cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800"
                  >
                    <td className="w-8" onClick={e => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        checked={selectedIds.has(log.id)}
                        onChange={() => toggleSelect(log.id)}
                        className="rounded border-gray-300 text-blue-600"
                      />
                    </td>
                    <td className="whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
                      {formatTimestamp(log.timestamp)}
                    </td>
                    <td
                      className="font-mono text-xs max-w-[120px] truncate"
                      title={log.request_id}
                    >
                      {log.request_id.slice(0, 12)}...
                    </td>
                    <td
                      className="text-sm max-w-[150px] truncate"
                      title={log.model_requested ?? ''}
                    >
                      {log.model_requested || '-'}
                    </td>
                    <td className="text-sm">{log.provider_name || '-'}</td>
                    <td className="text-sm">{log.credential_name || '-'}</td>
                    <td className="text-sm">{log.client || '-'}</td>
                    <td>
                      <span className={statusBadge(log.status_code)}>
                        {log.status_code ?? '-'}
                      </span>
                    </td>
                    <td className="text-sm text-gray-500">
                      {log.is_streaming ? 'true' : 'false'}
                    </td>
                    <td className="text-sm tabular-nums">
                      <span
                        title={`In: ${log.input_tokens} / Out: ${log.output_tokens}`}
                      >
                        {formatNumber(log.input_tokens)}/
                        {formatNumber(log.output_tokens)}
                      </span>
                    </td>
                    <td className="text-sm tabular-nums">
                      {formatDuration(log.total_duration_ms)}
                    </td>
                    <td className="text-sm tabular-nums">
                      {log.ttft_ms !== null ? formatDuration(log.ttft_ms) : '-'}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        )}
      </div>

      {/* Pagination */}
      {total > 0 && (
        <div className="flex items-center justify-between px-1">
          <span className="text-sm text-gray-500 dark:text-gray-400">
            {((page - 1) * PAGE_SIZE + 1).toLocaleString()} –{' '}
            {Math.min(page * PAGE_SIZE, total).toLocaleString()} of{' '}
            {total.toLocaleString()}
          </span>
          <div className="flex items-center space-x-2">
            <button
              onClick={() => setPage(p => Math.max(1, p - 1))}
              disabled={page === 1}
              className="btn btn-secondary text-sm"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Page {page} of {totalPages}
            </span>
            <button
              onClick={() => setPage(p => Math.min(totalPages, p + 1))}
              disabled={page === totalPages}
              className="btn btn-secondary text-sm"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        </div>
      )}

      {/* Detail Modal */}
      {(selectedLog || detailLoading) && (
        <div
          className="modal-overlay"
          onClick={() => !detailLoading && setSelectedLog(null)}
          role="presentation"
        >
          <div
            className="modal max-w-3xl w-full max-h-[90vh] overflow-y-auto animate-modal-enter"
            onClick={e => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label="Request Details"
            onKeyDown={e => {
              if (e.key === 'Escape') setSelectedLog(null);
            }}
          >
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
                Request Details
              </h3>
              <button
                onClick={() => setSelectedLog(null)}
                className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            {detailLoading ? (
              <div className="p-6 text-center text-gray-500">Loading...</div>
            ) : selectedLog ? (
              <div className="modal-body space-y-4">
                <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Request ID
                    </span>
                    <p className="font-mono text-xs break-all">
                      {selectedLog.request_id}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Timestamp
                    </span>
                    <p>{new Date(selectedLog.timestamp).toLocaleString()}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Model
                    </span>
                    <p>
                      {selectedLog.model_requested}
                      {selectedLog.model_mapped &&
                        selectedLog.model_mapped !==
                          selectedLog.model_requested && (
                          <span className="text-gray-400">
                            {' '}
                            {' \u2192 '}
                            {selectedLog.model_mapped}
                          </span>
                        )}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Provider
                    </span>
                    <p>
                      {selectedLog.provider_name}{' '}
                      {selectedLog.provider_type && (
                        <span className="badge badge-info text-xs">
                          {selectedLog.provider_type}
                        </span>
                      )}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Credential
                    </span>
                    <p>{selectedLog.credential_name || '-'}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Client
                    </span>
                    <p>{selectedLog.client || '-'}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Client IP
                    </span>
                    <p className="font-mono text-xs">
                      {extractClientIp(selectedLog.request_headers) || '-'}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      User-Agent
                    </span>
                    <p className="text-xs break-all">
                      {extractFromHeaders(
                        selectedLog.request_headers,
                        'user-agent'
                      ) || '-'}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Status
                    </span>
                    <p>
                      <span className={statusBadge(selectedLog.status_code)}>
                        {selectedLog.status_code}
                      </span>
                      {selectedLog.error_category && (
                        <span className="ml-2 text-red-600 dark:text-red-400 text-xs">
                          {selectedLog.error_category}
                        </span>
                      )}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Protocol
                    </span>
                    <p>
                      {selectedLog.client_protocol || '?'} {'\u2192'}{' '}
                      {selectedLog.provider_protocol || '?'}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Streaming
                    </span>
                    <p>{selectedLog.is_streaming ? 'Yes' : 'No'}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Tokens
                    </span>
                    <p>
                      In: {selectedLog.input_tokens.toLocaleString()} / Out:{' '}
                      {selectedLog.output_tokens.toLocaleString()} / Total:{' '}
                      {selectedLog.total_tokens.toLocaleString()}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Duration / TTFT
                    </span>
                    <p>
                      {formatDuration(selectedLog.total_duration_ms)}
                      {selectedLog.ttft_ms !== null && (
                        <span className="text-gray-400">
                          {' '}
                          (TTFT: {formatDuration(selectedLog.ttft_ms)})
                        </span>
                      )}
                    </p>
                  </div>
                </div>
                {selectedLog.error_message && (
                  <div>
                    <h4 className="text-sm font-medium text-red-600 dark:text-red-400 mb-1">
                      Error Message
                    </h4>
                    <pre className="bg-red-50 dark:bg-red-900/20 p-3 rounded text-sm overflow-x-auto text-red-800 dark:text-red-300">
                      {selectedLog.error_message}
                    </pre>
                  </div>
                )}
                {selectedLog.request_headers && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Request Headers
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-48 overflow-y-auto">
                      {sanitizeHeaders(selectedLog.request_headers)}
                    </pre>
                  </div>
                )}
                {selectedLog.request_body && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Request Body
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">
                      {tryFormatJson(selectedLog.request_body)}
                    </pre>
                  </div>
                )}
                {selectedLog.response_body && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Response Body
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">
                      {tryFormatJson(selectedLog.response_body)}
                    </pre>
                  </div>
                )}
              </div>
            ) : null}
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal */}
      {showDeleteConfirm && (
        <div
          className="modal-overlay"
          onClick={() => setShowDeleteConfirm(false)}
          role="presentation"
        >
          <div
            className="modal"
            onClick={e => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label="Delete Request Logs"
            onKeyDown={e => {
              if (e.key === 'Escape') setShowDeleteConfirm(false);
            }}
          >
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Delete Request Logs
              </h3>
              <button
                onClick={() => setShowDeleteConfirm(false)}
                className="btn-icon"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="modal-body">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Are you sure you want to delete{' '}
                <strong>{selectedIds.size}</strong> selected log(s)? This action
                cannot be undone.
              </p>
            </div>
            <div className="modal-footer">
              <button
                onClick={() => setShowDeleteConfirm(false)}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                onClick={handleBatchDelete}
                className="btn btn-danger flex items-center space-x-2"
                disabled={deleting}
              >
                {deleting && (
                  <span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                )}
                <span>Delete</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// =============================================================================
// Error Logs Tab
// =============================================================================

function ErrorLogsTab() {
  const { apiClient } = useAuth();
  const [logs, setLogs] = useState<ErrorLog[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);

  const [searchTerm, setSearchTerm] = useState('');
  const debouncedSearch = useDebounce(searchTerm, 300);
  const [providerFilter, setProviderFilter] = useState('');
  const [categoryFilter, setCategoryFilter] = useState('');
  const [showFilters, setShowFilters] = useState(false);

  const [selectedLog, setSelectedLog] = useState<ErrorLogDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [deleting, setDeleting] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const buildFilters = useCallback((): ErrorLogFilters => {
    const f: ErrorLogFilters = {};
    if (debouncedSearch) f.request_id = debouncedSearch;
    if (providerFilter) f.provider_name = providerFilter;
    if (categoryFilter) f.error_category = categoryFilter;
    f.sort_by = 'timestamp';
    f.sort_order = 'desc';
    return f;
  }, [debouncedSearch, providerFilter, categoryFilter]);

  const loadLogs = useCallback(async () => {
    if (!apiClient) return;
    setLoading(true);
    setError(null);
    try {
      const filters = buildFilters();
      const res = await apiClient.listErrorLogs(page, PAGE_SIZE, filters);
      setLogs(res.items);
      setTotal(res.total);
      setTotalPages(res.total_pages);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to load error logs'
      );
    } finally {
      setLoading(false);
    }
  }, [apiClient, page, buildFilters]);

  useEffect(() => {
    loadLogs();
  }, [loadLogs]);
  useEffect(() => {
    setPage(1);
  }, [debouncedSearch, providerFilter, categoryFilter]);

  const handleRowClick = async (log: ErrorLog) => {
    if (!apiClient) return;
    setDetailLoading(true);
    try {
      const detail = await apiClient.getErrorLog(log.id);
      setSelectedLog(detail);
    } catch {
      setSelectedLog(null);
    } finally {
      setDetailLoading(false);
    }
  };

  const clearFilters = () => {
    setSearchTerm('');
    setProviderFilter('');
    setCategoryFilter('');
  };

  const toggleSelect = (id: number) => {
    setSelectedIds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleSelectAll = () => {
    if (selectedIds.size === logs.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(logs.map(l => l.id)));
    }
  };

  const handleBatchDelete = async () => {
    if (!apiClient || selectedIds.size === 0) return;
    setDeleting(true);
    try {
      await apiClient.batchDeleteErrorLogs(Array.from(selectedIds));
      setSelectedIds(new Set());
      setShowDeleteConfirm(false);
      await loadLogs();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to delete error logs'
      );
    } finally {
      setDeleting(false);
    }
  };

  const hasActiveFilters = searchTerm || providerFilter || categoryFilter;

  return (
    <div className="space-y-6">
      {/* Search & Filters */}
      <div className="card">
        <div className="card-body space-y-3">
          <div className="flex items-center space-x-2">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
              <input
                type="text"
                placeholder="Search by full request ID..."
                value={searchTerm}
                onChange={e => setSearchTerm(e.target.value)}
                className="input pl-10 w-full"
              />
            </div>
            <button
              onClick={() => setShowFilters(!showFilters)}
              className={`btn ${showFilters ? 'btn-primary' : 'btn-secondary'} text-sm`}
            >
              <Filter className="w-4 h-4 mr-1" /> Filters
            </button>
            {hasActiveFilters && (
              <button
                onClick={clearFilters}
                className="btn btn-secondary text-sm"
              >
                <X className="w-4 h-4 mr-1" /> Clear
              </button>
            )}
            <button onClick={loadLogs} className="btn btn-secondary text-sm">
              Refresh
            </button>
            {selectedIds.size > 0 && (
              <button
                onClick={() => setShowDeleteConfirm(true)}
                className="btn btn-danger text-sm"
              >
                <Trash2 className="w-4 h-4 mr-1" /> Delete ({selectedIds.size})
              </button>
            )}
          </div>
          {showFilters && (
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-3 pt-2 border-t border-gray-200 dark:border-gray-700">
              <div>
                <label className="label text-xs">Provider</label>
                <input
                  type="text"
                  placeholder="Provider name"
                  value={providerFilter}
                  onChange={e => setProviderFilter(e.target.value)}
                  className="input text-sm"
                />
              </div>
              <div>
                <label className="label text-xs">Error Category</label>
                <input
                  type="text"
                  placeholder="e.g. timeout, auth_error"
                  value={categoryFilter}
                  onChange={e => setCategoryFilter(e.target.value)}
                  className="input text-sm"
                />
              </div>
            </div>
          )}
        </div>
      </div>

      {error && (
        <div className="alert-error">
          <AlertCircle className="w-5 h-5" />
          <span>{error}</span>
        </div>
      )}

      {/* Table */}
      <div className="table-container">
        {loading ? (
          <TableSkeleton rows={10} columns={9} />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th className="w-8">
                  <input
                    type="checkbox"
                    checked={
                      logs.length > 0 && selectedIds.size === logs.length
                    }
                    onChange={toggleSelectAll}
                    className="rounded border-gray-300 text-blue-600"
                  />
                </th>
                <th>Time</th>
                <th>Request ID</th>
                <th>Category</th>
                <th>Provider</th>
                <th>Credential</th>
                <th>Model</th>
                <th>Message</th>
                <th>Duration</th>
              </tr>
            </thead>
            <tbody>
              {logs.length === 0 ? (
                <tr>
                  <td
                    colSpan={9}
                    className="text-center py-8 text-gray-500 dark:text-gray-400"
                  >
                    No error logs found
                  </td>
                </tr>
              ) : (
                logs.map(log => (
                  <tr
                    key={log.id}
                    onClick={() => handleRowClick(log)}
                    className="cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800"
                  >
                    <td className="w-8" onClick={e => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        checked={selectedIds.has(log.id)}
                        onChange={() => toggleSelect(log.id)}
                        className="rounded border-gray-300 text-blue-600"
                      />
                    </td>
                    <td className="whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
                      {formatTimestamp(log.timestamp)}
                    </td>
                    <td
                      className="font-mono text-xs max-w-[120px] truncate"
                      title={log.request_id ?? ''}
                    >
                      {log.request_id
                        ? `${log.request_id.slice(0, 12)}...`
                        : '-'}
                    </td>
                    <td>
                      <span className={categoryBadge(log.error_category)}>
                        {log.error_category}
                      </span>
                    </td>
                    <td className="text-sm">{log.provider_name || '-'}</td>
                    <td className="text-sm">{log.credential_name || '-'}</td>
                    <td
                      className="text-sm max-w-[150px] truncate"
                      title={log.model_requested ?? ''}
                    >
                      {log.model_requested || '-'}
                    </td>
                    <td
                      className="text-sm max-w-[250px] truncate text-red-600 dark:text-red-400"
                      title={log.error_message ?? ''}
                    >
                      {log.error_message || '-'}
                    </td>
                    <td className="text-sm tabular-nums">
                      {formatDuration(log.total_duration_ms)}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        )}
      </div>

      {/* Pagination */}
      {total > 0 && (
        <div className="flex items-center justify-between px-1">
          <span className="text-sm text-gray-500 dark:text-gray-400">
            {((page - 1) * PAGE_SIZE + 1).toLocaleString()} –{' '}
            {Math.min(page * PAGE_SIZE, total).toLocaleString()} of{' '}
            {total.toLocaleString()}
          </span>
          <div className="flex items-center space-x-2">
            <button
              onClick={() => setPage(p => Math.max(1, p - 1))}
              disabled={page === 1}
              className="btn btn-secondary text-sm"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Page {page} of {totalPages}
            </span>
            <button
              onClick={() => setPage(p => Math.min(totalPages, p + 1))}
              disabled={page === totalPages}
              className="btn btn-secondary text-sm"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        </div>
      )}

      {/* Detail Modal */}
      {(selectedLog || detailLoading) && (
        <div
          className="modal-overlay"
          onClick={() => !detailLoading && setSelectedLog(null)}
          role="presentation"
        >
          <div
            className="modal max-w-3xl w-full max-h-[90vh] overflow-y-auto animate-modal-enter"
            onClick={e => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label="Error Details"
            onKeyDown={e => {
              if (e.key === 'Escape') setSelectedLog(null);
            }}
          >
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
                Error Details
              </h3>
              <button
                onClick={() => setSelectedLog(null)}
                className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            {detailLoading ? (
              <div className="p-6 text-center text-gray-500">Loading...</div>
            ) : selectedLog ? (
              <div className="modal-body space-y-4">
                <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Request ID
                    </span>
                    <p className="font-mono text-xs break-all">
                      {selectedLog.request_id || '-'}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Timestamp
                    </span>
                    <p>{new Date(selectedLog.timestamp).toLocaleString()}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Category
                    </span>
                    <p>
                      <span
                        className={categoryBadge(selectedLog.error_category)}
                      >
                        {selectedLog.error_category}
                      </span>
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Error Code
                    </span>
                    <p>{selectedLog.error_code ?? '-'}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Provider
                    </span>
                    <p>{selectedLog.provider_name || '-'}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Model
                    </span>
                    <p>
                      {selectedLog.model_requested || '-'}
                      {selectedLog.model_mapped &&
                        selectedLog.model_mapped !==
                          selectedLog.model_requested && (
                          <span className="text-gray-400">
                            {' '}
                            {'\u2192'} {selectedLog.model_mapped}
                          </span>
                        )}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Credential
                    </span>
                    <p>{selectedLog.credential_name || '-'}</p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Protocol
                    </span>
                    <p>
                      {selectedLog.client_protocol || '?'} {'\u2192'}{' '}
                      {selectedLog.provider_protocol || '?'}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Streaming
                    </span>
                    <p>
                      {selectedLog.is_streaming === null
                        ? '-'
                        : selectedLog.is_streaming
                          ? 'Yes'
                          : 'No'}
                    </p>
                  </div>
                  <div>
                    <span className="text-gray-500 dark:text-gray-400">
                      Duration
                    </span>
                    <p>{formatDuration(selectedLog.total_duration_ms)}</p>
                  </div>
                </div>
                {selectedLog.error_message && (
                  <div>
                    <h4 className="text-sm font-medium text-red-600 dark:text-red-400 mb-1">
                      Error Message
                    </h4>
                    <pre className="bg-red-50 dark:bg-red-900/20 p-3 rounded text-sm overflow-x-auto text-red-800 dark:text-red-300 whitespace-pre-wrap">
                      {selectedLog.error_message}
                    </pre>
                  </div>
                )}
                {selectedLog.request_body && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Request Body
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">
                      {tryFormatJson(selectedLog.request_body)}
                    </pre>
                  </div>
                )}
                {selectedLog.response_body && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Response Body
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">
                      {tryFormatJson(selectedLog.response_body)}
                    </pre>
                  </div>
                )}
                {selectedLog.provider_request_body && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Provider Request Body
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-64 overflow-y-auto">
                      {tryFormatJson(selectedLog.provider_request_body)}
                    </pre>
                  </div>
                )}
                {selectedLog.provider_request_headers && (
                  <div>
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                      Provider Request Headers
                    </h4>
                    <pre className="bg-gray-50 dark:bg-gray-800 p-3 rounded text-xs overflow-x-auto max-h-48 overflow-y-auto">
                      {sanitizeHeaders(selectedLog.provider_request_headers)}
                    </pre>
                  </div>
                )}
              </div>
            ) : null}
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal */}
      {showDeleteConfirm && (
        <div
          className="modal-overlay"
          onClick={() => setShowDeleteConfirm(false)}
          role="presentation"
        >
          <div
            className="modal"
            onClick={e => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label="Delete Error Logs"
            onKeyDown={e => {
              if (e.key === 'Escape') setShowDeleteConfirm(false);
            }}
          >
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Delete Error Logs
              </h3>
              <button
                onClick={() => setShowDeleteConfirm(false)}
                className="btn-icon"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="modal-body">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Are you sure you want to delete{' '}
                <strong>{selectedIds.size}</strong> selected error log(s)? This
                action cannot be undone.
              </p>
            </div>
            <div className="modal-footer">
              <button
                onClick={() => setShowDeleteConfirm(false)}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                onClick={handleBatchDelete}
                className="btn btn-danger flex items-center space-x-2"
                disabled={deleting}
              >
                {deleting && (
                  <span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                )}
                <span>Delete</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// =============================================================================
// Main Logs Page (Tab Container)
// =============================================================================

export default function Logs() {
  const [activeTab, setActiveTab] = useState<TabKey>('requests');

  return (
    <div className="space-y-6">
      {/* Header + Tabs */}
      <div>
        <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">
          Logs
        </h1>
        <div className="mt-4 border-b border-gray-200 dark:border-gray-700">
          <nav className="flex space-x-8">
            <button
              onClick={() => setActiveTab('requests')}
              className={`pb-3 text-sm font-medium border-b-2 transition-colors ${
                activeTab === 'requests'
                  ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                  : 'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300'
              }`}
            >
              Request Logs
            </button>
            <button
              onClick={() => setActiveTab('errors')}
              className={`pb-3 text-sm font-medium border-b-2 transition-colors ${
                activeTab === 'errors'
                  ? 'border-red-500 text-red-600 dark:text-red-400'
                  : 'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300'
              }`}
            >
              Error Logs
            </button>
          </nav>
        </div>
      </div>

      {/* Tab Content */}
      <div style={{ display: activeTab === 'requests' ? 'block' : 'none' }}>
        <RequestLogsTab />
      </div>
      <div style={{ display: activeTab === 'errors' ? 'block' : 'none' }}>
        <ErrorLogsTab />
      </div>
    </div>
  );
}
