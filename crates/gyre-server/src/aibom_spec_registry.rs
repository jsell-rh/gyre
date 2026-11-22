//! Spec-registry section of the AIBOM (spec-registry.md §Supply Chain).
//!
//! When generating an AIBOM for a release, the spec registry contributes the
//! manifest state (spec-registry.md §Supply Chain / supply-chain.md §AIBOM):
//!
//! - every spec in the repo's ledger with its approval status
//!   (approved / pending / deprecated / revoked / rejected),
//! - the approval chain for approved specs (who approved, when, as what),
//! - a coverage summary (how many specs are approved vs pending),
//! - drift risk (specs whose ledger drift status is "drifted"),
//! - the manifest version (git blob SHA of `specs/manifest.yaml` at HEAD),
//! - the spec-to-code binding status: which MRs carried a `spec_ref` and
//!   whether the referenced spec version had an active approval at merge
//!   time — the same ledger the merge gate (`verify_spec_ref`) enforces.
//!
//! The section is shared by `GET /api/v1/repos/:id/aibom` (embedded in the
//! AIBOM response) and `POST /api/v1/release/prepare` (embedded in the
//! release metadata), so a release artifact and the standalone AIBOM can
//! never disagree about spec provenance.

use gyre_domain::{
    ApprovalStatus, MergeRequest, MrStatus, Repository, SpecApproval, SpecLedgerEntry,
};
use serde::Serialize;

use crate::AppState;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// The `spec_registry` section of an AIBOM / release metadata.
#[derive(Debug, Clone, Serialize)]
pub struct AibomSpecRegistry {
    pub repo_id: String,
    /// Git blob SHA of `specs/manifest.yaml` at the repository HEAD.
    /// `None` when the repo has no manifest at HEAD — reported as absent,
    /// never fabricated.
    pub manifest_version: Option<String>,
    pub total_specs: usize,
    pub approved: usize,
    pub pending: usize,
    pub deprecated: usize,
    pub revoked: usize,
    pub rejected: usize,
    /// Specs flagged as risk: ledger drift status "drifted"
    /// (spec-registry.md §Supply Chain: "Which specs had open drift-review
    /// tasks (flagged as risk)").
    pub drifted: usize,
    /// One entry per spec in the repo's ledger, sorted by path.
    pub specs: Vec<AibomSpecEntry>,
    /// Spec-to-code bindings: every MR in this repo that carried a
    /// `spec_ref`, with whether the pinned spec version was approved at
    /// merge time.
    pub spec_code_bindings: Vec<AibomSpecCodeBinding>,
    pub bindings_summary: AibomBindingsSummary,
}

/// Ledger state for a single spec.
#[derive(Debug, Clone, Serialize)]
pub struct AibomSpecEntry {
    pub path: String,
    pub title: String,
    pub owner: String,
    /// Ledger approval status: "approved" | "pending" | "deprecated" |
    /// "revoked" | "rejected".
    pub status: String,
    /// Approval mode from the manifest (e.g. "human_only",
    /// "human_and_agent").
    pub approval_mode: String,
    /// Drift status: "clean" | "drifted" | "unknown".
    pub drift_status: String,
    /// Git blob SHA of the spec file at HEAD (empty when unresolvable).
    pub current_sha: String,
    /// Earliest active approval timestamp for the current spec version
    /// (approved specs only).
    pub approved_at: Option<u64>,
    /// Approver identities of the active approval chain for the current
    /// version (approved specs only).
    pub approved_by: Vec<String>,
    /// Full approval chain — who approved, when, and as what (human/agent
    /// persona) — for the current version (approved specs only).
    pub approval_chain: Vec<AibomSpecApproval>,
}

/// One link in a spec's approval chain.
#[derive(Debug, Clone, Serialize)]
pub struct AibomSpecApproval {
    pub approver_id: String,
    /// "human" or "agent".
    pub approver_type: String,
    /// Agent persona (null for human approvers).
    pub persona: Option<String>,
    pub approved_at: u64,
}

