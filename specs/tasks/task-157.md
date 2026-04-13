---
title: "Implement meta-spec rollout policies"
spec_ref: "meta-spec-reconciliation.md §7"
depends_on: [task-156]
progress: not-started
coverage_sections:
  - "meta-spec-reconciliation.md §7 Rollout Policy"
commits: []
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §7 — Rollout Policy:

> Workspace admins configure rollout policies for meta-spec changes:
>
> | Strategy | Behavior |
> |---|---|
> | `immediate` | All repos reconciled simultaneously on approval |
> | `rolling` | Repos reconciled in batches (configurable batch size and delay) |
> | `manual` | Repos reconciled one-at-a-time on admin trigger |
>
> Default: `immediate` for workspaces with ≤10 repos, `rolling` for larger workspaces.

## Implementation Plan

1. **Add RolloutPolicy type** (`crates/gyre-domain/src/meta_spec.rs`):
   ```rust
   pub enum RolloutStrategy {
       Immediate,
       Rolling { batch_size: u32, delay_secs: u64 },
       Manual,
   }

   pub struct RolloutPolicy {
       pub strategy: RolloutStrategy,
       pub auto_strategy_threshold: u32,  // default 10 — auto-select based on repo count
   }
   ```

2. **Store rollout policy per workspace**:
   - Add `rollout_policy` field to workspace or meta-spec set configuration
   - Migration to add `rollout_policy_json` column to workspaces or meta_spec_sets table
   - Default: `{ "strategy": "immediate", "auto_strategy_threshold": 10 }`

3. **Integrate with reconciliation controller** (task-156):
   - When reconciliation is triggered, read the workspace's rollout policy
   - For `immediate`: create all reconciliation tasks at once (current behavior)
   - For `rolling`: create tasks in batches, with a delay between batches (use a scheduled follow-up job)
   - For `manual`: create tasks only for repos explicitly selected by the admin

4. **Add API endpoints for rollout policy**:
   - `GET /api/v1/workspaces/{id}/meta-specs/rollout-policy` — get current policy
   - `PUT /api/v1/workspaces/{id}/meta-specs/rollout-policy` — update policy (Admin only)

5. **Tests**:
   - Unit test: immediate strategy creates all tasks at once
   - Unit test: rolling strategy creates tasks in batches
   - Unit test: manual strategy creates no tasks without explicit trigger
   - Unit test: auto-selection picks immediate for ≤10 repos, rolling for >10

## Acceptance Criteria

- [ ] RolloutPolicy with Immediate, Rolling, Manual strategies
- [ ] Workspace-level rollout policy stored in database
- [ ] Reconciliation controller respects rollout policy
- [ ] Rolling strategy creates tasks in configurable batches with delay
- [ ] API endpoints for get/set rollout policy
- [ ] Default auto-selection based on repo count threshold
- [ ] Tests cover all 3 strategies

## Agent Instructions

- Read task-156 for the reconciliation controller this task integrates with
- Read `crates/gyre-domain/src/meta_spec.rs` for existing MetaSpec types
- Read `crates/gyre-server/src/api/meta_specs.rs` for meta-spec API patterns
- The rolling strategy needs a follow-up mechanism — use the job framework to schedule delayed batch processing
- Hexagonal architecture: RolloutPolicy goes in `gyre-domain`, rollout logic in server
