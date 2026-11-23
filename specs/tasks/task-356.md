---
title: "VerificationResult trees persisted on attestations and provenance/identity events recorded in the audit system"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / verification-result-persistence / 1d3b1257654b"
depends_on: [task-352]
progress: not-started
commits: []
---

## Required behavior

§6.4: every verification (push enforcement, merge enforcement, and the GET .../verification endpoint) produces a VerificationResult tree that is attached to the attestation record and stored for audit — persisted through the existing ChainAttestationRepository (attached to the attestation record and re-saved, so it is durable across restarts on every backend including PostgreSQL once chain-task-commit-linkage lands the PG adapter; no bare in-memory AppState map, per the in-memory-state-stores invariant), queryable via GET /api/v1/repos/:id/attestations/:commit_sha/verification (the endpoint returns the stored tree; recomputation may remain only as a fallback when none is stored — today the endpoint recomputes on each request and no code path stores a result tree against an attestation record). §7.7: the seven event kinds — attestation.created, attestation.verified, attestation.constraint_failed, attestation.chain_invalid (Provenance) and key_binding.created, key_binding.expired, key_binding.revoked (Identity) — are recorded through the audit system (AuditEvent records with new AuditEventType variants/categories, state.audit persistence, SIEM forwarding, /api/v1/audit queryability) in addition to the existing tracing logs (today they exist only as structured tracing logs with scalar fields; the AuditEventType enum in gyre-domain/src/audit.rs ends at supply-chain/Custom with no provenance or identity variants); every event that arises from a verification includes the full serialized VerificationResult tree in the audit record for forensic analysis, not just scalar fields. Existing tracing logs are retained (no regression).

## Acceptance Criteria

- [ ] After a push/merge verification, the attestation record carries the VerificationResult tree (with children naming failing constraints); GET .../verification returns the stored tree.
- [ ] The seven event kinds are queryable via /api/v1/audit with the correct categories (Provenance/Identity); events arising from verifications embed the full VerificationResult tree.
- [ ] Verification results survive a server restart (durable storage); key-binding lifecycle events (create/expire/revoke) are recorded as Identity audit events.

## Verification intent

At the base: after any verification no VerificationResult is persisted against the attestation (the endpoint recomputes the tree on each request; no code path stores a result tree against an attestation record), and /api/v1/audit returns no attestation.* or key_binding.* events (the AuditEventType enum in gyre-domain/src/audit.rs ends at supply-chain/Custom — no provenance or identity variants exist), while attestation.verified exists only as a tracing log with scalar fields and no tree (constraint_check.rs:1569). Probe: run a verification, then query the audit store and the attestation record — both come back empty at the base; after the task the stored tree is returned by the endpoint and the audit query finds the events with embedded trees.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
  {
    "id": "f3d63f8ba82d318a407da9e3",
    "decision": "gap",
    "requirement_quote": "- Attached to the attestation record",
    "detail": "Every verification does produce a VerificationResult tree (label/valid/message/children) — the push/merge enforcement paths and the GET .../verification endpoint build real trees, and constraint children name the failing constraint. But of the three storage requirements: 'Logged to the audit system' is only satisfied as tracing structured logs (category/event fields), not as AuditEvent records in the audit store (no attestation.* AuditEventType variants exist); 'Attached to the attestation record' is not implemented — verification results are never persisted to the attestation or any store, only returned in the HTTP response; 'Queryable via GET .../verification' is implemented (the endpoint recomputes the tree on demand rather than returning a stored one).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "quote": "no attestation found for commit {commit_sha}",
        "detail": "The verification endpoint recomputes the result tree on each request; no code path stores a VerificationResult against the attestation record."
      },
      {
        "file": "crates/gyre-domain/src/audit.rs",
        "quote": "Custom(String),",
        "detail": "The AuditEventType enum ends at supply-chain/custom — there are no attestation.created/verified/constraint_failed/chain_invalid or key_binding.* variants, so provenance verification results cannot be recorded in the audit event store."
      },
      {
        "file": "crates/gyre-common/src/attestation.rs",
        "quote": "pub struct VerificationResult {",
        "detail": "The recursive result-tree type exists and is produced by every verification path; the gap is purely its persistence/audit recording."
      }
    ]
  },
  {
    "id": "c8e81723fe471db62eb85bbc",
    "decision": "gap",
    "requirement_quote": "All events include the full `VerificationResult` tree for forensic analysis.",
    "detail": "All seven event names are emitted at the right places, but as structured tracing logs only: attestation.created (approval, spawn derivation, gate attestation), attestation.verified / attestation.constraint_failed (constraint result logging), attestation.chain_invalid (chain and context-binding failures), key_binding.created / key_binding.revoked (key-binding endpoints) — each with category ('Provenance'/'Identity') and event fields. None of them are recorded through the audit system the amendment references: the AuditEvent/AuditEventType machinery (state.audit.record, SIEM forwarding, /api/v1/audit endpoints) has no attestation.* or key_binding.* variants, so these events never reach the audit store or SIEM stream. And no event includes the full VerificationResult tree — the log lines carry scalar fields (constraint counts, messages), and the spec's requirement that every event carry the tree for forensics is unmet.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "event = \"attestation.verified\",",
        "detail": "The attestation.verified event is emitted as a tracing log with scalar fields only — no VerificationResult tree is attached to the event."
      },
      {
        "file": "crates/gyre-domain/src/audit.rs",
        "quote": "Custom(String),",
        "detail": "The audit event type enum has no attestation.* or key_binding.* variants, so provenance/identity events cannot be recorded into the audit store that observability.md defines (total auditing, SIEM forwarding) — they exist only in tracing output."
      }
    ]
  }
]
