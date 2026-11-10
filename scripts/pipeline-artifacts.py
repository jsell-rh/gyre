#!/usr/bin/env python3
"""Canonical generated assets: merge source, rebuild remotely, validate publication."""
import json
from pathlib import Path
import re
import subprocess
import time


def git(repo,*args,check=True):
    result=subprocess.run(['git',*args],cwd=repo,capture_output=True,text=True,timeout=180)
    if check and result.returncode:raise RuntimeError('generated artifact Git operation failed: '+result.stderr[-1000:])
    return result.stdout.strip() if check else result


def restore_generated(repo,base):
    tracked=git(repo,'ls-files','--','web/dist')
    upstream=git(repo,'ls-tree','-r','--name-only',base,'--','web/dist')
    # Clear all generated index stages first, including rename/rename paths
    # absent from the selected base. git restore alone leaves those unmerged.
    if tracked:git(repo,'rm','-r','-f','--ignore-unmatch','--','web/dist')
    if upstream:git(repo,'restore','--source='+base,'--staged','--worktree','--','web/dist')
    git(repo,'clean','-fd','--','web/dist')


def resolve_generated_conflicts(repo,base):
    conflicts=git(repo,'diff','--name-only','--diff-filter=U').splitlines()
    generated=[name for name in conflicts if name.startswith('web/dist/')]
    if generated:restore_generated(repo,base)
    return generated,git(repo,'diff','--name-only','--diff-filter=U').splitlines()


def append_commit(body,sha):
    if not re.fullmatch('[a-f0-9]{40}',sha):raise ValueError('invalid artifact commit')
    parts=body.split('---',2)
    if len(parts)!=3 or parts[0].strip():raise ValueError('task frontmatter missing')
    lines=parts[1].splitlines();start=next((i for i,line in enumerate(lines) if line.startswith('commits:')),len(lines));end=start+1
    while end<len(lines) and lines[end].startswith((' ','\t')):end+=1
    hashes=[]
    if start<len(lines):
        value=lines[start].split(':',1)[1].strip()
        if value.startswith('['):hashes=json.loads(value)
        elif value:raise ValueError('unrecognized task commit list')
        else:
            for line in lines[start+1:end]:
                match=re.fullmatch(r'\s*-\s*[\"\x27]?([a-f0-9]{40})[\"\x27]?\s*',line)
                if not match:raise ValueError('unrecognized task commit entry')
                hashes.append(match[1])
    if not isinstance(hashes,list) or any(not isinstance(h,str) or not re.fullmatch('[a-f0-9]{40}',h) for h in hashes):raise ValueError('invalid task commit list')
    if sha not in hashes:hashes.append(sha)
    lines[start:end]=['commits: '+json.dumps(hashes)]
    parts[1]='\n'.join(lines)+'\n'
    return '---'.join(parts)


def publish(repo,task,branch):
    if not re.fullmatch(r'task-\d+',task) or not re.fullmatch(r'pipeline/verified/[a-f0-9]{32}-\d+',branch):raise ValueError('invalid artifact publication identity')
    git(repo,'config','user.name','gyre-pipeline');git(repo,'config','user.email','jsell-rh@users.noreply.github.com')
    git(repo,'add','-A','--','web/dist')
    changed=git(repo,'diff','--cached','--quiet','--','web/dist',check=False)
    artifact=None
    if changed.returncode==1:
        git(repo,'-c','core.hooksPath=/dev/null','commit','--no-gpg-sign','--only','-m',f'build(web): regenerate verified assets ({task})','--','web/dist')
        artifact=git(repo,'rev-parse','HEAD')
        path=Path(repo)/'specs/tasks'/f'{task}.md';path.write_text(append_commit(path.read_text(),artifact))
        git(repo,'-c','core.hooksPath=/dev/null','commit','--no-gpg-sign','--only','-m',f'process({task}): attribute verified asset build','--',str(path))
    elif changed.returncode:raise RuntimeError('cannot inspect generated assets')
    head,tree=git(repo,'rev-parse','HEAD'),git(repo,'rev-parse','HEAD^{tree}')
    for attempt in range(5):
        try:
            found=git(repo,'ls-remote','--heads','origin',branch).split()
            if found:
                if found[0]!=head:raise ValueError('artifact branch belongs to another head')
                break
            git(repo,'push','--force-with-lease=refs/heads/'+branch+':','origin',head+':refs/heads/'+branch)
            break
        except RuntimeError:
            if attempt==4:raise
            time.sleep(min(20,2**attempt))
    return {'head':head,'tree':tree,'branch':branch,'artifact_commit':artifact}


def validate(repo,identity,publication,task,branch):
    if publication.get('branch')!=branch:raise ValueError('unexpected artifact branch')
    head,tree=publication.get('head'),publication.get('tree')
    if any(not isinstance(value,str) or not re.fullmatch('[a-f0-9]{40}',value) for value in (head,tree)):raise ValueError('invalid artifact object identity')
    if git(repo,'rev-parse',head+'^{tree}')!=tree:raise ValueError('artifact tree mismatch')
    artifact=publication.get('artifact_commit')
    if artifact is None:
        if head!=identity['head'] or tree!=identity['tree']:raise ValueError('unexpected source change without artifact commit')
        return
    if not isinstance(artifact,str) or not re.fullmatch('[a-f0-9]{40}',artifact):raise ValueError('invalid artifact commit')
    if git(repo,'rev-list','--parents','-n','1',artifact).split()!=[artifact,identity['head']]:raise ValueError('artifact commit does not extend tested head')
    paths=git(repo,'diff','--name-only',identity['head'],artifact).splitlines()
    if not paths or any(not path.startswith('web/dist/') for path in paths):raise ValueError('artifact commit changes source')
    if git(repo,'rev-list','--parents','-n','1',head).split()!=[head,artifact]:raise ValueError('artifact attribution ancestry mismatch')
    path=f'specs/tasks/{task}.md'
    if git(repo,'diff','--name-only',artifact,head).splitlines()!=[path]:raise ValueError('artifact attribution changes another file')
    # Preserve exact body whitespace, not git()'s metadata-oriented strip.
    before=subprocess.check_output(['git','show',identity['head']+':'+path],cwd=repo,text=True,timeout=30)
    after=subprocess.check_output(['git','show',head+':'+path],cwd=repo,text=True,timeout=30)
    if after!=append_commit(before,artifact):raise ValueError('artifact attribution changed task requirements or existing commits')
