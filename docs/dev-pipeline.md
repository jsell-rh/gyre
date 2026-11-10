# Development reconcilers

Each stage independently discovers work, atomically claims it, runs one operation,
and records its outcome. The Go dispatcher extracted from HyperShell supplies
scheduling, worker monitoring, and its terminal cockpit. Its adapter calls Gyre's
existing stage handlers; periodic discovery repairs missed events. The previous
Python scheduling loop has been removed.

The extracted source, license, and pinned provenance live in
`tools/fullsend-dispatcher/`. It supports an adapter backend so Gyre retains
`pipeline.sqlite3` as its single workflow authority; it does not create a second
jobs database or import HyperShell's state. The original SQLite backend remains
available to the source integration.

Admission reserves one shared compute slot atomically with a stage claim, before
starting a worker, cloning source, creating providers, or invoking fullsend.
Implementation, review, triage, and verification share this budget. A full pool
leaves work queued without incrementing its attempt or failure count. The
reservation transfers atomically to the actual sandbox; pending deletion remains
charged. Lowering slots drains new admission while already admitted work finishes.
Cleanup and publication remain schedulable when every compute slot is occupied.

The stages are triage, implementation, independent review, verification,
publication/merge, CI observation, and cleanup. Review and verification bind to an exact candidate
and upstream base. Failures produce durable findings consumed by implementation.
Infrastructure failures retry with backoff without changing the task contract.

## Thin workers and runtime recovery

Build-capable assignments request one CPU and 4 GiB RAM. Cargo builds and Rust
unit tests use one worker; the guarded frontend test command also serializes
Vitest files. Thin-worker dev/test builds omit DWARF debug symbols (test
behavior and debug assertions remain enabled) to reduce memory and workspace
footprint. A confirmed OOM raises only that task/stage to 8 GiB for its next
assignment. An OOM at 8 GiB requires attention (including across main advances and partial checkpoints); connection failures never raise
memory. Compute choices and the triggering evidence remain in the ledger.
Build workspaces need 30 GiB persistent disk; this is separate from worker RAM.
With the scoped Kubernetes runtime context configured, the harness verifies
gateway ownership and PVC owner UID, expands only that sandbox's volume, and
waits for the mounted filesystem before starting builds. Other backends must
provision adequate workspace storage at the gateway. This does not alter
gateway-wide defaults. Disk or inode exhaustion is reported as storage failure,
never as transport loss or an OOM; identical attempts require an explicit retry.
After stopping an implementation agent, its generated build cache can be
reclaimed to leave room for saving source. Source and symlink targets are never
deleted by this reclamation.

An optional, read-only Kubernetes runtime observer uses `runtime_kube_context`
and `runtime_namespace` settings to capture workload termination reasons before
pods disappear. It only records pods matching existing owned resource records;
cluster diagnostics are not a prerequisite for healthy gateway execution. Read-only
cgroup counters also detect child-only OOM kills that leave the pod running;
a compiler failure by itself never raises memory.
Without an authoritative OOM and its killed allocation size, no memory escalation
is inferred. The cockpit reports the actual runtime failure instead of calling
an OOM a model or generic transport failure.

Agent polling refreshes expired gateway authentication. Failed polls preserve
process identity and output offsets while the sandbox remains Ready, bounded by
the assignment deadline. A stopped owned sandbox may restart once per claim;
trusted temporary inputs are restored and Pi resumes its persisted session.
Agent spool and build cache live on the workspace volume. Another stop requires
attention. OOM recovery can salvage source before retiring undersized compute;
it does not restart the coding process at the insufficient tier. Verification
also reconnects to its detached process and may resume after an owned restart.
Infrastructure backoff releases the integration lane so another reviewed
candidate can advance; claims and exact-source checks still gate publication.
Cleanup salvages source only from implementation assignments; reviewers and
exact-tree verifiers retain bounded logs and are purged without indexing their
generated output. Verification defaults to four hours. Exhausting that budget
requires inspection and an explicit retry, with a budget change if needed, rather than repeating the same cold
build automatically, including when main advances.

