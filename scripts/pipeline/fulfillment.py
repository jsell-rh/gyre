"""Recognize implementations already upstream using completed trusted receipts.

A notes-only candidate is not a new implementation. Its independently reviewed,
fully verified upstream base proves fulfillment without inventing a shipped merge.
"""
import json
from pathlib import Path
import re
import subprocess

from .catalog import contract
from .repairs import satisfied
from .verification import COMMANDS


def git(path, *args):
    try:
        return subprocess.run(['git',*args],cwd=path,capture_output=True,text=True,timeout=20)
    except (OSError,subprocess.TimeoutExpired):
        return subprocess.CompletedProcess(['git',*args],1,stdout='',stderr='proof unavailable')


def reconcile(store, checkout, base):
    resolved=[]
    for task in store.tasks():
        data=task['data'];verified=data.get('verified') or {};review=data.get('review') or {}
        if (satisfied(data) or (data.get('requirements') or {}).get('errors') or data.get('assessment_pending') or data.get('repair') or data.get('pr') or
                not verified.get('passed') or verified.get('engine')!='gateway' or
                verified.get('candidate')!=data.get('candidate') or
                review.get('candidate')!=data.get('candidate') or not review.get('approved') or
                verified.get('tested_head')!=verified.get('head')):
            continue
        if any(not re.fullmatch(r'[0-9a-f]{40,64}',str(verified.get(k,''))) for k in ('head','tree','base')):continue
        checks=verified.get('checks') or []
        if (not all(isinstance(c,dict) for c in checks) or [c.get('command') for c in checks]!=COMMANDS or any(c.get('exit_code')!=0 for c in checks)):
            continue
        row=next((r for r in store.db.execute("SELECT * FROM work WHERE task=? AND generation=? AND stage='verify' AND state='succeeded' ORDER BY updated DESC",(task['name'],task['generation']))
                  if json.loads(r['result'] or '{}')==verified),None)
        if not row:continue
        path=(store.directory/'attempts'/row['id']/str(row['token'])/'checkout').resolve()
        if not path.is_relative_to(store.directory) or not (path/'.git').exists():continue
        ancestor=git(checkout,'merge-base','--is-ancestor',verified['base'],base)
        tree=git(path,'rev-parse',verified['head']+'^{tree}')
        changed=git(path,'diff','--name-only',verified['base'],verified['head'],'--')
        document=git(path,'show',verified['head']+':specs/tasks/'+task['name']+'.md')
        if ancestor.returncode or tree.returncode or changed.returncode or document.returncode:continue
        if tree.stdout.strip()!=verified['tree']:continue
        # Include generated web assets: those are production output, not notes.
        if any(p!=f"specs/tasks/{task['name']}.md" and not re.fullmatch(
                r'specs/reviews/(?:audit-)?'+re.escape(task['name'])+r'(?:[.-].*)?',p)
                for p in changed.stdout.splitlines()):continue
        try:
            if contract.requirement_parts(document.stdout)!=contract.requirement_parts(task['body']):continue
        except ValueError:continue
        proof={'kind':'already_upstream','source':verified['base'],'generation':task['generation'],
               'reason':'Implementation already exists upstream; independent review and full remote verification passed. Candidate changes only task/review notes.',
               'verification_work':row['id'],'verified_head':verified['head'],'verified_tree':verified['tree']}
        with store.transaction():
            current=store.task(task['name'])
            if current['generation']!=task['generation'] or current['data']!=data:continue
            active=store.db.execute("SELECT stage,input FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchall()
            if any(r['stage']!='verify' or json.loads(r['input']).get('candidate')!=data['candidate'] for r in active):continue
            store.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,unixepoch())',
                (task['name'],task['generation'],task['body'],json.dumps(data)))
            store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data|{'resolved':proof}),task['name']))
            # Supersede only redundant read-only verification. Source and the
            # completed receipt remain intact; the cleanup reconciler purges it.
            store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE task=? AND generation=? AND stage='verify' AND state IN ('ready','retry','claimed') AND json_extract(input,'$.candidate')=?",
                (task['name'],task['generation'],data['candidate']))
            lane=store.setting('integration_lane')
            if lane and lane['task']==task['name']:
                store.db.execute("DELETE FROM settings WHERE key='integration_lane'")
                store.event('integration_released',{'reason':'already verified upstream'},task['name'])
            store.event('implementation_already_upstream',proof,task['name'])
            resolved.append(task['name'])
    return resolved
