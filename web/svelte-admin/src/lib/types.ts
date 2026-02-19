// API Response Types
export interface ApiError {
  detail: string;
}

// Model Mapping Types
export interface ModelMappingEntry {
  mapped_model: string;
  max_tokens?: number;
  max_input_tokens?: number;
  max_output_tokens?: number;
  input_cost_per_1k_tokens?: number;
  output_cost_per_1k_tokens?: number;
  supports_vision?: boolean;
  supports_function_calling?: boolean;
  supports_streaming?: boolean;
  supports_response_schema?: boolean;
  supports_reasoning?: boolean;
  supports_computer_use?: boolean;
  supports_pdf_input?: boolean;
  mode?: 'chat' | 'completion' | 'embedding' | 'image_generation';
}

export type ModelMappingValue = string | ModelMappingEntry;

// Provider Types
export interface Provider {
  id: number;
  provider_key: string;
  provider_type: string;
  api_base: string;
  model_mapping: Record<string, ModelMappingValue>;
  is_enabled: boolean;
  provider_params?: Record<string, unknown>;
}

export interface ProviderCreate {
  provider_key: string;
  provider_type: string;
  api_base: string;
  api_key: string;
  model_mapping?: Record<string, ModelMappingValue>;
  provider_params?: Record<string, unknown>;
}

export interface ProviderUpdate {
  provider_type?: string;
  api_base?: string;
  api_key?: string;
  model_mapping?: Record<string, ModelMappingValue>;
  is_enabled?: boolean;
  provider_params?: Record<string, unknown>;
}

export interface ProviderListResponse {
  version: number;
  providers: Provider[];
}

// Backend returns Provider directly (not wrapped)
export type ProviderCreateResponse = Provider;

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

// Backend returns Credential directly (not wrapped)
export type CredentialCreateResponse = Credential;

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
  model_mapping: Record<string, ModelMappingValue>;
  is_enabled: boolean;
  // GCP Vertex AI fields (stored in provider_params on backend)
  gcp_project: string;
  gcp_location: string;
  gcp_publisher: string;
  gcp_blocking_action: string;
  gcp_streaming_action: string;
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

export interface ImageAttachment {
  id: string;
  dataUrl: string;
  name: string;
  type: string;
  size: number;
  source: 'upload' | 'paste';
}

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string | ChatContentPart[];
  thinking?: string;
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
  provider_type: string;
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
  provider_type: string;
  status: HealthStatus;
  summary: ProviderHealthSummary;
  models: ModelHealthStatus[];
  avg_response_time_ms: number | null;
  checked_at: string;
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
  data?: unknown;
  isOpen: boolean;
}

// Request Log Types
export interface RequestLog {
  id: number;
  timestamp: string;
  request_id: string;
  endpoint: string | null;
  credential_name: string | null;
  model_requested: string | null;
  model_mapped: string | null;
  provider_name: string | null;
  provider_type: string | null;
  client_protocol: string | null;
  provider_protocol: string | null;
  is_streaming: boolean | null;
  status_code: number | null;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  total_duration_ms: number | null;
  ttft_ms: number | null;
  error_category: string | null;
  error_message: string | null;
  client: string | null;
}

export interface RequestLogDetail extends RequestLog {
  request_headers: string | null;
  request_body: string | null;
  response_body: string | null;
}

export interface RequestLogListResponse {
  items: RequestLog[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
}

export interface RequestLogFilters {
  request_id?: string;
  provider_name?: string;
  model?: string;
  credential_name?: string;
  status_code?: number;
  is_streaming?: boolean;
  error_only?: boolean;
  start_time?: string;
  end_time?: string;
  sort_by?: string;
  sort_order?: 'asc' | 'desc';
}

export interface RequestLogStats {
  total_requests: number;
  total_errors: number;
  error_rate: number;
  total_input_tokens: number;
  total_output_tokens: number;
  avg_duration_ms: number | null;
  avg_ttft_ms: number | null;
  requests_by_provider: Record<string, number>;
  requests_by_model: Record<string, number>;
  requests_by_status: Record<string, number>;
}

// Error Log Types
export interface ErrorLog {
  id: number;
  timestamp: string;
  request_id: string | null;
  error_category: string;
  error_code: number | null;
  error_message: string | null;
  provider_name: string | null;
  credential_name: string | null;
  model_requested: string | null;
  model_mapped: string | null;
  endpoint: string | null;
  client_protocol: string | null;
  provider_protocol: string | null;
  is_streaming: boolean | null;
  total_duration_ms: number | null;
}

export interface ErrorLogDetail extends ErrorLog {
  request_body: string | null;
  response_body: string | null;
  provider_request_body: string | null;
  provider_request_headers: string | null;
}

export interface ErrorLogListResponse {
  items: ErrorLog[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
}

export interface ErrorLogFilters {
  request_id?: string;
  provider_name?: string;
  error_category?: string;
  start_time?: string;
  end_time?: string;
  sort_by?: string;
  sort_order?: 'asc' | 'desc';
}
