-- Migration: Change id columns from VARCHAR to auto-increment SERIAL
-- Add provider_key column for providers (the actual provider identifier)
-- Rename master_keys to credentials with credential_key for authentication

-- Step 1: Drop existing triggers temporarily
DROP TRIGGER IF EXISTS trg_providers_version ON providers;
DROP TRIGGER IF EXISTS trg_master_keys_version ON master_keys;

-- Step 2: Rename old tables
ALTER TABLE providers RENAME TO providers_old;
ALTER TABLE master_keys RENAME TO master_keys_old;

-- Step 3: Create new providers table with auto-increment id
CREATE TABLE providers (
    id SERIAL PRIMARY KEY,
    provider_key VARCHAR(255) NOT NULL UNIQUE,
    provider_type VARCHAR(50) NOT NULL,
    api_base VARCHAR(500) NOT NULL,
    api_key VARCHAR(500) NOT NULL,
    model_mapping JSONB NOT NULL DEFAULT '{}',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Step 4: Create new credentials table (renamed from master_keys) with auto-increment id
CREATE TABLE credentials (
    id SERIAL PRIMARY KEY,
    credential_key VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    allowed_models JSONB NOT NULL DEFAULT '[]',
    rate_limit INTEGER,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Step 5: Migrate data from old tables to new tables
INSERT INTO providers (provider_key, provider_type, api_base, api_key, model_mapping, is_enabled, created_at, updated_at)
SELECT id, provider_type, api_base, api_key, model_mapping, is_enabled, created_at, updated_at
FROM providers_old;

INSERT INTO credentials (credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at)
SELECT key_hash, name,
    CASE
        WHEN allowed_models IS NULL THEN '[]'::jsonb
        ELSE to_jsonb(allowed_models)
    END,
    rate_limit, is_enabled, created_at, updated_at
FROM master_keys_old;

-- Step 6: Drop old tables
DROP TABLE providers_old;
DROP TABLE master_keys_old;

-- Step 7: Create indexes
CREATE INDEX idx_providers_provider_key ON providers(provider_key);
CREATE INDEX idx_credentials_credential_key ON credentials(credential_key);
CREATE INDEX idx_providers_is_enabled ON providers(is_enabled);
CREATE INDEX idx_credentials_is_enabled ON credentials(is_enabled);

-- Step 8: Recreate triggers for auto-increment version
CREATE TRIGGER trg_providers_version
    AFTER INSERT OR UPDATE OR DELETE ON providers
    FOR EACH STATEMENT EXECUTE FUNCTION increment_config_version();

CREATE TRIGGER trg_credentials_version
    AFTER INSERT OR UPDATE OR DELETE ON credentials
    FOR EACH STATEMENT EXECUTE FUNCTION increment_config_version();