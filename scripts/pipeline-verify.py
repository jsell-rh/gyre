#!/usr/bin/env python3
"""Model-free verification inside one disposable gateway sandbox."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

STAGE = Path('/tmp/stage')
CHECKOUT = Path('/sandbox/workspace/checkout')


def atomic(path, value):
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value))
    temporary.replace(path)


def run_checks(commands, receipt, identity, env):
    records = []
    try:
        for command, cwd, timeout in commands:
            print('\n$ ' + ' '.join(command), flush=True)
            child = subprocess.Popen(command, cwd=cwd, env=env, start_new_session=True)
            try:
                code = child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
                code = 124
            code = 137 if code == -signal.SIGKILL else code
            records.append({'command': command, 'exit_code': code})
            if code:
                break
        atomic(receipt, identity | {'checks': records, 'passed': bool(records) and all(r['exit_code'] == 0 for r in records)})
    except Exception as exc:
        print('verification infrastructure error: ' + str(exc), flush=True)
        atomic(receipt, identity | {'checks': records, 'passed': False, 'infrastructure': True})


def git(*args):
    return subprocess.check_output(['git', *args], cwd=CHECKOUT, text=True, timeout=180).strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('operation', choices=['launch', 'run', 'poll'])
    parser.add_argument('--offset', type=int, default=0)
    args = parser.parse_args()
    receipt = STAGE / 'verification.json'
    if args.operation == 'launch':
        # Stable launch identity makes a lost acknowledgement safe to repeat.
        identity = STAGE / 'step.exit.process.json'
        if identity.exists():
            value = json.loads(identity.read_text())
            stat = Path(f"/proc/{value['pid']}/stat")
            if receipt.exists() or (stat.exists() and stat.read_text().rsplit(')', 1)[1].split()[19] == value['start']):
                return
            raise RuntimeError('previous verification process disappeared without a receipt')
        with (STAGE / 'step.log').open('ab', buffering=0) as output:
            child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), 'run'],
                                     stdin=subprocess.DEVNULL, stdout=output, stderr=output, start_new_session=True)
        stat = Path(f'/proc/{child.pid}/stat').read_text().rsplit(')', 1)[1].split()[19]
        atomic(identity, {'pid': child.pid, 'start': stat, 'boot': Path('/proc/sys/kernel/random/boot_id').read_text().strip()})
        return
    if args.operation == 'poll':
        import base64
        log = STAGE / 'step.log'
        with log.open('rb') as source:
            source.seek(args.offset)
            data = source.read(131072)
            offset = source.tell()
        # Drain the complete log before returning the terminal receipt.
        value = json.loads(receipt.read_text()) if receipt.exists() and offset >= log.stat().st_size else None
        identity = json.loads((STAGE / 'step.exit.process.json').read_text())
        stat = Path(f"/proc/{identity['pid']}/stat")
        fields = stat.read_text().rsplit(')', 1)[1].split() if stat.exists() else []
        alive = bool(fields and fields[0] != 'Z' and fields[19] == identity['start'])
        print('GYRE_VERIFY_POLL ' + json.dumps({'offset': offset, 'log': base64.b64encode(data).decode(), 'receipt': value, 'alive': alive}))
        return
    job = json.loads((STAGE / 'verify-job.json').read_text())
    identity = {key: job[key] for key in ('candidate', 'base', 'head', 'tree')}
    env = {**os.environ, 'SKIP_WEB_BUILD': '1', 'CARGO_BUILD_JOBS': '2',
           'CARGO_TARGET_DIR': '/tmp/gyre-target', 'CARGO_HOME': '/tmp/cargo',
           'RUSTUP_HOME': '/usr/local/rustup', 'GYRE_DEV_IMAGE': job['image']}
    try:
        # Retry Git transport in this sandbox; never reallocate for clone errors.
        CHECKOUT.parent.mkdir(parents=True, exist_ok=True)
        for attempt in range(5):
            try:
                if not (CHECKOUT / '.git').exists():
                    CHECKOUT.mkdir(exist_ok=True)
                    subprocess.run(['git', 'init', '-q', str(CHECKOUT)], check=True, timeout=30)
                    git('remote', 'add', 'origin', job['repo_url'])
                    git('config', 'credential.helper', '!f(){ echo username=x-access-token; echo password=${GITHUB_TOKEN:-$GH_TOKEN}; }; f')
                git('fetch', '--quiet', '--filter=blob:none', 'origin', job['base'])
                if (STAGE / 'candidate.bundle').exists():
                    git('fetch', '--quiet', str(STAGE / 'candidate.bundle'), 'HEAD')
                git('checkout', '--quiet', '--detach', job['head'])
                if git('rev-parse', 'HEAD') != job['head'] or git('rev-parse', 'HEAD^{tree}') != job['tree']:
                    raise ValueError('verification checkout differs from the exact assigned tree')
                break
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                if attempt == 4:
                    raise
                time.sleep(min(60, 5 * 2 ** attempt))
        commands = [(['bash', str(STAGE / 'checks.sh')], str(CHECKOUT), 3600),
                    (['cargo', 'test', '--all', '--quiet'], str(CHECKOUT), 3600),
                    (['npm', 'test', '--', '--no-file-parallelism'], str(CHECKOUT / 'web'), 1800)]
        checks_receipt = STAGE / 'verification-checks.json'
        run_checks(commands, checks_receipt, identity, env)
        value = json.loads(checks_receipt.read_text())
        dirty = git('status', '--porcelain').splitlines()
        if any('web/dist/' not in line for line in dirty) or git('rev-parse', 'HEAD^{tree}') != job['tree'] or git('rev-parse', 'HEAD') != job['head']:
            value.update(passed=False, infrastructure=True, error='verification changed its assigned source')
        atomic(receipt, value)
    except Exception as exc:
        print('verification infrastructure error: ' + str(exc), flush=True)
        atomic(receipt, identity | {'passed': False, 'infrastructure': True, 'checks': []})


if __name__ == '__main__':
    main()