## Scheduling priority and reserved capacity

The harness uses non-preempting priority classes, inspired by Kubernetes
`PriorityClass` with `preemptionPolicy: Never`. A class changes queue order,
never authorization, dependencies, backoff, source identity, or running claims.
Built-in values are `background=-100`, `normal=0`, `high=100`, `repair=200`, and
`urgent=300`. CI, verification, rebase and review findings automatically use
`repair`; that priority survives incomplete implementation checkpoints until
fresh review resolves the findings. Cleanup, PR observation, publication and
verification have protected system priorities above user classes.

Apply exactly one `priority/<class>` label to a task's associated GitHub PR,
including a PR whose failed CI is already queued for implementation. The
observer refreshes these labels without repeating the failed CI handoff. A
cockpit task's Priority selector or the CLI below sets a durable local override
that takes precedence over PR labels; clear it to return to labels/automatic
priority. Unknown classes and conflicting labels block new compute admission
for that task and appear as a scheduling condition; control/cleanup continue.

```bash
python3 scripts/dev-pipeline.py priority task-063 urgent
python3 scripts/dev-pipeline.py priority task-063 --clear
python3 scripts/dev-pipeline.py priority-classes
python3 scripts/dev-pipeline.py priority-class hotfix 500
python3 scripts/dev-pipeline.py reserve-verification 1
```

Classes accept values from -1000 to 1000. Unlike strict Kubernetes priority,
the harness adds one point per minute spent queued, with FIFO ties. Execution,
retry backoff, and obsolete work do not accumulate queue age. Priority changes
do not reset queue age or invalidate independent review. The claim event records
the actual class/score used for admission. Custom classes and overrides live
only in this Gyre ledger, separate from another local dispatcher's settings.

One physical/admission slot is reserved for verification by default. Other model
stages cannot borrow it even when the verifier is idle; that is what guarantees
a finishing stage can run without evicting a four-hour coding assignment.
Reservations count pending creation/deletion, share the existing total sandbox
budget, and are checked atomically with claims. The reservation clamps to zero
for a single-slot deployment so it can bootstrap; zero total slots drains all
compute. Changing reservation/priority lets existing workers finish naturally.
The cockpit shows the reservation, class, and class source. These are host
scheduling controls, not PriorityClass objects in the remote Kubernetes cluster.

The adapter ledger owns cross-stage admission. The Go dispatcher preserves
per-work backoff and deduplication, while the atomic claim permits CI observation
alongside verification and cleanup alongside model work. Native item serialization
must not suppress these independently safe operations.

CI observation is a separate, sandbox-free stage bound to the recorded published
head and its verified base. It can run alongside a verifier, but never races a
branch publisher. A completed failing check is actionable even while sibling
checks remain pending or main advances. Head and check state are rechecked after
evidence collection. A confirmed candidate failure retains the failed head and
logs, requests implementation, and fences any now-obsolete verifier. Pending CI
waits. If an Actions check contradicts itself (pending status with a terminal
conclusion and completion timestamp), the observer requires the exact job and
completed current workflow attempt on the published head before treating it as
terminal. Raw observations and reconciliation evidence are retained; missing,
changed-head, or ambiguous evidence stays pending. GitHub still enforces all
required checks at merge. A merged PR goes directly to confirmation of its
original verified tree; its own merge must not start another verification.
Confirmed PR conflicts (`DIRTY` plus `CONFLICTING`) become durable rebase
findings immediately, independently of CI readiness or integration-lane ownership.
The observer rechecks the open PR, exact published head, and current target base
before handing off. Implementation resumes the retained published head against
current main, resolves conflicts, and must obtain fresh review and verification
before the existing PR can merge. The handoff fences obsolete verification,
releases integration, and creates one repair assignment; later polls cannot
create duplicate repairs. Unknown mergeability and genuine policy waits remain
observations. Upstream failures and transient CI infrastructure retain their
distinct recovery paths. PR readiness is visible separately from a successful
review run.

