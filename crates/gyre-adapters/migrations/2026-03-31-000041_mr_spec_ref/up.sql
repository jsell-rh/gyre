-- Add spec_ref column to merge_requests for spec→MR provenance binding.
-- Format: "path@sha" (e.g. "system/hello-world.md@abc123...").
ALTER TABLE merge_requests ADD COLUMN spec_ref TEXT;
