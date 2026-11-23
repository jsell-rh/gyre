---
title: "Version derived from git tags at build time (build script + Nix flake injection), Cargo.toml as record only"
spec_ref: "Spec gap spec-17849f2036d5093f11e2a1ae / build-time-version-injection / d1e5376399b8"
depends_on: [task-319]
progress: not-started
commits: []
---

## Required behavior

Per specs/development/ci-docs-release.md §Version in Code, the version is derived from git tags at build time, not hardcoded in Cargo.toml; the Nix flake or build script injects it: tagged commit -> exact version (e.g. 1.2.3); untagged commit -> dev version (e.g. 1.3.0-dev.42+abc123). (1) Build-script injection: add version derivation to the gyre-cli and gyre-server build scripts (gyre-server's existing build.rs gains this alongside its web-asset build — additive, web build behavior and SKIP_WEB_BUILD unchanged; gyre-cli gains a build.rs): resolve the version from the source checkout's git state — exact semver tag on HEAD -> that version; otherwise the dev version computed by the prerelease-dev-version task's logic (next-version base + dev.{count}) suffixed with '+' + the short commit SHA — and emit cargo:rustc-env=GYRE_BUILD_VERSION={value} (with cargo:rerun-if-changed inputs so version changes rebuild). Precedence: an explicit GYRE_BUILD_VERSION environment override (the Nix path) wins over git-derived; when neither git metadata nor an override is available (e.g. a source tarball), fall back to the manifest version suffixed with an explicit marker (e.g. '+nogit') so the degradation is observable, never a silent hardcoded claim. (2) Runtime surfaces consume the injected value instead of CARGO_PKG_VERSION: the CLI's clap version (gyre --version), the admin info endpoint (api/admin.rs), and the /api/v1/version route (currently the literal "0.1.0", api/version.rs) all report GYRE_BUILD_VERSION. (3) Nix flake injection: flake.nix passes a git-derived version into the crane builds of gyre-server and gyre-cli (from the flake's self.rev / self.dirtyRev resolution, or by exporting the tag-derived version), so flake-built binaries carry the git-derived version. (4) Cargo.toml's version remains a record updated only by the release MR (release-flow-artifacts-publish step 5) — never the source of truth; after this task, the served/printed version of a build from an untagged checkout differs from the manifest literal. Coordination: the speed-backpressure plan (assigned) also edits gyre-server/build.rs and flake.nix — keep changes additive and independent. Do not alter the spec.

## Acceptance Criteria

- [ ] Building gyre-cli from a checkout whose HEAD carries tag v1.2.3 yields `gyre --version` printing 1.2.3; building from an untagged HEAD N commits past v1.2.3 with short SHA abc1234 prints the dev form 1.3.0-dev.N+abc1234 (per the bump rules for the range) — both differ from the manifest literal 0.1.0 that every build prints today.
- [ ] GYRE_BUILD_VERSION=9.9.9 in the environment at build time is honored by the build script (the Nix injection path is exercisable without Nix), and a build with no git metadata and no override produces a value carrying the explicit nogit marker rather than a plain manifest claim.
- [ ] The admin info endpoint and GET /api/v1/version report the injected build version (no longer the literal "0.1.0"), and their existing tests are updated to the injected-value contract.
- [ ] flake.nix feeds a git-derived version into the crane package builds of gyre-server and gyre-cli (self.rev/dirtyRev or tag-derived), so flake-built binaries carry the git-derived version; the web-asset build behavior of gyre-server's build.rs (incl. SKIP_WEB_BUILD) is unchanged.

## Verification intent

Tag a test checkout's HEAD as v2.5.1, cargo build -p gyre-cli, and run the built `gyre --version`: pre-task it prints 'gyre 0.1.0' (the clap version attribute resolves to the hardcoded workspace manifest constant regardless of any tag — the real defect: no build script or flake reads git tags, and every runtime version surface traces to CARGO_PKG_VERSION), so the assertion that it prints 2.5.1 fails today; then remove the tag, rebuild from the untagged HEAD, and assert the dev form {next}-dev.{count}+{shortsha} — an implementation that only special-cases tags (or that copies the manifest version) fails the second arm. Control: GYRE_BUILD_VERSION=9.9.9 cargo build must yield 9.9.9, proving the injection path the Nix flake uses is real rather than git-only.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/ci-docs-release.md`. Source: `7e74623345bd6c3cffe0d828aa1b7c8d3690c884`; plan: `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`.

[
  {
    "id": "4a0c06c902b41dd1a2000ebe",
    "decision": "gap",
    "requirement_quote": "The version is derived from git tags at build time, not hardcoded in `Cargo.toml`. The Nix flake or build script injects the version:\n- Tagged commit -> exact version (e.g., `1.2.3`)\n- Untagged commit -> dev version (e.g., `1.3.0-dev.42+abc123`)",
    "detail": "The version is in fact hardcoded in Cargo.toml and never derived from git tags. The workspace package version is the literal '0.1.0' and every crate inherits it; the gyre-server build script only builds the web assets and performs no version injection; the Nix flake builds the server and CLI with crane without any git-tag-derived version override; and the runtime version surfaces (CLI --version, admin version endpoint) report CARGO_PKG_VERSION. None of the specced injection behaviors (tagged commit -> exact version, untagged -> dev version with commit count and short SHA) exist. Note this section also assumes the Pre-Release Versions computation, which is itself unimplemented (see that heading).",
    "evidence": [
      {
        "file": "Cargo.toml",
        "quote": "[workspace.package]\nversion = \"0.1.0\"",
        "detail": "The version is a hardcoded literal in the workspace manifest, inherited by all crates — exactly what the spec forbids as the source of truth ('not hardcoded in Cargo.toml'). Since no release MR flow ever updates it either (see the Release Process heading), it is both the de facto source and never updated as a record."
      },
      {
        "file": "crates/gyre-server/build.rs",
        "quote": "fn main() {\n    // Watch web source files for changes",
        "detail": "The gyre-server build script was read in full: its only behavior is watching and building the Svelte web assets (npm ci / npm run build, skippable via SKIP_WEB_BUILD). It never reads git tags, never computes a version, and never emits cargo:rustc-env or cargo:rerun-if-changed inputs for version derivation — the 'build script injects the version' mechanism does not exist."
      },
      {
        "file": "flake.nix",
        "quote": "        gyreServer = craneLib.buildPackage (commonArgs // {\n          inherit cargoArtifacts;\n          cargoExtraArgs = \"-p gyre-server\";\n        });",
        "detail": "The Nix flake builds gyre-server and gyre-cli with crane using the manifest version; the flake was read in full and contains no version override, no git tag reading (no self.rev / self.dirtyRev usage), and no dev-version computation — the 'Nix flake injects the version' mechanism does not exist."
      },
      {
        "file": "crates/gyre-server/src/api/admin.rs",
        "quote": "        version: env!(\"CARGO_PKG_VERSION\"),",
        "detail": "The server's runtime version report (admin info endpoint) is the compile-time manifest constant, confirming that every version surface in the product traces to the hardcoded Cargo.toml value rather than to git tags at build time."
      }
    ]
  }
]