A durable **integration lane** admits one candidate at a time to final
verification, CI, and merge. Implementation and independent review remain
parallel up to the normal shared sandbox budget. The lane survives worker
restarts, infrastructure retries, and gaps while GitHub checks run; repair,
delivery, cancellation, or a changed contract/candidate releases it. Queued
candidates pick up current main when admitted. This prevents the controller's
own merges from repeatedly invalidating one another's full integration checks.
If an external merge advances main, the current lane owner first observes its
published CI and then refreshes integration once needed. Independent review is
retained for the unchanged candidate. An exact-head CI or policy wait lasting
15 minutes (`integration_wait_timeout` seconds) yields the integration lane so
other candidates can finish. Sandbox-free observation continues; that task may
rejoin only after its external gate changes to ready or merged. Receipt changes
reset the wait, but repeated polls do not. Yielding grants no merge permission
and discards no candidate or review. A PR with green exact-head CI and GitHub
merge state `BEHIND` is eligible for base refresh rather than parked: updating
that base is our integration work, and waiting for GitHub to become ready first
would deadlock. Pending CI and other policy holds remain parked. The cockpit
lists waiting tasks separately.
Checks on a different merge tree are
never reused as proof of the final tree; protected exact-head GitHub checks
and authoritative upstream-tree comparison still gate delivery.

Generated `web/dist` is produced by final integration, not coding workers.
The executor discards worker bundle changes against the assigned base and
mechanically resolves generated-only merge conflicts. Actual source conflicts
still go to implementation. For an already-published PR with green checks, the
observer reproduces conflict paths using Git only; generated-only conflicts
request integration refresh without an LLM, preserving the source review.
Classification is bound to both refs and rechecked after inspection.

The remote verifier builds assets from the integrated source, runs the Rust and
frontend suites, and publishes the generated build on its claim-specific branch.
The host verifies a linear artifact commit changes only `web/dist`, followed by
exactly the assigned task's append-only commit attribution. No other source or
requirement change is accepted. The final receipt records the tested source head
and published artifact tree; GitHub CI and exact-tree merge confirmation apply
to the final published head. Generated files are never merely chosen from an old
branch and shipped without rebuilding.

Model assignments default to four hours (`model_timeout: 14400` seconds in the
private ledger). Existing assignments retain the harness deadline they started
with. A failed run records `diagnostic.json`: the runner's actual timeout budget,
agent and runner exit codes when available, inference error category, and names
of unfinished tools. Credentials, tool arguments, and model reasoning are not
copied into these diagnostics. The cockpit's retry reason uses this diagnosis.

Interrupted independent reviews resume saved notes and bounded probe evidence
only when the candidate and assignment still match and protected source was not
changed. The original candidate remains the checkout; previous verdicts are
never restored. Workspace-wide Cargo commands are rejected in review and remain
the deterministic verifier's responsibility. Focused package/test probes remain
available with the longer assignment budget.

Historical generated static-repair tasks first enter model-free triage. A
strictly recognized `bash scripts/check-*.sh` probe runs on exact current main
in a disposable gateway sandbox. Its receipt is shared across matching repairs
for that main revision and worker image. A passing probe records **resolved**,
distinct from a shipped merge; failures stay repair work, and infrastructure
errors resolve nothing. Resolved prerequisites unblock their dependents without
creating a documentation-only PR. New repair identities use the reproduced
probe/failure rather than incidental environment paths; identical unresolved
failures share a prerequisite even as main advances.

SQLite is the local authority for work, leases, findings, resource reservations,
and observations. Claim tokens fence late results. GitHub is authoritative for
published branches, checks, and merges. Gateway observations are authoritative
for compute. External operations happen outside database transactions and have
stable identities so an uncertain response can be adopted after restart.

Sandbox reservations include pending creation and deletion. Expiring a worker
lease does not release its compute reservation. Cleanup releases capacity only
after the gateway confirms deletion. Source checkpoints and logs are captured
before cleanup. The input clone happens on the host before compute is allocated.

Completion requires independent review, deterministic checks, GitHub policy
approval, an exact-head merge, and observation of the resulting upstream commit.
Local model verdicts and successful process exits alone never mean delivery.

