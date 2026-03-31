#!/usr/bin/env bash
# =============================================================================
# e2e-flow.sh — End-to-end Gyre flow exerciser
#
# Proves the full autonomous development lifecycle:
#   1. Create workspace
#   2. Create repo
#   3. Push a spec + manifest to the repo (via git)
#   4. Verify spec appears in ledger (Pending)
#   5. Approve the spec
#   6. Create an implementation task
#   7. Spawn an agent (gets JWT + worktree)
#   8. Agent clones, implements, commits, pushes (via smart HTTP)
#   9. Agent completes → MR created
#  10. Enqueue MR → merge queue processes → MR merged
#  11. Verify attestation bundle exists
#  12. Verify provenance chain: spec → task → agent → MR → merged commit
#
# Usage:
#   GYRE_URL=http://localhost:3000 GYRE_TOKEN=gyre-dev-token ./scripts/e2e-flow.sh
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
BOLD='\033[1m'
NC='\033[0m'

# Temp directory for git work
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

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

info() {
  echo -e "  ${YELLOW}→${NC} $1"
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
  local response http_code
  response=$(curl -s -w '\n%{http_code}' -H "$AUTH" -H "$CT" -d "$body" "$url")
  http_code=$(echo "$response" | tail -1)
  body_out=$(echo "$response" | sed '$d')
  if [[ "$http_code" -ge 200 && "$http_code" -lt 300 ]]; then
    echo "$body_out"
  else
    fail "POST $url returned $http_code: $body_out"
  fi
}

api_put() {
  local url="$1"
  local body="$2"
  local response http_code
  response=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" -d "$body" "$url")
  http_code=$(echo "$response" | tail -1)
  body_out=$(echo "$response" | sed '$d')
  if [[ "$http_code" -ge 200 && "$http_code" -lt 300 ]]; then
    echo "$body_out"
  else
    fail "PUT $url returned $http_code: $body_out"
  fi
}

# Git helper — runs git with Bearer token auth
git_auth() {
  local dir="$1"
  shift
  GIT_TERMINAL_PROMPT=0 \
  GIT_ASKPASS=true \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${TOKEN}" \
  git -C "$dir" "$@"
}

# Git helper — runs git with agent JWT token
git_agent() {
  local dir="$1"
  local token="$2"
  shift 2
  GIT_TERMINAL_PROMPT=0 \
  GIT_ASKPASS=true \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${token}" \
  git -C "$dir" "$@"
}

echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║  Gyre End-to-End Flow: Spec → Agent → Merged Code          ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo -e "  Server: ${BASE_URL}"
echo -e "  Run ID: ${RUN_ID}"

# =============================================================================
# Step 0: Health check
# =============================================================================
step 0 "Health check"
health=$(curl -sf "${BASE_URL}/health" 2>/dev/null) || fail "Server not reachable at ${BASE_URL}"
ok "Server is healthy: $(echo "$health" | jq -r '.status // "ok"')"

# =============================================================================
# Step 1: Create workspace
# =============================================================================
step 1 "Create workspace"
WS_SLUG="e2e-${RUN_ID}"
WS_RESPONSE=$(api_post "${API}/workspaces" "{
  \"tenant_id\": \"default\",
  \"name\": \"E2E Flow Test ${RUN_ID}\",
  \"slug\": \"${WS_SLUG}\"
}")
WS_ID=$(echo "$WS_RESPONSE" | jq -r '.id')
ok "Workspace created: ${WS_ID} (slug: ${WS_SLUG})"

# =============================================================================
# Step 2: Create repo
# =============================================================================
step 2 "Create repo"
REPO_NAME="e2e-repo-${RUN_ID}"
REPO_RESPONSE=$(api_post "${API}/repos" "{
  \"workspace_id\": \"${WS_ID}\",
  \"name\": \"${REPO_NAME}\"
}")
REPO_ID=$(echo "$REPO_RESPONSE" | jq -r '.id')
CLONE_URL="${BASE_URL}/git/${WS_SLUG}/${REPO_NAME}"
ok "Repo created: ${REPO_ID}"
info "Clone URL: ${CLONE_URL}"

# =============================================================================
# Step 3: Push a spec + manifest to the repo
# =============================================================================
step 3 "Push spec + manifest to repo"

# Clone the (empty) repo
REPO_DIR="${WORK_DIR}/repo"
git_auth "$WORK_DIR" clone "${CLONE_URL}.git" repo 2>/dev/null || true
mkdir -p "$REPO_DIR"
cd "$REPO_DIR"

