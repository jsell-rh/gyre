# Supply Chain Security for Agentic Development

## The Problem

In traditional software development, the compiler is a deterministic binary - hard to tamper with, easy to verify. In agentic development, the "compiler" is a set of markdown files, hooks, MCP servers, model configurations, and plugin stacks. These are trivially editable, unversioned relative to the code they produce, and currently unattested.

This creates a supply chain gap: you can prove WHAT code was produced, but not HOW it was produced. Specifically:

- Which AGENTS.md/CLAUDE.md instructions were active when code was generated?
- Were pre-commit hooks enabled or bypassed?
- Which MCP servers were connected?
- Which model and version was used?
- Were quality gates enforced or skipped?
- Was the agent running on a controlled runtime or an uncontrolled developer laptop?

If an attacker (or careless developer) modifies the agent's configuration locally, the resulting code may look correct but violate security policies, skip review steps, or bypass quality gates - with no record that this happened.

## Agent Stack: The New Build Environment

An **agent stack** is the complete configuration under which an agent produces code. It is the agentic equivalent of a build toolchain and must be treated with the same rigor.

### Agent Stack Components

```
agent_stack = hash(
  agents_md:        sha256 of AGENTS.md / CLAUDE.md content,
  hooks:            sorted list of (hook_id, sha256 of hook script),
  mcp_servers:      sorted list of (server_name, version, config_hash),
  model:            model ID + version (e.g., "claude-opus-4-6"),
  cli_version:      gyre CLI version string,
  plugins:          sorted list of (plugin_name, version),
  settings:         sha256 of relevant settings.json / permissions,
  persona:          sha256 of active persona prompt (if any),
)
```

The composite hash of all components is the **stack fingerprint** - a single value that identifies the exact configuration an agent was running.

## Stack Attestation at Push Time

### How It Works

When an agent pushes code to the Gyre forge, the CLI computes the stack fingerprint and includes a signed attestation in the push:

```json
{
  "stack_hash": "sha256:abc123def456...",
  "components": {
    "agents_md": { "hash": "sha256:...", "path": "AGENTS.md" },
    "hooks": [
      { "id": "block-secrets", "hash": "sha256:...", "enabled": true },
      { "id": "cargo-fmt", "hash": "sha256:...", "enabled": true }
    ],
    "mcp_servers": [
      { "name": "jira", "version": "1.2.0", "config_hash": "sha256:..." }
    ],
    "model": "claude-opus-4-6",
    "cli_version": "gyre-cli 0.4.2",
    "plugins": [],
    "settings_hash": "sha256:...",
    "persona_hash": "sha256:..."
  },
  "runtime": {
    "type": "gyre-managed | local",
    "spiffe_id": "spiffe://gyre.example.com/agent/worker-42/session/abc",
    "attestation_method": "spiffe | self-reported"
  },
  "timestamp": "2026-03-20T10:00:00Z",
  "signature": "<sigstore signature via agent OIDC identity>"
}
```

### Forge Verification Policy

The forge evaluates the attestation against the repo's **stack policy** before accepting the push:

| Scenario | Forge Action |
|---|---|
| Stack matches expected fingerprint | Accept |
| Stack is unknown but user has override permission | Accept with warning, log to audit |
| Stack violates policy (hooks disabled, unauthorized MCP server, wrong model) | Reject push with explanation |
| No attestation provided (raw git push without Gyre CLI) | Reject or accept-with-flag per policy |

This is pre-accept validation (from forge-advantages spec) extended to configuration attestation.

## gyre-stack.lock

Each repository contains a **gyre-stack.lock** file that defines the required agent stack for that repo. This is analogous to a dependency lockfile - it pins the exact agent configuration required to contribute.

```toml
[stack]
version = 1
fingerprint = "sha256:abc123def456..."

[agents_md]
hash = "sha256:..."
path = "AGENTS.md"

[hooks]
required = ["block-secrets", "cargo-fmt", "arch-lint", "emdash-check"]

[hooks.block-secrets]
hash = "sha256:..."

[hooks.cargo-fmt]
hash = "sha256:..."

[mcp_servers]
allowed = ["jira", "github"]

[model]
allowed = ["claude-opus-4-6", "claude-sonnet-4-6"]

[cli]
min_version = "0.4.0"

[policy]
require_attestation = true
allow_local_unattested = false
allow_override_roles = ["Admin"]
```

The lockfile is:
- **Versioned with the code** - changes to the stack require a commit
- **Signed** - the lockfile itself is signed by the person who approved the stack change
- **Enforced by the forge** - pushes with non-matching stacks are rejected
- **Auditable** - every change to the lockfile is tracked in git history

## Attestation Levels

Not all execution environments provide the same assurance. Gyre defines three attestation levels:

### Level 3: Gyre-Managed Runtime (Highest)
- Agent runs on Gyre-provisioned infrastructure (NixOS, container, VM)
- SPIFFE workload attestation proves the runtime is real and unmodified
- eBPF captures all system activity
- Configuration injected server-side - agent cannot modify it
- Stack attestation is infrastructure-verified, not self-reported
- **Trust: cryptographic proof of environment + configuration**