## Fullsend worker execution

The controller invokes pinned fullsend 0.50.0 for triage, implementation, and
independent review. `.fullsend/` declares the Pi runtime, prompts, policy, and
EnMaaS route. `toolchain.json` pins release archive/binary checksums and the
public worker image digest. The system's independently installed fullsend is
not overwritten; modified or mismatched runners are rejected before allocation.

The model is `gateway/rits/zai-org/glm-5-3`. Fullsend strips `gateway/` and calls
`https://api.enmaas.devshift.net/v1/chat/completions`, using the inline pinned
OpenAI-completions model entry and `supportsDeveloperRole: false`. The configured
gateway URL intentionally omits `/v1`: fullsend appends the API path. No model
catalog lookup or fallback to Claude/Vertex is involved.

Fullsend creates and retires its run-scoped inference provider. The key is read
from the existing secret-store entry or `ENMAAS_API_KEY`, passed only in the host
process environment as `FULLSEND_INFERENCE_GATEWAY_API_KEY`, and never put in a
harness, prompt, checkout, or host-file upload. `GH_TOKEN` comes from the local
GitHub environment or `gh auth token`; fullsend configures its GitHub provider.
No GCP credential file is needed.

A narrow OpenShell adapter records sandbox/provider intent before the external
create, attaches the claim's ownership labels, enforces shared compute capacity,
and refreshes expired gateway authentication. Fullsend's normal cleanup runs;
unconfirmed compute release stays charged and deletion is retried independently. A pending
allocation is retained across fullsend's readiness retries; a repeated create
adopts only the existing claim's ownership-labeled sandbox. Terminal/deleted
allocations cannot be recreated within an attempt. A deleted sandbox
cannot be recreated within the same attempt. A failed run's source checkpoint
is explicitly incomplete and cannot approve itself. Cleanup also fences and
stops an orphaned host runner using its boot ID, PID, and process start time.

The controller prepares a self-contained checkout on the exact assignment branch.
A host validator checkpoints and publishes actual source before deletion;
source guards, exact-candidate independent review, remote verification, CI repair,
and protected merge observation remain controller responsibilities. Raw fullsend
logs/transcripts are retained and Pi events feed the existing cockpit stream.
Pi runs under a detached supervisor with a bounded deadline inside the sandbox.
Stdout and stderr spool separately and clients resume by verified byte offsets.
Idle readers long-poll rather than creating a busy RPC loop for every worker.
A lost launch acknowledgement or broken RPC reconnects to the same claim/command
identity; it cannot start another coding process. Transport failures retry with
backoff for up to five minutes before normal source capture and sandbox cleanup.
A genuine process failure or incomplete verdict remains a failure. Local
checkpoint adoption also records its originating attempt and a bounded log tail;
a missing log cannot discard an otherwise recoverable checkpoint.
The large trusted input checkout is removed after execution, keeping receipts
and diagnostics without retaining duplicate repository clones indefinitely.

## Run and control

```bash
python3 scripts/dev-fullsend.py install
python3 scripts/dev-fullsend.py check       # local checks; no model request
python3 scripts/dev-fullsend.py check --live # gateway auth + pinned endpoint probe
python3 scripts/dev-pipeline.py resume
python3 scripts/dev-pipeline.py serve --task task-123 # first live canary
# Once the canary is independently reviewed, verified, and merged:
python3 scripts/dev-pipeline.py serve
# Equivalent terminal cockpit:
make fullsend
node scripts/loop-dashboard.mjs
```

The browser cockpit listens on `http://127.0.0.1:7690` and remains connected to
the same ledger. Type a sandbox count (0–1000) in the header and press Enter or
Apply; Escape cancels the edit. Zero drains new work. The +/− controls also remain
available. The terminal cockpit offers filtering, live logs, attempt
history, retry, and cancellation. Press `?` for keys; `q`/Ctrl-C detaches while
workers continue. In a run log, `/` filters source lines, `m` toggles Markdown
for agent prose, `w` toggles wrapping, and `a` toggles command colors (`NO_COLOR`
is respected). Left/right arrows pan long lines. Scrolling up pauses following
without moving the reading position as output arrives; `f` or `G` returns to
live output. The bordered pane shows follow state, active modes, and line/column
ranges, including on short terminals. Full logs remain available rather than
only the last 64 KiB.

