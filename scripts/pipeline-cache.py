#!/usr/bin/env python3
"""Remote disposable build caches; never contain verification decisions or secrets."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import time

MAX_ARCHIVE=2*1024**3
MAX_EXPANDED=12*1024**3
ROOT=Path('/tmp/gyre-build')
PREFIXES=('target/','cargo/registry/','npm/_cacache/content-v2/')


def digest(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        while block:=f.read(1024**2):h.update(block)
    return h.hexdigest()


def key(checkout, job, env):
    checkout=Path(checkout)
    paths=['Cargo.lock','web/package-lock.json','.cargo/config.toml','rust-toolchain.toml']
    paths+=sorted(str(p.relative_to(checkout)) for p in checkout.glob('**/Cargo.toml') if '.git' not in p.parts and 'node_modules' not in p.parts and 'target' not in p.parts)
    inputs={p:digest(checkout/p) for p in paths if (checkout/p).is_file()}
    versions=[subprocess.check_output(cmd,text=True,timeout=30,env={**os.environ,**env}).strip() for cmd in (['rustc','-vV'],['node','--version'],['mold','--version'])]
    value={'schema':1,'image':job['image'],'versions':versions,'inputs':inputs,'environment':env}
    return hashlib.sha256(json.dumps(value,sort_keys=True).encode()).hexdigest()


def restore(archive, expected, root=ROOT):
    archive=Path(archive);root=Path(root)
    if archive.stat().st_size>MAX_ARCHIVE or digest(archive)!=expected:raise ValueError('cache archive digest/size mismatch')
    if root.is_symlink():raise ValueError('cache root is a symlink')
    root.mkdir(parents=True,exist_ok=True)
    with tarfile.open(archive,'r:gz') as tar:
        members=[];size=0
        for member in tar:
            path=PurePosixPath(member.name)
            if (path.is_absolute() or '..' in path.parts or not member.isfile()
                    or not member.name.startswith(PREFIXES)):
                raise ValueError('unsafe build cache entry')
            destination=root/member.name
            if any(p.is_symlink() for p in (destination,*destination.parents) if p!=root.parent and p.is_relative_to(root)):
                raise ValueError('cache destination is a symlink')
            size+=member.size
            if size>MAX_EXPANDED or len(members)>=200000:raise ValueError('expanded cache exceeds budget')
            members.append(member)
        if shutil.disk_usage(root).free<size+1024**3:raise ValueError('insufficient disk for cache restore')
        tar.extractall(root,members=members,filter='data')
    return size


def pack(destination, root=ROOT):
    destination=Path(destination);root=Path(root)
    files=[];size=0
    for prefix in PREFIXES:
        folder=root/prefix
        if not folder.exists() or folder.is_symlink():continue
        for directory,dirs,names in os.walk(folder,followlinks=False):
            dirs[:]=[d for d in dirs if not (Path(directory)/d).is_symlink() and d!='_logs']
            for name in names:
                path=Path(directory)/name
                if path.is_symlink() or not path.is_file():continue
                size+=path.stat().st_size
                if size>MAX_EXPANDED or len(files)>=200000:raise ValueError('build cache exceeds export budget')
                files.append(path)
    if not files:return None
    if shutil.disk_usage(destination.parent).free<min(size,MAX_ARCHIVE)+1024**3:raise ValueError('insufficient cache export disk')
    try:
        with tarfile.open(destination,'w:gz',compresslevel=1,dereference=True) as tar:
            for path in files:
                tar.add(path,arcname=str(path.relative_to(root)),recursive=False)
                if destination.stat().st_size>MAX_ARCHIVE:raise ValueError('compressed cache exceeds budget')
        if destination.stat().st_size>MAX_ARCHIVE:raise ValueError('compressed cache exceeds budget')
        return {'sha256':digest(destination),'bytes':destination.stat().st_size,'expanded_bytes':size}
    except Exception:
        destination.unlink(missing_ok=True);raise


def prepare(checkout, job, env, stage):
    cache_key=key(checkout,job,env);stage=Path(stage)
    request=stage/'cache-request.json';temporary=request.with_suffix('.tmp');temporary.write_text(json.dumps({'key':cache_key}));temporary.replace(request)
    deadline=time.monotonic()+600
    while not (stage/'cache-ready.json').exists() and time.monotonic()<deadline:time.sleep(1)
    try:
        ready=json.loads((stage/'cache-ready.json').read_text())
        if ready.get('key')==cache_key and ready.get('hit'):
            size=restore(stage/'cache-input.tar.gz',ready['sha256'])
            # Never reuse workspace compilation products across source trees.
            subprocess.run(['python3',str(stage/'dev-cargo-clean.py')],cwd=checkout,env={**os.environ,**env},check=True,timeout=180)
            print(f'Build cache restored: {cache_key}, {size} bytes; workspace packages invalidated.',flush=True)
            return {'key':cache_key,'hit':True,'bytes':size}
    except Exception as exc:
        print('Build cache miss: '+type(exc).__name__,flush=True)
        target=ROOT/'target'
        if target.exists() and not target.is_symlink():shutil.rmtree(target)
    print('Build cache miss: '+cache_key,flush=True)
    return {'key':cache_key,'hit':False}


def export(checkout,env,stage,cache):
    try:
        subprocess.run(['python3',str(Path(stage)/'dev-cargo-clean.py')],cwd=checkout,env={**os.environ,**env},check=True,timeout=180)
        result=pack(Path(checkout).parent/'.gyre-build-cache.tar.gz')
        return cache|{'export':result} if result else cache
    except Exception as exc:
        print('Build cache export skipped: '+type(exc).__name__,flush=True)
        return cache
