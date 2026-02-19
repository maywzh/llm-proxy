import { writable } from 'svelte/store';
import { browser } from '$app/environment';
import { ApiClient } from './api';
import type {
  AuthState,
  LoadingState,
  ErrorState,
  Provider,
  Credential,
  ConfigVersionResponse,
  ModalState,
  ProviderFormData,
  ProviderUpdate,
  CredentialFormData,
} from './types';

const API_BASE_URL =
  import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

function createAuthStore() {
  const defaultAuth: AuthState = {
    isAuthenticated: false,
    apiKey: '',
  };

  const { subscribe, set } = writable<AuthState>(defaultAuth);
  let currentClient: ApiClient | null = null;
  let currentClientApiKey: string | null = null;
  let currentState: AuthState = defaultAuth;

  subscribe(state => {
    currentState = state;
  });

  return {
    subscribe,
    get apiClient(): ApiClient | null {
      if (currentState.isAuthenticated) {
        if (!currentClient || currentClientApiKey !== currentState.apiKey) {
          currentClient = new ApiClient(API_BASE_URL, currentState.apiKey);
          currentClientApiKey = currentState.apiKey;
        }
        return currentClient;
      }
      return null;
    },
    login: (apiKey: string) => {
      const authState = { isAuthenticated: true, apiKey };
      set(authState);
      currentClient = new ApiClient(API_BASE_URL, apiKey);
      currentClientApiKey = apiKey;

      if (browser) {
        localStorage.setItem('admin-auth', JSON.stringify(authState));
      }
    },
    logout: () => {
      set(defaultAuth);
      currentClient = null;
      currentClientApiKey = null;

      if (browser) {
        localStorage.removeItem('admin-auth');
      }
    },
    init: () => {
      if (browser) {
        const stored = localStorage.getItem('admin-auth');
        if (stored) {
          try {
            const authState = JSON.parse(stored);
            if (authState.isAuthenticated && authState.apiKey) {
              set(authState);
              currentClient = new ApiClient(API_BASE_URL, authState.apiKey);
              currentClientApiKey = authState.apiKey;
            }
          } catch {
            // ignore
          }
        }
      }
    },
  };
}

export const auth = createAuthStore();

export const loading = writable<LoadingState>({
  providers: false,
  credentials: false,
  config: false,
});

export const errors = writable<ErrorState>({
  providers: null,
  credentials: null,
  config: null,
  general: null,
});

export const providers = writable<Provider[]>([]);
export const credentials = writable<Credential[]>([]);
export const configVersion = writable<ConfigVersionResponse | null>(null);

export const modal = writable<ModalState>({
  type: null,
  data: undefined,
  isOpen: false,
});

export const providerForm = writable<Partial<ProviderFormData>>({});
export const credentialForm = writable<Partial<CredentialFormData>>({});

