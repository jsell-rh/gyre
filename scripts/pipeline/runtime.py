"""Scoped runtime evidence and recovery; control-plane failures are not code failures."""
import fcntl
import json
import os
from pathlib import Path
import subprocess
import time


def observe(store, force=False):
    context, namespace = store.setting('runtime_kube_context'), store.setting('runtime_namespace')
    if not context or not namespace:
        return
    with (store.directory/'runtime-observation.lock').open('a') as lock:
        try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except BlockingIOError:return
        if not force and time.time()-store.setting('runtime_observed_at',0)<5:return
        store.set_setting('runtime_observed_at',time.time())
        try:
            sandboxes=None
            pending=list(store.db.execute("SELECT * FROM resources WHERE kind='sandbox' AND state='deleting'"))
            if pending:
                try:
                    value=subprocess.run(['kubectl','--context',context,'--request-timeout=8s','-n',namespace,
                                          'get','sandboxes.agents.x-k8s.io','-o','json'],capture_output=True,text=True,timeout=10,check=True)
                    listing=json.loads(value.stdout)
                    if listing.get('metadata',{}).get('continue'):raise ValueError('incomplete sandbox inventory')
                    sandboxes={item['metadata']['name']:item for item in listing['items']}
                except (OSError,ValueError,KeyError,TypeError,subprocess.SubprocessError) as exc:
                    # Capacity diagnostics must not suppress existing OOM evidence.
                    store.set_setting('runtime_observer_error',{'kind':type(exc).__name__,'at':time.time()})
            value=subprocess.run(['kubectl','--context',context,'--request-timeout=8s','-n',namespace,'get','pods','-o','json'],capture_output=True,text=True,timeout=10,check=True)
            listing=json.loads(value.stdout)
            if listing.get('metadata',{}).get('continue'):raise ValueError('incomplete pod inventory')
            pods=listing['items']
            for pod in pods:
                meta=pod['metadata'];name=meta['name']
                if meta.get('labels',{}).get('openshell.ai/boundary-role')!='workload':continue
                terminal=next((c['state']['terminated'] for c in pod.get('status',{}).get('containerStatuses',[]) if 'terminated' in c.get('state',{})),None)
                if not terminal:continue
                evidence={'pod_uid':meta['uid'],'reason':terminal.get('reason'),'exit_code':terminal.get('exitCode'),
                          'finished_at':terminal.get('finishedAt'),'resources':pod['spec']['containers'][0].get('resources',{}),'observed_at':time.time()}
                with store.transaction():
                    row=store.db.execute("SELECT * FROM resources WHERE name=? AND kind='sandbox' AND state IN ('intent','present')",(name,)).fetchone()
                    if not row:continue
                    data=json.loads(row['data'])
                    if data.get('runtime',{}).get('pod_uid')==meta['uid']:continue
                    data['runtime']=evidence
                    store.db.execute('UPDATE resources SET data=? WHERE name=?',(json.dumps(data),name))
                    store.event('sandbox_terminated',{'sandbox':name,**evidence},work=row['work'])
            if sandboxes is not None:observe_capacity(store,pods,context,namespace,sandboxes,pending)
        except (OSError,ValueError,KeyError,TypeError,subprocess.SubprocessError) as exc:
            # Missing cluster diagnostics must never make an otherwise healthy worker fail.
            store.set_setting('runtime_observer_error',{'kind':type(exc).__name__,'at':time.time()})


def observe_capacity(store, pods, context, namespace, sandboxes, pending):
    """Do not confuse retained gateway metadata with an allocated workload.

    A missing pod alone is insufficient: an active sandbox could recreate it.
    Require a suspended/deleting or absent Kubernetes sandbox as well. Never
    change cleanup state or release in-flight creation/admission reservations.
    """
    for original in pending:
        name=original['name'];sandbox=sandboxes.get(name)
        meta=(sandbox or {}).get('metadata',{})
        ident=meta.get('labels',{}).get('openshell.ai/sandbox-id')
        def belongs(pod):
            pm=pod['metadata'];labels=pm.get('labels',{})
            return (pm['name']==name or labels.get('openshell.ai/sandbox-name')==name
                or bool(ident and ident in (labels.get('openshell.ai/sandbox-id'),labels.get('openshell.ai/boundary-pair')))
                or bool(meta.get('uid') and any(r.get('uid')==meta['uid'] for r in pm.get('ownerReferences',[]))))
        inactive=(sandbox is None or bool(meta.get('deletionTimestamp'))
                  or sandbox.get('spec',{}).get('operatingMode')=='Suspended')
        absent=inactive and not any(belongs(pod) for pod in pods)
        with store.transaction():
            row=store.db.execute("SELECT * FROM resources WHERE name=? AND kind='sandbox' AND state='deleting'",(name,)).fetchone()
            if not row or row['work']!=original['work'] or row['token']!=original['token']:continue
            data=json.loads(row['data']);old=data.get('compute_observation') or {}
            data['compute_observation']={'absent':absent,'at':time.time(),'context':context,'namespace':namespace,
                'work':row['work'],'token':row['token'],'sandbox_uid':meta.get('uid')}
            store.db.execute('UPDATE resources SET data=? WHERE name=?',(json.dumps(data),name))
            if old.get('absent')!=absent:
                store.event('compute_occupancy_observed',{'sandbox':name,'absent':absent,'cleanup_pending':True},work=row['work'])