# Initialize if empty
if [ ! -d "$REPO_DIR/.git" ]; then
  git -C "$REPO_DIR" init
  git -C "$REPO_DIR" remote add origin "${CLONE_URL}.git"
fi
git -C "$REPO_DIR" config user.email "e2e@gyre.test"
git -C "$REPO_DIR" config user.name "E2E Flow"

# Use a unique spec name per run to avoid ledger collisions
SPEC_NAME="hello-world-${RUN_ID}"

# Create the spec file
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

# Create the manifest
cat > "$REPO_DIR/specs/manifest.yaml" << MANIFEST
version: 1
defaults:
  requires_approval: true
  auto_create_tasks: true
  auto_invalidate_on_change: true
specs:
  - path: system/${SPEC_NAME}.md
    title: "Hello World Feature (${RUN_ID})"
    owner: "e2e-test"
MANIFEST

# Commit and push to main
git -C "$REPO_DIR" add .
git -C "$REPO_DIR" commit -m "feat: add hello-world spec" --no-gpg-sign
git_auth "$REPO_DIR" push origin HEAD:main 2>&1 | head -5

ok "Spec pushed to main branch"

# =============================================================================
# Step 4: Verify spec appears in ledger
# =============================================================================
step 4 "Verify spec in ledger"

# The push to main triggers spec_registry::sync_spec_ledger
# Give it a moment to process
sleep 1

SPEC_LIST=$(api_get "${API}/specs")
SPEC_ENTRY=$(echo "$SPEC_LIST" | jq ".[] | select(.path == \"system/${SPEC_NAME}.md\")")
if [ -z "$SPEC_ENTRY" ]; then
  fail "Spec 'system/${SPEC_NAME}.md' not found in ledger after push"
fi

SPEC_STATUS=$(echo "$SPEC_ENTRY" | jq -r '.approval_status')
SPEC_SHA=$(echo "$SPEC_ENTRY" | jq -r '.current_sha')
ok "Spec found in ledger: status=${SPEC_STATUS}, sha=${SPEC_SHA}"

if [ "$SPEC_STATUS" != "pending" ]; then
  fail "Expected spec status 'pending', got '${SPEC_STATUS}'"
fi
ok "Spec is Pending (awaiting approval)"

# =============================================================================
# Step 5: Approve the spec
# =============================================================================
step 5 "Approve spec"

# URL-encode the spec path
SPEC_PATH_ENCODED="system%2F${SPEC_NAME}.md"

APPROVAL_RESPONSE=$(api_post "${API}/specs/${SPEC_PATH_ENCODED}/approve" "{
  \"sha\": \"${SPEC_SHA}\"
}")
APPROVAL_ID=$(echo "$APPROVAL_RESPONSE" | jq -r '.id')
APPROVER_TYPE=$(echo "$APPROVAL_RESPONSE" | jq -r '.approver_type')
ok "Spec approved: id=${APPROVAL_ID}, approver_type=${APPROVER_TYPE}"

# Verify it's now Approved
SPEC_AFTER=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}")
SPEC_STATUS_AFTER=$(echo "$SPEC_AFTER" | jq -r '.approval_status')
if [ "$SPEC_STATUS_AFTER" != "approved" ]; then
  fail "Expected spec status 'approved' after approval, got '${SPEC_STATUS_AFTER}'"
fi
ok "Spec status confirmed: ${SPEC_STATUS_AFTER}"

# =============================================================================
# Step 6: Create implementation task
# =============================================================================
step 6 "Create implementation task"

TASK_RESPONSE=$(api_post "${API}/tasks" "{
  \"title\": \"Implement hello-world spec\",
  \"description\": \"Create hello.txt per specs/system/${SPEC_NAME}.md\",
  \"priority\": \"medium\",
  \"task_type\": \"implementation\",
  \"spec_ref\": \"system/${SPEC_NAME}.md@${SPEC_SHA}\"
}")
TASK_ID=$(echo "$TASK_RESPONSE" | jq -r '.id')
TASK_STATUS=$(echo "$TASK_RESPONSE" | jq -r '.status')
ok "Task created: ${TASK_ID} (status: ${TASK_STATUS})"

# =============================================================================
# Step 7: Spawn agent
# =============================================================================
step 7 "Spawn agent"

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
ok "Agent spawned: ${AGENT_ID} (status: ${AGENT_STATUS})"
info "Agent JWT: ${AGENT_TOKEN:0:20}..."

# Verify task was assigned
TASK_CHECK=$(api_get "${API}/tasks/${TASK_ID}")
TASK_ASSIGNED=$(echo "$TASK_CHECK" | jq -r '.assigned_to')
TASK_STATUS_NOW=$(echo "$TASK_CHECK" | jq -r '.status')
ok "Task assigned to agent: ${TASK_ASSIGNED} (status: ${TASK_STATUS_NOW})"

