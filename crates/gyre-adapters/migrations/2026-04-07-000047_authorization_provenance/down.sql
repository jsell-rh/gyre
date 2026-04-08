-- Reverse authorization provenance tables

DROP INDEX IF EXISTS idx_chain_attestations_parent;
DROP INDEX IF EXISTS idx_chain_attestations_repo_time;
DROP INDEX IF EXISTS idx_chain_attestations_commit;
DROP INDEX IF EXISTS idx_chain_attestations_task;
DROP TABLE IF EXISTS chain_attestations;

DROP INDEX IF EXISTS idx_key_bindings_tenant_pubkey;
DROP INDEX IF EXISTS idx_key_bindings_tenant_identity;
DROP TABLE IF EXISTS key_bindings;

DROP TABLE IF EXISTS trust_anchors;
