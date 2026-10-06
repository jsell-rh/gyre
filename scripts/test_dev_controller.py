#!/usr/bin/env python3
"""Local Git integration checks for the durable controller; no cloud access."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import time
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("dev_controller", Path(__file__).with_name("dev-controller.py"))
controller = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(controller)


def git(cwd, *args):
    return subprocess.check_output(["git", *args], cwd=cwd, text=True, stderr=subprocess.DEVNULL).strip()


class ControllerGitTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.remote = root / "remote.git"
        self.work = root / "work"
        controller.STATE = root / "state"
        controller.SOURCE = controller.STATE / "source"
        git(root, "init", "--bare", str(self.remote))
        git(root, "clone", str(self.remote), str(self.work))
        git(self.work, "config", "user.name", "Test")
        git(self.work, "config", "user.email", "test@example.com")
        git(self.work, "checkout", "-b", "main")
        (self.work / "specs/tasks").mkdir(parents=True)
        self.write_task("not-started")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "base")
        git(self.work, "push", "origin", "main")
        git(self.remote, "symbolic-ref", "HEAD", "refs/heads/main")
        controller.SOURCE.parent.mkdir()
        git(root, "clone", "--no-checkout", str(self.remote), str(controller.SOURCE))
        self.db = controller.db_open()
        self.addCleanup(self.db.close)

    def write_task(self, progress):
        (self.work / "specs/tasks/task-001.md").write_text(
            f"---\nprogress: {progress}\ndepends_on: []\n---\n\nImplement the task.\n")

    def candidate(self):
        git(self.work, "checkout", "-b", "worker/task-001")
        self.write_task("complete")
        (self.work / "implementation.txt").write_text("real work\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "implement")
        sha = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", "worker/task-001")
        return sha

    def test_imports_legacy_candidate_without_claiming_it_merged(self):
        sha = self.candidate()
        controller.sync(self.db)
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual((task["state"], task["candidate"], task["progress"]),
                         ("candidate", sha, "not-started"))

    def test_recovers_pushed_worker_after_controller_restart(self):
        sha = self.candidate()
        controller.sync(self.db)
        git(self.work, "push", "origin", "HEAD:refs/heads/devloop/task-001/attempt-1")
        directory = controller.STATE / "attempts" / "recovered"
        directory.mkdir(parents=True)
        (directory / "exit").write_text("0\n")
        self.db.execute("""INSERT INTO attempts(id,task,kind,branch,state,pid,started)
                           VALUES('recovered','task-001','worker',
                           'devloop/task-001/attempt-1','running',999999,1)""")
        self.db.execute("UPDATE tasks SET state='running',candidate=NULL WHERE name='task-001'")
        self.db.commit()
        controller.reap(self.db)
        task = self.db.execute("SELECT state,candidate FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual((task["state"], task["candidate"]), ("candidate", sha))

    def test_bootstrap_and_push_failures_require_explicit_retry(self):
        controller.sync(self.db)
        for code in (75, 76):
            with self.subTest(exit_code=code):
                ident = f"failure{code}"
                directory = controller.STATE / "attempts" / ident
                directory.mkdir(parents=True)
                (directory / "exit").write_text(f"{code}\n")
                self.db.execute("""INSERT INTO attempts(id,task,kind,branch,state,pid,started)
                                   VALUES(?, 'task-001','worker',
                                   'devloop/task-001/attempt-1','running',999999,1)""", (ident,))
                self.db.execute("UPDATE tasks SET state='running',attempts=1 WHERE name='task-001'")
                self.db.commit()
                controller.reap(self.db)
                task = self.db.execute("SELECT state,attempts FROM tasks WHERE name='task-001'").fetchone()
                self.assertEqual((task["state"], task["attempts"]), ("failed", 1))

    def test_only_task_limits_dispatch(self):
        controller.sync(self.db)
        with patch.object(controller, "spawn") as spawn:
            controller.schedule(self.db, 1, 3, only_task="task-999")
            spawn.assert_not_called()
            controller.schedule(self.db, 1, 3, only_task="task-001")
            spawn.assert_called_once()
            self.assertEqual(spawn.call_args.args[1]["name"], "task-001")

    def test_sync_keeps_checkpoint_seed_for_ready_task(self):
        controller.sync(self.db)
        base = git(self.work, "rev-parse", "main")
        self.db.execute("UPDATE tasks SET state='ready',seed=? WHERE name='task-001'", (base,))
        self.db.commit()
        controller.sync(self.db)
        task = self.db.execute("SELECT seed FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual(task["seed"], base)

    def test_retry_refreshes_stale_candidate_from_latest_completed_attempt(self):
        old = self.candidate()
        controller.sync(self.db)
        (self.work / "implementation.txt").write_text("fixed work\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "fix")
        latest = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", "HEAD:refs/heads/devloop/task-001/attempt-2")
        self.db.execute("UPDATE tasks SET state='failed',candidate=?,attempts=2 WHERE name='task-001'", (old,))
        self.db.commit()
        subprocess.run([sys.executable, str(Path(__file__).with_name("dev-controller.py")),
                        "retry", "task-001"], cwd=self.temp.name,
                       env={**os.environ, "GYRE_DEV_STATE": str(controller.STATE)}, check=True,
                       capture_output=True)
        task = self.db.execute("SELECT state,seed,candidate FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual(tuple(task), ("candidate", latest, latest))

    def test_controller_survives_remote_fetch_failure(self):
        git(self.temp.name, "-C", str(controller.SOURCE), "remote", "set-url", "origin", "/nonexistent/gyre.git")
        (controller.STATE / "slots").write_text("0\n")
        process = subprocess.Popen(
            ["python3", str(Path(__file__).with_name("dev-controller.py")), "run", "--slots", "0", "--interval", "1"],
            cwd=self.temp.name, env={**os.environ, "GYRE_DEV_STATE": str(controller.STATE)},
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if self.db.execute("SELECT 1 FROM events WHERE message LIKE 'source refresh failed:%'").fetchone():
                    break
                time.sleep(0.1)
            self.assertIsNone(process.poll(), "transient fetch failure killed the controller")
            self.assertIsNotNone(self.db.execute("SELECT 1 FROM events WHERE message LIKE 'source refresh failed:%'").fetchone())
        finally:
            process.terminate()
            process.wait(timeout=5)

    def test_cleanup_deletes_finished_sandbox_but_preserves_running_one(self):
        controller.sync(self.db)
        self.db.execute("""INSERT INTO attempts(id,task,kind,state,started)
                           VALUES('deadbeef12345678','task-001','worker','failed',1)""")
        self.db.execute("""INSERT INTO attempts(id,task,kind,state,started)
                           VALUES('feedface12345678','task-001','worker','running',1)""")
        self.db.commit()
        fake = Path(self.temp.name) / "fake-openshell"
        record = Path(self.temp.name) / "deleted.txt"
        fake.write_text("""#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