The terminal's **Configuration** tab is the third tab (`Tab`/`Shift-Tab`), or
press `,` from a run/list view. Select a setting with `j`/`k`, press Enter to
edit, Enter to save, and Escape to discard. It edits the same private SQLite
ledger as the browser and CLI, with no HyperShell preference file. External
changes refresh automatically; an edit based on an older value is rejected
rather than overwriting another controller's change.

- Capacity, pause state, verification reservations, and per-stage concurrency
  affect new admission. Existing workers finish. An empty stage limit inherits
  total capacity; limits never bypass global admission or reservations.
- Agent and verification deadlines apply to new assignments, in seconds.
- Verification commands inherit the configured verification deadline; no shorter
  hidden compiler timeout discards a healthy thin worker's cache.
- Retry base and ceiling affect future exponential-backoff schedules, retaining
  jitter. Existing wake times and explicit stage-specific delays remain intact.
- Model, repository, gateway, and last successful gateway health check are
  read-only. The pinned model must agree with inference routing and admission;
  credentials remain outside the editor.

`serve --plain` selects text output. Stop the supervisor with Ctrl-C;
already claimed stages finish independently. Restarting `serve` adopts their
leases and discovers pending work. `pause` stops new model-stage dispatch while
preserving the desired slot count; `resume` enables it again. Slots zero also
drains model work. Migration leaves model admission paused, preserving the saved
82-slot target. Begin with a task whose dependencies are already delivered;
remove `--task` only after the real GLM canary has merged.

Stages can also be run independently, including from a timer:

```bash
python3 scripts/dev-pipeline.py tick triage
python3 scripts/dev-pipeline.py tick implement
python3 scripts/dev-pipeline.py tick review
python3 scripts/dev-pipeline.py tick verify
python3 scripts/dev-pipeline.py tick publish
python3 scripts/dev-pipeline.py tick observe
python3 scripts/dev-pipeline.py tick cleanup
python3 scripts/dev-pipeline.py status --json
python3 scripts/dev-pipeline.py retry task-123
```

`discover STAGE` queues eligible work; `run STAGE` claims one queued item;
`tick STAGE` does both. Concurrent callers share the same database and atomic
claims. `serve --task task-123` restricts dispatch to that task and its transitive
prerequisites. The global slot budget covers all model stages together; verification uses the same shared sandbox budget; the upstream merge budget
defaults to one. Stage limits can
be set in the private state's `limits` setting.

Private state defaults to `.gyre-pipeline/`; override with `--state /absolute/path`
or `GYRE_PIPELINE_STATE`. Optional `gateway.env` in that directory must have mode
0600 and contain only credential assignments, such as
`OPENSHELL_OIDC_CLIENT_SECRET`. Environment values take precedence. The gateway
name is the private state's `gateway` setting (default `gyre-gyre`); every
fullsend subprocess is bound to that gateway. A gateway-scoped automation identity
allows unattended authentication refresh. Do not replace it with interactive
browser auth for an unattended supervisor. Fullsend configures its providers;
`dev-gateway.py --inference` is the older standalone setup helper, not a required
step of the fullsend harness.

Gateway authentication refresh is coordinated under the private ledger's lock.
The controller proactively refreshes at most every five minutes, and known
expired-token errors request a forced refresh before retrying the operation.
Token-service failures retry with exponential backoff (5 seconds to 5 minutes).
A real read-only gateway request must succeed before cached authentication can
be used during refresh failures; known-expired credentials never get that
fallback. The cockpit reports degraded cached operation and the next retry.
Status files contain timing/error types only, not credentials, and are scoped
to this ledger and gateway alias so another local dispatcher is unaffected.

## Isolation from another local dispatcher

