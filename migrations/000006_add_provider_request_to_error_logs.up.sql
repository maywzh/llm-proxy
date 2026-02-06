ALTER TABLE error_logs ADD COLUMN provider_request_body JSONB;
ALTER TABLE error_logs ADD COLUMN provider_request_headers JSONB;