/// Spec-to-code binding status for one MR.
#[derive(Debug, Clone, Serialize)]
pub struct AibomSpecCodeBinding {
    pub mr_id: String,
    /// The MR's raw `spec_ref` ("path@sha").
    pub spec_ref: String,
    /// Path portion of the spec_ref (ledger path form).
    pub spec_path: String,
    /// SHA portion of the spec_ref (empty when the ref carries no pinned
    /// version).
    pub spec_sha: String,
    /// MR status: "open" | "approved" | "merged" | "closed" | "reverted".
    pub mr_status: String,
    /// Merge timestamp — the MR's `updated_at` at the Merged transition.
    /// `None` while the MR is unmerged, and for merged-then-reverted MRs
    /// whose record no longer retains the merge moment.
    pub merged_at: Option<u64>,
    /// Whether the pinned spec version had an active approval at merge
    /// time (the check `verify_spec_ref` performs when the merge gate
    /// runs). `Some(true)` when a ledger row was determinably active at
    /// the merge moment; `Some(false)` when the rows determinably show no
    /// active approval then (granted after the merge, or withdrawn at or
    /// before it); `None` when it cannot be determined (MR not merged,
    /// merged-then-reverted, no pinned version, or the relevant approvals
    /// were withdrawn AFTER the merge — withdrawal clears `approved_at`
    /// (mutual exclusivity), so a post-merge withdrawal leaves no record
    /// of whether the grant preceded the merge).
    pub approved_at_merge_time: Option<bool>,
}

/// Summary of the spec-to-code bindings.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AibomBindingsSummary {
    /// MRs in this repo that carried a spec_ref.
    pub total: usize,
    /// MRs that merged (or merged-then-reverted).
    pub merged: usize,
    /// Merged MRs whose pinned spec version was approved at merge time.
    pub approved_at_merge_time: usize,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build the `spec_registry` AIBOM section for one repository.
///
/// Reads real state: the spec ledger (synced from `specs/manifest.yaml` on
/// push), the approval event history, the per-SHA approval ledger the merge
/// gate enforces, the repo's MRs, and git for the manifest version. Storage
/// failures propagate — an AIBOM must not silently under-report spec
/// provenance.
pub async fn build_spec_registry_section(
    state: &AppState,
    repo: &Repository,
) -> anyhow::Result<AibomSpecRegistry> {
    let repo_id = repo.id.to_string();

    // Manifest version: git blob SHA of specs/manifest.yaml at HEAD.
    // `None` (absent manifest, empty repo, or no git directory) is reported
    // as-is; it is a real "no manifest" signal, not a lookup failure being
    // papered over.
    let manifest_version =
        crate::git_refs::resolve_blob_sha(&repo.path, "specs/manifest.yaml").await;

    // Ledger entries owned by this repo. The ledger sync records the pushing
    // repo on every entry; entries belonging to other repos (or to no repo)
    // are not this release's spec state.
    let entries: Vec<SpecLedgerEntry> = state
        .spec_ledger
        .list_all()
        .await?
        .into_iter()
        .filter(|e| e.repo_id.as_deref() == Some(repo_id.as_str()))
        .collect();

    let mut specs: Vec<AibomSpecEntry> = Vec::with_capacity(entries.len());
    let (mut approved, mut pending, mut deprecated, mut revoked, mut rejected, mut drifted) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);

    for entry in entries {
        match entry.approval_status {
            ApprovalStatus::Approved => approved += 1,
            ApprovalStatus::Pending => pending += 1,
            ApprovalStatus::Deprecated => deprecated += 1,
            ApprovalStatus::Revoked => revoked += 1,
            ApprovalStatus::Rejected => rejected += 1,
        }
        if entry.drift_status == "drifted" {
            drifted += 1;
        }

        // Approval chain for approved specs: the active approval events for
        // the current version. An approved spec with no events (e.g.
        // `requires_approval: false`) reports an empty chain — that is the
        // honest ledger state, not a missing one.
        let (approved_at, approved_by, approval_chain) =
            if entry.approval_status == ApprovalStatus::Approved {
                let mut active: Vec<_> = state
                    .spec_approval_history
                    .list_by_path(&entry.path)
                    .await?
                    .into_iter()
                    .filter(|e| e.spec_sha == entry.current_sha && e.is_active())
                    .collect();
                active.sort_by_key(|e| e.approved_at);
                let approved_at = active.first().map(|e| e.approved_at);
                let approved_by = active
                    .iter()
                    .map(|e| e.approver_id.clone())
                    .collect::<Vec<_>>();
                let chain = active
                    .into_iter()
                    .map(|e| AibomSpecApproval {
                        approver_id: e.approver_id,
                        approver_type: e.approver_type,
                        persona: e.persona,
                        approved_at: e.approved_at,
                    })
                    .collect::<Vec<_>>();
                (approved_at, approved_by, chain)
            } else {
                (None, Vec::new(), Vec::new())
            };

        specs.push(AibomSpecEntry {
            path: entry.path.clone(),
            title: entry.title,
            owner: entry.owner,
            status: entry.approval_status.to_string(),
            approval_mode: entry.approval_mode,
            drift_status: entry.drift_status,
            current_sha: entry.current_sha,
            approved_at,
            approved_by,
            approval_chain,
        });
    }
    specs.sort_by(|a, b| a.path.cmp(&b.path));

    // Spec-to-code bindings: every MR in this repo that declared a spec_ref.
    let bindings = build_code_bindings(state, repo).await?;
    let bindings_total = bindings.len();
    let merged = bindings
        .iter()
        .filter(|b| matches!(b.mr_status.as_str(), "merged" | "reverted"))
        .count();
    let approved_at_merge = bindings
        .iter()
        .filter(|b| b.approved_at_merge_time == Some(true))
        .count();

    Ok(AibomSpecRegistry {
        repo_id,
        manifest_version,
        total_specs: specs.len(),
        approved,
        pending,
        deprecated,
        revoked,
        rejected,
        drifted,
        specs,
        spec_code_bindings: bindings,
        bindings_summary: AibomBindingsSummary {
            total: bindings_total,
            merged,
            approved_at_merge_time: approved_at_merge,
        },
    })
}

