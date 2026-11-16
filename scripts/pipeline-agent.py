#!/usr/bin/env python3
"""One detached Pi invocation with reconnectable, offset-based output."""
import argparse
import base64
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


def atomic(path, value):
    temporary=path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value));temporary.chmod(0o600);temporary.replace(path)


def alive(value):
    try:
        fields=Path(f"/proc/{value['pid']}/stat").read_text().rsplit(')',1)[1].split()
        return fields[0]!='Z' and fields[19]==value['start'] and Path('/proc/sys/kernel/random/boot_id').read_text().strip()==value['boot']
    except (OSError,KeyError):
        return False


def quiesce(root):
    """Kill this invocation's detached/orphaned tools, without logging environments."""
    marker=b'GYRE_AGENT_COMMAND='+os.fsencode(root.resolve())
    boot=Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    deadline=time.monotonic()+5
    while time.monotonic()<deadline:
        active=[]
        for proc in Path('/proc').glob('[0-9]*'):
            try:
                fields=(proc/'stat').read_text().rsplit(')',1)[1].split()
                value={'pid':int(proc.name),'start':fields[19],'boot':boot}
                if marker not in (proc/'environ').read_bytes().split(b'\0'):continue
                if alive(value):active.append(value)
            except (OSError,ValueError):continue
        if not active:return
        for value in active:
            try:
                if alive(value):os.kill(value['pid'],signal.SIGKILL)
            except ProcessLookupError:pass
        time.sleep(.02)
    raise RuntimeError('owned detached tools did not quiesce')


def read_chunk(root,name,offset):
    path=root/name
    if not path.exists():return '',offset
    with path.open('rb') as source:
        if offset<0 or offset>path.stat().st_size:raise ValueError('invalid stream offset')
        source.seek(offset);data=source.read(65536)
        return base64.b64encode(data).decode(),source.tell()


