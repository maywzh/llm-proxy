-- Add provider_params JSONB column for provider-specific configuration
-- This allows storing provider-specific parameters like GCP Vertex settings
-- without requiring schema changes for each new provider type.
--
-- Example for GCP Vertex:
-- provider_params = {
--   "gcp_project": "bottle-rocket-cbs",
--   "gcp_location": "global",
--   "gcp_publisher": "anthropic"
-- }

ALTER TABLE providers ADD COLUMN provider_params JSONB NOT NULL DEFAULT '{}';
