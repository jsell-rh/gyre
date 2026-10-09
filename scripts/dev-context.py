#!/usr/bin/env python3
"""Project current requirements into prompts; leave complete history on disk."""
import argparse
import importlib.util
from pathlib import Path
import re


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


contract = load('dev-contract')


def bounded(text, path, limit=8192):
    if len(text) <= limit:
        return text
    half = limit // 2
    return (text[:half] + f'\n\n[History omitted from prompt. Read {path} for the complete record.]\n\n'
            + text[-half:])


def task_context(text, path, include_history=True):
    _, prose = contract.requirement_parts(text)
    front = text.split('---', 2)[1]
    # Unknown headings remain normative. Reuse generation's exact exclusions
    # so compaction cannot silently drop an acceptance criterion.
    original_sections = re.split(r'(?=^## )', text.split('---', 2)[2], flags=re.M)
    history = []
    for section in original_sections:
        if section.strip() and section not in prose:
            history.append(section)
    projected = '---' + front + '---' + prose
    if history and include_history:
        projected += '\n## Review\n\nCurrent operational history (full file: ' + str(path) + '):\n\n'
        projected += re.sub(r'^## ', '    ## ', bounded('\n'.join(history), path), flags=re.M)
    if contract.generation(projected, {}) != contract.generation(text, {}):
        raise ValueError('context projection changed task requirements')
    return projected


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kind', choices=['task', 'history'])
    parser.add_argument('path', type=Path)
    parser.add_argument('--requirements-only', action='store_true')
    args = parser.parse_args()
    text = args.path.read_text()
    print(task_context(text, args.path, not args.requirements_only) if args.kind == 'task' else bounded(text, args.path))
