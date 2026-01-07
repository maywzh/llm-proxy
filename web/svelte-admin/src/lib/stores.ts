import { writable, derived, get } from 'svelte/store';
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
  CredentialFormData,
} from './types';

const API_BASE_URL =
  import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

// Auth Store
function createAuthStore() {
  const defaultAuth: AuthState = {
    isAuthenticated: false,
    apiKey: '',
  };

  const { subscribe, set } = writable<AuthState>(defaultAuth);

  return {
    subscribe,
    login: (apiKey: string) => {
      const authState = { isAuthenticated: true, apiKey };
      set(authState);

      // Save to localStorage
      if (browser) {
        localStorage.setItem('admin-auth', JSON.stringify(authState));
      }
    },
    logout: () => {
      set(defaultAuth);

      // Clear localStorage
      if (browser) {
        localStorage.removeItem('admin-auth');
      }
    },
    init: () => {
      // Load from localStorage on init
      if (browser) {
        const stored = localStorage.getItem('admin-auth');
        if (stored) {
          try {
            const authState = JSON.parse(stored);
            if (authState.isAuthenticated && authState.apiKey) {
              set(authState);
            }
          } catch (e) {
            console.error('Failed to parse stored auth:', e);
          }
        }
      }
    },
  };
}

export const auth = createAuthStore();

// API Client Store (derived from auth)
export const apiClient = derived(auth, $auth => {
  if ($auth.isAuthenticated) {
    return new ApiClient(API_BASE_URL, $auth.apiKey);
  }
  return null;
});

// Loading States
export const loading = writable<LoadingState>({
  providers: false,
  credentials: false,
  config: false,
});

// Error States
export const errors = writable<ErrorState>({
  providers: null,
  credentials: null,
  config: null,
  general: null,
});

// Data Stores
export const providers = writable<Provider[]>([]);
export const credentials = writable<Credential[]>([]);
export const configVersion = writable<ConfigVersionResponse | null>(null);

// Modal Store
export const modal = writable<ModalState>({
  type: null,
  data: undefined,
  isOpen: false,
});

// Form Data Stores
export const providerForm = writable<Partial<ProviderFormData>>({});
export const credentialForm = writable<Partial<CredentialFormData>>({});

// Actions
export const actions = {
  // Provider Actions
  async loadProviders() {
    const client = get(apiClient);
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
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      const response = await client.createProvider({
        provider_key: data.provider_key,
        provider_type: data.provider_type,
        api_base: data.api_base,
        api_key: data.api_key,
        model_mapping: data.model_mapping,
      });

      // Add to providers list
      providers.update(list => [...list, response.provider]);
      configVersion.update(current =>
        current ? { ...current, version: response.version } : null
      );

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

  async updateProvider(id: number, data: Partial<ProviderFormData>) {
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      await client.updateProvider(id, data);

      // Reload providers to get updated data
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
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, providers: true }));
    errors.update(state => ({ ...state, providers: null }));

    try {
      await client.deleteProvider(id);

      // Remove from providers list
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
    const client = get(apiClient);
    if (!client) return false;

    try {
      await client.setProviderStatus(id, enabled);

      // Update provider in list
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

  // Credential Actions
  async loadCredentials() {
    const client = get(apiClient);
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
    const client = get(apiClient);
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

      // Add to credentials list
      credentials.update(list => [...list, response.credential]);
      configVersion.update(current =>
        current ? { ...current, version: response.version } : null
      );

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
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      await client.updateCredential(id, data);

      // Reload credentials to get updated data
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
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      await client.deleteCredential(id);

      // Remove from credentials list
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
    const client = get(apiClient);
    if (!client) return false;

    try {
      await client.setCredentialStatus(id, enabled);

      // Update credential in list
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
    const client = get(apiClient);
    if (!client) return null;

    loading.update(state => ({ ...state, credentials: true }));
    errors.update(state => ({ ...state, credentials: null }));

    try {
      const response = await client.rotateCredential(id);

      // Reload credentials to get updated data
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

  // Config Actions
  async loadConfigVersion() {
    const client = get(apiClient);
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
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, config: true }));
    errors.update(state => ({ ...state, config: null }));

    try {
      const response = await client.reloadConfig();
      configVersion.set({
        version: response.version,
        timestamp: response.timestamp,
      });

      // Reload all data
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

  // Modal Actions
  openModal(type: ModalState['type'], data?: any) {
    modal.set({ type, data, isOpen: true });
  },

  closeModal() {
    modal.set({ type: null, data: undefined, isOpen: false });
  },

  // Error Actions
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