def poll(root,offset,stderr_offset,wait=0):
    deadline=time.monotonic()+min(10,max(0,wait))
    while time.monotonic()<deadline:
        if ((root/'result.json').exists() or
                ((root/'stdout').exists() and (root/'stdout').stat().st_size>offset) or
                ((root/'stderr').exists() and (root/'stderr').stat().st_size>stderr_offset)):
            break
        if (root/'identity.json').exists() and not alive(json.loads((root/'identity.json').read_text())):break
        time.sleep(.1)
    out,offset=read_chunk(root,'stdout',offset)
    err,stderr_offset=read_chunk(root,'stderr',stderr_offset)
    result=root/'result.json';identity=root/'identity.json';receipt=None
    # A process can append its terminal event between our reads and receipt
    # creation. Once the receipt exists, compare against the FINAL spool sizes.
    if result.exists():
        final=json.loads(result.read_text())
        out_size=(root/'stdout').stat().st_size if (root/'stdout').exists() else 0
        err_size=(root/'stderr').stat().st_size if (root/'stderr').exists() else 0
        if offset>=out_size and stderr_offset>=err_size:receipt=final
    running=alive(json.loads(identity.read_text())) if identity.exists() else True
    # Exit can occur between the receipt read and the process liveness read.
    # A finished producer can also have many unread chunks. Neither case is
    # a lost process: drain its final spool before returning the exit receipt.
    if not running and result.exists():
        final=json.loads(result.read_text())
        out_size=(root/'stdout').stat().st_size if (root/'stdout').exists() else 0
        err_size=(root/'stderr').stat().st_size if (root/'stderr').exists() else 0
        if offset>=out_size and stderr_offset>=err_size:receipt=final
        else:running=True

    import runpy
    helper=Path(__file__).with_name('pipeline-usage.py')
    usage=runpy.run_path(str(helper))['usage']() if helper.exists() else {}
    return {'usage':usage,'stdout':out,'stderr':err,'offset':offset,'stderr_offset':stderr_offset,'result':receipt,'alive':running,'long_poll':True}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation',choices=('launch','run','poll','cancel'))
    parser.add_argument('root',type=Path)
    parser.add_argument('--offset',type=int,default=0)
    parser.add_argument('--stderr-offset',type=int,default=0)
    parser.add_argument('--wait',type=float,default=8)
    parser.add_argument('--resume-after-stop',action='store_true')
    args=parser.parse_args();root=args.root
    identity=root/'identity.json';result=root/'result.json'
    if args.operation=='launch':
        root.mkdir(parents=True,exist_ok=True,mode=0o700)
        with (root/'launch.lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            if result.exists():
                exited=json.loads(result.read_text()).get('exit_code')
                resumable=(args.resume_after_stop and exited in (74,130,137,143) and identity.exists()
                           and not alive(json.loads(identity.read_text())))
                if not resumable:return
                result.rename(root/'result.before-restart.json')
            if identity.exists():
                if alive(json.loads(identity.read_text())):return
                if not args.resume_after_stop:
                    raise RuntimeError('agent supervisor disappeared without an exit receipt; refusing duplicate execution')
                job=json.loads((root/'command.json').read_text())
                # The host has proved a stop/start transition. Resume the same
                # persisted Pi session, preserving source and spool offsets.
                if len(job['argv'])==3 and job['argv'][:2]==['sh','-c']:
                    job['argv'][-1]=job['argv'][-1].replace(' --print --mode json ',' --continue --print --mode json ',1)
                atomic(root/'command.json',job)
                identity.rename(root/'identity.before-restart.json')
            subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'run',str(root)],
                             stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
        return
    if args.operation=='run':
        with (root/'run.lock').open('a') as lock:
            try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
            except BlockingIOError:return
            if identity.exists() or result.exists():return
            fields=Path(f'/proc/{os.getpid()}/stat').read_text().rsplit(')',1)[1].split()
            atomic(identity,{'pid':os.getpid(),'start':fields[19],'boot':Path('/proc/sys/kernel/random/boot_id').read_text().strip()})
            child=None
            def stop(*_):
                if child and child.poll() is None:
                    try:os.killpg(child.pid,signal.SIGTERM)
                    except ProcessLookupError:pass
                    try:child.wait(timeout=1)
                    except subprocess.TimeoutExpired:pass
                quiesce(root)
                if child:child.wait(timeout=5)
                atomic(result,{'exit_code':130})
                raise SystemExit(130)
            signal.signal(signal.SIGTERM,stop)
            try:
                job=json.loads((root/'command.json').read_text())
                with (root/'stdout').open('ab',buffering=0) as out,(root/'stderr').open('ab',buffering=0) as err:
                    child=subprocess.Popen(job['argv'],cwd=job['cwd'],env={**os.environ,'GYRE_AGENT_COMMAND':str(root.resolve())},stdin=subprocess.DEVNULL,stdout=out,stderr=err,start_new_session=True)
                    try:
                        fields=Path(f'/proc/{child.pid}/stat').read_text().rsplit(')',1)[1].split()
                        atomic(root/'child.json',{'pid':child.pid,'start':fields[19],'boot':Path('/proc/sys/kernel/random/boot_id').read_text().strip()})
                    except Exception:
                        quiesce(root);child.wait()
                        raise
                    try:code=child.wait(timeout=job['timeout'])
                    except subprocess.TimeoutExpired:
                        quiesce(root);child.wait();code=124
                quiesce(root)
                atomic(result,{'exit_code':128-code if code<0 else code})
            except Exception as exc:
                atomic(result,{'exit_code':74,'error_kind':type(exc).__name__})
        return
    if args.operation=='cancel':
        child=root/'child.json'
        # Give Pi's own SIGTERM handler a chance to stop its tracked tool groups.
        children=[json.loads(child.read_text())] if child.exists() else []
        if not children and identity.exists():
            parent=json.loads(identity.read_text())
            if alive(parent):
                try:pids=Path(f"/proc/{parent['pid']}/task/{parent['pid']}/children").read_text().split()
                except FileNotFoundError:pids=[]
                for pid in pids:
                    try:
                        fields=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()
                        children.append({'pid':int(pid),'start':fields[19],'boot':parent['boot']})
                    except FileNotFoundError:pass
        for value in children:
            try:
                if alive(value):os.killpg(value['pid'],signal.SIGTERM)
            except ProcessLookupError:pass
        time.sleep(.1)
        if identity.exists():
            value=json.loads(identity.read_text())
            try:
                if alive(value):os.kill(value['pid'],signal.SIGTERM)
            except ProcessLookupError:pass
        quiesce(root)
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            active=[p for p in (identity,child) if p.exists() and alive(json.loads(p.read_text()))]
            if not active:return
            time.sleep(.05)
        raise RuntimeError('agent cancellation did not quiesce owned processes')
    print('GYRE_AGENT_POLL '+json.dumps(poll(root,args.offset,args.stderr_offset,args.wait)))


if __name__=='__main__':main()
