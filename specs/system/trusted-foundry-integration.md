# Trusted Foundry Integration (Future Pattern)

> **Status: Reference spec only.** This describes a potential future integration pattern with the [Trusted Software Foundry](https://github.com/jsell-rh/trusted-software-foundry) (TSF). It is not scheduled for implementation. It exists so that Gyre's architecture decisions remain compatible with this pattern.

## What Is the Trusted Software Foundry?

TSF is an IR-first application platform that inverts AI code generation. Instead of agents writing arbitrary code, they write a **declarative, schema-validated IR spec** (`app.foundry.yaml`). A deterministic compiler (`forge`) then assembles pre-audited, version-pinned components into a working application.

**The key guarantee:** Same IR + same component versions = same binary, always.

### How TSF's Trust Boundary Works

```
Agent writes IR (app.foundry.yaml)
  |
  v
Forge compiler
  1. Validates IR against JSON Schema (structural)
  2. Validates semantic rules (cross-field constraints)
  3. Resolves components from registry
  4. Verifies SHA-256 audit hash of each component against registry
  5. Generates wiring code (main.go, go.mod, migrations)
  |
  v
go build -> deterministic binary
```

Three layers of trust:
1. **Component registry** - each component version is audited once, SHA-256 hashed, and immutable. Bug fixes create new versions, never modify existing ones.
2. **Component interface contract** - frozen `spec.Component` interface. Components can't deviate from their contract.
3. **Compile-time verification** - compiler verifies every component's hash against the registry before generating code. Hash mismatch = hard error.

### What Agents Do and Don't Touch

| Agents Write | Agents Never Touch |
|---|---|
| `app.foundry.yaml` (declarative IR) | Component source code |
| Resource definitions (data schema) | Generated wiring code (main.go) |
| Component selection + version pinning | go.mod dependencies |
| API configuration | SQL migrations |
| Hook declarations (custom code entry points) | Component internals |

The IR is structured, schema-validated YAML. LLM hallucinations are caught by JSON Schema validation before compilation. Agents can't invent components or field types - the schema enumerates all valid options.

## How This Would Integrate with Gyre

### Extended Provenance Chain

Today:
```
spec -> agent -> code -> gates -> merge
```

With TSF:
```
spec -> agent -> IR (app.foundry.yaml) -> foundry compiler (attested) -> code -> gates -> merge
```

The foundry compiler becomes a new link in the attestation chain. Gyre's existing machinery handles it naturally:

### 1. The IR Is the Review Surface

Gate agents review the **IR**, not the generated code. This is dramatically more tractable:
- A 20-line resource definition vs. 500 lines of generated CRUD code
- The Accountability agent checks: "does this IR match the spec?"
- The Security agent checks: "does this IR expose any dangerous patterns?"
- Neither needs to read the generated Go code

This directly addresses the review scalability problem. With 20+ agents producing code, human review of all output is impossible. With TSF, humans/agents review IRs and trust the compiler.

### 2. The Foundry Compiler as an Attested Gate

The foundry compiler runs as a trusted process inside Gyre's gate chain:

```toml
[[gates]]
name = "foundry-compile"
type = "FoundryValidation"
command = "forge compile app.foundry.yaml --output ./out --verify"
required = true
```

The gate:
1. Runs `forge compile` with `--verify` (checks all component audit hashes)
2. Compares generated output against what's in the MR
3. If the committed code doesn't match what the compiler would produce from the IR, the gate **fails** - someone modified generated code by hand
4. Signs the result with the forge's OIDC identity

This is **reproducible build verification** at the application level.

### 3. Foundry Version in Agent Stack Attestation

The foundry compiler version goes into `gyre-stack.lock`:

```toml
[foundry]
version = "v1.2.3"
registry_hash = "sha256:..."
compiler_hash = "sha256:..."
```

If an agent uses a different foundry version than what the repo expects, the stack attestation breaks and the push is rejected.

### 4. AIBOM Extension

The AIBOM gains foundry-specific fields:

```json
{
  "foundry": {
    "compiler_version": "v1.2.3",
    "compiler_hash": "sha256:...",
    "ir_hash": "sha256:...",
    "components": [
      {
        "name": "foundry-http",
        "version": "v1.0.0",
        "audit_hash": "sha256:...",
        "audit_verified": true
      }
    ],
    "deterministic": true,
    "ir_to_output_reproducible": true
  }
}
```

This is richer than a traditional SBOM because it captures not just what dependencies exist, but that they were **audited, hash-verified, and deterministically compiled**.

### 5. Component Registry as a Trusted Artifact Store

Gyre's forge could host the component registry alongside git repos:
- Component versions are immutable (like container image tags)
- Audit hashes are computed and stored at publication time
- The registry is itself versioned and signed
- Agents can browse available components but can't modify them

### 6. IR-Level Spec Binding

The spec-to-code binding (from `agent-gates.md`) extends to IRs:
- The spec says "this service needs JWT auth and PostgreSQL"
- The IR includes `foundry-auth-jwt: v1.0.0` and `foundry-postgres: v1.0.0`
- The gate agent verifies the IR satisfies the spec
- The compiler verifies the IR produces valid code
- Two-layer validation: intent (spec -> IR) and implementation (IR -> code)

## What This Changes About Review

| Without TSF | With TSF |
|---|---|
| Gate agents review all generated code | Gate agents review IR (10-100x smaller) |
| Security review requires reading every file | Security review checks IR declarations + component audit status |
| Accountability agent compares code against spec | Accountability agent compares IR against spec (much simpler) |
| Every agent's output is unique, non-deterministic | Every agent's IR produces identical output (deterministic) |
| SBOM computed after the fact | SBOM is the IR's `components:` block (built in) |
| Code review is the bottleneck | IR review is tractable; compiler is trusted |

## Hooks: The Trust Boundary Seam

TSF's `hooks:` block is the escape hatch for business logic the IR can't express. Hook code is **copied unchanged** by the compiler - it is not generated from audited components and is **outside the trust boundary**.

### What Hooks Are

A hook is a custom code file at a well-defined entry point in the application lifecycle. The IR declares where hooks exist:

```yaml
hooks:
  - name: validate-order
    trigger: before_create
    resource: Order
    file: hooks/validate_order.go
```

The compiler copies `hooks/validate_order.go` into the output unchanged. It does not validate, audit, or transform it. The hook runs with the same permissions as the compiled application.

### Why This Matters for Gyre

Hooks break the clean trust model. With pure IR:
- Agent writes IR -> schema validates -> compiler deterministically produces code -> trusted
- Review surface: the IR (small, declarative)

With hooks:
- Agent writes IR + arbitrary hook code -> IR is validated, hook code is NOT -> compiler copies hook unchanged -> hook is untrusted
- Review surface: the IR (small) + all hook files (arbitrary size and complexity)

### Split Review Model

Gyre's gate chain should distinguish between IR-only changes and changes that touch hooks:

| MR Contains | Review Path |
|---|---|
| IR changes only (version bumps, resources, config) | Fast path: gate agent reviews IR, foundry validation gate verifies deterministic output. Minimal review. |
| Hook code changes | Full path: security gate agent reviews hook code, accountability gate checks hook against spec, all standard gates apply. |
| Both IR and hook changes | Full path applies to the entire MR. |

The forge can detect this automatically by checking which files in the MR are under `hooks/` vs. changes to `app.foundry.yaml`.

### AIBOM Distinction

The AIBOM should clearly separate trusted and untrusted code provenance:

```json
{
  "foundry": {
    "trusted_code": {
      "source": "compiler",
      "components": [...],
      "deterministic": true,
      "review_required": false
    },
    "untrusted_code": {
      "source": "agent-written hooks",
      "files": [
        {
          "path": "hooks/validate_order.go",
          "agent_id": "worker-42",
          "commit_sha": "abc123",
          "review_status": "approved",
          "gate_results": ["security: passed", "accountability: passed"]
        }
      ],
      "deterministic": false,
      "review_required": true
    }
  }
}
```

Consumers of the AIBOM can immediately see: 95% of this binary came from audited components via deterministic compilation. 5% is custom hook code written by agent worker-42, reviewed by security and accountability gates.

### Hook Minimization as a Quality Signal

The ratio of trusted (compiler-generated) to untrusted (hook) code is a quality metric. A high hook ratio means the IR isn't expressive enough for the use case, or agents are working around the IR instead of extending it.

Gyre's analytics system could track this:
- Hook LOC / total LOC per application
- Hook count trend over time
- Which resources have the most hooks (candidates for new IR features)

## Component Lifecycle

### Routine Version Bump

1. Component maintainer publishes `foundry-postgres v1.0.1`
2. New version audited, SHA-256 computed, added to registry as a new immutable entry
3. `v1.0.0` remains unchanged - nothing breaks for existing consumers
4. Someone (human, agent, or automated) updates `app.foundry.yaml` to pin `v1.0.1`
5. `forge compile` resolves new version, verifies audit hash, generates wiring code
6. Diff is minimal: one version string in IR + deterministic regenerated code

### Security Vulnerability

1. CVE discovered in `foundry-http v1.0.0`
2. Patched `v1.0.1` published with fix, audited, hashed
3. With Gyre: the forge knows every repo and every `app.foundry.yaml`
4. Forge auto-creates a task per affected repo: "Update foundry-http v1.0.0 -> v1.0.1"
5. Agent dispatched per repo: update version pin, recompile, run tests
6. Diff is mechanically verifiable - only the version pin changed, output is deterministic
7. Fleet-wide patching becomes: N parallel agents updating N repos with identical, verifiable changes

### Breaking Change (Major Version)

1. `foundry-postgres v2.0.0` changes the component interface
2. Requires IR changes (new config fields, changed semantics) - not just a version bump
3. Agent needs to understand the migration path and update IR accordingly
4. Gate agent reviews IR changes against the component's changelog
5. The compiler catches IR/component mismatches at compile time (schema validation fails if new required fields are missing)

### Speculative Upgrade Preview

Because the compiler is deterministic, Gyre can preview upgrades without committing:
- Run `forge compile` with the new component version against every repo's current IR
- Diff the output against current compiled code
- Report: "upgrading foundry-http to v1.0.1 across 12 repos changes 47 files with 0 conflicts"
- Or: "upgrading foundry-postgres to v2.0.0 across 12 repos fails compilation in 3 repos due to missing config fields"

This is dependency upgrade impact analysis for free.

## What This Does NOT Change

- **Gyre's core architecture stays the same.** TSF is an optional integration, not a replacement for any Gyre component.
- **Not all code goes through TSF.** Gyre itself (the platform) is written in Rust, not compiled from IR. TSF applies to applications built on Gyre, not Gyre itself.
- **The agent still needs to understand the domain.** Writing a good IR requires understanding what resources exist, what operations they need, and how they relate. TSF constrains the output format, not the design thinking.

## Compatibility Requirements

To keep this integration path open, Gyre's architecture should:

1. **Keep gate types extensible.** A future `FoundryValidation` gate type should be addable without modifying core gate infrastructure.
2. **Keep the AIBOM schema extensible.** Foundry-specific fields should be addable without breaking existing AIBOM consumers.
3. **Keep stack attestation composable.** The `gyre-stack.lock` should support arbitrary tool entries, not just a fixed set.
4. **Keep the provenance chain extensible.** A new link (IR -> compiled output) should be insertable between agent commits and gate validation.

These are all already true given Gyre's current architecture. This spec exists to ensure they stay true.

## Reference

- [Trusted Software Foundry repo](https://github.com/jsell-rh/trusted-software-foundry)
- TSF architecture: `FOUNDRY-ARCHITECTURE.md` in the TSF repo
- V2 IR extensions: `docs/complex-applications.md` in the TSF repo
- JSON Schema for IR: `foundry/spec/schema.json` in the TSF repo
