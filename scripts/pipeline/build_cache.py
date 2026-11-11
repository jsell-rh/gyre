"""Bounded disk relay for remote build caches. No host compilation or credentials."""
import fcntl
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import time
import uuid

MAX_ARCHIVE=2*1024**3
TOTAL=8*1024**3


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        while block:=f.read(1024**2):h.update(block)
    return h.hexdigest()


def directory(execution):
    path=execution.store.directory/'build-cache';path.mkdir(exist_ok=True);path.chmod(0o700);return path


def select(execution,key,base):
    if not re.fullmatch('[a-f0-9]{64}',key):raise ValueError('invalid cache key')
    folder=directory(execution)
    for manifest in sorted(folder.glob(key+'-*.json'),key=lambda p:p.stat().st_mtime,reverse=True):
        try:
            value=json.loads(manifest.read_text());sha=value['source']
            if not re.fullmatch('[a-f0-9]{40}',sha):continue
            # Only cache producers now included in upstream are trusted. A
            # pending candidate cannot seed another independent verification.
            ancestor=execution.command('git','merge-base','--is-ancestor',sha,base,cwd=execution.store.directory/'source',check=False)
            if ancestor.returncode:continue
            archive=manifest.with_suffix('.tar.gz')
            if archive.is_symlink() or archive.stat().st_size!=value['bytes'] or value['bytes']>MAX_ARCHIVE:continue
            if digest(archive)!=value['sha256']:continue
            return archive,value
        except (OSError,ValueError,KeyError):continue
    return None


def restore(execution,name,request,base):
    key=request['key'];start=time.monotonic();ready={'key':key,'hit':False}
    try:
        found=select(execution,key,base)
        if found:
            archive,value=found
            execution.os('sandbox','upload',name,str(archive),'/tmp/stage/cache-input.tar.gz','--no-git-ignore',timeout=300)
            ready.update(hit=True,sha256=value['sha256'],bytes=value['bytes'])
    except (OSError,ValueError,RuntimeError,subprocess.TimeoutExpired):pass
    marker=execution.directory/'cache-ready.json';marker.write_text(json.dumps(ready));marker.chmod(0o600)
    execution.os('sandbox','upload',name,str(marker),'/tmp/stage/cache-ready.json','--no-git-ignore',timeout=30)
    execution.store.event('build_cache_offered',ready|{'seconds':time.monotonic()-start},execution.claim['task'],execution.claim['id'])


def save(execution,name,receipt):
    cache=receipt.get('cache') or {};value=cache.get('export') or {};key=cache.get('key','')
    if (not receipt.get('passed') or not re.fullmatch('[a-f0-9]{64}',key)
            or not re.fullmatch('[a-f0-9]{64}',value.get('sha256',''))
            or not isinstance(value.get('bytes'),int) or not 0<value['bytes']<=MAX_ARCHIVE):return
    folder=directory(execution);temporary=folder/(uuid.uuid4().hex+'.part')
    if shutil.disk_usage(folder).free<value['bytes']+2*1024**3:return
    try:
        execution.os('sandbox','download',name,'/sandbox/workspace/.gyre-build-cache.tar.gz',str(temporary),timeout=300)
        if temporary.stat().st_size!=value['bytes'] or digest(temporary)!=value['sha256']:raise ValueError('downloaded build cache differs')
        value=value|{'source':receipt['head'],'key':key,'at':time.time()}
        with (folder/'cache.lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            stem=folder/(key+'-'+receipt['head']);temporary.replace(stem.with_suffix('.tar.gz'))
            manifest=stem.with_suffix('.json');manifest.write_text(json.dumps(value));manifest.chmod(0o600)
            manifests=sorted(folder.glob('*.json'),key=lambda p:p.stat().st_mtime,reverse=True)
            total=0
            for item in manifests:
                archive=item.with_suffix('.tar.gz')
                if not archive.exists():item.unlink();continue
                total+=archive.stat().st_size
                if total>TOTAL:archive.unlink();item.unlink()
        execution.store.event('build_cache_saved',value,execution.claim['task'],execution.claim['id'])
    except (OSError,ValueError,RuntimeError,subprocess.TimeoutExpired) as exc:
        execution.store.event('build_cache_unavailable',{'operation':'save','reason':type(exc).__name__},execution.claim['task'],execution.claim['id'])
    finally:temporary.unlink(missing_ok=True)
