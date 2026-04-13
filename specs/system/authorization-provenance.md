# Authorization Provenance

Cryptographic proof that work was authorized, constrained to what was authorized, and verifiable without trusting the platform.

## The Problem

Gyre's current provenance chain is **referential**: spec approval records point to spec SHAs, MRs reference spec paths, merge attestation bundles record gate results, and the server signs the bundle. Every link depends on trusting the server to have enforced the chain correctly. A compromised server — or a compromised admin with database access — can forge any link.

The chain today:

```
spec@SHA (approved, recorded in ledger)
  → task (references spec_ref string)
    → agent (OIDC JWT, stack hash in claims)
      → commits (Sigstore signed)
        → gates passed (server asserts)
          → merge attestation (server signs)
```

This is a **single-authority attestation**: the server is the sole witness to every step. The multi-party signing in `agent-gates.md` improves this (gate agents sign their own results), but the fundamental problem remains: nothing constrains *what the agent actually commits* to be consistent with *what the spec authorized*. The spec says "add a payments endpoint"; the agent could modify auth middleware; the merge attestation records `spec_fully_approved: true` regardless.

Three specific gaps:

1. **No output constraints.** The spec approval authorizes work, but nothing mechanically limits what that work can touch. The accountability agent review is a best-effort check, not a cryptographic guarantee.

2. **No delegation provenance.** When an orchestrator spawns a sub-agent, the sub-agent's JWT proves it exists and is scoped to a task, but there is no cryptographic link between the orchestrator's authorization and the sub-agent's. A compromised orchestrator could grant a sub-agent permissions the human never authorized.

3. **No constraint propagation.** If a spec is updated (derived from a prior version), there is no mechanism to ensure the new spec's constraints are at least as strict as the prior's. A spec revision could silently drop a security constraint.

## Design Approach

Two orthogonal concerns, following the same decomposition as the delivery problem in fleet management:

- **Provenance** — cryptographic proof of *who* authorized *what*, verifiable without trusting the platform.
- **Constraints** — CEL predicates over the actual output (diff, committed files, manifest changes), evaluated at system boundaries (pre-accept, merge), that the output must satisfy to be accepted.

The platform assembles and routes attestations, but cannot forge them. A compromised server can deny service (refuse to merge) but cannot approve unauthorized work.

---

## 1 Trust Model

### 1.1 Trust Anchors

A trust anchor is an identity issuer the verification algorithm trusts to authenticate signers. Trust anchors are **external to the platform** — Gyre is never its own trust root for authorization provenance.

| Anchor | Identity type | What it proves | Example |
|---|---|---|---|
| **Tenant IdP** | Human user | A human with this identity authorized this work | Keycloak, Okta, Entra ID |
| **Gyre OIDC** | Agent workload | This agent was spawned with these claims on this infrastructure | `https://gyre.example.com` OIDC issuer |
| **Addon IdP** | External system | This external system (CI, scanner, linter) produced this result | GitHub Actions OIDC, Sigstore |

Each trust anchor is registered with:

```
TrustAnchor {
  id:           string          -- stable identifier (e.g., "tenant-keycloak")
  issuer:       string          -- OIDC issuer URL or SPIFFE trust domain
  jwks_uri:     string          -- public key endpoint
  anchor_type:  User | Agent | Addon
  constraints:  OutputConstraint[]  -- anchor-level constraints (§3.2)
}
```

Trust anchors are tenant-scoped. A workspace inherits its tenant's anchors. The platform cannot add, remove, or modify trust anchors without a human admin action — this is enforced by an immutable Deny ABAC policy (same pattern as `builtin:require-human-spec-approval`).

### 1.2 Residual Risk

Even with this design, a compromised platform can:
- **Deny service** (refuse to process legitimate attestations)
- **Replay valid attestations** to the wrong context (mitigated by context binding — §2.4)
- **Withhold constraint violations** from humans (mitigated by offline verification — §6.3)

A compromised platform **cannot**:
- Forge a user's signature on an authorization they didn't approve
- Drop constraints from a propagation chain
- Substitute manifests/diffs that violate signed constraints
- Attribute work to an agent that didn't produce it (if the agent signed its output)

---

## 2 Signed Input: The Authorization Root

Every chain of work authorization begins with a human signing a **Signed Input** — a cryptographic authorization that binds a spec approval to a set of output constraints.

### 2.1 When a Signed Input Is Created

