# M12: Quality Gates & Repository Mirroring

## Goal

The merge queue enforces quality gates (tests, lints, reviews) before merging.
External repositories can be mirrored into Gyre for a unified view.

## Problem

The merge queue currently merges MRs without validation. There's no way to
define or run checks. External repos (GitHub, GitLab) can't be imported.

## Deliverables

### M12.1 Quality Gate Framework
- Gate definition: list of checks required before merge (configurable per-repo)
- Built-in gate types: test command, lint command, required approvals count
- Gate execution: spawn a short-lived agent to run the check
- Gate status: pending -> running -> passed/failed
- MR status includes gate results
- Merge queue only merges when all gates pass
- Failed gates block merge and notify the MR author

### M12.2 Repository Mirroring
- `POST /api/v1/repos/mirror` — create a pull mirror from external URL
- Supported sources: any git remote (HTTPS, SSH)
- GitHub App authentication for private repos (optional)
- Periodic sync (configurable interval, default 5 minutes)
- Mirror namespace: `mirrors/{source}/{owner}/{repo}`
- Read-only mirrors — pushes rejected
- Mirror status dashboard in UI

### M12.3 Diff Viewer
- Side-by-side diff view in MR detail
- Syntax highlighting (basic — language detection + token coloring)
- Inline commenting on diff lines
- File tree navigation for multi-file diffs

## Acceptance Criteria

- [ ] MR with failing tests cannot be merged via queue
- [ ] External GitHub repo mirrored and browsable in Gyre
- [ ] Diff viewer shows file changes with syntax highlighting
- [ ] Quality gate results visible in MR detail view
