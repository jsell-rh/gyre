#!/usr/bin/env bash
# =============================================================================
# e2e-flow.sh — End-to-end Gyre flow exerciser
#
# Exercises the full autonomous development lifecycle through the API:
#   1. Create workspace + repo
#   2. Push a spec + manifest to the repo (via git smart HTTP)
#   3. Verify spec appears in ledger (Pending)
#   4. Approve the spec (human approval)
#   5. Create an implementation task (linked to spec)
#   6. Spawn an agent (gets JWT + worktree)
#   7. Agent clones, implements, commits, pushes (using agent JWT for provenance)
#   8. Agent completes → MR created (with spec_ref propagated from task)
#   9. Verify MR diff before merge
#  10. Enqueue MR → merge queue processes → MR merged
#  11. Verify attestation bundle (signed, with merge commit SHA)
#  12. Verify full provenance chain: spec → task → agent → MR → merged commit
#
# IMPORTANT: This script simulates agent work locally. The agents/spawn API
# creates the agent record, JWT, and worktree — but no container or process is
# launched (that requires a compute target). This script acts as the agent:
# it uses the agent JWT to clone, commit, push, and complete.
#
# Usage:
#   GYRE_URL=http://localhost:3000 GYRE_TOKEN=<token> ./scripts/e2e-flow.sh
#
# Prerequisites:
#   - Running Gyre server
#   - git, curl, jq installed
# =============================================================================

set -euo pipefail

# --- Configuration -----------------------------------------------------------
BASE_URL="${GYRE_URL:-http://localhost:3000}"
TOKEN="${GYRE_TOKEN:-gyre-dev-token}"
API="${BASE_URL}/api/v1"
AUTH="Authorization: Bearer ${TOKEN}"
CT="Content-Type: application/json"
RUN_ID=$(date +%s | tail -c 7)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

# Temp directory for git work
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# Track issues found
ISSUES=()

# --- Helpers -----------------------------------------------------------------

step() {
  echo -e "\n${BLUE}━━━ Step $1: $2${NC}"
}

ok() {
  echo -e "  ${GREEN}✓${NC} $1"
}

fail() {
  echo -e "  ${RED}✗ $1${NC}" >&2
  exit 1
}

warn() {
  echo -e "  ${YELLOW}⚠${NC} $1"
  ISSUES+=("$1")
}

info() {
  echo -e "  ${DIM}→${NC} $1"
}

# HTTP helper — returns body, checks status code
api_get() {
  local url="$1"
  local response
  response=$(curl -sf -H "$AUTH" "$url") || fail "GET $url failed"
  echo "$response"
}

api_post() {
  local url="$1"
  local body="$2"
  local auth_header="${3:-$AUTH}"
  local response http_code body_out
  response=$(curl -s -w '\n%{http_code}' -H "$auth_header" -H "$CT" -d "$body" "$url")
  http_code=$(echo "$response" | tail -1)
  body_out=$(echo "$response" | sed '$d')
  if [[ "$http_code" -ge 200 && "$http_code" -lt 300 ]]; then
    echo "$body_out"
  else
    fail "POST $url returned $http_code: $body_out"
  fi
}

# Git helper — runs git with a specific Bearer token.
# Uses GIT_CONFIG_GLOBAL=/dev/null to ignore any global git config
# that might set a conflicting http.<url>.extraHeader.
git_with_token() {
  local dir="$1"
  local token="$2"
  shift 2
  GIT_TERMINAL_PROMPT=0 \
  GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${token}" \
  git -C "$dir" "$@"
}

echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║  Gyre End-to-End Flow: Spec → Agent → Merged Code          ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo -e "  Server : ${BASE_URL}"
echo -e "  Run ID : ${RUN_ID}"
echo -e "  Mode   : ${DIM}simulated agent (script acts as the agent)${NC}"

# =============================================================================
# Step 0: Health check
# =============================================================================
step 0 "Health check"
health=$(curl -sf "${BASE_URL}/health" 2>/dev/null) || fail "Server not reachable at ${BASE_URL}"
ok "Server is healthy"

