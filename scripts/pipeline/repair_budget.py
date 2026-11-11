"""Bound unchanged repair cycles across work identities and main advances."""
import hashlib
import json
from pathlib import PurePosixPath

from .execution import Attention


def source_findings(findings):
    if not isinstance(findings,list):return findings,[]
    source=[];generated=[]
    for finding in findings:
        name=finding.get('file','') if isinstance(finding,dict) else ''
        # Only explicit generated paths qualify. General frontend, build, test,
        # security or source findings are never suppressed by text matching.
        path=PurePosixPath(name.split(':',1)[0])
        target=generated if name.startswith('web/dist/') and '..' not in path.parts else source
        target.append(finding)
    return source,generated


def check(execution, task, path, candidate, findings):
    tree=execution.command('git','ls-tree','-r',candidate,cwd=path).stdout
    production='\n'.join(line for line in tree.splitlines() if not line.split('\t',1)[-1].startswith(
        ('specs/tasks/','specs/reviews/','web/dist/')))
    fingerprint=hashlib.sha256(json.dumps([production,findings],sort_keys=True).encode()).hexdigest()
    key='repair_budget:'+task['name']+':'+task['generation']+':'+execution.claim['stage']
    state=execution.store.setting(key,{})
    count=state.get('count',0)+1 if state.get('fingerprint')==fingerprint else 1
    record={'fingerprint':fingerprint,'count':count,'candidate':candidate,'work':execution.claim['id']}
    execution.store.set_setting(key,record)
    if count>=execution.store.setting('repair_stall_limit',3):
        error=Attention('Repair stalled: identical findings and unchanged production source across repeated rounds; retained checkpoint needs diagnosis')
        error.category='repair_stalled';error.diagnostic=record;raise error
