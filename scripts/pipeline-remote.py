#!/usr/bin/env python3
"""One isolated model step. Checkpoint publication is separate from approval."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import socket
import sys
import tempfile
import time

STAGE = Path('/tmp/stage')


def run(*args, check=True, **kwargs):
    kwargs.setdefault('timeout', 300)
    return subprocess.run(args, check=check, **kwargs)


def git(*args):
    return subprocess.check_output(['git', *args], text=True, timeout=180).strip()


def checkout_seed(branch, seed):
    for attempt in range(5):
        try:
            options = ['--refetch', '--no-filter'] if attempt else []
            run('git', 'fetch', '--quiet', *options, 'origin', 'main', seed)
            run('git', 'checkout', '-b', branch, seed)
            return
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            if attempt == 4:
                raise
            # Materialize the seed on retry instead of repeating an implicit
            # promisor fetch during checkout. Reuse this sandbox throughout.
            time.sleep(min(60, 5 * 2 ** attempt))


def record_outcome(value):
    temporary = STAGE / 'outcome.json.tmp'
    temporary.write_text(json.dumps(value))
    temporary.chmod(0o600)
    temporary.replace(STAGE / 'outcome.json')


def reattach_candidate(branch, checkout):
    if not git('branch', '--show-current') and git('rev-parse', 'HEAD') == git('rev-parse', branch):
        if not (checkout / '.git/rebase-merge').exists() and not (checkout / '.git/rebase-apply').exists():
            run('git', 'symbolic-ref', 'HEAD', 'refs/heads/' + branch)


def normalize_new_rust(base, formatter='/usr/local/cargo/bin/rustfmt', rustup_home='/usr/local/rustup'):
    """Format newly added Rust files before source is committed for review.

    Use the assignment base, not the last (often bookkeeping-only) commit.
    Existing files retain their historical formatting debt. Tools have already
    been stopped by the trusted collector before this finalizer runs.
    """
    def paths(*args):
        return {os.fsdecode(p) for p in subprocess.check_output(
            ['git', *args], timeout=180).split(b'\0') if p}
    existing = paths('ls-tree', '-r', '--name-only', '-z', base)
    added = paths('diff', '--name-only', '--diff-filter=A', '-z', base, '--', '*.rs')
    added |= paths('ls-files', '--others', '--exclude-standard', '-z', '--', '*.rs')
    root = Path.cwd().resolve()
    report = {'files': [], 'errors': []}
    for name in sorted(added - existing):
        source = root / name
        try:
            if (source.is_symlink() or not source.is_file()
                    or not source.resolve().is_relative_to(root) or source.stat().st_nlink != 1):
                raise ValueError('Rust source must be an owned regular file')
            original = source.read_bytes()
            # Match the differential format gate's config-independent copy.
            # A temp cwd also avoids repository rustup overrides. The default
            # formatter path is in the immutable worker image, not agent PATH.
            # Native exec drops the image's ENV; pin the installed toolchain
            # explicitly instead of relying on a shell or downloading another.
            with tempfile.TemporaryDirectory() as directory:
                copy = Path(directory) / source.name
                copy.write_bytes(original)
                run(formatter, '--edition', '2021', '--config', 'skip_children=true', str(copy),
                    cwd=directory, capture_output=True, text=True, timeout=30,
                    env={'PATH': '/usr/local/cargo/bin:/usr/bin:/bin', 'RUSTUP_HOME': rustup_home})
                formatted = copy.read_bytes()
            if formatted != original:
                source.write_bytes(formatted)
                report['files'].append(name)
        except (OSError, ValueError, subprocess.SubprocessError) as exc:
            # Keep the source checkpoint even when syntax/tooling needs repair.
            # Do not print source-bearing formatter diagnostics or secrets.
            report['errors'].append({'path': name, 'error': type(exc).__name__})
    return report


def capabilities():
    browser = Path('/usr/local/share/gyre-playwright/chromium_headless_shell-1208/chrome-headless-shell-linux64/chrome-headless-shell')
    result = {'chromium': {'path': str(browser), 'installed': browser.is_file() and os.access(browser, os.X_OK)}}
    operation = 'socket'
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            operation = 'bind'
            listener.bind(('127.0.0.1', 0))
            operation = 'listen'
            listener.listen(1)
            listener.settimeout(1)
            operation = 'getsockname'
            address = listener.getsockname()
            operation = 'loopback_connect'
            # An empty nonblocking accept can report EAGAIN even when actual
            # peer acceptance is unsupported. Exercise a real connection.
            with socket.create_connection(address, timeout=1):
                operation = 'accept'
                connection, _ = listener.accept()
                connection.close()
        result['tcp_listener_probe'] = {'supported': True, 'operation': operation}
    except OSError as exc:
        result['tcp_listener_probe'] = {'supported': False, 'operation': operation,
                                        'errno': exc.errno, 'error': str(exc)}
    return result


def main():
    if sys.argv[1:] == ['capabilities']:
        run('cc', '-fuse-ld=lld', '-Wl,--version', '-x', 'c', '/dev/null', '-o', '/tmp/gyre-linker-probe')
        value = capabilities()
        (STAGE / 'capabilities.json').write_text(json.dumps(value))
        print(json.dumps(value))
        return 0
    if len(sys.argv) != 3 or sys.argv[1] != 'finalize':
        raise ValueError('usage: pipeline-remote.py finalize 0|1')
    success = sys.argv[2] == '1'
    job = json.loads((STAGE / 'job.json').read_text())
    role, task, branch = job['stage'], job['task'], job['branch']
    checkout = Path(job['checkout'])
    os.chdir(checkout)
    # Identity and credential helpers come from trusted executor configuration.
    run('git', 'config', 'user.name', 'gyre-pipeline')
    run('git', 'config', 'user.email', 'jsell-rh@users.noreply.github.com')
    run('git', 'config', 'core.hooksPath', '/dev/null')
    run('git', 'config', 'credential.helper', '!f(){ echo username=x-access-token; echo password=${GITHUB_TOKEN:-$GH_TOKEN}; }; f')
    run('git', 'remote', 'set-url', 'origin', job['repo_url'])
    path = checkout / 'specs/tasks' / (task + '.md')
    before = STAGE / 'review-before.json'
    valid = success
    source_valid = True
    formatting = None
    # Returning to the same immutable candidate via detached HEAD is harmless.
    # Reattach without changing the tree or approving edits.
    reattach_candidate(branch, checkout)
    if role in ('review', 'assess', 'triage', 'spec_audit', 'decompose', 'plan_review'):
        guard = run('python3', str(STAGE / 'dev-review-guard.py'), 'check', str(before), check=False)
        source_valid = guard.returncode == 0
        valid = valid and source_valid
    run('python3', str(STAGE / 'dev-checkpoint.py'), branch)
    if role == 'implement':
        import importlib.util
        spec=importlib.util.spec_from_file_location('artifacts',STAGE/'pipeline-artifacts.py')
        artifacts=importlib.util.module_from_spec(spec);spec.loader.exec_module(artifacts)
        artifacts.restore_generated(checkout,job['input']['base'])
        spec=importlib.util.spec_from_file_location('contract',STAGE/'dev-contract.py')
        contract=importlib.util.module_from_spec(spec);spec.loader.exec_module(contract)
        path.write_text(contract.restore_checkbox_markers(job['body'],path.read_text()))
        if success:
            formatting = normalize_new_rust(job['input']['base'])
            print('GYRE_RUSTFMT ' + json.dumps(formatting), flush=True)
            valid = valid and not formatting['errors']
    run('git', 'add', '-A')
    if run('git', 'diff', '--cached', '--quiet', check=False).returncode:
        run('git', '-c', 'core.hooksPath=/dev/null', 'commit', '-m', f'{role}({task}): pipeline checkpoint')
    if role == 'implement':
        run('python3', str(STAGE / 'dev-attribution.py'), task, '--commit')
    head = git('rev-parse', 'HEAD')
    outcome = {'stage': role, 'head': head, 'base': job['input']['base'], 'tree': git('rev-parse', 'HEAD^{tree}'),
               'agent_exit': 0 if success else 1, 'stream_exit': 0 if success else 1,
               'valid': valid, 'source_valid': source_valid, 'task_body': path.read_text()}
    if formatting is not None:
        outcome['formatting'] = formatting
    if role in ('review','assess'):
        report = 'assessment' if role == 'assess' else 'verdict'
        verdict = STAGE / (report + '.json')
        if verdict.exists():
            outcome[report] = json.loads(verdict.read_text())
        else:
            outcome['valid'] = False
    if role == 'triage' and job['input'].get('decision_request'):
        report=STAGE/'decision.json'
        if report.exists():outcome['decision']=json.loads(report.read_text())
        else:outcome['valid']=False
    if role in ('spec_audit','decompose','plan_review'):
        report=STAGE/'planning.json'
        if report.exists():outcome['planning']=json.loads(report.read_text())
        else:outcome['valid']=False
    record_outcome(outcome)
    # This private, claim-specific branch can never overwrite another worker.
    # Lost push responses are resolved by inspecting the exact remote head.
    for attempt in range(5):
        observed = git('ls-remote', '--heads', 'origin', branch).split()
        (STAGE / 'push-expected').write_text(observed[0] if observed else '')
        if observed and observed[0] == head:
            break
        if observed:
            raise RuntimeError('checkpoint branch contains an unexpected head')
        if run('git', 'push', f'--force-with-lease=refs/heads/{branch}:', 'origin',
               f'HEAD:refs/heads/{branch}', check=False).returncode == 0:
            break
        if attempt == 4:
            raise RuntimeError('checkpoint publication unavailable; capture recovery before cleanup')
        time.sleep(min(60, 5 * 2 ** attempt))
    record_outcome(outcome | {'published': True, 'branch': branch})
    (STAGE / 'push-expected').write_text(head)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
