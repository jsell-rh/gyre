"""Private local credential loading shared by controller and fullsend hooks."""
import os
import shlex


def load(store):
    path = store.directory / 'gateway.env'
    if not path.exists():
        return
    if path.stat().st_mode & 0o077:
        raise ValueError('gateway.env must have mode 0600')
    for line in path.read_text().splitlines():
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        key, value = line.removeprefix('export ').split('=', 1)
        if key not in ('OPENSHELL_OIDC_CLIENT_SECRET', 'GH_TOKEN', 'GITHUB_TOKEN', 'ENMAAS_API_KEY'):
            raise ValueError('unsupported credential field')
        words = shlex.split(value)
        if len(words) != 1:
            raise ValueError('invalid credential assignment')
        os.environ.setdefault(key, words[0])