def failure(execution, name):
    row=execution.store.db.execute('SELECT data FROM resources WHERE name=?',(name,)).fetchone()
    evidence=json.loads(row['data']).get('runtime') if row else None
    if evidence and evidence.get('reason')=='OOMKilled':
        from .compute import record_oom
        memory=evidence.get('resources',{}).get('limits',{}).get('memory','')
        if memory.endswith('Gi'):gib=float(memory[:-2])
        elif memory.endswith('Mi'):gib=float(memory[:-2])/1024
        else:
            # No guessed escalation when the actual killed allocation is unknown.
            from .execution import Attention
            error=Attention('Observed OOM but cannot determine the killed memory limit');error.category='sandbox_oom';return error
        return record_oom(execution.store,execution.claim['task'],execution.claim['stage'],gib,{'sandbox':name,**evidence})
    return None


def snapshot(execution, name, invoke):
    execution.current()
    try:result=invoke(['sandbox','get',name,'--output','json'],capture_output=True,text=True,timeout=20)
    except (OSError,subprocess.TimeoutExpired):return None
    if result.returncode:return None
    raw=result.stdout
    start=raw.find('{')
    if start<0:return None
    try:value=json.loads(raw[start:])
    except (ValueError,TypeError):return None
    labels=value.get('labels',{})
    if labels.get('gyre.dev/pipeline')!=execution.store.setting('owner') or labels.get('gyre.dev/work')!=execution.claim['id']:
        raise ValueError('sandbox ownership differs from the live assignment')
    return value


def save_failure(execution, error):
    path=execution.directory/'runtime-failure.json'
    path.write_text(json.dumps({'category':getattr(error,'category','gateway_transport'),'message':str(error),
        'attention':getattr(error,'attention',False),'diagnostic':getattr(error,'diagnostic',None)}));path.chmod(0o600)


def ensure_workspace(execution, name, invoke=None):
    """Inspect owned sandbox disk via OpenShell; never patch Kubernetes storage."""
    from .execution import Attention, Retry
    execution.current()
    store=execution.store
    if not store.setting('runtime_namespace'):return
    row=store.db.execute('SELECT * FROM resources WHERE name=?',(name,)).fetchone()
    if not row or row['work']!=execution.claim['id'] or row['token']!=execution.claim['token'] or row['state'] not in ('intent','present'):
        raise Attention('refuse storage inspection after sandbox ownership changed')
    def run(args,**kwargs):
        if invoke:return invoke(args,**kwargs)
        return execution.os(*args,check=False,**{('capture' if k=='capture_output' else k):v for k,v in kwargs.items() if k!='text'})
    value=snapshot(execution,name,run)
    if not value:raise Retry('Cannot establish owned sandbox before checking build disk')
    execution.phase('CheckingBuildStorage',sandbox=name)
    code="""import pathlib,shutil,json
p=pathlib.Path('/tmp/gyre-build')
if p.is_symlink():raise RuntimeError('refuse symlinked build cache')
p.mkdir(exist_ok=True)
mounts=[line.split() for line in pathlib.Path('/proc/mounts').read_text().splitlines()]
mount=max((v for v in mounts if p==pathlib.Path(v[1]) or pathlib.Path(v[1]) in p.parents),key=lambda v:len(v[1]))
disk=shutil.disk_usage(p)
print(json.dumps({'total':disk.total,'free':disk.free,'filesystem':mount[2]}))
"""
    result=run(['sandbox','exec','-n',name,'--','python3','-c',code],capture_output=True,text=True,timeout=30)
    if result.returncode:raise Retry('Cannot inspect sandbox-local build disk through OpenShell')
    evidence=json.loads(result.stdout)
    if evidence['filesystem'] in ('tmpfs','ramfs') or evidence['total']<30*1024**3 or evidence['free']<16*1024**3:
        error=Attention('Insufficient disk-backed build cache space; use supported gateway storage configuration, never a direct PVC patch')
        error.category='sandbox_storage';error.diagnostic=evidence;raise error
    execution.current()
    with store.transaction():
        current=store.db.execute('SELECT * FROM resources WHERE name=?',(name,)).fetchone()
        if not current or current['work']!=row['work'] or current['token']!=row['token'] or current['state'] not in ('intent','present'):
            raise Attention('sandbox ownership changed during build storage inspection')
        data=json.loads(current['data'])|{'build_storage':evidence}
        store.db.execute('UPDATE resources SET data=? WHERE name=?',(json.dumps(data),name))