Gyre builds its host orchestration binary into
`.gyre-pipeline/tools/gyre-dispatcher-<source hash>` under a build lock. This does
not overwrite `fullsend`, OpenShell, or HyperShell's cached dispatcher. Only the
host orchestration tool is compiled locally; product builds and verification
continue on the OpenShell gateway.

The dispatcher uses Gyre's `supervisor.lock`, `dispatcher/logs`, existing attempt
directories, and a scope derived from this ledger's owner UUID. It never uses
HyperShell's XDG state, `.hypershell` directory, labels, Jira configuration, or
Vertex credentials. Adapter selection is explicit per process; an inherited
`FUNNEL_ADAPTER` cannot redirect Gyre. Host temporary files/profile caches use
`dispatcher/tmp`, and Pi's EnMaaS model is pinned on the fullsend command line
so another project's exported model cannot override it. The configured gateway is explicit, and an
inherited endpoint override is cleared without changing another process's
environment or the machine's default gateway selection. A private local OIDC
registration (`gyre-d-<instance hash>`) snapshots the connection metadata and
mints its own token; it does not copy a refresh token or modify the source
registration. This also isolates authentication when HyperShell uses the same
physical gateway. Gyre's persistent GitHub provider and profile use
`gyre-gh-<instance hash>` instead of the shared `github` name. Its credentials are reconciled under a
private lock: unchanged values are health-checked, never rewritten by each
Fullsend worker. A changed credential is updated once. This avoids unnecessary
provider generations propagating to running sandboxes. Per-run inference
providers remain separate and are purged normally. Sandbox/provider cleanup
retains the existing owner and claim checks.

Existing stage workers are adopted from their valid leases, not relaunched.
Cancellation is bound to a work ID and claim token; it pauses further work on
that task until explicit retry. Workers predating cancellation support are
monitored until they finish normally. Retry and history requests must match the
ledger's scope, so a different local dispatcher cannot control these attempts.

The pinned worker image includes the project-configured `clang` and `mold`
linkers, plus Chromium and its system dependencies matching
Playwright 1.58.2 in the web lockfile. Rebuild the image when that version changes.
Browser dependencies do not require runtime downloads or system writes. Individual
probes still need to check the sandbox's actual process and network capabilities.
Remote Chromium probes may need `--no-zygote` and `--disable-dev-shm-usage` because
the sandbox restricts process credentials and system shared-memory writes. These
are probe runtime options; repository tests and GitHub assertions stay intact.
Before inference, the executor measures browser installation and accepts a real
loopback peer with a bounded socket probe. It records the exact failing operation
and errno, gives the report to the model, and streams it in the cockpit. Merely
binding a socket or accepting an empty nonblocking queue is insufficient proof
of working server transport.
The npm registry policy permits encoded slashes for scoped package metadata
(`@scope%2fpackage`); registry requests remain read-only and restricted to the
listed binaries. Blocking those paths prevents ordinary dependency installs.

`seed /absolute/path/task-123.md --fresh` deliberately starts an unshipped task
from current main with new triage/implementation work identities. Drain its active
work first. Previous task state is archived under the private `task-resets/`
directory; source branches, stage receipts, and logs remain intact. The task
contract generation does not change. Baseline findings from older upstream bases
are excluded from current prompts. Normal reseeding preserves a valid metadata
triage receipt instead of leaving discovery stranded on a completed work item.

`seed /absolute/path/task-123.md --candidate SHA --base SHA` retains an existing
unfinished checkpoint. `--pr URL` associates an existing open PR at that exact
candidate. Imported progress labels do not grant approval: the replacement
requires fresh independent review, verification, and authoritative merge
observation. Logs and recovery artifacts live under `attempts/WORK/TOKEN/`.

## Retry and repair

