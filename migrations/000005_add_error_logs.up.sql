CREATE TABLE error_logs (
    id BIGSERIAL PRIMARY KEY,
    request_id VARCHAR(64) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    error_category VARCHAR(32) NOT NULL,
    error_code INTEGER,
    error_message TEXT,
    endpoint VARCHAR(255),
    client_protocol VARCHAR(32),
    request_headers JSONB,
    request_body JSONB,
    provider_name VARCHAR(255),
    provider_api_base VARCHAR(500),
    provider_protocol VARCHAR(32),
    mapped_model VARCHAR(255),
    response_status_code INTEGER,
    response_body JSONB,
    total_duration_ms INTEGER,
    credential_name VARCHAR(255),
    client VARCHAR(255),
    is_streaming BOOLEAN DEFAULT false
);

CREATE INDEX idx_error_logs_request_id ON error_logs(request_id);
CREATE INDEX idx_error_logs_timestamp ON error_logs(timestamp);
CREATE INDEX idx_error_logs_category ON error_logs(error_category);
CREATE INDEX idx_error_logs_provider ON error_logs(provider_name);
