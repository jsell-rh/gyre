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


def run_checks(commands, receipt, identity, env, retry_delays=(5, 10, 20, 40)):
    records = []
    try:
        for command, cwd, timeout in commands:
            for attempt in range(len(retry_delays) + 1):
                print('\n$ ' + ' '.join(command), flush=True)
                child = subprocess.Popen(command, cwd=cwd, env=env, start_new_session=True)
                try:
                    code = child.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
                    code = 124
                code = 137 if code == -signal.SIGKILL else code
                if code not in (75, 77) or attempt == len(retry_delays):
                    break
                print(f'Infrastructure retry in {retry_delays[attempt]}s; retaining sandbox and build cache.', flush=True)
                time.sleep(retry_delays[attempt])
            records.append({'command': command, 'exit_code': code, 'attempts': attempt + 1})
            if code:
                break
        atomic(receipt, identity | {'checks': records, 'passed': bool(records) and all(r['exit_code'] == 0 for r in records)})
    except Exception as exc:
        print('verification infrastructure error: ' + str(exc), flush=True)
        atomic(receipt, identity | {'checks': records, 'passed': False, 'infrastructure': True})


def git(*args):
    return subprocess.check_output(['git', *args], cwd=CHECKOUT, text=True, timeout=180).strip()


def source_integrity(identity):
    fields = subprocess.check_output(['git','status','--porcelain=v1','-z','--untracked-files=all'],cwd=CHECKOUT).split(b'\0')
    paths=[]
    index=0
    while index < len(fields):
        entry=fields[index];index+=1
        if not entry:continue
        paths.append(os.fsdecode(entry[3:]))
        if b'R' in entry[:2] or b'C' in entry[:2]:
            paths.append(os.fsdecode(fields[index]));index+=1
    unexpected=sorted({path for path in paths if not path.startswith('web/dist/')})
    head,tree=git('rev-parse','HEAD'),git('rev-parse','HEAD^{tree}')
    if unexpected or head != identity['head'] or tree != identity['tree']:
        return {'error_kind':'source_integrity','error':'verification changed its assigned source',
                'dirty_paths':unexpected[:100],'dirty_path_count':len(unexpected),
                'observed_head':head,'observed_tree':tree}
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('operation', choices=['launch', 'run', 'poll'])
    parser.add_argument('--offset', type=int, default=0)
    parser.add_argument('--resume-after-stop', action='store_true')
    args = parser.parse_args()
    receipt = STAGE / 'verification.json'
    if args.operation == 'launch':
        import fcntl
        with (STAGE/'launch.lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            # Stable launch identity makes a lost acknowledgement safe to repeat.
            identity = STAGE / 'step.exit.process.json'
            if identity.exists():
                value = json.loads(identity.read_text())
                stat = Path(f"/proc/{value['pid']}/stat")
                if receipt.exists() or (stat.exists() and stat.read_text().rsplit(')', 1)[1].split()[19] == value['start']):
                    return
                if not args.resume_after_stop:
                    raise RuntimeError('previous verification process disappeared without a receipt')
                identity.rename(STAGE/'step.exit.process.before-restart.json')
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
        if not alive and receipt.exists() and offset>=log.stat().st_size:
            value=json.loads(receipt.read_text())
        import runpy
        usage=runpy.run_path(str(STAGE/'pipeline-usage.py'))['usage']() if (STAGE/'pipeline-usage.py').exists() else {}
        print('GYRE_VERIFY_POLL ' + json.dumps({'usage':usage,'offset': offset, 'log': base64.b64encode(data).decode(), 'receipt': value, 'alive': alive, 'cache_request':json.loads((STAGE/'cache-request.json').read_text()) if (STAGE/'cache-request.json').exists() else None}))
        return
    job = json.loads((STAGE / 'verify-job.json').read_text())
    identity = {key: job[key] for key in ('candidate', 'base', 'head', 'tree')}
    import runpy
    build_env=runpy.run_path(str(STAGE/'pipeline-build-env.py'))['environment']()
    env = {**os.environ, **build_env, 'SKIP_WEB_BUILD':'1',
           'GYRE_DEV_IMAGE':job['image'],'GYRE_VERIFY_KEEP_WEB_DIST':'1'}
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
        # The host bounds the complete assignment. A thin worker must not lose
        # its healthy compiler/cache to a shorter, hidden per-command deadline.
        timeout=job.get('timeout_seconds',14400)
        commands = [(['bash', str(STAGE / 'checks.sh')], str(CHECKOUT), timeout),
                    (['cargo', 'test', '--all', '--quiet'], str(CHECKOUT), timeout),
                    (['npm', 'test', '--', '--no-file-parallelism'], str(CHECKOUT / 'web'), timeout)]
        if 'probe' in job:
            import re
            probe = job['probe']
            if (not isinstance(probe, list) or len(probe) != 2 or probe[0] != 'bash' or
                    not isinstance(probe[1], str) or not re.fullmatch(r'scripts/check-[a-z0-9-]+\.sh', probe[1])):
                raise ValueError('unsupported baseline probe')
            commands = [(probe, str(CHECKOUT), timeout)]
        cache=None
        if job.get('cache_enabled') and 'probe' not in job:
            cache_module=runpy.run_path(str(STAGE/'pipeline-cache.py'))
            cache=cache_module['prepare'](CHECKOUT,job,build_env,STAGE)
        checks_receipt = STAGE / 'verification-checks.json'
        run_checks(commands, checks_receipt, identity, env)
        value = json.loads(checks_receipt.read_text())
        integrity = source_integrity(identity)
        if integrity:
            value.update(integrity,passed=False,infrastructure=True)
            print('Verification source integrity failed: '+json.dumps(integrity),flush=True)
        if value.get('passed') and 'probe' not in job:
            import importlib.util
            spec=importlib.util.spec_from_file_location('artifacts',STAGE/'pipeline-artifacts.py')
            artifacts=importlib.util.module_from_spec(spec);spec.loader.exec_module(artifacts)
            if cache is not None:
                value['cache']=cache_module['export'](CHECKOUT,build_env,STAGE,cache)
            value['publication']=artifacts.publish(CHECKOUT,job['task'],job['publication_branch'])
        atomic(receipt, value)
    except Exception as exc:
        print('verification infrastructure error: ' + str(exc), flush=True)
        atomic(receipt, identity | {'passed': False, 'infrastructure': True, 'checks': []})


if __name__ == '__main__':
    main()
