#!/usr/bin/env python3
"""Verify a task summary becomes the message on the exact two-parent merge."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("dev-merge-message.py")


def git(cwd, *args):
    return subprocess.check_output(["git", *args], cwd=cwd, text=True).strip()


class MergeMessageTest(unittest.TestCase):
    def test_verified_merge_describes_shipped_behavior(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "-q")
            git(root, "config", "user.name", "Test")
            git(root, "config", "user.email", "test@example.com")
            git(root, "config", "commit.gpgsign", "false")
            hooks = root / "empty-hooks"
            hooks.mkdir()
            git(root, "config", "core.hooksPath", str(hooks))
            task = root / "specs/tasks/task-001.md"
            task.parent.mkdir(parents=True)
            task.write_text('---\ntitle: "Add durable notifications"\nspec_ref: "messages.md §3"\n---\n')
            git(root, "add", ".")
            git(root, "commit", "-qm", "base")
            base = git(root, "rev-parse", "HEAD")
            git(root, "checkout", "-qb", "candidate")
            task.write_text(task.read_text() + "\n## Shipped\n\n- Notifications persist across restarts.\n- Expired entries are removed.\n")
            (root / "implementation.txt").write_text("implementation\n")
            git(root, "add", ".")
            git(root, "commit", "-qm", "implement")
            candidate = git(root, "rev-parse", "HEAD")
            git(root, "checkout", "-q", "--detach", base)
            git(root, "merge", "--no-ff", "--no-commit", candidate)
            message = subprocess.check_output([sys.executable, str(SCRIPT), "task-001", candidate], cwd=root)
            (root / "merge-message.txt").write_bytes(message)
            git(root, "commit", "-qF", "merge-message.txt")
            self.assertEqual(git(root, "rev-list", "--parents", "-n", "1", "HEAD").split()[1:], [base, candidate])
            body = git(root, "show", "-s", "--format=%B", "HEAD")
            self.assertIn("Ship task-001: Add durable notifications", body)
            self.assertIn("Spec: messages.md §3", body)
            self.assertIn("- Notifications persist across restarts.", body)
            self.assertNotIn("Implementation Plan", body)


if __name__ == "__main__":
    unittest.main()
