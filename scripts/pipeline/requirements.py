"""Explicit, versioned requirement inputs. Missing/ambiguous bindings fail closed.

Registry IDs are stable identities; selectors are maintained alongside spec edits.
Only selected subtrees and their ancestors' introductory text are inherited.
"""
import hashlib
import json
from pathlib import Path
import re


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()).hexdigest()


def sections(text):
    nodes = [{'heading': '', 'path': [], 'level': 0, 'start': 0, 'own': len(text), 'end': len(text)}]
    offset, fence = 0, None
    for line in text.splitlines(True):
        marker = re.match(r'^ {0,3}(`{3,}|~{3,})', line)
        if marker:
            token = marker[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
        elif fence is None:
            match = re.match(r'^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$', line)
            if match:
                level, heading = len(match[1]), match[2]
                parent = next(n for n in reversed(nodes) if n['level'] < level)
                for n in nodes:
                    if n['end'] == len(text) and n['level'] >= level:
                        n['end'] = offset
                if parent['own'] == len(text):
                    parent['own'] = offset
                nodes.append({'heading': heading, 'path': parent['path'] + [heading],
                              'level': level, 'start': offset, 'own': len(text), 'end': len(text)})
        offset += len(line)
    return nodes


def snapshot(checkout, body, contract):
    root = Path(checkout)
    registry_path = root / 'specs/task-spec-inputs.json'
    if not registry_path.exists():
        # Old checkouts retain their original fingerprint until the registry lands.
        specs = {str(p.relative_to(root)): p.read_text() for folder in ('system', 'development')
                 for p in (root / 'specs' / folder).glob('*.md')}
        return {'version': 1, 'fingerprint': contract.generation(body, specs, (root / 'specs/GOAL.md').read_text())}
    registry = json.loads(registry_path.read_text())
    try:
        front, prose = contract.requirement_parts(body)
    except ValueError as exc:
        return {'version':2,'fingerprint':digest(body),'components':{},'inputs':[],
                'errors':[str(exc)]}
    front = re.sub(r'^depends_on:[^\n]*(?:\n[ \t]+-[^\n]*)*', '', front, flags=re.M).strip()
    match = re.search(r'^spec_ref:\s*(.*)$', front, re.M)
    ref = match[1].strip().strip('\"\'') if match else ''
    inputs = registry['bindings'].get(ref)
    errors, components = [], {'task': front + '\n' + prose, 'goal': (root / 'specs/GOAL.md').read_text()}
    if not inputs:
        errors.append('No explicit spec input binding for ' + repr(ref))
    for identity in (inputs or []) + registry.get('shared', []):
        entry = registry['sections'].get(identity)
        if not entry:
            errors.append('Unknown section ID: ' + identity)
            continue
        path = (root / entry['file']).resolve()
        if not path.is_relative_to((root / 'specs').resolve()) or not path.is_file():
            errors.append('Missing spec: ' + identity)
            continue
        text = path.read_text()
        if entry.get('document') is True:
            components[identity] = text
            continue
        nodes = sections(text)
        matches = [n for n in nodes if n['path'] == entry['heading']]
        if len(matches) != 1:
            errors.append('Missing or ambiguous heading: ' + identity)
            continue
        node = matches[0]
        pieces = [text[n['start']:n['own']] for n in nodes if len(n['path']) < len(node['path'])
                  and node['path'][:len(n['path'])] == n['path']]
        components[identity] = '\n'.join(pieces + [text[node['start']:node['end']]])
    return {'version': 2, 'fingerprint': digest({'components': components, 'errors': errors}),
            'components': components, 'errors': errors, 'inputs': inputs or []}


def change(before, after):
    old, new = before.get('components', {}), after.get('components', {})
    return {key: {'before': old.get(key), 'after': new.get(key)}
            for key in sorted(old.keys() | new.keys()) if old.get(key) != new.get(key)}
