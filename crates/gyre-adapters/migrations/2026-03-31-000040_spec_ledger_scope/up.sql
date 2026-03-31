-- Add repo_id and workspace_id to spec_ledger_entries for SpecApproved signal chain routing.
-- These enable Destination::Workspace routing and repo_id in the SpecApproved payload.
-- Nullable because existing entries may not have been associated with a repo yet.
ALTER TABLE spec_ledger_entries ADD COLUMN repo_id TEXT;
ALTER TABLE spec_ledger_entries ADD COLUMN workspace_id TEXT;
