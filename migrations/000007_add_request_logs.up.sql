CREATE TABLE request_logs (
    id BIGSERIAL PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    request_id VARCHAR(64) NOT NULL,
    endpoint VARCHAR(255),
    credential_name VARCHAR(255),
    model_requested VARCHAR(255),
    model_mapped VARCHAR(255),
    provider_name VARCHAR(255),
    provider_type VARCHAR(50),
    client_protocol VARCHAR(50),
    provider_protocol VARCHAR(50),
    is_streaming BOOLEAN DEFAULT false,
    status_code INTEGER,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    total_duration_ms INTEGER,
    ttft_ms INTEGER,
    error_category VARCHAR(50),
    error_message TEXT,
    request_body TEXT,
    response_body TEXT
);

CREATE INDEX idx_request_logs_timestamp ON request_logs(timestamp);
CREATE INDEX idx_request_logs_request_id ON request_logs(request_id);
CREATE INDEX idx_request_logs_provider ON request_logs(provider_name);
CREATE INDEX idx_request_logs_model ON request_logs(model_requested);
CREATE INDEX idx_request_logs_credential ON request_logs(credential_name);
CREATE INDEX idx_request_logs_status ON request_logs(status_code);