export const actions = {
  async loadProviders() {
    const client = auth.apiClient;
    if (!client) return;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      const response = await client.listProviders();
      providers.set(response.providers);
      configVersion.update(current =>
        current ? { ...current, version: response.version } : null
      );
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to load providers';
      errors.update(state => ({ ...state, providers: message }));
    } finally {
      loading.update(state => ({ ...state, providers: false }));
    }
  },

  async createProvider(data: ProviderFormData) {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      const createData: Parameters<typeof client.createProvider>[0] = {
        provider_key: data.provider_key,
        provider_type: data.provider_type,
        api_base: data.api_base,
        api_key: data.api_key,
        model_mapping: data.model_mapping,
      };

      // Include provider_params for GCP Vertex
      if (data.provider_type === 'gcp-vertex') {
        const params: Record<string, unknown> = {
          gcp_project: data.gcp_project,
          gcp_location: data.gcp_location?.trim() || 'us-central1',
          gcp_publisher: data.gcp_publisher?.trim() || 'anthropic',
        };
        if (data.gcp_blocking_action?.trim() || data.gcp_streaming_action?.trim()) {
          params.gcp_vertex_actions = {
            blocking: data.gcp_blocking_action?.trim() || 'rawPredict',
            streaming: data.gcp_streaming_action?.trim() || 'streamRawPredict',
          };
        }
        createData.provider_params = params;
      } else {
        createData.provider_params = {};
      }

      const response = await client.createProvider(createData);

      // Backend returns Provider directly (not wrapped in { version, provider })
      providers.update(list => [...list, response]);

      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to create provider';
      errors.update(state => ({ ...state, providers: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, providers: false }));
    }
  },

  async updateProvider(id: number, data: ProviderUpdate) {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      await client.updateProvider(id, data);
      await actions.loadProviders();
      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to update provider';
      errors.update(state => ({ ...state, providers: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, providers: false }));
    }
  },

  async deleteProvider(id: number) {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      await client.deleteProvider(id);
      providers.update(list => list.filter(p => p.id !== id));
      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to delete provider';
      errors.update(state => ({ ...state, providers: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, providers: false }));
    }
  },

  async toggleProviderStatus(id: number, enabled: boolean) {
    const client = auth.apiClient;
    if (!client) return false;

    try {
      await client.setProviderStatus(id, enabled);
      providers.update(list =>
        list.map(p => (p.id === id ? { ...p, is_enabled: enabled } : p))
      );
      return true;
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : 'Failed to update provider status';
      errors.update(state => ({ ...state, providers: message }));
      return false;
    }
  },

  async loadCredentials() {
    const client = auth.apiClient;
    if (!client) return;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      const response = await client.listCredentials();
      credentials.set(response.credentials);
      configVersion.update(current =>
        current ? { ...current, version: response.version } : null
      );
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to load credentials';
      errors.update(state => ({ ...state, credentials: message }));
    } finally {
      loading.update(state => ({ ...state, credentials: false }));
    }
  },

  async createCredential(data: CredentialFormData) {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      const response = await client.createCredential({
        key: data.key,
        name: data.name,
        allowed_models: data.allowed_models,
        rate_limit: data.rate_limit,
      });

      // Backend returns Credential directly (not wrapped in { version, credential })
      credentials.update(list => [...list, response]);

      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to create credential';
      errors.update(state => ({ ...state, credentials: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, credentials: false }));
    }
  },

  async updateCredential(id: number, data: Partial<CredentialFormData>) {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      await client.updateCredential(id, data);
      await actions.loadCredentials();
      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to update credential';
      errors.update(state => ({ ...state, credentials: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, credentials: false }));
    }
  },

  async deleteCredential(id: number) {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      await client.deleteCredential(id);
      credentials.update(list => list.filter(k => k.id !== id));
      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to delete credential';
      errors.update(state => ({ ...state, credentials: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, credentials: false }));
    }
  },

  async toggleCredentialStatus(id: number, enabled: boolean) {
    const client = auth.apiClient;
    if (!client) return false;

    try {
      await client.setCredentialStatus(id, enabled);
      credentials.update(list =>
        list.map(k => (k.id === id ? { ...k, is_enabled: enabled } : k))
      );
      return true;
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : 'Failed to update credential status';
      errors.update(state => ({ ...state, credentials: message }));
      return false;
    }
  },

  async rotateCredential(id: number) {
    const client = auth.apiClient;
    if (!client) return null;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      const response = await client.rotateCredential(id);
      await actions.loadCredentials();
      return response.new_key;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to rotate credential';
      errors.update(state => ({ ...state, credentials: message }));
      return null;
    } finally {
      loading.update(state => ({ ...state, credentials: false }));
    }
  },

  async loadConfigVersion() {
    const client = auth.apiClient;
    if (!client) return;

    loading.update(state => ({ ...state, config: true }));
    errors.update(state => ({ ...state, config: null }));

    try {
      const response = await client.getConfigVersion();
      configVersion.set(response);
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : 'Failed to load config version';
      errors.update(state => ({ ...state, config: message }));
    } finally {
      loading.update(state => ({ ...state, config: false }));
    }
  },

  async reloadConfig() {
    const client = auth.apiClient;
    if (!client) return false;

    loading.update(state => ({ ...state, config: true }));
    errors.update(state => ({ ...state, config: null }));

    try {
      const response = await client.reloadConfig();
      configVersion.set({
        version: response.version,
        timestamp: response.timestamp,
      });

      await Promise.all([actions.loadProviders(), actions.loadCredentials()]);

      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to reload config';
      errors.update(state => ({ ...state, config: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, config: false }));
    }
  },

  openModal(type: ModalState['type'], data?: unknown) {
    modal.set({ type, data, isOpen: true });
  },

  closeModal() {
    modal.set({ type: null, data: undefined, isOpen: false });
  },

  clearError(type: keyof ErrorState) {
    errors.update(state => ({ ...state, [type]: null }));
  },

  clearAllErrors() {
    errors.set({
      providers: null,
      credentials: null,
      config: null,
      general: null,
    });
  },
};