if 'list' in args:
    print('gyre-001-w-deadbeef')
    print('gyre-001-w-feedface')
elif 'delete' in args:
    with open(os.environ['GYRE_GC_RECORD'], 'a') as out:
        out.write(args[-1] + '\\n')
""")
        fake.chmod(0o755)
        with patch.dict("os.environ", {"OPENSHELL_OIDC_CLIENT_SECRET": "test",
                                    "OPENSHELL": str(fake), "GYRE_GC_RECORD": str(record)}):
            controller.gc_sandboxes(self.db)
        self.assertEqual(record.read_text().splitlines(), ["gyre-001-w-deadbeef"])

    def test_promotion_requires_checked_base_and_candidate(self):
        sha = self.candidate()
        controller.sync(self.db)
        base = git(self.work, "rev-parse", "main")
        git(self.work, "checkout", "main")
        git(self.work, "merge", "--no-ff", "--no-edit", "worker/task-001")
        merge = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", f"{merge}:refs/heads/devloop/verified/check1")
        self.db.execute("""INSERT INTO attempts(id,task,kind,sha,base,state,started)
                           VALUES('check1','task-001','check',?,?,'done',1)""", (sha, base))
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'")
        self.db.commit()
        # A new main commit invalidates the earlier check, even though the merge exists.
        (self.work / "new.txt").write_text("other task\n")
        git(self.work, "checkout", "main")
        git(self.work, "reset", "--hard", base)
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "other task")
        git(self.work, "push", "origin", "main")
        controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "candidate")
        self.assertNotEqual(git(self.remote, "rev-parse", "main"), merge)

    def test_promotes_only_the_verified_merge_commit(self):
        sha = self.candidate()
        controller.sync(self.db)
        base = git(self.work, "rev-parse", "main")
        git(self.work, "checkout", "main")
        git(self.work, "merge", "--no-ff", "--no-edit", "worker/task-001")
        merge = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", f"{merge}:refs/heads/devloop/verified/check1")
        self.db.execute("""INSERT INTO attempts(id,task,kind,sha,base,state,started)
                           VALUES('check1','task-001','check',?,?,'done',1)""", (sha, base))
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'")
        self.db.commit()
        controller.promote(self.db)
        self.assertEqual(git(self.remote, "rev-parse", "main"), merge)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "merged")

    def test_ambiguous_push_timeout_checks_remote_before_retrying(self):
        sha = self.candidate()
        controller.sync(self.db)
        base = git(self.work, "rev-parse", "main")
        git(self.work, "checkout", "main")
        git(self.work, "merge", "--no-ff", "--no-edit", "worker/task-001")
        merge = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", f"{merge}:refs/heads/devloop/verified/check1")
        self.db.execute("""INSERT INTO attempts(id,task,kind,sha,base,state,started)
                           VALUES('check1','task-001','check',?,?,'done',1)""", (sha, base))
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'")
        self.db.commit()
        real_run = controller.run

        def timeout_push(*args, **kwargs):
            if args[:2] == ("git", "push"):
                raise subprocess.TimeoutExpired(args, 90)
            return real_run(*args, **kwargs)

        with patch.object(controller, "run", side_effect=timeout_push):
            with self.assertRaises(controller.SourceUnavailable):
                controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "promoting")
        self.assertEqual(self.db.execute("SELECT merge_sha FROM attempts WHERE id='check1'").fetchone()[0], merge)
        git(self.work, "push", "origin", f"{merge}:refs/heads/main")
        controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "merged")


if __name__ == "__main__":
    unittest.main()
