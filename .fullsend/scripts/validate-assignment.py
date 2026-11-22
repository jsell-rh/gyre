#!/usr/bin/python3
"""Host validator: checkpoint actual source and preserve the stage contract."""
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(os.environ['GYRE_PIPELINE_ROOT']) / 'scripts'))
from pipeline.fullsend import context, collect, owned_sandbox

execution = context(os.environ['GYRE_FULLSEND_CONTEXT'])
result = collect(execution, owned_sandbox(execution), success=True)
execution.store.close()
if not result.get('valid'):
    raise SystemExit('Source guard or stage verdict rejected this assignment')
print('Published claim-specific source checkpoint; controller review and verification remain required.')
