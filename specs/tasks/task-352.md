---
title: "SignedInput roots travel with tasks; attestations bind real commits; merge node with mapped fields; release bundle; PostgreSQL adapters"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / chain-task-commit-linkage / 1d3b1257654b"
depends_on: [task-351]
progress: not-started
commits: []
---

## Required behavior

Task linkage (§2.1 'travels with every task derived from that spec'): chain lookups keyed by task (spawn's create_derived_input_for_agent, spawn.rs:4292/4307; push/merge enforcement in constraint_check.rs, find_by_task at :51/:288/:594/:844; gate_executor) resolve the approval-time SignedInput root from the task's spec_path — the same resolution §2.4 already uses (newest active approval for the path in the task's workspace/repo scope) — so the chain (root + derived nodes) is locatable for every task whose spec has an approved SignedInput. A root shared by multiple tasks must remain resolvable for each (no single-slot task_id overwrite that orphans sibling tasks; implement via spec-path resolution at lookup or a per-task link, without breaking the §5.4 port's method contract). Observable effects: spawn no longer takes the 'no attestation chain for task — skipping DerivedInput creation' branch for approved specs (a spawner-signed DerivedInput node is created); push/merge enforcement and gate_executor locate the chain. Commit binding (§5.3 locations 2–3, §6.3): when a push is accepted, the pushing agent's chain leaf is updated with the real pushed commit SHA (AttestationOutput.commit_sha; today approval stores commit_sha "" at specs.rs and spawn stores commit_sha: String::new() at spawn.rs) and re-persisted (upsert), so find_by_commit matches real commits. The already-implemented downstream machinery must become reachable for real commits: the chain git note is written to refs/notes/attestations on the commit, the AIBOM's per-commit chain_attestation is populated, and GET /api/v1/repos/:id/attestations/:commit_sha/bundle returns the VerificationBundle (chain root-to-leaf, the tenant's trust anchors, the actual git diff, assembly timestamp) instead of 404 'no attestation found for commit'. Release attachment (§6.3, review correction): the VerificationBundle is attached to releases alongside the AIBOM — the release artifact path (GET /repos/:id/aibom?format=release and the release-prepare flow) carries or references the per-commit VerificationBundle, coordinating with the staged supply-chain plan's release-artifacts-aibom-sbom task (planner spec-bdd98e1a5c6d22f3f997de92) and the ci-docs-release plan's release-flow task so the bundle rides the same release-artifact mechanism rather than duplicating release machinery. §5.2 subsumption completion (review correction): AttestationOutput gains the mapping table's extended fields conversation_sha and completion_summary (today the struct has only content_hash/commit_sha/agent_signature/gate_results, gyre-common/src/attestation.rs:181-192, and the values exist only on the legacy MergeAttestation, merge_processor.rs:2830-2834); the merge attestation becomes the final attestation in a chain — merge appends a merge-time attestation node to the chain (linked to the chain leaf, recording the merge commit SHA as AttestationOutput.commit_sha per the merge_commit_sha mapping row, carrying conversation_sha/completion_summary populated from the same sources the legacy bundle uses, and the gate results), so both mapping-table rows that name 'AttestationOutput extended field (unchanged)' are realized on the chain and a chained merge loses no data relative to the legacy bundle. PostgreSQL adapters (§5.4): the PgStorage implementations of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository in crates/gyre-adapters/src/postgres/authorization_provenance.rs are real — every method currently anyhow::bail!s (16 stubs); replace them with real SQL against PostgreSQL via the existing store! wiring (PgStorage is selected whenever GYRE_DATABASE_URL is postgres://…, a documented production backend), tenant-scoped like the SQLite adapter, with the same port contract semantics (content-addressed upsert, parent-ref chain walk with loop guard, task/commit/repo indexes, TTL/revocation semantics for key bindings, CRUD for trust anchors); schema comes from the shared embedded migrations that already run on both backends (any new migration uses the next unused 6-digit sequence number and portable SQL per check-migration-sql-portability); on a PostgreSQL deployment the provenance stores must behave identically to SQLite, because later enforcement (boundary-enforcement) must not silently assume store availability it does not have on PG.

## Acceptance Criteria

- [ ] For a task whose spec was approved with a SignedInput, spawning an agent creates a spawner-signed DerivedInput chain node (the skip branch does not fire), and the chain (root → derived) is locatable from the task for push/merge enforcement and gate attestation attachment.
- [ ] Two tasks derived from the same approved spec both resolve the shared root (no orphaning).
- [ ] After the agent pushes, the chain leaf carries the pushed commit SHA: GET .../attestations/:commit_sha/bundle returns 200 with the full VerificationBundle, the chain note exists on the commit under refs/notes/attestations, and the AIBOM entry for the commit includes the chain.
- [ ] After merge, a merge-time attestation node is appended to the chain recording the merge commit SHA (merge_commit_sha mapping row) and carrying conversation_sha and completion_summary (both §5.2 mapping rows realized on the chain); the merge-time ABAC gate and note attachment locate the chain by commit.
- [ ] A produced release artifact carries or references the VerificationBundle for the release's commits (chain + trust anchors + git diff + timestamp) alongside the AIBOM.
- [ ] On a PostgreSQL deployment (GYRE_DATABASE_URL=postgres://…): chain attestation save/find_by_id/load_chain/find_by_task/find_by_commit/find_by_repo, key-binding persistence, and trust-anchor CRUD all succeed with tenant isolation — no method bails; the approval→spawn→push→merge flow that works on SQLite works on PG.

## Verification intent

At the base: approve a spec with a binding and signature, create a task from it, spawn an agent — spawn logs 'no attestation chain for task — skipping DerivedInput creation' (spawn.rs:4307) and no derived node exists (find_by_task returns nothing because the root is stored with task_id "", specs.rs); after the agent pushes, GET .../attestations/<pushed-sha>/bundle returns 404 'no attestation found for commit' (approval and spawn both store commit_sha "" and nothing ever updates it), git notes show no chain note for the commit, and push enforcement logs 'no attestation chain found for task (Phase 3 enforcement, allowing)' (constraint_check.rs:299). Merge-node probe: after a merge, the chain contains no merge-time node (merge_processor attaches the pre-existing chain as a git note only, and the legacy AttestationBundle remains the merge-time record), and AttestationOutput has no conversation_sha/completion_summary fields (attestation.rs:181-192) — a consumer reading the chain cannot obtain values the legacy bundle carries. Release probe: the format=release AIBOM artifact contains per-commit chain_attestation but no VerificationBundle, and no release-preparation output references one. PostgreSQL probe: start the server with a postgres:// GYRE_DATABASE_URL and perform any provenance store operation — at the base every one fails with 'ChainAttestationRepository not implemented for PgStorage' (authorization_provenance.rs is a self-described stub, 16 bail! calls); after the task the operations succeed and a PG-backed approval→push→verification flow returns real chain data. Every one of these must flip after the task.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
  {
    "id": "0a001bfe1c6ccfa6bd7e4cb3",
    "decision": "gap",
    "requirement_quote": "the approval now produces a Signed Input that travels with every task derived from that spec.",
    "detail": "Spec approval does produce a real SignedInput when the approver has an active KeyBinding and supplies user_content_signature (specs.rs): the InputContent is built (spec registry SHA, workspace/repo from the ledger, persona constraints, meta_spec_set_sha, scope), the user's Ed25519 signature over SHA256(content) is verified against the KeyBinding public key, and the root attestation is saved to the chain store. But it does NOT travel with tasks: the root attestation is stored with metadata.task_id = \"\" and commit_sha = \"\", and no production code ever re-saves or links it to a task. Every downstream consumer looks chains up by task_id (spawn's create_derived_input_for_agent, push/merge enforcement in constraint_check.rs, gate_executor) or by commit_sha (merge ABAC gate, git-note attachment, verification endpoints), so in the production flow the root is unreachable: derived inputs are never created ('no attestation chain for task'), push enforcement finds no chain and allows, and the verification/bundle/chain endpoints 404. The wiring step that associates the approval-time SignedInput with tasks derived from the spec is missing.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "task_id: String::new(), // No task yet at approval time.",
        "detail": "The root SignedInput attestation is persisted with an empty task_id. The only production save sites for chain attestations are this root (empty task_id), spawn's derived nodes (real task_id, but creation is gated on find_by_task which never returns the root), and gate_executor's leaf update — nothing links the root to a task."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "no attestation chain for task — skipping DerivedInput creation",
        "detail": "create_derived_input_for_agent looks up the parent chain exclusively via chain_attestations.find_by_task(task_id). Because the root is stored with task_id \"\", this returns empty for every real task and derived-input creation is skipped in the production flow."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "no attestation chain found for task (Phase 3 enforcement, allowing)",
        "detail": "Push-time enforcement also keys on find_by_task; with the root unlinked, no chain is ever found, so the enforcement path takes the allow branch for every production push. Tests pass only because they insert attestations whose task_id matches directly (make_attestation in this file's test module)."
      }
    ]
  },
  {
    "id": "acda07a63c3a25e9c32461e5",
    "decision": "gap",
    "requirement_quote": "| `conversation_sha` | `AttestationOutput` extended field (unchanged) |",
    "detail": "The backward-compatibility retention is real: the legacy AttestationBundle/MergeAttestation types and the AttestationRepository port are kept, the bundle carries a Phase-4 deprecation notice, and the merge processor performs the dual-write (chain attestation note when a chain is locatable, legacy bundle note otherwise). But the mapping table that defines the subsumption is not fully realized, and the subsumption statement itself is not implemented. Two rows of the table map `conversation_sha` and `completion_summary` to 'AttestationOutput extended field (unchanged)' — the shipped AttestationOutput has no such extended fields (only content_hash, commit_sha, agent_signature, gate_results); these values are constructed only on the legacy MergeAttestation and never enter any chain attestation, so any consumer that stops producing the legacy bundle loses them. And 'The merge attestation becomes the final attestation in a chain' is unrealized: merge appends no attestation node to the chain — it only attaches the pre-existing chain (located via find_by_commit or the author-agent task fallback) as a git note on the merge commit, while the legacy bundle remains the merge-time record stored in the attestation store. The subsumption is therefore structural only for the rows that are realized (spec_ref/spec_sha at the chain root, per-node signing keys, gate results per node); the conversation_sha/completion_summary rows and the final merge node are missing behavior.",
    "evidence": [
      {
        "file": "crates/gyre-common/src/attestation.rs",
        "quote": "pub struct AttestationOutput {",
        "detail": "The shipped AttestationOutput contains exactly content_hash, commit_sha, agent_signature and gate_results — there are no conversation_sha or completion_summary 'extended fields', so the two mapping rows that promise them ('AttestationOutput extended field (unchanged)') have no realization in the chain format."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "completion_summary: None,",
        "detail": "conversation_sha and completion_summary are carried only by the legacy MergeAttestation constructed at merge time (crates/gyre-server/src/merge_processor.rs, the crate::attestation::MergeAttestation literal); no chain attestation node ever carries them."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "// During the dual-write period, we write EITHER the chain attestation",
        "detail": "Merge appends no final attestation node to the chain: the chain note path only re-attaches the pre-existing chain (found via find_by_commit or the author agent's task) as a git note on the merge commit; the merge-time record stored in the attestation store is still the legacy bundle, so 'the merge attestation becomes the final attestation in a chain' is not implemented."
      },
      {
        "file": "crates/gyre-domain/src/attestation.rs",
        "quote": "**Deprecated (Phase 4):** This legacy format is subsumed by the chain attestation",
        "detail": "The deprecation notice directing consumers to the chain format is retained — the retention half of the section is genuinely implemented; the gap is the unrealized mapping rows and the missing final merge node."
      }
    ]
  },
  {
    "id": "c49cf3ba98e72736cfcb8bee",
    "decision": "gap",
    "requirement_quote": "2. **Git notes** (`refs/notes/attestations`) — the chain attestation is attached to the relevant commit",
    "detail": "All three storage locations have real implementations, but the git-notes and commit-indexed paths are unreachable for real commits: (1) the attestation store is real on SQLite and mem (tenant isolation, task/commit/repo indexes, parent_ref chain walking), but the PgStorage adapter is a bail! stub (see §5.4), so on a PostgreSQL deployment location 1 fails entirely; (2) attach_chain_attestation_note writes the chain JSON to refs/notes/attestations and read helpers parse it back — but it only runs when chain_attestations.find_by_commit(sha) or the task fallback finds an attestation, and no production code ever records a real commit_sha on an attestation (approval stores commit_sha: \"\" 'No commit yet', spawn stores commit_sha: String::new(), gate_executor preserves the leaf's empty value), so no chain is ever attached to any real commit and the legacy-bundle fallback fires instead; (3) AIBOM aggregation includes chain_attestation per commit, reading the same never-matching store. The DB location is genuinely implemented; the commit-binding wiring that locations 2 and 3 depend on is missing.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/attestation.rs",
        "quote": "pub const CHAIN_ATTESTATION_NOTES_REF: &str = \"refs/notes/attestations\";",
        "detail": "Real git-notes writer/reader for chain attestations under the specced ref exists (attach/read helpers invoke git notes)."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "commit_sha: String::new(), // No commit yet at approval time.",
        "detail": "The approval-time root is stored with an empty commit_sha and nothing later updates it when the work is committed."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "commit_sha: String::new(),",
        "detail": "Derived attestations are also created with empty commit_sha; with no production save path ever setting a real commit SHA, find_by_commit (merge-time note attachment, merge ABAC gate, verification/bundle/chain endpoints) never matches a real commit."
      }
    ]
  },
  {
    "id": "1a1c2218815dfe88c38a9789",
    "decision": "gap",
    "requirement_quote": "pub trait ChainAttestationRepository: Send + Sync {",
    "detail": "The port trait exists verbatim with all six specced methods (save, find_by_id, load_chain walking parent_ref, find_by_task, find_by_commit, find_by_repo with time range). Two of the three adapters are real: SQLite (Diesel, tenant-scoped, upsert, parent-walk with loop guard) and the in-memory adapter wired into AppState. The third is not: the PgStorage implementation is a self-described stub in which every method of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository calls anyhow::bail!. The store! wiring in gyre-server/src/lib.rs selects PgStorage whenever GYRE_DATABASE_URL is a postgres:// URL — a documented production backend (docs/server-config.md) — so on a PostgreSQL deployment every provenance store operation fails. Combined with the fail-open error handling in the enforcement path ('Query failures are non-blocking', constraint_check.rs), provenance silently degrades to nothing on PG, and once boundary enforcement makes provenance mandatory this either bricks PG deployments or — if the fail-open paths survive — exempts them from the mandatory policy. The prior audit's claim that Postgres is a 'real adapter' was factually false and is corrected here; per the goal's rule 1 a bail!() stub is an open gap.",
    "evidence": [
      {
        "file": "crates/gyre-ports/src/chain_attestation.rs",
        "quote": "pub trait ChainAttestationRepository: Send + Sync {",
        "detail": "The port trait with the exact §5.4 method set."
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/chain_attestation.rs",
        "quote": "impl ChainAttestationRepository for SqliteStorage {",
        "detail": "Full SQLite implementation: content-addressed upsert, tenant isolation on every query, parent_ref chain walk with a 100-node loop guard, task/commit/repo/time-range indexes — real storage, not a stub."
      },
      {
        "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
        "quote": "Stub PgStorage implementations for authorization provenance port traits.",
        "detail": "The file header self-describes as a stub with 'Full implementation deferred to future milestone' — this contradicts the prior audit's claim of a real Postgres adapter."
      },
      {
        "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
        "quote": "anyhow::bail!(\"ChainAttestationRepository not implemented for PgStorage\")",
        "detail": "Every method of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository bails in this file (16 bail! calls total); no PgStorage provenance operation can succeed."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "chain_attestations: store!(",
        "detail": "The chain attestation store is wired through the store! macro, whose first branch is 'if let Some(ref d) = pg_db' — PgStorage is selected whenever a postgres:// GYRE_DATABASE_URL is configured, so a documented production backend routes all provenance storage to the bail! stub."
      }
    ]
  },
  {
    "id": "bb664ae221456d242e8ae921",
    "decision": "gap",
    "requirement_quote": "- Attached to releases alongside the AIBOM",
    "detail": "The export route is implemented with the specced path and assembles a real VerificationBundle — attestation chain (load_chain), the tenant's trust anchors (workspace → tenant → list_by_tenant), the actual git diff of the commit (git diff commit^..commit), and the assembly timestamp. Two of the section's requirements are not met. (1) Reachability: the endpoint is keyed by find_by_commit, and no production path ever records a real commit SHA on an attestation (approval and spawn both store empty commit_sha and nothing updates them), so for every real commit the endpoint returns 404 'no attestation found for commit' — the export machinery is unreachable for production commits. (2) 'Attached to releases alongside the AIBOM': not implemented anywhere — the release artifact path (GET /api/v1/repos/:id/aibom?format=release) embeds per-commit chain_attestation (the chain JSON) but never the VerificationBundle (chain + trust anchors + git diff + timestamp); the bundle is assembled only inside the export endpoint and no release artifact or release-preparation path (POST /api/v1/release/prepare) carries or references it. External compliance verification of such a bundle is performed by external tools outside platform scope, but attachability to releases is an in-platform, implementable requirement with no implementation. Both the commit-binding wiring and the release attachment are missing behavior.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "/api/v1/repos/:id/attestations/:commit_sha/bundle",
        "detail": "The specced export route is registered — the export half of the section has a real endpoint."
      },
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "quote": "no attestation found for commit {commit_sha}",
        "detail": "The 404 branch taken whenever find_by_commit misses — which, since no production save path ever sets a real commit_sha on an attestation, is every real commit; the bundle is never exportable for production work."
      },
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "quote": "let bundle = serde_json::json!({",
        "detail": "The VerificationBundle (attestation_chain + trust_anchors + git_diff + timestamp) is assembled only inside this export endpoint — the only place the bundle exists at all."
      },
      {
        "file": "crates/gyre-server/src/api/aibom.rs",
        "quote": "pub struct ReleaseCommit {",
        "detail": "The release artifact's per-commit entry carries only chain_attestation (chain JSON: attestation_id, chain_depth, chain nodes) — it never includes the VerificationBundle (trust anchors, git diff, timestamp), so 'attached to releases alongside the AIBOM' has no implementation in the release path."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "commit_sha: String::new(),",
        "detail": "Derived attestations (like the approval root) carry an empty commit_sha and are never updated with the pushed commit, so the commit index the export depends on is never populated in production."
      }
    ]
  }
]
