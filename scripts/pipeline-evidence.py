#!/usr/bin/env python3
"""Fast literal-citation preflight, shared with the controller's audit validator.

This does not approve an audit or replace its schema and independent review.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath


def file_digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def production_source(name):
    """Recognize executable source and public build/deployment configuration."""
    path=PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or not path.parts:return False
    if len(path.parts)>1 and path.parts[0] in ('crates','web','scripts'):return True
    if path.parts[:2]==('.github','workflows'):
        return len(path.parts)==3 and path.suffix in ('.yml','.yaml')
    if path.as_posix() in ('Cargo.toml','Cargo.lock','Dockerfile','flake.nix','flake.lock',
                           'rust-toolchain.toml','.cargo/config.toml'):return True
    if len(path.parts)>1 and path.parts[0]=='docker' and not any(p.startswith('.') for p in path.parts):
        return (path.name in ('Dockerfile','package.json','package-lock.json','flake.lock')
                or path.suffix in ('.nix','.mjs','.sh','.yaml','.yml','.toml'))
    return False


def evidence(root, record):
    if not isinstance(record,dict):raise ValueError('Evidence must be an object')
    name=record.get('file');quote=record.get('quote');detail=record.get('detail')
    if not isinstance(name,str) or not name.strip() or len(name)>500:raise ValueError('Invalid evidence file')
    path=PurePosixPath(name)
    if (path.is_absolute() or '..' in path.parts or not path.parts or
            (path.parts[0] not in ('crates','web','scripts','specs','docs') and not production_source(name))):
        raise ValueError('Evidence path outside repository source')
    local=Path(root)/name
    if not local.is_file() or local.is_symlink() or not local.resolve().is_relative_to(Path(root).resolve()):
        raise ValueError('Evidence file missing or unsafe: '+name)
    if not isinstance(quote,str) or not quote.strip() or len(quote)>2000:raise ValueError('Invalid evidence quote: '+name)
    if not isinstance(detail,str) or not detail.strip() or len(detail)>12000:raise ValueError('Invalid evidence detail: '+name)
    if quote not in local.read_text():raise ValueError('Evidence quote is not present in '+name)
    return {'file':name,'sha256':file_digest(local)}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report',type=Path)
    parser.add_argument('--root',type=Path,default=Path.cwd())
    args=parser.parse_args();report=json.loads(args.report.read_text());errors=[]
    sections=report.get('sections') if isinstance(report,dict) else None
    if not isinstance(sections,list) or not sections:
        print('Expected a spec audit report with nonempty sections');return 2
    for section in sections:
        if not isinstance(section,dict) or not isinstance(section.get('id'),str) or not isinstance(section.get('evidence'),list):
            print('Invalid section/evidence structure');return 2
        for record in section['evidence']:
            try:evidence(args.root,record)
            except ValueError as exc:errors.append(str(section.get('id'))+': '+str(exc))
    if errors:
        for error in errors:print(error)
        return 1
    print('Literal source citations match. Full schema validation and independent review remain required.')
    return 0


if __name__=='__main__':raise SystemExit(main())
