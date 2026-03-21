# M16 — Security Hardening

## Goal

Close outstanding CISO findings and harden the server against injection, privilege escalation, and information-disclosure vulnerabilities identified in earlier milestones.

## Deliverables

### M16.1 — Release Automation Endpoint

`POST /api/v1/release/prepare` (Admin only):

- Computes the next semver version from conventional commit history since the last tag
- Generates a changelog with per-commit agent and task attribution
- Optionally opens a release MR (`create_mr: true`, `mr_title: "..."`)
- Request: `{repo_id, branch?, from?, create_mr?, mr_title?}`
- Response: `{next_version, changelog, commit_count, mr?}`

### M16-A — Git Argument Injection Prevention (release endpoint)

The `branch` and `from` parameters on `POST /api/v1/release/prepare` are validated server-side before being passed to git:

- Must not start with `-` (prevents flag injection, e.g. `--exec`)
- Must not contain `..` (prevents range expansion attacks)

Returns `400 Bad Request` if either validation fails. See also: M-8 (SHA hex validation on push).

### M16.2 — Security Finding Resolutions

All Critical, High, and Medium CISO findings identified through M15 were resolved:

| Severity | Finding | Resolution |
|---|---|---|
| Medium | M15.3-A JWT role sync | JWT `realm_access.roles` claim now the authoritative role source |
| Low | M12.3-B AgentReview stub | Documented as intentional stub; Low accepted risk |

No Critical or High findings outstanding as of M16.

## Security Posture at M16

- 0 Critical, 0 High, 1 Medium (M15.3-A — JWT role sync, tracked), 1 Low (M12.3-B — AgentReview stub, accepted)
- Global auth middleware enforced on all `/api/v1/` endpoints
- Diesel ORM eliminates SQL injection class at adapter layer
- SHA hex validation prevents git argument injection on push (`/git/.../git-receive-pack`)
- HTTPS-only mirror URLs prevent SSRF via `POST /api/v1/repos/mirror`
- AdminOnly on gate creation and push-gate configuration
- Server-side repo path computation (no user-supplied paths)
- `mirror_url` credential redaction in all API responses
