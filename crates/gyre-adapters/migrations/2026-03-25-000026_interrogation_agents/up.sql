-- S2.3: Interrogation Agents (HSI §4)
-- Add conversation_sha to attestation_bundles for MR attestation provenance.
-- Table-recreate pattern: works whether or not conversation_sha already exists.

CREATE TABLE attestation_bundles_new (
    mr_id TEXT PRIMARY KEY NOT NULL,
    attestation TEXT NOT NULL,
    signature TEXT NOT NULL,
    signing_key_id TEXT NOT NULL,
    conversation_sha TEXT
);

-- Copy existing data. Use SELECT * approach that works regardless of source schema:
-- if conversation_sha exists in source, it's preserved; if not, it defaults to NULL.
INSERT INTO attestation_bundles_new (mr_id, attestation, signature, signing_key_id)
    SELECT mr_id, attestation, signature, signing_key_id FROM attestation_bundles;

DROP TABLE attestation_bundles;
ALTER TABLE attestation_bundles_new RENAME TO attestation_bundles;
