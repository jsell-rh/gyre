#!/usr/bin/env python3
"""Build the human-readable message for a verified task merge."""
import argparse
from pathlib import Path
import re


def frontmatter(body, name):
    match = re.search(rf'^{re.escape(name)}:\s*(.+)$', body.split('---', 2)[1], re.M)
    return match.group(1).strip().strip('"') if match else ''


def section_bullets(body, heading):
    match = re.search(rf'^##+ {re.escape(heading)}\s*$', body, re.M)
    if not match:
        return []
    section = re.split(r'^##+ ', body[match.end():], maxsplit=1, flags=re.M)[0]
    bullets = []
    for line in section.splitlines():
        if re.match(r'^[-*] ', line):
            bullets.append(line[2:].strip())
        elif bullets and line.strip() and not line.lstrip().startswith('#'):
            bullets[-1] += ' ' + line.strip()
    return bullets


def message(task, task_file, candidate):
    body = task_file.read_text()
    title = frontmatter(body, 'title')
    spec = frontmatter(body, 'spec_ref')
    if not title or not spec:
        raise ValueError(f'{task_file}: title and spec_ref are required')
    shipped = section_bullets(body, 'Shipped') or section_bullets(body, 'Implementation Notes')
    lines = [f'Ship {task}: {title}', '', f'Task: specs/tasks/{task}.md',
             f'Spec: {spec}', f'Candidate: {candidate}']
    if shipped:
        lines.extend(['', 'What shipped:', *(f'- {item}' for item in shipped[:5])])
    return '\n'.join(lines) + '\n'


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('task')
    parser.add_argument('candidate')
    args = parser.parse_args()
    if not re.fullmatch(r'task-\d+', args.task) or not re.fullmatch(r'[0-9a-f]{40}', args.candidate):
        parser.error('invalid task or candidate SHA')
    print(message(args.task, Path(f'specs/tasks/{args.task}.md'), args.candidate), end='')
