# Documentation, CI & Release

## User-Facing Docs

- **Starlight** (Astro) for user-facing documentation.
- Deployed via **GitHub Pages**.
- Docs are a first-class deliverable, not an afterthought.

## CI

- **GitHub Actions** - robust CI pipeline from day one.
- CI enforces all mechanical invariants (architecture lints, tests, i18n coverage, doc freshness, etc.).

## Versioning & Release

### Conventional Commits: Forge-Enforced

Conventional commit format is enforced at the **git protocol level** via pre-accept validation. Pushes with non-conforming commit messages are rejected before entering the repo.

#### Format

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

#### Types

| Type | Semver Effect | When to Use |
|---|---|---|
| `feat` | Minor bump | New functionality |
| `fix` | Patch bump | Bug fix |
| `docs` | No bump | Documentation only |
| `style` | No bump | Formatting, whitespace |
| `refactor` | No bump | Code change that neither fixes nor adds |
| `perf` | Patch bump | Performance improvement |
| `test` | No bump | Adding or fixing tests |
| `build` | No bump | Build system or dependencies |
| `ci` | No bump | CI configuration |
| `chore` | No bump | Maintenance, tooling |

#### Breaking Changes

A `BREAKING CHANGE:` footer or `!` after the type triggers a **major** version bump:
```
feat(auth)!: replace token auth with OIDC

BREAKING CHANGE: The GYRE_AUTH_TOKEN environment variable is no longer supported.
Use GYRE_OIDC_ISSUER instead.
```

#### Pre-Accept Enforcement

The forge validates commit messages at push time:
- Every commit in the push must match the conventional format
- Unknown types are rejected
- Empty descriptions are rejected
- Scope is optional but must be lowercase alphanumeric if present
- The forge returns a clear error in git sideband output:
  ```
  remote: [GYRE] Push rejected: commit abc123 has invalid message format
  remote: [GYRE] Expected: <type>(<scope>): <description>
  remote: [GYRE] Got: "fixed the thing"
  ```

This is not a CI check that runs after the push - the push itself fails. Immediate feedback, zero latency.

### Automatic Version Computation

The forge computes the next version from the commit history between the latest semver tag and HEAD:

```
v1.2.3 (latest tag)
  |
  +-- fix(auth): handle expired JWT gracefully          -> patch
  +-- feat(forge): add speculative merge endpoint       -> minor
  +-- docs: update agent-gates spec                     -> no bump
  +-- fix(cli): correct WebSocket reconnect logic       -> patch
  +-- feat(admin)!: restructure RBAC model              -> MAJOR

Result: v2.0.0 (major wins)
```

Rules:
- Any `BREAKING CHANGE` or `!` -> major bump
- Any `feat` (without breaking) -> minor bump
- Any `fix` or `perf` (without feat or breaking) -> patch bump
- Only `docs`, `style`, `refactor`, `test`, `build`, `ci`, `chore` -> no release

### Changelog Generation

The forge generates changelogs automatically, grouped by type with full attribution:

```markdown
# v2.0.0 (2026-03-20)

## BREAKING CHANGES
- **admin:** restructure RBAC model (worker-42, task TASK-091,
  spec: system/identity-security.md@abc123)

## Features
- **forge:** add speculative merge endpoint (worker-17, task TASK-087,
  spec: system/forge-advantages.md@def456)

## Bug Fixes
- **auth:** handle expired JWT gracefully (worker-42, task TASK-089)
- **cli:** correct WebSocket reconnect logic (worker-23, task TASK-090)
```

Each entry includes:
- **Agent attribution** - which agent produced the change
- **Task reference** - which task it was part of
- **Spec reference** - which spec it implements (from commit provenance)
- **Conventional commit scope** - which area of the codebase

This is richer than any external changelog generator because the forge has native access to the provenance chain.

### Release Process

Releases are triggered explicitly (not on every push). The flow:

1. **Human or workspace orchestrator triggers release:**
   ```
   gyre release prepare
   ```

2. **Forge computes version** from commits since last tag.

3. **Forge generates changelog** with full attribution.

4. **Forge creates release artifacts:**
   - Changelog (markdown)
   - AIBOM (from supply chain spec - which agents, stacks, models produced this release)
   - SBOM (dependencies)
   - Signed git tag (Sigstore, via the releasing agent's or user's OIDC identity)
   - Binary artifacts (from Nix build)
   - Docker image (from Nix)

5. **Release MR is created** containing:
   - Version bump in `Cargo.toml`
   - Updated changelog
   - AIBOM snapshot

6. **Gate agents review the release** (if configured):
   - Security agent scans for vulnerabilities introduced since last release
   - Accountability agent verifies all spec-drift-review tasks are resolved

7. **On merge, forge tags and publishes:**
   - Signed git tag: `v{major}.{minor}.{patch}`
   - GitHub Release with artifacts
   - Docker image pushed to registry
   - Docs deployed to GitHub Pages

### Pre-Release Versions

For development builds between releases:
- `v1.3.0-dev.42` - 42 commits since last release
- Computed automatically, not tagged
- Available via `gyre --version` for debugging

### Version in Code

The version is derived from git tags at build time, not hardcoded in `Cargo.toml`. The Nix flake or build script injects the version:
- Tagged commit -> exact version (e.g., `1.2.3`)
- Untagged commit -> dev version (e.g., `1.3.0-dev.42+abc123`)

`Cargo.toml` version is updated only during the release MR, as a record, not as the source of truth.

### Relationship to Existing Specs

- **Supply Chain** (`supply-chain.md`): AIBOM is a release artifact. Release tags are Sigstore-signed.
- **Agent Gates** (`agent-gates.md`): Release MRs can have gate agents (security scan, spec alignment).
- **Source Control** (`source-control.md`): Pre-accept validation enforces commit format. Tags are forge-native.
- **Forge Advantages** (`forge-advantages.md`): Version computation, changelog with provenance, and commit validation at push time are only possible because Gyre owns the forge.
- **Spec Lifecycle** (`spec-lifecycle.md`): Release gates can verify all spec-drift tasks are resolved before shipping.
