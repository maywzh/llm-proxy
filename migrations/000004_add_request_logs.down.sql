-- Drop indexes for request_logs table
DROP INDEX IF EXISTS idx_request_logs_model;
DROP INDEX IF EXISTS idx_request_logs_provider_id;
DROP INDEX IF EXISTS idx_request_logs_credential_id;
DROP INDEX IF EXISTS idx_request_logs_created_at;

-- Drop request_logs table
DROP TABLE IF EXISTS request_logs;
