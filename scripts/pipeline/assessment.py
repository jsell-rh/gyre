"""Independent requirement-change assessment; models cannot approve themselves."""
import json
from .execution import Attention


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
    if not isinstance(gaps, list) or (report['decision'] == 'gap') != bool(gaps):
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
        raise Attention('Assessment failed its read-only source guard; inspect the retained outcome')
    from .catalog import contract
    if contract.requirement_parts(result.get('task_body','')) != contract.requirement_parts(task['body']):
        raise Attention('Assessment changed its assigned task contract')
    try:
        report = validate(result.get('assessment'),inp)
    except ValueError as exc:
        raise Attention(str(exc)) from exc
    receipt = report | {'artifact':str(execution.directory/'outcome.json'), 'generation':task['generation']}
    if report['decision'] == 'needs_triage':
        raise Attention('Requirement assessment needs triage: ' + json.dumps(report['evidence']))
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
    tree = execution.command('git','rev-parse','HEAD^{tree}',cwd=path).stdout.strip()
    checked = verify(execution,path,{'candidate':source,'base':inp['base'],'head':source,'tree':tree})
    if not checked['passed']:
        raise Attention('Assessment found no requirement gap, but deterministic verification failed; inspect remote-verification.json before assigning any code repair')
    receipt['verification'] = checked
    updates = {'assessment_pending':None, 'assessment':receipt, 'triaged':task['generation']}
    previous = task['data'].get('previous_delivery')
    if previous:
        updates['delivered'] = previous | {'generation':task['generation'], 'assessment':receipt['artifact']}
    elif source == inp['base']:
        updates['resolved'] = {'reason':'Current requirements independently assessed and verified on main',
                               'source':source,'generation':task['generation'],'assessment':receipt['artifact']}
    else:
        # An unmerged candidate still needs normal independent review and publication.
        updates['review'] = None
    return result, updates, []
