"""Read-only cgroup counters; never inspect process environment or command text."""
from pathlib import Path
import time


def usage():
    root=Path('/sys/fs/cgroup');value={'at':time.time()}
    for name in ('memory.current','memory.peak','memory.max','cpu.max'):
        try:value[name]= (root/name).read_text().strip()
        except OSError:pass
    for name in ('memory.events','cpu.stat'):
        try:value[name]=dict(line.split() for line in (root/name).read_text().splitlines())
        except (OSError,ValueError):pass
    return value
