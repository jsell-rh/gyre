#!/usr/bin/env python3
"""Durable, parallel sandbox development controller."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import fcntl
import json
import os
from pathlib import Path
import random
import re
import sqlite3
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent
STATE = Path(os.environ.get("GYRE_DEV_STATE", ROOT / ".gyre-dev-controller")).resolve()
SOURCE = STATE / "source"
TASK_RE = re.compile(r"^specs/tasks/(task-\d+)\.md$")
DEP_RE = re.compile(r"task-\d+")
HOST_GATE_PROCESSES = {}


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
    match = re.search(rf"^{re.escape(key)}:\s*(.*)$", body.split("---", 2)[1], re.M)
    return match.group(1).strip().strip('"') if match else ""


def deps(body):
    value = field(body, "depends_on")
    if value and value != "[]":
        return DEP_RE.findall(value)
    match = re.search(r"^depends_on:\s*\n((?:  - .*\n)*)", body.split("---", 2)[1], re.M)
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
    """)
    if "retry_baseline" not in {row[1] for row in db.execute("PRAGMA table_info(tasks)")}:
        db.execute("ALTER TABLE tasks ADD COLUMN retry_baseline INTEGER NOT NULL DEFAULT 0")
    for column, definition in (("retry_at", "INTEGER NOT NULL DEFAULT 0"), ("condition", "TEXT")):
        if column not in {row[1] for row in db.execute("PRAGMA table_info(tasks)")}:
            db.execute(f"ALTER TABLE tasks ADD COLUMN {column} {definition}")
    if "ready_observed" not in {row[1] for row in db.execute("PRAGMA table_info(attempts)")}:
        db.execute("ALTER TABLE attempts ADD COLUMN ready_observed INTEGER NOT NULL DEFAULT 0")
    if "host_pid" not in {row[1] for row in db.execute("PRAGMA table_info(attempts)")}:
        db.execute("ALTER TABLE attempts ADD COLUMN host_pid INTEGER")
    return db


def health(db):
    return db.execute("SELECT * FROM controller_health WHERE id=1").fetchone()


def effective_admission(db, slots, running, now=None):
    """Existing work keeps its slots; a degraded gateway gets one probe."""
    now = int(time.time()) if now is None else now
    gate = health(db)
    if gate["condition"] == "ConfigurationInvalid":
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
        if current["condition"] == "ConfigurationInvalid":
            db.commit()
            continue
        running = db.execute("SELECT count(*) FROM attempts WHERE state='running'").fetchone()[0]
        db.execute("UPDATE controller_health SET failures=0,retry_at=0,admission=?,condition='Healthy' WHERE id=1",
                   (min(max(current["admission"] + 1, running + 1), 1000),))
        event(db, attempt["task"], "sandbox Ready; gateway admission increased")


def recover_prior_infrastructure_failures(db):
    """Migrate the old explicit-retry failures using evidence in their local logs."""
    for task in db.execute("SELECT * FROM tasks WHERE state='failed'").fetchall():
        attempt = db.execute("SELECT * FROM attempts WHERE task=? ORDER BY rowid DESC LIMIT 1",
                             (task["name"],)).fetchone()
        if not attempt or attempt["detail"] not in ("exit=75", "exit=77"):
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


def source():
    try:
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
    source()
    paths = git("ls-tree", "-r", "--name-only", "origin/main", "specs/tasks").splitlines()
    for path in paths:
        match = TASK_RE.match(path)
        if not match:
            continue
        name = match.group(1)
        body = task_body("origin/main", name)
        progress = field(body, "progress")
        old = db.execute("SELECT * FROM tasks WHERE name=?", (name,)).fetchone()
        branch = f"origin/worker/{name}"
        seed = ref_sha(branch)
        candidate = seed if seed and task_progress(branch, name) == "complete" and run(
            "git", "merge-base", "--is-ancestor", seed, "origin/main", check=False).returncode != 0 else None
        if progress == "complete":
            state = "merged"
        elif old and old["state"] in ("running", "checking", "promoting", "candidate", "failed", "deferred"):
            state = old["state"]
            candidate = old["candidate"] or candidate
            seed = old["seed"] or seed
        else:
            # A ready task may carry a checkpoint from a failed attempt.
            # Keep it through sync so the next worker resumes that commit.
            if old:
                seed = old["seed"] or seed
            state = "candidate" if candidate else "ready"
        db.execute("""INSERT INTO tasks(name,progress,deps,state,seed,candidate) VALUES(?,?,?,?,?,?)
          ON CONFLICT(name) DO UPDATE SET progress=excluded.progress,deps=excluded.deps,
          state=excluded.state,seed=excluded.seed,candidate=excluded.candidate""",
                   (name, progress, json.dumps(deps(body)), state, seed, candidate))
    db.commit()


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


