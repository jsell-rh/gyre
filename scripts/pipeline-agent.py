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
    return {'stdout':out,'stderr':err,'offset':offset,'stderr_offset':stderr_offset,'result':receipt,'alive':running,'long_poll':True}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation',choices=('launch','run','poll','cancel'))
    parser.add_argument('root',type=Path)
    parser.add_argument('--offset',type=int,default=0)
    parser.add_argument('--stderr-offset',type=int,default=0)
    parser.add_argument('--wait',type=float,default=8)
    args=parser.parse_args();root=args.root
    identity=root/'identity.json';result=root/'result.json'
    if args.operation=='launch':
        root.mkdir(parents=True,exist_ok=True,mode=0o700)
        with (root/'launch.lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            if result.exists():return
            if identity.exists():
                if alive(json.loads(identity.read_text())):return
                raise RuntimeError('agent supervisor disappeared without an exit receipt; refusing duplicate execution')
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
                    try:os.killpg(child.pid,signal.SIGKILL)
                    except ProcessLookupError:pass
                    child.wait(timeout=5)
                atomic(result,{'exit_code':130})
                raise SystemExit(130)
            signal.signal(signal.SIGTERM,stop)
            try:
                job=json.loads((root/'command.json').read_text())
                with (root/'stdout').open('ab',buffering=0) as out,(root/'stderr').open('ab',buffering=0) as err:
                    child=subprocess.Popen(job['argv'],cwd=job['cwd'],stdin=subprocess.DEVNULL,stdout=out,stderr=err,start_new_session=True)
                    try:code=child.wait(timeout=job['timeout'])
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid,signal.SIGKILL);child.wait();code=124
                atomic(result,{'exit_code':128-code if code<0 else code})
            except Exception as exc:
                atomic(result,{'exit_code':74,'error_kind':type(exc).__name__})
        return
    if args.operation=='cancel':
        if identity.exists():
            value=json.loads(identity.read_text())
            if alive(value):os.kill(value['pid'],signal.SIGTERM)
        return
    print('GYRE_AGENT_POLL '+json.dumps(poll(root,args.offset,args.stderr_offset,args.wait)))


if __name__=='__main__':main()
