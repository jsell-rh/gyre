-- S2.3: Interrogation Agents (HSI §4)
-- Add conversation_sha to attestation_bundles for MR attestation provenance.
ALTER TABLE attestation_bundles ADD COLUMN conversation_sha TEXT;