A Signed Input is produced when a human approves a spec. The approval event in the spec ledger (`agent-gates.md`) is extended: in addition to recording `approver_id`, `spec_sha`, and `signature`, the approval now produces a Signed Input that travels with every task derived from that spec.

### 2.2 Structure

```
SignedInput {
  content:              InputContent
  output_constraints:   OutputConstraint[]
  valid_until:          timestamp
  expected_generation:  u32 | null
  signature:            Signature
  key_binding:          KeyBinding
}

InputContent {
  spec_path:            string          -- e.g., "specs/system/payments.md"
  spec_sha:             string          -- git blob SHA at approval time
  workspace_id:         string          -- scoping boundary
  repo_id:              string          -- target repository
  persona_constraints:  PersonaRef[]    -- required persona(s) for implementation
  meta_spec_set_sha:    string          -- hash of bound meta-spec set at approval time
  scope:                ScopeConstraint -- what parts of the repo this authorization covers
}
```

`ScopeConstraint` defines the file-level boundaries of what this authorization permits:

```
ScopeConstraint {
  allowed_paths:    glob[]    -- files the agent may modify (e.g., ["src/payments/**"])
  forbidden_paths:  glob[]    -- files the agent must not modify (e.g., ["src/auth/**"])
}
```

The approver may leave `allowed_paths` empty (meaning "any file"), but `forbidden_paths` are always enforced. Strategy-implied constraints (§3.2) may add further path restrictions.

### 2.3 Key Binding

The human does not sign with their IdP token directly (tokens are short-lived and opaque). Instead, they bind an ephemeral Ed25519 keypair to their identity:

```
KeyBinding {
  public_key:           bytes           -- Ed25519 public key
  user_identity:        string          -- subject claim from IdP JWT (e.g., "user:jsell")
  issuer:               string          -- IdP issuer URL
  trust_anchor_id:      string          -- which TrustAnchor authenticated this user
  issued_at:            timestamp
  expires_at:           timestamp
  user_signature:       bytes           -- user signs this binding document with the ephemeral key
  platform_countersign: bytes           -- platform countersigns (proves binding was registered)
}
```

**Key lifecycle:**
1. User authenticates to the platform via their tenant IdP (Keycloak, etc.).
2. Client generates an ephemeral Ed25519 keypair.
3. Client constructs the `KeyBinding` document, signs it with the private key.
4. Platform verifies the user's IdP session is valid, countersigns the binding, and stores the public key.
5. The private key remains client-side (browser, CLI). It signs `SignedInput` documents.
6. On expiry or logout, the binding is invalidated. A new session requires a new binding.

The `platform_countersign` proves the binding was registered at a specific time with a valid IdP session. It does NOT make the platform a trust root — verification checks the user's signature against the bound public key and the IdP's trust anchor, not the platform's countersignature. The countersignature is a timestamp witness, not an authority delegation.

### 2.4 Context Binding (Replay Prevention)

Each `SignedInput` is bound to a specific context:

- `workspace_id` + `repo_id` — the input cannot be replayed to a different repo.
- `spec_sha` — the input is bound to a specific spec version. A modified spec produces a different SHA, requiring a new approval and new `SignedInput`.
- `expected_generation` — optional monotonic counter. If present, the input is only valid for a specific generation of the deployment (task assignment). Prevents replay of an old authorization after a task has been reassigned.
- `valid_until` — hard expiry. After this time, the authorization is invalid regardless of other checks.

---

## 3 Output Constraints

An output constraint is a named CEL predicate that the actual output (diff, committed files) must satisfy at verification time.

### 3.1 Structure

```
OutputConstraint {
  name:         string      -- human-readable description
  expression:   string      -- CEL expression that must evaluate to true
}
```

### 3.2 Constraint Sources

Constraints come from three sources. All are additive — the full constraint set is the union of all three:

#### Source 1: Explicit User Constraints

The human approver includes constraints in the `SignedInput` at spec approval time. These are signed by the user's key and travel with the authorization.

Examples:

```cel
// Only modify files in the payments module
output.changed_files.all(f, f.startsWith("src/payments/"))

// Don't add new dependencies
output.changed_files.all(f, f != "Cargo.toml" && f != "Cargo.lock")

// Commit message must reference the spec
output.commit_message.contains("specs/system/payments.md")
```

Explicit constraints are optional. A spec approval with no explicit constraints means the user trusts the agent (and the strategy-implied constraints) to do the right thing.

#### Source 2: Strategy-Implied Constraints