/// Build the spec-to-code binding list for one repo: every MR with a
/// `spec_ref`, and whether the pinned spec version had an active approval
/// at merge time.
async fn build_code_bindings(
    state: &AppState,
    repo: &Repository,
) -> anyhow::Result<Vec<AibomSpecCodeBinding>> {
    let mrs: Vec<MergeRequest> = state.merge_requests.list_by_repo(&repo.id).await?;

    let mut bindings = Vec::new();
    for mr in mrs {
        let Some(spec_ref) = mr.spec_ref.clone() else {
            continue;
        };

        // spec_ref format is "path@sha" (validated at MR creation); a bare
        // path pins no version, so merge-time approval is not determinable.
        let (spec_path, spec_sha) = match spec_ref.rsplit_once('@') {
            Some((p, s)) => (p.to_string(), s.to_string()),
            None => (spec_ref.clone(), String::new()),
        };

        let mr_status = mr_status_label(&mr.status);
        // The merge processor stamps `updated_at` with the merge moment when
        // it transitions an MR to Merged, so for Merged MRs that IS the merge
        // time. A Reverted MR's `updated_at` was overwritten at revert time —
        // the MR record no longer retains the merge moment, so it is reported
        // as indeterminable rather than evaluated against the wrong
        // timestamp (which would misjudge approvals revoked between merge
        // and revert).
        let merged_at = if mr.status == MrStatus::Merged {
            Some(mr.updated_at)
        } else {
            None
        };

        // Merge-time approval: the same per-SHA ledger the merge gate checks
        // (`verify_spec_ref`), evaluated three-valued — see
        // `merge_time_approval`. Withdrawal (revoked/rejected) clears
        // `approved_at` on the ledger row, so a withdrawal AFTER the merge
        // makes merge-time approval indeterminable rather than false.
        let approved_at_merge_time = match (mr.status, merged_at, spec_sha.as_str()) {
            (MrStatus::Merged, Some(merged_ts), sha) if !sha.is_empty() => {
                let rows = state
                    .spec_approvals
                    .find_by_spec_sha(&spec_path, sha)
                    .await?;
                merge_time_approval(&rows, merged_ts)
            }
            _ => None,
        };

        bindings.push(AibomSpecCodeBinding {
            mr_id: mr.id.to_string(),
            spec_ref,
            spec_path,
            spec_sha,
            mr_status: mr_status.to_string(),
            merged_at,
            approved_at_merge_time,
        });
    }
    bindings.sort_by(|a, b| a.mr_id.cmp(&b.mr_id));
    Ok(bindings)
}

