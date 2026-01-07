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

  // Credential Management
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
