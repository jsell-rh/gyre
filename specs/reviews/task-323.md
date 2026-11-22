# Review — task-323 (Provision the Pass 2 toolchains in the server container image)

Spec: `specs/system/lsp-call-graph.md` §11 Prerequisites (spec gap spec-b89674dbcb695eb34e8d5e15 / server-image-pass2-toolchains).
Candidate under review: `1e70a979a7583f21bb0a61de62adabf8e5826c13` (branch `pipeline/task-323/a2e23c6397e34f3aa55bbd54613c00cf-1`, base `c42bfe033949ef04f8c4c41681126f116ab6a9de`; product commit `c148189a`). The working tree carries only the executor's protected-task-text restoration of `specs/tasks/task-323.md`, which is not part of the product diff.
Verdict: **approved** — no findings. Every acceptance criterion reachable in this sandbox was independently verified; the docker build and the two in-image executions are recorded as mandatory remote-verifier checks (docker is not installed here and static.rust-lang.org is network-blocked — infrastructure restrictions, not code defects).

## Diff scope

`git diff --stat base..candidate`: Dockerfile (+45/-8), docs/server-config.md (+5/-2), specs/coverage/system/lsp-call-graph.md (row §11 → `implemented`), specs/tasks/task-323.md (Shipped section). **No driver code, adapter discovery logic, or scripts/*.mjs changed** — the task's "do not alter the Pass 2 driver code or the adapters' discovery logic" constraint holds; the image satisfies the existing discovery contracts.

## Required behavior — all five sub-items present

1. **rust-analyzer, pinned, wired per discovery contract.** New `rust:1.87-slim` stage (same pinned base as the builder, so the binary's glibc matches the bookworm runtime — the same compatibility the compiled server binary already relies on) runs `rustup component add rust-analyzer` and copies the real toolchain binary (not the rustup proxy shim) via `cp "$(rustup which rust-analyzer)"`. Runtime: `COPY --from=rust-analyzer ... /usr/local/bin/rust-analyzer` + `ENV RUST_ANALYZER_BIN=/usr/local/bin/rust-analyzer` — exactly the first branch of `find_rust_analyzer()` (call_graph_rust.rs:269-282, `RUST_ANALYZER_BIN` → PATH), mirroring GO_CALLGRAPH_BIN/PYRIGHT_LANGSERVER_BIN wiring.
2. **typescript-language-server + typescript, pinned, wired per discovery order.** New `ts-tools` stage: `node:22-bookworm-slim` + `npm install -g typescript@5.8.3 typescript-language-server@4.3.4` (explicit pins, pyright@1.1.414 stage pattern). Runtime copies both `node_modules` packages, symlinks `/usr/local/bin/typescript-language-server` → `lib/cli.mjs`, and sets `TS_CALLGRAPH_SERVER` (the authoritative env branch of `findLanguageServer`, ts-callgraph.mjs:361) and `TS_CALLGRAPH_TSSERVER=/usr/local/lib/node_modules/typescript/lib` (the authoritative env branch of `findTsserverLib`, validated against `tsserver.js`).
3. **Driver script copied.** Dockerfile COPY line now includes `scripts/ts-callgraph.mjs` alongside the Python drivers into `/app/scripts/`; with runtime `WORKDIR /app`, the adapter's `find_script` (call_graph.rs:169-200, walks up from cwd) resolves all three Pass 2 drivers — same proven pattern as the Python drivers.
4. **Stale comment replaced.** "Rust/TS toolchains are task-073/075 scope" is gone; the comment now states the toolchains are wired in.
5. **Pins explicit, everything else unchanged.** rust:1.87-slim, node:22-bookworm-slim, typescript@5.8.3, typescript-language-server@4.3.4 — no floating tags. Non-root `USER gyre`, Go/pyright wiring, graceful-degradation contract all untouched.

## Probes run (evidence: /tmp/stage/review-evidence/)

- **TS Pass 2 end-to-end with the exact image wiring (PASS).** Installed the exact pinned packages (`npm install -g typescript@5.8.3 typescript-language-server@4.3.4`), created the Dockerfile's symlink (`bin/typescript-language-server` → `lib/cli.mjs`), set `TS_CALLGRAPH_SERVER`/`TS_CALLGRAPH_TSSERVER` to the Dockerfile's values, and ran the real `scripts/ts-callgraph.mjs` on a fixture repo with one known call site: returned `[{"from":"src/app.compute","to":"src/math.add","line":4}]`, exit 0. This exercises the exact env-override discovery branches the image selects, with the symlink spawned directly (as the driver does) — proving the shebang/exec-bit/layout claims.
- **Pinned package layout (PASS).** `typescript-language-server@4.3.4`'s own package.json bin is `./lib/cli.mjs` (the symlink target is the package's official entry point); `cli.mjs` starts `#!/usr/bin/env node`, mode 755; `typescript@5.8.3` `lib/tsserver.js` exists at the `TS_CALLGRAPH_TSSERVER` dir; `--version` at the wired paths → 4.3.4 / Version 5.8.3.
- **Source-contract verification (PASS).** `find_rust_analyzer` and `find_script`/`findLanguageServer`/`findTsserverLib` read at the candidate; the Dockerfile ENV values satisfy each first branch. All COPY sources exist in the repo; no `.dockerignore` excludes them.
- **No Rust/JS code changed** → no cargo/vitest probes needed (diff-scoped); no mechanical check scripts affected (they scan source code, which is untouched).

## Infrastructure restrictions (recorded, not defects)

Docker is not installed in this sandbox, and static.rust-lang.org is network-blocked with a root-owned `RUSTUP_HOME` lacking the component, so a real rust-analyzer binary could not be fetched and the image could not be built here. Per the review contract these are transport restrictions, not code defects.

## Verifier must run (from /tmp/stage/review-evidence/README.md)

1. `docker build -t gyre-server-task323 .` — must succeed.
2. `docker run gyre-server-task323 /usr/local/bin/rust-analyzer --version` — exit 0.
3. `docker run gyre-server-task323 /usr/local/bin/typescript-language-server --version` — 4.3.4.
4. `docker run gyre-server-task323 ls /app/scripts/` — ts-callgraph.mjs + both Python drivers; `gyre-go-callgraph` present.
5. In-image TS Pass 2 on a one-call-site fixture via `node /app/scripts/ts-callgraph.mjs` — non-zero edge (independently reproduced outside the image above).
6. In-image Rust Pass 2 — extract the image's rust-analyzer and run `RUST_ANALYZER_BIN=<bin> cargo test -p gyre-adapters real_rust_analyzer_extracts_known_call_edges` (call_graph_rust.rs:1230), or push a fixture Rust repo through the server's extraction path — non-zero Calls edges.
7. Non-root user (`id -u` ≠ 0) and unchanged GO_CALLGRAPH_BIN/PYRIGHT_LANGSERVER_BIN wiring.

## Non-blocking observations

- Coverage row §11 is marked `implemented` with the docker build + in-image execution explicitly disclosed in the row as pending remote verification. The production wiring is real, follows the proven pyright pattern, and the TS half was executed end-to-end in this review; the disclosure is honest, so this is not fake completion. If the verifier's docker build or in-image probes fail, the row must be reverted to task-assigned.
- `TS_CALLGRAPH_TSSERVER` as an image ENV is authoritative and therefore overrides a repo-local TypeScript install for analyzed repos. This is exactly what the task text prescribes ("set ENV TS_CALLGRAPH_TSSERVER to the typescript lib dir") and mirrors the pyright wiring; noted for the record only.
