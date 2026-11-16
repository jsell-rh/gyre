"""Read-only namespace capacity observations and transactional local reservations."""
from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal, ROUND_CEILING
import fcntl
import json
import re
import subprocess
import time

KEYS = ('requests.cpu','requests.memory','limits.cpu','limits.memory','pods')
TTL = 30


def enabled(store):
    return bool(store.setting('runtime_kube_context') and store.setting('runtime_namespace'))


def quantity(value, cpu=False):
    match=re.fullmatch(r'([+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)([a-zA-Z]*)',str(value))
    if not match:raise ValueError('invalid resource quantity')
    suffix=match[2]
    factors={'':1,'n':Decimal('0.000000001'),'u':Decimal('0.000001'),'m':Decimal('0.001'),
             'k':10**3,'K':10**3,'M':10**6,'G':10**9,'T':10**12,'P':10**15,'E':10**18,
             **{s:2**(10*i) for i,s in enumerate(('Ki','Mi','Gi','Ti','Pi','Ei'),1)}}
    if suffix not in factors:raise ValueError('unknown resource unit')
    return int((Decimal(match[1])*factors[suffix]*(1000 if cpu else 1)).to_integral_value(rounding=ROUND_CEILING))


def vector(resources):
    return {k:quantity((resources.get(k.split('.')[0]) or {}).get(k.split('.')[1],0),k.endswith('.cpu'))
            for k in KEYS if k!='pods'}


def plus(a,b):return {k:a.get(k,0)+b.get(k,0) for k in KEYS}
def maximum(a,b):return {k:max(a.get(k,0),b.get(k,0)) for k in KEYS}


def pod_cost(spec):
    # Kubernetes sums application and restartable-init sidecars, while ordinary
    # init containers contribute their peak, not the sum of sequential phases.
    sidecars={};peak={};apps={}
    for c in spec.get('initContainers',[]):
        cost=vector(c.get('resources') or {})
        if c.get('restartPolicy')=='Always':sidecars=plus(sidecars,cost);peak=maximum(peak,sidecars)
        else:peak=maximum(peak,plus(sidecars,cost))
    for c in spec.get('containers',[]):apps=plus(apps,vector(c.get('resources') or {}))
    result=maximum(plus(apps,sidecars),peak)
    overhead=spec.get('overhead') or {}
    for k in KEYS:
        if k!='pods':result[k]+=quantity(overhead.get(k.split('.')[1],0),k.endswith('.cpu'))
    return result | {'pods':1}


def template(spec):
    return {key:[{k:c[k] for k in ('name','resources','restartPolicy') if k in c}
                 for c in spec.get(key,[])] for key in ('containers','initContainers')} | {'overhead':spec.get('overhead') or {}}


