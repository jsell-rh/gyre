.PHONY: fullsend fullsend-status

# The dispatcher and browser cockpit share .gyre-pipeline/pipeline.sqlite3.
fullsend:
	/usr/bin/python3 scripts/dev-pipeline.py serve

fullsend-status:
	/usr/bin/python3 scripts/dev-pipeline.py status --json
