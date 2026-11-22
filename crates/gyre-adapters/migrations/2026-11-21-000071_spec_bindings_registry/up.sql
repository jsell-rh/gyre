-- agent-runtime.md §2 Spec-Level Binding: the binding set is spec-registry
-- metadata covered by the spec SHA recorded in the approval ledger. The
-- ledger entry gains the digest of the spec's meta-spec binding set; the
-- registry SHA derived from (current_sha, bindings_sha) is what approvals
-- are recorded and validated against.
ALTER TABLE spec_ledger_entries ADD COLUMN bindings_sha TEXT NOT NULL DEFAULT '';

-- meta-spec-reconciliation.md amendments row: the workspace/tenant
-- meta-spec-set KV model is retired in favor of registry required flags
-- plus spec-level bindings. No production reader or writer remains.
DROP TABLE IF EXISTS meta_spec_sets;
DROP TABLE IF EXISTS tenant_meta_spec_sets;
