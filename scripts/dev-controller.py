#!/usr/bin/env python3
"""Durable, parallel sandbox development controller."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import random
import re
import sqlite3
import shutil
import subprocess
import sys
import time
import uuid

ROOT = Path(os.environ.get("GYRE_DEV_ROOT", Path(__file__).resolve().parent.parent)).resolve()
STATE = Path(os.environ.get("GYRE_DEV_STATE", ROOT / ".gyre-dev-controller")).resolve()
SOURCE = STATE / "source"
TASK_RE = re.compile(r"^specs/tasks/(task-\d+)\.md$")
DEP_RE = re.compile(r"task-\d+")
HOST_GATE_PROCESSES = {}
GATEWAY_PROCESSES = {}
MAX_REPAIRS = 3
DISPATCH_TASK = None
PUBLICATION_MODE = os.environ.get("GYRE_DEV_PUBLICATION", "merge")
contract_spec = importlib.util.spec_from_file_location("dev_contract", Path(__file__).with_name("dev-contract.py"))
contract = importlib.util.module_from_spec(contract_spec)
contract_spec.loader.exec_module(contract)
coverage_spec = importlib.util.spec_from_file_location("dev_coverage", Path(__file__).with_name("dev-coverage.py"))
coverage = importlib.util.module_from_spec(coverage_spec)
coverage_spec.loader.exec_module(coverage)
ci_spec = importlib.util.spec_from_file_location("dev_ci", Path(__file__).with_name("dev-ci.py"))
ci = importlib.util.module_from_spec(ci_spec)
ci_spec.loader.exec_module(ci)


class SourceUnavailable(RuntimeError):
    """Remote source could not be refreshed; the next controller cycle may retry."""


def run(*args, cwd=None, check=True, capture=True, env=None, timeout=None):
    p = subprocess.run(args, cwd=cwd or SOURCE, text=True, stdout=subprocess.PIPE if capture else None,
                       stderr=subprocess.PIPE if capture else None, env=env, timeout=timeout)
    if check and p.returncode:
        raise RuntimeError(f"{' '.join(map(str, args))}: {p.stderr or p.stdout}")
    return p


def git(*args, **kw):
    return run("git", *args, **kw).stdout.strip()


def field(body, key):
    # Horizontal whitespace only: \s would eat a newline and steal the
    # first item of a multiline list (silently discarding later dependencies).
    match = re.search(rf"^{re.escape(key)}:[ \t]*(.*)$", body.split("---", 2)[1], re.M)
    return match.group(1).strip().strip('"') if match else ""


def deps(body):
    value = field(body, "depends_on")
    if value and value != "[]":
        return DEP_RE.findall(value)
    match = re.search(r"^depends_on:[ \t]*\n((?:[ \t]+- [^\n]*\n?)*)", body.split("---", 2)[1], re.M)
    return DEP_RE.findall(match.group(1)) if match else []


def db_open():
    STATE.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(STATE / "state.sqlite3")
    db.row_factory = sqlite3.Row
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript("""
    CREATE TABLE IF NOT EXISTS tasks (
      name TEXT PRIMARY KEY, progress TEXT NOT NULL, deps TEXT NOT NULL,
      state TEXT NOT NULL, seed TEXT, candidate TEXT, attempts INTEGER NOT NULL DEFAULT 0,
      retry_baseline INTEGER NOT NULL DEFAULT 0,
      retry_at INTEGER NOT NULL DEFAULT 0, condition TEXT);
    CREATE TABLE IF NOT EXISTS attempts (
      id TEXT PRIMARY KEY, task TEXT NOT NULL, kind TEXT NOT NULL,
      branch TEXT, sha TEXT, base TEXT, merge_sha TEXT,
      state TEXT NOT NULL, pid INTEGER, started INTEGER NOT NULL,
      ended INTEGER, detail TEXT, ready_observed INTEGER NOT NULL DEFAULT 0,
      host_pid INTEGER);
    CREATE TABLE IF NOT EXISTS events (
      id INTEGER PRIMARY KEY, at INTEGER NOT NULL, task TEXT, message TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS controller_health (
      id INTEGER PRIMARY KEY CHECK (id=1), failures INTEGER NOT NULL DEFAULT 0,
      retry_at INTEGER NOT NULL DEFAULT 0, admission INTEGER NOT NULL DEFAULT 1,
      condition TEXT NOT NULL DEFAULT 'Healthy');
    INSERT OR IGNORE INTO controller_health(id) VALUES(1);
    CREATE TABLE IF NOT EXISTS resources (
      name TEXT PRIMARY KEY, attempt_id TEXT, present INTEGER NOT NULL DEFAULT 0,
      phase TEXT, last_seen INTEGER, delete_pending INTEGER NOT NULL DEFAULT 0,
      retry_at INTEGER NOT NULL DEFAULT 0, failures INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS gateway_jobs (
      name TEXT PRIMARY KEY, kind TEXT NOT NULL, sandbox TEXT,
      state TEXT NOT NULL, pid INTEGER, started INTEGER NOT NULL,
      result TEXT NOT NULL, retry_at INTEGER NOT NULL DEFAULT 0,
      failures INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS task_reservations (name TEXT PRIMARY KEY, owner TEXT NOT NULL);
    """)
    if "retry_baseline" not in {row[1] for row in db.execute("PRAGMA table_info(tasks)")}:
        db.execute("ALTER TABLE tasks ADD COLUMN retry_baseline INTEGER NOT NULL DEFAULT 0")
    for column, definition in (("retry_at", "INTEGER NOT NULL DEFAULT 0"), ("condition", "TEXT"),
                               ("feedback", "TEXT"), ("repairs", "INTEGER NOT NULL DEFAULT 0"),
                               ("pr_url", "TEXT"), ("pr_number", "INTEGER"), ("generation", "TEXT"),
                               ("observed_generation", "TEXT"), ("candidate_generation", "TEXT"), ("candidate_at", "INTEGER")):
        if column not in {row[1] for row in db.execute("PRAGMA table_info(tasks)")}:
            db.execute(f"ALTER TABLE tasks ADD COLUMN {column} {definition}")
    if 'reconcile_failures' not in {row[1] for row in db.execute('PRAGMA table_info(tasks)')}:
        db.execute('ALTER TABLE tasks ADD COLUMN reconcile_failures INTEGER NOT NULL DEFAULT 0')
    for column in ("definition_path", "blocked_base", "origin_key", "audit_contract"):
        if column not in {row[1] for row in db.execute("PRAGMA table_info(tasks)")}:
            db.execute(f"ALTER TABLE tasks ADD COLUMN {column} TEXT")
    db.execute("CREATE UNIQUE INDEX IF NOT EXISTS task_origin_key ON tasks(origin_key)")
    if "ready_observed" not in {row[1] for row in db.execute("PRAGMA table_info(attempts)")}:
        db.execute("ALTER TABLE attempts ADD COLUMN ready_observed INTEGER NOT NULL DEFAULT 0")
    for column, definition in (("host_pid", "INTEGER"), ("bundle_sha", "TEXT"),
                               ("generation", "TEXT"), ("phase", "TEXT"), ("reason", "TEXT"),
                               ("host_started", "INTEGER")):
        if column not in {row[1] for row in db.execute("PRAGMA table_info(attempts)")}:
            db.execute(f"ALTER TABLE attempts ADD COLUMN {column} {definition}")
    for column, definition in (("inventory_at", "INTEGER NOT NULL DEFAULT 0"), ("inventory_error", "TEXT")):
        if column not in {row[1] for row in db.execute("PRAGMA table_info(controller_health)")}:
            db.execute(f"ALTER TABLE controller_health ADD COLUMN {column} {definition}")
    return db


def controller_owner():
    path = STATE / "owner"
    try:
        with path.open("x") as stream:
            stream.write(uuid.uuid4().hex + "\n")
    except FileExistsError:
        pass
    value = path.read_text().strip()
    if not re.fullmatch(r"[0-9a-f]{32}", value):
        raise RuntimeError("invalid controller owner identity; preserve state and repair its owner file")
    return value


def sandbox_name(attempt):
    if len(attempt['task'][5:]) > 3:
        # Base36 keeps future reserved task IDs within the gateway's 19 chars.
        number, encoded = int(attempt['task'][5:]), ''
        while number:
            number, digit = divmod(number, 36)
            encoded = '0123456789abcdefghijklmnopqrstuvwxyz'[digit] + encoded
        name = f"gyr-z{encoded or '0'}-{attempt['kind'][0]}-{attempt['id'][:8]}"
        if len(name) > 19:
            raise RuntimeError('task ID exceeds compact sandbox naming capacity')
        return name
    return f"gyre-{attempt['task'][5:]}-{attempt['kind'][0]}-{attempt['id'][:8]}"


def inventory_ready(db):
    gate = health(db)
    return bool(gate["inventory_at"] and not gate["inventory_error"] and time.time() - gate["inventory_at"] <= 120)


def resource_usage(db):
    names = {row["name"] for row in db.execute("SELECT name FROM resources WHERE present=1")}
    names.update(sandbox_name(row) for row in db.execute("SELECT id,task,kind FROM attempts WHERE state IN ('launching','running')"))
    return len(names)


def health(db):
    return db.execute("SELECT * FROM controller_health WHERE id=1").fetchone()


@contextmanager
def gateway_lock():
    with (STATE / "gateway-login.lock").open("a") as lock:
        deadline = time.monotonic() + 5
        while True:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise TimeoutError("gateway authentication lock busy")
                time.sleep(0.1)
        yield lock


def missing_provider_failure(attempt):
    path = STATE / "attempts" / attempt["id"] / "output.log"
    try:
        with path.open("rb") as log:
            log.seek(max(0, path.stat().st_size - 131072))
            output = log.read().decode(errors="replace")
    except OSError:
        return False
    return bool(re.search(r"provider ['\"]gyre-(?:github-rw|enmaas)['\"] not found", output))


def defer_missing_provider(db, task, attempt):
    db.execute("UPDATE tasks SET state='deferred',condition='Required provider missing',retry_at=0,retry_baseline=retry_baseline+? WHERE name=?",
               (int(attempt["kind"] == "worker"), task["name"]))
    db.execute("UPDATE attempts SET state='deferred' WHERE id=?", (attempt["id"],))
    event(db, task["name"], "required gateway provider missing; queued until provider setup is repaired")


def providers_ready(db):
    """Check shared prerequisites once per cycle before any sandbox is created."""
    env = dict(os.environ, OPENSHELL_NO_BROWSER="1", OPENSHELL_GATEWAY_INSECURE="true",
               OPENSHELL_WORKSPACE="default")
    cli = env.get("OPENSHELL", "openshell")
    reason = None
    try:
        with gateway_lock():
            login = subprocess.run([cli, "gateway", "login", "gyre-gyre"], env=env,
                                   capture_output=True, text=True, timeout=30)
        if login.returncode:
            reason = "ProviderCheckUnavailable: gateway login failed"
        else:
            result = subprocess.run([cli, "-g", "gyre-gyre", "provider", "list", "--names"],
                                    env=env, capture_output=True, text=True, timeout=30)
            if result.returncode:
                reason = "ProviderCheckUnavailable: gateway provider list failed"
            else:
                missing = {"gyre-enmaas", "gyre-github-rw"} - set(result.stdout.splitlines())
                if missing:
                    reason = "MissingProviders: " + ", ".join(sorted(missing))
    except (OSError, subprocess.TimeoutExpired):
        reason = "ProviderCheckUnavailable: gateway unavailable"
    previous = health(db)["condition"]
    if reason:
        db.execute("UPDATE controller_health SET condition=? WHERE id=1", (reason,))
        if previous != reason:
            event(db, None, reason + "; sandbox admission paused")
        db.commit()
        return False
    if previous.startswith(("MissingProviders:", "ProviderCheckUnavailable:")):
        db.execute("UPDATE controller_health SET condition='Healthy' WHERE id=1")
        event(db, None, "required gateway providers available; sandbox admission resumed")
    return True


def effective_admission(db, slots, running, now=None):
    """Existing work keeps its slots; a degraded gateway gets one probe."""
    now = int(time.time()) if now is None else now
    gate = health(db)
    if gate["condition"] == "ConfigurationInvalid" or gate["condition"].startswith(("MissingProviders:", "ProviderCheckUnavailable:")):
        return running
    if gate["failures"]:
        probe = db.execute("""SELECT 1 FROM attempts WHERE state='running' AND started>=?
                              LIMIT 1""", (gate["retry_at"],)).fetchone()
        return running if now < gate["retry_at"] or probe else min(slots, running + 1)
    return min(slots, max(running, gate["admission"]))


def backoff_seconds(failures):
    return min(900, 30 * 2 ** min(max(failures - 1, 0), 5))


def defer_infrastructure(db, task, reason, worker=False, now=None):
    """Keep code progress, but make gateway pressure a retryable condition."""
    now = int(time.time()) if now is None else now
    previous = health(db)
    failures = previous["failures"] + 1
    delay = min(900, max(1, round(backoff_seconds(failures) * random.uniform(0.8, 1.2))))
    retry_at = max(previous["retry_at"], now + delay)
    db.execute("UPDATE controller_health SET failures=?,retry_at=?,admission=1,condition=? WHERE id=1",
               (failures, retry_at, reason))
    db.execute("""UPDATE tasks SET state='deferred',retry_at=?,condition=?,
               retry_baseline=retry_baseline+? WHERE name=?""",
               (retry_at, reason, int(worker), task["name"]))
    event(db, task["name"], f"{reason}; waiting for gateway recovery, retry after {retry_at} (backoff {delay}s)")


def observe_ready(db):
    """Ramp admission only after a sandbox actually reaches Ready."""
    for attempt in db.execute("SELECT * FROM attempts WHERE state='running' AND ready_observed=0").fetchall():
        marker = STATE / "attempts" / attempt["id"] / "sandbox.ready"
        if not marker.exists():
            continue
        db.execute("UPDATE attempts SET ready_observed=1 WHERE id=?", (attempt["id"],))
        current = health(db)
        if current["condition"] == "ConfigurationInvalid" or current["condition"].startswith(("MissingProviders", "ProviderCheckUnavailable")):
            db.commit()
            continue
        running = db.execute("SELECT count(*) FROM attempts WHERE state='running'").fetchone()[0]
        db.execute("UPDATE controller_health SET failures=0,retry_at=0,admission=?,condition='Healthy' WHERE id=1",
                   (min(max(current["admission"] + 1, running + 1), 1000),))
        event(db, attempt["task"], "sandbox Ready; gateway admission increased")


def observe_phases(db):
    for attempt in db.execute("SELECT * FROM attempts WHERE state='running'").fetchall():
        path = STATE / "attempts" / attempt["id"] / "phase.json"
        try:
            value = json.loads(path.read_text())
            phase, reason = value["phase"], value.get("reason", "")
            if phase not in ("Provisioning", "Pending", "Staging", "Worker", "Check", "Deleting"):
                continue
        except (OSError, ValueError, KeyError):
            continue
        db.execute("UPDATE attempts SET phase=?,reason=? WHERE id=?", (phase, reason, attempt["id"]))
    db.commit()


def recover_prior_infrastructure_failures(db):
    """Migrate the old explicit-retry failures using evidence in their local logs."""
    for task in db.execute("SELECT * FROM tasks WHERE state='failed'").fetchall():
        attempt = db.execute("SELECT * FROM attempts WHERE task=? ORDER BY rowid DESC LIMIT 1",
                             (task["name"],)).fetchone()
        if not attempt or attempt["detail"] not in ("exit=75", "exit=77"):
            continue
        if missing_provider_failure(attempt):
            defer_missing_provider(db, task, attempt)
            continue
        path = STATE / "attempts" / attempt["id"] / "output.log"
        try:
            log = path.read_text(errors="replace")
        except OSError:
            continue
        if "ConfigurationInvalid" in log:
            continue
        if not any(marker in log.lower() for marker in
                   ("configurationpending", "provisioningtimedout", "h2 protocol error",
                    "tls handshake eof", "failed to connect to gateway", "transport interrupted persisted")):
            continue
        db.execute("UPDATE tasks SET state='deferred',retry_at=0,condition='Gateway recovery' WHERE name=?",
                   (task["name"],))
        if attempt["kind"] == "worker":
            db.execute("UPDATE tasks SET retry_baseline=attempts WHERE name=?", (task["name"],))
        event(db, task["name"], "recovered prior gateway failure; queued for automatic retry")


def event(db, task, message):
    db.execute("INSERT INTO events(at,task,message) VALUES(?,?,?)", (int(time.time()), task, message))
    db.commit()
    print(f"{task or 'controller'}: {message}", flush=True)


def queue_repair(db, task, attempt, log_name="output.log"):
    """Preserve a rejected candidate and give bounded, durable findings to its next worker."""
    directory = STATE / "attempts" / attempt["id"]
    directory.mkdir(parents=True, exist_ok=True)
    log = directory / log_name
    try:
        with log.open("rb") as stream:
            stream.seek(max(0, log.stat().st_size - 65536))
            findings = stream.read().decode(errors="replace")
    except OSError:
        findings = "No failure log was recorded. Reproduce the failure before changing code."
    feedback = directory / "repair.md"
    feedback.write_text(
        f"# Integration failure for {task['name']}\n\n"
        f"Rejected candidate: {attempt['sha']}\nChecked base: {attempt['base']}\n"
        f"Attempt: {attempt['id']}\nGate log: {log_name}\n\n"
        "Repair the candidate against current main. Diagnose whether the failure is caused by "
        "this task or already exists on main; do not weaken checks or claim unrelated failures fixed. "
        "Run a focused reproduction, repair the production behavior, and request independent review.\n\n"
        f"## Failure output (last 64 KiB)\n\n```text\n{findings}\n```\n")
    exhausted = task["repairs"] >= MAX_REPAIRS
    db.execute("""UPDATE tasks SET state=?,seed=?,candidate=NULL,feedback=?,
                  repairs=repairs+?,retry_baseline=attempts,retry_at=0,condition=? WHERE name=?""",
               ("failed" if exhausted else "ready", attempt["sha"] or task["seed"],
                str(feedback), int(not exhausted),
                "RepairLimitExceeded" if exhausted else "RepairPending", task["name"]))
    event(db, task["name"], f"{log_name} failed; " +
          (f"repair limit {MAX_REPAIRS} reached; operator attention required" if exhausted else
           f"queued implementation repair {task['repairs'] + 1}/{MAX_REPAIRS} from {attempt['sha']}"))


def recover_prior_quality_failures(db):
    for task in db.execute("SELECT * FROM tasks WHERE state='failed' AND feedback IS NULL").fetchall():
        attempt = db.execute("SELECT * FROM attempts WHERE task=? ORDER BY rowid DESC LIMIT 1",
                             (task["name"],)).fetchone()
        if not attempt or attempt["kind"] != "check" or not attempt["sha"]:
            continue
        gate_exit = STATE / "attempts" / attempt["id"] / "host-tests.exit"
        if gate_exit.exists() and gate_exit.read_text().strip() != "0":
            queue_repair(db, task, attempt, "host-tests.log")
        elif attempt["detail"] == "exit=1":
            queue_repair(db, task, attempt)


def source():
    try:
        # Dashboard retry/sync commands share this cache with the controller.
        # Git's individual ref locks cannot serialize two whole fetches.
        with (STATE / 'source.lock').open('a+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            if not SOURCE.exists():
                url = git("remote", "get-url", "origin", cwd=ROOT)
                git("clone", "--quiet", "--filter=blob:none", "--single-branch", "--branch", "main",
                    "--no-checkout", url, str(SOURCE), cwd=ROOT, timeout=90)
            git("fetch", "--quiet", "--filter=blob:none", "origin", "+refs/heads/main:refs/remotes/origin/main",
                "+refs/heads/worker/task-*:refs/remotes/origin/worker/task-*",
                "+refs/heads/devloop/task-*:refs/remotes/origin/devloop/task-*", timeout=90)
    except (RuntimeError, subprocess.TimeoutExpired) as exc:
        raise SourceUnavailable(f"source refresh failed: {exc}") from exc


def ref_exists(ref):
    return run("git", "rev-parse", "--verify", "--quiet", ref, check=False).returncode == 0


def ref_sha(ref):
    return git("rev-parse", ref) if ref_exists(ref) else None


def task_body(ref, name):
    p = run("git", "show", f"{ref}:specs/tasks/{name}.md", check=False)
    return p.stdout if p.returncode == 0 else ""


def task_progress(ref, name):
    body = task_body(ref, name)
    return field(body, "progress") if body else ""


def sync(db):
    db.commit()
    source()
    main_sha = ref_sha('origin/main')
    if not main_sha:
        raise SourceUnavailable('source main does not resolve')
    paths = git("ls-tree", "-r", "--name-only", main_sha, "specs/tasks").splitlines()
    spec_paths = git("ls-tree", "-r", "--name-only", main_sha, "specs/system", "specs/development").splitlines()
    spec_bodies = {path: git("show", f"{main_sha}:{path}") for path in spec_paths if path.endswith(".md")}
    goal = run("git", "show", f"{main_sha}:specs/GOAL.md", check=False).stdout
    observations = []
    for path in paths:
        match = TASK_RE.match(path)
        if not match:
            continue
        name = match.group(1)
        body = task_body(main_sha, name)
        branch = f"origin/worker/{name}"
        seed = ref_sha(branch)
        candidate = seed if seed and task_progress(seed, name) == "complete" and run(
            "git", "merge-base", "--is-ancestor", seed, main_sha, check=False).returncode != 0 else None
        observations.append((name, body, seed, candidate))
    # Lazy Git blob downloads and subprocess reads can take seconds. Observe
    # them before opening the SQLite write transaction so cockpit controls
    # remain writable, and bind all desired definitions to one main snapshot.
    upgrade_baseline_generations(db, goal)
    for name, body, seed, candidate in observations:
        progress = field(body, "progress")
        old = db.execute("SELECT * FROM tasks WHERE name=?", (name,)).fetchone()
        generation = contract.generation(body, spec_bodies, goal)
        changed = bool(old and old["generation"] and generation != old["generation"])
        if progress == "complete":
            state = "merged"
        elif old and old["state"] in ("running", "checking", "promoting", "candidate", "published", "blocked", "failed", "deferred"):
            state = old["state"]
            candidate = old["candidate"] or candidate
            seed = old["seed"] or seed
        else:
            # A ready task may carry a checkpoint from a failed attempt.
            # Keep it through sync so the next worker resumes that commit.
            if old:
                seed = old["seed"] or seed
            state = "candidate" if candidate else "ready"
        if old and old["feedback"] and state == "candidate":
            # Do not rediscover the same rejected legacy branch during sync.
            state, candidate = "ready", None
        if changed:
            directory = STATE / "contracts" / name
            directory.mkdir(parents=True, exist_ok=True)
            handoff = directory / f"{generation}.md"
            handoff.write_text(f"# Requirements changed for {name}\n\n"
                               f"Previous generation: {old['generation']}\nDesired generation: {generation}\n\n"
                               "Re-read the task and cited specs on current main. Reconcile production behavior "
                               "with these requirements; previous completion is stale. Preserve useful code, "
                               "reproduce relevant acceptance behavior, and obtain independent review.\n")
            state = old["state"] if old["state"] in ("running", "checking") else "ready"
            candidate, seed = None, old["candidate"] or old["seed"] or main_sha
        elif old and old["generation"] and old["observed_generation"] and old["observed_generation"] != generation and progress == "complete":
            state, candidate = old["state"], old["candidate"]
        db.execute("""INSERT INTO tasks(name,progress,deps,state,seed,candidate) VALUES(?,?,?,?,?,?)
          ON CONFLICT(name) DO UPDATE SET progress=excluded.progress,deps=excluded.deps,
          state=excluded.state,seed=excluded.seed,candidate=excluded.candidate""",
                   (name, progress, json.dumps(deps(body)), state, seed, candidate))
        db.execute("UPDATE tasks SET generation=? WHERE name=?", (generation, name))
        if candidate and (not old or old["candidate"] != candidate or not old["candidate_at"]):
            db.execute("UPDATE tasks SET candidate_at=? WHERE name=?", (int(time.time()), name))
        if changed:
            db.execute("UPDATE tasks SET feedback=?,condition='SpecChanged',repairs=0,retry_baseline=attempts WHERE name=?", (str(handoff), name))
            event(db, name, "desired requirements changed; prior completion and candidate invalidated")
        elif progress == "complete" and state == "merged":
            db.execute("UPDATE tasks SET observed_generation=? WHERE name=?", (generation, name))
    graph = {row["name"]: json.loads(row["deps"]) for row in db.execute("SELECT name,deps FROM tasks")}
    errors = contract.graph_errors(graph)
    for name, reason in errors.items():
        old = db.execute("SELECT state,condition FROM tasks WHERE name=?", (name,)).fetchone()
        db.execute("UPDATE tasks SET condition=? WHERE name=?", ("InvalidDependencies: " + reason, name))
        if not (old["condition"] or "").startswith("InvalidDependencies:"):
            event(db, name, reason)
    db.execute("UPDATE tasks SET condition=NULL WHERE condition LIKE 'InvalidDependencies:%' AND name NOT IN (SELECT value FROM json_each(?))", (json.dumps(list(errors)),))
    db.commit()
    current_main = main_sha
    for task in db.execute("SELECT * FROM tasks WHERE state='blocked' AND blocked_base IS NOT NULL").fetchall():
        prerequisite = re.search(r'repair (task-\d+)', task['condition'] or '')
        if prerequisite:
            repair = db.execute('SELECT state FROM tasks WHERE name=?', (prerequisite[1],)).fetchone()
            if repair and repair['state'] != 'merged':
                continue
        if current_main != task["blocked_base"]:
            db.execute("UPDATE tasks SET state='candidate',blocked_base=NULL,condition=NULL WHERE name=?", (task["name"],))
            event(db, task["name"], "main changed; rechecking previously blocked candidate")


def legacy_running():
    me = os.getpid()
    for path in Path("/proc").glob("[0-9]*/cmdline"):
        if int(path.parent.name) == me:
            continue
        try:
            cmd = path.read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except (OSError, ProcessLookupError):
            continue
        if re.search(r"(?:^| )(?:\S*/)?(?:fleet-sandbox|worker-sandbox|loop)\.sh(?: |$)", cmd):
            return cmd[:180]
    return None


def snapshot_bundle(directory):
    bundle = directory / "bundle"
    files = list((ROOT / "scripts").glob("dev-*.*"))
    files += [ROOT / "scripts" / name for name in ("check-rustfmt-diff.py", "check-clippy-diff.py")]
    files += list((ROOT / "specs/prompts").glob("dev-*.md"))
    files += list((ROOT / "docker/dev-worker").glob("*.y*ml"))
    for file in files:
        if file.is_file():
            target = bundle / file.relative_to(ROOT)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(file, target)
    for env, name in (("MODELS_YML", "models.yml"), ("CONFIG_YML", "config.yml"), ("GYRE_DEV_POLICY", "policy.yaml")):
        if os.environ.get(env):
            shutil.copy2(os.environ[env], bundle / "docker/dev-worker" / name)
    manifest = {"image": os.environ.get("GYRE_DEV_IMAGE", "ghcr.io/jsell-rh/gyre-worker@sha256:0c4a04a340e20c91e89f855c5d75d940b8550798441990ca823c7b8ebb8cbcec"),
                "model": os.environ.get("GYRE_DEV_MODEL", "enmaas-glm-5-3/rits/zai-org/glm-5-3"),
                "implementation_model": os.environ.get("GYRE_DEV_IMPLEMENTATION_MODEL"),
                "files": {str(file.relative_to(bundle)): hashlib.sha256(file.read_bytes()).hexdigest()
                          for file in sorted(bundle.rglob("*")) if file.is_file()}}
    encoded = json.dumps(manifest, sort_keys=True).encode()
    (directory / "bundle.json").write_bytes(encoded)
    return bundle, hashlib.sha256(encoded).hexdigest()


def spawn(db, task, kind, branch=None, sha=None, base=None):
    ident = uuid.uuid4().hex[:16]
    directory = STATE / "attempts" / ident
    directory.mkdir(parents=True)
    bundle, digest = snapshot_bundle(directory)
    if kind == "worker" and task["feedback"]:
        (directory / "repair.md").write_bytes(Path(task["feedback"]).read_bytes())
    if kind == "worker" and task["definition_path"]:
        (directory / "task.md").write_bytes(Path(task["definition_path"]).read_bytes())
    if task["audit_contract"]:
        (directory / "audit-contract.json").write_bytes(Path(task["audit_contract"]).read_bytes())
    if kind == "worker":
        script = ROOT / "scripts/dev-sandbox.sh"
        args = ["worker", task["name"], branch, task["seed"] or "origin/main", ident]
    else:
        script = ROOT / "scripts/dev-sandbox.sh"
        args = ["check", task["name"], sha, base, ident]
    # Bash reads scripts incrementally. Snapshot the driver before launch so
    # edits to the repo cannot corrupt a running attempt's parse stream.
    snapshot = directory / "dev-sandbox.sh"
    snapshot.write_bytes((bundle / "scripts/dev-sandbox.sh").read_bytes())
    snapshot.chmod(0o700)
    # The wrapper records exit status even if the controller itself exits.
    wrapper = bundle / "scripts/dev-process.sh"
    db.execute("INSERT INTO attempts(id,task,kind,branch,sha,base,state,started,bundle_sha,phase,generation) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
               (ident, task["name"], kind, branch, sha, base, "launching", int(time.time()), digest, "LaunchPending", task["generation"]))
    db.execute("UPDATE tasks SET state=?,condition=NULL,retry_at=0 WHERE name=?",
               ("running" if kind == "worker" else "checking", task["name"]))
    db.commit()  # Durable intent precedes the first process or cloud operation.
    env = {**os.environ, "GYRE_DEV_ROOT": str(bundle), "GYRE_DEV_STATE": str(STATE), "GYRE_DEV_OWNER": controller_owner()}
    env.update({key: str(bundle / "docker/dev-worker" / name) for key, name in
                (("MODELS_YML", "models.yml"), ("CONFIG_YML", "config.yml"), ("GYRE_DEV_POLICY", "policy.yaml"))})
    try:
        with (directory / "output.log").open("ab", buffering=0) as log:
            p = subprocess.Popen([str(wrapper), str(directory / "exit"), str(snapshot), *args],
                                 cwd=ROOT, env=env,
                                 stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    except OSError as exc:
        db.execute("UPDATE attempts SET state='failed',ended=?,reason='ProcessLaunchFailed' WHERE id=?", (int(time.time()), ident))
        defer_infrastructure(db, task, f"Process launch unavailable: {exc}", worker=kind == "worker")
        return
    db.execute("UPDATE attempts SET state='running',pid=?,phase='Provisioning' WHERE id=?", (p.pid, ident))
    db.commit()
    event(db, task["name"], f"{kind} {ident} pid={p.pid} branch={branch or '-'} sha={sha or '-'}")


def alive(pid, identity_path=None):
    if not pid:
        return False
    try:
        stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        if identity_path and identity_path.exists():
            identity = json.loads(identity_path.read_text())
            if identity != {"pid": pid, "start": stat[19],
                            "boot": Path("/proc/sys/kernel/random/boot_id").read_text().strip()}:
                return False
        return stat[0] != "Z"
    except (OSError, ValueError, IndexError):
        return False


def recover_launches(db):
    for attempt in db.execute("SELECT * FROM attempts WHERE state='launching'").fetchall():
        identity = STATE / "attempts" / attempt["id"] / "exit.process.json"
        if identity.exists():
            try:
                pid = int(json.loads(identity.read_text())["pid"])
            except (ValueError, KeyError, OSError):
                pid = None
            db.execute("UPDATE attempts SET state='running',pid=? WHERE id=?", (pid, attempt["id"]))
            event(db, attempt["task"], "adopted durable launch intent")
        elif time.time() - attempt["started"] > 30:
            db.execute("UPDATE attempts SET state='failed',ended=?,reason='LaunchUncertain' WHERE id=?", (int(time.time()), attempt["id"]))
            db.execute("UPDATE tasks SET state='failed',condition='LaunchUncertain' WHERE name=?", (attempt["task"],))
            event(db, attempt["task"], "launch has no process identity; blocked until owned resource inventory is reconciled")
    db.commit()


def reap(db):
    for attempt in db.execute("SELECT * FROM attempts WHERE state='running'").fetchall():
        exit_file = STATE / "attempts" / attempt["id"] / "exit"
        if not exit_file.exists() and alive(attempt["pid"], exit_file.with_name("exit.process.json")):
            continue
        rc = int(exit_file.read_text().strip()) if exit_file.exists() else 255
        task = db.execute("SELECT * FROM tasks WHERE name=?", (attempt["task"],)).fetchone()
        if attempt["kind"] == "worker" and rc not in (75, 76, 77, 78, 79):
            # Refresh before consuming the durable outcome. A fetch outage must
            # leave this attempt recoverable, rather than strand a running task.
            source()
        outcome = "done" if rc == 0 else "deferred" if rc in (77, 78, 82) else "failed"
        db.execute("UPDATE attempts SET state=?,ended=?,detail=? WHERE id=?",
                   (outcome, int(time.time()), f"exit={rc}", attempt["id"]))
        if stale_audit(task):
            db.execute("UPDATE tasks SET state='superseded',candidate=NULL,condition='AuditGenerationChanged' WHERE name=?", (task['name'],))
            event(db, task['name'], 'production generation changed; scheduling a fresh scoped fidelity audit')
        elif task['state'] == 'merged' and task['observed_generation'] == task['generation']:
            event(db, task['name'], 'upstream already completed this generation; completed attempt retired without another candidate')
        elif rc and missing_provider_failure(attempt):
            defer_missing_provider(db, task, attempt)
        elif rc in (77, 78):
            reason = "Capacity unavailable" if rc == 78 else "Sandbox or gateway infrastructure unavailable"
            defer_infrastructure(db, task, reason, worker=attempt["kind"] == "worker")
        elif rc == 82:
            failures = task['reconcile_failures'] + 1
            delay = int(backoff_seconds(failures) * random.uniform(0.8, 1.2))
            seed = ref_sha(f"origin/{attempt['branch']}") if attempt['kind'] == 'worker' else None
            db.execute("""UPDATE tasks SET state='deferred',condition='InferenceUnavailable',
                       retry_at=?,reconcile_failures=?,retry_baseline=retry_baseline+?,
                       seed=COALESCE(?,seed) WHERE name=?""",
                       (int(time.time()) + delay, failures, int(attempt['kind'] == 'worker'),
                        seed, task['name']))
            event(db, task['name'], f'inference unavailable; checkpoint retained, retry after {delay}s; gateway admission unchanged')
        elif rc == 79:
            db.execute("UPDATE tasks SET state='failed',condition='Sandbox configuration invalid' WHERE name=?", (task["name"],))
            db.execute("UPDATE controller_health SET condition='ConfigurationInvalid',admission=1 WHERE id=1")
            event(db, task["name"], "sandbox configuration invalid; inspect attempt log and repair gateway configuration")
        elif rc == 81 and attempt['kind'] == 'check':
            output = STATE / 'attempts' / attempt['id'] / 'output.log'
            try:
                records = [json.loads(line.split('GYRE_BASELINE_FAILURE_JSON ', 1)[1])
                           for line in output.read_text(errors='replace').splitlines() if line.startswith('GYRE_BASELINE_FAILURE_JSON ')]
                classified = records[-1]
                if classified['base'] != attempt['base']:
                    raise ValueError('baseline report does not match checked main')
                baseline_log = output.with_name('baseline-gate.log')
                baseline_log.write_text('$ ' + ' '.join(classified['probe']) + '\n' + classified['log'])
                classified['baseline_log'] = str(baseline_log)
                repair = propose_baseline_repair(db, attempt, classified)
                db.execute("UPDATE tasks SET state='blocked',blocked_base=?,condition=? WHERE name=?",
                           (attempt['base'], f'MainBaselineFailed: repair {repair}', task['name']))
                event(db, task['name'], f'cloud gate reproduced failure on main; prerequisite {repair}')
            except (OSError, ValueError, IndexError, KeyError, TypeError):
                db.execute("UPDATE tasks SET state='failed',condition='InvalidBaselineReport' WHERE name=?", (task['name'],))
                event(db, task['name'], 'baseline gate outcome has no valid evidence; promotion blocked')
        elif rc in (75, 76):
            # Bootstrap and push failures already retried inside this sandbox.
            # A push failure may have a local recovery.patch; never provision
            # another heavy sandbox automatically for the same outage.
            db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
            reason = {75: "sandbox bootstrap", 76: "branch push"}[rc]
            event(db, task["name"], f"{reason} failed; inspect log/recovery.patch and retry explicitly")
        elif attempt["kind"] == "worker":
            # A crashed driver may have pushed useful progress. Always inspect its exact ref.
            remote = f"origin/{attempt['branch']}"
            sha = ref_sha(remote)
            stale = bool(attempt["generation"] and attempt["generation"] != task["generation"])
            if sha and task_progress(remote, task["name"]) == "complete" and not stale:
                db.execute("UPDATE tasks SET state='candidate',candidate=?,seed=?,feedback=NULL,candidate_generation=?,candidate_at=? WHERE name=?",
                           (sha, sha, task["generation"], int(time.time()), task["name"]))
                event(db, task["name"], f"candidate {sha}")
            else:
                db.execute("UPDATE tasks SET state='ready',seed=COALESCE(?,seed) WHERE name=?",
                           (sha, task["name"]))
                event(db, task["name"], f"worker exited {rc}; progress retained")
        elif rc == 0:
            db.execute("UPDATE tasks SET state='promoting' WHERE name=?", (task["name"],))
            event(db, task["name"], f"checks passed for {attempt['sha']} on {attempt['base']}")
        elif rc == 1:
            queue_repair(db, task, attempt)
        else:
            db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
            event(db, task["name"], f"checks failed for {attempt['sha']}; see attempts/{attempt['id']}/output.log")
        db.commit()


def start_gateway_job(db, name, kind, sandbox=None):
    directory = STATE / "gateway-jobs" / uuid.uuid4().hex
    directory.mkdir(parents=True)
    result = directory / "result.json"
    db.execute("""INSERT INTO gateway_jobs(name,kind,sandbox,state,started,result)
                  VALUES(?,?,?,'launching',?,?) ON CONFLICT(name) DO UPDATE SET
                  state='launching',started=excluded.started,result=excluded.result,pid=NULL""",
               (name, kind, sandbox, int(time.time()), str(result)))
    db.commit()
    args = [sys.executable, str(ROOT / "scripts/dev-gateway-job.py"), kind, "--result", str(result)]
    if sandbox:
        args += ["--sandbox", sandbox]
    try:
        with (directory / "output.log").open("ab", buffering=0) as log:
            process = subprocess.Popen([str(ROOT / "scripts/dev-process.sh"), str(directory / "exit"), *args],
                                       env={**os.environ, "GYRE_DEV_STATE": str(STATE)},
                                       stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    except OSError as exc:
        result.write_text(json.dumps({"ok": False, "error": f"gateway process launch failed: {exc}"}))
        return
    db.execute("UPDATE gateway_jobs SET state='running',pid=? WHERE name=?", (process.pid, name))
    db.commit()
    GATEWAY_PROCESSES[name] = process


def gc_sandboxes(db):
    """Reconcile owned inventory and a bounded deletion queue without blocking dispatch."""
    now = int(time.time())
    for name, process in list(GATEWAY_PROCESSES.items()):
        if process.poll() is not None:
            GATEWAY_PROCESSES.pop(name)
    known = {sandbox_name(row): row for row in db.execute("SELECT * FROM attempts")}
    for job in db.execute("SELECT * FROM gateway_jobs WHERE state IN ('launching','running')").fetchall():
        path = Path(job["result"])
        identity_path = path.parent / "exit.process.json"
        pid = job["pid"]
        if job["state"] == "launching" and identity_path.exists():
            pid = json.loads(identity_path.read_text())["pid"]
            db.execute("UPDATE gateway_jobs SET state='running',pid=? WHERE name=?", (pid, job["name"]))
        if not path.exists():
            if (pid and alive(pid, identity_path)) or now - job["started"] < 30:
                continue
            outcome = {"ok": False, "error": "gateway job disappeared without a result"}
        else:
            try:
                outcome = json.loads(path.read_text())
            except (ValueError, OSError):
                outcome = {"ok": False, "error": "invalid gateway job result"}
        success = outcome.get("ok") is True
        failures = 0 if success else job["failures"] + 1
        delay = 30 if success else backoff_seconds(failures)
        db.execute("UPDATE gateway_jobs SET state='done',retry_at=?,failures=? WHERE name=?", (now + delay, failures, job["name"]))
        if job["kind"] == "inventory":
            if success:
                items = outcome.get("items")
                if not isinstance(items, list):
                    success = False
                    outcome = {"error": "inventory omitted its objects"}
            if success:
                owner = controller_owner()
                # Only a complete, successfully decoded inventory can clear
                # deletion debt or free capacity.
                db.execute("UPDATE resources SET present=0,delete_pending=0")
                for item in items:
                    name = item["name"]
                    if name not in known and item.get("labels", {}).get("gyre.dev/controller") != owner:
                        continue
                    attempt = known.get(name)
                    ident = attempt["id"] if attempt else None
                    active = bool(attempt and attempt["state"] in ("launching", "running") and not
                                  (STATE / "attempts" / ident / "exit").exists())
                    db.execute("""INSERT INTO resources(name,attempt_id,present,phase,last_seen,delete_pending)
                                  VALUES(?,?,1,?,?,?) ON CONFLICT(name) DO UPDATE SET
                                  present=1,phase=excluded.phase,last_seen=excluded.last_seen,
                                  delete_pending=excluded.delete_pending""",
                               (name, ident, item.get("phase", "Unknown"), now, int(not active)))
                db.execute("UPDATE controller_health SET inventory_at=?,inventory_error=NULL WHERE id=1", (now,))
            else:
                db.execute("UPDATE controller_health SET inventory_error=? WHERE id=1", (outcome.get("error", "inventory unavailable"),))
                event(db, None, "sandbox inventory unavailable; admission paused until resource accounting recovers")
        elif success:
            # A delete request isn't proof that remote compute has vanished.
            # Keep it charged until inventory confirms absence.
            db.execute("UPDATE resources SET retry_at=?,failures=0 WHERE name=?", (now + 30, job["sandbox"]))
            event(db, None, f"requested sandbox deletion {job['sandbox']}; awaiting inventory confirmation")
            db.execute("UPDATE gateway_jobs SET retry_at=0 WHERE name='inventory' AND state='done'")
        else:
            db.execute("UPDATE resources SET retry_at=?,failures=failures+1 WHERE name=?", (now + delay, job["sandbox"]))
            event(db, None, f"sandbox deletion will retry in {delay}s: {job['sandbox']}")
    db.commit()
    if not os.environ.get("OPENSHELL_OIDC_CLIENT_SECRET"):
        return
    inventory_job = db.execute("SELECT * FROM gateway_jobs WHERE name='inventory'").fetchone()
    if not inventory_job or (inventory_job["state"] == "done" and inventory_job["retry_at"] <= now):
        start_gateway_job(db, "inventory", "inventory")
    active = db.execute("SELECT count(*) FROM gateway_jobs WHERE kind='delete' AND state IN ('launching','running')").fetchone()[0]
    for resource in db.execute("SELECT * FROM resources WHERE present=1 AND delete_pending=1 AND retry_at<=? ORDER BY last_seen", (now,)).fetchall():
        if active >= 4:
            break
        key = "delete:" + resource["name"]
        job = db.execute("SELECT * FROM gateway_jobs WHERE name=?", (key,)).fetchone()
        if job and (job["state"] != "done" or job["retry_at"] > now):
            continue
        start_gateway_job(db, key, "delete", resource["name"])
        active += 1


def promote(db):
    # Published PRs remain under reconciliation in both publication modes.
    db.execute("UPDATE tasks SET state='promoting' WHERE state='published'")
    merged = {row["name"] for row in db.execute("SELECT name FROM tasks WHERE state='merged'")}
    for task in db.execute("SELECT * FROM tasks WHERE state='promoting' ORDER BY name").fetchall():
        if task['retry_at'] > time.time():
            continue
        if stale_audit(task):
            db.execute("UPDATE tasks SET state='superseded',condition='AuditGenerationChanged' WHERE name=?", (task['name'],))
            continue
        missing = set(json.loads(task["deps"])) - merged
        if missing:
            # An older controller may have admitted this check with truncated
            # dependencies. Release the integration lane for its prerequisites.
            db.execute("UPDATE tasks SET state='candidate',condition='DependenciesPending' WHERE name=?", (task["name"],))
            event(db, task["name"], f"promotion waits for dependencies: {', '.join(sorted(missing))}")
            continue
        check = db.execute("SELECT * FROM attempts WHERE task=? AND kind='check' AND state='done' ORDER BY rowid DESC LIMIT 1",
                           (task["name"],)).fetchone()
        if not check or check["sha"] != task["candidate"]:
            db.execute("UPDATE tasks SET state='candidate' WHERE name=?", (task["name"],))
            db.commit()
            continue
        if check["generation"] and check["generation"] != task["generation"]:
            db.execute("UPDATE tasks SET state='ready',candidate=NULL,condition='SpecChanged' WHERE name=?", (task["name"],))
            event(db, task["name"], "checked generation is stale; returning to implementation")
            continue
        # Observe the already-published head before allocating a fresh checker
        # just because main moved. CI failures must reach their repair path.
        if task['pr_url'] and check['merge_sha'] and (task['condition'] or '').startswith(('GitHub', 'PullRequest')):
            if not reconcile_pr_checks(db, task, check, check['merge_sha'],
                                       {'url': task['pr_url'], 'number': task['pr_number']}):
                continue
        source()
        main = ref_sha("origin/main")
        if main == check["merge_sha"]:
            db.execute("UPDATE tasks SET state='merged',observed_generation=generation WHERE name=?", (task["name"],))
            event(db, task["name"], f"merged {main}")
            continue
        if main != check["base"]:
            db.execute("UPDATE tasks SET state='candidate' WHERE name=?", (task["name"],))
            event(db, task["name"], "main moved; rechecking candidate against new base")
            continue
        # The checker writes a tested merge commit to an isolated ref. Push exactly that SHA.
        verified_name = check["id"]
        merge_ref = f"refs/remotes/origin/devloop/verified/{verified_name}"
        try:
            git("fetch", "--quiet", "origin", f"refs/heads/devloop/verified/{verified_name}:{merge_ref}", timeout=90)
        except (RuntimeError, subprocess.TimeoutExpired) as exc:
            raise SourceUnavailable(f"verified ref fetch failed: {exc}") from exc
        merge_sha = ref_sha(merge_ref)
        if not merge_sha or git("rev-parse", f"{merge_sha}^1") != main or not run(
                "git", "merge-base", "--is-ancestor", check["sha"], merge_sha, check=False).returncode == 0:
            db.execute("UPDATE tasks SET state='failed',condition='InvalidVerifiedRef' WHERE name=?", (task["name"],))
            event(db, task["name"], "verified integration ref does not match checked base/candidate; promotion blocked")
            continue
        db.execute("UPDATE attempts SET merge_sha=? WHERE id=?", (merge_sha, check["id"]))
        db.commit()
        gate_dir = STATE / "attempts" / check["id"]
        gate_exit = gate_dir / "host-tests.exit"
        gate_ok = gate_dir / "host-tests.ok"
        if check["host_pid"] == -1:
            identity = gate_dir / "host-tests.exit.process.json"
            if identity.exists():
                try:
                    pid = int(json.loads(identity.read_text())["pid"])
                except (OSError, ValueError, KeyError):
                    pid = None
                db.execute("UPDATE attempts SET host_pid=? WHERE id=?", (pid, check["id"]))
                db.commit()
                return
            if time.time() - (check["host_started"] or 0) < 30:
                return
        if not host_gate_marker_valid(gate_dir, merge_sha):
            if gate_ok.exists() and gate_exit.exists() and gate_exit.read_text().strip() == '0':
                gate_ok.unlink(missing_ok=True)
                gate_exit.unlink(missing_ok=True)
                db.execute("UPDATE attempts SET host_pid=NULL,host_started=NULL WHERE id=?", (check['id'],))
                db.commit()
                event(db, task['name'], 'prior host proof lacks workspace artifact isolation; rechecking exact tree')
                return
            if not gate_exit.exists() and check["host_pid"] and alive(check["host_pid"], gate_dir / "host-tests.exit.process.json"):
                return  # keep reconciling other tasks while the host gate runs
            if not gate_exit.exists() and not check["host_pid"]:
                # Older controllers may have launched several checkers. Keep
                # their host gates serialized too, including invalidated gates.
                if any(row["host_pid"] and alive(row["host_pid"]) and not
                       (STATE / "attempts" / row["id"] / "host-tests.exit").exists()
                       for row in db.execute("SELECT id,host_pid FROM attempts WHERE host_pid IS NOT NULL")):
                    return
                start_host_gate(db, check["id"], merge_sha)
                event(db, task["name"], f"host full-suite gate started for {merge_sha}")
                return
            # A process that vanished without an exit record also fails closed.
            result = gate_exit.read_text().strip() if gate_exit.exists() else "missing"
            if result == "1":
                result_path = gate_dir / "host-tests.result.json"
                classified = json.loads(result_path.read_text()) if result_path.exists() else {}
                if classified.get("status") == "baseline_failed":
                    repair = propose_baseline_repair(db, check, classified)
                    db.execute("UPDATE tasks SET state='blocked',blocked_base=?,condition=? WHERE name=?",
                               (check["base"], f"MainBaselineFailed: repair {repair}", task["name"]))
                    event(db, task["name"], f"candidate blocked by existing main failure; prerequisite {repair}")
                elif classified.get("status") == "unavailable":
                    defer_promotion(db, task, 'HostEnvironmentUnavailable')
                    db.execute("UPDATE attempts SET host_pid=NULL,host_started=NULL,phase='HostWaiting',reason='HostEnvironmentUnavailable' WHERE id=?", (check['id'],))
                    gate_exit.unlink(missing_ok=True)
                    event(db, task['name'], 'host environment unavailable; retrying the same verified tree with backoff, without another sandbox')
                else:
                    queue_repair(db, task, check, "host-tests.log")
            else:
                db.execute("UPDATE tasks SET state='failed',condition='HostGateUnavailable' WHERE name=?", (task["name"],))
                event(db, task["name"], f"host gate unavailable (exit={result}); inspect host-tests.log")
            continue
        pr = ensure_pull_request(db, task, check, merge_sha)
        if not reconcile_pr_checks(db, task, check, merge_sha, pr):
            continue
        if PUBLICATION_MODE == "pr":
            db.execute("UPDATE tasks SET state='published',condition='PullRequestChecksPassed',retry_at=? WHERE name=?",
                       (int(time.time()) + 30, task['name']))
            db.commit()
            continue
        try:
            landed = merge_pull_request(pr, merge_sha)
        except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired) as exc:
            defer_promotion(db, task, 'GitHubMergeUnavailable')
            event(db, task['name'], f'GitHub merge will reconcile after backoff: {exc}')
            continue
        if landed:
            source()
            if run('git', 'merge-base', '--is-ancestor', landed, 'origin/main', check=False).returncode:
                defer_promotion(db, task, 'MergeNotObservedUpstream')
                continue
            db.execute("UPDATE tasks SET state='merged',observed_generation=generation,condition=NULL,retry_at=0 WHERE name=?", (task['name'],))
            event(db, task['name'], f'merged {landed}')
        else:
            defer_promotion(db, task, 'GitHubMergePending')


def reconcile_pr_checks(db, task, check, merge_sha, pr):
    directory = STATE / 'attempts' / check['id']
    try:
        result = ci.observe(run, pr['url'], merge_sha, check['base'], directory)
        status = result['status']
        if status == 'baseline_failed':
            repair = propose_baseline_repair(db, check, result)
            db.execute("UPDATE tasks SET state='blocked',blocked_base=?,condition=? WHERE name=?",
                       (check['base'], f'MainCIFailed: repair {repair}', task['name']))
            event(db, task['name'], f'GitHub checks reproduce on exact main base; prerequisite {repair}')
        elif status == 'candidate_failed':
            queue_repair(db, task, check, 'github-checks.log')
        elif status == 'closed':
            db.execute("UPDATE tasks SET state='failed',condition='PullRequestClosed' WHERE name=?", (task['name'],))
        elif status == 'infrastructure':
            # Retry Actions on this same head, without allocating a sandbox or
            # spending implementation repair budget. Attempts persist over restart.
            retry_path = directory / 'github-retries.json'
            retries = json.loads(retry_path.read_text()) if retry_path.exists() else {}
            for ident in result['runs']:
                key = str(ident)
                if ident is None or retries.get(key, 0) >= 3:
                    db.execute("UPDATE tasks SET state='failed',condition='GitHubInfrastructureRetryLimit' WHERE name=?", (task['name'],))
                    break
                retries[key] = retries.get(key, 0) + 1
                temporary = retry_path.with_suffix('.tmp')
                temporary.write_text(json.dumps(retries))
                temporary.replace(retry_path)
                run('gh', 'run', 'rerun', str(ident), '--repo', pr['url'].split('/pull/')[0].replace('https://github.com/', ''), '--failed', timeout=30)
            else:
                defer_promotion(db, task, 'GitHubInfrastructureBackoff')
        elif status in ('passed', 'merged'):
            db.execute("UPDATE tasks SET reconcile_failures=0,retry_at=0,condition='GitHubChecksPassed' WHERE name=?", (task['name'],))
            db.commit()
            return True
        else:
            db.execute("UPDATE tasks SET state='published',condition=?,retry_at=? WHERE name=?",
                       ('GitHubChecksPending' if status == 'pending' else 'GitHubPolicyPending', int(time.time()) + 30, task['name']))
        db.commit()
    except (RuntimeError, OSError, ValueError, KeyError, subprocess.TimeoutExpired) as exc:
        defer_promotion(db, task, 'GitHubChecksUnavailable')
        event(db, task['name'], f'GitHub checks unavailable; retaining verified tree: {exc}')
    return False


def merge_pull_request(pr, merge_sha):
    message = git('show', '-s', '--format=%B', merge_sha)
    directory = STATE / 'publication'
    directory.mkdir(parents=True, exist_ok=True)
    body = directory / f"pr-{pr['number']}.md"
    body.write_text(message)
    run('gh', 'pr', 'merge', pr['url'], '--merge', '--match-head-commit', merge_sha,
        '--subject', message.splitlines()[0], '--body-file', str(body), timeout=30)
    actual = json.loads(run('gh', 'pr', 'view', pr['url'], '--json', 'state,headRefOid,mergeCommit', timeout=30).stdout)
    if actual['headRefOid'] != merge_sha:
        raise RuntimeError('merged PR head does not match verified commit')
    return actual['mergeCommit']['oid'] if actual['state'] == 'MERGED' else None


def defer_promotion(db, task, reason):
    failures = task['reconcile_failures'] + 1
    delay = int(backoff_seconds(failures) * random.uniform(0.8, 1.2))
    db.execute('UPDATE tasks SET condition=?,reconcile_failures=?,retry_at=? WHERE name=?',
               (reason, failures, int(time.time()) + delay, task['name']))
    db.commit()


def ensure_pull_request(db, task, check, merge_sha):
    """Idempotently publish a verified head and confirm GitHub actually holds it."""
    remote = git("remote", "get-url", "origin")
    match = re.search(r"github\.com[:/]([^/\s]+/[^/\s]+?)(?:\.git)?$", remote)
    repo = os.environ.get("GYRE_DEV_GITHUB_REPO") or (match.group(1) if match else None)
    if not repo or not re.fullmatch(r"[\w.-]+/[\w.-]+", repo):
        raise SourceUnavailable("pull request publication requires a GitHub upstream or GYRE_DEV_GITHUB_REPO")
    head = f"devloop/verified/{check['id']}"
    try:
        # Keep the task's PR through repair/rechecks. The newly checked branch
        # remains immutable; only the associated PR branch advances with a lease.
        if task['pr_url']:
            previous = json.loads(run('gh', 'pr', 'view', task['pr_url'], '--repo', repo,
                                      '--json', 'headRefName,headRefOid,state', timeout=30).stdout)
            if previous['state'] == 'OPEN' and previous['headRefOid'] != merge_sha:
                old = previous['headRefOid']
                if not db.execute("SELECT 1 FROM attempts WHERE task=? AND merge_sha=? AND kind='check'", (task['name'], old)).fetchone():
                    raise RuntimeError('associated PR head is not a recorded verified tree')
                branch = previous['headRefName']
                if not branch.startswith('devloop/'):
                    raise RuntimeError('associated PR branch is not owned by the controller')
                git('push', 'origin', f'--force-with-lease=refs/heads/{branch}:{old}', f'{merge_sha}:refs/heads/{branch}', timeout=90)
                message = git('show', '-s', '--format=%B', merge_sha)
                body = STATE / 'attempts' / check['id'] / 'pull-request.md'
                body.write_text(message + f'\n\nVerified integration head: `{merge_sha}`. Cloud gates, independent review and host suites passed. GitHub checks remain required.\n')
                run('gh', 'pr', 'edit', task['pr_url'], '--repo', repo, '--title', message.splitlines()[0][:240], '--body-file', str(body), timeout=30)
            if previous['state'] == 'OPEN':
                head = previous['headRefName']
        listed = run("gh", "pr", "list", "--repo", repo, "--head", head, "--state", "all",
                     "--json", "number,url,state,headRefOid,baseRefName", timeout=30)
        items = json.loads(listed.stdout)
        if items:
            pr = items[0]
        else:
            directory = STATE / "attempts" / check["id"]
            body = directory / "pull-request.md"
            message = git("show", "-s", "--format=%B", merge_sha)
            subject = message.splitlines()[0]
            body.write_text(message + "\n\n## Verification\n\n"
                            f"- Verified integration commit: `{merge_sha}`\n"
                            f"- Checked upstream base: `{check['base']}`\n"
                            "- Cloud deterministic gates and independent integration review passed.\n"
                            "- Full Rust and frontend suites passed on this exact integration tree.\n"
                            f"- Attempt bundle: `{check['bundle_sha'] or 'pre-snapshot attempt'}`\n")
            created = run("gh", "pr", "create", "--repo", repo, "--head", head, "--base", "main",
                          "--title", subject[:240], "--body-file", str(body), timeout=30)
            url = created.stdout.strip()
            pr = json.loads(run("gh", "pr", "view", url, "--repo", repo,
                                "--json", "number,url,state,headRefOid,baseRefName", timeout=30).stdout)
        if pr.get("headRefOid") != merge_sha or pr.get("baseRefName") != "main" or pr.get("state") not in ("OPEN", "MERGED"):
            raise RuntimeError("GitHub PR does not contain the verified head on the expected base")
        if not re.fullmatch(r"https://github\.com/" + re.escape(repo) + r"/pull/\d+", pr.get("url", "")):
            raise RuntimeError("GitHub returned an unexpected pull request URL")
    except (RuntimeError, ValueError, OSError, subprocess.TimeoutExpired) as exc:
        defer_promotion(db, task, 'PublicationUnavailable')
        raise SourceUnavailable(f"pull request publication unavailable: {exc}") from exc
    db.execute("UPDATE tasks SET pr_url=?,pr_number=?,reconcile_failures=0,retry_at=0,condition=NULL WHERE name=?", (pr["url"], pr["number"], task["name"]))
    db.commit()
    return pr


HOST_ARTIFACT_POLICY = 'workspace-clean-v1'


def host_gate_marker_valid(directory, sha):
    try:
        result = json.loads((directory / 'host-tests.result.json').read_text())
        return ((directory / 'host-tests.ok').read_text().strip() == sha
                and result.get('status') == 'passed' and result.get('sha') == sha
                and result.get('artifact_policy') == HOST_ARTIFACT_POLICY)
    except (OSError, ValueError):
        return False


def host_test_verified(merge_sha, check_id):
    """Run full Rust and frontend suites on the exact verified merge tree."""
    attempt_dir = STATE / "attempts" / check_id
    attempt_dir.mkdir(parents=True, exist_ok=True)
    marker = attempt_dir / "host-tests.ok"
    if host_gate_marker_valid(attempt_dir, merge_sha):
        return True
    marker.unlink(missing_ok=True)
    worktree = STATE / "host-check" / check_id
    worktree.parent.mkdir(parents=True, exist_ok=True)
    if worktree.exists():
        run("git", "worktree", "remove", "--force", str(worktree), check=False)
    added = run("git", "worktree", "add", "--detach", str(worktree), merge_sha, check=False)
    if added.returncode:
        (attempt_dir / "host-tests.log").write_text(added.stderr or added.stdout)
        return False
    passed = False
    operational = False
    try:
        env = {**os.environ, "SKIP_WEB_BUILD": "1",
               "CARGO_TARGET_DIR": str(STATE / "host-target")}
        with (attempt_dir / "host-tests.log").open("wb") as log:
            passed = True
            for command, cwd, timeout in (
                ([sys.executable, str(ROOT / 'scripts/dev-cargo-clean.py')], worktree, 120),
                (["cargo", "test", "--all", "--quiet"], worktree, 1800),
                (["npm", "ci", "--no-audit", "--no-fund"], worktree / "web", 600),
                (["npm", "test"], worktree / "web", 1800),
            ):
                log.write(f"\n$ {' '.join(command)}\n".encode())
                try:
                    result = subprocess.run(command, cwd=cwd, env=env, stdout=log,
                                            stderr=subprocess.STDOUT, timeout=timeout)
                    if result.returncode:
                        log.flush()
                        if command[0] == sys.executable and result.returncode == 75:
                            operational = True
                        elif command[:2] == ['npm', 'ci'] or command[0] in ('cargo', sys.executable):
                            text = (attempt_dir / 'host-tests.log').read_text(errors='replace')[-65536:]
                            operational = bool(re.search(r'failed to download|failed to get .* dependency|Could not resolve (?:host|proxy)|Temporary failure in name resolution|ENOTFOUND|ENETUNREACH|EAI_AGAIN|ECONNRESET|ETIMEDOUT', text))
                        passed = False
                        break
                except (OSError, subprocess.TimeoutExpired) as exc:
                    log.write(f"\nhost test execution failed: {exc}\n".encode())
                    passed = False
                    operational = True
                    break
            audit_contract = attempt_dir / 'audit-contract.json'
            if passed and audit_contract.exists():
                try:
                    result = subprocess.run([sys.executable, str(ROOT / 'scripts/dev-audit-check.py'),
                                             str(audit_contract), '--replay'], cwd=worktree, env=env,
                                            stdout=log, stderr=subprocess.STDOUT, timeout=1800)
                    passed = result.returncode == 0
                except (OSError, subprocess.TimeoutExpired) as exc:
                    log.write(f'acceptance probe execution unavailable: {exc}\n'.encode())
                    operational, passed = True, False
            if passed:
                changed = run('git', 'diff', '--name-only', 'HEAD', cwd=worktree).stdout.splitlines()
                changed += run('git', 'ls-files', '--others', '--exclude-standard', cwd=worktree).stdout.splitlines()
                changed = [path for path in changed if not path.startswith('web/dist/')]
                if changed:
                    log.write(('host gates modified the verified tree: ' + ', '.join(changed) + '\n').encode())
                    passed = False
    finally:
        run("git", "worktree", "remove", "--force", str(worktree), check=False)
    if passed:
        marker.write_text(merge_sha + "\n")
        (attempt_dir / "host-tests.result.json").write_text(json.dumps({"status": "passed", "sha": merge_sha,
                                                                     "artifact_policy": HOST_ARTIFACT_POLICY}))
    else:
        db = db_open()
        check = db.execute("SELECT base FROM attempts WHERE id=?", (check_id,)).fetchone()
        db.close()
        base = check["base"] if check else None
        classification = {"status": "unavailable" if operational else "candidate_failed", "sha": merge_sha, "base": base}
        if base and not operational:
            signature = host_environment_signature()
            baseline_id = "baseline-" + base[:12] + "-" + signature[:12]
            # Reuse the same real suites on main. The baseline has no checker
            # record, so a baseline failure cannot recursively recheck itself.
            baseline_dir = STATE / "attempts" / baseline_id
            cached = baseline_dir / "host-tests.result.json"
            if not cached.exists() or json.loads(cached.read_text()).get('status') == 'unavailable':
                host_test_verified(base, baseline_id)
            baseline = json.loads(cached.read_text()) if cached.exists() else {"status": "unavailable"}
            classification.update(status="candidate_failed" if baseline["status"] == "passed" else
                                  "unavailable" if baseline["status"] == "unavailable" else "baseline_failed",
                                  environment=signature, baseline_log=str(baseline_dir / "host-tests.log"))
        (attempt_dir / "host-tests.result.json").write_text(json.dumps(classification))
    return passed


def host_environment_signature():
    values = {key: value for key, value in sorted(os.environ.items()) if key not in ('GYRE_DEV_ROOT', '_', 'SHLVL')}
    values['artifact_policy'] = HOST_ARTIFACT_POLICY
    for tool in ('cargo', 'rustc', 'node', 'npm'):
        try:
            result = subprocess.run([tool, '--version'], capture_output=True, text=True, timeout=5)
            values['tool:' + tool] = result.stdout + result.stderr
        except (OSError, subprocess.TimeoutExpired):
            values['tool:' + tool] = 'unavailable'
    return hashlib.sha256(json.dumps(values, sort_keys=True).encode()).hexdigest()


def stale_audit(task):
    if not task['audit_contract']:
        return False
    assigned = json.loads(Path(task['audit_contract']).read_text())
    return assigned['code_generation'] != coverage.code_generation(git, 'origin/main')


def upgrade_baseline_generations(db, goal):
    for task in db.execute("SELECT * FROM tasks WHERE origin_key LIKE 'baseline:%'").fetchall():
        if not task['definition_path'] or not Path(task['definition_path']).is_file():
            continue
        body = Path(task['definition_path']).read_text()
        old = contract.generation(body, {}, goal, include_baseline_diagnostics=True)
        new = contract.generation(body, {}, goal)
        # Only migrate the known diagnostic-only hash change, never an actual
        # requirement edit or a changed GOAL. Retain the candidate and leases.
        if old == new or task['generation'] != old:
            continue
        for column in ('generation', 'candidate_generation', 'observed_generation'):
            db.execute(f'UPDATE tasks SET {column}=? WHERE name=? AND {column}=?', (new, task['name'], old))
        db.execute("UPDATE attempts SET generation=? WHERE task=? AND generation=? AND state IN ('running','launching')",
                   (new, task['name'], old))
        event(db, task['name'], f'operational baseline diagnostics excluded from requirement hash: {old} -> {new}')


def baseline_failure_summary(log):
    lines = [line.rstrip() for line in log.splitlines()]
    failures = ci.failure_signature('\n'.join(lines))
    errors = list(dict.fromkeys(line.strip() for line in lines
                               if re.search(r'(?:Error:|error\[|error:|FAIL\b|FAILED\b)', line)))
    text = '\n'.join(failures + errors) if failures or errors else '\n'.join(lines[-60:])
    return text[:8192].rstrip()


def propose_baseline_repair(db, check, classification):
    key = classification["base"] + ":" + classification["environment"]
    existing = db.execute("SELECT name FROM tasks WHERE origin_key=?", ("baseline:" + key,)).fetchone()
    if existing:
        return existing["name"]
    name = reserve_task_names(db, 'baseline:' + key, 1)[0]
    directory = STATE / "proposed"
    directory.mkdir(parents=True, exist_ok=True)
    definition = directory / f"{name}.md"
    log = baseline_failure_summary(Path(classification["baseline_log"]).read_text(errors="replace"))
    body = (f'---\ntitle: "Repair verified failure on main {check["base"][:12]}"\n'
            'spec_ref: "GOAL.md — real implementations and meaningful verification"\n'
            'depends_on: []\nprogress: needs-revision\ncommits: []\n---\n\n'
            '## Required behavior\n\nA required delivery gate failed on upstream main before this candidate. '
            'Reproduce and repair the existing production defect or meaningful broken test setup. '
            'Do not implement the blocked feature in this task. Do not remove tests, weaken gates, '
            'or claim an environmental outage is a production fix. Run the failing probe and obtain '
            'independent review; integration reruns cloud gates and the full suites.\n\n'
            f'Base: `{check["base"]}`\nEnvironment fingerprint: `{classification["environment"]}`\n\n'
            'The full diagnostic log is retained in the controller attempt artifacts.\n\n'
            f'## Baseline failure\n\n```text\n{log}\n```\n')
    definition.write_text(body)
    generation = contract.generation(body, {}, run("git", "show", "origin/main:specs/GOAL.md", check=False).stdout)
    db.execute("""INSERT INTO tasks(name,progress,deps,state,seed,generation,definition_path,condition,origin_key)
                  VALUES(?,'needs-revision','[]','ready',?,?,?,?,?)""",
               (name, check["base"], generation, str(definition), "BaselineRepair: " + key, "baseline:" + key))
    event(db, name, f"proposed scoped prerequisite for proven main failure at {check['base']}")
    return name


def reserve_task_names(db, owner, count):
    number = 1 + max((int(row[0][5:]) for row in db.execute(
        "SELECT name FROM tasks UNION SELECT name FROM task_reservations")), default=0)
    names = [f"task-{number + offset:03}" for offset in range(count)]
    db.executemany("INSERT INTO task_reservations(name,owner) VALUES(?,?)", [(name, owner) for name in names])
    return names


def plan_audits(db, limit=2):
    """Bounded metadata work after runnable implementation work has drained."""
    rows = db.execute("SELECT * FROM tasks").fetchall()
    merged = {row['name'] for row in rows if row['state'] == 'merged'}
    if any(not row['audit_contract'] and (row['state'] in ('running', 'checking', 'promoting', 'candidate', 'published', 'blocked') or
           (row['state'] == 'ready' and set(json.loads(row['deps'])) <= merged)) for row in rows):
        return
    generation = coverage.code_generation(git, 'origin/main')
    busy = set()
    active = 0
    for row in rows:
        if not row['audit_contract']:
            continue
        assigned = json.loads(Path(row['audit_contract']).read_text())
        if assigned['code_generation'] != generation and row['state'] in ('ready', 'candidate', 'failed', 'deferred'):
            db.execute("UPDATE tasks SET state='superseded',condition='AuditGenerationChanged' WHERE name=?", (row['name'],))
            continue
        if row['state'] in ('ready', 'running', 'checking', 'promoting', 'candidate', 'published', 'deferred', 'failed'):
            busy.add(assigned['coverage'])
            busy.update(task for item in assigned['rows'] for task in item['tasks'])
            active += 1
    if active >= limit:
        db.commit()
        return
    paths = git('ls-tree', '-r', '--name-only', 'origin/main', 'specs/coverage').splitlines()
    for path in paths:
        if active >= limit:
            break
        if not path.endswith('.md') or path.endswith('/SUMMARY.md') or path.endswith('/trusted-foundry-integration.md') or path in busy:
            continue
        spec_path = path.replace('specs/coverage/', 'specs/', 1)
        if run('git', 'cat-file', '-e', f'origin/main:{spec_path}', check=False).returncode:
            continue
        eligible = [item for item in coverage.rows(git('show', f'origin/main:{path}'))
                    if item['status'] != 'n/a' and set(item['tasks']) <= merged and not set(item['tasks']) & busy]
        for offset in range(0, len(eligible), 4):
            assigned_rows = eligible[offset:offset + 4]
            key = 'audit:' + generation + ':' + path + ':' + ','.join(str(item['id']) for item in assigned_rows)
            if db.execute('SELECT 1 FROM tasks WHERE origin_key=?', (key,)).fetchone():
                continue
            names = reserve_task_names(db, key, 5)
            name = names[0]
            directory = STATE / 'proposed'; directory.mkdir(parents=True, exist_ok=True)
            scope = {'version': 1, 'task': name, 'code_generation': generation, 'spec': spec_path,
                     'coverage': path, 'rows': assigned_rows, 'reserved_tasks': names[1:]}
            scope_path = directory / f'{name}.json'; scope_path.write_text(json.dumps(scope, indent=2))
            definition = directory / f'{name}.md'
            body = (f'---\ntitle: "Inspect fidelity: {Path(path).name}, sections {", ".join(str(item["id"]) for item in assigned_rows)}"\n'
                    f'spec_ref: "{Path(spec_path).name}"\ndepends_on: []\nprogress: not-started\ncommits: []\n---\n\n'
                    '## Required behavior\n\nInspect only the assigned sections in the staged audit contract. '
                    'Trace real production behavior and existing meaningful acceptance probes. Preserve evidence in '
                    f'`specs/reviews/audit-{name}.json`. Correct coverage claims; reopen existing incomplete implementations '
                    'or create scoped follow-up tasks using reserved IDs. Leave missing product behavior open. '
                    'This task delivers an independently reviewed fidelity assessment, not a product implementation. '
                    'Never add tests for already-working behavior or edit production/spec requirements here.\n')
            definition.write_text(body)
            desired = contract.generation(body, {spec_path: git('show', f'origin/main:{spec_path}')},
                                          run('git', 'show', 'origin/main:specs/GOAL.md', check=False).stdout)
            db.execute("""INSERT INTO tasks(name,progress,deps,state,seed,generation,definition_path,audit_contract,origin_key,condition)
                          VALUES(?,'not-started','[]','ready',?,?,?,?,?,'FidelityAudit')""",
                       (name, ref_sha('origin/main'), desired, str(definition), str(scope_path), key))
            event(db, name, f'proposed bounded fidelity audit for {path}; product gaps remain open')
            active += 1
            busy.add(path); busy.update(task for item in assigned_rows for task in item['tasks'])
            break  # One audit owns a coverage matrix at a time.
    db.commit()


def start_host_gate(db, check_id, merge_sha):
    gate_dir = STATE / "attempts" / check_id
    gate_dir.mkdir(parents=True, exist_ok=True)
    bundle = gate_dir / "bundle"
    if not bundle.exists():
        snapshot_bundle(gate_dir)
    db.execute("UPDATE attempts SET host_pid=-1,host_started=?,phase='HostChecking',reason=NULL WHERE id=?", (int(time.time()), check_id))
    db.commit()
    with (gate_dir / "host-process.log").open("ab", buffering=0) as log:
        p = subprocess.Popen([str(bundle / "scripts/dev-process.sh"), str(gate_dir / "host-tests.exit"),
                              sys.executable, str(bundle / "scripts/dev-controller.py"), "host-gate",
                              "--merge-sha", merge_sha, "--check-id", check_id],
                             cwd=ROOT, env={**os.environ, "GYRE_DEV_STATE": str(STATE), "GYRE_DEV_ROOT": str(bundle)},
                             stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    db.execute("UPDATE attempts SET host_pid=? WHERE id=?", (p.pid, check_id))
    db.commit()
    HOST_GATE_PROCESSES[check_id] = p


def reap_host_processes():
    for check_id, process in list(HOST_GATE_PROCESSES.items()):
        if process.poll() is not None:
            HOST_GATE_PROCESSES.pop(check_id)


def schedule(db, slots, max_attempts, only_task=None, launch_burst=8):
    now = int(time.time())
    gate = health(db)
    rows = db.execute("SELECT * FROM tasks ORDER BY CASE WHEN origin_key LIKE 'baseline:%' THEN 0 WHEN state='candidate' OR (state='deferred' AND candidate IS NOT NULL) THEN 1 WHEN feedback IS NOT NULL THEN 2 WHEN progress='needs-revision' THEN 3 ELSE 4 END,name").fetchall()
    blocked_bases = {task['blocked_base'] for task in rows if task['state'] == 'blocked'}
    baseline_repairs = {task['name'] for task in rows if (task['origin_key'] or '').startswith('baseline:') and
                        task['state'] != 'merged' and (task['origin_key'].split(':')[1] in blocked_bases or
                        task['state'] in ('running', 'checking', 'promoting', 'candidate'))}
    integration_busy = any(task["state"] in ("checking", "promoting") for task in rows)
    backlog = sum(task["state"] in ("candidate", "checking", "promoting", "published") for task in rows)
    backlog_limit = max(1, int(os.environ.get("GYRE_DEV_MAX_CANDIDATES", "8")))
    # Surface exhausted tasks even when every slot is occupied or dispatch is
    # drained; otherwise they stay misleadingly ready until a slot opens.
    exhausted = {task["name"] for task in rows if task["state"] == "ready" and
                 task["attempts"] - task["retry_baseline"] >= max_attempts}
    for name in sorted(exhausted):
        db.execute("UPDATE tasks SET state='failed' WHERE name=? AND state='ready'", (name,))
        event(db, name, f"attempt limit {max_attempts} reached; inspect logs and retry explicitly")
    running = db.execute("SELECT count(*) FROM attempts WHERE state='running'").fetchone()[0]
    workers = db.execute("SELECT count(*) FROM attempts WHERE state='running' AND kind='worker'").fetchone()[0]
    launches = 0
    effective_slots = effective_admission(db, slots, running, now)
    # Charge remote deletion debt and owned orphans as well as live drivers.
    effective_slots = min(effective_slots, max(0, slots - (resource_usage(db) - running)))
    if running >= effective_slots:
        return
    if gate["failures"]:
        launch_burst = 1
    merged = {r["name"] for r in rows if r["state"] == "merged"}
    for task in rows:
        if (task["condition"] or "").startswith("InvalidDependencies:"):
            continue
        if task["name"] in exhausted:
            continue
        if only_task and task["name"] != only_task and task["name"] not in baseline_repairs:
            continue
        if running >= effective_slots or launches >= launch_burst:
            break
        is_due = task["state"] == "deferred" and task["retry_at"] <= now
        if (task["state"] == "candidate" or (is_due and task["candidate"])) and set(json.loads(task["deps"])) <= merged:
            # A broken delivery baseline blocks integration, not independent
            # implementation. Repair owns the lane while workers can progress
            # within admission and candidate-backlog limits.
            if baseline_repairs and task['name'] not in baseline_repairs:
                continue
            if integration_busy:
                continue
            source()
            sha = task["candidate"]
            base = ref_sha("origin/main")
            if not sha or not ref_exists(sha):
                db.execute("UPDATE tasks SET state='ready',candidate=NULL WHERE name=?", (task["name"],))
                db.commit()
                continue
            spawn(db, task, "check", sha=sha, base=base)
            integration_busy = True
            running += 1
            launches += 1
        elif (task["state"] == "ready" or is_due) and (task["feedback"] or task["progress"] in ("not-started", "in-progress", "ready-for-review", "needs-revision")) and set(json.loads(task["deps"])) <= merged:
            if backlog >= backlog_limit and task["name"] not in baseline_repairs:
                continue
            # Reserve one admitted slot for integration. With a single slot,
            # implementation and integration take turns instead.
            if effective_slots > 1 and workers >= effective_slots - 1:
                continue
            attempt_no = task["attempts"] + 1
            branch = f"devloop/{task['name']}/attempt-{attempt_no}"
            db.execute("UPDATE tasks SET attempts=? WHERE name=?", (attempt_no, task["name"]))
            db.commit()
            spawn(db, task, "worker", branch=branch)
            running += 1
            workers += 1
            launches += 1


def status(db):
    for state, count in db.execute("SELECT state,count(*) FROM tasks GROUP BY state ORDER BY state"):
        print(f"{state}: {count}")
    rows = db.execute("SELECT name,state,progress,deps,feedback,condition FROM tasks").fetchall()
    merged = {r["name"] for r in rows if r["state"] == "merged"}
    eligible = sum((r["state"] == "candidate" or
                    (r["state"] == "ready" and (r['feedback'] or r["progress"] in ("not-started", "in-progress", "ready-for-review", "needs-revision")))) and
                   not (r['condition'] or '').startswith('InvalidDependencies:') and
                   set(json.loads(r["deps"])) <= merged for r in rows)
    print(f"eligible now: {eligible}")
    for r in db.execute("SELECT id,task,kind,state,started FROM attempts ORDER BY started DESC LIMIT 20"):
        print(f"{r['task']} {r['kind']} {r['state']} {r['id']} ({STATE / 'attempts' / r['id'] / 'output.log'})")


def configured_slots(default=None):
    try:
        raw = (STATE / "slots").read_text().strip()
    except FileNotFoundError:
        return default
    if not re.fullmatch(r"\d{1,4}", raw):
        return 0  # malformed control input pauses dispatch
    return min(int(raw), 1000)


def status_snapshot(db):
    tasks = [dict(row) for row in db.execute("SELECT * FROM tasks ORDER BY name")]
    shipped = {}
    for row in db.execute("SELECT task,message FROM events WHERE message LIKE 'merged %' ORDER BY id"):
        match = re.fullmatch(r"merged ([0-9a-f]{40})", row["message"])
        if match:
            shipped[row["task"]] = match.group(1)
    for task in tasks:
        task["deps"] = json.loads(task["deps"])
        task["merge_sha"] = shipped.get(task["name"]) if task["state"] == "merged" else None
    attempts = [dict(row) for row in db.execute(
        "SELECT * FROM attempts WHERE state='running' ORDER BY rowid DESC")]
    attempts += [dict(row) for row in db.execute(
        "SELECT * FROM attempts WHERE state!='running' ORDER BY rowid DESC LIMIT 100")]
    events = [dict(row) for row in db.execute(
        "SELECT * FROM events ORDER BY id DESC LIMIT 100")]
    merged = {task["name"] for task in tasks if task["state"] == "merged"}
    eligible = sum((task["state"] == "candidate" or
                    (task["state"] == "ready" and (task['feedback'] or task["progress"] in ("not-started", "in-progress", "ready-for-review", "needs-revision")))) and
                   not (task['condition'] or '').startswith('InvalidDependencies:') and
                   set(task["deps"]) <= merged for task in tasks)
    blocked_bases = {task['blocked_base'] for task in tasks if task['state'] == 'blocked'}
    prerequisite_repairs = [task['name'] for task in tasks if (task['origin_key'] or '').startswith('baseline:') and
                           task['state'] != 'merged' and task['origin_key'].split(':')[1] in blocked_bases]
    counts = {}
    for task in tasks:
        counts[task["state"]] = counts.get(task["state"], 0) + 1
    gate = dict(health(db))
    running = sum(a["state"] == "running" for a in attempts)
    gate["effective_slots"] = effective_admission(db, configured_slots() or 0, running)
    gate["effective_slots"] = min(gate["effective_slots"], max(0, (configured_slots() or 0) - (resource_usage(db) - running))) if inventory_ready(db) else running
    dispatch_path = STATE / 'controller.json'
    dispatch = json.loads(dispatch_path.read_text()) if dispatch_path.exists() else {}
    if not alive(dispatch.get('pid')):
        dispatch = {}
    dispatch['prerequisite_repairs'] = prerequisite_repairs
    dispatch['candidate_limit'] = max(1, int(os.environ.get('GYRE_DEV_MAX_CANDIDATES', '8')))
    started = db.execute("SELECT min(started) FROM attempts").fetchone()[0]
    hours = max(1 / 60, (time.time() - started) / 3600) if started else 0
    check_counts = dict(db.execute("SELECT state,count(*) FROM attempts WHERE kind='check' GROUP BY state"))
    ended_checks = check_counts.get("done", 0) + check_counts.get("failed", 0)
    queued = db.execute("SELECT min(candidate_at) FROM tasks WHERE state IN ('candidate','promoting','published')").fetchone()[0]
    deliveries = db.execute("SELECT count(DISTINCT events.message) FROM events JOIN tasks ON events.task=tasks.name WHERE message LIKE 'merged %' AND tasks.audit_contract IS NULL").fetchone()[0]
    repairs = db.execute("SELECT count(*) FROM events WHERE message LIKE '%queued implementation repair %'").fetchone()[0]
    return {"tasks": tasks, "attempts": attempts, "events": events,
            "counts": counts, "eligible": eligible,
            "dispatch": dispatch,
            "confirmed_merges": sum(task["merge_sha"] is not None and not task['audit_contract'] for task in tasks),
            "confirmed_audits": sum(task["merge_sha"] is not None and bool(task['audit_contract']) for task in tasks),
            "resources": {"used": resource_usage(db), "inventory_ready": inventory_ready(db),
                          "deletion_pending": db.execute("SELECT count(*) FROM resources WHERE present=1 AND delete_pending=1").fetchone()[0]},
            "metrics": {"deliveries_per_hour": deliveries / hours if hours else 0,
                        "check_pass_rate": check_counts.get("done", 0) / ended_checks if ended_checks else None,
                        "automatic_repairs": repairs,
                        "oldest_candidate_age": int(time.time() - queued) if queued else 0,
                        "candidate_backlog": sum(task["state"] in ("candidate", "checking", "promoting", "published") for task in tasks)},
            "running": running,
            "slots": configured_slots(), "health": gate}


def retry_failed_task(db, task):
    latest = run("git", "for-each-ref", "--format=%(refname:short) %(objectname)",
                 f"refs/remotes/origin/devloop/{task['name']}/attempt-*").stdout.splitlines()
    branches = []
    for line in latest:
        match = re.fullmatch(rf"origin/devloop/{re.escape(task['name'])}/attempt-(\d+) ([0-9a-f]{{40}})", line)
        if match:
            branches.append((int(match.group(1)), match.group(2)))
    number, seed = max(branches, default=(task["attempts"], task["seed"] or ""))
    seed = seed or task["seed"]
    candidate = seed if not task["feedback"] and seed and task_progress(seed, task["name"]) == "complete" else None
    attempt_number = max(number, task["attempts"])
    updated = db.execute("UPDATE tasks SET state=?,candidate=?,attempts=?,retry_baseline=?,seed=?,repairs=0 WHERE name=? AND state='failed'",
                         ("candidate" if candidate else "ready", candidate, attempt_number, attempt_number, seed, task["name"]))
    if updated.rowcount != 1:
        db.rollback()
        return False
    event(db, task["name"], f"operator requested retry from {seed or task['seed']}")
    if task["condition"] == "Sandbox configuration invalid":
        db.execute("UPDATE controller_health SET condition='Healthy',failures=0,retry_at=0 WHERE id=1")
        db.commit()
    return True


def main():
    global PUBLICATION_MODE, DISPATCH_TASK
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("sync", "status", "run", "retry", "retry-all", "host-gate"))
    parser.add_argument("task", nargs="?", help="task name for retry")
    parser.add_argument("--slots", type=int, default=1)
    parser.add_argument("--launch-burst", type=int, default=int(os.environ.get("GYRE_DEV_LAUNCH_BURST", "8")),
                        help="maximum new sandboxes to start per scheduling cycle")
    parser.add_argument("--max-attempts", type=int, default=3)
    parser.add_argument("--interval", type=int, default=30)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--json", action="store_true", help="machine-readable status")
    parser.add_argument("--only-task", help="dispatch only this task (task-NNN)")
    parser.add_argument("--publication", choices=("merge", "pr"), default=PUBLICATION_MODE,
                        help="create a verified PR, then merge upstream (default) or leave the PR open")
    parser.add_argument("--merge-sha", help=argparse.SUPPRESS)
    parser.add_argument("--check-id", help=argparse.SUPPRESS)
    args = parser.parse_args()
    PUBLICATION_MODE = args.publication
    DISPATCH_TASK = args.only_task
    if args.slots < 0 or args.interval < 1 or args.max_attempts < 1 or args.launch_burst < 1:
        parser.error("slots must be nonnegative; interval, max-attempts, and launch-burst must be positive")
    if args.only_task and not re.fullmatch(r"task-\d+", args.only_task):
        parser.error("--only-task requires task-NNN")
    if args.command == "host-gate":
        if not re.fullmatch(r"[0-9a-f]{40}", args.merge_sha or "") or not re.fullmatch(r"[0-9a-f]{1,32}", args.check_id or ""):
            parser.error("host-gate requires a merge SHA and check id")
        sys.exit(0 if host_test_verified(args.merge_sha, args.check_id) else 1)
    db = db_open()
    if args.command == "status":
        if args.json:
            print(json.dumps(status_snapshot(db)))
        else:
            status(db)
        return
    if args.json:
        parser.error("--json is only valid with status")
    if args.command in ("retry", "retry-all"):
        if args.command == "retry":
            if not args.task or not re.fullmatch(r"task-\d+", args.task):
                parser.error("retry requires task-NNN")
            tasks = db.execute("SELECT * FROM tasks WHERE name=? AND state='failed'", (args.task,)).fetchall()
            if not tasks:
                raise RuntimeError(f"{args.task} is not a failed task")
        else:
            if args.task:
                parser.error("retry-all takes no task name")
            tasks = db.execute("SELECT * FROM tasks WHERE state='failed' ORDER BY name").fetchall()
            if not tasks:
                print("retried 0 failed tasks")
                return
        source()
        count = sum(retry_failed_task(db, task) for task in tasks)
        if args.command == "retry-all":
            print(f"retried {count} failed tasks")
        return
    if args.command == "sync":
        sync(db)
        status(db)
        return
    lock = (STATE / "controller.lock").open("a+")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        lock.seek(0)
        owner = lock.read().strip()
        parser.error(f"another dev controller is running{f' (PID {owner})' if owner.isdigit() else ''}")
    old = legacy_running()
    if old:
        parser.error(f"existing loop is active: {old}. Stop it before starting the new controller")
    lock.seek(0)
    lock.truncate()
    lock.write(str(os.getpid()) + "\n")
    lock.flush()
    (STATE / 'controller.json').write_text(json.dumps({'pid': os.getpid(), 'only_task': args.only_task,
                                                       'publication': PUBLICATION_MODE}))
    if not (STATE / "slots").exists():
        (STATE / "slots").write_text(str(args.slots) + "\n")
    source_error = None
    recover_prior_infrastructure_failures(db)
    recover_prior_quality_failures(db)
    while True:
        old = legacy_running()
        if old:
            raise RuntimeError(f"existing loop started during this run: {old}; controller halted")
        # Resource cleanup must remain independent of GitHub availability.
        recover_launches(db)
        observe_phases(db)
        reap_host_processes()
        observe_ready(db)
        gc_sandboxes(db)
        try:
            sync(db)
            reap(db)
            promote(db)
            if not args.only_task and configured_slots():
                plan_audits(db)
            if args.only_task and not db.execute("SELECT 1 FROM tasks WHERE name=?", (args.only_task,)).fetchone():
                raise RuntimeError(f"unknown task: {args.only_task}")
            if configured_slots() and inventory_ready(db) and providers_ready(db):
                schedule(db, configured_slots(), args.max_attempts, args.only_task, args.launch_burst)
            elif not configured_slots():
                schedule(db, 0, args.max_attempts, args.only_task, args.launch_burst)
        except SourceUnavailable as exc:
            db.rollback()
            if str(exc) != source_error:
                event(db, None, str(exc))
                source_error = str(exc)
            if args.once:
                raise
        else:
            if source_error:
                event(db, None, "source refresh recovered")
                source_error = None
        if args.once:
            break
        time.sleep(args.interval)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, sqlite3.Error) as exc:
        print(f"dev-controller: {exc}", file=sys.stderr)
        sys.exit(1)
