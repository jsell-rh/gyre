-- Add enforce_manifest column to spec_policies table.
-- spec-registry.md §Manifest Rules rule 1 + §Ledger Sync on Push step 4.
ALTER TABLE spec_policies ADD COLUMN enforce_manifest INTEGER NOT NULL DEFAULT 0;
