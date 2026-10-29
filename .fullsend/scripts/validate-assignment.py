#!/usr/bin/python3
"""Host validator: checkpoint actual source and preserve the stage contract."""
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(os.environ['GYRE_PIPELINE_ROOT']) / 'scripts'))
from pipeline.fullsend import context, collect

execution = context(os.environ['GYRE_FULLSEND_CONTEXT'])
rows = execution.store.db.execute("SELECT name FROM resources WHERE work=? AND kind='sandbox' AND state<>'absent'", (execution.claim['id'],)).fetchall()
if len(rows) != 1:
    raise SystemExit('Expected exactly one owned fullsend sandbox')
result = collect(execution, rows[0]['name'], success=True)
execution.store.close()
if not result.get('valid'):
    raise SystemExit('Source guard or stage verdict rejected this assignment')
print('Published claim-specific source checkpoint; controller review and verification remain required.')