def reclaim_build_cache(execution, name, invoke):
    """After stopping an agent, free generated output so Git can save source."""
    from .execution import Attention
    row=execution.store.db.execute('SELECT * FROM resources WHERE name=?',(name,)).fetchone()
    if not row or row['work']!=execution.claim['id'] or row['token']!=execution.claim['token'] or row['state'] not in ('intent','present'):
        raise Attention('refuse cache reclamation after sandbox ownership changed')
    code="import pathlib,shutil\np=pathlib.Path('/tmp/gyre-build/target')\nif p.is_symlink(): raise RuntimeError('refuse symlinked build cache')\nif p.exists(): shutil.rmtree(p)"
    result=invoke(['sandbox','exec','-n',name,'--','python3','-c',code],capture_output=True,text=True,timeout=120)
    if result.returncode:raise RuntimeError('generated build cache could not be reclaimed for source capture')
    execution.store.event('build_cache_reclaimed',{'sandbox':name,'reason':'workspace exhausted; preserve source'},execution.claim['task'],execution.claim['id'])


def restart(execution, name, invoke):
    """One restart per live claim. Never start an unowned or deleting sandbox."""
    from .execution import Attention
    value=snapshot(execution,name,invoke)
    if not value or value.get('phase','').lower()!='stopped':return False
    row=execution.store.db.execute('SELECT * FROM resources WHERE name=?',(name,)).fetchone()
    if not row or row['work']!=execution.claim['id'] or row['token']!=execution.claim['token'] or row['state'] in ('deleting','absent'):
        raise Attention('refuse restart after sandbox ownership changed')
    data=json.loads(row['data'])
    if data.get('restart_count',0)>=1:
        error=Attention('Sandbox stopped again after its bounded restart; inspect the retained runtime evidence');error.category='sandbox_stopped';raise error
    execution.store.resource_state(name,row['state'],data|{'restart_count':1})
    try:result=invoke(['sandbox','start',name],capture_output=True,text=True,timeout=120)
    except subprocess.TimeoutExpired:result=None  # Lost acknowledgement: inspect the same sandbox.
    if result is not None and result.returncode:
        error=Attention('Owned sandbox restart was rejected; evidence retained, no blind replacement');error.category='sandbox_stopped';raise error
    deadline=time.monotonic()+1800
    while time.monotonic()<deadline:
        value=snapshot(execution,name,invoke)
        phase=(value or {}).get('phase','').lower()
        if phase=='ready':
            execution.store.event('sandbox_restarted',{'sandbox':name},execution.claim['task'],execution.claim['id'])
            return True
        if phase in ('error','failed','deleted','deleting'):break
        execution.cancelled.wait(5)
    error=Attention('Restarted sandbox did not become Ready within its infrastructure deadline');error.category='sandbox_stopped';raise error


def restore_inputs(execution, name, invoke, bundle=None):
    """Reinstall trusted inputs after a stopped sandbox loses its /tmp filesystem."""
    from .execution import ROOT
    bundle=bundle or execution.directory/'bundle'
    result=invoke(['sandbox','exec','-n',name,'--','mkdir','-p','/tmp/stage'],capture_output=True,text=True,timeout=30)
    if result.returncode:raise RuntimeError('could not restore stage directory')
    inputs=[(p,'/tmp/stage/'+str(p.relative_to(bundle))) for p in sorted(bundle.rglob('*')) if p.is_file()]
    inputs.extend((ROOT/'scripts'/name,'/tmp/stage/'+name) for name in ('pipeline-agent.py','pipeline-usage.py'))
    for source,target in inputs:
        result=invoke(['sandbox','upload',name,str(source),target,'--no-git-ignore'],capture_output=True,text=True,timeout=180)
        if result.returncode:raise RuntimeError('could not restore trusted stage input '+source.name)