def build_observation(store,pods,quotas,limits,at=None):
    if any(q.get('spec',{}).get('scopes') or q.get('spec',{}).get('scopeSelector') for q in quotas):
        raise ValueError('scoped namespace quota needs an explicit capacity model')
    hard={};used={};defaults={};max_container={}
    for q in quotas:
        for source in (q.get('spec',{}).get('hard',{}),q.get('status',{}).get('hard',{})):
            for k,v in source.items():
                if k in KEYS:
                    n=quantity(v,k.endswith('.cpu'));hard[k]=min(hard.get(k,n),n)
        for k,v in q.get('status',{}).get('used',{}).items():
            if k in KEYS:used[k]=max(used.get(k,0),quantity(v,k.endswith('.cpu')))
    for limit in limits:
        for item in limit.get('spec',{}).get('limits',[]):
            if item.get('type')!='Container':continue
            defaults=maximum(defaults,vector({'requests':item.get('defaultRequest',{}),'limits':item.get('default',{})}))
            for k,v in item.get('max',{}).items():
                if k in ('cpu','memory'):
                    n=quantity(v,k=='cpu');max_container[k]=min(max_container.get(k,n),n)
    owners={r['name'] for r in store.db.execute("SELECT name FROM resources WHERE kind='sandbox'")}
    physical={};covered={};supervisor=defaults | {'pods':1};templates=[]
    for pod in pods:
        if pod.get('status',{}).get('phase') in ('Succeeded','Failed'):continue
        cost=pod_cost(pod['spec']);physical=plus(physical,cost)
        meta=pod['metadata'];labels=meta.get('labels') or {}
        name=next((r['name'] for r in meta.get('ownerReferences',[]) if r.get('kind')=='Sandbox' and r.get('name') in owners),None)
        if meta.get('name') in owners:name=meta['name']
        if name:covered[name]=plus(covered.get(name,{}),cost)
        if labels.get('openshell.ai/boundary-role')=='supervisor':supervisor=maximum(supervisor,cost)
        if name and labels.get('openshell.ai/boundary-role')=='workload':
            t=template(pod['spec'])
            if not any(c.get('name')=='agent' for c in t['containers']):raise ValueError('unknown workload container layout')
            if t not in templates:templates.append(t)
    if not templates:
        previous=store.setting('namespace_capacity',{})
        if previous.get('namespace')==store.setting('runtime_namespace') and previous.get('context')==store.setting('runtime_kube_context'):
            templates=previous.get('templates',[])
    if not templates:
        if any(k not in defaults for k in KEYS if k!='pods'):raise ValueError('cannot establish supervisor resource defaults')
        resources={'requests':{},'limits':{}}
        for k,n in defaults.items():
            if k!='pods':resources[k.split('.')[0]][k.split('.')[1]]=f'{n}m' if k.endswith('.cpu') else str(n)
        templates=[{'containers':[{'name':'agent','resources':{}}],
                    'initContainers':[{'name':'namespace-default-init','resources':resources}]}]
    return {'at':time.time() if at is None else at,'context':store.setting('runtime_kube_context'),
            'namespace':store.setting('runtime_namespace'),'hard':hard,'used':maximum(used,physical),
            'covered':covered,'supervisor':supervisor,'templates':templates,'container_max':max_container}


def observe(store,pods=None,force=False):
    if not enabled(store):return
    with (store.directory/'capacity-observation.lock').open('a') as lock:
        try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except BlockingIOError:return
        if not force and time.time()-store.setting('capacity_observed_at',0)<5:return
        store.set_setting('capacity_observed_at',time.time())
        prefix=['kubectl','--context',store.setting('runtime_kube_context'),'--request-timeout=8s','-n',store.setting('runtime_namespace'),'get']
        def read(kinds):
            result=subprocess.run(prefix+[kinds,'-o','json'],capture_output=True,text=True,timeout=12,check=True)
            value=json.loads(result.stdout)
            if value.get('metadata',{}).get('continue'):raise ValueError('partial namespace capacity inventory')
            return value['items']
        try:
            kinds=['resourcequotas,limitranges']+(['pods'] if pods is None else [])
            with ThreadPoolExecutor(max_workers=2) as pool:values=list(pool.map(read,kinds))
            items=values[0]
            quota=[v for v in items if v.get('kind')=='ResourceQuota'];limits=[v for v in items if v.get('kind')=='LimitRange']
            observation=build_observation(store,pods if pods is not None else values[1],quota,limits)
            store.set_setting('namespace_capacity',observation)
            store.db.execute("DELETE FROM settings WHERE key='namespace_capacity_error'")
        except (OSError,ValueError,KeyError,TypeError,subprocess.SubprocessError) as exc:
            store.set_setting('namespace_capacity_error',{'at':time.time(),'kind':type(exc).__name__})


def cost(snapshot,budget):
    workload={}
    for original in snapshot['templates']:
        spec=json.loads(json.dumps(original))
        for c in spec['containers']:
            if c['name']=='agent':c['resources']={k:{'cpu':budget['cpu'],'memory':budget['memory']} for k in ('requests','limits')}
        workload=maximum(workload,pod_cost(spec))
    return plus(workload,snapshot['supervisor'])


