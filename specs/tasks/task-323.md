---
title: "Provision the Pass 2 toolchains in the server container image: rust-analyzer, typescript-language-server + typescript, and the ts-callgraph driver script"
spec_ref: "Spec gap spec-b89674dbcb695eb34e8d5e15 / server-image-pass2-toolchains / d96599f10659"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

The Prerequisites section requires the extraction host to have all four language toolchains and says Docker images should include them. Extraction runs server-side (graph_extraction.rs in gyre-server, invoked from git push and mirror-sync), so the operative extraction host is the server container image (root Dockerfile). At base it includes the Go toolchain, the committed go-callgraph binary (GO_CALLGRAPH_BIN), node 22, and pinned pyright 1.1.414 (PYRIGHT_LANGSERVER_BIN) - but no rust-analyzer, no typescript-language-server, and not even the TS driver script, so Rust and TypeScript Pass 2 silently degrade to zero Calls edges in the containerized deployment. Close the gap in the Dockerfile: (1) install a pinned rust-analyzer binary in the runtime image (e.g. a rustup component in a build stage, copied into the final image) and wire it the way the adapters discover it - crates/gyre-adapters/src/call_graph_rust.rs find_rust_analyzer() checks RUST_ANALYZER_BIN then PATH - so set ENV RUST_ANALYZER_BIN (and/or place it on PATH), matching how GO_CALLGRAPH_BIN and PYRIGHT_LANGSERVER_BIN are wired; (2) install typescript-language-server and the typescript package via a pinned npm install in the existing node/pyright stage pattern, and wire them per the discovery order documented in scripts/ts-callgraph.mjs: set ENV TS_CALLGRAPH_SERVER to the typescript-language-server entry point and ENV TS_CALLGRAPH_TSSERVER to the typescript lib dir (node is already in the image); (3) COPY scripts/ts-callgraph.mjs into /app/scripts alongside the already-copied python-callgraph.sh / python-callgraph-pyright.mjs (Dockerfile line 91 copies only the Python drivers; without the driver script the server's find_script('ts-callgraph.mjs') returns None and TS Pass 2 skips even with the language server installed); (4) update the stale comment 'Rust/TS toolchains are task-073/075 scope' (tasks 073/075 shipped the driver code without updating the image) to reflect the shipped state; (5) keep pins explicit (no floating latest tags) consistent with pyright@1.1.414 and golang:1.24. The graceful-degradation contract itself (warn + zero edges, never fail a push) must not change; the fix makes the toolchains present so degradation no longer fires for Rust/TS in the shipped image. The Nix-built agent image (docker/gyre-agent/flake.nix) is out of scope: agents do not run extraction in the current architecture. Do not alter the Pass 2 driver code or the adapters' discovery logic - the image must satisfy the existing discovery contract.

## Acceptance Criteria

- [ ] docker build of the root Dockerfile succeeds, and the built runtime image contains a working rust-analyzer binary at the RUST_ANALYZER_BIN path and a working typescript-language-server plus typescript at the TS_CALLGRAPH_SERVER / TS_CALLGRAPH_TSSERVER paths (verifiable by docker run <image> <binary> --version).
- [ ] The image contains /app/scripts/ts-callgraph.mjs (and still the Python drivers and gyre-go-callgraph), so the server's find_script discovery resolves all three Pass 2 drivers inside the container.
- [ ] Running Rust Pass 2 inside the built image (extract_call_edges on a fixture Rust repo with at least one known cross-function call site, via the server's push-triggered extraction or the SubprocessCallGraphExtractor entry point) returns the expected non-zero Calls edges - not the zero-edge degradation result.
- [ ] Running TypeScript Pass 2 inside the built image on a fixture TS repo with a known call site likewise returns non-zero Calls edges.
- [ ] The Dockerfile no longer claims Rust/TS toolchains are out of scope; all toolchain additions are pinned; the image still builds with the non-root user and existing Go/pyright wiring unchanged.

## Verification intent

Build the server image and run Rust and TypeScript Pass 2 extraction inside it against fixture repos each containing one known call site, asserting the expected Calls edge appears in each result. The real defect that must make this fail at base 8546a781: the Dockerfile installs no rust-analyzer and no typescript-language-server (grep for both returns only the stale 'task-073/075 scope' comment at line 73) and never copies scripts/ts-callgraph.mjs (line 91 copies only the Python drivers), so inside the container find_rust_analyzer() returns None (call_graph_rust.rs logs 'rust-analyzer binary not found' and returns no edges) and TS Pass 2 either skips at find_script or warns 'typescript-language-server not found' (ts-callgraph.mjs) - zero Calls edges for both languages with no error surfaced. A reverted build therefore produces zero edges and the probe fails; grepping the Dockerfile for toolchain names without executing the drivers in the built image is not an acceptable probe.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/lsp-call-graph.md`. Source: `8546a781f772f05c823bbf4df6127bec812e2152`; plan: `a8e88983c3060cd3af4d90d16e8709208b9361fa1dece0940873c9e8d32bc60d`.

[
  {
    "id": "9476a77de45d2786274c547f",
    "decision": "gap",
    "requirement_quote": "The extraction host (server or agent container) must have the language toolchains installed:",
    "detail": "The section requires the extraction host to have all four toolchains — rust-analyzer (Rust), pyright (Python), Go toolchain (Go), and node+typescript+typescript-language-server (TypeScript) — and says Docker images should include them. In the current architecture extraction runs server-side only (graph_extraction.rs in gyre-server, invoked from the git push and mirror-sync paths), so the operative extraction host is the server. The shipped server container image (root Dockerfile) includes the Go toolchain, the committed go-callgraph binary (installed as /usr/local/bin/gyre-go-callgraph with GO_CALLGRAPH_BIN set), node, and pinned pyright 1.1.414 with PYRIGHT_LANGSERVER_BIN set — but it contains no rust-analyzer and no typescript-language-server (grep for both in the Dockerfile returns zero matches; the image's own comment still says 'Rust/TS toolchains are task-073/075 scope', stale because tasks 073/075 shipped the driver code without updating the image). Consequence: in the containerized deployment, Rust and TypeScript Pass 2 silently degrade to zero Calls edges (the graceful-degradation contract logs a warning and returns no edges, so no error surfaces and the call graph is silently incomplete for those languages). The Nix-built agent image (docker/gyre-agent/flake.nix) includes none of the four toolchains either (nodejs, bash, curl, git, util-linux, cacert only), though agents do not run extraction in the current architecture. Missing behavior: rust-analyzer and typescript-language-server (+typescript) must be added to the extraction host's container image (or the deployment must otherwise provision them) for the specced prerequisites to hold in the shipped deployment.",
    "evidence": [
      {
        "file": "Dockerfile",
        "quote": "# go-callgraph CHA binary. Rust/TS toolchains are task-073/075 scope.",
        "detail": "The server image's runtime stage comment scopes in only the Go (and later Python) toolchains. Grep confirms zero occurrences of 'rust-analyzer' and 'typescript-language-server' anywhere in the Dockerfile: the Rust (task-073) and TypeScript (task-075) drivers shipped, but their required binaries were never added to the image, so those languages' Pass 2 degrade to zero edges in containerized deployments."
      },
      {
        "file": "Dockerfile",
        "quote": "COPY scripts/python-callgraph.sh scripts/python-callgraph-pyright.mjs /app/scripts/",
        "detail": "For contrast, the Python prerequisites are fully shipped in the image (node, pinned pyright 1.1.414 as pyright-langserver, driver scripts, PYRIGHT_LANGSERVER_BIN), showing the intended pattern that the Rust/TS halves do not follow."
      },
      {
        "file": "docker/gyre-agent/flake.nix",
        "quote": "pkgs.nodejs_22",
        "detail": "The Nix-built agent image contents (nodejs_22, bashInteractive, curl, git, util-linux, cacert, gyre scripts, node deps): none of the four specced extraction toolchains (rust-analyzer, pyright, Go, typescript-language-server) are included — the spec's 'For Docker agent containers, these should be included in the base image' is unmet for the agent image, though extraction currently runs only on the server."
      },
      {
        "file": "crates/gyre-adapters/src/call_graph_rust.rs",
        "quote": "extract_with_binary(repo_path, find_rust_analyzer()).await",
        "detail": "The Rust driver resolves the rust-analyzer binary only via RUST_ANALYZER_BIN/PATH discovery; with no rust-analyzer in the server image and no RUST_ANALYZER_BIN set, find_rust_analyzer returns None and the run degrades to zero edges - the prerequisite is genuinely load-bearing, not hypothetical."
      }
    ]
  }
]

## Shipped

The server container image (root Dockerfile) now ships all four specced Pass 2
toolchains; no driver code or adapter discovery logic was changed — the image
satisfies the existing discovery contracts.

### Behavior shipped (Dockerfile)

- New `rust-analyzer` stage: `FROM rust:1.87-slim` (same pinned base as the
  builder stage) + `rustup component add rust-analyzer`, binary copied to
  `/usr/local/bin/rust-analyzer`; runtime sets `ENV
  RUST_ANALYZER_BIN=/usr/local/bin/rust-analyzer` (the first branch of
  `find_rust_analyzer()` in crates/gyre-adapters/src/call_graph_rust.rs).
  Pinned via the toolchain image — no floating tag.
- New `ts-tools` stage: `FROM node:22-bookworm-slim` +
  `npm install -g typescript@5.8.3 typescript-language-server@4.3.4`
  (explicit pins, pyright@1.1.414 stage pattern). Runtime copies both
  `node_modules` packages, symlinks
  `/usr/local/bin/typescript-language-server` ->
  `.../typescript-language-server/lib/cli.mjs`, and sets
  `TS_CALLGRAPH_SERVER=/usr/local/bin/typescript-language-server` and
  `TS_CALLGRAPH_TSSERVER=/usr/local/lib/node_modules/typescript/lib` — the
  authoritative env-override branch of scripts/ts-callgraph.mjs discovery
  (the lib dir contains tsserver.js, which the driver validates).
- `scripts/ts-callgraph.mjs` is now COPYed to `/app/scripts/` alongside the
  Python drivers, so the server's `find_script` workspace discovery (cwd
  /app) resolves all three Pass 2 drivers inside the container.
- The stale "Rust/TS toolchains are task-073/075 scope" comment is replaced
  with the shipped state; Go/pyright wiring and the non-root user are
  unchanged.

### Test evidence

- TypeScript Pass 2 executed end-to-end in this sandbox with the exact image
  wiring (pinned npm -g install of typescript@5.8.3 +
  typescript-language-server@4.3.4, bin symlink to lib/cli.mjs,
  TS_CALLGRAPH_SERVER/TS_CALLGRAPH_TSSERVER set to the Dockerfile's values):
  the real driver `scripts/ts-callgraph.mjs` on a fixture repo with one known
  call site returned the expected non-zero edge
  `{"from":"src/app.compute","to":"src/math.add"}` (exit 0). npm's own
  global layout was confirmed identical to the Dockerfile's symlink/lib-dir
  wiring, and `typescript-language-server --version` -> 4.3.4,
  `tsc --version` -> 5.8.3. Probe transcript and the fixture are in
  /tmp/stage/review-evidence (README.md, probes.log, ts-probe-out.json).
- Docker build and the in-image rust-analyzer/TS executions are deferred to
  remote verification: docker is not installed in this sandbox and its
  network allowlist blocks static.rust-lang.org, crates.io and
  release-assets.githubusercontent.com (a real rust-analyzer binary could
  not be fetched locally; the system rustup home is root-owned without the
  component). These are infrastructure restrictions, not code defects. Exact
  remote probe commands — including the docker build, `--version` checks at
  the ENV paths, `/app/scripts` listing, the in-image TS driver run, and the
  in-image Rust Pass 2 run (server push path or `cargo test -p gyre-adapters
  real_rust_analyzer_extracts_known_call_edges` with RUST_ANALYZER_BIN from
  the built image) — are recorded in /tmp/stage/review-evidence/README.md.
- The graceful-degradation contract is untouched: where a toolchain is
  absent (e.g. bare-metal hosts), Pass 2 still warns and returns zero edges
  rather than failing a push.
