-- Create request_logs table for logging all API requests and responses
CREATE TABLE request_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    credential_id INTEGER REFERENCES credentials(id),
    credential_name VARCHAR(255),
    provider_id INTEGER REFERENCES providers(id),
    provider_name VARCHAR(255),
    endpoint VARCHAR(100) NOT NULL,
    method VARCHAR(10) NOT NULL,
    model VARCHAR(100),
    is_streaming BOOLEAN DEFAULT false,
    status_code INTEGER,
    duration_ms INTEGER,
    ttft_ms INTEGER,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    total_tokens INTEGER,
    request_body JSONB,
    response_body JSONB,
    error_message TEXT,
    client_ip VARCHAR(45),
    user_agent TEXT
);

-- Create indexes for common query patterns
CREATE INDEX idx_request_logs_created_at ON request_logs(created_at DESC);
CREATE INDEX idx_request_logs_credential_id ON request_logs(credential_id);
CREATE INDEX idx_request_logs_provider_id ON request_logs(provider_id);
CREATE INDEX idx_request_logs_model ON request_logs(model);
