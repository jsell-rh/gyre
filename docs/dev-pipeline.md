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
before cleanup; clone retries use the existing sandbox.

Completion requires independent review, deterministic checks, GitHub policy
approval, an exact-head merge, and observation of the resulting upstream commit.
Local model verdicts and successful process exits alone never mean delivery.

## Run and control

```bash
python3 scripts/dev-pipeline.py slots 8
python3 scripts/dev-pipeline.py serve
node scripts/loop-dashboard.mjs
```

The cockpit listens on `http://127.0.0.1:7690`. Stop the supervisor with Ctrl-C;
already claimed stages finish independently. Restarting `serve` adopts their
leases and discovers pending work. Set slots to zero to drain model work.

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
0600 and contain only exported credential assignments, such as
`OPENSHELL_OIDC_CLIENT_SECRET`. Environment values take precedence. Configure the
gateway and providers with `scripts/dev-gateway.py --inference` before admission.
Model discovery is not used: the EnMaaS GLM model is pinned explicitly.

The pinned worker image includes Chromium and its system dependencies matching
Playwright 1.58.2 in the web lockfile. Rebuild the image when that version changes.
Browser dependencies do not require runtime downloads or system writes. Individual
probes still need to check the sandbox's actual process and network capabilities.

`seed /absolute/path/task-123.md --candidate SHA --base SHA` retains an existing
unfinished checkpoint. `--pr URL` associates an existing open PR at that exact
candidate. Imported progress labels do not grant approval: the replacement
requires fresh independent review, verification, and authoritative merge
observation. Logs and recovery artifacts live under `attempts/WORK/TOKEN/`.

## Retry and repair

Infrastructure errors retain the same work identity and use exponential backoff
from five seconds up to fifteen minutes. Pending autoscaler capacity waits within
the existing sandbox. Failed Git clones retry there. No failure retains a sandbox
for debugging: artifacts are captured locally, deletion is attempted immediately,
and cleanup retries unresolved deletion while the reservation remains charged.

A code, review, rebase, verification, or candidate CI defect becomes a structured
finding and a new implementation assignment on the retained source. Findings
carry exact source identity and bounded diagnostic evidence. Reproducible failures
on upstream main become deduplicated prerequisite repair tasks. Successful review
resolves findings for the approved revision. A moved upstream base requires new
verification before merge. Publication polls GitHub checks and requests a normal
protected merge with an exact head match; completion is recorded only after the
resulting upstream commit and its verified tree are observed.

Candidate CI repair starts from the exact verified PR head. Current candidate
logs are kept separate from baseline failures; bounded logs and screenshot
artifacts are staged inside the repair sandbox. Pending checks and merge
confirmation are expected observations, polled every 30 seconds without
accumulating infrastructure failure backoff.