Infrastructure errors retain the same work identity and use exponential backoff
from five seconds up to fifteen minutes. Pending autoscaler capacity waits within
the existing sandbox. Failed input clones retry before sandbox allocation. No failure retains a sandbox
for debugging: artifacts are captured locally, deletion is attempted immediately,
and cleanup retries unresolved deletion. Cleanup debt and compute occupancy are
tracked separately: a deleting allocation releases its compute slot only after
scoped Kubernetes inventories establish that its sandbox is suspended, deleting,
or absent and no matching pod remains. Sandbox state is read before pod state;
even a terminating pod remains charged. Evidence is bound to the resource claim,
cluster context, and namespace, and expires after three minutes. Unknown,
incomplete, or stale evidence keeps the slot charged. Gateway sandbox/provider
cleanup records and retries remain intact, and cleanup of the same task still
precedes a replacement attempt. The cockpit reports occupied slots separately
from pending deletions.

Model admission also checks the pinned inference endpoint before reserving a
sandbox. The shared probe uses `ENMAAS_API_KEY` or the local secret-store entry
`service=pricetag key=api-token`. Successful probes are cached for one minute;
failures back off up to fifteen minutes and appear as `InferenceUnavailable` in
the cockpit. A changed key bypasses the failed cache; fullsend uses it for the next run-scoped provider. Credentials and response bodies
are never included in these diagnostics. Set slots to zero to drain existing
work while replacing an expired credential, then restore the desired budget.

A code, review, rebase, verification, or candidate CI defect becomes a structured
finding and a new implementation assignment on the retained source. Findings
carry exact source identity and bounded diagnostic evidence. Reproducible failures
on upstream main become deduplicated prerequisite repair tasks. Successful review
resolves findings for the approved revision. A moved upstream base requires new
verification before merge. Publication polls GitHub checks and requests a normal
protected merge with an exact head match; completion is recorded only after the
resulting upstream commit and its verified tree are observed.
The shared merge permit survives unresolved queued or automatic merge requests.
Cleanup releases it when GitHub reports no pending request, including a strict
merge rejection after main advances, so one rejected request cannot block all
subsequent deliveries. Missing observations keep the permit reserved.

Candidate CI repair starts from the exact verified PR head. Current candidate
logs are kept separate from baseline failures; bounded logs and screenshot
artifacts are staged inside the repair sandbox. Pending checks and merge
confirmation are expected observations, polled every 30 seconds without
accumulating infrastructure failure backoff.

Shared CI setup failures are coordinated separately from product defects.
For Playwright's unsupported-runner failure, the observer verifies the terminal
Actions job/attempt and failed installation step, then compares workflow and npm
input blob identities against the exact upstream base. Product-test failures,
mixed failures, and candidates that changed those inputs remain candidate repairs.
An older green base run cannot establish the health of today's hosted runner.
Matching setup failures share one SQLite-deduplicated repair task, even as main
advances. Affected tasks retain their candidate, review, and product dependency
contract; they do not start individual implementation workers for that failure.
The shared repair receives automatic repair priority.

Once the repair ships, retained candidates re-enter verification against a base
containing the fix. If main already has different CI inputs when the failure is
observed, the observer requests that integration refresh directly, without a
repair model. This permits fresh verification only; it never marks CI green or
merges an old failing head. New publication still requires all GitHub checks.

Completing a repair clears its active finding but retains the original evidence
as review context. The independent reviewer receives the failure source identity,
bounded logs, and artifacts inside its own sandbox; it can verify the repair
without trusting the implementer's explanation or losing the original failure.

## Qualification without model access

Run `python3 -m unittest discover -s scripts -p 'test_pipeline*.py'`. These exercise
claims, fencing, dependencies, review and CI repair transitions, actual Git
checkpoint publication to a fixture remote, authentication retry, owned resource
admission, delayed deletion, and recovery. Scripted fixtures are infrastructure
proofs, not evidence that GLM produces correct code or an independent review.

Run `python3 scripts/prove-fullsend.py` for the live transport proof (requires
gateway and GitHub credentials, creates one sandbox and a temporary branch).
The proof uses fullsend's `dummy` runtime on an isolated task
catalog, never the production task queue. It invokes real OpenShell, uploads the
actual worker image and repository, writes a labeled fixture change, and exercises
the same checkpoint validator and sandbox teardown. It makes no inference request
and must not be recorded as a product delivery. The actual GLM tool-call stream,
independent semantic review, CI repair, and upstream PR merge must still be
qualified with a live canary after model access returns.

