#!/usr/bin/env python3
"""One isolated model step. Checkpoint publication is separate from approval."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import time

STAGE = Path('/tmp/stage')


def run(*args, check=True, **kwargs):
    return subprocess.run(args, check=check, **kwargs)


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def main():
    job = json.loads((STAGE / 'job.json').read_text())
    role, task, branch = job['stage'], job['task'], job['branch']
    os.environ.update(HOME='/tmp', CARGO_BUILD_JOBS='2',
                      CARGO_TARGET_DIR='/tmp/gyre-target', CARGO_HOME='/tmp/cargo',
                      RUSTUP_HOME='/usr/local/rustup',
                      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER='cc',
                      RUSTFLAGS='-C link-arg=-fuse-ld=lld')
    # Fail before inference when the image cannot link Rust build scripts.
    run('cc', '-fuse-ld=lld', '-Wl,--version', '-x', 'c', '/dev/null', '-o', '/tmp/gyre-linker-probe')
    binaries = STAGE / 'bin'
    binaries.mkdir(exist_ok=True)
    for name in ('cargo', 'npm', 'npx'):
        actual = shutil.which(name)
        if not actual:
            raise RuntimeError(f'worker image has no {name}')
        os.environ['GYRE_DEV_REAL_' + name.upper()] = actual
        shim = binaries / name
        if not shim.exists():
            shim.symlink_to('../dev-build-command.sh')
    os.environ['PATH'] = str(binaries) + ':' + os.environ['PATH']
    config = Path('/tmp/.omp/agent')
    config.mkdir(parents=True, exist_ok=True)
    for name in ('models.yml', 'config.yml'):
        (config / name).write_bytes((STAGE / name).read_bytes())
    run('git', 'config', '--global', 'user.name', 'gyre-pipeline')
    run('git', 'config', '--global', 'user.email', 'jsell-rh@users.noreply.github.com')
    run('git', 'config', '--global', '--add', 'safe.directory', '*')
    run('git', 'config', '--global', 'http.version', 'HTTP/1.1')
    run('git', 'config', '--global', 'credential.helper',
        '!f(){ echo username=x-access-token; echo password=$GITHUB_TOKEN; }; f')
    checkout = Path('/tmp/gyre')
    if not (checkout / '.git').exists():
        for attempt in range(5):
            result = run('git', 'clone', '--quiet', '--filter=blob:none',
                         job['repo_url'], str(checkout), check=False)
            if result.returncode == 0:
                break
            # Clone failures do not allocate another sandbox.
            shutil.rmtree(checkout, ignore_errors=True)
            if attempt == 4:
                raise RuntimeError('clone failed after retries in the same sandbox')
            time.sleep(min(60, 5 * 2 ** attempt))
    os.chdir(checkout)
    seed = job['input'].get('candidate') or job['input']['base']
    run('git', 'fetch', '--quiet', 'origin', 'main', seed)
    run('git', 'checkout', '-b', branch, seed)
    observed = git('ls-remote', '--heads', 'origin', branch).split()
    if observed:
        raise RuntimeError('claim branch already exists before execution')
    (STAGE / 'push-expected').write_text('')
    path = checkout / 'specs/tasks' / (task + '.md')
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(job['body'])
    # A checkpoint may contain a newer task contract than main. Keep it when
    # resuming implementation or reviewing it; discovery supplies its body.
    before = STAGE / 'review-before.json'
    if role in ('review', 'triage'):
        run('python3', str(STAGE / 'dev-review-guard.py'), 'snapshot', str(before), '--task', task)
    prompt = (STAGE / 'prompt.md').read_text()
    if role == 'implement':
        # Let the model resolve conflicts, with the rebase state included in
        # its single assignment. No hidden implementation/review round loop.
        run('git', 'rebase', job['input']['base'], check=False)
    agent = subprocess.Popen(['omp', '-p', '--model', job['model'], '--no-session',
                              '--mode=json', '--approval-mode', 'yolo'],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    stream = subprocess.Popen(['node', str(STAGE / 'dev-stream.mjs'), role,
                               str(STAGE / 'response.txt')], stdin=agent.stdout)
    agent.stdout.close()
    try:
        agent.stdin.write(prompt.encode())
        agent.stdin.close()
        agent.wait(timeout=job.get('timeout', 1800))
    except subprocess.TimeoutExpired:
        agent.send_signal(2)
        try:
            agent.wait(timeout=30)
        except subprocess.TimeoutExpired:
            agent.kill()
            agent.wait()
    stream.wait(timeout=30)
    valid = agent.returncode == 0 and stream.returncode == 0
    if role in ('review', 'triage'):
        guard = run('python3', str(STAGE / 'dev-review-guard.py'), 'check', str(before), check=False)
        valid = valid and guard.returncode == 0
    run('python3', str(STAGE / 'dev-checkpoint.py'), branch)
    run('git', 'add', '-A')
    if run('git', 'diff', '--cached', '--quiet', check=False).returncode:
        run('git', '-c', 'core.hooksPath=/dev/null', 'commit', '-m', f'{role}({task}): pipeline checkpoint')
    if role == 'implement':
        run('python3', str(STAGE / 'dev-attribution.py'), task, '--commit')
    head = git('rev-parse', 'HEAD')
    outcome = {'stage': role, 'head': head, 'base': job['input']['base'], 'tree': git('rev-parse', 'HEAD^{tree}'),
               'agent_exit': agent.returncode, 'stream_exit': stream.returncode,
               'valid': valid, 'task_body': path.read_text()}
    if role == 'review':
        verdict = STAGE / 'verdict.json'
        if verdict.exists():
            outcome['verdict'] = json.loads(verdict.read_text())
        else:
            outcome['valid'] = False
    (STAGE / 'outcome.json').write_text(json.dumps(outcome))
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
    (STAGE / 'outcome.json').write_text(json.dumps(outcome | {'published': True}))
    (STAGE / 'push-expected').write_text(head)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
