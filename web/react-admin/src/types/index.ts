// API Response Types
export interface ApiError {
  detail: string;
}

// Provider Types
export interface Provider {
  id: string;
  provider_type: string;
  api_base: string;
  model_mapping: Record<string, string>;
  is_enabled: boolean;
}

export interface ProviderCreate {
  id: string;
  provider_type: string;
  api_base: string;
  api_key: string;
  model_mapping?: Record<string, string>;
}

export interface ProviderUpdate {
  provider_type?: string;
  api_base?: string;
  api_key?: string;
  model_mapping?: Record<string, string>;
  is_enabled?: boolean;
}

export interface ProviderListResponse {
  version: number;
  providers: Provider[];
}

export interface ProviderCreateResponse {
  version: number;
  provider: Provider;
}

// Master Key Types
export interface MasterKey {
  id: string;
  name: string;
  key_preview: string;
  allowed_models: string[];
  rate_limit: number | null;
  is_enabled: boolean;
}

export interface MasterKeyCreate {
  id: string;
  key: string;
  name: string;
  allowed_models?: string[];
  rate_limit?: number | null;
}

export interface MasterKeyUpdate {
  name?: string;
  allowed_models?: string[];
  rate_limit?: number | null;
  is_enabled?: boolean;
}

export interface MasterKeyListResponse {
  version: number;
  keys: MasterKey[];
}

export interface MasterKeyCreateResponse {
  version: number;
  key: MasterKey;
}

export interface MasterKeyRotateResponse {
  version: number;
  new_key: string;
  message: string;
}

// Config Types
export interface ConfigVersionResponse {
  version: number;
  timestamp: string;
}

export interface ConfigReloadResponse {
  version: number;
  timestamp: string;
  providers_count: number;
  master_keys_count: number;
}

// Generic Response Types
export interface UpdateResponse {
  version: number;
  status: string;
}

export interface StatusUpdateResponse {
  version: number;
  is_enabled: boolean;
}

export interface HealthResponse {
  status: string;
  database_configured: boolean;
  admin_key_configured: boolean;
  config_loaded: boolean;
  config_version: number;
}

export interface AuthValidateResponse {
  valid: boolean;
  message: string;
}

// UI State Types
export interface AuthState {
  isAuthenticated: boolean;
  apiKey: string;
}

export interface LoadingState {
  providers: boolean;
  masterKeys: boolean;
  config: boolean;
}

export interface ErrorState {
  providers: string | null;
  masterKeys: string | null;
  config: string | null;
  general: string | null;
}

// Form Types
export interface ProviderFormData {
  id: string;
  provider_type: string;
  api_base: string;
  api_key: string;
  model_mapping: Record<string, string>;
  is_enabled: boolean;
}

export interface MasterKeyFormData {
  id: string;
  key: string;
  name: string;
  allowed_models: string[];
  rate_limit: number | null;
  is_enabled: boolean;
}
