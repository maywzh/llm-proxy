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

// Chat Types
export type ChatContentPart =
  | { type: 'text'; text: string }
  | { type: 'image_url'; image_url: { url: string } };

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string | ChatContentPart[];
  thinking?: string;
  timestamp?: number;
}

export interface ChatRequestMessage {
  role: 'system' | 'user' | 'assistant';
  content: string | ChatContentPart[];
}

export interface ChatRequest {
  model: string;
  messages: ChatRequestMessage[];
  stream?: boolean;
  temperature?: number;
  max_tokens?: number;
  top_p?: number;
}

export interface ChatResponse {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: {
    index: number;
    message: {
      role: string;
      content: string;
    };
    finish_reason: string;
  }[];
  usage: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

export interface StreamChunk {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: {
    index: number;
    delta: {
      role?: string;
      content?: string;
      reasoning?: string;
      reasoning_content?: string;
      thinking?: string;
    };
    finish_reason: string | null;
  }[];
}

export interface Model {
  id: string;
  object: string;
  created: number;
  owned_by: string;
}

export interface ModelsResponse {
  object: string;
  data: Model[];
}

// Health Check Types
export type HealthStatus = 'healthy' | 'unhealthy' | 'disabled' | 'unknown';

export interface ModelHealthStatus {
  model: string;
  status: HealthStatus;
  response_time_ms: number | null;
  error: string | null;
}

export interface ProviderHealthStatus {
  provider_id: number;
  provider_key: string;
  status: HealthStatus;
  models: ModelHealthStatus[];
  avg_response_time_ms: number | null;
  checked_at: string;
}

export interface HealthCheckRequest {
  provider_ids?: number[];
  models?: string[];
  timeout_secs?: number;
  max_concurrent?: number;
}

export interface HealthCheckResponse {
  providers: ProviderHealthStatus[];
  total_providers: number;
  healthy_providers: number;
  unhealthy_providers: number;
}

// Single Provider Health Check Types
export interface CheckProviderHealthRequest {
  models?: string[];
  max_concurrent?: number;
  timeout_secs?: number;
}

export interface ProviderHealthSummary {
  total_models: number;
  healthy_models: number;
  unhealthy_models: number;
}

export interface CheckProviderHealthResponse {
  provider_id: number;
  provider_key: string;
  status: HealthStatus;
  summary: ProviderHealthSummary;
  models: ModelHealthStatus[];
  avg_response_time_ms: number | null;
  checked_at: string;
}
