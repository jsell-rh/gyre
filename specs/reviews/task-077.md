# Review: TASK-077 — HSI Trust Gradient

## R1 Findings

- [-] [process-revision-complete] **F1: Silent policy creation error swallowing in `create_workspace` — `let _ =` on async operation.**
  `crates/gyre-server/src/api/workspaces.rs` (commit `1d599ac9`), in the `create_workspace` handler, lines after workspace creation:
  ```rust
  for policy in initial_policies {
      let _ = state.policies.create(&policy).await;
  }
  ```
  The `let _ =` discards the `Result` from `policies.create()`. If the policy store fails (DB connection error, constraint violation), the workspace is created and returned as `Supervised` — but the `trust:require-human-mr-review` policy does not exist. The merge processor is then unblocked in a workspace the user believes is Supervised. This silently violates the trust invariant. The handler should propagate the error (using `?` or `.map_err(...)`) so the caller gets an error response instead of a silently broken workspace.

- [-] [process-revision-complete] **F2: Workspace creation policy seeding is not atomic with workspace creation.**
  `crates/gyre-server/src/api/workspaces.rs`, `create_workspace` handler: the workspace is created via `state.workspaces.create(&ws).await?`, then trust policies are created in a separate loop of individual `state.policies.create()` calls. If policy creation fails mid-loop, the workspace exists without all its trust policies. The `update_workspace` handler correctly uses `apply_trust_transition` (single DB transaction) for trust transitions. The `create_workspace` handler should use the same atomic approach for initial policy seeding — either calling `apply_trust_transition` or a similar transactional method — to prevent partially-initialized workspaces.

- [-] [process-revision-complete] **F3: Double workspace write — `apply_trust_transition` and `workspaces.update` both write `trust_level`, creating a partial-update failure window.**
  `crates/gyre-server/src/api/workspaces.rs`, `update_workspace` handler: `apply_trust_transition` atomically commits `trust_level` + policy changes (step 1), then `state.workspaces.update(&ws)` writes ALL workspace fields including `trust_level` again (step 2). If step 1 succeeds but step 2 fails (e.g., DB error on the second write), the trust transition is committed but other field changes (name, description, budget) are lost. The handler returns an error, so the user thinks the entire update failed — but the trust level was already changed. On retry, the trust_level change is a no-op (same value), but the user doesn't know the trust transition succeeded on the first attempt. Fix: either (a) move all workspace field writes into the `apply_trust_transition` transaction, or (b) perform `state.workspaces.update()` first (for non-trust fields), then `apply_trust_transition` last — so if the trust transition fails, the 409 is returned cleanly and the user can retry with the non-trust changes already persisted.

- [-] [process-revision-complete] **F4: No test for the 409 error path on failed trust transition.**
  Acceptance criterion: "409 returned on failed trust transition." The code maps `apply_trust_transition` errors to `ApiError::Conflict` (which returns HTTP 409), but no test exercises this path. The `MemPolicyRepository::apply_trust_transition` always returns `Ok(())` — there is no mechanism to make it fail in tests. Without a test, the 409 status code behavior is unverified. At minimum, add a test that verifies the error mapping produces a 409 status code, or document why the in-memory adapter cannot simulate transaction failure.
