import type {
  Provider,
  ProviderCreate,
  ProviderUpdate,
  ProviderListResponse,
  ProviderCreateResponse,
  Credential,
  CredentialCreate,
  CredentialUpdate,
  CredentialListResponse,
  CredentialCreateResponse,
  CredentialRotateResponse,
  ConfigVersionResponse,
  ConfigReloadResponse,
  UpdateResponse,
  StatusUpdateResponse,
  HealthResponse,
  AuthValidateResponse,
  ApiError,
  ChatRequest,
  ChatResponse,
  StreamChunk,
  ModelsResponse,
  HealthCheckRequest,
  HealthCheckResponse,
  ProviderHealthStatus,
  CheckProviderHealthRequest,
  CheckProviderHealthResponse,
  RequestLogListResponse,
  RequestLogDetail,
  RequestLogStats,
  RequestLogFilters,
  ErrorLogListResponse,
  ErrorLogDetail,
  ErrorLogFilters,
} from './types';

export class ApiClient {
  private baseUrl: string;
  private apiKey: string;

  constructor(baseUrl: string, apiKey: string) {
    this.baseUrl = baseUrl.replace(/\/$/, '');
    this.apiKey = apiKey;
  }

  private async request<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    return this.requestWithApiKey<T>(this.apiKey, endpoint, options);
  }

  private async requestWithApiKey<T>(
    apiKey: string,
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;

    const response = await fetch(url, {
      ...options,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${apiKey}`,
        ...options.headers,
      },
    });

    if (!response.ok) {
      let errorMessage = `HTTP ${response.status}: ${response.statusText}`;

      try {
        const errorData: ApiError = await response.json();
        errorMessage = errorData.detail || errorMessage;
      } catch {
        // ignore
      }

      throw new Error(errorMessage);
    }

    if (response.status === 204) {
      return {} as T;
    }

    return response.json();
  }

  async validateAdminKey(): Promise<AuthValidateResponse> {
    return this.request<AuthValidateResponse>('/admin/v1/auth/validate', {
      method: 'POST',
    });
  }

  async health(): Promise<HealthResponse> {
    const config = await this.request<{ version: number; timestamp: string }>(
      '/admin/v1/config/version'
    );
    return {
      status: 'ok',
      database_configured: true,
      admin_key_configured: true,
      config_loaded: true,
      config_version: config.version,
    };
  }

  async listProviders(): Promise<ProviderListResponse> {
    return this.request<ProviderListResponse>('/admin/v1/providers');
  }

  async getProvider(id: number): Promise<Provider> {
    return this.request<Provider>(`/admin/v1/providers/${id}`);
  }

  async createProvider(
    provider: ProviderCreate
  ): Promise<ProviderCreateResponse> {
    return this.request<ProviderCreateResponse>('/admin/v1/providers', {
      method: 'POST',
      body: JSON.stringify(provider),
    });
  }

  async updateProvider(
    id: number,
    update: ProviderUpdate
  ): Promise<UpdateResponse> {
    return this.request<UpdateResponse>(`/admin/v1/providers/${id}`, {
      method: 'PUT',
      body: JSON.stringify(update),
    });
  }

  async deleteProvider(id: number): Promise<void> {
    await this.request<void>(`/admin/v1/providers/${id}`, {
      method: 'DELETE',
    });
  }

  async setProviderStatus(
    id: number,
    enabled: boolean
  ): Promise<StatusUpdateResponse> {
    return this.request<StatusUpdateResponse>(
      `/admin/v1/providers/${id}/status`,
      {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      }
    );
  }

  async listCredentials(): Promise<CredentialListResponse> {
    return this.request<CredentialListResponse>('/admin/v1/credentials');
  }

  async getCredential(id: number): Promise<Credential> {
    return this.request<Credential>(`/admin/v1/credentials/${id}`);
  }

  async createCredential(
    credential: CredentialCreate
  ): Promise<CredentialCreateResponse> {
    return this.request<CredentialCreateResponse>('/admin/v1/credentials', {
      method: 'POST',
      body: JSON.stringify(credential),
    });
  }

  async updateCredential(
    id: number,
    update: CredentialUpdate
  ): Promise<UpdateResponse> {
    return this.request<UpdateResponse>(`/admin/v1/credentials/${id}`, {
      method: 'PUT',
      body: JSON.stringify(update),
    });
  }

  async deleteCredential(id: number): Promise<void> {
    await this.request<void>(`/admin/v1/credentials/${id}`, {
      method: 'DELETE',
    });
  }

  async setCredentialStatus(
    id: number,
    enabled: boolean
  ): Promise<StatusUpdateResponse> {
    return this.request<StatusUpdateResponse>(
      `/admin/v1/credentials/${id}/status`,
      {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      }
    );
  }

  async rotateCredential(id: number): Promise<CredentialRotateResponse> {
    return this.request<CredentialRotateResponse>(
      `/admin/v1/credentials/${id}/rotate`,
      {
        method: 'POST',
      }
    );
  }

  async getConfigVersion(): Promise<ConfigVersionResponse> {
    return this.request<ConfigVersionResponse>('/admin/v1/config/version');
  }

  async reloadConfig(): Promise<ConfigReloadResponse> {
    return this.request<ConfigReloadResponse>('/admin/v1/config/reload', {
      method: 'POST',
    });
  }

  // Health Check API
  async checkProvidersHealth(
    request?: HealthCheckRequest
  ): Promise<HealthCheckResponse> {
    return this.request<HealthCheckResponse>('/admin/v1/health/check', {
      method: 'POST',
      body: JSON.stringify(request || {}),
    });
  }

  async getProviderHealth(
    providerId: number,
    models?: string[],
    timeoutSecs?: number
  ): Promise<ProviderHealthStatus> {
    const params = new URLSearchParams();
    if (models && models.length > 0) {
      params.append('models', models.join(','));
    }
    if (timeoutSecs !== undefined) {
      params.append('timeout_secs', timeoutSecs.toString());
    }
    const queryString = params.toString();
    const endpoint = `/admin/v1/health/providers/${providerId}${queryString ? `?${queryString}` : ''}`;
    return this.request<ProviderHealthStatus>(endpoint);
  }

  async checkProviderHealth(
    providerId: number,
    request?: CheckProviderHealthRequest
  ): Promise<CheckProviderHealthResponse> {
    return this.request<CheckProviderHealthResponse>(
      `/admin/v1/providers/${providerId}/health`,
      {
        method: 'POST',
        body: JSON.stringify(request || {}),
      }
    );
  }

  // Chat API
  async listModels(credentialKey: string): Promise<ModelsResponse> {
    return this.requestWithApiKey<ModelsResponse>(credentialKey, '/v1/models');
  }

  async createChatCompletion(
    request: ChatRequest,
    credentialKey: string
  ): Promise<ChatResponse> {
    return this.requestWithApiKey<ChatResponse>(
      credentialKey,
      '/v1/chat/completions',
      {
        method: 'POST',
        body: JSON.stringify({ ...request, stream: false }),
      }
    );
  }

  async createChatCompletionStream(
    request: ChatRequest,
    credentialKey: string,
    onChunk: (chunk: StreamChunk) => void,
    onComplete: () => void,
    onError: (error: Error) => void
  ): Promise<() => void> {
    const url = `${this.baseUrl}/v1/chat/completions`;

    try {
      const abortController = new AbortController();
      const response = await fetch(url, {
        method: 'POST',
        signal: abortController.signal,
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${credentialKey}`,
        },
        body: JSON.stringify({ ...request, stream: true }),
      });

      if (!response.ok) {
        let errorMessage = `HTTP ${response.status}: ${response.statusText}`;
        try {
          const errorData: ApiError = await response.json();
          errorMessage = errorData.detail || errorMessage;
        } catch {
          // ignore
        }
        throw new Error(errorMessage);
      }

      if (!response.body) {
        throw new Error('Response body is null');
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let stopped = false;
      let buffer = '';

      const read = async () => {
        try {
          while (!stopped) {
            const { done, value } = await reader.read();
            if (done) {
              onComplete();
              break;
            }

            buffer += decoder.decode(value, { stream: true });
            while (true) {
              const newlineIndex = buffer.indexOf('\n');
              if (newlineIndex === -1) break;

              const rawLine = buffer.slice(0, newlineIndex);
              buffer = buffer.slice(newlineIndex + 1);

              const line = rawLine.trim();
              if (!line.startsWith('data:')) continue;

              const data = line.slice(5).trim();
              if (!data) continue;
              if (data === '[DONE]') {
                onComplete();
                return;
              }

              try {
                const parsed: StreamChunk = JSON.parse(data);
                onChunk(parsed);
              } catch {
                // ignore
              }
            }
          }
        } catch (error) {
          if (!stopped) {
            onError(error instanceof Error ? error : new Error(String(error)));
          }
        }
      };

      read();

      return () => {
        stopped = true;
        abortController.abort();
        reader.cancel().catch(() => {});
      };
    } catch (error) {
      onError(error instanceof Error ? error : new Error(String(error)));
      return () => {};
    }
  }

  // Request Logs API
  async listLogs(
    page: number = 1,
    pageSize: number = 50,
    filters?: RequestLogFilters
  ): Promise<RequestLogListResponse> {
    const params = new URLSearchParams();
    params.append('page', page.toString());
    params.append('page_size', pageSize.toString());
    if (filters) {
      Object.entries(filters).forEach(([key, value]) => {
        if (value !== undefined && value !== null && value !== '') {
          params.append(key, String(value));
        }
      });
    }
    return this.request<RequestLogListResponse>(
      `/admin/v1/logs?${params.toString()}`
    );
  }

  async getLog(id: number): Promise<RequestLogDetail> {
    return this.request<RequestLogDetail>(`/admin/v1/logs/${id}`);
  }

  async getLogStats(filters?: {
    start_time?: string;
    end_time?: string;
    provider_name?: string;
    model?: string;
  }): Promise<RequestLogStats> {
    const params = new URLSearchParams();
    if (filters) {
      Object.entries(filters).forEach(([key, value]) => {
        if (value !== undefined && value !== null && value !== '') {
          params.append(key, String(value));
        }
      });
    }
    const qs = params.toString();
    return this.request<RequestLogStats>(
      `/admin/v1/logs/stats${qs ? `?${qs}` : ''}`
    );
  }

  async deleteLog(id: number): Promise<void> {
    await this.request<void>(`/admin/v1/logs/${id}`, { method: 'DELETE' });
  }

  async batchDeleteLogs(ids: number[]): Promise<{ deleted: number }> {
    return this.request<{ deleted: number }>('/admin/v1/logs/batch-delete', {
      method: 'POST',
      body: JSON.stringify({ ids }),
    });
  }

  // Error Logs API
  async listErrorLogs(
    page: number = 1,
    pageSize: number = 50,
    filters?: ErrorLogFilters
  ): Promise<ErrorLogListResponse> {
    const params = new URLSearchParams();
    params.append('page', page.toString());
    params.append('page_size', pageSize.toString());
    if (filters) {
      Object.entries(filters).forEach(([key, value]) => {
        if (value !== undefined && value !== null && value !== '') {
          params.append(key, String(value));
        }
      });
    }
    return this.request<ErrorLogListResponse>(
      `/admin/v1/error-logs?${params.toString()}`
    );
  }

  async getErrorLog(id: number): Promise<ErrorLogDetail> {
    return this.request<ErrorLogDetail>(`/admin/v1/error-logs/${id}`);
  }

  async batchDeleteErrorLogs(ids: number[]): Promise<{ deleted: number }> {
    return this.request<{ deleted: number }>(
      '/admin/v1/error-logs/batch-delete',
      {
        method: 'POST',
        body: JSON.stringify({ ids }),
      }
    );
  }

  async deleteErrorLog(id: number): Promise<void> {
    await this.request<void>(`/admin/v1/error-logs/${id}`, {
      method: 'DELETE',
    });
  }
}

export function generateApiKey(): string {
  const chars =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  let result = 'sk-';
  for (let i = 0; i < 48; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return result;
}

export function isValidApiKey(key: string): boolean {
  return /^sk-[A-Za-z0-9]{48}$/.test(key);
}

export function isValidUrl(url: string): boolean {
  try {
    new URL(url);
    return true;
  } catch {
    return false;
  }
}
