import React, { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
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
} from 'lucide-react';
import ProviderIcon from '../components/ProviderIcon';
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
  const [reloading, setReloading] = useState(false);
  const [checkingProviders, setCheckingProviders] = useState<Set<number>>(
    new Set()
  );
  const [checkingModels, setCheckingModels] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [expandedProviders, setExpandedProviders] = useState<Set<number>>(
    new Set()
  );

  useEffect(() => {
    const cached = localStorage.getItem(HEALTH_CACHE_KEY);
    if (cached) {
      try {
        const { timestamp, data }: HealthCheckCache = JSON.parse(cached);
        setHealthData(data);
        setLastCheckTime(timestamp);
        if (data.providers) {
          const unhealthyIds = new Set(
            data.providers
              .filter(p => p.status === 'unhealthy')
              .map(p => p.provider_id)
          );
          setExpandedProviders(unhealthyIds);
        }
      } catch {
        localStorage.removeItem(HEALTH_CACHE_KEY);
      }
    }
  }, []);

  const handleReloadProviders = useCallback(async () => {
    if (!apiClient) return;

    setReloading(true);
    setError(null);

    try {
      const providersResponse = await apiClient.listProviders();
      const updatedProviders: ProviderHealthStatus[] =
        providersResponse.providers.map(p => {
          const existing = healthData?.providers.find(
            ep => ep.provider_id === p.id
          );
          return (
            existing || {
              provider_id: p.id,
              provider_key: p.provider_key,
              provider_type: p.provider_type,
              status: 'unknown' as HealthStatus,
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

      setHealthData(updatedHealthData);

      const cache: HealthCheckCache = {
        timestamp: lastCheckTime || new Date().toISOString(),
        data: updatedHealthData,
      };
      localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to reload providers'
      );
    } finally {
      setReloading(false);
    }
  }, [apiClient, healthData, lastCheckTime]);

  const handleCheckHealth = useCallback(async () => {
    if (!apiClient) return;

    setChecking(true);
    setError(null);

    try {
      const providersResponse = await apiClient.listProviders();
      const providerIds = providersResponse.providers.map(p => p.id);
      const initialProviders: ProviderHealthStatus[] =
        providersResponse.providers.map(p => {
          const existing = healthData?.providers.find(
            ep => ep.provider_id === p.id
          );
          return (
            existing || {
              provider_id: p.id,
              provider_key: p.provider_key,
              provider_type: p.provider_type,
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

      const checkPromises = providerIds.map(async id => {
        setCheckingProviders(prev => new Set(prev).add(id));
        try {
          const response = await apiClient.checkProviderHealth(id, {
            max_concurrent: 2,
            timeout_secs: 30,
          });

          setHealthData(prev => {
            if (!prev) return prev;

            const updatedProviders = prev.providers.map(p => {
              if (p.provider_id === id) {
                return {
                  provider_id: response.provider_id,
                  provider_key: response.provider_key,
                  provider_type: response.provider_type,
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

            const cache: HealthCheckCache = {
              timestamp: new Date().toISOString(),
              data: updatedHealthData,
            };
            localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

            if (response.status === 'unhealthy') {
              setExpandedProviders(prev => new Set(prev).add(id));
            }

            return updatedHealthData;
          });
        } catch {
          // Provider check failed silently
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

          const cache: HealthCheckCache = {
            timestamp: lastCheckTime || new Date().toISOString(),
            data: updatedHealthData,
          };
          localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));

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

  const handleCheckModelHealth = useCallback(
    async (providerId: number, modelName: string) => {
      if (!apiClient) return;

      const modelKey = `${providerId}-${modelName}`;
      setCheckingModels(prev => new Set(prev).add(modelKey));
      setError(null);

      try {
        const response = await apiClient.checkProviderHealth(providerId, {
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

          const updatedHealthData = {
            ...healthData,
            providers: updatedProviders,
          };

          setHealthData(updatedHealthData);

          const cache: HealthCheckCache = {
            timestamp: lastCheckTime || new Date().toISOString(),
            data: updatedHealthData,
          };
          localStorage.setItem(HEALTH_CACHE_KEY, JSON.stringify(cache));
        }
      } catch (err) {
        setError(
          err instanceof Error ? err.message : 'Failed to check model health'
        );
      } finally {
        setCheckingModels(prev => {
          const next = new Set(prev);
          next.delete(modelKey);
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

  const getStatusIcon = (status: HealthStatus, size = 'w-5 h-5') => {
    switch (status) {
      case 'healthy':
        return <CheckCircle2 className={`${size} text-emerald-500`} />;
      case 'unhealthy':
        return <XCircle className={`${size} text-red-500`} />;
      case 'disabled':
        return <MinusCircle className={`${size} text-gray-400`} />;
      default:
        return <CircleDashed className={`${size} text-gray-400`} />;
    }
  };

  const getStatusBadge = (status: HealthStatus) => {
    const base =
      'inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium';
    switch (status) {
      case 'healthy':
        return (
          <span
            className={`${base} bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400`}
          >
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
            healthy
          </span>
        );
      case 'unhealthy':
        return (
          <span
            className={`${base} bg-red-50 text-red-700 dark:bg-red-900/30 dark:text-red-400`}
          >
            <span className="w-1.5 h-1.5 rounded-full bg-red-500" />
            unhealthy
          </span>
        );
      case 'disabled':
        return (
          <span
            className={`${base} bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400`}
          >
            <span className="w-1.5 h-1.5 rounded-full bg-gray-400" />
            disabled
          </span>
        );
      default:
        return (
          <span
            className={`${base} bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400`}
          >
            <span className="w-1.5 h-1.5 rounded-full bg-gray-400" />
            unknown
          </span>
        );
    }
  };

  const getCardBorderClass = (status: HealthStatus) => {
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
    if (ms < 1000) return `${Math.round(ms)}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  const getResponseTimeColor = (ms: number | null) => {
    if (ms === null) return 'text-gray-400';
    if (ms < 500) return 'text-emerald-600 dark:text-emerald-400';
    if (ms < 2000) return 'text-amber-600 dark:text-amber-400';
    return 'text-red-600 dark:text-red-400';
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-primary-100 dark:bg-primary-600/20 rounded-lg">
            <HeartPulse className="w-6 h-6 text-primary-600 dark:text-primary-400" />
          </div>
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
              Health Check
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Monitor provider and model health status
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleReloadProviders}
            disabled={reloading || checking}
            className="btn btn-secondary flex items-center gap-2"
          >
            <RotateCw
              className={`w-4 h-4 ${reloading ? 'animate-spin' : ''}`}
            />
            <span>Reload</span>
          </button>
          <button
            onClick={handleCheckHealth}
            disabled={checking || loading}
            className="btn btn-primary flex items-center gap-2"
          >
            <RefreshCw
              className={`w-4 h-4 ${checking ? 'animate-spin' : ''}`}
            />
            <span>{checking ? 'Checking...' : 'Check All'}</span>
          </button>
        </div>
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
        <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 px-1">
          <Clock className="w-3.5 h-3.5" />
          <span>Last checked: {formatRelativeTime(lastCheckTime)}</span>
          <span className="text-gray-300 dark:text-gray-600">|</span>
          <span className="text-xs">{formatTimestamp(lastCheckTime)}</span>
        </div>
      )}

      {/* Statistics Cards */}
      {healthData && (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="card overflow-hidden">
            <div className="card-body relative">
              <div className="absolute top-0 right-0 w-20 h-20 bg-blue-50 dark:bg-blue-900/10 rounded-bl-full" />
              <div className="flex items-center justify-between relative">
                <div>
                  <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1">
                    Total Providers
                  </p>
                  <p className="text-3xl font-bold text-gray-900 dark:text-gray-100">
                    {healthData.total_providers}
                  </p>
                </div>
                <div className="p-3 bg-blue-100 dark:bg-blue-900/30 rounded-xl">
                  <Server className="w-6 h-6 text-blue-600 dark:text-blue-400" />
                </div>
              </div>
            </div>
          </div>

          <div className="card overflow-hidden">
            <div className="card-body relative">
              <div className="absolute top-0 right-0 w-20 h-20 bg-emerald-50 dark:bg-emerald-900/10 rounded-bl-full" />
              <div className="flex items-center justify-between relative">
                <div>
                  <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1">
                    Healthy
                  </p>
                  <p className="text-3xl font-bold text-emerald-600 dark:text-emerald-400">
                    {healthData.healthy_providers}
                  </p>
                </div>
                <div className="p-3 bg-emerald-100 dark:bg-emerald-900/30 rounded-xl">
                  <ShieldCheck className="w-6 h-6 text-emerald-600 dark:text-emerald-400" />
                </div>
              </div>
            </div>
          </div>

          <div className="card overflow-hidden">
            <div className="card-body relative">
              <div className="absolute top-0 right-0 w-20 h-20 bg-red-50 dark:bg-red-900/10 rounded-bl-full" />
              <div className="flex items-center justify-between relative">
                <div>
                  <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1">
                    Unhealthy
                  </p>
                  <p className="text-3xl font-bold text-red-600 dark:text-red-400">
                    {healthData.unhealthy_providers}
                  </p>
                </div>
                <div className="p-3 bg-red-100 dark:bg-red-900/30 rounded-xl">
                  <ShieldAlert className="w-6 h-6 text-red-600 dark:text-red-400" />
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Providers Grid */}
      <div className="space-y-4">
        <div className="flex justify-between items-center">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Provider Health Status
            {healthData && (
              <span className="ml-2 text-sm font-normal text-gray-500 dark:text-gray-400">
                ({healthData.providers.length})
              </span>
            )}
          </h2>
          {loading && (
            <div className="flex items-center text-gray-500 dark:text-gray-400">
              <Loader2 className="w-4 h-4 animate-spin mr-2" />
              <span className="text-sm">Loading...</span>
            </div>
          )}
        </div>

        {!healthData ? (
          <div className="card">
            <div className="card-body text-center py-16">
              <div className="inline-flex p-4 bg-gray-100 dark:bg-gray-800 rounded-full mb-4">
                <HeartPulse className="w-10 h-10 text-gray-400" />
              </div>
              <p className="text-gray-700 dark:text-gray-300 font-medium mb-1">
                No health check data available
              </p>
              <p className="text-sm text-gray-500 dark:text-gray-400 mb-6">
                Click &quot;Check All&quot; to start monitoring your providers
              </p>
              <button
                onClick={handleCheckHealth}
                disabled={checking}
                className="btn btn-primary"
              >
                {checking ? (
                  <>
                    <Loader2 className="w-4 h-4 animate-spin mr-2" />
                    Checking...
                  </>
                ) : (
                  'Check All'
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
                className={`card hover:shadow-lg transition-all duration-200 ${getCardBorderClass(provider.status)}`}
              >
                <div className="card-body">
                  {/* Provider Header */}
                  <div className="flex items-start justify-between mb-3">
                    <div className="flex items-center gap-3 flex-1 min-w-0">
                      <div className="shrink-0 p-1.5 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
                        <ProviderIcon
                          providerKey={provider.provider_key}
                          providerType={provider.provider_type}
                          className="w-6 h-6"
                        />
                      </div>
                      <div className="flex-1 min-w-0">
                        <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {provider.provider_key}
                        </h3>
                        {getStatusBadge(provider.status)}
                      </div>
                    </div>
                    {checkingProviders.has(provider.provider_id) && (
                      <Loader2 className="w-4 h-4 animate-spin text-blue-500 shrink-0" />
                    )}
                  </div>

                  {/* Provider Stats */}
                  <div className="grid grid-cols-2 gap-3 mb-3 py-3 border-y border-gray-100 dark:border-gray-700">
                    <div>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mb-0.5">
                        Models
                      </p>
                      <p className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                        {provider.models.length}
                      </p>
                    </div>
                    <div>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mb-0.5">
                        Avg Response
                      </p>
                      <p
                        className={`text-sm font-semibold flex items-center gap-1 ${getResponseTimeColor(provider.avg_response_time_ms)}`}
                      >
                        <Zap className="w-3 h-3" />
                        {formatResponseTime(provider.avg_response_time_ms)}
                      </p>
                    </div>
                  </div>

                  {/* Action Buttons */}
                  <div className="flex gap-2 mb-3">
                    <button
                      onClick={e => {
                        e.stopPropagation();
                        handleCheckProviderHealth(provider.provider_id);
                      }}
                      disabled={checkingProviders.has(provider.provider_id)}
                      className="btn btn-sm btn-secondary flex-1 flex items-center justify-center gap-1.5"
                      title="Check this provider's health"
                    >
                      <RefreshCw
                        className={`w-3.5 h-3.5 ${checkingProviders.has(provider.provider_id) ? 'animate-spin' : ''}`}
                      />
                      <span>Check</span>
                    </button>
                    <button
                      onClick={() => toggleProvider(provider.provider_id)}
                      className="btn btn-sm btn-ghost flex items-center justify-center gap-1"
                    >
                      <span className="text-xs">
                        {expandedProviders.has(provider.provider_id)
                          ? 'Hide'
                          : 'Details'}
                      </span>
                      {expandedProviders.has(provider.provider_id) ? (
                        <ChevronUp className="w-3.5 h-3.5" />
                      ) : (
                        <ChevronDown className="w-3.5 h-3.5" />
                      )}
                    </button>
                  </div>

                  {/* Last Checked */}
                  <div className="flex items-center justify-center gap-1 text-xs text-gray-400 dark:text-gray-500">
                    <Clock className="w-3 h-3" />
                    <span>{formatTimestamp(provider.checked_at)}</span>
                  </div>

                  {/* Model Details (Expanded) */}
                  {expandedProviders.has(provider.provider_id) && (
                    <div className="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700 space-y-2">
                      <h4 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-2">
                        Model Results
                      </h4>
                      {checkingProviders.has(provider.provider_id) ? (
                        <div className="flex items-center justify-center p-6 bg-gray-50 dark:bg-gray-900 rounded-lg">
                          <Loader2 className="w-5 h-5 animate-spin text-blue-500 mr-2" />
                          <span className="text-sm text-gray-600 dark:text-gray-400">
                            Checking...
                          </span>
                        </div>
                      ) : (
                        <div className="space-y-1.5">
                          {provider.models.map((model, idx) => (
                            <div
                              key={idx}
                              className="flex items-center justify-between p-2.5 bg-gray-50 dark:bg-gray-900 rounded-lg group"
                            >
                              <div className="flex items-center gap-2 min-w-0 flex-1">
                                {getStatusIcon(model.status, 'w-4 h-4')}
                                <span className="text-sm text-gray-900 dark:text-gray-100 truncate">
                                  {model.model}
                                </span>
                              </div>
                              <div className="flex items-center gap-2 shrink-0">
                                {model.response_time_ms !== null && (
                                  <span
                                    className={`text-xs font-mono ${getResponseTimeColor(model.response_time_ms)}`}
                                  >
                                    {formatResponseTime(model.response_time_ms)}
                                  </span>
                                )}
                                <button
                                  onClick={e => {
                                    e.stopPropagation();
                                    handleCheckModelHealth(
                                      provider.provider_id,
                                      model.model
                                    );
                                  }}
                                  disabled={checkingModels.has(
                                    `${provider.provider_id}-${model.model}`
                                  )}
                                  className="opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:bg-gray-200 dark:hover:bg-gray-700 rounded"
                                  title={`Check ${model.model}`}
                                >
                                  <RefreshCw
                                    className={`w-3 h-3 text-gray-500 ${checkingModels.has(`${provider.provider_id}-${model.model}`) ? 'animate-spin' : ''}`}
                                  />
                                </button>
                              </div>
                              {model.error && (
                                <p className="text-xs text-red-600 dark:text-red-400 mt-1 w-full pl-6">
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
