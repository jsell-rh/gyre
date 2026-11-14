"""Independent requirement-change assessment; models cannot approve themselves."""
import json
from .execution import Attention


class AssessmentAttention(Attention):
    category = "assessment"


class NeedsTriage(AssessmentAttention):
    category = "requirements_triage"


def validate(report, inputs):
    if not isinstance(report, dict):
        raise ValueError('missing assessment report')
    if report.get('candidate') != (inputs.get('candidate') or inputs['base']):
        raise ValueError('assessment source does not match assignment')
    if report.get('requirements') != inputs['requirements']:
        raise ValueError('assessment requirement fingerprint does not match assignment')
    if report.get('decision') not in ('still_satisfied', 'gap', 'needs_triage'):
        raise ValueError('invalid assessment decision')
    evidence = report.get('evidence')
    if not isinstance(evidence, list) or not evidence or any(
            not isinstance(item, dict) or not all(isinstance(item.get(k), str) and item[k].strip()
            for k in ('requirement', 'file', 'detail')) for item in evidence):
        raise ValueError('assessment needs concrete requirement-to-source evidence')
    if report['decision'] == 'still_satisfied':
        missing = set(inputs.get('required_evidence', [])) - {item['requirement'] for item in evidence}
        if missing:
            raise ValueError('Assessment omits requirement evidence: ' + ', '.join(sorted(missing)))
    gaps = report.get('gaps')
    if (not isinstance(gaps, list) or (report['decision'] == 'gap' and not gaps)
            or (report['decision'] == 'still_satisfied' and gaps)):
        raise ValueError('gap decision must carry concrete findings, and no-gap must not')
    if any(not isinstance(item, dict) or not all(isinstance(item.get(k),str) and item[k].strip()
           for k in ('requirement','file','detail')) for item in gaps):
        raise ValueError('invalid gap finding')
    return report


def run(execution, task):
    from .stages import prompt, checkout
    inp = execution.claim['input']
    result = execution.cloud_step(task, prompt(execution,task))
    if not result.get('valid') or result.get('agent_exit') or result.get('stream_exit'):
        raise AssessmentAttention('Assessment failed its read-only source guard; inspect the retained outcome')
    from .catalog import contract
    if contract.requirement_parts(result.get('task_body','')) != contract.requirement_parts(task['body']):
        raise AssessmentAttention('Assessment changed its assigned task contract')
    try:
        report = validate(result.get('assessment'),inp)
    except ValueError as exc:
        raise AssessmentAttention(str(exc)) from exc
    receipt = report | {'artifact':str(execution.directory/'outcome.json'), 'generation':task['generation']}
    if report['decision'] == 'needs_triage':
        raise NeedsTriage('Requirement assessment needs triage: ' + json.dumps(report['evidence']))
    if report['decision'] == 'gap':
        finding = {'id':execution.claim['id'], 'category':'requirements_gap',
                   'source':inp.get('candidate') or inp['base'], 'details':report['gaps'],
                   'artifact':receipt['artifact']}
        return result, {'assessment_pending':None, 'assessment':receipt, 'repair':finding,
                        'triaged':task['generation']}, [finding]
    # Independent source inspection is necessary but insufficient for currentness.
    # Run the normal deterministic verification gates on the exact assessed tree.
    from .verification import run as verify
    source = inp.get('candidate') or inp['base']
    path = checkout(execution,source)
    retained = retained_contribution(execution,path,source,task['data'].get('candidate'),task['name'])
    if retained:
        # Main meeting the product contract does not erase useful unmerged work.
        # Preserve an exact-candidate review; normal verification/publication
        # must finish before this assignment can become current/delivered.
        receipt['retained_work'] = retained
        return result, {'assessment_pending':None,'assessment':receipt,'resolved':None,
                        'delivered':None,'verified':None,'repair':None,'triaged':task['generation']}, []
    tree = execution.command('git','rev-parse','HEAD^{tree}',cwd=path).stdout.strip()
    checked = verify(execution,path,{'candidate':source,'base':inp['base'],'head':source,'tree':tree})
    if not checked['passed']:
        raise AssessmentAttention('Assessment found no requirement gap, but deterministic verification failed; inspect remote-verification.json before assigning any code repair')
    receipt['verification'] = checked
    updates = {'assessment_pending':None, 'assessment':receipt, 'repair':None, 'triaged':task['generation']}
    previous = task['data'].get('previous_delivery')
    if previous:
        updates['delivered'] = previous | {'generation':task['generation'], 'assessment':receipt['artifact']}
    elif ((checked.get('publication') or {}).get('artifact_commit') is None and
          execution.command('git','merge-base','--is-ancestor',source,inp['base'],cwd=path,check=False).returncode==0):
        updates['resolved'] = {'reason':'Current requirements independently assessed and verified on upstream',
                               'source':source,'generation':task['generation'],'assessment':receipt['artifact']}
    else:
        # An unmerged candidate still needs normal independent review and publication.
        updates['review'] = None
    return result, updates, []


def retained_contribution(execution, path, base, candidate, task_name):
    """Inspect the actual merge contribution, not differences from an old base."""
    if not candidate or candidate == base:
        return None
    execution.command('git','fetch','--quiet','origin',candidate,cwd=path,timeout=180)
    merged = execution.command('git','merge-tree','--write-tree',base,candidate,cwd=path,check=False)
    if merged.returncode == 1:
        return {'candidate':candidate,'base':base,'requires_rebase':True}
    if merged.returncode:
        from .execution import Retry
        raise Retry('Cannot determine retained candidate contribution; preserve the checkpoint')
    tree = merged.stdout.splitlines()[0]
    paths = execution.command('git','diff','--name-only',base,tree,'--',cwd=path).stdout.splitlines()
    paths = [p for p in paths if p != f'specs/tasks/{task_name}.md'
             and not p.startswith(('specs/reviews/','web/dist/'))]
    return {'candidate':candidate,'base':base,'tree':tree,'paths':paths} if paths else None


def resume_retained(store, expected, proof):
    """Backfill an assessment completed by an older executor, without recoding.

    The independent assessment and exact-source review are retained. Normal
    review (if needed), verification and publication gate the saved candidate.
    """
    with store.transaction():
        task=store.task(expected['name']);data=task['data']
        report=data.get('assessment') or {}
        if (task['generation']!=expected['generation'] or data!=expected['data'] or
                (data.get('repair') or {}).get('category') not in (None,'requirements_changed') or
                data.get('candidate')!=proof['candidate'] or not (proof.get('paths') or proof.get('requires_rebase')) or
                report.get('decision')!='still_satisfied' or report.get('generation')!=task['generation'] or
                report.get('requirements')!=(data.get('requirements') or {}).get('fingerprint') or
                store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchone()):
            return False
        store.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,unixepoch())',
            (task['name'],task['generation'],task['body'],json.dumps(data)))
        data=data|{'resolved':None,'delivered':None,'assessment_pending':None,'verified':None,
                   'triaged':task['generation'],'repair':None,'assessment':report|{'retained_work':proof}}
        store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),task['name']))
        store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE task=? AND stage='assess' AND state IN ('ready','retry')",(task['name'],))
        store.event('assessment_retained_work',proof,task['name'])
        return True