def decision(store,work=None,budget=None,replace=(),name=None):
    if not enabled(store):return {'allowed':True,'condition':'Unobserved','message':'Slot admission; namespace observer is not configured'}
    snapshot=store.setting('namespace_capacity',{});error=store.setting('namespace_capacity_error',{})
    if (snapshot.get('context')!=store.setting('runtime_kube_context') or snapshot.get('namespace')!=store.setting('runtime_namespace')
            or not 0<=time.time()-snapshot.get('at',0)<=TTL or error.get('at',0)>=snapshot.get('at',0)):
        return {'allowed':False,'condition':'CapacityUnknown','message':'Waiting for fresh namespace quota observation'}
    if budget is None:
        from .compute import profile
        budget=profile(store,work['task'],work['stage']) if work else {'cpu':'1','memory':'4Gi'}
    for k in ('cpu','memory'):
        if k in snapshot['container_max'] and quantity(budget[k],k=='cpu')>snapshot['container_max'][k]:
            return {'allowed':False,'condition':'ContainerLimit','resource':k,'message':f'Namespace container {k} maximum rejects {budget[k]}'}
    pending={}
    for r in store.compute_resources():
        if r['name'] in replace:continue
        assigned=(json.loads(r['data']).get('compute') or {'cpu':'1','memory':'6Gi'})
        expected=cost(snapshot,assigned);covered=snapshot['covered'].get(r['name'],{})
        pending=plus(pending,{k:max(0,expected[k]-covered.get(k,0)) for k in KEYS})
    used=plus(snapshot['used'],pending);requested=cost(snapshot,budget)
    if name:requested={k:max(0,requested[k]-snapshot['covered'].get(name,{}).get(k,0)) for k in KEYS}
    reserve={}
    if work and work['stage']!='verify':
        # A 100-slot target must not let model jobs fill a 14-worker quota and
        # strand verification. Reserve resources, not just a logical slot.
        from .compute import memory_ceiling
        count=store.verification_reserve()
        target=cost(snapshot,{'cpu':'1','memory':f'{memory_ceiling(store)}Gi'})
        occupied={}
        for r in store.compute_resources():
            row=store.db.execute('SELECT stage FROM work WHERE id=?',(r['work'],)).fetchone()
            if row and row['stage']=='verify':occupied=plus(occupied,cost(snapshot,json.loads(r['data']).get('compute') or {'cpu':'1','memory':'6Gi'}))
        reserve={k:max(0,count*target[k]-occupied.get(k,0)) for k in KEYS}
        requested=plus(requested,reserve)
    headroom={k:n-used.get(k,0) for k,n in snapshot['hard'].items()}
    limiting=next((k for k,n in snapshot['hard'].items() if used.get(k,0)+requested.get(k,0)>n),None)
    message='Namespace quota has room for this allocation'
    if limiting:
        def fmt(n):
            if limiting.endswith('.memory'):return f'{n/1024**3:.2f} GiB'
            if limiting.endswith('.cpu'):return f'{n/1000:g} CPUs'
            return str(n)
        message=f'Namespace quota {limiting}: needs {fmt(requested[limiting])}, available {fmt(max(0,headroom[limiting]))}'
        if reserve.get(limiting,0):message+=' (including reserved verification headroom)'
    return {'allowed':not limiting,'condition':'QuotaLimited' if limiting else 'Ready','resource':limiting,
            'message':message,'hard':snapshot['hard'],'used':used,'headroom':headroom,'requested':requested,
            'verification_reserve':reserve,'observed_at':snapshot['at']}


def summary(store):
    fallback=decision(store)
    if not enabled(store) or fallback['condition']=='CapacityUnknown':return fallback
    from .eligibility import eligible
    blocked=None
    for work in store.ordered_ready():
        if work['stage'] not in ('triage','assess','implement','review','verify') or not eligible(store,work):continue
        value=decision(store,work)
        if value['allowed']:return value
        if blocked is None:blocked=value
    return blocked or fallback
