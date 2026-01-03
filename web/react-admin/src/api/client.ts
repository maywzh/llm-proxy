import type {
  Provider,
  ProviderCreate,
  ProviderUpdate,
  ProviderListResponse,
  ProviderCreateResponse,
  MasterKey,
  MasterKeyCreate,
  MasterKeyUpdate,
  MasterKeyListResponse,
  MasterKeyCreateResponse,
  MasterKeyRotateResponse,
  ConfigVersionResponse,
  ConfigReloadResponse,
  UpdateResponse,
  StatusUpdateResponse,
  HealthResponse,
  AuthValidateResponse,
  ApiError,
} from '../types';

export class ApiClient {
  private baseUrl: string;
  private apiKey: string;

  constructor(baseUrl: string, apiKey: string) {
    this.baseUrl = baseUrl.replace(/\/$/, ''); // Remove trailing slash
    this.apiKey = apiKey;
  }

  private async request<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;

    const response = await fetch(url, {
      ...options,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${this.apiKey}`,
        ...options.headers,
      },
    });

    if (!response.ok) {
      let errorMessage = `HTTP ${response.status}: ${response.statusText}`;

      try {
        const errorData: ApiError = await response.json();
        errorMessage = errorData.detail || errorMessage;
      } catch {
        // If we can't parse the error response, use the default message
      }

      throw new Error(errorMessage);
    }

    // Handle 204 No Content responses
    if (response.status === 204) {
      return {} as T;
    }

    return response.json();
  }

  // Admin Key Validation - dedicated endpoint for UI login
  async validateAdminKey(): Promise<AuthValidateResponse> {
    return this.request<AuthValidateResponse>('/admin/v1/auth/validate', {
      method: 'POST',
    });
  }

  // Health Check - use config version endpoint since health endpoint doesn't exist in Rust server
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

  // Provider Management
  async listProviders(): Promise<ProviderListResponse> {
    return this.request<ProviderListResponse>('/admin/v1/providers');
  }

  async getProvider(id: string): Promise<Provider> {
    return this.request<Provider>(
      `/admin/v1/providers/${encodeURIComponent(id)}`
    );
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
    id: string,
    update: ProviderUpdate
  ): Promise<UpdateResponse> {
    return this.request<UpdateResponse>(
      `/admin/v1/providers/${encodeURIComponent(id)}`,
      {
        method: 'PUT',
        body: JSON.stringify(update),
      }
    );
  }

  async deleteProvider(id: string): Promise<void> {
    await this.request<void>(`/admin/v1/providers/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  }

  async setProviderStatus(
    id: string,
    enabled: boolean
  ): Promise<StatusUpdateResponse> {
    return this.request<StatusUpdateResponse>(
      `/admin/v1/providers/${encodeURIComponent(id)}/status`,
      {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      }
    );
  }

  // Master Key Management
  async listMasterKeys(): Promise<MasterKeyListResponse> {
    return this.request<MasterKeyListResponse>('/admin/v1/master-keys');
  }

  async getMasterKey(id: string): Promise<MasterKey> {
    return this.request<MasterKey>(
      `/admin/v1/master-keys/${encodeURIComponent(id)}`
    );
  }

  async createMasterKey(
    key: MasterKeyCreate
  ): Promise<MasterKeyCreateResponse> {
    return this.request<MasterKeyCreateResponse>('/admin/v1/master-keys', {
      method: 'POST',
      body: JSON.stringify(key),
    });
  }

  async updateMasterKey(
    id: string,
    update: MasterKeyUpdate
  ): Promise<UpdateResponse> {
    return this.request<UpdateResponse>(
      `/admin/v1/master-keys/${encodeURIComponent(id)}`,
      {
        method: 'PUT',
        body: JSON.stringify(update),
      }
    );
  }

  async deleteMasterKey(id: string): Promise<void> {
    await this.request<void>(
      `/admin/v1/master-keys/${encodeURIComponent(id)}`,
      {
        method: 'DELETE',
      }
    );
  }

  async setMasterKeyStatus(
    id: string,
    enabled: boolean
  ): Promise<StatusUpdateResponse> {
    return this.request<StatusUpdateResponse>(
      `/admin/v1/master-keys/${encodeURIComponent(id)}/status`,
      {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      }
    );
  }

  async rotateMasterKey(id: string): Promise<MasterKeyRotateResponse> {
    return this.request<MasterKeyRotateResponse>(
      `/admin/v1/master-keys/${encodeURIComponent(id)}/rotate`,
      {
        method: 'POST',
      }
    );
  }

  // Configuration Management
  async getConfigVersion(): Promise<ConfigVersionResponse> {
    return this.request<ConfigVersionResponse>('/admin/v1/config/version');
  }

  async reloadConfig(): Promise<ConfigReloadResponse> {
    return this.request<ConfigReloadResponse>('/admin/v1/config/reload', {
      method: 'POST',
    });
  }
}

// Utility function to generate random API keys
export function generateApiKey(): string {
  const chars =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  let result = 'sk-';
  for (let i = 0; i < 48; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return result;
}

// Utility function to validate API key format
export function isValidApiKey(key: string): boolean {
  return /^sk-[A-Za-z0-9]{48}$/.test(key);
}

// Utility function to validate URL format
export function isValidUrl(url: string): boolean {
  try {
    new URL(url);
    return true;
  } catch {
    return false;
  }
}
