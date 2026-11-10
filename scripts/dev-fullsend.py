#!/usr/bin/python3
"""Install the pinned runner and check readiness without admitting model work."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import urllib.request

from pipeline.execution import ROOT
from pipeline.fullsend import binary
from pipeline.store import Store


def install(store):
    lock = json.loads((ROOT / '.fullsend/toolchain.json').read_text())
    arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}.get(platform.machine())
    if not arch or platform.system() != 'Linux':
        raise ValueError('This controller requires a Linux fullsend runner')
    name = f"fullsend_{lock['version']}_linux_{arch}.tar.gz"
    url = f"https://github.com/fullsend-ai/fullsend/releases/download/v{lock['version']}/{name}"
    destination = store.directory / 'tools' / ('fullsend-' + lock['version'])
    destination.parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as directory:
        archive = Path(directory) / name
        with urllib.request.urlopen(url, timeout=120) as response:
            archive.write_bytes(response.read(128 * 1024 * 1024))
        if hashlib.sha256(archive.read_bytes()).hexdigest() != lock[f'linux_{arch}_archive_sha256']:
            raise ValueError('Fullsend release archive checksum mismatch')
        with tarfile.open(archive) as package:
            data = package.extractfile('fullsend').read()
        if hashlib.sha256(data).hexdigest() != lock[f'linux_{arch}_binary_sha256']:
            raise ValueError('Fullsend binary checksum mismatch')
        temporary = destination.with_suffix('.tmp')
        temporary.write_bytes(data)
        temporary.chmod(0o700)
        temporary.replace(destination)
    print('Installed verified fullsend:', destination)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--state', type=Path, default=ROOT / '.gyre-pipeline')
    p.add_argument('command', choices=('install', 'check'))
    p.add_argument('--live', action='store_true', help='also refresh gateway login and probe pinned inference')
    args = p.parse_args()
    store = Store(args.state)
    try:
        if args.command == 'install':
            install(store)
        runner = binary(store)
        result = subprocess.run([str(runner), '--version'], capture_output=True, text=True, timeout=15, check=True)
        print(result.stdout.strip())
        print('OpenShell:', subprocess.check_output([os.environ.get('OPENSHELL', 'openshell'), '--version'], text=True, timeout=15).strip())
        print('Model: gateway/rits/zai-org/glm-5-3; endpoint: https://api.enmaas.devshift.net/v1/chat/completions')
        print('Gateway:', store.setting('gateway', 'gyre-gyre'))
        print('Model admission:', 'paused' if store.setting('admission_paused', False) else 'enabled')
        print('Worker image:', store.setting('fullsend_image', json.loads((ROOT / '.fullsend/toolchain.json').read_text())['worker_image']))
        if args.live:
            # Use the same credential parser as the controller, never source a
            # shell script or emit its contents.
            from pipeline.credentials import load
            load(store)
            from pipeline.execution import gateway
            os.environ.update(OPENSHELL_NO_BROWSER='1', OPENSHELL_GATEWAY_INSECURE='true',
                              GYRE_OPEN_GATEWAY=store.setting('gateway', 'gyre-gyre'))
            gateway.login(store.directory)
            from pipeline.inference import check
            check(store)
            print('Pinned endpoint: Ready (no sandbox allocated)')
    finally:
        store.close()


if __name__ == '__main__':
    main()
