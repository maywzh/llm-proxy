-- Add weight column to providers table for weighted load balancing
ALTER TABLE providers ADD COLUMN weight INTEGER NOT NULL DEFAULT 1;

-- Add index for enabled providers with weight (for efficient weighted selection)
CREATE INDEX idx_providers_enabled_weight ON providers(is_enabled, weight) WHERE is_enabled = true;