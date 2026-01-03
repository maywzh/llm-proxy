import { writable, derived, get } from 'svelte/store';
import { browser } from '$app/environment';
import { ApiClient } from './api';
import type {
  AuthState,
  LoadingState,
  ErrorState,
  Provider,
  MasterKey,
  ConfigVersionResponse,
  ModalState,
  ProviderFormData,
  MasterKeyFormData,
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
  masterKeys: false,
  config: false,
});

// Error States
export const errors = writable<ErrorState>({
  providers: null,
  masterKeys: null,
  config: null,
  general: null,
});

// Data Stores
export const providers = writable<Provider[]>([]);
export const masterKeys = writable<MasterKey[]>([]);
export const configVersion = writable<ConfigVersionResponse | null>(null);

// Modal Store
export const modal = writable<ModalState>({
  type: null,
  data: undefined,
  isOpen: false,
});

// Form Data Stores
export const providerForm = writable<Partial<ProviderFormData>>({});
export const masterKeyForm = writable<Partial<MasterKeyFormData>>({});

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
        id: data.id,
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

  async updateProvider(id: string, data: Partial<ProviderFormData>) {
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

  async deleteProvider(id: string) {
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

  async toggleProviderStatus(id: string, enabled: boolean) {
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

  // Master Key Actions
  async loadMasterKeys() {
    const client = get(apiClient);
    if (!client) return;

    loading.update(state => ({ ...state, masterKeys: true }));
    errors.update(state => ({ ...state, masterKeys: null }));

    try {
      const response = await client.listMasterKeys();
      masterKeys.set(response.keys);
      configVersion.update(current =>
        current ? { ...current, version: response.version } : null
      );
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to load master keys';
      errors.update(state => ({ ...state, masterKeys: message }));
    } finally {
      loading.update(state => ({ ...state, masterKeys: false }));
    }
  },

  async createMasterKey(data: MasterKeyFormData) {
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, masterKeys: true }));
    errors.update(state => ({ ...state, masterKeys: null }));

    try {
      const response = await client.createMasterKey({
        id: data.id,
        key: data.key,
        name: data.name,
        allowed_models: data.allowed_models,
        rate_limit: data.rate_limit,
      });

      // Add to master keys list
      masterKeys.update(list => [...list, response.key]);
      configVersion.update(current =>
        current ? { ...current, version: response.version } : null
      );

      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to create master key';
      errors.update(state => ({ ...state, masterKeys: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, masterKeys: false }));
    }
  },

  async updateMasterKey(id: string, data: Partial<MasterKeyFormData>) {
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, masterKeys: true }));
    errors.update(state => ({ ...state, masterKeys: null }));

    try {
      await client.updateMasterKey(id, data);

      // Reload master keys to get updated data
      await actions.loadMasterKeys();

      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to update master key';
      errors.update(state => ({ ...state, masterKeys: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, masterKeys: false }));
    }
  },

  async deleteMasterKey(id: string) {
    const client = get(apiClient);
    if (!client) return false;

    loading.update(state => ({ ...state, masterKeys: true }));
    errors.update(state => ({ ...state, masterKeys: null }));

    try {
      await client.deleteMasterKey(id);

      // Remove from master keys list
      masterKeys.update(list => list.filter(k => k.id !== id));

      return true;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to delete master key';
      errors.update(state => ({ ...state, masterKeys: message }));
      return false;
    } finally {
      loading.update(state => ({ ...state, masterKeys: false }));
    }
  },

  async toggleMasterKeyStatus(id: string, enabled: boolean) {
    const client = get(apiClient);
    if (!client) return false;

    try {
      await client.setMasterKeyStatus(id, enabled);

      // Update master key in list
      masterKeys.update(list =>
        list.map(k => (k.id === id ? { ...k, is_enabled: enabled } : k))
      );

      return true;
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : 'Failed to update master key status';
      errors.update(state => ({ ...state, masterKeys: message }));
      return false;
    }
  },

  async rotateMasterKey(id: string) {
    const client = get(apiClient);
    if (!client) return null;

    loading.update(state => ({ ...state, masterKeys: true }));
    errors.update(state => ({ ...state, masterKeys: null }));

    try {
      const response = await client.rotateMasterKey(id);

      // Reload master keys to get updated data
      await actions.loadMasterKeys();

      return response.new_key;
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to rotate master key';
      errors.update(state => ({ ...state, masterKeys: message }));
      return null;
    } finally {
      loading.update(state => ({ ...state, masterKeys: false }));
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
      await Promise.all([actions.loadProviders(), actions.loadMasterKeys()]);

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
      masterKeys: null,
      config: null,
      general: null,
    });
  },
};