### Level 2: Gyre CLI on Developer Machine
- Agent runs locally but uses the Gyre CLI
- CLI computes and signs the stack fingerprint
- Stack attestation is self-reported (CLI trusts its own environment)
- No eBPF, no SPIFFE workload attestation
- **Trust: signed self-report - tamper-evident but not tamper-proof**

### Level 1: Raw Git Push (Lowest)
- Code pushed via standard git without Gyre CLI
- No stack attestation available
- **Trust: none - code provenance is unknown**

### Policy per Level

Repos can require minimum attestation levels:
- Production repos may require Level 3 (Gyre-managed only)
- Development repos may accept Level 2 (CLI-attested)
- No repo should accept Level 1 for production code without explicit override

## AIBOM (AI Bill of Materials)

For every release, Gyre generates an **AI Bill of Materials** that accompanies the traditional SBOM:

### AIBOM Contents

```json
{
  "aibom_version": "1.0",
  "release": "v1.2.3",
  "generated_at": "2026-03-20T12:00:00Z",
  "agents": [
    {
      "agent_id": "worker-42",
      "commits": ["sha1", "sha2", "sha3"],
      "stack_attestation": {
        "fingerprint": "sha256:...",
        "level": 3,
        "verified": true
      },
      "model": "claude-opus-4-6",
      "mcp_servers": ["jira:1.2.0"],
      "spawned_by": "user:jsell",
      "task_ids": ["TASK-007"],
      "sigstore_entries": ["rekor-entry-uuid-1", "rekor-entry-uuid-2"]
    }
  ],
  "stack_policy": {
    "lockfile_hash": "sha256:...",
    "all_commits_attested": true,
    "minimum_level": 3,
    "policy_violations": []
  },
  "summary": {
    "total_commits": 47,
    "attested_commits": 47,
    "unattested_commits": 0,
    "models_used": ["claude-opus-4-6"],
    "agents_involved": 8,
    "human_commits": 0
  }
}
```

### AIBOM vs. SBOM

| Artifact | Answers | Standard |
|---|---|---|
| **SBOM** | What dependencies are in the code? | SPDX / CycloneDX |
| **AIBOM** | What AI tooling produced the code, and can we prove it? | Gyre-native (aligns with emerging AIBOM standards) |

Both are generated as release artifacts. Together they provide full provenance: what's in the software AND how it was made.

## SLSA Provenance Integration

Gyre's attestation model maps to SLSA (Supply Chain Levels for Software Artifacts):

| SLSA Requirement | Gyre Implementation |
|---|---|
| Source integrity | Built-in forge, signed commits (Sigstore) |
| Build integrity | Agent stack attestation, gyre-stack.lock |
| Provenance | AIBOM with per-commit agent + config attribution |
| Non-falsifiable | Sigstore signatures + Rekor transparency log |
| Hermetic | Level 3 (Gyre-managed NixOS runtimes) |
| Reproducible | NixOS definitions + pinned agent stacks |

Level 3 attestation on Gyre-managed runtimes achieves SLSA Build Level 3 equivalent.

## The Laptop Problem

Developer laptops are the weakest link. Gyre's approach:

1. **Require attestation** - the CLI always computes and signs the stack fingerprint. Even on laptops, you get a signed self-report.
2. **Detect drift** - if a developer's stack doesn't match `gyre-stack.lock`, the push is rejected or flagged. They must either fix their config or get an override.
3. **Incentivize managed runtimes** - make it trivially easy to spin up a Gyre-managed remote agent. If the managed path is easier than local, developers will choose it.
4. **Tag attestation level in AIBOM** - production releases show which commits came from Level 3 (fully attested) vs. Level 2 (self-reported). Policy decides what's acceptable.
5. **Audit trail for overrides** - if a developer pushes with a non-matching stack and gets an admin override, that override is logged, attributed, and visible in the AIBOM.

## Relationship to Existing Specs

- **Identity & Security** (`identity-security.md`): Stack attestation uses the same OIDC + Sigstore chain as commit signing. The attestation is signed with the agent's OIDC identity.
- **Source Control** (`source-control.md`): Pre-accept validation now includes stack verification. The forge rejects pushes with policy-violating stacks.
- **Forge Advantages** (`forge-advantages.md`): This IS a forge advantage. GitHub cannot verify agent configuration at push time because it doesn't control the agent runtime or CLI.
- **Observability** (`observability.md`): Stack attestation events feed the audit system. SIEM forwarding includes attestation verification status.
- **Agent Runtime** (`agent-runtime.md`): Gyre-managed runtimes achieve Level 3 attestation. Server-side MCP injection is part of the controlled stack.
- **Design Principles** (`design-principles.md`): "No shortcuts" and "Security by default" - stack attestation ensures agents can't shortcut their configuration.