fn mr_status_label(status: &MrStatus) -> &'static str {
    match status {
        MrStatus::Open => "open",
        MrStatus::Approved => "approved",
        MrStatus::Merged => "merged",
        MrStatus::Closed => "closed",
        MrStatus::Reverted => "reverted",
    }
}

/// Evaluate whether any ledger row was active for the pinned spec version
/// at the merge moment — three-valued:
///
/// - `Some(true)` — some row was determinably active at `merged_ts`
///   (granted at or before it, and not withdrawn at or before it);
/// - `Some(false)` — the rows determinably show no active approval then
///   (every row was granted after the merge, withdrawn at or before it,
///   or never approved);
/// - `None` — indeterminable.
///
/// Why `None` exists: `SpecApproval::revoke()`/`reject()` clear
/// `approved_at` (mutual exclusivity, gyre-domain/src/spec_approval.rs), so
/// a row withdrawn AFTER the merge retains no grant time — it may have
/// been active when the gate ran (granted before, withdrawn after) or
/// never active (granted after the merge, then withdrawn). Reporting
/// `Some(false)` there would misreport every MR pinned to a spec version
/// whose approval was later withdrawn — the systematic case, since every
/// push that modifies a spec file auto-revokes that path's approvals
/// (git_http.rs), and `POST /specs/:path/revoke` exists. A withdrawn row
/// proves non-approval at merge only when the withdrawal itself happened
/// at or before the merge (granted ≤ withdrawn ≤ merged).
fn merge_time_approval(rows: &[SpecApproval], merged_ts: u64) -> Option<bool> {
    let mut indeterminate = false;
    for a in rows {
        // Withdrawn row: the withdrawal timestamp is the only evidence
        // left (the transition cleared approved_at).
        if let Some(withdrawn_at) = a.revoked_at.or(a.rejected_at) {
            if withdrawn_at > merged_ts {
                // Withdrawn after the merge: grant time unknown — this row
                // may or may not have been active when the gate ran.
                indeterminate = true;
            }
            // Withdrawn at or before the merge: granted ≤ withdrawn ≤
            // merged_ts, so this row was not active at the merge —
            // determinably false; keep scanning the other rows.
            continue;
        }
        // Active or pending row: approved_at carries the grant time.
        match a.approved_at {
            Some(granted_at) if granted_at <= merged_ts => return Some(true),
            // Granted after the merge, or never granted (pending): not
            // active at the merge — determinably false for this row.
            _ => continue,
        }
    }
    if indeterminate {
        None
    } else {
        Some(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;
    use gyre_domain::{MergeRequest, SpecApproval, SpecApprovalEvent, SpecLedgerEntry};

    /// A 40-char hex SHA standing in for a git blob (approval-ledger rows
    /// key on the exact string; validity of the hex form is what matters).
    fn sha(tag: u8) -> String {
        format!("{tag:040x}")
    }

    fn ledger_entry(path: &str, status: ApprovalStatus, repo_id: &str) -> SpecLedgerEntry {
        SpecLedgerEntry {
bindings_sha: String::new(),
            path: path.to_string(),
            title: format!("Spec {path}"),
            owner: "owner-1".to_string(),
            kind: None,
            current_sha: sha(path.len() as u8),
            approval_mode: "human_and_agent".to_string(),
            approval_status: status,
            linked_tasks: Vec::new(),
            linked_mrs: Vec::new(),
            drift_status: "clean".to_string(),
            created_at: 100,
            updated_at: 100,
            repo_id: Some(repo_id.to_string()),
            workspace_id: None,
        }
    }

    fn approval_event(path: &str, sha_val: &str, approver: &str, at: u64) -> SpecApprovalEvent {
        SpecApprovalEvent {
            id: format!("evt-{path}-{approver}-{at}"),
            spec_path: path.to_string(),
            spec_sha: sha_val.to_string(),
            approver_type: if approver.starts_with("agent:") {
                "agent".to_string()
            } else {
                "human".to_string()
            },
            approver_id: approver.to_string(),
            persona: if approver.starts_with("agent:") {
                Some("reviewer".to_string())
            } else {
                None
            },
            attestation_level: if approver.starts_with("agent:") {
                Some(2)
            } else {
                None
            },
            stack_hash: None,
            approved_at: at,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
        }
    }

    /// Create an approved per-SHA ledger row (the store the merge gate
    /// consults) for `path@sha`, granted at `at`.
    async fn seed_sha_approval(state: &AppState, path: &str, sha_val: &str, at: u64) -> Id {
        let id = Id::new(format!("appr-{path}-{sha_val}-{at}"));
        let row = SpecApproval::new(id.clone(), path, sha_val, "user:alice");
        state.spec_approvals.create(&row).await.unwrap();
        state
            .spec_approvals
            .approve(&id, at)
            .await
            .expect("approve transition");
        id
    }

    fn mr(repo_id: &str, n: u8) -> MergeRequest {
        MergeRequest::new(
            Id::new(format!("mr-{n}")),
            Id::new(repo_id),
            Id::new("ws-1"),
            format!("MR {n}"),
            format!("feat/x{n}"),
            "main",
            10,
        )
    }

    /// Merge an MR at timestamp `at` (Open → Approved → Merged, with
    /// updated_at = the merge moment, exactly like the merge processor).
    async fn merge_mr(state: &AppState, mut m: MergeRequest, at: u64) {
        m.transition_status(gyre_domain::MrStatus::Approved)
            .unwrap();
        m.transition_status(gyre_domain::MrStatus::Merged).unwrap();
        m.updated_at = at;
        state.merge_requests.update(&m).await.unwrap();
    }

    fn make_repo(id: &str, path: &str) -> Repository {
        Repository::new(Id::new(id), Id::new("ws-1"), "repo", path, 1)
    }

    fn entry_for<'a>(section: &'a AibomSpecRegistry, path: &str) -> &'a AibomSpecEntry {
        section
            .specs
            .iter()
            .find(|s| s.path == path)
            .unwrap_or_else(|| panic!("spec {path} missing from section"))
    }

    fn binding_for<'a>(section: &'a AibomSpecRegistry, mr_id: &str) -> &'a AibomSpecCodeBinding {
        section
            .spec_code_bindings
            .iter()
            .find(|b| b.mr_id == mr_id)
            .unwrap_or_else(|| panic!("binding for {mr_id} missing"))
    }

    #[tokio::test]
    async fn section_reports_every_ledger_status_and_approval_chain() {
        let state = crate::mem::test_state();
        let repo = make_repo("repo-a", "/nonexistent/repo-a");
        state.repos.create(&repo).await.unwrap();

        let approved_spec = ledger_entry("system/auth.md", ApprovalStatus::Approved, "repo-a");
        let auth_sha = approved_spec.current_sha.clone();
        state.spec_ledger.save(&approved_spec).await.unwrap();
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/wip.md",
                ApprovalStatus::Pending,
                "repo-a",
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/old.md",
                ApprovalStatus::Deprecated,
                "repo-a",
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/pulled.md",
                ApprovalStatus::Revoked,
                "repo-a",
            ))
            .await
            .unwrap();
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/declined.md",
                ApprovalStatus::Rejected,
                "repo-a",
            ))
            .await
            .unwrap();
        // Drifted spec must be flagged as risk.
        let mut drifted = ledger_entry("system/drift.md", ApprovalStatus::Pending, "repo-a");
        drifted.drift_status = "drifted".to_string();
        state.spec_ledger.save(&drifted).await.unwrap();

        // Another repo's entries must NOT leak into this repo's section.
        state
            .spec_ledger
            .save(&ledger_entry(
                "system/other-repo.md",
                ApprovalStatus::Approved,
                "repo-b",
            ))
            .await
            .unwrap();

        // Approval chain for the approved spec: human then agent, in time
        // order, for the CURRENT version. A stale event (different SHA) and
        // a revoked event must be excluded from the chain.
        state
            .spec_approval_history
            .record(&approval_event(
                "system/auth.md",
                &auth_sha,
                "user:alice",
                500,
            ))
            .await
            .unwrap();
        state
            .spec_approval_history
            .record(&approval_event(
                "system/auth.md",
                &auth_sha,
                "agent:worker-1",
                600,
            ))
            .await
            .unwrap();
        state
            .spec_approval_history
            .record(&approval_event(
                "system/auth.md",
                &sha(99),
                "user:stale",
                700,
            ))
            .await
            .unwrap();
        let mut revoked = approval_event("system/auth.md", &auth_sha, "user:bob", 550);
        revoked.revoked_at = Some(800);
        state.spec_approval_history.record(&revoked).await.unwrap();

        let section = build_spec_registry_section(&state, &repo).await.unwrap();

        assert_eq!(section.repo_id, "repo-a");
        // No git directory → manifest version is reported absent, not fabricated.
        assert!(section.manifest_version.is_none());
        assert_eq!(section.total_specs, 6);
        assert_eq!(section.approved, 1);
        assert_eq!(section.pending, 2);
        assert_eq!(section.deprecated, 1);
        assert_eq!(section.revoked, 1);
        assert_eq!(section.rejected, 1);
        assert_eq!(section.drifted, 1);

        let auth = entry_for(&section, "system/auth.md");
        assert_eq!(auth.status, "approved");
        assert_eq!(auth.approval_mode, "human_and_agent");
        assert_eq!(auth.approved_at, Some(500));
        assert_eq!(auth.approved_by, vec!["user:alice", "agent:worker-1"]);
        assert_eq!(auth.approval_chain.len(), 2);
        assert_eq!(auth.approval_chain[0].approver_id, "user:alice");
        assert_eq!(auth.approval_chain[0].approver_type, "human");
        assert_eq!(auth.approval_chain[0].approved_at, 500);
        assert_eq!(auth.approval_chain[1].approver_id, "agent:worker-1");
        assert_eq!(auth.approval_chain[1].approver_type, "agent");
        assert_eq!(auth.approval_chain[1].persona.as_deref(), Some("reviewer"));

        // Non-approved specs carry no chain.
        let wip = entry_for(&section, "system/wip.md");
        assert_eq!(wip.status, "pending");
        assert_eq!(wip.approved_at, None);
        assert!(wip.approved_by.is_empty());

        let drift = entry_for(&section, "system/drift.md");
        assert_eq!(drift.drift_status, "drifted");

        // Cross-repo isolation.
        assert!(section
            .specs
            .iter()
            .all(|s| s.path != "system/other-repo.md"));
    }

    #[tokio::test]
    async fn bindings_report_merge_time_approval_truthfully() {
        let state = crate::mem::test_state();
        let repo = make_repo("repo-b", "/nonexistent/repo-b");
        state.repos.create(&repo).await.unwrap();

        let path = "system/auth.md";
        let v1 = sha(1);
        let v2 = sha(2);

        // mr-1: merged at t=1000; spec approved at t=900 → approved at merge.
        let mut m1 = mr("repo-b", 1);
        m1.spec_ref = Some(format!("{path}@{v1}"));
        state.merge_requests.create(&m1).await.unwrap();
        seed_sha_approval(&state, path, &v1, 900).await;
        merge_mr(&state, m1, 1000).await;

        // mr-2: merged at t=1000; spec approved only at t=1100 (after the
        // merge) → NOT approved at merge time, even though it is approved
        // now. This is the case a "currently approved" check would get wrong.
        let mut m2 = mr("repo-b", 2);
        m2.spec_ref = Some(format!("{path}@{v2}"));
        state.merge_requests.create(&m2).await.unwrap();
        seed_sha_approval(&state, path, &v2, 1100).await;
        merge_mr(&state, m2, 1000).await;

        // mr-3: merged at t=2000; spec approved at t=1500 but revoked at
        // t=1800 (before the merge) → NOT approved at merge time.
        let v3 = sha(3);
        let mut m3 = mr("repo-b", 3);
        m3.spec_ref = Some(format!("{path}@{v3}"));
        state.merge_requests.create(&m3).await.unwrap();
        let row = seed_sha_approval(&state, path, &v3, 1500).await;
        state
            .spec_approvals
            .revoke(&row, "user:alice", "content changed", 1800)
            .await
            .expect("revoke transition");
        merge_mr(&state, m3, 2000).await;

        // mr-4: approved at t=900 but the MR is still open → merge-time
        // approval is not determinable (None), not true.
        let v4 = sha(4);
        let mut m4 = mr("repo-b", 4);
        m4.spec_ref = Some(format!("{path}@{v4}"));
        state.merge_requests.create(&m4).await.unwrap();
        seed_sha_approval(&state, path, &v4, 900).await;

        // mr-5: merged, but carries no spec_ref → no binding at all.
        let m5 = mr("repo-b", 5);
        state.merge_requests.create(&m5).await.unwrap();
        merge_mr(&state, m5, 1000).await;

        // mr-6 in a different repo: must not appear in this repo's bindings.
        let mut m6 = mr("repo-z", 6);
        m6.spec_ref = Some(format!("{path}@{v1}"));
        state.merge_requests.create(&m6).await.unwrap();
        merge_mr(&state, m6, 1000).await;

        // mr-7: merged then reverted. The MR record's `updated_at` is the
        // revert time — the merge moment is no longer retained, so
        // merge-time approval must be reported as indeterminable (None),
        // never evaluated against the revert timestamp.
        let v5 = sha(5);
        let mut m7 = mr("repo-b", 7);
        m7.spec_ref = Some(format!("{path}@{v5}"));
        state.merge_requests.create(&m7).await.unwrap();
        seed_sha_approval(&state, path, &v5, 900).await;
        merge_mr(&state, m7, 1000).await;
        let mut reverted = state
            .merge_requests
            .find_by_id(&Id::new("mr-7"))
            .await
            .unwrap()
            .unwrap();
        reverted.revert("r".repeat(40), 3000).unwrap();
        state.merge_requests.update(&reverted).await.unwrap();

        // mr-8: approved at t=900, merged at t=1000, revoked at t=2000
        // (AFTER the merge — the systematic case: every push that modifies
        // the spec file auto-revokes its approvals). The approval WAS
        // active when the merge gate ran, but revoke() clears approved_at
        // (mutual exclusivity), so the grant time is gone from the row and
        // merge-time approval is indeterminable — it must NOT be reported
        // as Some(false).
        let v6 = sha(6);
        let mut m8 = mr("repo-b", 8);
        m8.spec_ref = Some(format!("{path}@{v6}"));
        state.merge_requests.create(&m8).await.unwrap();
        let row8 = seed_sha_approval(&state, path, &v6, 900).await;
        merge_mr(&state, m8, 1000).await;
        state
            .spec_approvals
            .revoke(&row8, "user:alice", "spec superseded", 2000)
            .await
            .expect("revoke transition");

        // mr-9: same shape via the reject path — approved at t=900,
        // merged at t=1000, rejected at t=2000 → indeterminable (None).
        let v7 = sha(7);
        let mut m9 = mr("repo-b", 9);
        m9.spec_ref = Some(format!("{path}@{v7}"));
        state.merge_requests.create(&m9).await.unwrap();
        let row9 = seed_sha_approval(&state, path, &v7, 900).await;
        merge_mr(&state, m9, 1000).await;
        state
            .spec_approvals
            .reject(&row9, "user:alice", "declined after review", 2000)
            .await
            .expect("reject transition");

        // mr-10: two rows for the same version — one still-active approval
        // granted at t=800 (before the merge) and one granted at t=900
        // then revoked at t=2000 (after the merge). The active row
        // determinably proves merge-time approval: Some(true) wins over
        // the indeterminate withdrawn row.
        let v8 = sha(8);
        let mut m10 = mr("repo-b", 10);
        m10.spec_ref = Some(format!("{path}@{v8}"));
        state.merge_requests.create(&m10).await.unwrap();
        seed_sha_approval(&state, path, &v8, 800).await;
        let row10 = seed_sha_approval(&state, path, &v8, 900).await;
        merge_mr(&state, m10, 1000).await;
        state
            .spec_approvals
            .revoke(&row10, "user:alice", "superseded by earlier approval", 2000)
            .await
            .expect("revoke transition");

        // mr-11: one row granted at t=1100 (after the merge, still active —
        // determinably not active at merge) and one withdrawn at t=2000
        // (after the merge, grant time unknown). No row proves activity at
        // the merge, and the withdrawn row leaves it undecidable → None,
        // not Some(false).
        let v9 = sha(9);
        let mut m11 = mr("repo-b", 11);
        m11.spec_ref = Some(format!("{path}@{v9}"));
        state.merge_requests.create(&m11).await.unwrap();
        seed_sha_approval(&state, path, &v9, 1100).await;
        let row11 = seed_sha_approval(&state, path, &v9, 900).await;
        merge_mr(&state, m11, 1000).await;
        state
            .spec_approvals
            .revoke(&row11, "user:alice", "superseded", 2000)
            .await
            .expect("revoke transition");

        let section = build_spec_registry_section(&state, &repo).await.unwrap();

        let b1 = binding_for(&section, "mr-1");
        assert_eq!(b1.mr_status, "merged");
        assert_eq!(b1.merged_at, Some(1000));
        assert_eq!(b1.approved_at_merge_time, Some(true));

        let b2 = binding_for(&section, "mr-2");
        assert_eq!(
            b2.approved_at_merge_time,
            Some(false),
            "approval granted AFTER the merge must not count as merge-time approval"
        );

        let b3 = binding_for(&section, "mr-3");
        assert_eq!(
            b3.approved_at_merge_time,
            Some(false),
            "approval revoked BEFORE the merge must not count as merge-time approval"
        );

        let b4 = binding_for(&section, "mr-4");
        assert_eq!(b4.mr_status, "open");
        assert_eq!(b4.merged_at, None);
        assert_eq!(b4.approved_at_merge_time, None);

        assert!(section.spec_code_bindings.iter().all(|b| b.mr_id != "mr-5"));
        assert!(section.spec_code_bindings.iter().all(|b| b.mr_id != "mr-6"));

        let b7 = binding_for(&section, "mr-7");
        assert_eq!(b7.mr_status, "reverted");
        assert_eq!(b7.merged_at, None);
        assert_eq!(
            b7.approved_at_merge_time, None,
            "merged-then-reverted MR must not be evaluated against the revert timestamp"
        );

        let b8 = binding_for(&section, "mr-8");
        assert_eq!(
            b8.approved_at_merge_time, None,
            "approval granted before the merge but revoked AFTER it is indeterminable, not Some(false)"
        );

        let b9 = binding_for(&section, "mr-9");
        assert_eq!(
            b9.approved_at_merge_time, None,
            "approval granted before the merge but rejected AFTER it is indeterminable, not Some(false)"
        );

        let b10 = binding_for(&section, "mr-10");
        assert_eq!(
            b10.approved_at_merge_time,
            Some(true),
            "a still-active approval granted before the merge proves merge-time approval even when a sibling row was withdrawn after the merge"
        );

        let b11 = binding_for(&section, "mr-11");
        assert_eq!(
            b11.approved_at_merge_time, None,
            "no determinable active-at-merge row plus a post-merge withdrawal is indeterminable, not Some(false)"
        );

        assert_eq!(section.bindings_summary.total, 9);
        assert_eq!(section.bindings_summary.merged, 8);
        assert_eq!(section.bindings_summary.approved_at_merge_time, 2);
    }

    #[tokio::test]
    async fn manifest_version_is_the_real_git_blob_sha() {
        let state = crate::mem::test_state();

        // Real git repo with a committed specs/manifest.yaml.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@gyre.dev"]);
        run(&["config", "user.name", "Test"]);
        std::fs::create_dir_all(path.join("specs")).unwrap();
        std::fs::write(path.join("specs/manifest.yaml"), "version: 1\nspecs: []\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "manifest"]);

        let repo = make_repo("repo-git", path.to_str().unwrap());
        state.repos.create(&repo).await.unwrap();

        let section = build_spec_registry_section(&state, &repo).await.unwrap();

        let expected = {
            let out = std::process::Command::new("git")
                .args(["rev-parse", "HEAD:specs/manifest.yaml"])
                .current_dir(path)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        assert_eq!(
            section.manifest_version.as_deref(),
            Some(expected.as_str()),
            "manifest_version must be the git blob SHA of specs/manifest.yaml at HEAD"
        );
        assert_eq!(section.total_specs, 0);
    }
}
