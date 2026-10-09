#!/usr/bin/env python3
"""Local Git integration checks for the durable controller; no cloud access."""
import importlib.util
import json
import sqlite3
import fcntl
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
REAL_ENSURE_PR = controller.ensure_pull_request


def git(cwd, *args):
    return subprocess.check_output(["git", *args], cwd=cwd, text=True, stderr=subprocess.DEVNULL).strip()


class ProviderPrerequisitesTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gyre-provider-test-")
        self.addCleanup(self.temp.cleanup)
        override = patch.object(controller, "STATE", Path(self.temp.name))
        override.start()
        self.addCleanup(override.stop)
        self.db = controller.db_open()
        self.addCleanup(self.db.close)

    def test_missing_shared_provider_blocks_dispatch_then_recovers(self):
        def run(command, **kwargs):
            return subprocess.CompletedProcess(command, 0,
                "gyre-enmaas\n" if "list" in command else "", "")
        with patch.object(controller.subprocess, "run", side_effect=run):
            self.assertFalse(controller.providers_ready(self.db))
        self.assertEqual(controller.health(self.db)["condition"], "MissingProviders: gyre-github-rw")
        self.assertEqual(controller.effective_admission(self.db, 50, 4), 4)
        with patch.object(controller.subprocess, "run", return_value=
                          subprocess.CompletedProcess([], 0, "gyre-enmaas\ngyre-github-rw\n", "")):
            self.assertTrue(controller.providers_ready(self.db))
        self.assertEqual(controller.health(self.db)["condition"], "Healthy")

    def test_provider_failure_does_not_consume_work_budget_or_require_manual_retry(self):
        self.db.execute("INSERT INTO tasks(name,progress,deps,state,attempts) VALUES('task-001','not-started','[]','failed',1)")
        self.db.execute("INSERT INTO attempts(id,task,kind,state,started,detail) VALUES('missing','task-001','worker','failed',1,'exit=75')")
        self.db.commit()
        directory = controller.STATE / "attempts/missing"
        directory.mkdir(parents=True)
        (directory / "output.log").write_text("provider 'gyre-github-rw' not found and no provider profile available")
        controller.recover_prior_infrastructure_failures(self.db)
        task = self.db.execute("SELECT state,retry_baseline FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual(tuple(task), ("deferred", 1))
        controller.recover_prior_infrastructure_failures(self.db)
        self.assertEqual(self.db.execute("SELECT retry_baseline FROM tasks").fetchone()[0], 1)


class ControllerLockTest(unittest.TestCase):
    def test_second_controller_preserves_lock_owner_pid(self):
        with tempfile.TemporaryDirectory(prefix="gyre-lock-test-") as directory:
            lock_path = Path(directory) / "controller.lock"
            with lock_path.open("w+") as lock:
                lock.write("123456\n")
                lock.flush()
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                result = subprocess.run(
                    [sys.executable, str(Path(__file__).with_name("dev-controller.py")), "run", "--once", "--slots", "0"],
                    env={**os.environ, "GYRE_DEV_STATE": directory},
                    capture_output=True, text=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("another dev controller is running (PID 123456)", result.stderr)
                self.assertEqual(lock_path.read_text(), "123456\n")

class ControllerGitTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        def stop_gateway_jobs():
            for name, process in list(controller.GATEWAY_PROCESSES.items()):
                if process.poll() is None:
                    os.killpg(process.pid, 15)
                process.wait(timeout=5)
                controller.GATEWAY_PROCESSES.pop(name)
        self.addCleanup(stop_gateway_jobs)
        self.remote = root / "remote.git"
        self.work = root / "work"
        controller.STATE = root / "state"
        controller.SOURCE = controller.STATE / "source"
        git(root, "init", "--bare", str(self.remote))
        git(root, "clone", str(self.remote), str(self.work))
        git(self.work, "config", "user.name", "Test")
        git(self.work, "config", "user.email", "test@example.com")
        git(self.work, "config", "commit.gpgsign", "false")
        git(self.work, "config", "core.hooksPath", "/dev/null")
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
        publisher = patch.object(controller, "ensure_pull_request", return_value={"url": "https://github.com/example/gyre/pull/1", "number": 1})
        ci_gate = patch.object(controller.ci, 'observe', return_value={'status': 'passed'})
        ci_gate.start()
        self.addCleanup(ci_gate.stop)
        def fixture_merge(pr, sha):
            git(controller.SOURCE, 'push', 'origin', f'{sha}:refs/heads/main')
            return sha
        merger = patch.object(controller, 'merge_pull_request', side_effect=fixture_merge)
        merger.start()
        self.addCleanup(merger.stop)
        publisher.start()
        self.addCleanup(publisher.stop)
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
                (directory / 'host-tests.result.json').write_text(json.dumps({
                    'status': 'passed', 'sha': merge_sha,
                    'artifact_policy': controller.HOST_ARTIFACT_POLICY}))
        host_start = patch.object(controller, "start_host_gate", side_effect=fake_host_gate)
        host_start.start()
        self.addCleanup(host_start.stop)

    def write_task(self, progress):
        (self.work / "specs/tasks/task-001.md").write_text(
            f"---\nprogress: {progress}\ndepends_on: []\n---\n\nImplement the task.\n")

    def test_fidelity_discovery_reserves_ids_and_preserves_open_gaps(self):
        self.write_task('complete')
        matrix = self.work / 'specs/coverage/system/example.md'
        matrix.parent.mkdir(parents=True)
        matrix.write_text('| # | Section | Depth | Status | Task | Notes |\n'
                          '| 1 | Enforcement | 2 | implemented | task-001 | claimed |\n'
                          '| 2 | Missing behavior | 2 | not-started | — | open |\n')
        spec = self.work / 'specs/system/example.md'; spec.parent.mkdir(parents=True)
        spec.write_text('# Enforcement\nReal required behavior.\n')
        git(self.work, 'add', '.')
        git(self.work, 'commit', '-m', 'coverage claims')
        git(self.work, 'push', 'origin', 'main')
        controller.sync(self.db)
        controller.plan_audits(self.db)
        controller.plan_audits(self.db)
        audit = self.db.execute('SELECT * FROM tasks WHERE audit_contract IS NOT NULL').fetchall()
        self.assertEqual(len(audit), 1)
        scoped = json.loads(Path(audit[0]['audit_contract']).read_text())
        self.assertEqual([row['id'] for row in scoped['rows']], [1, 2])
        self.assertEqual(len(set(scoped['reserved_tasks'])), 4)
        self.assertTrue(all(int(name[5:]) > int(audit[0]['name'][5:]) for name in scoped['reserved_tasks']))
        self.assertEqual(self.db.execute('SELECT count(*) FROM task_reservations').fetchone()[0], 5)
        self.assertEqual(audit[0]['state'], 'ready')
        self.assertIn('Missing behavior', git(controller.SOURCE, 'show', 'origin/main:specs/coverage/system/example.md'))
        controller.source()
        self.assertFalse(controller.stale_audit(audit[0]))
        code = self.work / 'crates/example'; code.mkdir(parents=True); (code / 'lib.rs').write_text('fn changed() {}\n')
        git(self.work, 'add', '.')
        git(self.work, 'commit', '-m', 'production changed')
        git(self.work, 'push', 'origin', 'main')
        controller.source()
        controller.plan_audits(self.db)
        self.assertEqual(self.db.execute('SELECT state FROM tasks WHERE name=?', (audit[0]['name'],)).fetchone()[0], 'superseded')
        self.assertEqual(self.db.execute("SELECT count(*) FROM tasks WHERE audit_contract IS NOT NULL AND state='ready'").fetchone()[0], 1)

    def test_baseline_failure_creates_one_scoped_task_without_spending_feature_repairs(self):
        controller.sync(self.db)
        base = git(self.work, 'rev-parse', 'main')
        log = controller.STATE / 'baseline.log'
        log.write_text(('unrelated build output ' * 20 + '  \n') * 1000 + 'Error: existing main defect  \n')
        classified = {'base': base, 'environment': 'environment', 'baseline_log': str(log)}
        name = controller.propose_baseline_repair(self.db, {'base': base}, classified)
        self.db.commit()
        self.db.execute('UPDATE tasks SET condition=NULL WHERE name=?', (name,)); self.db.commit()
        self.assertEqual(controller.propose_baseline_repair(self.db, {'base': base}, classified), name)
        self.assertEqual(self.db.execute('SELECT repairs FROM tasks WHERE name="task-001"').fetchone()[0], 0)
        proposed = self.db.execute('SELECT * FROM tasks WHERE name=?', (name,)).fetchone()
        body = Path(proposed['definition_path']).read_text()
        self.assertIn('existing main defect', body)
        self.assertLess(len(body), 10000)
        self.assertNotIn('unrelated build output', body)
        path = self.work / f'specs/tasks/{name}.md'
        path.write_text(body)
        git(self.work, 'add', str(path))
        # The former raw log copied trailing whitespace into generated code
        # and made every real integration fail its actual Git whitespace gate.
        subprocess.run(['git', 'diff', '--cached', '--check'], cwd=self.work, check=True)

    def test_baseline_diagnostics_migrate_without_reopening_reviewed_code(self):
        controller.sync(self.db)
        base = git(self.work, 'rev-parse', 'main')
        log = controller.STATE / 'baseline.log'; log.write_text('Error: main defect  \n')
        name = controller.propose_baseline_repair(self.db, {'base': base},
            {'base': base, 'environment': 'environment', 'baseline_log': str(log)})
        task = self.db.execute('SELECT * FROM tasks WHERE name=?', (name,)).fetchone()
        body = Path(task['definition_path']).read_text()
        old = controller.contract.generation(body, {}, '', include_baseline_diagnostics=True)
        new = controller.contract.generation(body, {}, '')
        self.assertNotEqual(old, new)
        self.db.execute("UPDATE tasks SET generation=?,candidate_generation=?,candidate=?,state='checking' WHERE name=?",
                        (old, old, base, name))
        self.db.execute("INSERT INTO attempts(id,task,kind,state,started,generation) VALUES('diagnostic-migration',?,'check','running',1,?)",
                        (name, old))
        controller.upgrade_baseline_generations(self.db, '')
        self.assertEqual(tuple(self.db.execute('SELECT generation,candidate_generation,candidate,state FROM tasks WHERE name=?', (name,)).fetchone()),
                         (new, new, base, 'checking'))
        self.assertEqual(self.db.execute("SELECT generation FROM attempts WHERE id='diagnostic-migration'").fetchone()[0], new)
        self.assertEqual(controller.contract.generation(body.replace('Error: main defect', 'Different diagnostic output'), {}, ''), new)
        changed = body.replace('Reproduce and repair', 'Investigate without repairing')
        self.assertNotEqual(controller.contract.generation(changed, {}, ''), new)
        # A real desired-state edit must not be silently relabeled by migration.
        Path(task['definition_path']).write_text(changed)
        self.db.execute('UPDATE tasks SET generation=? WHERE name=?', (old, name))
        controller.upgrade_baseline_generations(self.db, '')
        self.assertEqual(self.db.execute('SELECT generation FROM tasks WHERE name=?', (name,)).fetchone()[0], old)

    def test_cloud_baseline_failure_blocks_candidate_and_proposes_prerequisite(self):
        sha = self.candidate(); controller.sync(self.db)
        base = git(self.work, 'rev-parse', 'main')
        directory = controller.STATE / 'attempts/cloudbaseline'; directory.mkdir(parents=True)
        (directory / 'exit').write_text('81\n')
        evidence = {'base': base, 'environment': 'cloud', 'probe': ['bash', 'scripts/check-arch.sh'], 'log': 'existing production defect\n'}
        (directory / 'output.log').write_text('GYRE_BASELINE_FAILURE_JSON ' + json.dumps(evidence) + '\n')
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,base,state,started) VALUES('cloudbaseline','task-001','check',?,?,'running',1)", (sha, base))
        self.db.execute("UPDATE tasks SET state='checking' WHERE name='task-001'"); self.db.commit()
        controller.reap(self.db)
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual((task['state'], task['repairs'], task['candidate']), ('blocked', 0, sha))
        self.assertEqual(self.db.execute("SELECT count(*) FROM tasks WHERE origin_key LIKE 'baseline:%'").fetchone()[0], 1)

    def test_backlog_limits_new_workers_but_keeps_integration_admissible(self):
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET state='candidate',candidate='missing' WHERE name='task-001'")
        self.db.execute("INSERT INTO tasks(name,progress,deps,state) VALUES('task-002','not-started','[]','ready')")
        self.db.execute('UPDATE controller_health SET admission=5'); self.db.commit()
        with patch.dict(os.environ, {'GYRE_DEV_MAX_CANDIDATES': '1'}), patch.object(controller, 'ref_exists', return_value=True), patch.object(controller, 'spawn') as dispatch:
            controller.schedule(self.db, 5, 3)
            self.assertEqual([call.args[2] for call in dispatch.call_args_list], ['check'])

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

    def test_every_multiline_dependency_blocks_dispatch_until_merged(self):
        task = self.work / "specs/tasks/task-001.md"
        task.write_text("---\nprogress: not-started\ndepends_on:\n  - task-002\n  - task-003\n---\nImplement after both prerequisites.\n")
        for name, progress in (("task-002", "complete"), ("task-003", "not-started")):
            (self.work / "specs/tasks" / f"{name}.md").write_text(f"---\nprogress: {progress}\ndepends_on: []\n---\nPrerequisite.\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "declare prerequisites")
        git(self.work, "push", "origin", "main")
        controller.sync(self.db)
        import json
        self.assertEqual(json.loads(self.db.execute("SELECT deps FROM tasks WHERE name='task-001'").fetchone()[0]), ["task-002", "task-003"])
        self.db.commit()
        with patch.object(controller, "spawn") as dispatch:
            controller.schedule(self.db, 1, 3, only_task="task-001")
            dispatch.assert_not_called()
            self.db.execute("UPDATE tasks SET state='merged' WHERE name='task-003'")
            self.db.commit()
            controller.schedule(self.db, 1, 3, only_task="task-001")
            dispatch.assert_called_once()
        self.assertEqual(controller.deps("---\ndepends_on: [task-002, task-003]\n---\n"), ["task-002", "task-003"])
        self.assertEqual(controller.deps("---\ndepends_on: []\nprogress: not-started\n---\n"), [])

    def test_launch_burst_ramps_sandbox_creation(self):
        controller.sync(self.db)
        for name in ("task-002", "task-003"):
            self.db.execute("INSERT INTO tasks(name,progress,deps,state,attempts) VALUES(?, 'not-started', '[]', 'ready', 0)",
                            (name,))
        self.db.commit()
        self.db.execute("UPDATE controller_health SET admission=3 WHERE id=1")
        with patch.object(controller, "spawn") as spawn:
            controller.schedule(self.db, slots=50, max_attempts=3, launch_burst=2)
        self.assertEqual([call.args[1]["name"] for call in spawn.call_args_list], ["task-001", "task-002"])

    def test_source_observation_does_not_hold_sqlite_writer_lock(self):
        (self.work / 'specs/tasks/task-002.md').write_text('---\ntitle: second task\nprogress: not-started\ndepends_on: []\ncommits: []\n---\nRequired behavior\n')
        git(self.work, 'add', '.')
        git(self.work, 'commit', '-m', 'second task')
        git(self.work, 'push', 'origin', 'main')
        actual_body = controller.task_body
        observations = []
        def body(ref, name):
            observations.append(ref)
            if name == 'task-002':
                with sqlite3.connect(controller.STATE / 'state.sqlite3', timeout=.1) as other:
                    other.execute("INSERT INTO events(at,task,message) VALUES(1,NULL,'concurrent control write')")
            return actual_body(ref, name)
        with patch.object(controller, 'task_body', side_effect=body):
            controller.sync(self.db)
        self.assertEqual(len(set(observations)), 1)
        self.assertRegex(observations[0], r'^[0-9a-f]{40}$')
        self.assertEqual(self.db.execute("SELECT count(*) FROM events WHERE message='concurrent control write'").fetchone()[0], 1)

    def test_capacity_recovery_worker_probe_can_use_reserved_slot(self):
        (self.work / 'specs/tasks/task-002.md').write_text('---\ntitle: probe\nprogress: not-started\ndepends_on: []\ncommits: []\n---\nRequired behavior\n')
        git(self.work, 'add', '.')
        git(self.work, 'commit', '-m', 'probe task')
        git(self.work, 'push', 'origin', 'main')
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET state='running' WHERE name='task-001'")
        self.db.execute("INSERT INTO attempts(id,task,kind,state,started) VALUES('active','task-001','worker','running',0)")
        self.db.execute("UPDATE controller_health SET failures=1,retry_at=1,admission=1,condition='Capacity unavailable'")
        self.db.commit()
        with patch.object(controller, 'spawn') as spawn:
            controller.schedule(self.db, slots=50, max_attempts=3, launch_burst=4)
        self.assertEqual([call.args[1]['name'] for call in spawn.call_args_list], ['task-002'])

    def test_database_initialization_and_idle_promotion_release_writer(self):
        for phase in ('initialized', 'idle promotion'):
            with self.subTest(phase=phase):
                if phase == 'idle promotion':
                    controller.sync(self.db)
                    controller.promote(self.db)
                with sqlite3.connect(controller.STATE / 'state.sqlite3', timeout=.1) as other:
                    other.execute('INSERT INTO events(at,task,message) VALUES(1,NULL,?)', (phase,))

    def test_shared_source_refresh_waits_for_other_process_lock(self):
        script = Path(__file__).with_name('dev-controller.py').resolve()
        code = ('import importlib.util; '
                f's=importlib.util.spec_from_file_location("controller", {str(script)!r}); '
                'm=importlib.util.module_from_spec(s); s.loader.exec_module(m); '
                'print("starting", flush=True); m.source(); print("finished", flush=True)')
        with (controller.STATE / 'source.lock').open('a+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            child = subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     text=True, env={**os.environ, 'GYRE_DEV_ROOT': str(self.work),
                                                    'GYRE_DEV_STATE': str(controller.STATE)})
            self.addCleanup(lambda: child.kill() if child.poll() is None else None)
            self.assertEqual(child.stdout.readline().strip(), 'starting')
            with self.assertRaises(subprocess.TimeoutExpired):
                child.wait(timeout=.1)
            fcntl.flock(lock, fcntl.LOCK_UN)
        output, errors = child.communicate(timeout=10)
        self.assertEqual(child.returncode, 0, errors)
        self.assertIn('finished', output)
        self.assertTrue((controller.SOURCE / '.git').exists())

    def test_main_repair_blocks_integration_but_allows_independent_workers(self):
        sha = self.candidate()
        controller.sync(self.db)
        self.db.execute("INSERT INTO tasks(name,progress,deps,state,origin_key) VALUES('task-002','needs-revision','[]','ready','baseline:broken:env')")
        self.db.execute("INSERT INTO tasks(name,progress,deps,state) VALUES('task-003','not-started','[]','ready')")
        self.db.execute("INSERT INTO tasks(name,progress,deps,state,blocked_base) VALUES('task-004','not-started','[]','blocked','broken')")
        self.db.execute('UPDATE controller_health SET admission=5')
        self.db.commit()
        with patch.object(controller, 'spawn') as dispatch:
            controller.schedule(self.db, 5, 3)
        self.assertEqual([(call.args[1]['name'], call.args[2]) for call in dispatch.call_args_list],
                         [('task-002', 'worker'), ('task-003', 'worker')])
        self.db.execute("UPDATE tasks SET state='candidate',candidate=? WHERE name='task-002'", (sha,))
        self.db.commit()
        with patch.object(controller, 'spawn') as dispatch:
            controller.schedule(self.db, 5, 3)
        self.assertIn(('task-002', 'check'), [(call.args[1]['name'], call.args[2]) for call in dispatch.call_args_list])
        self.assertNotIn(('task-001', 'check'), [(call.args[1]['name'], call.args[2]) for call in dispatch.call_args_list])

    def test_inference_failure_retains_checkpoint_without_throttling_gateway(self):
        sha = self.candidate()
        git(self.work, 'push', 'origin', 'HEAD:devloop/task-001/attempt-1')
        controller.sync(self.db)
        directory = controller.STATE / 'attempts/inference'
        directory.mkdir(parents=True)
        (directory / 'exit').write_text('82\n')
        self.db.execute("""INSERT INTO attempts(id,task,kind,branch,state,pid,started)
                           VALUES('inference','task-001','worker','devloop/task-001/attempt-1','running',999999,1)""")
        self.db.execute("UPDATE tasks SET state='running',attempts=1,seed=NULL WHERE name='task-001'")
        self.db.execute('UPDATE controller_health SET admission=50')
        self.db.commit()
        with patch.object(controller.random, 'uniform', return_value=1), patch.object(controller.time, 'time', return_value=1000):
            controller.reap(self.db)
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual((task['state'], task['condition'], task['retry_at'], task['retry_baseline'], task['seed']),
                         ('deferred', 'InferenceUnavailable', 1030, 1, sha))
        self.assertEqual(controller.health(self.db)['admission'], 50)
        self.assertEqual(task['repairs'], 0)
        with patch.object(controller, 'spawn') as dispatch, patch.object(controller.time, 'time', return_value=1029):
            controller.schedule(self.db, 50, 3)
            dispatch.assert_not_called()

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
        bundle = Path(popen.call_args.kwargs["env"]["GYRE_DEV_ROOT"])
        self.assertEqual(bundle, snapshot.parent / "bundle")
        self.assertEqual((bundle / "specs/prompts/dev-implementation.md").read_bytes(),
                         (controller.ROOT / "specs/prompts/dev-implementation.md").read_bytes())
        self.assertTrue((snapshot.parent / "bundle.json").exists())

    def test_launch_intent_is_durable_before_child_can_allocate_resources(self):
        controller.sync(self.db)
        task = self.db.execute("SELECT * FROM tasks").fetchone()
        def launching(*args, **kwargs):
            separate = controller.db_open()
            try:
                row = separate.execute("SELECT state,bundle_sha,generation FROM attempts").fetchone()
                self.assertEqual(row["state"], "launching")
                self.assertEqual(len(row["bundle_sha"]), 64)
                self.assertEqual(row["generation"], task["generation"])
            finally:
                separate.close()
            from unittest.mock import Mock
            return Mock(pid=12345)
        with patch.object(controller.subprocess, "Popen", side_effect=launching):
            controller.spawn(self.db, task, "worker", branch="devloop/task-001/attempt-1")

    def test_remote_deletion_debt_prevents_replacement_sandbox(self):
        controller.sync(self.db)
        self.db.execute("INSERT INTO resources(name,present,delete_pending) VALUES('owned-orphan',1,1)")
        self.db.commit()
        with patch.object(controller, "spawn") as dispatch:
            controller.schedule(self.db, 1, 3)
        dispatch.assert_not_called()

    def test_spec_change_reopens_completed_task_without_trusting_old_completion(self):
        (self.work / "specs/system").mkdir()
        spec = self.work / "specs/system/example.md"
        spec.write_text("# Example\nReject invalid input.\n")
        task = self.work / "specs/tasks/task-001.md"
        task.write_text('---\ntitle: "Example"\nspec_ref: "example.md §1"\nprogress: complete\ndepends_on: []\n---\nImplement real input validation.\n')
        git(self.work, "add", "."); git(self.work, "commit", "-m", "existing implementation contract")
        git(self.work, "push", "origin", "main")
        controller.sync(self.db)
        original = self.db.execute("SELECT * FROM tasks").fetchone()
        self.assertEqual(original["state"], "merged")
        spec.write_text("# Example\nReject invalid input and persist rejection reasons.\n")
        git(self.work, "add", "."); git(self.work, "commit", "-m", "change spec requirement")
        git(self.work, "push", "origin", "main")
        controller.sync(self.db)
        row = self.db.execute("SELECT * FROM tasks").fetchone()
        self.assertEqual((row["state"], row["condition"]), ("ready", "SpecChanged"))
        self.assertNotEqual(row["generation"], original["observed_generation"])
        controller.sync(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks").fetchone()[0], "ready")
        with patch.object(controller, "spawn") as dispatch:
            controller.schedule(self.db, 1, 3)
        self.assertEqual(dispatch.call_args.args[2], "worker")

    def test_generations_ignore_completion_claims_and_shipped_notes(self):
        body = '---\ntitle: "Example"\nspec_ref: "example.md §1"\nprogress: not-started\ncommits: []\n---\nImplement validation.\n'
        completed = body.replace("not-started", "complete").replace("commits: []", "commits: [deadbeef]") + "\n## Shipped\n\n- Input validation.\n"
        self.assertEqual(controller.contract.generation(body, {}), controller.contract.generation(completed, {}))

    def test_cycles_have_explicit_diagnostics(self):
        errors = controller.contract.graph_errors({"task-001": ["task-002"], "task-002": ["task-001"], "task-003": ["task-999"]})
        self.assertIn("Dependency cycle", errors["task-001"])
        self.assertIn("task-999", errors["task-003"])

    def test_process_identity_rejects_a_reused_pid(self):
        identity = Path(self.temp.name) / "process.json"
        identity.write_text(json.dumps({"pid": os.getpid(), "start": "impossible-start", "boot": "wrong-boot"}))
        self.assertTrue(controller.alive(os.getpid()))
        self.assertFalse(controller.alive(os.getpid(), identity))

    def test_pr_publication_is_idempotent_and_confirms_exact_head(self):
        sha = self.candidate(); controller.sync(self.db)
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,base,state,started) VALUES('publish','task-001','check',?,?,'done',1)", (sha, sha))
        self.db.commit()
        (controller.STATE / "attempts/publish").mkdir(parents=True)
        task = self.db.execute("SELECT * FROM tasks").fetchone()
        check = self.db.execute("SELECT * FROM attempts").fetchone()
        pr = {"url": "https://github.com/example/gyre/pull/42", "number": 42, "state": "OPEN", "headRefOid": sha, "baseRefName": "main"}
        calls, created = [], []
        real_run = controller.run
        def github(*args, **kwargs):
            if args[0] != "gh":
                return real_run(*args, **kwargs)
            calls.append(args)
            if args[2] == "list":
                output = json.dumps([pr] if created else [])
            elif args[2] == "create":
                created.append(True); output = pr["url"]
            else:
                output = json.dumps(pr)
            return subprocess.CompletedProcess(args, 0, output, "")
        with patch.dict(os.environ, {"GYRE_DEV_GITHUB_REPO": "example/gyre"}), patch.object(controller, "run", side_effect=github):
            for _ in range(2):
                self.assertEqual(REAL_ENSURE_PR(self.db, task, check, sha)["url"], pr["url"])
            pr["headRefOid"] = "0" * 40
            with self.assertRaisesRegex(controller.SourceUnavailable, "verified head"):
                REAL_ENSURE_PR(self.db, task, check, sha)
        self.assertEqual(sum(call[2] == "create" for call in calls), 1)
        self.assertEqual(self.db.execute("SELECT pr_url FROM tasks").fetchone()[0], "https://github.com/example/gyre/pull/42")
        row = self.db.execute('SELECT condition,retry_at,repairs FROM tasks').fetchone()
        self.assertEqual((row['condition'], row['repairs']), ('PublicationUnavailable', 0))
        self.assertGreater(row['retry_at'], time.time())

    def test_pr_mode_publishes_verified_commit_without_pushing_main(self):
        sha = self.candidate(); controller.sync(self.db)
        base = git(self.work, 'rev-parse', 'main')
        git(self.work, 'checkout', 'main'); git(self.work, 'merge', '--no-ff', '--no-edit', 'worker/task-001')
        merge = git(self.work, 'rev-parse', 'HEAD')
        git(self.work, 'push', 'origin', f'{merge}:refs/heads/devloop/verified/check1')
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,base,state,started) VALUES('check1','task-001','check',?,?,'done',1)", (sha, base))
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'"); self.db.commit()
        with patch.object(controller, 'PUBLICATION_MODE', 'pr'):
            controller.promote(self.db); controller.promote(self.db)
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), base)
        self.assertEqual(self.db.execute('SELECT state FROM tasks').fetchone()[0], 'published')
        controller.sync(self.db)
        self.assertEqual(self.db.execute('SELECT state FROM tasks').fetchone()[0], 'published')
        self.db.execute('UPDATE tasks SET retry_at=0'); self.db.commit()
        controller.promote(self.db)
        self.assertEqual(self.db.execute('SELECT state FROM tasks').fetchone()[0], 'merged')
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), merge)

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
import json, os, sys
args = sys.argv[1:]
if 'list' in args:
    print(json.dumps({'sandboxes': [{'name': name, 'labels': {}} for name in
          ('gyre-001-w-deadbeef','gyre-001-w-feedface','gyre-001-w-cabecafe')]}))
