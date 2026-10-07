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
REAL_HOST_TEST = controller.host_test_verified
REAL_START_HOST_GATE = controller.start_host_gate


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
        host_gate = patch.object(controller, "host_test_verified", return_value=True)
        self.host_gate = host_gate.start()
        self.addCleanup(host_gate.stop)
        def fake_host_gate(db, check_id, merge_sha):
            passed = controller.host_test_verified(merge_sha, check_id)
            directory = controller.STATE / "attempts" / check_id
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "host-tests.exit").write_text("0\n" if passed else "1\n")
            if passed:
                (directory / "host-tests.ok").write_text(merge_sha + "\n")
        host_start = patch.object(controller, "start_host_gate", side_effect=fake_host_gate)
        host_start.start()
        self.addCleanup(host_start.stop)

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

    def test_launch_burst_ramps_sandbox_creation(self):
        controller.sync(self.db)
        for name in ("task-002", "task-003"):
            self.db.execute("INSERT INTO tasks(name,progress,deps,state,attempts) VALUES(?, 'not-started', '[]', 'ready', 0)",
                            (name,))
        self.db.commit()
        self.db.execute("UPDATE controller_health SET admission=2 WHERE id=1")
        with patch.object(controller, "spawn") as spawn:
            controller.schedule(self.db, slots=50, max_attempts=3, launch_burst=2)
        self.assertEqual([call.args[1]["name"] for call in spawn.call_args_list], ["task-001", "task-002"])

    def test_capacity_failure_backs_off_without_using_task_budget_and_survives_restart(self):
        self.assertEqual([controller.backoff_seconds(n) for n in (1, 2, 3, 7)], [30, 60, 120, 900])
        controller.sync(self.db)
        directory = controller.STATE / "attempts" / "capacity"
        directory.mkdir(parents=True)
        (directory / "exit").write_text("78\n")
        self.db.execute("""INSERT INTO attempts(id,task,kind,branch,state,pid,started)
                           VALUES('capacity','task-001','worker','devloop/task-001/attempt-1','running',999999,1)""")
        self.db.execute("UPDATE tasks SET state='running',attempts=1 WHERE name='task-001'")
        self.db.commit()
        with patch.object(controller.random, "uniform", return_value=1), patch.object(controller.time, "time", return_value=1000):
            controller.reap(self.db)
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual((task["state"], task["retry_at"], task["retry_baseline"]), ("deferred", 1030, 1))
        self.assertEqual(self.db.execute("SELECT state FROM attempts WHERE id='capacity'").fetchone()[0], "deferred")
        self.assertEqual(controller.health(self.db)["admission"], 1)
        with patch.object(controller, "spawn") as spawn, patch.object(controller.time, "time", return_value=1029):
            controller.schedule(self.db, 50, 3)
            spawn.assert_not_called()
        reopened = controller.db_open()
        self.addCleanup(reopened.close)
        with patch.object(controller, "spawn") as spawn, patch.object(controller.time, "time", return_value=1030):
            controller.schedule(reopened, 50, 3)
            spawn.assert_called_once()
            self.assertEqual(spawn.call_args.kwargs["branch"], "devloop/task-001/attempt-2")

    def test_capacity_probe_does_not_wait_for_existing_workers_to_finish(self):
        controller.sync(self.db)
        self.db.executemany("""INSERT INTO attempts(id,task,kind,state,started)
                               VALUES(?,'task-001','worker','running',900)""",
                            [(f"active{i}",) for i in range(28)])
        self.db.execute("UPDATE controller_health SET failures=1,retry_at=1030,admission=1,condition='Capacity unavailable' WHERE id=1")
        self.db.commit()
        self.assertEqual(controller.effective_admission(self.db, 50, 28, 1029), 28)
        self.assertEqual(controller.effective_admission(self.db, 50, 28, 1030), 29)
        self.db.execute("""INSERT INTO attempts(id,task,kind,state,started)
                           VALUES('probe','task-001','worker','running',1030)""")
        self.db.commit()
        self.assertEqual(controller.effective_admission(self.db, 50, 29, 1031), 29)

    def test_ready_signal_recovers_gateway_and_ramps_one_slot(self):
        controller.sync(self.db)
        directory = controller.STATE / "attempts" / "ready"
        directory.mkdir(parents=True)
        (directory / "sandbox.ready").touch()
        self.db.execute("""INSERT INTO attempts(id,task,kind,state,pid,started)
                           VALUES('ready','task-001','worker','running',999999,1)""")
        self.db.execute("UPDATE controller_health SET failures=4,retry_at=5000,condition='Capacity unavailable' WHERE id=1")
        self.db.commit()
        controller.observe_ready(self.db)
        self.assertEqual(tuple(controller.health(self.db)[k] for k in ("failures", "retry_at", "admission", "condition")),
                         (0, 0, 2, "Healthy"))
        controller.observe_ready(self.db)
        self.assertEqual(controller.health(self.db)["admission"], 2)

    def test_prior_gateway_failure_migrates_but_clone_failure_stays_failed(self):
        controller.sync(self.db)
        for name, log in (("task-001", "ProvisioningTimedOut"),
                          ("task-002", "bootstrap fetch failed")):
            ident = name[-3:]
            directory = controller.STATE / "attempts" / ident
            directory.mkdir(parents=True)
            (directory / "output.log").write_text(log)
            if name == "task-002":
                self.db.execute("INSERT INTO tasks(name,progress,deps,state,attempts) VALUES(?, 'not-started','[]','failed',1)", (name,))
            self.db.execute("INSERT INTO attempts(id,task,kind,state,started,detail) VALUES(?,?,'worker','failed',1,'exit=75')", (ident, name))
        self.db.execute("UPDATE tasks SET state='failed',attempts=1 WHERE name='task-001'")
        self.db.commit()
        controller.recover_prior_infrastructure_failures(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "deferred")
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-002'").fetchone()[0], "failed")

    def test_invalid_configuration_halts_new_sandbox_admission(self):
        controller.sync(self.db)
        directory = controller.STATE / "attempts" / "invalid"
        directory.mkdir(parents=True)
        (directory / "exit").write_text("79\n")
        self.db.execute("INSERT INTO attempts(id,task,kind,state,pid,started) VALUES('invalid','task-001','worker','running',999999,1)")
        self.db.execute("UPDATE tasks SET state='running' WHERE name='task-001'")
        self.db.commit()
        controller.reap(self.db)
        self.assertEqual(controller.health(self.db)["condition"], "ConfigurationInvalid")
        self.db.execute("INSERT INTO tasks(name,progress,deps,state) VALUES('task-002','not-started','[]','ready')")
        self.db.commit()
        with patch.object(controller, "spawn") as spawn:
            controller.schedule(self.db, 50, 3)
            spawn.assert_not_called()

    def test_spawn_snapshots_driver_before_launch(self):
        controller.sync(self.db)
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        with patch.object(controller.subprocess, "Popen") as popen:
            popen.return_value.pid = 12345
            controller.spawn(self.db, task, "worker", branch="devloop/task-001/attempt-1")
        command = popen.call_args.args[0]
        snapshot = Path(command[2])
        self.assertEqual(snapshot.read_bytes(),
                         (controller.ROOT / "scripts/dev-sandbox.sh").read_bytes())
        self.assertEqual(popen.call_args.kwargs["env"]["GYRE_DEV_ROOT"], str(controller.ROOT))

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

    def test_retry_all_grants_new_attempts_after_limit(self):
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET state='failed',attempts=3 WHERE name='task-001'")
        self.db.commit()
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("dev-controller.py")),
                                 "retry-all"], cwd=self.temp.name,
                                env={**os.environ, "GYRE_DEV_STATE": str(controller.STATE)}, check=True,
                                capture_output=True, text=True)
        self.assertIn("retried 1 failed tasks", result.stdout)
        task = self.db.execute("SELECT state,attempts,retry_baseline FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual(tuple(task), ("ready", 3, 3))
        with patch.object(controller, "spawn") as spawn:
            controller.schedule(self.db, slots=1, max_attempts=3)
        self.assertEqual(spawn.call_args.kwargs["branch"], "devloop/task-001/attempt-4")

    def test_exhausted_ready_task_needs_attention_even_when_drained(self):
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET attempts=3 WHERE name='task-001'")
        self.db.commit()
        controller.schedule(self.db, slots=0, max_attempts=3)
        task = self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual(task["state"], "failed")

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
        self.db.execute("""INSERT INTO attempts(id,task,kind,state,started)
                           VALUES('cabecafe12345678','task-001','worker','deferred',1)""")
        self.db.commit()
        fake = Path(self.temp.name) / "fake-openshell"
        record = Path(self.temp.name) / "deleted.txt"
        fake.write_text("""#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
if 'list' in args:
    print('gyre-001-w-deadbeef')
    print('gyre-001-w-feedface')
    print('gyre-001-w-cabecafe')
elif 'delete' in args:
    with open(os.environ['GYRE_GC_RECORD'], 'a') as out:
        out.write(args[-1] + '\\n')
""")
        fake.chmod(0o755)
        with patch.dict("os.environ", {"OPENSHELL_OIDC_CLIENT_SECRET": "test",
                                    "OPENSHELL": str(fake), "GYRE_GC_RECORD": str(record)}):
            controller.gc_sandboxes(self.db)
        self.assertCountEqual(record.read_text().splitlines(), ["gyre-001-w-deadbeef", "gyre-001-w-cabecafe"])

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
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "promoting")
        controller.promote(self.db)
        self.host_gate.assert_called_once_with(merge, "check1")
        self.assertEqual(git(self.remote, "rev-parse", "main"), merge)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "merged")
        self.assertEqual(controller.status_snapshot(self.db)["tasks"][0]["merge_sha"], merge)

    def test_host_full_suite_failure_blocks_promotion(self):
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
        self.host_gate.return_value = False
        controller.promote(self.db)
        controller.promote(self.db)
        self.host_gate.assert_called_once_with(merge, "check1")
        self.assertEqual(git(self.remote, "rev-parse", "main"), base)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "failed")

    def test_host_gate_runs_frontend_suite_and_rejects_its_failure(self):
        self.candidate()
        (self.work / "web").mkdir()
        (self.work / "web/package.json").write_text('{}\n')
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "add frontend")
        sha = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", "worker/task-001")
        controller.sync(self.db)
        fake_bin = Path(self.temp.name) / "bin"
        fake_bin.mkdir()
        for name, script in (("cargo", "#!/bin/sh\nexit 0\n"),
                             ("npm", "#!/bin/sh\n[ \"$1\" != test ]\n")):
            path = fake_bin / name
            path.write_text(script)
            path.chmod(0o755)
        with patch.dict(os.environ, {"PATH": f"{fake_bin}:{os.environ['PATH']}"}), \
             patch.object(controller, "host_test_verified", REAL_HOST_TEST):
            self.assertFalse(controller.host_test_verified(sha, "check1"))
        log = (controller.STATE / "attempts/check1/host-tests.log").read_text()
        self.assertIn("$ cargo test --all --quiet", log)
        self.assertIn("$ npm test", log)
        self.assertFalse((controller.STATE / "attempts/check1/host-tests.ok").exists())

    def test_host_gate_process_records_result_without_blocking_controller(self):
        (self.work / "web").mkdir()
        (self.work / "web/package.json").write_text('{}\n')
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "add frontend")
        git(self.work, "push", "origin", "main")
        controller.source()
        sha = git(self.work, "rev-parse", "HEAD")
        fake_bin = Path(self.temp.name) / "bin"
        fake_bin.mkdir()
        for name in ("cargo", "npm"):
            path = fake_bin / name
            path.write_text("#!/bin/sh\nexit 0\n")
            path.chmod(0o755)
        check_id = "a" * 16
        self.db.execute("INSERT INTO attempts(id,task,kind,state,started) VALUES(?, 'task-001','check','done',1)", (check_id,))
        self.db.commit()
        with patch.dict(os.environ, {"PATH": f"{fake_bin}:{os.environ['PATH']}"}), \
             patch.object(controller, "start_host_gate", REAL_START_HOST_GATE):
            controller.start_host_gate(self.db, check_id, sha)
        exit_file = controller.STATE / "attempts" / check_id / "host-tests.exit"
        deadline = time.monotonic() + 10
        while not exit_file.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertEqual(exit_file.read_text().strip(), "0")
        self.assertEqual((exit_file.parent / "host-tests.ok").read_text().strip(), sha)
        controller.reap_host_processes()

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
            controller.promote(self.db)
            with self.assertRaises(controller.SourceUnavailable):
                controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "promoting")
        self.assertEqual(self.db.execute("SELECT merge_sha FROM attempts WHERE id='check1'").fetchone()[0], merge)
        git(self.work, "push", "origin", f"{merge}:refs/heads/main")
        controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "merged")


if __name__ == "__main__":
    unittest.main()