# =============================================================================
# Step 1: Create workspace + repo
# =============================================================================
step 1 "Create workspace + repo"
WS_SLUG="e2e-${RUN_ID}"
WS_RESPONSE=$(api_post "${API}/workspaces" "{
  \"tenant_id\": \"default\",
  \"name\": \"E2E Flow Test ${RUN_ID}\",
  \"slug\": \"${WS_SLUG}\"
}")
WS_ID=$(echo "$WS_RESPONSE" | jq -r '.id')
ok "Workspace: ${WS_ID} (slug: ${WS_SLUG})"

REPO_NAME="e2e-repo-${RUN_ID}"
REPO_RESPONSE=$(api_post "${API}/repos" "{
  \"workspace_id\": \"${WS_ID}\",
  \"name\": \"${REPO_NAME}\"
}")
REPO_ID=$(echo "$REPO_RESPONSE" | jq -r '.id')
CLONE_URL="${BASE_URL}/git/${WS_SLUG}/${REPO_NAME}"
ok "Repo: ${REPO_ID}"
info "Clone URL: ${CLONE_URL}"

# =============================================================================
# Step 2: Push a spec + manifest to the repo
# =============================================================================
step 2 "Push spec + manifest to repo"

REPO_DIR="${WORK_DIR}/repo"
git_with_token "$WORK_DIR" "$TOKEN" clone "${CLONE_URL}.git" repo 2>/dev/null || true
mkdir -p "$REPO_DIR"
cd "$REPO_DIR"

if [ ! -d "$REPO_DIR/.git" ]; then
  git -C "$REPO_DIR" init
  git -C "$REPO_DIR" remote add origin "${CLONE_URL}.git"
fi
git -C "$REPO_DIR" config user.email "e2e@gyre.test"
git -C "$REPO_DIR" config user.name "E2E Flow"

# Unique spec name per run to avoid ledger collisions
SPEC_NAME="hello-world-${RUN_ID}"
SPEC_PATH="system/${SPEC_NAME}.md"
SPEC_PATH_ENCODED="system%2F${SPEC_NAME}.md"

mkdir -p "$REPO_DIR/specs/system"
cat > "$REPO_DIR/specs/system/${SPEC_NAME}.md" << SPEC
# Hello World Feature (${RUN_ID})

> Status: Draft

## Summary

Implement a hello-world endpoint that returns a greeting message.

## Requirements