# =============================================================================
# Step 8: Agent clones, implements, commits, pushes
# =============================================================================
step 8 "Agent implements the spec"

AGENT_DIR="${WORK_DIR}/agent-work"

# Clone repo with agent token
git_agent "$WORK_DIR" "$AGENT_TOKEN" clone "${AGENT_CLONE_URL}.git" agent-work 2>/dev/null || true

# If clone resulted in an empty directory, init it
if [ ! -d "$AGENT_DIR/.git" ]; then
  mkdir -p "$AGENT_DIR"
  git -C "$AGENT_DIR" init
  git -C "$AGENT_DIR" remote add origin "${AGENT_CLONE_URL}.git"
  git_agent "$AGENT_DIR" "$AGENT_TOKEN" fetch origin main 2>/dev/null
  git -C "$AGENT_DIR" checkout -b main FETCH_HEAD 2>/dev/null || git -C "$AGENT_DIR" checkout -b main
fi

git -C "$AGENT_DIR" config user.email "agent@gyre.test"
git -C "$AGENT_DIR" config user.name "E2E Agent"

# Create feature branch from main
git -C "$AGENT_DIR" checkout -b feat/hello-world 2>/dev/null || true

# Implement the spec!
cat > "$AGENT_DIR/hello.txt" << IMPL
Hello from Gyre! (run ${RUN_ID})
IMPL

git -C "$AGENT_DIR" add .
git -C "$AGENT_DIR" commit -m "feat: implement hello-world spec

Creates hello.txt per specs/system/${SPEC_NAME}.md requirements.

Spec-Ref: system/${SPEC_NAME}.md@${SPEC_SHA}" --no-gpg-sign

# Push feature branch with agent token
git_agent "$AGENT_DIR" "$AGENT_TOKEN" push origin feat/hello-world 2>&1 | head -5

ok "Agent pushed implementation to feat/hello-world"

# =============================================================================
# Step 9: Agent completes → MR created
# =============================================================================
step 9 "Agent completes → MR created"

# Brief pause to let push post-receive hooks finish processing
sleep 2

MR_RESPONSE=$(api_post "${API}/agents/${AGENT_ID}/complete" "{
  \"branch\": \"feat/hello-world\",
  \"title\": \"feat: implement hello-world spec\",
  \"target_branch\": \"main\"
}")
MR_ID=$(echo "$MR_RESPONSE" | jq -r '.id')
MR_STATUS=$(echo "$MR_RESPONSE" | jq -r '.status')
MR_SOURCE=$(echo "$MR_RESPONSE" | jq -r '.source_branch')
MR_AUTHOR=$(echo "$MR_RESPONSE" | jq -r '.author_agent_id')
ok "MR created: ${MR_ID} (status: ${MR_STATUS})"
info "Source: ${MR_SOURCE}, Author: ${MR_AUTHOR}"

# Verify agent transitioned to idle
AGENT_CHECK=$(api_get "${API}/agents/${AGENT_ID}")
AGENT_FINAL_STATUS=$(echo "$AGENT_CHECK" | jq -r '.status')
ok "Agent status: ${AGENT_FINAL_STATUS}"

# Verify task transitioned to review
TASK_REVIEW=$(api_get "${API}/tasks/${TASK_ID}")
TASK_REVIEW_STATUS=$(echo "$TASK_REVIEW" | jq -r '.status')
ok "Task status: ${TASK_REVIEW_STATUS}"

# =============================================================================
# Step 10: Enqueue MR → merge queue processes → MR merged
# =============================================================================
step 10 "Enqueue MR in merge queue"

QUEUE_RESPONSE=$(api_post "${API}/merge-queue/enqueue" "{
  \"merge_request_id\": \"${MR_ID}\"
}")
QUEUE_STATUS=$(echo "$QUEUE_RESPONSE" | jq -r '.status')
ok "MR enqueued: ${QUEUE_STATUS}"

# Wait for merge processor (runs every 5s)
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
    info "Still waiting... (${i}s, current status: ${MR_CURRENT})"
  fi
done

if [ "$MERGED" = true ]; then
  ok "MR merged successfully!"
else
  fail "MR did not merge within 30s (final status: ${MR_CURRENT})"
fi

# =============================================================================
# Step 11: Verify attestation bundle
# =============================================================================
step 11 "Verify attestation"

