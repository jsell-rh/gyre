# Development reconcilers

Each stage independently discovers work, atomically claims it, runs one operation,
and records its outcome. A supervisor only wakes stages and limits admission;
it does not contain task transitions. Periodic discovery repairs missed events.

The stages are triage, implementation, independent review, verification,
publication/merge, and cleanup. Review and verification bind to an exact candidate
and upstream base. Failures produce durable findings consumed by implementation.
Infrastructure failures retry with backoff without changing the task contract.

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
unconfirmed deletion stays charged and is retried independently. A pending
allocation is retained across fullsend's readiness retries. A deleted sandbox
cannot be recreated within the same attempt. A failed run's source checkpoint
is explicitly incomplete and cannot approve itself. Cleanup also fences and
stops an orphaned host runner using its boot ID, PID, and process start time.

The controller prepares a self-contained checkout on the exact assignment branch.
A host validator checkpoints and publishes actual source before deletion;
source guards, exact-candidate independent review, host verification, CI repair,
and protected merge observation remain controller responsibilities. Raw fullsend
logs/transcripts are retained and Pi events feed the existing cockpit stream.
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
node scripts/loop-dashboard.mjs
```

The cockpit listens on `http://127.0.0.1:7690`. Stop the supervisor with Ctrl-C;
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
python3 scripts/dev-pipeline.py tick cleanup
python3 scripts/dev-pipeline.py status --json
python3 scripts/dev-pipeline.py retry task-123
```

`discover STAGE` queues eligible work; `run STAGE` claims one queued item;
`tick STAGE` does both. Concurrent callers share the same database and atomic
claims. `serve --task task-123` restricts dispatch to that task and its transitive
prerequisites. The global slot budget covers all model stages together; the host
verification and upstream merge budgets each default to one. Stage limits can
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

The pinned worker image includes Chromium and its system dependencies matching
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
and cleanup retries unresolved deletion while the reservation remains charged.

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