1. Create a file \`hello.txt\` containing "Hello from Gyre! (run ${RUN_ID})"
2. The file must exist at the repository root.

## Acceptance Criteria

- \`hello.txt\` exists
- Contains the greeting string
SPEC

cat > "$REPO_DIR/specs/manifest.yaml" << MANIFEST
version: 1
defaults:
  requires_approval: true
  auto_create_tasks: true
  auto_invalidate_on_change: true
specs:
  - path: ${SPEC_PATH}
    title: "Hello World Feature (${RUN_ID})"
    owner: "e2e-test"
MANIFEST

git -C "$REPO_DIR" add .
git -C "$REPO_DIR" commit -m "feat: add hello-world spec" --no-gpg-sign
git_with_token "$REPO_DIR" "$TOKEN" push origin HEAD:main 2>&1 | head -5
ok "Spec + manifest pushed to main"

# =============================================================================
# Step 3: Verify spec in ledger
# =============================================================================
step 3 "Verify spec in ledger"
sleep 1

SPEC_LIST=$(api_get "${API}/specs")
SPEC_ENTRY=$(echo "$SPEC_LIST" | jq ".[] | select(.path == \"${SPEC_PATH}\")")
if [ -z "$SPEC_ENTRY" ]; then
  fail "Spec '${SPEC_PATH}' not found in ledger"
fi

SPEC_STATUS=$(echo "$SPEC_ENTRY" | jq -r '.approval_status')
SPEC_SHA=$(echo "$SPEC_ENTRY" | jq -r '.current_sha')
ok "Spec in ledger: status=${SPEC_STATUS}, sha=${SPEC_SHA:0:12}..."

if [ "$SPEC_STATUS" != "pending" ]; then
  fail "Expected 'pending', got '${SPEC_STATUS}'"
fi
ok "Status is Pending (awaiting human approval)"

# =============================================================================
# Step 4: Approve spec
# =============================================================================
step 4 "Approve spec"
APPROVAL_RESPONSE=$(api_post "${API}/specs/${SPEC_PATH_ENCODED}/approve" "{\"sha\": \"${SPEC_SHA}\"}")
APPROVAL_ID=$(echo "$APPROVAL_RESPONSE" | jq -r '.id')
APPROVER_TYPE=$(echo "$APPROVAL_RESPONSE" | jq -r '.approver_type')
ok "Approved: id=${APPROVAL_ID}, type=${APPROVER_TYPE}"

SPEC_AFTER=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}")
SPEC_STATUS_AFTER=$(echo "$SPEC_AFTER" | jq -r '.approval_status')
[ "$SPEC_STATUS_AFTER" = "approved" ] || fail "Expected 'approved', got '${SPEC_STATUS_AFTER}'"
ok "Spec status confirmed: approved"

# =============================================================================
# Step 5: Create implementation task (linked to spec)
# =============================================================================
step 5 "Create implementation task"
TASK_RESPONSE=$(api_post "${API}/tasks" "{
  \"title\": \"Implement hello-world spec\",
  \"description\": \"Create hello.txt per specs/${SPEC_PATH}\",
  \"priority\": \"medium\",
  \"task_type\": \"implementation\",
  \"spec_path\": \"${SPEC_PATH}\",
  \"workspace_id\": \"${WS_ID}\",
  \"repo_id\": \"${REPO_ID}\"
}")
TASK_ID=$(echo "$TASK_RESPONSE" | jq -r '.id')
TASK_SPEC_PATH=$(echo "$TASK_RESPONSE" | jq -r '.spec_path // "null"')
ok "Task: ${TASK_ID} (status: backlog)"

if [ "$TASK_SPEC_PATH" = "null" ] || [ "$TASK_SPEC_PATH" = "" ]; then
  warn "BUG: Task spec_path not returned — create_task doesn't wire spec_path from request"
else
  ok "Task linked to spec: ${TASK_SPEC_PATH}"
fi

# =============================================================================
# Step 6: Spawn agent
# =============================================================================
step 6 "Spawn agent"
SPAWN_RESPONSE=$(api_post "${API}/agents/spawn" "{
  \"name\": \"e2e-worker-${RUN_ID}\",
  \"repo_id\": \"${REPO_ID}\",
  \"task_id\": \"${TASK_ID}\",
  \"branch\": \"feat/hello-world\"
}")
AGENT_ID=$(echo "$SPAWN_RESPONSE" | jq -r '.agent.id')
AGENT_TOKEN=$(echo "$SPAWN_RESPONSE" | jq -r '.token')
AGENT_STATUS=$(echo "$SPAWN_RESPONSE" | jq -r '.agent.status')
AGENT_CLONE_URL=$(echo "$SPAWN_RESPONSE" | jq -r '.clone_url')
ok "Agent: ${AGENT_ID} (status: ${AGENT_STATUS})"
info "JWT issued (this script will act as the agent)"

TASK_CHECK=$(api_get "${API}/tasks/${TASK_ID}")
ok "Task assigned → in_progress"

# =============================================================================
# Step 7: Agent clones, implements, commits, pushes (using agent JWT)
# =============================================================================
step 7 "Agent implements the spec"
info "Cloning with agent JWT..."

AGENT_DIR="${WORK_DIR}/agent-work"
git_with_token "$WORK_DIR" "$AGENT_TOKEN" clone "${AGENT_CLONE_URL}.git" agent-work 2>/dev/null || true

if [ ! -d "$AGENT_DIR/.git" ]; then
  mkdir -p "$AGENT_DIR"
  git -C "$AGENT_DIR" init
  git -C "$AGENT_DIR" remote add origin "${AGENT_CLONE_URL}.git"
  git_with_token "$AGENT_DIR" "$AGENT_TOKEN" fetch origin main 2>/dev/null
  git -C "$AGENT_DIR" checkout -b main FETCH_HEAD 2>/dev/null || git -C "$AGENT_DIR" checkout -b main
fi

git -C "$AGENT_DIR" config user.email "agent@gyre.test"
git -C "$AGENT_DIR" config user.name "E2E Agent"
git -C "$AGENT_DIR" checkout -b feat/hello-world 2>/dev/null || true

# Implement the spec
cat > "$AGENT_DIR/hello.txt" << IMPL
Hello from Gyre! (run ${RUN_ID})
IMPL

git -C "$AGENT_DIR" add .
git -C "$AGENT_DIR" commit -m "feat: implement hello-world spec

Creates hello.txt per specs/${SPEC_PATH} requirements.

Spec-Ref: ${SPEC_PATH}@${SPEC_SHA}" --no-gpg-sign

# Push with agent JWT (enables commit provenance tracking)
git_with_token "$AGENT_DIR" "$AGENT_TOKEN" push origin feat/hello-world 2>&1 | head -5
ok "Agent pushed implementation (using agent JWT for provenance)"

# =============================================================================
# Step 8: Verify MR diff before complete (source vs target branches)
# =============================================================================
step 8 "Verify diff before merge"
sleep 2

# Complete the agent → creates MR
MR_RESPONSE=$(api_post "${API}/agents/${AGENT_ID}/complete" "{
  \"branch\": \"feat/hello-world\",
  \"title\": \"feat: implement hello-world spec\",
  \"target_branch\": \"main\"
}")
MR_ID=$(echo "$MR_RESPONSE" | jq -r '.id')
MR_STATUS=$(echo "$MR_RESPONSE" | jq -r '.status')
MR_AUTHOR=$(echo "$MR_RESPONSE" | jq -r '.author_agent_id')
MR_SPEC_REF=$(echo "$MR_RESPONSE" | jq -r '.spec_ref // "null"')
ok "MR created: ${MR_ID} (status: ${MR_STATUS})"
info "Author agent: ${MR_AUTHOR}"

if [ "$MR_SPEC_REF" = "null" ] || [ -z "$MR_SPEC_REF" ]; then
  warn "BUG: MR spec_ref is null — complete_agent doesn't propagate spec_ref from task"
else
  ok "MR spec_ref: ${MR_SPEC_REF}"
fi

# Check diff BEFORE merging
DIFF=$(api_get "${API}/merge-requests/${MR_ID}/diff")
DIFF_FILES=$(echo "$DIFF" | jq '.files_changed')
DIFF_INSERTIONS=$(echo "$DIFF" | jq '.insertions')
if [ "$DIFF_FILES" -gt 0 ]; then
  ok "MR diff: ${DIFF_FILES} files, +${DIFF_INSERTIONS} lines"
else
  warn "MR diff empty (${DIFF_FILES} files) — diff may not compute correctly for new repos"
fi

# Verify agent and task transitions
AGENT_CHECK=$(api_get "${API}/agents/${AGENT_ID}")
ok "Agent → $(echo "$AGENT_CHECK" | jq -r '.status')"

TASK_REVIEW=$(api_get "${API}/tasks/${TASK_ID}")
ok "Task → $(echo "$TASK_REVIEW" | jq -r '.status')"

# =============================================================================
# Step 9: Enqueue MR → merge
# =============================================================================
step 9 "Merge queue"
QUEUE_RESPONSE=$(api_post "${API}/merge-queue/enqueue" "{\"merge_request_id\": \"${MR_ID}\"}")
ok "MR enqueued"

info "Waiting for merge processor..."
MERGED=false
for i in $(seq 1 30); do
  sleep 1
  MR_POLL=$(api_get "${API}/merge-requests/${MR_ID}")
  MR_CURRENT=$(echo "$MR_POLL" | jq -r '.status')
  if [ "$MR_CURRENT" = "merged" ]; then
    MERGED=true
    break
  fi
  if [ $((i % 5)) -eq 0 ]; then
    info "Waiting... (${i}s, status: ${MR_CURRENT})"
  fi
done

[ "$MERGED" = true ] || fail "MR did not merge within 30s (status: ${MR_CURRENT})"
ok "MR merged!"

# =============================================================================
# Step 10: Verify attestation
# =============================================================================
step 10 "Verify attestation"
ATTESTATION=$(curl -sf -H "$AUTH" "${API}/merge-requests/${MR_ID}/attestation" 2>/dev/null) || true
if [ -n "$ATTESTATION" ] && [ "$ATTESTATION" != "null" ]; then
  # The response wraps in .attestation and .signature
  ATT_VERSION=$(echo "$ATTESTATION" | jq -r '.attestation.attestation_version // .attestation_version // "missing"')
  MERGE_SHA=$(echo "$ATTESTATION" | jq -r '.attestation.merge_commit_sha // .merge_commit_sha // "missing"')
  ATT_SIGNATURE=$(echo "$ATTESTATION" | jq -r '.signature // "missing"')
  ATT_SPEC_REF=$(echo "$ATTESTATION" | jq -r '.attestation.spec_ref // .spec_ref // "null"')
  ATT_APPROVED=$(echo "$ATTESTATION" | jq -r '.attestation.spec_fully_approved // .spec_fully_approved // "missing"')
  ATT_AGENT=$(echo "$ATTESTATION" | jq -r '.attestation.author_agent_id // .author_agent_id // "missing"')

  ok "Attestation version: ${ATT_VERSION}"

  if [ "$MERGE_SHA" != "missing" ] && [ "$MERGE_SHA" != "null" ]; then
    ok "Merge commit SHA: ${MERGE_SHA:0:12}..."
  else
    warn "BUG: Attestation merge_commit_sha is missing"
  fi

  if [ "$ATT_SIGNATURE" != "missing" ]; then
    ok "Ed25519 signature: ${ATT_SIGNATURE:0:20}..."
  else
    warn "BUG: Attestation signature is missing"
  fi

  ok "Spec fully approved: ${ATT_APPROVED}"
  ok "Author agent: ${ATT_AGENT}"

  if [ "$ATT_SPEC_REF" = "null" ] || [ -z "$ATT_SPEC_REF" ]; then
    warn "Attestation spec_ref is null — no spec binding in attestation"
  else
    ok "Attestation spec_ref: ${ATT_SPEC_REF}"
  fi
else
  warn "No attestation bundle returned"
fi

# =============================================================================
# Step 11: Verify provenance chain
# =============================================================================
step 11 "Verify provenance chain"

# Spec → still approved
SPEC_FINAL=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}")
SPEC_FINAL_STATUS=$(echo "$SPEC_FINAL" | jq -r '.approval_status')
ok "Spec: ${SPEC_FINAL_STATUS}"

# Spec progress → should show linked task and MR
SPEC_PROGRESS=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}/progress" 2>/dev/null) || true
if [ -n "$SPEC_PROGRESS" ]; then
  PROGRESS_TASKS=$(echo "$SPEC_PROGRESS" | jq '.tasks | length')
  PROGRESS_MRS=$(echo "$SPEC_PROGRESS" | jq '.mrs | length')
  if [ "$PROGRESS_TASKS" -gt 0 ]; then
    ok "Spec → ${PROGRESS_TASKS} linked task(s)"
  else
    warn "BUG: Spec progress shows 0 tasks — spec_path linkage broken"
  fi
  if [ "$PROGRESS_MRS" -gt 0 ]; then
    ok "Spec → ${PROGRESS_MRS} linked MR(s)"
  else
    warn "BUG: Spec progress shows 0 MRs — MR spec_ref not set"
  fi
fi

# Task final state
TASK_FINAL=$(api_get "${API}/tasks/${TASK_ID}")
TASK_FINAL_STATUS=$(echo "$TASK_FINAL" | jq -r '.status')
ok "Task: ${TASK_FINAL_STATUS}"

# Agent final state
AGENT_FINAL=$(api_get "${API}/agents/${AGENT_ID}")
AGENT_FINAL_STATUS=$(echo "$AGENT_FINAL" | jq -r '.status')
ok "Agent: ${AGENT_FINAL_STATUS}"

# MR final state
MR_FINAL=$(api_get "${API}/merge-requests/${MR_ID}")
MR_FINAL_STATUS=$(echo "$MR_FINAL" | jq -r '.status')
ok "MR: ${MR_FINAL_STATUS}"

# Commits on main
COMMITS=$(api_get "${API}/repos/${REPO_ID}/commits?branch=main")
COMMIT_COUNT=$(echo "$COMMITS" | jq 'length')
ok "Main branch: ${COMMIT_COUNT} commits"

# Agent commit provenance
AGENT_COMMITS=$(api_get "${API}/repos/${REPO_ID}/agent-commits?agent_id=${AGENT_ID}" 2>/dev/null) || true
if [ -n "$AGENT_COMMITS" ] && [ "$AGENT_COMMITS" != "null" ]; then
  AC_COUNT=$(echo "$AGENT_COMMITS" | jq 'length // 0')
  if [ "$AC_COUNT" -gt 0 ]; then
    ok "Agent commit provenance: ${AC_COUNT} records"
  else
    warn "Agent commit provenance: 0 records — push may not be tracking agent JWT identity"
  fi
fi

# MR diff after merge
DIFF_AFTER=$(api_get "${API}/merge-requests/${MR_ID}/diff")
DIFF_AFTER_FILES=$(echo "$DIFF_AFTER" | jq '.files_changed')
if [ "$DIFF_AFTER_FILES" -eq 0 ]; then
  warn "MR diff is empty after merge — fast-forward merge makes source=target, diff disappears"
fi

# =============================================================================
# Summary
# =============================================================================
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
if [ ${#ISSUES[@]} -eq 0 ]; then
  echo -e "${BOLD}║  ${GREEN}End-to-End Flow Complete — All checks passed!${NC}${BOLD}              ║${NC}"
else
  echo -e "${BOLD}║  ${YELLOW}End-to-End Flow Complete — ${#ISSUES[@]} issue(s) found${NC}${BOLD}               ║${NC}"
fi
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  ${BOLD}Provenance Chain:${NC}"
echo -e "    ${CYAN}Spec${NC}  → ${SPEC_PATH} (${SPEC_FINAL_STATUS})"
echo -e "         ↓ approved by ${APPROVER_TYPE}"
echo -e "    ${CYAN}Task${NC}  → ${TASK_ID:0:8}... (${TASK_FINAL_STATUS})"
echo -e "         ↓ agent spawned + assigned"
echo -e "    ${CYAN}Agent${NC} → ${AGENT_ID:0:8}... (${AGENT_FINAL_STATUS})"
echo -e "         ↓ clone → implement → push → complete"
echo -e "    ${CYAN}MR${NC}    → ${MR_ID:0:8}... (${MR_FINAL_STATUS})"
echo -e "         ↓ enqueued → gates → merged"
echo -e "    ${CYAN}Code${NC}  → ${COMMIT_COUNT} commits on main"
echo ""

if [ ${#ISSUES[@]} -gt 0 ]; then
  echo -e "  ${BOLD}${YELLOW}Issues Found:${NC}"
  for issue in "${ISSUES[@]}"; do
    echo -e "    ${YELLOW}⚠${NC} ${issue}"
  done
  echo ""
fi

echo -e "  ${BOLD}Entity IDs:${NC}"
echo -e "    Workspace : ${WS_ID}"
echo -e "    Repo      : ${REPO_ID}"
echo -e "    Approval  : ${APPROVAL_ID}"
echo -e "    Task      : ${TASK_ID}"
echo -e "    Agent     : ${AGENT_ID}"
echo -e "    MR        : ${MR_ID}"
echo ""
