-- Rollback: Revert auto-increment ids back to VARCHAR ids and credentials back to master_keys

-- Step 1: Drop existing triggers temporarily
DROP TRIGGER IF EXISTS trg_providers_version ON providers;
DROP TRIGGER IF EXISTS trg_credentials_version ON credentials;

-- Step 2: Rename new tables
ALTER TABLE providers RENAME TO providers_new;
ALTER TABLE credentials RENAME TO credentials_new;

-- Step 3: Recreate original providers table with VARCHAR id
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

-- Step 4: Recreate original master_keys table with VARCHAR id
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

-- Step 5: Migrate data back from new tables to original tables
INSERT INTO providers (id, provider_type, api_base, api_key, model_mapping, is_enabled, created_at, updated_at)
SELECT provider_key, provider_type, api_base, api_key, model_mapping, is_enabled, created_at, updated_at
FROM providers_new;

INSERT INTO master_keys (id, key_hash, name, allowed_models, rate_limit, is_enabled, created_at, updated_at)
SELECT
    'key-' || id::text,
    credential_key,
    name,
    ARRAY(SELECT jsonb_array_elements_text(allowed_models)),
    rate_limit,
    is_enabled,
    created_at,
    updated_at
FROM credentials_new;

-- Step 6: Drop new tables
DROP TABLE providers_new;
DROP TABLE credentials_new;

-- Step 7: Recreate original index
CREATE INDEX idx_master_keys_key_hash ON master_keys(key_hash);

-- Step 8: Recreate triggers
CREATE TRIGGER trg_providers_version
    AFTER INSERT OR UPDATE OR DELETE ON providers
    FOR EACH STATEMENT EXECUTE FUNCTION increment_config_version();

CREATE TRIGGER trg_master_keys_version
    AFTER INSERT OR UPDATE OR DELETE ON master_keys
    FOR EACH STATEMENT EXECUTE FUNCTION increment_config_version();