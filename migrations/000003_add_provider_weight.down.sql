-- Remove weight column from providers table
DROP INDEX IF EXISTS idx_providers_enabled_weight;
ALTER TABLE providers DROP COLUMN IF EXISTS weight;