elif 'delete' in args:
    with open(os.environ['GYRE_GC_RECORD'], 'a') as out:
        out.write(args[-1] + '\\n')
""")
        fake.chmod(0o755)
        with patch.dict("os.environ", {"OPENSHELL_OIDC_CLIENT_SECRET": "test",
                                    "OPENSHELL": str(fake), "GYRE_GC_RECORD": str(record)}):
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                controller.gc_sandboxes(self.db)
                if record.exists() and len(record.read_text().splitlines()) == 2:
                    break
                time.sleep(0.05)
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

    def test_old_inflight_check_cannot_promote_with_an_unmerged_dependency(self):
        sha = self.candidate()
        controller.sync(self.db)
        base = git(self.work, "rev-parse", "main")
        self.db.execute("INSERT INTO tasks(name,progress,deps,state) VALUES('task-002','not-started','[]','ready')")
        self.db.execute("UPDATE tasks SET state='promoting',deps='[\"task-002\"]' WHERE name='task-001'")
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,base,state,started) VALUES('old-check','task-001','check',?,?,'done',1)", (sha, base))
        self.db.commit()
        controller.promote(self.db)
        self.assertEqual(git(self.remote, "rev-parse", "main"), base)
        self.assertEqual(tuple(self.db.execute("SELECT state,condition FROM tasks WHERE name='task-001'").fetchone()),
                         ("candidate", "DependenciesPending"))
        self.host_gate.assert_not_called()

    def verified_pr_fixture(self):
        sha = self.candidate()
        controller.sync(self.db)
        base = git(self.work, 'rev-parse', 'main')
        git(self.work, 'checkout', 'main')
        git(self.work, 'merge', '--no-ff', '--no-edit', 'worker/task-001')
        merged = git(self.work, 'rev-parse', 'HEAD')
        git(self.work, 'push', 'origin', f'{merged}:refs/heads/devloop/verified/ci-check')
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,base,merge_sha,state,started) VALUES('ci-check','task-001','check',?,?,?,'done',1)", (sha, base, merged))
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'")
        self.db.commit()
        controller.promote(self.db)  # host verification completes in fixture
        return base, sha, merged

    def test_github_pending_checks_prevent_upstream_merge_then_resume(self):
        base, sha, merged = self.verified_pr_fixture()
        with patch.object(controller.ci, 'observe', return_value={'status': 'pending'}):
            controller.promote(self.db)
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), base)
        self.assertEqual(tuple(self.db.execute('SELECT state,condition FROM tasks').fetchone()), ('published', 'GitHubChecksPending'))
        self.db.execute('UPDATE tasks SET retry_at=0'); self.db.commit()
        controller.promote(self.db)
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), merged)

    def test_failed_pr_checks_feed_implementation_without_publishing(self):
        base, sha, merged = self.verified_pr_fixture()
        log = controller.STATE / 'attempts/ci-check/github-checks.log'
        log.write_text('actual required CI assertion: wrong payload accepted')
        with patch.object(controller.ci, 'observe', return_value={'status': 'candidate_failed'}):
            controller.promote(self.db)
        task = self.db.execute('SELECT * FROM tasks').fetchone()
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), base)
        self.assertEqual((task['state'], task['seed'], task['repairs']), ('ready', sha, 1))
        self.assertIn('wrong payload accepted', Path(task['feedback']).read_text())

    def test_github_outage_preserves_checked_tree_without_code_repair(self):
        base, sha, merged = self.verified_pr_fixture()
        with patch.object(controller.ci, 'observe', side_effect=RuntimeError('API unavailable')):
            controller.promote(self.db)
        task = self.db.execute('SELECT * FROM tasks').fetchone()
        self.assertEqual((task['candidate'], task['repairs'], task['condition']), (sha, 0, 'GitHubChecksUnavailable'))
        self.assertGreater(task['retry_at'], time.time())
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), base)

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
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "ready")
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual(task["seed"], sha)
        self.assertIsNone(task["candidate"])
        self.assertEqual(task["condition"], "RepairPending")

    def test_host_environment_outage_retries_local_gate_without_new_sandbox(self):
        sha = self.candidate(); controller.sync(self.db)
        base = git(self.work, 'rev-parse', 'main')
        git(self.work, 'checkout', 'main'); git(self.work, 'merge', '--no-ff', '--no-edit', 'worker/task-001')
        merge = git(self.work, 'rev-parse', 'HEAD')
        git(self.work, 'push', 'origin', f'{merge}:refs/heads/devloop/verified/check1')
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,base,state,started,host_pid) VALUES('check1','task-001','check',?,?,'done',1,999999)", (sha, base))
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'"); self.db.commit()
        directory = controller.STATE / 'attempts/check1'; directory.mkdir(parents=True)
        (directory / 'host-tests.exit').write_text('1\n')
        (directory / 'host-tests.result.json').write_text(json.dumps({'status': 'unavailable'}))
        controller.promote(self.db)
        row = self.db.execute('SELECT * FROM tasks').fetchone()
        self.assertEqual((row['state'], row['repairs'], row['condition']), ('promoting', 0, 'HostEnvironmentUnavailable'))
        self.assertGreater(row['retry_at'], time.time())
        self.assertFalse((directory / 'host-tests.exit').exists())
        with patch.object(controller, 'spawn') as cloud:
            controller.promote(self.db)
            cloud.assert_not_called()
        with patch.object(controller.time, 'time', return_value=row['retry_at'] + 1):
            controller.promote(self.db)
            controller.promote(self.db)
        self.assertEqual(git(self.remote, 'rev-parse', 'main'), merge)

    def test_failed_check_repairs_a_new_candidate_then_pushes_exact_merge(self):
        rejected = self.candidate()
        controller.sync(self.db)
        base = git(self.work, "rev-parse", "main")
        directory = controller.STATE / "attempts" / "rejected"
        directory.mkdir(parents=True)
        (directory / "exit").write_text("1\n")
        (directory / "output.log").write_text("error: production behavior rejects valid inputs\n")
        self.db.execute("""INSERT INTO attempts(id,task,kind,sha,base,state,pid,started)
                           VALUES('rejected','task-001','check',?,?,'running',999999,1)""", (rejected, base))
        self.db.execute("UPDATE tasks SET state='checking',attempts=3 WHERE name='task-001'")
        self.db.commit()
        controller.reap(self.db)
        # Restart and sync must retain the repair, rather than reimport the
        # completed legacy candidate and check it unchanged.
        controller.sync(self.db)
        task = self.db.execute("SELECT * FROM tasks WHERE name='task-001'").fetchone()
        self.assertEqual((task["state"], task["candidate"], task["seed"]), ("ready", None, rejected))
        self.assertIn("rejects valid inputs", Path(task["feedback"]).read_text())
        with patch.object(controller, "spawn") as dispatch:
            controller.schedule(self.db, 1, 3)
        self.assertEqual(dispatch.call_args.args[2], "worker")
        self.assertEqual(dispatch.call_args.args[1]["feedback"], task["feedback"])

        git(self.work, "checkout", "-b", "devloop/task-001/attempt-4")
        (self.work / "implementation.txt").write_text("repaired production behavior\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "fix(task-001): handle valid inputs")
        repaired = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", "devloop/task-001/attempt-4")
        worker = controller.STATE / "attempts" / "repaired"
        worker.mkdir(parents=True)
        (worker / "exit").write_text("0\n")
        self.db.execute("""INSERT INTO attempts(id,task,kind,branch,state,pid,started)
                           VALUES('repaired','task-001','worker','devloop/task-001/attempt-4','running',999999,2)""")
        self.db.execute("UPDATE tasks SET state='running' WHERE name='task-001'")
        self.db.commit()
        controller.reap(self.db)
        self.assertEqual(self.db.execute("SELECT candidate FROM tasks").fetchone()[0], repaired)
        git(self.work, "checkout", "main")
        git(self.work, "merge", "--no-ff", "--no-edit", repaired)
        merge = git(self.work, "rev-parse", "HEAD")
        git(self.work, "push", "origin", f"{merge}:refs/heads/devloop/verified/passed")
        checked = controller.STATE / "attempts" / "passed"
        checked.mkdir(parents=True)
        (checked / "exit").write_text("0\n")
        self.db.execute("""INSERT INTO attempts(id,task,kind,sha,base,state,pid,started)
                           VALUES('passed','task-001','check',?,?,'running',999999,3)""", (repaired, base))
        self.db.execute("UPDATE tasks SET state='checking' WHERE name='task-001'")
        self.db.commit()
        controller.reap(self.db)
        controller.promote(self.db)
        controller.promote(self.db)
        self.assertEqual(git(self.remote, "rev-parse", "main"), merge)
        self.assertEqual(git(self.remote, "show", "main:implementation.txt"), "repaired production behavior")
        self.assertEqual(controller.status_snapshot(self.db)["confirmed_merges"], 1)

    def test_repair_limit_requires_attention_and_explicit_retry_keeps_findings(self):
        sha = self.candidate()
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET repairs=? WHERE name='task-001'", (controller.MAX_REPAIRS,))
        self.db.execute("""INSERT INTO attempts(id,task,kind,sha,base,state,started,detail)
                           VALUES('limit','task-001','check',?,?,'failed',1,'exit=1')""", (sha, sha))
        self.db.commit()
        task = self.db.execute("SELECT * FROM tasks").fetchone()
        attempt = self.db.execute("SELECT * FROM attempts").fetchone()
        controller.queue_repair(self.db, task, attempt)
        task = self.db.execute("SELECT * FROM tasks").fetchone()
        self.assertEqual((task["state"], task["condition"]), ("failed", "RepairLimitExceeded"))
        controller.retry_failed_task(self.db, task)
        task = self.db.execute("SELECT * FROM tasks").fetchone()
        self.assertEqual((task["state"], task["candidate"], task["repairs"]), ("ready", None, 0))
        self.assertTrue(Path(task["feedback"]).exists())

    def test_source_outage_does_not_consume_finished_worker_outcome(self):
        controller.sync(self.db)
        directory = controller.STATE / "attempts" / "offline"
        directory.mkdir(parents=True)
        (directory / "exit").write_text("0\n")
        self.db.execute("INSERT INTO attempts(id,task,kind,state,pid,started) VALUES('offline','task-001','worker','running',999999,1)")
        self.db.execute("UPDATE tasks SET state='running' WHERE name='task-001'")
        self.db.commit()
        with patch.object(controller, "source", side_effect=controller.SourceUnavailable("offline")):
            with self.assertRaises(controller.SourceUnavailable):
                controller.reap(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM attempts").fetchone()[0], "running")
        self.assertEqual(self.db.execute("SELECT state FROM tasks").fetchone()[0], "running")

    def test_candidates_take_priority_and_only_one_checker_is_dispatched(self):
        sha = self.candidate()
        controller.sync(self.db)
        self.db.execute("INSERT INTO tasks(name,progress,deps,state,candidate) VALUES('task-002','not-started','[]','candidate',?)", (sha,))
        self.db.execute("INSERT INTO tasks(name,progress,deps,state) VALUES('task-003','not-started','[]','ready')")
        self.db.execute("UPDATE controller_health SET admission=50 WHERE id=1")
        self.db.commit()
        with patch.object(controller, "spawn") as dispatch:
            controller.schedule(self.db, 50, 3)
        self.assertEqual([call.args[2] for call in dispatch.call_args_list], ["check", "worker"])
        self.assertEqual(dispatch.call_args_list[0].args[1]["name"], "task-001")

    def test_running_host_gate_reserves_capacity_for_next_integration(self):
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET state='promoting' WHERE name='task-001'")
        for number in range(2, 5):
            self.db.execute("INSERT INTO tasks(name,progress,deps,state) VALUES(?,'not-started','[]','ready')", (f"task-{number:03}",))
        self.db.execute("UPDATE controller_health SET admission=3 WHERE id=1")
        self.db.commit()
        with patch.object(controller, "spawn") as dispatch:
            controller.schedule(self.db, 3, 3)
        self.assertEqual(len(dispatch.call_args_list), 2)

    def test_prior_quality_failure_migrates_once_to_implementation(self):
        sha = self.candidate()
        controller.sync(self.db)
        self.db.execute("UPDATE tasks SET state='failed' WHERE name='task-001'")
        self.db.execute("INSERT INTO attempts(id,task,kind,sha,state,started,detail) VALUES('old','task-001','check',?,'failed',1,'exit=1')", (sha,))
        self.db.commit()
        controller.recover_prior_quality_failures(self.db)
        controller.recover_prior_quality_failures(self.db)
        self.assertEqual(tuple(self.db.execute("SELECT state,repairs,seed FROM tasks").fetchone()), ("ready", 1, sha))

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
        for name, script in (("cargo", "#!/bin/sh\n[ \"$1\" = metadata ] && echo '{\"workspace_members\":[\"fixture\"],\"packages\":[{\"id\":\"fixture\"}]}'\nexit 0\n"),
                             ("npm", "#!/bin/sh\n[ \"$1\" != test ]\n")):
            path = fake_bin / name
            path.write_text(script)
            path.chmod(0o755)
        with patch.dict(os.environ, {"PATH": f"{fake_bin}:{os.environ['PATH']}"}), \
             patch.object(controller, "host_test_verified", REAL_HOST_TEST):
            old = controller.STATE / 'attempts/check1'
            old.mkdir(parents=True, exist_ok=True)
            (old / 'host-tests.ok').write_text(sha + '\n')
            (old / 'host-tests.result.json').write_text(json.dumps({'status': 'passed', 'sha': sha}))
            self.assertFalse(controller.host_test_verified(sha, "check1"))
        log = (controller.STATE / "attempts/check1/host-tests.log").read_text()
        self.assertIn("$ cargo test --all --quiet", log)
        self.assertIn("$ npm test", log)
        self.assertFalse((controller.STATE / "attempts/check1/host-tests.ok").exists())

    def test_host_gate_distinguishes_main_failure_from_candidate_regression(self):
        (self.work / 'web').mkdir(); (self.work / 'web/package.json').write_text('{}\n')
        marker = self.work / 'broken.txt'; marker.write_text('preexisting\n')
        git(self.work, 'add', '.'); git(self.work, 'commit', '-m', 'broken baseline')
        git(self.work, 'push', 'origin', 'main')
        base = git(self.work, 'rev-parse', 'HEAD')
        sha = self.candidate()
        controller.sync(self.db)
        fake_bin = Path(self.temp.name) / 'bin'; fake_bin.mkdir()
        for name, script in [('cargo', '#!/bin/sh\n[ "$1" = metadata ] && { echo \'{"workspace_members":["fixture"],"packages":[{"id":"fixture"}]}\'; exit 0; }\n[ "$1" = clean ] && exit 0\n[ "$1" = --version ] && { echo cargo-version; exit 0; }\n[ ! -f broken.txt ]\n'),
                             ('npm', '#!/bin/sh\nexit 0\n')]:
            path = fake_bin / name; path.write_text(script); path.chmod(0o755)
        with patch.dict(os.environ, {'PATH': str(fake_bin) + ':' + os.environ['PATH']}), patch.object(controller, 'host_test_verified', REAL_HOST_TEST):
            for check_id, expected in [('baselinecheck', 'baseline_failed'), ('regressioncheck', 'candidate_failed')]:
                if expected == 'candidate_failed':
                    git(self.work, 'checkout', 'main'); marker.unlink()
                    git(self.work, 'add', '.'); git(self.work, 'commit', '-m', 'repair main baseline')
                    git(self.work, 'push', 'origin', 'main'); base = git(self.work, 'rev-parse', 'HEAD')
                    controller.source()
                self.db.execute("INSERT INTO attempts(id,task,kind,state,started,base) VALUES(?,'task-001','check','done',1,?)", (check_id, base)); self.db.commit()
                self.assertFalse(controller.host_test_verified(sha, check_id))
                result = json.loads((controller.STATE / 'attempts' / check_id / 'host-tests.result.json').read_text())
                self.assertEqual(result['status'], expected)
                self.assertEqual(result['base'], base)

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
            path.write_text("#!/bin/sh\n[ \"$1\" = metadata ] && echo '{\"workspace_members\":[\"fixture\"],\"packages\":[{\"id\":\"fixture\"}]}'\nexit 0\n" if name == 'cargo' else "#!/bin/sh\nexit 0\n")
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

    def test_candidate_metadata_failure_is_classified_as_code_failure(self):
        (self.work / 'web').mkdir()
        (self.work / 'web/package.json').write_text('{}\n')
        git(self.work, 'add', '.')
        git(self.work, 'commit', '-m', 'baseline with frontend')
        git(self.work, 'push', 'origin', 'main')
        base = git(self.work, 'rev-parse', 'HEAD')
        (self.work / 'bad-manifest').write_text('candidate manifest defect\n')
        sha = self.candidate()
        controller.sync(self.db)
        self.db.execute("INSERT INTO attempts(id,task,kind,state,started,base) VALUES('metadata','task-001','check','done',1,?)", (base,))
        self.db.commit()
        fake_bin = Path(self.temp.name) / 'bin'
        fake_bin.mkdir()
        scripts = {'cargo': '''#!/bin/sh
if [ "$1" = metadata ]; then
  [ -f bad-manifest ] && { echo 'error: invalid candidate manifest' >&2; exit 101; }
  echo '{"workspace_members":["fixture"],"packages":[{"id":"fixture"}]}'
fi
exit 0
''', 'npm': '#!/bin/sh\nexit 0\n'}
        for name, body in scripts.items():
            path = fake_bin / name
            path.write_text(body)
            path.chmod(0o755)
        with patch.dict(os.environ, {'PATH': str(fake_bin) + ':' + os.environ['PATH']}), \
             patch.object(controller, 'host_test_verified', REAL_HOST_TEST):
            self.assertFalse(controller.host_test_verified(sha, 'metadata'))
        result = json.loads((controller.STATE / 'attempts/metadata/host-tests.result.json').read_text())
        self.assertEqual(result['status'], 'candidate_failed')
        self.assertIn('invalid candidate manifest', (controller.STATE / 'attempts/metadata/host-tests.log').read_text())

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
        with patch.object(controller, "merge_pull_request", side_effect=subprocess.TimeoutExpired('gh pr merge', 30)):
            controller.promote(self.db)
            controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "promoting")
        self.assertEqual(self.db.execute("SELECT merge_sha FROM attempts WHERE id='check1'").fetchone()[0], merge)
        git(self.work, "push", "origin", f"{merge}:refs/heads/main")
        controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], 'promoting')
        retry_at = self.db.execute("SELECT retry_at FROM tasks WHERE name='task-001'").fetchone()[0]
        with patch.object(controller.time, 'time', return_value=retry_at + 1):
            controller.promote(self.db)
        self.assertEqual(self.db.execute("SELECT state FROM tasks WHERE name='task-001'").fetchone()[0], "merged")


if __name__ == "__main__":
    unittest.main()
