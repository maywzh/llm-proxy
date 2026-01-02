-- Providers table
CREATE TABLE providers (
    id VARCHAR(255) PRIMARY KEY,
    provider_type VARCHAR(50) NOT NULL,
    api_base VARCHAR(500) NOT NULL,
    api_key VARCHAR(500) NOT NULL,
    model_mapping JSONB NOT NULL DEFAULT '{}',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Master keys table
CREATE TABLE master_keys (
    id VARCHAR(255) PRIMARY KEY,
    key_hash VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    allowed_models TEXT[] NOT NULL DEFAULT '{}',
    rate_limit INTEGER,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for authentication lookup
CREATE INDEX idx_master_keys_key_hash ON master_keys(key_hash);

-- Config version table (singleton)
CREATE TABLE config_version (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    version BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Initialize version
INSERT INTO config_version (id, version) VALUES (1, 0);

-- Function to increment version
CREATE OR REPLACE FUNCTION increment_config_version()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE config_version SET version = version + 1, updated_at = NOW() WHERE id = 1;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Triggers for auto-increment version
CREATE TRIGGER trg_providers_version
    AFTER INSERT OR UPDATE OR DELETE ON providers
    FOR EACH STATEMENT EXECUTE FUNCTION increment_config_version();

CREATE TRIGGER trg_master_keys_version
    AFTER INSERT OR UPDATE OR DELETE ON master_keys
    FOR EACH STATEMENT EXECUTE FUNCTION increment_config_version();