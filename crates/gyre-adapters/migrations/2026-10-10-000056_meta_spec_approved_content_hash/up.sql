-- TASK-156 repair: persist the content hash covered by the current approval.
--
-- The §6 reconciliation trigger needs to distinguish "approving content that
-- changed since the last approval" (trigger) from "re-approving unchanged
-- content" (no-op). The shipped UI performs a two-step edit-then-approve flow
-- (PUT {prompt}, then PUT {approval_status}), so keying the trigger on the
-- presence of `prompt` in the same request misses the standard lifecycle.
-- `approved_content_hash` is stamped server-side at approval time — never
-- caller-supplied — and compared against `content_hash` at the next approval.
--
-- NULL for rows never approved or approved before this column existed; the
-- first approval of such a row stamps it (approving a Pending spec is always
-- a content change worth reconciling, which matches the pre-existing
-- behavior for the combined single-request flow).

ALTER TABLE meta_specs ADD COLUMN approved_content_hash TEXT;