def spawn(db, task, kind, branch=None, sha=None, base=None):
    ident = uuid.uuid4().hex[:16]
    directory = STATE / "attempts" / ident
    directory.mkdir(parents=True)
    if kind == "worker":
        script = ROOT / "scripts/dev-sandbox.sh"
        args = ["worker", task["name"], branch, task["seed"] or "origin/main", ident]
    else:
        script = ROOT / "scripts/dev-sandbox.sh"
        args = ["check", task["name"], sha, base, ident]
    # Bash reads scripts incrementally. Snapshot the driver before launch so
    # edits to the repo cannot corrupt a running attempt's parse stream.
    snapshot = directory / "dev-sandbox.sh"
    snapshot.write_bytes(script.read_bytes())
    snapshot.chmod(0o700)
    # The wrapper records exit status even if the controller itself exits.
    wrapper = ROOT / "scripts/dev-process.sh"
    with (directory / "output.log").open("ab", buffering=0) as log:
        p = subprocess.Popen([str(wrapper), str(directory / "exit"), str(snapshot), *args],
                             cwd=ROOT, env={**os.environ, "GYRE_DEV_ROOT": str(ROOT)},
                             stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    db.execute("INSERT INTO attempts(id,task,kind,branch,sha,base,state,pid,started) VALUES(?,?,?,?,?,?,?,?,?)",
               (ident, task["name"], kind, branch, sha, base, "running", p.pid, int(time.time())))
    db.execute("UPDATE tasks SET state=?,condition=NULL,retry_at=0 WHERE name=?",
               ("running" if kind == "worker" else "checking", task["name"]))
    db.commit()
    event(db, task["name"], f"{kind} {ident} pid={p.pid} branch={branch or '-'} sha={sha or '-'}")


def alive(pid):
    try:
        stat = Path(f"/proc/{pid}/stat").read_text().split()
        return stat[2] != "Z"
    except OSError:
        return False


def reap(db):
    for attempt in db.execute("SELECT * FROM attempts WHERE state='running'").fetchall():
        exit_file = STATE / "attempts" / attempt["id"] / "exit"
        if not exit_file.exists() and alive(attempt["pid"]):
            continue
        rc = int(exit_file.read_text().strip()) if exit_file.exists() else 255
        task = db.execute("SELECT * FROM tasks WHERE name=?", (attempt["task"],)).fetchone()
        outcome = "done" if rc == 0 else "deferred" if rc in (77, 78) else "failed"
        db.execute("UPDATE attempts SET state=?,ended=?,detail=? WHERE id=?",
                   (outcome, int(time.time()), f"exit={rc}", attempt["id"]))
        if rc in (77, 78):
            reason = "Capacity unavailable" if rc == 78 else "Gateway transport unavailable"
            defer_infrastructure(db, task, reason, worker=attempt["kind"] == "worker")
        elif rc == 79:
            db.execute("UPDATE tasks SET state='failed',condition='Sandbox configuration invalid' WHERE name=?", (task["name"],))
            db.execute("UPDATE controller_health SET condition='ConfigurationInvalid',admission=1 WHERE id=1")
            event(db, task["name"], "sandbox configuration invalid; inspect attempt log and repair gateway configuration")
        elif attempt["kind"] == "worker" and rc in (75, 76):
            # Bootstrap and push failures already retried inside this sandbox.
            # A push failure may have a local recovery.patch; never provision
            # another heavy sandbox automatically for the same outage.
            db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
            reason = {75: "sandbox bootstrap", 76: "branch push"}[rc]
            event(db, task["name"], f"{reason} failed; inspect log/recovery.patch and retry explicitly")
        elif attempt["kind"] == "worker":
            # A crashed driver may have pushed useful progress. Always inspect its exact ref.
            source()
            remote = f"origin/{attempt['branch']}"
            sha = ref_sha(remote)
            if sha and task_progress(remote, task["name"]) == "complete":
                db.execute("UPDATE tasks SET state='candidate',candidate=?,seed=? WHERE name=?",
                           (sha, sha, task["name"]))
                event(db, task["name"], f"candidate {sha}")
            else:
                db.execute("UPDATE tasks SET state='ready',seed=COALESCE(?,seed) WHERE name=?",
                           (sha, task["name"]))
                event(db, task["name"], f"worker exited {rc}; progress retained")
        elif rc == 0:
            db.execute("UPDATE tasks SET state='promoting' WHERE name=?", (task["name"],))
            event(db, task["name"], f"checks passed for {attempt['sha']} on {attempt['base']}")
        else:
            db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
            event(db, task["name"], f"checks failed for {attempt['sha']}; see attempts/{attempt['id']}/output.log")
        db.commit()


def gc_sandboxes(db):
    """Retry deletion of known attempts whose driver has already exited."""
    if not os.environ.get("OPENSHELL_OIDC_CLIENT_SECRET"):
        return
    env = dict(os.environ, OPENSHELL_NO_BROWSER="1", OPENSHELL_GATEWAY_INSECURE="true",
               OPENSHELL_WORKSPACE="default")
    openshell = os.environ.get("OPENSHELL", "openshell")
    try:
        with (STATE / "gateway-login.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            login = subprocess.run([openshell, "gateway", "login", "gyre-gyre"], env=env,
                                   capture_output=True, text=True, timeout=30)
        if login.returncode:
            event(db, None, "sandbox cleanup: gateway login failed")
            return
        listing = subprocess.run([openshell, "-g", "gyre-gyre", "sandbox", "list", "--names"],
                                 env=env, capture_output=True, text=True, timeout=30)
        if listing.returncode:
            event(db, None, "sandbox cleanup: gateway list failed")
            return
    except (OSError, subprocess.TimeoutExpired):
        event(db, None, "sandbox cleanup: gateway unavailable")
        return
    known = {}
    for attempt in db.execute("SELECT id,task,kind,state FROM attempts"):
        name = f"gyre-{attempt['task'][5:]}-{attempt['kind'][0]}-{attempt['id'][:8]}"
        known[name] = attempt["state"]
    stale = [line.strip() for line in listing.stdout.splitlines()
             if known.get(line.strip()) in ("done", "failed", "deferred")]
    def delete(name):
        try:
            deleted = subprocess.run([openshell, "-g", "gyre-gyre", "sandbox", "delete", name],
                                     env=env, capture_output=True, text=True, timeout=120)
            return name, deleted.returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            return name, False
    # Deletion is slow; a bounded pool clears failed attempts promptly without
    # letting cleanup itself flood the gateway or stall dispatch for minutes.
    with ThreadPoolExecutor(max_workers=4) as pool:
        for name, deleted in pool.map(delete, stale[:16]):
            event(db, None, f"{'deleted orphaned sandbox' if deleted else 'sandbox cleanup will retry'} {name}")


def promote(db):
    for task in db.execute("SELECT * FROM tasks WHERE state='promoting' ORDER BY name").fetchall():
        check = db.execute("SELECT * FROM attempts WHERE task=? AND kind='check' AND state='done' ORDER BY rowid DESC LIMIT 1",
                           (task["name"],)).fetchone()
        if not check or check["sha"] != task["candidate"]:
            db.execute("UPDATE tasks SET state='candidate' WHERE name=?", (task["name"],))
            db.commit()
            continue
        source()
        main = ref_sha("origin/main")
        if main == check["merge_sha"]:
            db.execute("UPDATE tasks SET state='merged' WHERE name=?", (task["name"],))
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
            raise RuntimeError("verified integration ref does not match checked base/candidate")
        db.execute("UPDATE attempts SET merge_sha=? WHERE id=?", (merge_sha, check["id"]))
        db.commit()
        gate_dir = STATE / "attempts" / check["id"]
        gate_exit = gate_dir / "host-tests.exit"
        gate_ok = gate_dir / "host-tests.ok"
        if not gate_ok.exists() or gate_ok.read_text().strip() != merge_sha:
            if not gate_exit.exists() and check["host_pid"] and alive(check["host_pid"]):
                return  # keep reconciling other tasks while the host gate runs
            if not gate_exit.exists() and not check["host_pid"]:
                start_host_gate(db, check["id"], merge_sha)
                event(db, task["name"], f"host full-suite gate started for {merge_sha}")
                return
            # A process that vanished without an exit record also fails closed.
            result = gate_exit.read_text().strip() if gate_exit.exists() else "missing"
            db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
            event(db, task["name"], f"host full-suite gate failed (exit={result}); see attempts/{check['id']}/host-tests.log")
            continue
        try:
            push = run("git", "push", "origin", f"{merge_sha}:refs/heads/main", check=False, timeout=90)
        except subprocess.TimeoutExpired as exc:
            # The push may have reached GitHub before the response timed out.
            # Leave this task promoting; the next fetch resolves the exact SHA.
            raise SourceUnavailable("main push timed out; checking remote state next cycle") from exc
        if push.returncode:
            db.execute("UPDATE tasks SET state='candidate' WHERE name=?", (task["name"],))
            event(db, task["name"], "main push rejected; rechecking against current main")
        else:
            db.execute("UPDATE tasks SET state='merged' WHERE name=?", (task["name"],))
            event(db, task["name"], f"merged {merge_sha}")


def host_test_verified(merge_sha, check_id):
    """Run full Rust and frontend suites on the exact verified merge tree."""
    attempt_dir = STATE / "attempts" / check_id
    attempt_dir.mkdir(parents=True, exist_ok=True)
    marker = attempt_dir / "host-tests.ok"
    if marker.exists() and marker.read_text().strip() == merge_sha:
        return True
    worktree = STATE / "host-check" / check_id
    worktree.parent.mkdir(parents=True, exist_ok=True)
    if worktree.exists():
        run("git", "worktree", "remove", "--force", str(worktree), check=False)
    added = run("git", "worktree", "add", "--detach", str(worktree), merge_sha, check=False)
    if added.returncode:
        (attempt_dir / "host-tests.log").write_text(added.stderr or added.stdout)
        return False
    passed = False
    try:
        env = {**os.environ, "SKIP_WEB_BUILD": "1",
               "CARGO_TARGET_DIR": str(STATE / "host-target")}
        with (attempt_dir / "host-tests.log").open("wb") as log:
            passed = True
            for command, cwd, timeout in (
                (["cargo", "test", "--all", "--quiet"], worktree, 1800),
                (["npm", "ci", "--no-audit", "--no-fund"], worktree / "web", 600),
                (["npm", "test"], worktree / "web", 1800),
            ):
                log.write(f"\n$ {' '.join(command)}\n".encode())
                try:
                    result = subprocess.run(command, cwd=cwd, env=env, stdout=log,
                                            stderr=subprocess.STDOUT, timeout=timeout)
                    if result.returncode:
                        passed = False
                        break
                except (OSError, subprocess.TimeoutExpired) as exc:
                    log.write(f"\nhost test execution failed: {exc}\n".encode())
                    passed = False
                    break
    finally:
        run("git", "worktree", "remove", "--force", str(worktree), check=False)
    if passed:
        marker.write_text(merge_sha + "\n")
    return passed


def start_host_gate(db, check_id, merge_sha):
    gate_dir = STATE / "attempts" / check_id
    gate_dir.mkdir(parents=True, exist_ok=True)
    with (gate_dir / "host-process.log").open("ab", buffering=0) as log:
        p = subprocess.Popen([str(ROOT / "scripts/dev-process.sh"), str(gate_dir / "host-tests.exit"),
                              sys.executable, str(ROOT / "scripts/dev-controller.py"), "host-gate",
                              "--merge-sha", merge_sha, "--check-id", check_id],
                             cwd=ROOT, env={**os.environ, "GYRE_DEV_STATE": str(STATE)},
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
    rows = db.execute("SELECT * FROM tasks ORDER BY CASE progress WHEN 'needs-revision' THEN 0 ELSE 1 END,name").fetchall()
    # Surface exhausted tasks even when every slot is occupied or dispatch is
    # drained; otherwise they stay misleadingly ready until a slot opens.
    exhausted = {task["name"] for task in rows if task["state"] == "ready" and
                 task["attempts"] - task["retry_baseline"] >= max_attempts}
    for name in sorted(exhausted):
        db.execute("UPDATE tasks SET state='failed' WHERE name=? AND state='ready'", (name,))
        event(db, name, f"attempt limit {max_attempts} reached; inspect logs and retry explicitly")
    running = db.execute("SELECT count(*) FROM attempts WHERE state='running'").fetchone()[0]
    launches = 0
    effective_slots = effective_admission(db, slots, running, now)
    if running >= effective_slots:
        return
    if gate["failures"]:
        launch_burst = 1
    merged = {r["name"] for r in rows if r["state"] == "merged"}
    for task in rows:
        if task["name"] in exhausted:
            continue
        if only_task and task["name"] != only_task:
            continue
        if running >= effective_slots or launches >= launch_burst:
            break
        is_due = task["state"] == "deferred" and task["retry_at"] <= now
        if (task["state"] == "candidate" or (is_due and task["candidate"])) and set(json.loads(task["deps"])) <= merged:
            source()
            sha = task["candidate"]
            base = ref_sha("origin/main")
            if not sha or not ref_exists(sha):
                db.execute("UPDATE tasks SET state='ready',candidate=NULL WHERE name=?", (task["name"],))
                db.commit()
                continue
            spawn(db, task, "check", sha=sha, base=base)
            running += 1
            launches += 1
        elif (task["state"] == "ready" or is_due) and task["progress"] in ("not-started", "needs-revision") and set(json.loads(task["deps"])) <= merged:
            attempt_no = task["attempts"] + 1
            branch = f"devloop/{task['name']}/attempt-{attempt_no}"
            db.execute("UPDATE tasks SET attempts=? WHERE name=?", (attempt_no, task["name"]))
            db.commit()
            spawn(db, task, "worker", branch=branch)
            running += 1
            launches += 1


def status(db):
    for state, count in db.execute("SELECT state,count(*) FROM tasks GROUP BY state ORDER BY state"):
        print(f"{state}: {count}")
    rows = db.execute("SELECT name,state,progress,deps FROM tasks").fetchall()
    merged = {r["name"] for r in rows if r["state"] == "merged"}
    eligible = sum((r["state"] == "candidate" or
                    (r["state"] == "ready" and r["progress"] in ("not-started", "needs-revision"))) and
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
                    (task["state"] == "ready" and task["progress"] in ("not-started", "needs-revision"))) and
                   set(task["deps"]) <= merged for task in tasks)
    counts = {}
    for task in tasks:
        counts[task["state"]] = counts.get(task["state"], 0) + 1
    gate = dict(health(db))
    running = sum(a["state"] == "running" for a in attempts)
    gate["effective_slots"] = effective_admission(db, configured_slots() or 0, running)
    return {"tasks": tasks, "attempts": attempts, "events": events,
            "counts": counts, "eligible": eligible,
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
    candidate = seed if seed and task_progress(seed, task["name"]) == "complete" else None
    attempt_number = max(number, task["attempts"])
    updated = db.execute("UPDATE tasks SET state=?,candidate=?,attempts=?,retry_baseline=?,seed=? WHERE name=? AND state='failed'",
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
    parser.add_argument("--merge-sha", help=argparse.SUPPRESS)
    parser.add_argument("--check-id", help=argparse.SUPPRESS)
    args = parser.parse_args()
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
    lock = (STATE / "controller.lock").open("w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        parser.error("another dev controller is running")
    old = legacy_running()
    if old:
        parser.error(f"existing loop is active: {old}. Stop it before starting the new controller")
    lock.write(str(os.getpid()) + "\n")
    lock.flush()
    if not (STATE / "slots").exists():
        (STATE / "slots").write_text(str(args.slots) + "\n")
    last_gc = 0
    source_error = None
    recover_prior_infrastructure_failures(db)
    while True:
        old = legacy_running()
        if old:
            raise RuntimeError(f"existing loop started during this run: {old}; controller halted")
        try:
            sync(db)
            reap_host_processes()
            observe_ready(db)
            reap(db)
            if time.time() - last_gc >= 60:
                gc_sandboxes(db)
                last_gc = time.time()
            promote(db)
            if args.only_task and not db.execute("SELECT 1 FROM tasks WHERE name=?", (args.only_task,)).fetchone():
                raise RuntimeError(f"unknown task: {args.only_task}")
            schedule(db, configured_slots(), args.max_attempts, args.only_task, args.launch_burst)
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
