"""Resume evidence, never approval, from an interrupted exact-candidate review."""
import json
from pathlib import PurePosixPath
import re
import tarfile

from .catalog import contract


def restore(execution, repo, bundle):
    claim = execution.claim
    candidate = claim['input']['candidate']
    allowed = re.compile(r'specs/reviews/(?:audit-)?' + re.escape(claim['task']) + r'(?:[.-].*)?')
    for path in sorted(execution.directory.parent.glob('*/outcome.json'),
                       key=lambda p: int(p.parent.name), reverse=True):
        if path.parent == execution.directory:
            continue
        try:
            receipt = json.loads(path.read_text())
            job = json.loads((path.parent / 'bundle/job.json').read_text())
        except (ValueError, OSError):
            continue
        if (not receipt.get('published') or receipt.get('source_valid') is False or
                job.get('stage') != 'review' or job.get('task') != claim['task'] or
                job.get('input') != claim['input']):
            continue
        head = receipt.get('head')
        if not isinstance(head, str) or not re.fullmatch('[0-9a-f]{40}', head):
            continue
        execution.command('git', 'fetch', '--quiet', 'origin', head, cwd=repo, timeout=180)
        changes = execution.command('git', 'diff', '--name-only', candidate, head, cwd=repo).stdout.splitlines()
        task_path = f"specs/tasks/{claim['task']}.md"
        if any(name != task_path and not name.startswith('web/dist/') and not allowed.fullmatch(name)
               for name in changes):
            continue
        before = execution.command('git', 'show', candidate + ':' + task_path, cwd=repo).stdout
        after = execution.command('git', 'show', head + ':' + task_path, cwd=repo).stdout
        try:
            if contract.requirement_parts(before) != contract.requirement_parts(after):
                continue
        except ValueError:
            continue
        notes = []
        for name in changes:
            if not allowed.fullmatch(name):
                continue
            data = execution.command('git', 'show', head + ':' + name, cwd=repo).stdout
            if len(data.encode()) > 256 * 1024:
                continue
            # Copy notes into the input bundle, not the repository: the next
            # source guard snapshots the original immutable candidate.
            target = bundle / 'review-resume' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(data)
            notes.append(str(target.relative_to(bundle)))
        evidence, total = [], 0
        archive = path.parent / 'evidence.tar.gz'
        if archive.exists():
            with tarfile.open(archive) as source:
                for entry in source:
                    name = PurePosixPath(entry.name)
                    if (not entry.isfile() or name.is_absolute() or '..' in name.parts or
                            not name.parts or name.parts[0] != 'review-evidence' or entry.size > 4 * 1024 * 1024):
                        continue
                    total += entry.size
                    if total > 32 * 1024 * 1024:
                        break
                    target = bundle / 'review-resume' / entry.name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with source.extractfile(entry) as data:
                        target.write_bytes(data.read(entry.size))
                    evidence.append(str(target.relative_to(bundle)))
        if not notes and not evidence:
            continue
        manifest = {'candidate': candidate, 'checkpoint': head, 'attempt': path.parent.name,
                    'notes': notes, 'evidence': evidence, 'approved': False,
                    'instruction': 'Interrupted review only. Revalidate evidence against the exact candidate; do not reuse any prior verdict.'}
        (bundle / 'review-resume.json').write_text(json.dumps(manifest))
        execution.store.event('review_resumed', {'candidate': candidate, 'attempt': path.parent.name,
                              'notes': len(notes), 'evidence': len(evidence)}, claim['task'], claim['id'])
        return manifest
    return None
