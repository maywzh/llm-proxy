import React, { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
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
} from 'lucide-react';
import type {
  ProviderHealthStatus,
  HealthCheckResponse,
  HealthStatus,
  CheckProviderHealthResponse,
} from '../types';

const HEALTH_CACHE_KEY = 'llm-proxy-health-check-cache';

interface HealthCheckCache {
  timestamp: string;
  data: HealthCheckResponse;
}

const Health: React.FC = () => {
  const { apiClient } = useAuth();
  const [healthData, setHealthData] = useState<HealthCheckResponse | null>(
    null
  );
  const [lastCheckTime, setLastCheckTime] = useState<string | null>(null);
  const [loading] = useState(false);
  const [checking, setChecking] = useState(false);
  const [checkingProviders, setCheckingProviders] = useState<Set<number>>(
    new Set()
  );
  const [error, setError] = useState<string | null>(null);
  const [expandedProviders, setExpandedProviders] = useState<Set<number>>(
    new Set()
  );

  // Load cached data on mount
  useEffect(() => {
    const cached = localStorage.getItem(HEALTH_CACHE_KEY);
    if (cached) {
      try {
        const { timestamp, data }: HealthCheckCache = JSON.parse(cached);
        setHealthData(data);
        setLastCheckTime(timestamp);
        // Auto-expand unhealthy providers
        if (data.providers) {
          const unhealthyIds = new Set(
            data.providers
              .filter(p => p.status === 'unhealthy')
              .map(p => p.provider_id)
          );
          setExpandedProviders(unhealthyIds);
        }
      } catch {
        // Failed to load cached health data
        localStorage.removeItem(HEALTH_CACHE_KEY);
      }
    }
  }, []);

  const handleCheckHealth = useCallback(async () => {
    if (!apiClient) return;

    setChecking(true);
    setError(null);

    try {
      // Always fetch fresh provider list to pick up newly added providers
      const providersResponse = await apiClient.listProviders();
      const providerIds = providersResponse.providers.map(p => p.id);
      // Initialize health data, preserving existing results for known providers
      const initialProviders: ProviderHealthStatus[] =
        providersResponse.providers.map(p => {
          const existing = healthData?.providers.find(
            ep => ep.provider_id === p.id
          );
          return (
            existing || {
              provider_id: p.id,
              provider_key: p.provider_key,
              status: 'unknown' as HealthStatus,
              models: [],
              avg_response_time_ms: null,
              checked_at: new Date().toISOString(),
            }
          );
        });
      setHealthData({
        providers: initialProviders,
        total_providers: initialProviders.length,
        healthy_providers: initialProviders.filter(p => p.status === 'healthy')
          .length,
        unhealthy_providers: initialProviders.filter(
          p => p.status === 'unhealthy'
        ).length,
      });

      // Check all providers in parallel, updating each as results come in
      const checkPromises = providerIds.map(async id => {
        setCheckingProviders(prev => new Set(prev).add(id));
        try {
          const response = await apiClient.checkProviderHealth(id, {
            max_concurrent: 2,
            timeout_secs: 30,
          });

          // Update this specific provider in healthData
          setHealthData(prev => {
            if (!prev) return prev;

            const updatedProviders = prev.providers.map(p => {
              if (p.provider_id === id) {
                return {
                  provider_id: response.provider_id,
                  provider_key: response.provider_key,
                  status: response.status,
                  models: response.models,
                  avg_response_time_ms:
                    response.models.reduce(
                      (sum, m) => sum + (m.response_time_ms || 0),
                      0
                    ) / response.models.length || null,
                  checked_at: response.checked_at,
                };
              }
              return p;
            });

            const updatedHealthData = {
              ...prev,
              providers: updatedProviders,
              healthy_providers: updatedProviders.filter(
                p => p.status === 'healthy'
              ).length,
              unhealthy_providers: updatedProviders.filter(
                p => p.status === 'unhealthy'
              ).length,
            };

            // Update cache
            const cache: HealthCheckCache = {
              timestamp: new Date().toISOString(),
              data: updatedHealthData,
            };
            localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

            // Auto-expand if unhealthy
            if (response.status === 'unhealthy') {
              setExpandedProviders(prev => new Set(prev).add(id));
            }

            return updatedHealthData;
          });
        } catch {
          // Error handled silently - provider status remains unchanged
        } finally {
          setCheckingProviders(prev => {
            const next = new Set(prev);
            next.delete(id);
            return next;
          });
        }
      });

      await Promise.all(checkPromises);

      const timestamp = new Date().toISOString();
      setLastCheckTime(timestamp);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to check health status'
      );
    } finally {
      setChecking(false);
    }
  }, [apiClient]);

  const handleCheckProviderHealth = useCallback(
    async (providerId: number) => {
      if (!apiClient) return;

      setCheckingProviders(prev => new Set(prev).add(providerId));
      setError(null);

      try {
        const response: CheckProviderHealthResponse =
          await apiClient.checkProviderHealth(providerId, {
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
                    (sum, m) => sum + (m.response_time_ms || 0),
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

          setHealthData(updatedHealthData);

          // Update cache
          const cache: HealthCheckCache = {
            timestamp: lastCheckTime || new Date().toISOString(),
            data: updatedHealthData,
          };
          localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

          // Auto-expand if unhealthy
          if (response.status === 'unhealthy') {
            setExpandedProviders(prev => new Set(prev).add(providerId));
          }
        }
      } catch (err) {
        setError(
          err instanceof Error
            ? err.message
            : 'Failed to check provider health status'
        );
      } finally {
        setCheckingProviders(prev => {
          const next = new Set(prev);
          next.delete(providerId);
          return next;
        });
      }
    },
    [apiClient, healthData, lastCheckTime]
  );

  const toggleProvider = (providerId: number) => {
    setExpandedProviders(prev => {
      const next = new Set(prev);
      if (next.has(providerId)) {
        next.delete(providerId);
      } else {
        next.add(providerId);
      }
      return next;
    });
  };

  const getStatusIcon = (status: HealthStatus) => {
    switch (status) {
      case 'healthy':
        return <Check className="w-5 h-5 text-green-500" />;
      case 'unhealthy':
        return <XCircle className="w-5 h-5 text-red-500" />;
      case 'disabled':
        return <MinusCircle className="w-5 h-5 text-gray-400" />;
      default:
        return <HelpCircle className="w-5 h-5 text-gray-400" />;
    }
  };

  const getStatusBadgeClass = (status: HealthStatus) => {
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
  };

  const formatTimestamp = (timestamp: string) => {
    try {
      const date = new Date(timestamp);
      return date.toLocaleString();
    } catch {
      return timestamp;
    }
  };

  const formatRelativeTime = (timestamp: string) => {
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
  };

  const formatResponseTime = (ms: number | null) => {
    if (ms === null) return 'N/A';
    return `${ms}ms`;
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Health Check
          </h1>
          <p className="text-gray-600 dark:text-gray-400">
            Monitor provider and model health status
          </p>
        </div>
        <button
          onClick={handleCheckHealth}
          disabled={checking || loading}
          className="btn btn-primary flex items-center space-x-2"
        >
          <RefreshCw className={`w-5 h-5 ${checking ? 'animate-spin' : ''}`} />
          <span>Check Health</span>
        </button>
      </div>

      {/* Error Display */}
      {error && (
        <div className="alert-error">
          <div className="flex">
            <div className="shrink-0">
              <AlertCircle className="h-5 w-5 text-red-400" />
            </div>
            <div className="ml-3">
              <p className="text-sm text-red-700">{error}</p>
            </div>
            <div className="ml-auto pl-3">
              <button
                onClick={() => setError(null)}
                className="text-red-400 hover:text-red-600"
              >
                <X className="h-5 w-5" />
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Last Check Time */}
      {lastCheckTime && (
        <div className="card">
          <div className="card-body py-3">
            <div className="flex items-center space-x-2 text-sm text-gray-600 dark:text-gray-400">
              <Clock className="w-4 h-4" />
              <span>Last checked: {formatRelativeTime(lastCheckTime)}</span>
              <span className="text-gray-400">•</span>
              <span className="text-xs">{formatTimestamp(lastCheckTime)}</span>
            </div>
          </div>
        </div>
      )}

      {/* Statistics Cards */}
      {healthData && (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="card">
            <div className="card-body">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm text-gray-600 dark:text-gray-400">
                    Total Providers
                  </p>
                  <p className="text-2xl font-bold text-gray-900 dark:text-gray-100">
                    {healthData.total_providers}
                  </p>
                </div>
                <Activity className="w-8 h-8 text-blue-500" />
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-body">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm text-gray-600 dark:text-gray-400">
                    Healthy
                  </p>
                  <p className="text-2xl font-bold text-green-600">
                    {healthData.healthy_providers}
                  </p>
                </div>
                <Check className="w-8 h-8 text-green-500" />
              </div>
            </div>
          </div>

          <div className="card">
            <div className="card-body">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm text-gray-600 dark:text-gray-400">
                    Unhealthy
                  </p>
                  <p className="text-2xl font-bold text-red-600">
                    {healthData.unhealthy_providers}
                  </p>
                </div>
                <XCircle className="w-8 h-8 text-red-500" />
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Providers Grid */}
      <div className="space-y-4">
        <div className="flex justify-between items-center">
          <h2 className="text-xl font-bold text-gray-900 dark:text-gray-100">
            Provider Health Status
            {healthData && ` (${healthData.providers.length})`}
          </h2>
          {loading && (
            <div className="flex items-center text-gray-500 dark:text-gray-400">
              <Loader2 className="w-5 h-5 animate-spin mr-2" />
              <span className="text-sm">Loading...</span>
            </div>
          )}
        </div>

        {!healthData ? (
          <div className="card">
            <div className="card-body text-center py-12 text-gray-500 dark:text-gray-400">
              <Activity className="w-12 h-12 mx-auto mb-4 text-gray-400" />
              <p className="mb-2">No health check data available</p>
              <p className="text-sm mb-4">
                Click &quot;Check Health&quot; to start monitoring
              </p>
              <button
                onClick={handleCheckHealth}
                disabled={checking}
                className="btn btn-primary"
              >
                {checking ? (
                  <>
                    <Loader2 className="w-5 h-5 animate-spin mr-2" />
                    Checking...
                  </>
                ) : (
                  'Check Health'
                )}
              </button>
            </div>
          </div>
        ) : healthData.providers.length === 0 ? (
          <div className="card">
            <div className="card-body text-center py-12 text-gray-500 dark:text-gray-400">
              No providers configured yet.
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {healthData.providers.map((provider: ProviderHealthStatus) => (
              <div
                key={provider.provider_id}
                className="card hover:shadow-lg transition-shadow"
              >
                <div className="card-body">
                  {/* Provider Header */}
                  <div className="flex items-start justify-between mb-4">
                    <div className="flex items-center space-x-3 flex-1 min-w-0">
                      <div className="shrink-0">
                        {getStatusIcon(provider.status)}
                      </div>
                      <div className="flex-1 min-w-0">
                        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {provider.provider_key}
                        </h3>
                        <span className={getStatusBadgeClass(provider.status)}>
                          {provider.status}
                        </span>
                      </div>
                    </div>
                  </div>

                  {/* Provider Stats */}
                  <div className="space-y-2 mb-4">
                    <div className="flex items-center justify-between text-sm">
                      <span className="text-gray-600 dark:text-gray-400">
                        Models Tested
                      </span>
                      <span className="font-medium text-gray-900 dark:text-gray-100">
                        {provider.models.length}
                      </span>
                    </div>
                    {provider.avg_response_time_ms !== null && (
                      <div className="flex items-center justify-between text-sm">
                        <span className="text-gray-600 dark:text-gray-400">
                          Avg Response
                        </span>
                        <span className="font-medium text-gray-900 dark:text-gray-100">
                          {formatResponseTime(provider.avg_response_time_ms)}
                        </span>
                      </div>
                    )}
                  </div>

                  {/* Check Button */}
                  <button
                    onClick={e => {
                      e.stopPropagation();
                      handleCheckProviderHealth(provider.provider_id);
                    }}
                    disabled={checkingProviders.has(provider.provider_id)}
                    className="btn btn-sm btn-secondary w-full flex items-center justify-center space-x-2 mb-3"
                    title="Check this provider's health"
                  >
                    <RefreshCw
                      className={`w-4 h-4 ${checkingProviders.has(provider.provider_id) ? 'animate-spin' : ''}`}
                    />
                    <span>Check</span>
                  </button>

                  {/* Last Checked */}
                  <div className="text-xs text-gray-500 dark:text-gray-400 text-center border-t border-gray-200 dark:border-gray-700 pt-3">
                    <div className="flex items-center justify-center space-x-1">
                      <Clock className="w-3 h-3" />
                      <span>{formatTimestamp(provider.checked_at)}</span>
                    </div>
                  </div>

                  {/* Expand/Collapse Button */}
                  <button
                    onClick={() => toggleProvider(provider.provider_id)}
                    className="btn btn-sm btn-ghost w-full flex items-center justify-center space-x-2 mt-2"
                  >
                    <span className="text-sm">
                      {expandedProviders.has(provider.provider_id)
                        ? 'Hide Details'
                        : 'Show Details'}
                    </span>
                    {expandedProviders.has(provider.provider_id) ? (
                      <ChevronUp className="w-4 h-4" />
                    ) : (
                      <ChevronDown className="w-4 h-4" />
                    )}
                  </button>

                  {/* Model Details (Expanded) */}
                  {expandedProviders.has(provider.provider_id) && (
                    <div className="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700 space-y-2">
                      <h4 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2">
                        Model Test Results:
                      </h4>
                      {checkingProviders.has(provider.provider_id) ? (
                        <div className="flex items-center justify-center p-6 bg-gray-50 dark:bg-gray-900 rounded-lg">
                          <Loader2 className="w-5 h-5 animate-spin text-blue-500 mr-2" />
                          <span className="text-sm text-gray-600 dark:text-gray-400">
                            Checking...
                          </span>
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {provider.models.map((model, idx) => (
                            <div
                              key={idx}
                              className="p-3 bg-gray-50 dark:bg-gray-900 rounded-lg"
                            >
                              <div className="flex items-center justify-between mb-1">
                                <div className="flex items-center space-x-2">
                                  <div className="shrink-0">
                                    {getStatusIcon(model.status)}
                                  </div>
                                  <p className="font-medium text-sm text-gray-900 dark:text-gray-100 truncate">
                                    {model.model}
                                  </p>
                                </div>
                                <span
                                  className={`${getStatusBadgeClass(model.status)} text-xs`}
                                >
                                  {model.status}
                                </span>
                              </div>
                              {model.response_time_ms !== null && (
                                <div className="text-xs text-gray-600 dark:text-gray-400">
                                  {formatResponseTime(model.response_time_ms)}
                                </div>
                              )}
                              {model.error && (
                                <p className="text-xs text-red-600 dark:text-red-400 mt-1">
                                  {model.error}
                                </p>
                              )}
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

export default Health;
