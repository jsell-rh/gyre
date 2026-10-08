#!/usr/bin/env python3
"""Validate fidelity records and replay their bounded acceptance probes."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import subprocess

spec = importlib.util.spec_from_file_location("coverage", Path(__file__).with_name("dev-coverage.py"))
coverage = importlib.util.module_from_spec(spec); spec.loader.exec_module(coverage)


def git(*args):
    result = subprocess.run(["git", *args], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr)
    return result.stdout.strip()


def validate(contract, root, base=None):
    report_path = root / "specs/reviews" / f"audit-{contract['task']}.json"
    report = json.loads(report_path.read_text())
    if report.get("version") != 1 or report.get("task") != contract["task"] or report.get("code_generation") != contract["code_generation"]:
        raise ValueError("audit report must bind the assigned task and inspected code generation")
    if coverage.code_generation(git, "HEAD") != contract["code_generation"]:
        raise ValueError("audited production generation changed; re-audit current code")
    entries = report.get("findings", [])
    ids = [entry.get("row") for entry in entries]
    if sorted(ids) != sorted(row["id"] for row in contract["rows"]):
        raise ValueError("audit must report exactly every assigned coverage row once")
    matrix = {row["id"]: row for row in coverage.rows((root / contract["coverage"]).read_text())}
    for entry in entries:
        status = entry.get("status")
        if status not in ("verified", "gap", "n/a"):
            raise ValueError("audit finding has invalid status")
        if not entry.get("explanation"):
            raise ValueError("audit finding must explain behavior evidence or the remaining gap")
        if status == "verified":
            if matrix[entry["row"]]["status"] != "verified" or not entry.get("production") or not entry.get("probes"):
                raise ValueError("verified finding needs production entry points, probes, and matching coverage")
            for point in entry["production"]:
                path = (root / point["path"]).resolve()
                if not path.is_relative_to(root.resolve()) or not path.is_file() or not point.get("symbol"):
                    raise ValueError("production evidence must resolve inside the checked repository")
                if point["symbol"] not in path.read_text():
                    raise ValueError("production evidence symbol does not exist")
        elif status == "gap":
            if matrix[entry["row"]]["status"] in ("implemented", "verified") or not entry.get("tasks"):
                raise ValueError("an open gap needs real follow-up tasks and open coverage")
            for task in entry["tasks"]:
                permitted = set(contract['reserved_tasks']) | {task for row in contract['rows'] for task in row['tasks']}
                if task not in permitted or task not in matrix[entry['row']]['tasks']:
                    raise ValueError('gap tasks must be reserved or assigned, and linked from coverage')
                body = (root / "specs/tasks" / f"{task}.md").read_text()
                if re.search(r"^progress:[ \t]*complete[ \t]*$", body, re.M):
                    raise ValueError("gap follow-up task still claims completion")
        elif matrix[entry["row"]]["status"] != "n/a":
            raise ValueError("context classification must match the coverage matrix")
    if base:
        allowed = {contract["coverage"], "specs/coverage/SUMMARY.md", f"specs/tasks/{contract['task']}.md"}
        tasks = set(contract["reserved_tasks"]) | {task for row in contract["rows"] for task in row["tasks"]}
        allowed |= {f"specs/tasks/{task}.md" for task in tasks}
        for path in git("diff", "--name-only", base, "HEAD").splitlines():
            review_allowed = path in (f"specs/reviews/audit-{contract['task']}.json", f"specs/reviews/{contract['task']}-review.md")
            if path not in allowed and not review_allowed:
                raise ValueError(f"fidelity audit changed outside its metadata scope: {path}")
    return entries


def replay(entries, root):
    for entry in entries:
        for probe in entry.get("probes", []):
            args = probe["argv"]
            if not isinstance(args, list) or not args or not all(isinstance(arg, str) for arg in args):
                raise ValueError("probe argv must be a nonempty string list")
            if args[:2] not in (["cargo", "test"], ["npm", "test"]):
                if args[0] not in ("python3", "node", "bash") or len(args) < 2:
                    raise ValueError("acceptance probe must run tests or a repository script")
                script = (root / args[1]).resolve()
                if not script.is_relative_to(root.resolve()) or not script.is_file():
                    raise ValueError("acceptance probe script must resolve inside the repository")
            cwd = (root / probe.get("cwd", ".")).resolve()
            if not cwd.is_relative_to(root.resolve()):
                raise ValueError("acceptance probe cwd escapes repository")
            timeout = probe.get("timeout", 300)
            if not isinstance(timeout, int) or not 1 <= timeout <= 600:
                raise ValueError("acceptance probe must have a bounded timeout")
            print("$ " + " ".join(args), flush=True)
            result = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)
            output = result.stdout + result.stderr
            print(output[-65536:], flush=True)
            if result.returncode:
                raise ValueError("acceptance probe failed")
            if args[:2] == ["cargo", "test"] and not any(int(value) > 0 for value in re.findall(r"test result:.*?([0-9]+) passed", output)):
                raise ValueError("acceptance probe matched no passing Rust tests")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("contract", type=Path)
    parser.add_argument("--base")
    parser.add_argument("--replay", action="store_true")
    parser.add_argument('--generation-only', action='store_true')
    args = parser.parse_args()
    root = Path.cwd()
    contract = json.loads(args.contract.read_text())
    if args.generation_only:
        if coverage.code_generation(git, 'HEAD') != contract['code_generation']:
            raise ValueError('production generation changed; controller must assign a fresh audit')
        return
    entries = validate(contract, root, args.base)
    if args.replay:
        replay(entries, root)


if __name__ == "__main__":
    main()
