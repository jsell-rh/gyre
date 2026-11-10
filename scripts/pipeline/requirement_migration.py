"""Conservative repair of pre-section-fingerprint invalidations.

Only a succeeded, ancestral merge observation AND equivalent scoped contracts can
restore currentness. A changed/unprovable contract gets assessment, never coding.
Claims and unrelated findings are preserved. Git reads happen outside transactions.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import tarfile

from .catalog import contract, sync_checkout
from .requirements import snapshot
from .store import Store


def git(root,*args):
    return subprocess.run(['git',*args],cwd=root,text=True,capture_output=True,check=True).stdout


def old_snapshot(root, sha, name, registry):
    with tempfile.TemporaryDirectory() as tmp:
        archive=Path(tmp)/'specs.tar'
        subprocess.run(['git','archive','--output',str(archive),sha,'specs'],cwd=root,check=True,capture_output=True)
        with tarfile.open(archive) as tar:tar.extractall(tmp,filter='data')
        checkout=Path(tmp)
        body=(checkout/'specs/tasks'/f'{name}.md').read_text()
        specs={str(p.relative_to(checkout)):p.read_text() for folder in ('system','development')
               for p in (checkout/'specs'/folder).glob('*.md')}
        legacy=contract.generation(body,specs,(checkout/'specs/GOAL.md').read_text())
        (checkout/'specs/task-spec-inputs.json').write_text(registry)
        return legacy,snapshot(checkout,body,contract),body


def plan(store,root,change_revision=None):
    root=Path(root)
    registry=(root/'specs/task-spec-inputs.json').read_text()
    base=git(root,'rev-parse','HEAD').strip()
    results=[]
    for task in store.tasks():
        data=task['data'];requirements=data.get('requirements') or {}
        if data.get('delivered') or requirements.get('version')!=2 or data.get('requirement_migration'):
            continue
        if not ((data.get('assessment_pending') or {}).get('migration') or
                (data.get('repair') or {}).get('category') == 'requirements_changed' or
                (change_revision and (data.get('legacy_catalog_source') or data.get('catalog_source')) == change_revision)):
            continue
        # Only repair the known whole-document invalidation class.
        if not store.db.execute("SELECT 1 FROM events WHERE task=? AND kind='requirements_changed'",(task['name'],)).fetchone():continue
        if (data.get('repair') or {}).get('category') not in (None,'requirements_changed'):continue
        if store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchone():continue
        receipt=None;historical_body=None;equivalent=False;reason='No reconstructable confirmed merge receipt'
        for row in store.db.execute("SELECT * FROM work WHERE task=? AND stage IN ('observe','publish') AND state='succeeded' ORDER BY updated DESC",(task['name'],)):
            outcome=json.loads(row['result'] or '{}');inp=json.loads(row['input'])
            sha=outcome.get('sha') if outcome.get('status') == 'merged' else outcome.get('merged')
            if not isinstance(sha,str) or not sha:continue
            try:
                git(root,'merge-base','--is-ancestor',sha,base)
                legacy,old,body=old_snapshot(root,sha,task['name'],registry)
                if legacy!=row['generation']:
                    reason='Historical contract does not match the recorded merge generation';continue
                historical_body=body
                receipt={'sha':sha,'generation':row['generation'],'url':inp.get('url') or outcome.get('url'),
                         'reconstructed_from':row['id'],'source':'succeeded merge observation'}
                equivalent=not old.get('errors') and not requirements.get('errors') and old['fingerprint']==requirements['fingerprint']
                reason='Equivalent explicit section inputs' if equivalent else 'Bound requirements differ or cannot resolve'
                break
            except (subprocess.CalledProcessError,FileNotFoundError,ValueError) as exc:
                reason='Historical evidence unavailable: '+type(exc).__name__
        results.append({'task':task['name'],'generation':task['generation'],'expected_data':data,
                        'historical_body':historical_body,'receipt':receipt,'equivalent':equivalent,'reason':reason})
    return results


def apply(store,entries):
    applied=[]
    for entry in entries:
        with store.transaction():
            task=store.task(entry['task'])
            if (task['generation']!=entry['generation'] or task['data']!=entry['expected_data'] or
                    store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchone()):continue
            data=task['data']|{'requirement_migration':{k:v for k,v in entry.items() if k not in ('expected_data','historical_body')}}
            if entry['receipt']:
                data['previous_delivery']=entry['receipt']
                store.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,unixepoch())',
                    (task['name'],entry['receipt']['generation'],entry['historical_body'],json.dumps({'delivered':entry['receipt']})))
            if entry['equivalent']:
                data.update(delivered=entry['receipt']|{'generation':task['generation']},assessment_pending=None,repair=None)
            else:
                data.update(assessment_pending={'migration':True,'changes':{}},repair=None)
            # Retain all work outcomes/checkpoints/PRs. Suppress only unclaimed work.
            store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),task['name']))
            store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE task=? AND state IN ('ready','retry')",(task['name'],))
            store.event('requirement_migration',data['requirement_migration'],task['name'])
            applied.append(task['name'])
    return applied


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state',type=Path,required=True)
    parser.add_argument('--checkout',type=Path,required=True)
    parser.add_argument('--change-revision',help='Restrict optional extra backlog recovery to this recorded catalog source')
    parser.add_argument('--apply',action='store_true')
    args=parser.parse_args();store=Store(args.state)
    try:
        entries=plan(store,args.checkout,args.change_revision)
        print(json.dumps([{k:v for k,v in entry.items() if k not in ('expected_data','historical_body')} for entry in entries],indent=2))
        if args.apply:print(json.dumps({'applied':apply(store,entries)}))
    finally:store.close()

if __name__=='__main__':main()