The platform derives constraints mechanically from the `InputContent` at verification time. These are not signed by the user — they are derived from the signed content, which makes them tamper-proof (changing the content changes the derived constraints, but changing the content invalidates the user's signature).

**From `persona_constraints`:** If the signed input specifies persona requirements, the constraint verifies the implementing agent's JWT `persona` claim matches.

```cel
// Implied: agent must run the specified persona
agent.persona == input.persona_constraints[0].name
```

**From `meta_spec_set_sha`:** The meta-spec set active at approval time is recorded. The constraint verifies the implementing agent's meta-spec set matches.

```cel
// Implied: agent's meta-spec set must match what was approved
agent.meta_spec_set_sha == input.meta_spec_set_sha
```

**From `scope`:** Path constraints from the `ScopeConstraint` are converted to CEL predicates.

```cel
// Implied from allowed_paths: ["src/payments/**"]
output.changed_files.all(f, f.matches("^src/payments/.*"))

// Implied from forbidden_paths: ["src/auth/**"]
output.changed_files.all(f, !f.matches("^src/auth/.*"))
```

**From workspace trust level:** Workspaces with `Supervised` trust level imply stricter constraints than `Autonomous` workspaces. The trust level is recorded in the signed input content (via `workspace_id` → workspace config lookup at verification time).

**From attestation level policy:** If the repo's stack policy requires Level 3 attestation, the constraint verifies the agent's attestation level.

```cel
// Implied from repo stack policy
agent.attestation_level >= 3
```

Strategy-implied constraints are re-derived from the final computed content at verification time, not cached from signing time. This means updating a workspace's trust level or stack policy tightens constraints on all future verifications — even for existing authorizations.

#### Source 3: Gate Constraints

Quality gates (from `agent-gates.md`) can produce additional output constraints as part of their results. A gate agent that reviews code can attach constraints that subsequent gates or the merge processor must verify:

```
GateConstraint {
  gate_id:      string
  gate_name:    string
  constraint:   OutputConstraint
  signed_by:    Signature        -- gate agent signs its constraint
}
```

Gate constraints are verified at merge time alongside explicit and strategy-implied constraints. They are additive — a gate can tighten but never loosen the constraint set.

### 3.3 CEL Evaluation Context

At verification time, constraints are evaluated against a context assembled from the actual state of the work:

```
{
  "input": {
    "spec_path":          "specs/system/payments.md",
    "spec_sha":           "abc123",
    "workspace_id":       "ws-1",
    "repo_id":            "repo-1",
    "persona_constraints": [{"name": "security"}],
    "meta_spec_set_sha":  "def456",
    "scope": {
      "allowed_paths":    ["src/payments/**"],
      "forbidden_paths":  ["src/auth/**"]
    }
  },
  "output": {
    "changed_files":      ["src/payments/handler.rs", "src/payments/mod.rs"],
    "added_files":        ["src/payments/refund.rs"],
    "deleted_files":      [],
    "diff_stats": {
      "insertions":       142,
      "deletions":        7
    },
    "commit_message":     "feat(payments): add refund endpoint per specs/system/payments.md",
    "commit_sha":         "789abc"
  },
  "agent": {
    "id":                 "agent:worker-42",
    "persona":            "security",
    "stack_hash":         "sha256:...",
    "attestation_level":  3,
    "meta_spec_set_sha":  "def456",
    "spawned_by":         "user:jsell",
    "task_id":            "TASK-007",
    "container_id":       "abc123",
    "image_hash":         "sha256:..."
  },
  "target": {
    "repo_id":            "repo-1",
    "workspace_id":       "ws-1",
    "branch":             "main",
    "default_branch":     "main"
  },
  "action":               "push" | "merge"
}
```

The `output` fields are computed from the actual git diff at verification time — not self-reported by the agent. The platform computes the diff; the constraints evaluate against the platform's computation. This is safe even if the platform is compromised, because the constraints themselves are signed by the user and derived from signed content — a compromised platform would have to forge the user's signature to change them.

### 3.4 Constraint Evaluation: Fail Closed

Constraints are evaluated sequentially. The first failure stops evaluation and rejects the action.

```
for constraint in all_constraints:
    result = evaluate_cel(constraint.expression, context)
    if result is error:
        return REJECT("constraint evaluation error: {error}")
    if result is false:
        return REJECT("constraint failed: {constraint.name}")
return ACCEPT
```

CEL evaluation errors (malformed expressions, type errors, missing fields) are treated as failures. There is no "evaluation error → allow" path.

---

## 4 Derived Input: Delegation Provenance

When an orchestrator decomposes a task into sub-tasks and spawns sub-agents, each sub-agent's authorization is a **Derived Input** — a new authorization cryptographically linked to its parent.

### 4.1 Structure

```
DerivedInput {
  parent_ref:       bytes           -- content hash of the parent attestation
  preconditions:    string[]        -- CEL predicates that must hold on the parent's state
  update:           string          -- CEL expression defining what changed
  output_constraints: OutputConstraint[]  -- additional constraints (additive only)
  signature:        Signature       -- orchestrator signs the derivation
  key_binding:      KeyBinding      -- orchestrator's workload key binding
}
```

### 4.2 When a Derived Input Is Created

1. **Task decomposition:** The repo orchestrator receives a delegation task from the workspace orchestrator. It decomposes the task into sub-tasks. Each sub-task's authorization is derived from the parent task's `SignedInput`.

2. **Spec update:** A spec is modified and re-approved. The new approval's authorization is derived from the prior approval's `SignedInput`, carrying forward the prior's constraints.

3. **Re-spawn after gate failure:** When the Ralph loop re-spawns an agent after a gate failure, the re-spawn's authorization is derived from the original, with the gate failure's feedback as context. No constraints are dropped.

### 4.3 Constraint Propagation: Additive Only

The critical security property: derived constraints only grow.

```
derive_constraints(prior_constraints, update) -> new_constraints:
    additional = update.output_constraints
    if additional is empty:
        return prior_constraints            // unchanged
    return prior_constraints + additional   // additive
```

A derived input can **add** constraints (e.g., an orchestrator narrows a sub-agent's scope to a subset of files), but can never **remove** constraints established by the parent. This is enforced at the data structure level — the derivation function concatenates, never replaces.

### 4.4 Derivation Chain Verification

At verification time, the verifier walks the derivation chain back to the root `SignedInput`:

```
verify_chain(attestation):
    if attestation.input is SignedInput:
        return verify_signed_input(attestation)
    
    if attestation.input is DerivedInput:
        // 1. Verify this derivation's signature
        verify_signature(attestation.input.signature, attestation.input.key_binding)
        
        // 2. Verify the key binding (agent identity, trust anchor)
        verify_key_binding(attestation.input.key_binding)
        
        // 3. Load and verify the parent attestation
        parent = attestation_store.load(attestation.input.parent_ref)
        parent_result = verify_chain(parent)  // recursive
        if parent_result is REJECT:
            return REJECT("parent attestation invalid")
        
        // 4. Evaluate preconditions against parent's verified state
        for precondition in attestation.input.preconditions:
            if not evaluate_cel(precondition, parent.verified_state):
                return REJECT("precondition failed: {precondition}")
        
        // 5. Evaluate the update expression to derive new content
        derived_content = evaluate_cel(attestation.input.update, {
            "prior": parent.verified_content,
            "update": attestation.input.update_content
        })
        
        // 6. Verify derived content matches claimed content
        if hash(derived_content) != hash(attestation.claimed_content):
            return REJECT("derived content does not match")
        
        // 7. Accumulate constraints (parent + derived)
        all_constraints = parent.constraints + attestation.input.output_constraints
        
        return ACCEPT(content=derived_content, constraints=all_constraints)
```

### 4.5 Orchestrator Key Binding

Orchestrators sign derived inputs with a workload-bound key, not a user key. The key binding proves:

- The orchestrator is a specific agent (`agent:orchestrator-1`)
- Running on a specific compute target with a specific stack hash
- Spawned by a specific user for a specific task

This is the existing `AgentJwtClaims` structure extended to key binding:

```
OrchestratorKeyBinding = KeyBinding {
  public_key:       bytes
  user_identity:    agent JWT `sub` claim
  issuer:           Gyre OIDC issuer URL
  trust_anchor_id:  "gyre-oidc"
  issued_at:        JWT `iat`
  expires_at:       JWT `exp`
  // additional workload claims carried from JWT:
  task_id:          string
  spawned_by:       string      -- the human who initiated the chain
  stack_hash:       string
  persona:          string
}
```

The `spawned_by` field is critical — it links the orchestrator's key binding back to the human who approved the original spec, even though the orchestrator (not the human) is the immediate signer.

### 4.6 Depth Limits

Derivation chains can grow indefinitely in theory (orchestrator → sub-orchestrator → agent). In practice, the chain depth is bounded by:

- Workspace trust level: `Supervised` workspaces may limit chain depth to 2 (human → orchestrator → agent). `Autonomous` workspaces may allow deeper chains.
- The strategy-implied constraint `chain.depth <= N` is derived from workspace configuration.
- Each derivation adds overhead to verification (recursive chain walk). The verifier imposes a hard limit of 10 as a safety net.

---

## 5 Attestation: The Complete Record

An attestation packages the input (signed or derived), the output, and the verification result into a single record.

### 5.1 Structure

```
Attestation {
  id:               string          -- content-addressable hash of the attestation
  input:            SignedInput | DerivedInput
  output:           AttestationOutput
  metadata:         AttestationMetadata
}

AttestationOutput {
  content_hash:     bytes           -- hash of the actual output (diff, commit)
  commit_sha:       string          -- git commit SHA
  agent_signature:  Signature | null -- agent signs the output (if capable)
  gate_results:     GateAttestation[]
}

AttestationMetadata {
  created_at:       timestamp
  workspace_id:     string
  repo_id:          string
  task_id:          string
  agent_id:         string
  chain_depth:      u32             -- 0 for root SignedInput, increments per derivation
}

GateAttestation {
  gate_id:          string
  gate_name:        string
  gate_type:        GateType
  status:           GateStatus
  output_hash:      bytes           -- hash of gate output
  constraint:       GateConstraint | null
  signature:        Signature       -- gate agent or forge signs
  key_binding:      KeyBinding
}
```

### 5.2 Relationship to Existing Merge Attestation

The existing `MergeAttestation` / `AttestationBundle` (from `agent-gates.md`) is **subsumed** by this structure. The merge attestation becomes the final attestation in a chain:

```
SignedInput (human approves spec)
  → DerivedInput (orchestrator decomposes to sub-task)
    → Attestation (agent commits code, gates run)
      → Attestation (merge: all gates passed, constraints satisfied)
```

The merge attestation's existing fields map to the new structure:

| Existing field | New location |
|---|---|
| `mr_id` | `AttestationMetadata.task_id` (tasks subsume MR identity for provenance) |
| `merge_commit_sha` | `AttestationOutput.commit_sha` |
| `gate_results` | `AttestationOutput.gate_results` (now individually signed) |
| `spec_ref` | `SignedInput.content.spec_path` + `spec_sha` (at chain root) |
| `spec_fully_approved` | Derived: root `SignedInput` exists and is valid |
| `author_agent_id` | `AttestationMetadata.agent_id` |
| `conversation_sha` | `AttestationOutput` extended field (unchanged) |
| `completion_summary` | `AttestationOutput` extended field (unchanged) |
| `meta_specs_used` | `SignedInput.content.meta_spec_set_sha` (at chain root) |
| `signature` | Each attestation node is individually signed; bundle signature is the root's |
| `signing_key_id` | `KeyBinding.public_key` at each node |

The existing `AttestationBundle` type and `AttestationRepository` port are retained for backward compatibility during migration. New attestations are produced in both formats — the legacy bundle for existing consumers, the chain attestation for new verification.

### 5.3 Storage

Attestations are stored in three locations:

1. **Attestation store** (database) — indexed by `id` (content hash), `task_id`, `repo_id`, `workspace_id`. Supports chain traversal via `parent_ref`.

2. **Git notes** (`refs/notes/attestations`) — the chain attestation is attached to the relevant commit, same as the existing merge attestation bundle.

3. **AIBOM** — release-time aggregation includes the full chain for each commit, replacing the flat agent-attribution model.

### 5.4 Attestation Port

```rust
pub trait ChainAttestationRepository: Send + Sync {
    /// Store an attestation node.
    async fn save(&self, attestation: &Attestation) -> Result<()>;

    /// Load by content-addressable ID.
    async fn find_by_id(&self, id: &str) -> Result<Option<Attestation>>;

    /// Load the chain rooted at this attestation (walks parent_ref).
    async fn load_chain(&self, leaf_id: &str) -> Result<Vec<Attestation>>;

    /// Find attestations for a task.
    async fn find_by_task(&self, task_id: &str) -> Result<Vec<Attestation>>;

    /// Find attestations for a commit.
    async fn find_by_commit(&self, commit_sha: &str) -> Result<Option<Attestation>>;

    /// Find attestations for a repo within a time range.
    async fn find_by_repo(
        &self,
        repo_id: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<Attestation>>;
}
```

---

## 6 Verification

### 6.1 Verification Points

Verification occurs at two system boundaries:

**Pre-accept (push time):** When an agent pushes code, the forge verifies:
- The agent's JWT is valid and not revoked
- The agent has a valid attestation chain back to a `SignedInput`
- The pushed diff satisfies all accumulated constraints (explicit + strategy-implied)
- The agent's workload identity matches the key binding in the attestation chain

This is an extension of the existing pre-accept validation (`supply-chain.md` §2, `forge-advantages.md`). The stack attestation check is retained; the constraint check is added.

**Merge time:** When the merge processor executes a merge, it verifies:
- All quality gates passed (unchanged from `agent-gates.md`)
- The complete attestation chain is valid from root `SignedInput` to leaf
- All gate constraints (§3.2, Source 3) are satisfied
- The accumulated constraint set (explicit + strategy-implied + gate) evaluates to true against the final merged diff

### 6.2 Verification Algorithm

```
verify_attestation(attestation, context) -> VerificationResult:
    // Phase 1: Verify the input chain
    chain_result = verify_chain(attestation)  // §4.4
    if chain_result is REJECT:
        return chain_result
    
    // Phase 2: Collect all constraints
    explicit_constraints    = chain_result.constraints
    implied_constraints     = derive_strategy_constraints(chain_result.content)
    gate_constraints        = collect_gate_constraints(attestation.output.gate_results)
    all_constraints         = explicit_constraints + implied_constraints + gate_constraints
    
    // Phase 3: Build CEL context from actual output
    cel_context = build_cel_context(
        input   = chain_result.content,
        output  = compute_diff(attestation.output.commit_sha),
        agent   = resolve_agent(attestation.metadata.agent_id),
        target  = resolve_target(attestation.metadata.repo_id),
        action  = context.action,  // "push" or "merge"
    )
    
    // Phase 4: Evaluate all constraints
    for constraint in all_constraints:
        result = evaluate_cel(constraint.expression, cel_context)
        if result is error:
            return REJECT(
                constraint = constraint.name,
                reason     = "evaluation error: {error}",
            )
        if result is false:
            return REJECT(
                constraint = constraint.name,
                reason     = "predicate returned false",
            )
    
    // Phase 5: Verify output signatures (if present)
    if attestation.output.agent_signature is not null:
        verify_signature(
            attestation.output.agent_signature,
            attestation.output.content_hash,
        )
    
    for gate in attestation.output.gate_results:
        verify_signature(gate.signature, gate.output_hash)
    
    return ACCEPT(
        chain_depth     = attestation.metadata.chain_depth,
        constraints_evaluated = len(all_constraints),
        root_signer     = chain_result.root_signer,
    )
```

### 6.3 Offline Verification

Any party that trusts the relevant trust anchors (tenant IdP, Gyre OIDC issuer) can verify an attestation chain independently, without connecting to the Gyre server. The verification bundle includes:

```
VerificationBundle {
  attestation_chain:  Attestation[]     -- root to leaf
  trust_anchors:      TrustAnchor[]     -- public keys / JWKS
  git_diff:           bytes             -- the actual diff content
  timestamp:          timestamp         -- when the bundle was assembled
}
```

This bundle can be:
- Exported via `GET /api/v1/repos/{id}/attestations/{commit_sha}/bundle`
- Attached to releases alongside the AIBOM
- Verified by external compliance tools, auditors, or federated Gyre instances

### 6.4 Verification Result Storage

Every verification produces a `VerificationResult` tree stored for audit:

```
VerificationResult {
  label:      string              -- what was verified
  valid:      bool
  message:    string              -- human-readable explanation
  children:   VerificationResult[] -- sub-verifications
}
```

Verification results are:
- Logged to the audit system (`observability.md`)
- Attached to the attestation record
- Queryable via `GET /api/v1/repos/{id}/attestations/{commit_sha}/verification`

---

## 7 Integration with Existing Systems

### 7.1 Spec Approval (agent-gates.md)

**Amendment:** Spec approval (`POST /api/v1/specs/approve`) now produces a `SignedInput` in addition to the existing `SpecApproval` record. The approval endpoint accepts an optional `output_constraints` field and `scope` field:

```
POST /api/v1/specs/approve
{
  "path":               "specs/system/payments.md",
  "sha":                "abc123",
  "output_constraints": [
    {"name": "scope to payments", "expression": "output.changed_files.all(f, f.startsWith(\"src/payments/\"))"}
  ],
  "scope": {
    "allowed_paths":    ["src/payments/**"],
    "forbidden_paths":  ["src/auth/**", "migrations/**"]
  }
}
```

If `output_constraints` and `scope` are omitted, the `SignedInput` is created with only strategy-implied constraints. The human can always tighten via explicit constraints; the platform provides a baseline via strategy-implied constraints.

**Key exchange:** The approval endpoint requires the client to have a valid `KeyBinding` (§2.3). If the client does not have one, it must establish one first via `POST /api/v1/auth/key-binding`.

### 7.2 ABAC Policy Engine (abac-policy-engine.md)

**Amendment:** New resource type `attestation` with actions `verify`, `export`, `revoke`. New subject attributes:

| Attribute | Source | Description |
|---|---|---|
| `subject.chain_depth` | Attestation chain | How many derivation steps from the root |
| `subject.root_signer` | Root `SignedInput` | Who originally authorized the chain |
| `subject.constraint_count` | Accumulated constraints | How many constraints apply |

New built-in policy:

```
builtin:require-signed-authorization (Deny, immutable, priority 998)
  conditions: subject.type != "system" AND action IN ["push", "merge"]
  effect: Deny unless a valid attestation chain exists for the submitted work
```

This policy makes authorization provenance **mandatory** for all non-system pushes and merges. It is immutable — it cannot be overridden by an Allow policy, same as `builtin:require-human-spec-approval`.

### 7.3 Supply Chain (supply-chain.md)

**Amendment:** Stack attestation (§2 of supply-chain.md) is incorporated into the attestation chain as a strategy-implied constraint:

```cel
// Implied from repo's gyre-stack.lock
agent.stack_hash == "sha256:expected_fingerprint"
```

The stack attestation is no longer a separate check — it is one constraint among many, evaluated by the same verification algorithm. This unifies the two verification paths (stack attestation at push time + constraint verification at push time) into a single algorithm.

The AIBOM (§5 of supply-chain.md) is extended to include the full attestation chain per commit, replacing the flat `stack_attestation` field.

### 7.4 Agent Runtime (agent-runtime.md)

**Amendment:** Agent spawn (§1, Phase 4) now includes attestation chain setup:

1. System mints the agent's OIDC JWT (unchanged).
2. System creates a workload `KeyBinding` for the agent (new).
3. System creates a `DerivedInput` from the parent task's attestation, signed by the orchestrator's key (new).
4. The `DerivedInput` and `KeyBinding` are injected into the agent's environment (new).
5. Agent uses the `KeyBinding` to sign its output attestation at push time (new).

The agent's `KeyBinding` is ephemeral — it expires with the agent's JWT. A re-spawned agent (Ralph loop iteration) gets a new `KeyBinding` and a new `DerivedInput` derived from the same parent.

### 7.5 Message Bus (message-bus.md)

**Amendment:** New message kind:

```
ConstraintViolation {
  attestation_id:   string
  constraint_name:  string
  expression:       string
  context_snapshot: object    -- the CEL context at evaluation time
  action:           string    -- "push" or "merge"
  agent_id:         string
  repo_id:          string
  timestamp:        u64
}
```

Tier: **Event** (signed, TTL). Constraint violations are broadcast to the workspace and directed to the author agent. The human operator's inbox receives a notification (priority 2 — high but not critical, since the push/merge was already rejected).

### 7.6 Human-System Interface (human-system-interface.md)

**Amendment:** Spec approval UI gains constraint editing:

- The approval dialog shows strategy-implied constraints (derived from workspace config, trust level, stack policy) as read-only.
- The approver can add explicit constraints via a CEL expression editor with autocomplete for available context fields.
- The approver can define scope constraints via a file-tree picker (glob patterns).
- A dry-run button evaluates the constraint set against the current repo state to preview what would pass/fail.

The Explorer (§3) gains a provenance chain visualization:
- Clicking a commit shows its attestation chain as a directed graph.
- Each node shows: signer identity, constraint count, verification status.
- Failed constraints are highlighted with the failing expression and the value that caused the failure.

### 7.7 Observability (observability.md)

**Amendment:** New audit event categories:

| Event | Category | Description |
|---|---|---|
| `attestation.created` | Provenance | New attestation node created |
| `attestation.verified` | Provenance | Attestation chain verified (with result) |
| `attestation.constraint_failed` | Provenance | A constraint evaluation returned false |
| `attestation.chain_invalid` | Provenance | Chain verification failed (signature, expiry, etc.) |
| `key_binding.created` | Identity | New key binding registered |
| `key_binding.expired` | Identity | Key binding expired or invalidated |
| `key_binding.revoked` | Identity | Key binding explicitly revoked |

All events include the full `VerificationResult` tree for forensic analysis.

---

## 8 Migration

Authorization provenance is introduced incrementally:

### Phase 1: Key Binding and Signed Input (non-enforcing)

- Implement `KeyBinding` exchange on spec approval.
- Produce `SignedInput` on every spec approval.
- Store attestation chains alongside existing `AttestationBundle`.
- Verification runs in audit-only mode: log results, do not reject.
- No changes to agent spawn or push flow.

### Phase 2: Strategy-Implied Constraints (non-enforcing)

- Derive and log strategy-implied constraints at push and merge time.
- Surface constraint violations in the UI and message bus.
- Humans can add explicit constraints at approval time.
- Still audit-only: log, do not reject.

### Phase 3: Enforcement

- Enable `builtin:require-signed-authorization` policy.
- Push and merge are rejected if the attestation chain is invalid or constraints fail.
- Derived inputs are produced for orchestrator → agent delegation.
- Legacy `AttestationBundle` is still produced for backward compatibility.

### Phase 4: Derived Input Chains

- Orchestrators produce `DerivedInput` for sub-task delegation.
- Full chain verification at push and merge time.
- Gate agents produce `GateConstraint` records.
- AIBOM extended with full chain attestations.
- Legacy `AttestationBundle` deprecated.

---

## 9 What This Prevents

| Attack / Drift | How It's Prevented |
|---|---|
| Agent commits code outside spec scope | Explicit or strategy-implied path constraints fail at push time |
| Compromised server forges merge attestation | Server cannot forge user's `SignedInput` signature |
| Spec revision silently drops security constraint | Derived input constraint propagation is additive-only |
| Orchestrator grants sub-agent excessive permissions | Derived input can only narrow, not widen, parent's constraints |
| Agent runs with wrong persona or meta-spec set | Strategy-implied constraints verify persona and meta-spec SHA |
| Agent runs on unauthorized stack | Stack hash constraint (from gyre-stack.lock) evaluated as part of constraint set |
| Old authorization replayed for new task | Context binding: workspace_id, repo_id, spec_sha, expected_generation, valid_until |
| Gate result forged by compromised server | Each gate result is individually signed by the gate agent or forge identity |
| Admin with DB access marks gates as passed | Gate attestations require cryptographic signatures, not just DB records |
| Constraint evaluation error allows bad code | Fail closed: evaluation errors are treated as constraint failures |

---

## Relationship to Existing Specs

- **Agent Gates** (`agent-gates.md`): Merge attestation bundle subsumed by attestation chain (§5.2). Gate agents produce individually signed `GateAttestation` records. Spec approval produces `SignedInput`.
- **Supply Chain** (`supply-chain.md`): Stack attestation unified into constraint set (§7.3). AIBOM extended with chain attestations.
- **Identity & Security** (`identity-security.md`): Key binding (§2.3) extends the 3-layer identity stack. Agent JWT claims carried into `OrchestratorKeyBinding`.
- **ABAC Policy Engine** (`abac-policy-engine.md`): New `attestation` resource type and `builtin:require-signed-authorization` policy (§7.2).
- **Platform Model** (`platform-model.md`): Trust anchors are tenant-scoped (§1.1). Workspace trust level influences strategy-implied constraints (§3.2).
- **Agent Runtime** (`agent-runtime.md`): Agent spawn extended with key binding and derived input injection (§7.4).
- **Message Bus** (`message-bus.md`): New `ConstraintViolation` message kind (§7.5).
- **Human-System Interface** (`human-system-interface.md`): Constraint editing in spec approval UI, provenance chain visualization in Explorer (§7.6).
- **Observability** (`observability.md`): New provenance and identity audit event categories (§7.7).
- **Source Control** (`source-control.md`): Pre-accept validation extended with constraint verification (§6.1).
- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).
- **Spec Lifecycle** (`spec-lifecycle.md`): Spec modification invalidates existing `SignedInput` (new SHA requires new approval and new signed authorization).
- **Design Principles** (`design-principles.md`): "Specs first, always" becomes cryptographically enforced end-to-end, not just at the spec-to-MR reference level.
