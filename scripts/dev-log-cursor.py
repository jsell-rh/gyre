#!/usr/bin/env python3
"""Persist attachment offsets while forwarding the original agent stream."""
import os
from pathlib import Path
import re
import sys


def main():
    path = Path(sys.argv[1])
    for line in sys.stdin.buffer:
        match = re.fullmatch(rb'GYRE_REMOTE_LOG_OFFSET (\d+)\n', line)
        if match:
            temporary = path.with_suffix('.tmp')
            temporary.write_text(match[1].decode() + '\n')
            os.replace(temporary, path)
        else:
            sys.stdout.buffer.write(line)
            sys.stdout.buffer.flush()


if __name__ == '__main__':
    main()
