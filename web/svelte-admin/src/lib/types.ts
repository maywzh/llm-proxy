// API Response Types
export interface ApiError {
  detail: string;
}

// Provider Types
export interface Provider {
  id: number;
  provider_key: string;
  provider_type: string;
  api_base: string;
  model_mapping: Record<string, string>;
  is_enabled: boolean;
}

export interface ProviderCreate {
  provider_key: string;
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

// Credential Types
export interface Credential {
  id: number;
  name: string;
  key_preview: string;
  allowed_models: string[];
  rate_limit: number | null;
  is_enabled: boolean;
}

export interface CredentialCreate {
  key: string;
  name: string;
  allowed_models?: string[];
  rate_limit?: number | null;
}

export interface CredentialUpdate {
  key?: string;
  name?: string;
  allowed_models?: string[];
  rate_limit?: number | null;
  is_enabled?: boolean;
}

export interface CredentialListResponse {
  version: number;
  credentials: Credential[];
}

export interface CredentialCreateResponse {
  version: number;
  credential: Credential;
}

export interface CredentialRotateResponse {
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
  credentials_count: number;
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
  credentials: boolean;
  config: boolean;
}

export interface ErrorState {
  providers: string | null;
  credentials: string | null;
  config: string | null;
  general: string | null;
}

// Form Types
export interface ProviderFormData {
  provider_key: string;
  provider_type: string;
  api_base: string;
  api_key: string;
  model_mapping: Record<string, string>;
  is_enabled: boolean;
}

export interface CredentialFormData {
  key: string;
  name: string;
  allowed_models: string[];
  rate_limit: number | null;
  is_enabled: boolean;
}

// Modal Types
export type ModalType =
  | 'provider-create'
  | 'provider-edit'
  | 'provider-delete'
  | 'credential-create'
  | 'credential-edit'
  | 'credential-delete'
  | 'credential-rotate'
  | null;

export interface ModalState {
  type: ModalType;
  data?: any;
  isOpen: boolean;
}