On 2026-10-29 the shared runner passed this proof against `gyre-fullsend` on hyp0
(OpenShell server 0.1.5-rhaiv.3, CLI 0.1.5-rhaiv.0). The gateway was created using
`network/public` and `provider/ibm` capability selectors, without requesting a
dedicated route. The host fetched and checked the published commit and tree,
then deleted the temporary branch with an exact Git lease. Deletion was accepted
for the sandbox; the gateway retained its `Deleting` record, still charged in
local resource accounting. Private evidence is in
`.gyre-pipeline/fullsend-proof-3fccf6f497da/proof.json`.

To rebuild the image:

```bash
docker build -t ghcr.io/jsell-rh/gyre-worker:fullsend-0.50.0 \
  -f docker/dev-worker/Dockerfile docker/dev-worker
docker push ghcr.io/jsell-rh/gyre-worker:fullsend-0.50.0
```

Pin the resulting digest in `toolchain.json` and verify anonymous manifest and
layer access. A new GHCR package defaults to private: a successful authenticated
push does not establish that the gateway can pull it.

## Remote verification and Fullsend hooks

Implementation and review use Fullsend/Pi. Verification is a model-free gateway
job using the same pinned worker image and shared sandbox reservations. The local
controller reconciles Git metadata and uploads a prerequisite Git bundle; Cargo,
source guards, npm builds, and tests execute only in the disposable verifier.
Verification uses the gateway resource defaults, just like coding workers;
explicit `verification_cpu` and `verification_memory` settings override them.
Every verifier gets an isolated build target, so cross-checkout host cache
invalidation is unnecessary. There is no host build fallback. Per-stage `limits`
can bound verification concurrency independently without changing desired slots.

The verifier checks the exact assigned head and tree, runs unchanged upstream
and candidate guards, all-target Clippy, the web build, all Rust tests, and frontend
tests. GitHub's required checks (including browser E2E) still gate merging. A
complete source-bound receipt is mandatory. Every received receipt is saved
privately as `remote-verification-raw.json` before validation and sandbox deletion;
only accepted receipts become `remote-verification.json`. Source-integrity
failures report changed paths and observed HEAD/tree, stop in Needs attention,
and release integration for other candidates. A new base cannot bypass that
hold; operator retry resumes it after inspection. Ordinary transport loss,
missing receipts, and timeouts retry as infrastructure. Concrete check failures become durable
implementation findings. When testing upstream's scripts against the candidate,
the verifier records which upstream-only files it introduces. Restoration removes
only those unchanged files, leaving candidate deletions intact; modified or
pre-existing files are never blindly cleaned. This prevents the verifier itself
from contaminating the exact checkout it later checks. Git retries reuse the
allocation. Logs stream to the cockpit and remain locally; compute is immediately requested for deletion, with
unconfirmed compute release charged until scoped runtime evidence proves absence. Slots zero drains
verification too. SQLite, gateway authentication, Git publication, and GitHub
observation remain in the lightweight local control plane.

In pinned Fullsend 0.50.0 (`internal/cli/run.go`), `pre_script` runs on the runner
before sandbox creation and supports a structured skip decision. `post_script`
runs on the runner **after sandbox cleanup**, and can be skipped on failed
validation or failed agent execution. These are useful for discovery/admission
checks and reporting downloaded results, respectively. They cannot replace
pre-deletion source capture or unconditional cleanup/reconciliation. Gyre already
uses Fullsend's `validation_loop.script` for successful source checkpoint
collection; its gateway boundary also captures incomplete checkpoints on failed
runs. Keeping those responsibilities separate preserves failed work and fenced
claims. Hook-based publication would also mix PR/merge responsibilities back
into model execution, so publication remains its independent stage.

Run a model-free full-gate proof against a committed candidate:

```bash
python3 scripts/prove-remote-verification.py --candidate HEAD
```

This creates isolated proof state, uses one gateway sandbox, runs real gates,
and cleans its temporary verified branch. It does not mark a product task
delivered, publish a PR, or alter production slots.
