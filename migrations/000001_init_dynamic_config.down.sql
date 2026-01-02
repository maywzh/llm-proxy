DROP TRIGGER IF EXISTS trg_master_keys_version ON master_keys;
DROP TRIGGER IF EXISTS trg_providers_version ON providers;
DROP FUNCTION IF EXISTS increment_config_version();
DROP TABLE IF EXISTS config_version;
DROP TABLE IF EXISTS master_keys;
DROP TABLE IF EXISTS providers;