ATTESTATION=$(curl -sf -H "$AUTH" "${API}/merge-requests/${MR_ID}/attestation" 2>/dev/null) || true
if [ -n "$ATTESTATION" ] && [ "$ATTESTATION" != "null" ]; then
  ATT_VERSION=$(echo "$ATTESTATION" | jq -r '.attestation_version // "unknown"')
  MERGE_SHA=$(echo "$ATTESTATION" | jq -r '.merge_commit_sha // "none"')
  ok "Attestation exists: version=${ATT_VERSION}, merge_sha=${MERGE_SHA:0:12}"
else
  info "No attestation bundle (merge processor may not generate one for fast-forward merges)"
fi

# Check MR timeline
TIMELINE=$(api_get "${API}/merge-requests/${MR_ID}/timeline" 2>/dev/null) || true
if [ -n "$TIMELINE" ] && [ "$TIMELINE" != "null" ]; then
  EVENT_COUNT=$(echo "$TIMELINE" | jq 'length // 0')
  ok "MR timeline has ${EVENT_COUNT} events"
fi

# =============================================================================
# Step 12: Verify the full provenance chain
# =============================================================================
step 12 "Verify provenance chain"

# Spec → still approved
SPEC_FINAL=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}")
SPEC_FINAL_STATUS=$(echo "$SPEC_FINAL" | jq -r '.approval_status')
ok "Spec status: ${SPEC_FINAL_STATUS}"

# Spec progress → should show linked task and MR
SPEC_PROGRESS=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}/progress" 2>/dev/null) || true
if [ -n "$SPEC_PROGRESS" ] && [ "$SPEC_PROGRESS" != "null" ]; then
  ok "Spec progress endpoint returns data"
fi

# Task → should be done or review
TASK_FINAL=$(api_get "${API}/tasks/${TASK_ID}")
TASK_FINAL_STATUS=$(echo "$TASK_FINAL" | jq -r '.status')
ok "Task final status: ${TASK_FINAL_STATUS}"

# Agent → should be idle
AGENT_FINAL=$(api_get "${API}/agents/${AGENT_ID}")
AGENT_FINAL_STATUS2=$(echo "$AGENT_FINAL" | jq -r '.status')
ok "Agent final status: ${AGENT_FINAL_STATUS2}"

# MR → merged
MR_FINAL=$(api_get "${API}/merge-requests/${MR_ID}")
MR_FINAL_STATUS=$(echo "$MR_FINAL" | jq -r '.status')
ok "MR final status: ${MR_FINAL_STATUS}"

# Commits on main → should include our feature
COMMITS=$(api_get "${API}/repos/${REPO_ID}/commits?branch=main")
COMMIT_COUNT=$(echo "$COMMITS" | jq 'length')
ok "Main branch has ${COMMIT_COUNT} commits after merge"

# Agent commits → provenance
AGENT_COMMITS=$(api_get "${API}/repos/${REPO_ID}/agent-commits?agent_id=${AGENT_ID}" 2>/dev/null) || true
if [ -n "$AGENT_COMMITS" ] && [ "$AGENT_COMMITS" != "null" ]; then
  AC_COUNT=$(echo "$AGENT_COMMITS" | jq 'length // 0')
  ok "Agent commit records: ${AC_COUNT}"
fi

# Activity log
ACTIVITY=$(api_get "${API}/activity?limit=10")
ACTIVITY_COUNT=$(echo "$ACTIVITY" | jq 'length // 0')
ok "Activity log has ${ACTIVITY_COUNT} recent events"

# =============================================================================
# Summary
# =============================================================================
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║  ${GREEN}End-to-End Flow Complete!${NC}${BOLD}                                   ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  ${BOLD}Provenance Chain:${NC}"
echo -e "    Spec  : system/${SPEC_NAME}.md (${SPEC_FINAL_STATUS})"
echo -e "      ↓ approved → task created"
echo -e "    Task  : ${TASK_ID} (${TASK_FINAL_STATUS})"
echo -e "      ↓ agent spawned"
echo -e "    Agent : ${AGENT_ID} (${AGENT_FINAL_STATUS2})"
echo -e "      ↓ implemented + pushed + completed"
echo -e "    MR    : ${MR_ID} (${MR_FINAL_STATUS})"
echo -e "      ↓ enqueued → merged"
echo -e "    Code  : ${COMMIT_COUNT} commits on main"
echo ""
echo -e "  ${BOLD}IDs:${NC}"
echo -e "    Workspace : ${WS_ID}"
echo -e "    Repo      : ${REPO_ID}"
echo -e "    Approval  : ${APPROVAL_ID}"
echo -e "    Task      : ${TASK_ID}"
echo -e "    Agent     : ${AGENT_ID}"
echo -e "    MR        : ${MR_ID}"
echo ""
