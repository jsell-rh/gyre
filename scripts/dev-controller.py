#!/usr/bin/env python3
"""Durable, parallel sandbox development controller."""
import argparse
import fcntl
import json
import os
from pathlib import Path
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
      state TEXT NOT NULL, seed TEXT, candidate TEXT, attempts INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS attempts (
      id TEXT PRIMARY KEY, task TEXT NOT NULL, kind TEXT NOT NULL,
      branch TEXT, sha TEXT, base TEXT, merge_sha TEXT,
      state TEXT NOT NULL, pid INTEGER, started INTEGER NOT NULL,
      ended INTEGER, detail TEXT);
    CREATE TABLE IF NOT EXISTS events (
      id INTEGER PRIMARY KEY, at INTEGER NOT NULL, task TEXT, message TEXT NOT NULL);
    """)
    return db


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
        elif old and old["state"] in ("running", "checking", "promoting", "candidate", "failed"):
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
    log = (directory / "output.log").open("ab", buffering=0)
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
    p = subprocess.Popen([str(wrapper), str(directory / "exit"), str(snapshot), *args],
                         cwd=ROOT, env={**os.environ, "GYRE_DEV_ROOT": str(ROOT)},
                         stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    db.execute("INSERT INTO attempts(id,task,kind,branch,sha,base,state,pid,started) VALUES(?,?,?,?,?,?,?,?,?)",
               (ident, task["name"], kind, branch, sha, base, "running", p.pid, int(time.time())))
    db.execute("UPDATE tasks SET state=? WHERE name=?", ("running" if kind == "worker" else "checking", task["name"]))
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
        db.execute("UPDATE attempts SET state=?,ended=?,detail=? WHERE id=?",
                   ("done" if rc == 0 else "failed", int(time.time()), f"exit={rc}", attempt["id"]))
        if attempt["kind"] == "worker" and rc in (75, 76):
            # Bootstrap and push failures already retried inside this sandbox.
            # A push failure may have a local recovery.patch; never provision
            # another heavy sandbox automatically for the same outage.
            db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
            reason = "sandbox bootstrap" if rc == 75 else "branch push"
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
             if known.get(line.strip()) in ("done", "failed")]
    for name in stale[:8]:
        try:
            deleted = subprocess.run([openshell, "-g", "gyre-gyre", "sandbox", "delete", name],
                                     env=env, capture_output=True, text=True, timeout=120)
            if deleted.returncode == 0:
                event(db, None, f"deleted orphaned sandbox {name}")
            else:
                event(db, None, f"sandbox cleanup will retry {name}")
        except (OSError, subprocess.TimeoutExpired):
            event(db, None, f"sandbox cleanup timed out; will retry {name}")


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
            db.commit()
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


def schedule(db, slots, max_attempts, only_task=None):
    running = db.execute("SELECT count(*) FROM attempts WHERE state='running'").fetchone()[0]
    if running >= slots:
        return
    rows = db.execute("SELECT * FROM tasks ORDER BY CASE progress WHEN 'needs-revision' THEN 0 ELSE 1 END,name").fetchall()
    merged = {r["name"] for r in rows if r["state"] == "merged"}
    for task in rows:
        if only_task and task["name"] != only_task:
            continue
        if running >= slots:
            break
        if task["state"] == "candidate" and set(json.loads(task["deps"])) <= merged:
            source()
            sha = task["candidate"]
            base = ref_sha("origin/main")
            if not sha or not ref_exists(sha):
                db.execute("UPDATE tasks SET state='ready',candidate=NULL WHERE name=?", (task["name"],))
                db.commit()
                continue
            spawn(db, task, "check", sha=sha, base=base)
            running += 1
        elif task["state"] == "ready" and task["progress"] in ("not-started", "needs-revision") and set(json.loads(task["deps"])) <= merged:
            if task["attempts"] >= max_attempts:
                db.execute("UPDATE tasks SET state='failed' WHERE name=?", (task["name"],))
                event(db, task["name"], f"attempt limit {max_attempts} reached; inspect logs and retry explicitly")
                continue
            attempt_no = task["attempts"] + 1
            branch = f"devloop/{task['name']}/attempt-{attempt_no}"
            db.execute("UPDATE tasks SET attempts=? WHERE name=?", (attempt_no, task["name"]))
            db.commit()
            spawn(db, task, "worker", branch=branch)
            running += 1


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
    for task in tasks:
        task["deps"] = json.loads(task["deps"])
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
    return {"tasks": tasks, "attempts": attempts, "events": events,
            "counts": counts, "eligible": eligible,
            "running": sum(a["state"] == "running" for a in attempts),
            "slots": configured_slots()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("sync", "status", "run", "retry"))
    parser.add_argument("task", nargs="?", help="task name for retry")
    parser.add_argument("--slots", type=int, default=1)
    parser.add_argument("--max-attempts", type=int, default=3)
    parser.add_argument("--interval", type=int, default=30)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--json", action="store_true", help="machine-readable status")
    parser.add_argument("--only-task", help="dispatch only this task (task-NNN)")
    args = parser.parse_args()
    if args.slots < 0 or args.interval < 1 or args.max_attempts < 1:
        parser.error("slots must be nonnegative; interval and max-attempts must be positive")
    if args.only_task and not re.fullmatch(r"task-\d+", args.only_task):
        parser.error("--only-task requires task-NNN")
    db = db_open()
    if args.command == "status":
        if args.json:
            print(json.dumps(status_snapshot(db)))
        else:
            status(db)
        return
    if args.json:
        parser.error("--json is only valid with status")
    if args.command == "retry":
        if not args.task or not re.fullmatch(r"task-\d+", args.task):
            parser.error("retry requires task-NNN")
        task = db.execute("SELECT * FROM tasks WHERE name=? AND state='failed'", (args.task,)).fetchone()
        if task is None:
            raise RuntimeError(f"{args.task} is not a failed task")
        source()
        latest = run("git", "for-each-ref", "--format=%(refname:short) %(objectname)",
                     f"refs/remotes/origin/devloop/{args.task}/attempt-*").stdout.splitlines()
        branches = []
        for line in latest:
            match = re.fullmatch(rf"origin/devloop/{re.escape(args.task)}/attempt-(\d+) ([0-9a-f]{{40}})", line)
            if match:
                branches.append((int(match.group(1)), match.group(2)))
        number, seed = max(branches, default=(task["attempts"], task["seed"] or ""))
        seed = seed or task["seed"]
        candidate = seed if seed and task_progress(seed, args.task) == "complete" else None
        db.execute("UPDATE tasks SET state=?,candidate=?,attempts=?,seed=? WHERE name=?",
                   ("candidate" if candidate else "ready", candidate,
                    max(number, task["attempts"]), seed, args.task))
        db.commit()
        event(db, args.task, f"operator requested retry from {seed or task['seed']}")
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
    while True:
        old = legacy_running()
        if old:
            raise RuntimeError(f"existing loop started during this run: {old}; controller halted")
        try:
            sync(db)
            reap(db)
            if time.time() - last_gc >= 60:
                gc_sandboxes(db)
                last_gc = time.time()
            promote(db)
            if args.only_task and not db.execute("SELECT 1 FROM tasks WHERE name=?", (args.only_task,)).fetchone():
                raise RuntimeError(f"unknown task: {args.only_task}")
            schedule(db, configured_slots(), args.max_attempts, args.only_task)
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
