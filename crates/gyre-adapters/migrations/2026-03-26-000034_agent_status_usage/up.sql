-- Migration 000034: agent usage tracking + attestation meta_specs_used
-- Adds token/cost usage columns to agents table.
-- meta_specs_used is persisted inside the attestation JSON blob (attestation_bundles.attestation).

ALTER TABLE agents ADD COLUMN usage_tokens_input INTEGER;
ALTER TABLE agents ADD COLUMN usage_tokens_output INTEGER;
ALTER TABLE agents ADD COLUMN usage_cost_usd REAL;
