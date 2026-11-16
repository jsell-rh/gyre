"""Independent verification; serialize only actual upstream merge operations."""
import json
import time


def same_merge_tree(execution, path, base, receipt):
    """Reuse the original tested head only if its resulting merge tree is exact.

    Never rebind a receipt to a new head, or treat unchanged dependencies as
    proof of unchanged code. Changed source, gates, image or Git tree reverify.
    """
    from .verification import policy, COMMANDS
    from .execution import Wait
    if (not receipt.get('passed') or not receipt.get('policy') or receipt.get('engine')!='gateway'
            or receipt.get('policy')!=policy(execution.store)):
        return False
    rows=execution.store.db.execute("SELECT result FROM work WHERE task=? AND generation=? AND stage='verify' AND state='succeeded'",
        (execution.claim['task'],execution.claim['generation']))
    if not any(json.loads(row['result'] or '{}')==receipt for row in rows):return False
    checks=receipt.get('checks') or []
    if not all(isinstance(c,dict) for c in checks) or [c.get('command') for c in checks]!=COMMANDS or any(c.get('exit_code')!=0 for c in checks):return False
    execution.command('git','fetch','--quiet','origin',base,cwd=path,timeout=180)
    if execution.command('git','merge-base','--is-ancestor',receipt['base'],base,cwd=path,check=False).returncode:return False
    if execution.command('git','merge-base','--is-ancestor',base,receipt['head'],cwd=path,check=False).returncode:
        # Static gates also inspect commit history (notably task attribution).
        # Net-equal trees must not hide an untested edit/revert in upstream.
        commits=execution.command('git','rev-list',base,'^'+receipt['head'],cwd=path).stdout.splitlines()
        for commit in commits:
            changed=execution.command('git','diff-tree','--root','--no-commit-id','--name-only','-r','-m',commit,cwd=path).stdout
            if changed.strip():return False
    result=execution.command('git','merge-tree','--write-tree',base,receipt['head'],cwd=path,check=False)
    if result.returncode==1:return False
    if result.returncode or not result.stdout.strip():raise Wait('Cannot establish exact merge tree; retain the verified receipt')
    equal=result.stdout.splitlines()[0]==receipt['tree']
    if equal:execution.store.event('verification_tree_reused',{'head':receipt['head'],'tree':receipt['tree'],
        'tested_base':receipt['base'],'merge_base':base},execution.claim['task'],execution.claim['id'])
    return equal


def resolve_upstream_assessments(store, root, base):
    """A fully verified upstream candidate needs no second review/publication."""
    import subprocess
    from pathlib import Path
    from .verification import COMMANDS
    from .assessment import validate
    resolved=[]
    for task in store.tasks():
        data=task['data'];assessment=data.get('assessment') or {};verified=assessment.get('verification') or {}
        if (data.get('resolved') or data.get('delivered') or data.get('repair') or data.get('assessment_pending')
                or (data.get('requirements') or {}).get('errors')
                or assessment.get('decision')!='still_satisfied' or not verified.get('passed')
                or assessment.get('generation')!=task['generation']
                or assessment.get('requirements')!=(data.get('requirements') or {}).get('fingerprint')):continue
        publication=verified.get('publication') or {}
        if (verified.get('passed') is not True or verified.get('candidate')!=assessment.get('candidate')
                or verified.get('head')!=assessment.get('candidate') or not publication
                or publication.get('artifact_commit') is not None or publication.get('head')!=verified['head']
                or publication.get('tree')!=verified.get('tree')):continue
        checks=verified.get('checks') or []
        if (not all(isinstance(c,dict) for c in checks) or [c.get('command') for c in checks]!=COMMANDS
                or any(c.get('exit_code')!=0 for c in checks)):continue
        proven=False
        for row in store.db.execute("SELECT * FROM work WHERE task=? AND generation=? AND stage='assess' AND state='succeeded'",(task['name'],task['generation'])):
            path=store.directory/'attempts'/row['id']/str(row['token'])
            outcome=json.loads(row['result'] or '{}');report=outcome.get('assessment') or {}
            if (not outcome.get('valid') or not outcome.get('source_valid') or outcome.get('agent_exit')!=0
                    or outcome.get('stream_exit')!=0 or any(report.get(k)!=assessment.get(k) for k in ('candidate','requirements','decision'))):continue
            try:
                validate(assessment,json.loads(row['input']))
                proven=(assessment.get('artifact')==str(path/'outcome.json') and
                    json.loads((path/'remote-verification.json').read_text())==verified)
            except (OSError,ValueError,KeyError,TypeError):continue
            if proven:break
        if not proven:continue
        sources={assessment.get('candidate'),data.get('candidate')}
        try:
            if any(not sha or subprocess.run(['git','merge-base','--is-ancestor',sha,base],cwd=root,
                capture_output=True,timeout=20).returncode for sha in sources):continue
        except (OSError,subprocess.TimeoutExpired):continue
        with store.transaction():
            current=store.task(task['name'])
            if current['generation']!=task['generation'] or current['data']!=data:continue
            if store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchone():continue
            proof={'reason':'Current requirements independently assessed and fully verified on an upstream candidate',
                'source':assessment['candidate'],'generation':task['generation'],'assessment':assessment.get('artifact')}
            store.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,?)',
                (task['name'],task['generation'],task['body'],json.dumps(data),time.time()))
            store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data|{'resolved':proof}),task['name']))
            store.db.execute("UPDATE work SET state='obsolete',token=token+1,updated=? WHERE task=? AND stage<>'cleanup' AND state IN ('ready','retry')",(time.time(),task['name']))
            store.event('upstream_assessment_resolved',proof,task['name']);resolved.append(task['name'])
    return resolved
