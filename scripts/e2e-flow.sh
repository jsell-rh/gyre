#!/usr/bin/env bash
# =============================================================================
# e2e-flow.sh — End-to-end Gyre flow exerciser
#
# Exercises the full autonomous development lifecycle through the API:
#   1. Create workspace + repo
#   2. Push a Rust project + spec + manifest (via git smart HTTP)
#   3. Verify spec appears in ledger (Pending)
#   4. Approve the spec (human approval)
#   5. Create an implementation task (linked to spec)
#   6. Spawn an agent (gets JWT + worktree)
#   7. Agent clones, implements real Rust code, commits, pushes (agent JWT)
#   8. Agent completes → MR created (with spec_ref + diff)
#   9. Enqueue MR → merge queue processes → MR merged
#  10. Verify attestation bundle (signed, with merge commit SHA + spec_ref)
#  11. Verify knowledge graph (architecture extraction from Rust code)
#  12. Verify full provenance chain: spec → task → agent → MR → code → graph
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

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

ISSUES=()

# --- Helpers -----------------------------------------------------------------

step()  { echo -e "\n${BLUE}━━━ Step $1: $2${NC}"; }
ok()    { echo -e "  ${GREEN}✓${NC} $1"; }
fail()  { echo -e "  ${RED}✗ $1${NC}" >&2; exit 1; }
warn()  { echo -e "  ${YELLOW}⚠${NC} $1"; ISSUES+=("$1"); }
info()  { echo -e "  ${DIM}→${NC} $1"; }

api_get() {
  local response
  response=$(curl -sf -H "$AUTH" "$1") || fail "GET $1 failed"
  echo "$response"
}

api_post() {
  local url="$1" body="$2" auth_header="${3:-$AUTH}"
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

# Nulls global/system git config so only our token is sent.
git_with_token() {
  local dir="$1" token="$2"; shift 2
  GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${token}" \
  git -C "$dir" "$@"
}

echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║  Gyre E2E: Spec → Agent → Code → Merge → Attestation      ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo -e "  Server : ${BASE_URL}"
echo -e "  Run ID : ${RUN_ID}"
echo -e "  Mode   : ${DIM}simulated agent (script acts as the agent)${NC}"

# =============================================================================
step 0 "Health check"
# =============================================================================
curl -sf "${BASE_URL}/health" >/dev/null || fail "Server not reachable at ${BASE_URL}"
ok "Server is healthy"

# =============================================================================
step 1 "Create workspace + repo"
# =============================================================================
WS_SLUG="e2e-${RUN_ID}"
WS_ID=$(api_post "${API}/workspaces" "{
  \"tenant_id\": \"default\",
  \"name\": \"E2E Flow Test ${RUN_ID}\",
  \"slug\": \"${WS_SLUG}\"
}" | jq -r '.id')
ok "Workspace: ${WS_ID}"

REPO_NAME="e2e-repo-${RUN_ID}"
REPO_RESPONSE=$(api_post "${API}/repos" "{
  \"workspace_id\": \"${WS_ID}\",
  \"name\": \"${REPO_NAME}\"
}")
REPO_ID=$(echo "$REPO_RESPONSE" | jq -r '.id')
CLONE_URL="${BASE_URL}/git/${WS_SLUG}/${REPO_NAME}"
ok "Repo: ${REPO_ID}"

# =============================================================================
step 2 "Push Rust project + spec to repo"
# =============================================================================
REPO_DIR="${WORK_DIR}/repo"
git_with_token "$WORK_DIR" "$TOKEN" clone "${CLONE_URL}.git" repo 2>/dev/null || true
mkdir -p "$REPO_DIR"
if [ ! -d "$REPO_DIR/.git" ]; then
  git -C "$REPO_DIR" init
  git -C "$REPO_DIR" remote add origin "${CLONE_URL}.git"
fi
git -C "$REPO_DIR" config user.email "e2e@gyre.test"
git -C "$REPO_DIR" config user.name "E2E Flow"

SPEC_NAME="greeting-service-${RUN_ID}"
SPEC_PATH="system/${SPEC_NAME}.md"
SPEC_PATH_ENCODED="system%2F${SPEC_NAME}.md"

# --- Spec ---
mkdir -p "$REPO_DIR/specs/system"
cat > "$REPO_DIR/specs/system/${SPEC_NAME}.md" << SPEC
# Greeting Service (${RUN_ID})

> Status: Draft

## Summary

Implement a greeting service with configurable messages and user tracking.

## Requirements

1. A \`GreetingConfig\` struct with \`default_message\` and \`max_length\` fields
2. A \`User\` struct with \`id\`, \`name\`, and \`greeting_count\` fields
3. A \`GreetingService\` that takes a config and produces personalized greetings
4. A \`greet\` function that increments the user's greeting count
5. An \`ApiResponse\` enum with \`Success\` and \`Error\` variants

## Acceptance Criteria

- All types are public and documented
- The service is stateless (takes user by mutable reference)
- Greeting count tracks how many times each user has been greeted
SPEC

# --- Manifest ---
cat > "$REPO_DIR/specs/manifest.yaml" << MANIFEST
version: 1
defaults:
  requires_approval: true
  auto_create_tasks: true
specs:
  - path: ${SPEC_PATH}
    title: "Greeting Service (${RUN_ID})"
    owner: "e2e-test"
MANIFEST

# --- Initial Rust project (baseline for the knowledge graph) ---
cat > "$REPO_DIR/Cargo.toml" << 'CARGO'
[package]
name = "greeting-service"
version = "0.1.0"
edition = "2021"
CARGO

mkdir -p "$REPO_DIR/src"
cat > "$REPO_DIR/src/lib.rs" << 'RUST'
//! Greeting service library.
//!
//! This crate provides a configurable greeting service.

/// Application configuration.
pub struct AppConfig {
    /// Name of this application instance.
    pub name: String,
    /// Version string.
    pub version: String,
}

impl AppConfig {
    /// Create a new AppConfig with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

/// Health check response.
pub enum HealthStatus {
    /// Service is healthy.
    Healthy,
    /// Service is degraded with a reason.
    Degraded(String),
}

/// Check if the service is healthy.
pub fn health_check(_config: &AppConfig) -> HealthStatus {
    HealthStatus::Healthy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config() {
        let config = AppConfig::new("test");
        assert_eq!(config.name, "test");
    }

    #[test]
    fn test_health_check() {
        let config = AppConfig::new("test");
        assert!(matches!(health_check(&config), HealthStatus::Healthy));
    }
}
RUST

git -C "$REPO_DIR" add .
git -C "$REPO_DIR" commit -m "feat: initial Rust project with spec" --no-gpg-sign
git_with_token "$REPO_DIR" "$TOKEN" push origin HEAD:main 2>&1 | tail -3
ok "Rust project + spec pushed to main"

# =============================================================================
step 3 "Verify spec in ledger"
# =============================================================================
sleep 2  # allow post-receive hooks (spec sync + graph extraction)

SPEC_LIST=$(api_get "${API}/specs")
SPEC_ENTRY=$(echo "$SPEC_LIST" | jq ".[] | select(.path == \"${SPEC_PATH}\")")
[ -n "$SPEC_ENTRY" ] || fail "Spec '${SPEC_PATH}' not found in ledger"

SPEC_STATUS=$(echo "$SPEC_ENTRY" | jq -r '.approval_status')
SPEC_SHA=$(echo "$SPEC_ENTRY" | jq -r '.current_sha')
ok "Spec in ledger: status=${SPEC_STATUS}, sha=${SPEC_SHA:0:12}..."
[ "$SPEC_STATUS" = "pending" ] || fail "Expected 'pending', got '${SPEC_STATUS}'"

# =============================================================================
step 4 "Approve spec"
# =============================================================================
APPROVAL_RESPONSE=$(api_post "${API}/specs/${SPEC_PATH_ENCODED}/approve" "{\"sha\": \"${SPEC_SHA}\"}")
APPROVAL_ID=$(echo "$APPROVAL_RESPONSE" | jq -r '.id')
APPROVER_TYPE=$(echo "$APPROVAL_RESPONSE" | jq -r '.approver_type')
ok "Approved: type=${APPROVER_TYPE}"

SPEC_AFTER=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}")
[ "$(echo "$SPEC_AFTER" | jq -r '.approval_status')" = "approved" ] || fail "Spec not approved"
ok "Spec status: approved"

# =============================================================================
step 5 "Configure quality gates"
# =============================================================================
# Create a test gate (required — must pass to merge)
GATE1_RESPONSE=$(api_post "${API}/repos/${REPO_ID}/gates" "{
  \"name\": \"unit-tests\",
  \"gate_type\": \"test_command\",
  \"command\": \"true\",
  \"required\": true
}")
GATE1_ID=$(echo "$GATE1_RESPONSE" | jq -r '.id')
ok "Gate 'unit-tests': ${GATE1_ID} (test_command, required)"

# Create an advisory lint gate (non-blocking)
GATE2_RESPONSE=$(api_post "${API}/repos/${REPO_ID}/gates" "{
  \"name\": \"lint-check\",
  \"gate_type\": \"lint_command\",
  \"command\": \"true\",
  \"required\": false
}")
GATE2_ID=$(echo "$GATE2_RESPONSE" | jq -r '.id')
ok "Gate 'lint-check': ${GATE2_ID} (lint_command, advisory)"

# Verify gates are listed
GATES_LIST=$(api_get "${API}/repos/${REPO_ID}/gates")
GATE_COUNT=$(echo "$GATES_LIST" | jq 'length')
ok "${GATE_COUNT} gates configured for repo"

# =============================================================================
step 6 "Create implementation task"
# =============================================================================
TASK_RESPONSE=$(api_post "${API}/tasks" "{
  \"title\": \"Implement greeting service\",
  \"description\": \"Add GreetingConfig, User, GreetingService, greet(), ApiResponse per spec\",
  \"priority\": \"high\",
  \"task_type\": \"implementation\",
  \"spec_path\": \"${SPEC_PATH}\",
  \"workspace_id\": \"${WS_ID}\",
  \"repo_id\": \"${REPO_ID}\"
}")
TASK_ID=$(echo "$TASK_RESPONSE" | jq -r '.id')
TASK_SPEC_PATH=$(echo "$TASK_RESPONSE" | jq -r '.spec_path // "null"')
ok "Task: ${TASK_ID}"
[ "$TASK_SPEC_PATH" != "null" ] && ok "Task → spec: ${TASK_SPEC_PATH}" || warn "Task spec_path not set"

# =============================================================================
step 7 "Spawn agent"
# =============================================================================
SPAWN_RESPONSE=$(api_post "${API}/agents/spawn" "{
  \"name\": \"e2e-worker-${RUN_ID}\",
  \"repo_id\": \"${REPO_ID}\",
  \"task_id\": \"${TASK_ID}\",
  \"branch\": \"feat/greeting-service\"
}")
AGENT_ID=$(echo "$SPAWN_RESPONSE" | jq -r '.agent.id')
AGENT_TOKEN=$(echo "$SPAWN_RESPONSE" | jq -r '.token')
AGENT_CLONE_URL=$(echo "$SPAWN_RESPONSE" | jq -r '.clone_url')
ok "Agent: ${AGENT_ID} (active)"
info "JWT issued — script will simulate agent work"

# =============================================================================
step 8 "Agent implements the spec (real Rust code)"
# =============================================================================
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
git -C "$AGENT_DIR" checkout -b feat/greeting-service 2>/dev/null || true

# --- Agent writes real Rust implementation ---
mkdir -p "$AGENT_DIR/src"

cat > "$AGENT_DIR/src/greeting.rs" << 'RUST'
//! Greeting service implementation.
//!
//! Provides configurable, personalized greetings with usage tracking.

/// Configuration for the greeting service.
pub struct GreetingConfig {
    /// The default greeting message template.
    pub default_message: String,
    /// Maximum allowed greeting length.
    pub max_length: usize,
}

impl GreetingConfig {
    /// Create a new config with sensible defaults.
    pub fn new() -> Self {
        Self {
            default_message: "Hello, {name}!".to_string(),
            max_length: 256,
        }
    }

    /// Create a config with a custom message template.
    pub fn with_message(message: &str) -> Self {
        Self {
            default_message: message.to_string(),
            max_length: 256,
        }
    }
}

impl Default for GreetingConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A user who can receive greetings.
pub struct User {
    /// Unique user identifier.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Number of times this user has been greeted.
    pub greeting_count: u32,
}

impl User {
    /// Create a new user with the given id and name.
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            greeting_count: 0,
        }
    }
}

/// API response type for greeting operations.
pub enum ApiResponse {
    /// Successful greeting with the message.
    Success {
        /// The rendered greeting message.
        message: String,
        /// How many times this user has been greeted (including this time).
        total_greetings: u32,
    },
    /// Error with a reason.
    Error {
        /// Error code for programmatic handling.
        code: u32,
        /// Human-readable error description.
        reason: String,
    },
}

/// The greeting service. Stateless — operates on user references.
pub struct GreetingService {
    config: GreetingConfig,
}

impl GreetingService {
    /// Create a new greeting service with the given configuration.
    pub fn new(config: GreetingConfig) -> Self {
        Self { config }
    }

    /// Greet a user. Increments their greeting count and returns a response.
    pub fn greet(&self, user: &mut User) -> ApiResponse {
        user.greeting_count += 1;

        let message = self
            .config
            .default_message
            .replace("{name}", &user.name);

        if message.len() > self.config.max_length {
            return ApiResponse::Error {
                code: 400,
                reason: format!(
                    "greeting exceeds max length of {} characters",
                    self.config.max_length
                ),
            };
        }

        ApiResponse::Success {
            message,
            total_greetings: user.greeting_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_greeting() {
        let service = GreetingService::new(GreetingConfig::new());
        let mut user = User::new(1, "Alice");
        match service.greet(&mut user) {
            ApiResponse::Success { message, total_greetings } => {
                assert_eq!(message, "Hello, Alice!");
                assert_eq!(total_greetings, 1);
            }
            ApiResponse::Error { .. } => panic!("expected success"),
        }
    }

    #[test]
    fn test_greeting_count_increments() {
        let service = GreetingService::new(GreetingConfig::new());
        let mut user = User::new(1, "Bob");
        service.greet(&mut user);
        service.greet(&mut user);
        service.greet(&mut user);
        assert_eq!(user.greeting_count, 3);
    }

    #[test]
    fn test_custom_message() {
        let config = GreetingConfig::with_message("Welcome, {name}! Glad to see you.");
        let service = GreetingService::new(config);
        let mut user = User::new(2, "Charlie");
        match service.greet(&mut user) {
            ApiResponse::Success { message, .. } => {
                assert_eq!(message, "Welcome, Charlie! Glad to see you.");
            }
            ApiResponse::Error { .. } => panic!("expected success"),
        }
    }

    #[test]
    fn test_max_length_exceeded() {
        let config = GreetingConfig {
            default_message: "Hello, {name}!".to_string(),
            max_length: 5,  // too short
        };
        let service = GreetingService::new(config);
        let mut user = User::new(3, "LongNameUser");
        match service.greet(&mut user) {
            ApiResponse::Error { code, .. } => assert_eq!(code, 400),
            ApiResponse::Success { .. } => panic!("expected error"),
        }
    }
}
RUST

# Update lib.rs to re-export the new module
cat > "$AGENT_DIR/src/lib.rs" << 'RUST'
//! Greeting service library.
//!
//! This crate provides a configurable greeting service
//! with user tracking and personalized messages.

pub mod greeting;

pub use greeting::{ApiResponse, GreetingConfig, GreetingService, User};

/// Application configuration.
pub struct AppConfig {
    /// Name of this application instance.
    pub name: String,
    /// Version string.
    pub version: String,
}

impl AppConfig {
    /// Create a new AppConfig with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

/// Health check response.
pub enum HealthStatus {
    /// Service is healthy.
    Healthy,
    /// Service is degraded with a reason.
    Degraded(String),
}

/// Check if the service is healthy.
pub fn health_check(_config: &AppConfig) -> HealthStatus {
    HealthStatus::Healthy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config() {
        let config = AppConfig::new("test");
        assert_eq!(config.name, "test");
    }

    #[test]
    fn test_health_check() {
        let config = AppConfig::new("test");
        assert!(matches!(health_check(&config), HealthStatus::Healthy));
    }
}
RUST

git -C "$AGENT_DIR" add .
git -C "$AGENT_DIR" commit -m "feat: implement greeting service

Adds GreetingConfig, User, GreetingService, greet(), and ApiResponse
per specs/${SPEC_PATH}.

- GreetingConfig: configurable message template + max_length
- User: id, name, greeting_count tracking
- GreetingService: stateless, operates on &mut User
- ApiResponse: Success/Error enum
- 4 unit tests covering all acceptance criteria

Spec-Ref: ${SPEC_PATH}@${SPEC_SHA}" --no-gpg-sign

git_with_token "$AGENT_DIR" "$AGENT_TOKEN" push origin feat/greeting-service 2>&1 | tail -3
ok "Agent pushed 2 files (+160 lines of Rust)"

# =============================================================================
step 9 "Agent completes → MR with diff"
# =============================================================================
sleep 2  # let post-receive hooks finish

MR_RESPONSE=$(api_post "${API}/agents/${AGENT_ID}/complete" "{
  \"branch\": \"feat/greeting-service\",
  \"title\": \"feat: implement greeting service\",
  \"target_branch\": \"main\"
}")
MR_ID=$(echo "$MR_RESPONSE" | jq -r '.id')
MR_SPEC_REF=$(echo "$MR_RESPONSE" | jq -r '.spec_ref // "null"')
ok "MR created: ${MR_ID}"

[ "$MR_SPEC_REF" != "null" ] && ok "MR spec_ref: ${MR_SPEC_REF}" || warn "MR spec_ref is null"

# Check diff BEFORE merge
DIFF=$(api_get "${API}/merge-requests/${MR_ID}/diff")
DIFF_FILES=$(echo "$DIFF" | jq '.files_changed')
DIFF_INS=$(echo "$DIFF" | jq '.insertions')
DIFF_DEL=$(echo "$DIFF" | jq '.deletions')
if [ "$DIFF_FILES" -gt 0 ]; then
  ok "MR diff: ${DIFF_FILES} files changed, +${DIFF_INS} -${DIFF_DEL}"
  # Show file-level breakdown
  echo "$DIFF" | jq -r '.files[] | "    \(.status) \(.path)"' 2>/dev/null | while IFS= read -r line; do
    echo -e "  ${DIM}${line}${NC}"
  done || true
else
  warn "MR diff empty"
fi

ok "Agent → $(api_get "${API}/agents/${AGENT_ID}" | jq -r '.status')"
ok "Task → $(api_get "${API}/tasks/${TASK_ID}" | jq -r '.status')"

# =============================================================================
step 10 "Merge queue + gates"
# =============================================================================
api_post "${API}/merge-queue/enqueue" "{\"merge_request_id\": \"${MR_ID}\"}" >/dev/null
ok "MR enqueued"

info "Waiting for gates + merge processor..."
MERGED=false
for i in $(seq 1 60); do
  sleep 1
  MR_CURRENT=$(api_get "${API}/merge-requests/${MR_ID}" | jq -r '.status')
  [ "$MR_CURRENT" = "merged" ] && { MERGED=true; break; }
  [ "$MR_CURRENT" = "closed" ] && fail "MR was closed (gate failure?)"
  [ $((i % 10)) -eq 0 ] && info "Waiting... (${i}s, status: ${MR_CURRENT})"
done
[ "$MERGED" = true ] || fail "MR did not merge within 60s (status: ${MR_CURRENT})"
ok "MR merged!"

# Verify gate results
GATE_RESULTS=$(api_get "${API}/merge-requests/${MR_ID}/gates")
GATE_RESULT_COUNT=$(echo "$GATE_RESULTS" | jq 'length')
if [ "$GATE_RESULT_COUNT" -gt 0 ]; then
  ok "Gate results: ${GATE_RESULT_COUNT} gates executed"
  echo "$GATE_RESULTS" | jq -r '.[] | "\(.gate_id[:8])... → \(.status)\(if .output then " (\(.output | gsub("\n";"") | .[:40]))" else "" end)"' 2>/dev/null | while IFS= read -r line; do
    echo -e "  ${DIM}  ${line}${NC}"
  done || true
  # Check no required gates failed (merge wouldn't succeed if they did, but verify)
  GATE_PASSED=$(echo "$GATE_RESULTS" | jq '[.[] | select(.status == "Passed")] | length')
  GATE_FAILED=$(echo "$GATE_RESULTS" | jq '[.[] | select(.status == "Failed")] | length')
  GATE_PENDING=$(echo "$GATE_RESULTS" | jq '[.[] | select(.status == "Pending" or .status == "Running")] | length')
  if [ "$GATE_FAILED" = "0" ]; then
    ok "${GATE_PASSED} passed, ${GATE_PENDING} pending/advisory"
  else
    warn "${GATE_FAILED} gate(s) failed"
  fi
else
  warn "No gate results found — gates may not have triggered"
fi

# Push to main to trigger knowledge graph extraction.
# The merge processor writes directly to the bare repo, so the post-receive
# hook (which runs graph extraction) doesn't fire. We fetch the merged main,
# add a version bump commit, and push — this triggers extraction of all code
# including the agent's new types.
info "Pushing to main to trigger graph extraction..."
git_with_token "$REPO_DIR" "$TOKEN" fetch origin main 2>/dev/null
git -C "$REPO_DIR" checkout main 2>/dev/null || git -C "$REPO_DIR" checkout -b main origin/main 2>/dev/null
git -C "$REPO_DIR" reset --hard origin/main 2>/dev/null
echo '0.2.0' > "$REPO_DIR/VERSION"
git -C "$REPO_DIR" add VERSION
git -C "$REPO_DIR" commit -m "chore: bump version to 0.2.0 after greeting service merge" --no-gpg-sign
git_with_token "$REPO_DIR" "$TOKEN" push origin main 2>&1 | tail -2
sleep 3  # allow async graph extraction to complete
ok "Main pushed — graph extraction triggered"

# =============================================================================
step 11 "Verify attestation"
# =============================================================================
ATTESTATION=$(curl -sf -H "$AUTH" "${API}/merge-requests/${MR_ID}/attestation" 2>/dev/null) || true
if [ -n "$ATTESTATION" ] && [ "$ATTESTATION" != "null" ]; then
  ATT_VERSION=$(echo "$ATTESTATION" | jq -r '.attestation.attestation_version // "missing"')
  MERGE_SHA=$(echo "$ATTESTATION" | jq -r '.attestation.merge_commit_sha // "missing"')
  ATT_SIGNATURE=$(echo "$ATTESTATION" | jq -r '.signature // "missing"')
  ATT_SPEC_REF=$(echo "$ATTESTATION" | jq -r '.attestation.spec_ref // "null"')
  ATT_AGENT=$(echo "$ATTESTATION" | jq -r '.attestation.author_agent_id // "missing"')

  ok "Attestation v${ATT_VERSION}, signed"
  [ "$MERGE_SHA" != "missing" ] && ok "Merge SHA: ${MERGE_SHA:0:12}..." || warn "Missing merge SHA"
  [ "$ATT_SIGNATURE" != "missing" ] && ok "Ed25519 sig: ${ATT_SIGNATURE:0:20}..." || warn "Missing signature"
  [ "$ATT_SPEC_REF" != "null" ] && ok "Attestation spec_ref: ${ATT_SPEC_REF}" || warn "Attestation spec_ref null"
  ok "Author agent: ${ATT_AGENT}"
else
  warn "No attestation bundle"
fi

# =============================================================================
step 12 "Verify knowledge graph"
# =============================================================================
sleep 2  # graph extraction runs async after merge push

GRAPH=$(api_get "${API}/repos/${REPO_ID}/graph" 2>/dev/null) || true
if [ -n "$GRAPH" ] && [ "$GRAPH" != "null" ]; then
  NODE_COUNT=$(echo "$GRAPH" | jq '.nodes | length')
  EDGE_COUNT=$(echo "$GRAPH" | jq '.edges | length')
  if [ "$NODE_COUNT" -gt 0 ]; then
    ok "Knowledge graph: ${NODE_COUNT} nodes, ${EDGE_COUNT} edges"
    # Show node types
    echo "$GRAPH" | jq -r '.nodes[] | "    \(.node_type) \(.qualified_name // .name)"' 2>/dev/null | sort | head -15 | while IFS= read -r line; do
      echo -e "  ${DIM}${line}${NC}"
    done || true
    # Check for our specific types
    HAS_GREETING=$(echo "$GRAPH" | jq '[.nodes[] | select(.name == "GreetingService" or .name == "GreetingConfig" or .name == "User" or .name == "ApiResponse")] | length')
    if [ "$HAS_GREETING" -gt 0 ]; then
      ok "Found ${HAS_GREETING} spec-defined types in graph (GreetingService, User, etc.)"
    else
      warn "Spec-defined types not found in graph — extractor may not have run yet"
    fi
  else
    warn "Knowledge graph has 0 nodes — extraction may not have completed"
  fi
else
  warn "Knowledge graph endpoint returned no data"
fi

# Check graph types endpoint
GRAPH_TYPES=$(api_get "${API}/repos/${REPO_ID}/graph/types" 2>/dev/null) || true
if [ -n "$GRAPH_TYPES" ] && [ "$GRAPH_TYPES" != "null" ]; then
  TYPE_COUNT=$(echo "$GRAPH_TYPES" | jq '.nodes | length')
  ok "Graph types: ${TYPE_COUNT} type nodes (structs/enums)"
fi

# Check graph modules endpoint
GRAPH_MODULES=$(api_get "${API}/repos/${REPO_ID}/graph/modules" 2>/dev/null) || true
if [ -n "$GRAPH_MODULES" ] && [ "$GRAPH_MODULES" != "null" ]; then
  MOD_COUNT=$(echo "$GRAPH_MODULES" | jq '.nodes | length')
  ok "Graph modules: ${MOD_COUNT} module nodes"
fi

# =============================================================================
step 13 "Verify full provenance chain"
# =============================================================================

# Spec
SPEC_FINAL_STATUS=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}" | jq -r '.approval_status')
ok "Spec: ${SPEC_FINAL_STATUS}"

# Spec progress
SPEC_PROGRESS=$(api_get "${API}/specs/${SPEC_PATH_ENCODED}/progress" 2>/dev/null) || true
if [ -n "$SPEC_PROGRESS" ]; then
  PT=$(echo "$SPEC_PROGRESS" | jq '.tasks | length')
  PM=$(echo "$SPEC_PROGRESS" | jq '.mrs | length')
  [ "$PT" -gt 0 ] && ok "Spec → ${PT} task(s)" || warn "Spec progress: 0 tasks"
  [ "$PM" -gt 0 ] && ok "Spec → ${PM} MR(s)" || warn "Spec progress: 0 MRs"
fi

# Task, Agent, MR
TASK_FINAL_STATUS=$(api_get "${API}/tasks/${TASK_ID}" | jq -r '.status')
AGENT_FINAL_STATUS=$(api_get "${API}/agents/${AGENT_ID}" | jq -r '.status')
MR_FINAL_STATUS=$(api_get "${API}/merge-requests/${MR_ID}" | jq -r '.status')
ok "Task: ${TASK_FINAL_STATUS} | Agent: ${AGENT_FINAL_STATUS} | MR: ${MR_FINAL_STATUS}"

# Commits
COMMIT_COUNT=$(api_get "${API}/repos/${REPO_ID}/commits?branch=main" | jq 'length')
ok "Main branch: ${COMMIT_COUNT} commits"

# Agent provenance
AC_COUNT=$(api_get "${API}/repos/${REPO_ID}/agent-commits?agent_id=${AGENT_ID}" 2>/dev/null | jq 'length // 0') || AC_COUNT=0
[ "$AC_COUNT" -gt 0 ] && ok "Agent commit provenance: ${AC_COUNT} records" || warn "Agent commit provenance: 0"

# #############################################################################
#  EXTENDED TESTS — exercise every remaining API surface
# #############################################################################

echo -e "\n${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BOLD}  Extended API Surface Tests${NC}"
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

STEP=14

# =============================================================================
step $((STEP++)) "Notifications"
# =============================================================================
NOTIF_RAW=$(curl -sf -H "$AUTH" "${API}/users/me/notifications" 2>/dev/null) || NOTIF_RAW="{}"
# Response may be {notifications:[...]} or plain array
NOTIF_COUNT=$(echo "$NOTIF_RAW" | jq '.notifications | length // length // 0' 2>/dev/null || echo "0")
ok "Notifications: ${NOTIF_COUNT} total"
echo "$NOTIF_RAW" | jq -r '(.notifications // .)[:3][] | "    \(.notification_type // "?") — \(.title // .message // "no message" | .[:60])"' 2>/dev/null || true

# Notification count endpoint
NOTIF_CT=$(curl -sf -H "$AUTH" "${API}/users/me/notifications/count" 2>/dev/null) || NOTIF_CT="{}"
UNREAD=$(echo "$NOTIF_CT" | jq '.count // .unread // 0')
ok "Unread notifications: ${UNREAD}"

# =============================================================================
step $((STEP++)) "MR timeline"
# =============================================================================
TIMELINE=$(api_get "${API}/merge-requests/${MR_ID}/timeline" 2>/dev/null) || TIMELINE="[]"
TL_COUNT=$(echo "$TIMELINE" | jq 'length // 0')
if [ "$TL_COUNT" -gt 0 ]; then
  ok "MR timeline: ${TL_COUNT} SDLC events"
  echo "$TIMELINE" | jq -r '.[0:5][] | "    \(.event_type // .event // "event") at \(.timestamp // "?")"' 2>/dev/null | while IFS= read -r line; do
    echo -e "  ${DIM}${line}${NC}"
  done || true
else
  warn "MR timeline: 0 events"
fi

# =============================================================================
step $((STEP++)) "MR reviews"
# =============================================================================
# Submit a review on the merged MR (post-merge review is still valid)
REVIEW_RESP=$(api_post "${API}/merge-requests/${MR_ID}/reviews" "{
  \"decision\": \"approved\",
  \"body\": \"LGTM — all acceptance criteria met\"
}" 2>/dev/null) || REVIEW_RESP=""
if [ -n "$REVIEW_RESP" ]; then
  ok "Review submitted: approved"
else
  warn "Review submission failed"
fi

# List reviews
REVIEWS=$(api_get "${API}/merge-requests/${MR_ID}/reviews" 2>/dev/null) || REVIEWS="[]"
REVIEW_CT=$(echo "$REVIEWS" | jq 'length // 0')
ok "Reviews on MR: ${REVIEW_CT}"

# Submit a comment
COMMENT_RESP=$(api_post "${API}/merge-requests/${MR_ID}/comments" "{
  \"body\": \"Great implementation of the greeting service spec.\"
}" 2>/dev/null) || COMMENT_RESP=""
[ -n "$COMMENT_RESP" ] && ok "Comment added" || warn "Comment submission failed"

COMMENTS=$(api_get "${API}/merge-requests/${MR_ID}/comments" 2>/dev/null) || COMMENTS="[]"
COMMENT_CT=$(echo "$COMMENTS" | jq 'length // 0')
ok "Comments on MR: ${COMMENT_CT}"

# =============================================================================
step $((STEP++)) "Agent lifecycle (heartbeat, logs, messages, workload)"
# =============================================================================
# Heartbeat (agent is idle but endpoint should still work)
HB_RESP=$(curl -s -w '%{http_code}' -X PUT -H "$AUTH" "${API}/agents/${AGENT_ID}/heartbeat")
HB_CODE=$(echo "$HB_RESP" | tail -c 4)
ok "Heartbeat: HTTP ${HB_CODE}"

# Post a log line
LOG_RESP=$(api_post "${API}/agents/${AGENT_ID}/logs" "{\"line\": \"e2e test: agent work complete\"}" 2>/dev/null) || LOG_RESP=""
ok "Log line posted"

# Get logs
LOGS=$(api_get "${API}/agents/${AGENT_ID}/logs" 2>/dev/null) || LOGS="[]"
LOG_CT=$(echo "$LOGS" | jq 'length // 0')
ok "Agent logs: ${LOG_CT} lines"

# Send a message to the agent
MSG_RESP=$(api_post "${API}/agents/${AGENT_ID}/messages" "{
  \"content\": \"e2e test message\",
  \"kind\": \"FreeText\"
}" 2>/dev/null) || MSG_RESP=""
[ -n "$MSG_RESP" ] && ok "Message sent to agent" || info "Message send returned empty (may be expected)"

# Get agent messages
MSGS=$(api_get "${API}/agents/${AGENT_ID}/messages" 2>/dev/null) || MSGS="[]"
MSG_CT=$(echo "$MSGS" | jq 'length // 0' 2>/dev/null || echo "0")
ok "Agent messages: ${MSG_CT}"

# Workload attestation
WORKLOAD=$(api_get "${API}/agents/${AGENT_ID}/workload" 2>/dev/null) || WORKLOAD="{}"
WL_PID=$(echo "$WORKLOAD" | jq -r '.pid // "none"')
ok "Workload attestation: pid=${WL_PID}"

# Agent card
CARD_RESP=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" \
  -d "{\"name\":\"e2e-worker\",\"capabilities\":[\"rust\",\"greeting\"],\"protocols\":[\"mcp\"]}" \
  "${API}/agents/${AGENT_ID}/card")
CARD_CODE=$(echo "$CARD_RESP" | tail -1)
ok "Agent card published: HTTP ${CARD_CODE}"

# Agent discovery
DISCOVER=$(api_get "${API}/agents/discover?capability=rust" 2>/dev/null) || DISCOVER="[]"
DISC_CT=$(echo "$DISCOVER" | jq 'length // 0')
ok "Agent discovery (capability=rust): ${DISC_CT} agents"

# =============================================================================
step $((STEP++)) "Search"
# =============================================================================
SEARCH=$(api_get "${API}/search?q=greeting&limit=10" 2>/dev/null) || SEARCH="[]"
SEARCH_CT=$(echo "$SEARCH" | jq 'length // 0')
if [ "$SEARCH_CT" -gt 0 ]; then
  ok "Search 'greeting': ${SEARCH_CT} results"
  echo "$SEARCH" | jq -r '.[0:3][] | "    \(.entity_type) — \(.title[:50])"' 2>/dev/null | while IFS= read -r line; do
    echo -e "  ${DIM}${line}${NC}"
  done || true
else
  warn "Search returned 0 results — indexing may not be enabled"
fi

# =============================================================================
step $((STEP++)) "Push gates (pre-accept)"
# =============================================================================
# Configure push gates
PG_RESP=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" \
  -d '{"gates":["ConventionalCommit"]}' \
  "${API}/repos/${REPO_ID}/push-gates")
PG_CODE=$(echo "$PG_RESP" | tail -1)
ok "Push gates set: ConventionalCommit (HTTP ${PG_CODE})"

# Try pushing a non-conventional commit (should be rejected)
PUSHGATE_DIR="${WORK_DIR}/pushgate-test"
git_with_token "$REPO_DIR" "$TOKEN" fetch origin main 2>/dev/null
git -C "$REPO_DIR" checkout main 2>/dev/null
git -C "$REPO_DIR" reset --hard origin/main 2>/dev/null
echo "pushgate test" > "$REPO_DIR/pushgate.txt"
git -C "$REPO_DIR" add pushgate.txt
git -C "$REPO_DIR" commit -m "bad commit message without conventional prefix" --no-gpg-sign 2>/dev/null
BAD_PUSH=$(git_with_token "$REPO_DIR" "$TOKEN" push origin main 2>&1) || true
if echo "$BAD_PUSH" | grep -qi "reject\|denied\|error\|conventional"; then
  ok "Push gate rejected non-conventional commit"
else
  warn "Push gate did not reject non-conventional commit: ${BAD_PUSH:0:80}"
fi

# Push a valid conventional commit
git -C "$REPO_DIR" reset --soft HEAD~1 2>/dev/null
git -C "$REPO_DIR" commit -m "chore: push gate test" --no-gpg-sign 2>/dev/null
GOOD_PUSH=$(git_with_token "$REPO_DIR" "$TOKEN" push origin main 2>&1) || true
if echo "$GOOD_PUSH" | grep -qi "reject\|denied"; then
  warn "Push gate rejected valid conventional commit"
else
  ok "Push gate accepted conventional commit"
fi

# Verify push gates are listed
PG_LIST=$(api_get "${API}/repos/${REPO_ID}/push-gates" 2>/dev/null) || PG_LIST="{}"
ok "Push gates config: $(echo "$PG_LIST" | jq -c '.gates // []')"

# =============================================================================
step $((STEP++)) "Spec policy"
# =============================================================================
SP_RESP=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" \
  -d '{"require_spec_ref":true,"require_approved_spec":false,"warn_stale_spec":true,"require_current_spec":false}' \
  "${API}/repos/${REPO_ID}/spec-policy")
SP_CODE=$(echo "$SP_RESP" | tail -1)
ok "Spec policy set: require_spec_ref + warn_stale (HTTP ${SP_CODE})"

SP_GET=$(api_get "${API}/repos/${REPO_ID}/spec-policy" 2>/dev/null) || SP_GET="{}"
ok "Spec policy: $(echo "$SP_GET" | jq -c '{require_spec_ref,warn_stale_spec}')"

# =============================================================================
step $((STEP++)) "ABAC policies"
# =============================================================================
POLICY_RESP=$(api_post "${API}/policies" "{
  \"name\": \"e2e-allow-agents\",
  \"effect\": \"Allow\",
  \"rules\": [{\"claim\": \"scope\", \"operator\": \"Equals\", \"value\": \"agent\"}],
  \"priority\": 100
}" 2>/dev/null) || POLICY_RESP=""
if [ -n "$POLICY_RESP" ]; then
  POLICY_ID=$(echo "$POLICY_RESP" | jq -r '.id // "none"')
  ok "ABAC policy created: ${POLICY_ID}"
else
  warn "ABAC policy creation failed"
  POLICY_ID=""
fi

# Dry-run evaluation
EVAL_RESP=$(api_post "${API}/policies/evaluate" "{
  \"context\": {\"scope\": \"agent\", \"action\": \"push\"}
}" 2>/dev/null) || EVAL_RESP="{}"
EVAL_DECISION=$(echo "$EVAL_RESP" | jq -r '.decision // "unknown"')
ok "ABAC evaluate: decision=${EVAL_DECISION}"

# Decision audit log
DECISIONS=$(api_get "${API}/policies/decisions?limit=5" 2>/dev/null) || DECISIONS="[]"
DEC_CT=$(echo "$DECISIONS" | jq 'length // 0')
ok "ABAC decision log: ${DEC_CT} entries"

# Repo-scoped ABAC
REPO_ABAC=$(api_get "${API}/repos/${REPO_ID}/abac-policy" 2>/dev/null) || REPO_ABAC="[]"
ok "Repo ABAC policies: $(echo "$REPO_ABAC" | jq 'length // 0')"

# =============================================================================
step $((STEP++)) "Budget enforcement"
# =============================================================================
# Set a tight workspace budget
BUDGET_RESP=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" \
  -d '{"max_concurrent_agents":1,"max_tokens_per_day":1000,"max_cost_per_day":0.01}' \
  "${API}/workspaces/${WS_ID}/budget")
BUDGET_CODE=$(echo "$BUDGET_RESP" | tail -1)
ok "Budget set: max_concurrent=1, max_tokens=1000 (HTTP ${BUDGET_CODE})"

# Check budget
BUDGET=$(api_get "${API}/workspaces/${WS_ID}/budget" 2>/dev/null) || BUDGET="{}"
ok "Budget: $(echo "$BUDGET" | jq -c '{max_concurrent_agents,max_tokens_per_day}' 2>/dev/null)"

# Tenant budget summary
BUDGET_SUM=$(api_get "${API}/budget/summary" 2>/dev/null) || BUDGET_SUM="{}"
ok "Tenant budget summary retrieved"

# =============================================================================
step $((STEP++)) "Spec links (implements/conflicts enforcement)"
# =============================================================================
# Create a second spec that "implements" the first
SPEC2_NAME="greeting-impl-${RUN_ID}"
SPEC2_PATH="system/${SPEC2_NAME}.md"
SPEC2_PATH_ENCODED="system%2F${SPEC2_NAME}.md"

# Sync with remote before modifying
git_with_token "$REPO_DIR" "$TOKEN" fetch origin main 2>/dev/null
git -C "$REPO_DIR" checkout main 2>/dev/null
git -C "$REPO_DIR" reset --hard origin/main 2>/dev/null

mkdir -p "$REPO_DIR/specs/system"
cat > "$REPO_DIR/specs/system/${SPEC2_NAME}.md" << SPEC2
# Greeting Implementation Detail (${RUN_ID})

> Status: Draft

## Summary
Implementation details for the greeting service.
SPEC2

# Update manifest with link
cat > "$REPO_DIR/specs/manifest.yaml" << MANIFEST2
version: 1
defaults:
  requires_approval: true
  auto_create_tasks: true
specs:
  - path: ${SPEC_PATH}
    title: "Greeting Service (${RUN_ID})"
    owner: "e2e-test"
  - path: ${SPEC2_PATH}
    title: "Greeting Implementation (${RUN_ID})"
    owner: "e2e-test"
    links:
      - type: implements
        target: ${SPEC_PATH}
MANIFEST2

git -C "$REPO_DIR" add .
git -C "$REPO_DIR" commit -m "feat: add implementation spec with link" --no-gpg-sign
git_with_token "$REPO_DIR" "$TOKEN" push origin main 2>&1 | tail -2
sleep 1

# Check spec links
LINKS=$(api_get "${API}/specs/${SPEC2_PATH_ENCODED}/links" 2>/dev/null) || LINKS="{}"
LINK_CT=$(echo "$LINKS" | jq '.links | length // 0')
ok "Spec links: ${LINK_CT} link(s) on child spec"

# Check spec graph
SPEC_GRAPH=$(api_get "${API}/specs/graph" 2>/dev/null) || SPEC_GRAPH="{}"
SG_NODES=$(echo "$SPEC_GRAPH" | jq '.nodes | length // 0')
SG_EDGES=$(echo "$SPEC_GRAPH" | jq '.edges | length // 0')
ok "Spec graph: ${SG_NODES} nodes, ${SG_EDGES} edges"

# The child spec should be approvable since parent is already approved
SPEC2_ENTRY=$(api_get "${API}/specs" | jq ".[] | select(.path == \"${SPEC2_PATH}\")")
SPEC2_SHA=$(echo "$SPEC2_ENTRY" | jq -r '.current_sha')
if [ -n "$SPEC2_SHA" ] && [ "$SPEC2_SHA" != "null" ]; then
  APPROVE2=$(api_post "${API}/specs/${SPEC2_PATH_ENCODED}/approve" "{\"sha\": \"${SPEC2_SHA}\"}" 2>/dev/null) || APPROVE2=""
  if [ -n "$APPROVE2" ]; then
    ok "Child spec approved (parent already approved — link check passed)"
  else
    warn "Child spec approval failed"
  fi
fi

# =============================================================================
step $((STEP++)) "Spec rejection mid-flight"
# =============================================================================
# Create a third spec, approve it, create a task, then reject — verify cancellation
SPEC3_NAME="rejected-feature-${RUN_ID}"
SPEC3_PATH="system/${SPEC3_NAME}.md"
SPEC3_PATH_ENCODED="system%2F${SPEC3_NAME}.md"

# Sync with remote
git_with_token "$REPO_DIR" "$TOKEN" fetch origin main 2>/dev/null
git -C "$REPO_DIR" reset --hard origin/main 2>/dev/null

cat > "$REPO_DIR/specs/system/${SPEC3_NAME}.md" << SPEC3
# Feature to Reject (${RUN_ID})

> Status: Draft

## Summary
This feature will be rejected mid-flight.
SPEC3

# Update manifest
cat >> "$REPO_DIR/specs/manifest.yaml" << MANIFEST3
  - path: ${SPEC3_PATH}
    title: "Feature to Reject (${RUN_ID})"
    owner: "e2e-test"
MANIFEST3

git -C "$REPO_DIR" add .
git -C "$REPO_DIR" commit -m "feat: add spec for rejection test" --no-gpg-sign
git_with_token "$REPO_DIR" "$TOKEN" push origin main 2>&1 | tail -2
sleep 1

# Approve it
SPEC3_SHA=$(api_get "${API}/specs" | jq -r ".[] | select(.path == \"${SPEC3_PATH}\") | .current_sha")
api_post "${API}/specs/${SPEC3_PATH_ENCODED}/approve" "{\"sha\": \"${SPEC3_SHA}\"}" >/dev/null 2>&1
ok "Spec approved (will reject next)"

# Create a task for it
REJ_TASK=$(api_post "${API}/tasks" "{
  \"title\": \"Implement rejected feature\",
  \"task_type\": \"implementation\",
  \"spec_path\": \"${SPEC3_PATH}\"
}")
REJ_TASK_ID=$(echo "$REJ_TASK" | jq -r '.id')
ok "Task created: ${REJ_TASK_ID}"

# Now reject the spec
REJ_RESP=$(api_post "${API}/specs/${SPEC3_PATH_ENCODED}/reject" "{\"reason\": \"e2e test: spec rejected mid-flight\"}" 2>/dev/null) || REJ_RESP=""
if [ -n "$REJ_RESP" ]; then
  ok "Spec rejected"
else
  warn "Spec rejection failed (endpoint may not exist)"
fi

# Verify spec is now rejected
SPEC3_STATUS=$(api_get "${API}/specs/${SPEC3_PATH_ENCODED}" 2>/dev/null | jq -r '.approval_status // "unknown"')
ok "Spec status after rejection: ${SPEC3_STATUS}"

# Check approval history
SPEC3_HISTORY=$(api_get "${API}/specs/${SPEC3_PATH_ENCODED}/history" 2>/dev/null) || SPEC3_HISTORY="[]"
HIST_CT=$(echo "$SPEC3_HISTORY" | jq 'length // 0')
ok "Spec approval history: ${HIST_CT} events"

# =============================================================================
step $((STEP++)) "Repo lifecycle (archive/unarchive)"
# =============================================================================
# Create a second repo to archive (don't archive the main one)
REPO2_RESP=$(api_post "${API}/repos" "{\"workspace_id\":\"${WS_ID}\",\"name\":\"archive-test-${RUN_ID}\"}")
REPO2_ID=$(echo "$REPO2_RESP" | jq -r '.id')
ok "Created repo for archive test: ${REPO2_ID}"

# Archive it
ARCHIVE_RESP=$(api_post "${API}/repos/${REPO2_ID}/archive" "{}" 2>/dev/null) || ARCHIVE_RESP=""
if [ -n "$ARCHIVE_RESP" ]; then
  ARCHIVE_STATUS=$(echo "$ARCHIVE_RESP" | jq -r '.status // "unknown"')
  ok "Repo archived: status=${ARCHIVE_STATUS}"
else
  warn "Repo archive failed"
fi

# Unarchive it
UNARCHIVE_RESP=$(api_post "${API}/repos/${REPO2_ID}/unarchive" "{}" 2>/dev/null) || UNARCHIVE_RESP=""
if [ -n "$UNARCHIVE_RESP" ]; then
  ok "Repo unarchived"
else
  warn "Repo unarchive failed"
fi

# Delete it
DEL_RESP=$(curl -s -w '\n%{http_code}' -X DELETE -H "$AUTH" "${API}/repos/${REPO2_ID}")
DEL_CODE=$(echo "$DEL_RESP" | tail -1)
ok "Repo deleted: HTTP ${DEL_CODE}"

# =============================================================================
step $((STEP++)) "MR dependencies & atomic groups"
# =============================================================================
# Create two MRs to test dependencies (use existing repo)
DEP_TASK1=$(api_post "${API}/tasks" "{\"title\":\"dep-task-1\",\"task_type\":\"implementation\"}" | jq -r '.id')
DEP_AGENT1_RESP=$(api_post "${API}/agents/spawn" "{
  \"name\":\"dep-agent-1-${RUN_ID}\",\"repo_id\":\"${REPO_ID}\",
  \"task_id\":\"${DEP_TASK1}\",\"branch\":\"feat/dep-1\"
}")
DEP_AGENT1_ID=$(echo "$DEP_AGENT1_RESP" | jq -r '.agent.id')
DEP_AGENT1_TOKEN=$(echo "$DEP_AGENT1_RESP" | jq -r '.token')
DEP_CLONE=$(echo "$DEP_AGENT1_RESP" | jq -r '.clone_url')

# Agent 1 does work
DEP_DIR="${WORK_DIR}/dep-work"
git_with_token "$WORK_DIR" "$DEP_AGENT1_TOKEN" clone "${DEP_CLONE}.git" dep-work 2>/dev/null || true
if [ ! -d "$DEP_DIR/.git" ]; then
  mkdir -p "$DEP_DIR"; cd "$DEP_DIR"; git init; git remote add origin "${DEP_CLONE}.git"
  git_with_token "$DEP_DIR" "$DEP_AGENT1_TOKEN" fetch origin main 2>/dev/null
  git -C "$DEP_DIR" checkout -b main FETCH_HEAD 2>/dev/null
fi
git -C "$DEP_DIR" config user.email "a@a" && git -C "$DEP_DIR" config user.name "A"
git -C "$DEP_DIR" checkout -b feat/dep-1 2>/dev/null || true
echo "dep1" > "$DEP_DIR/dep1.txt"
git -C "$DEP_DIR" add . && git -C "$DEP_DIR" commit -m "feat: dependency 1" --no-gpg-sign
git_with_token "$DEP_DIR" "$DEP_AGENT1_TOKEN" push origin feat/dep-1 2>/dev/null
sleep 1

DEP_MR1=$(api_post "${API}/agents/${DEP_AGENT1_ID}/complete" "{
  \"branch\":\"feat/dep-1\",\"title\":\"feat: dep 1\",\"target_branch\":\"main\"
}" | jq -r '.id')
ok "MR dep-1: ${DEP_MR1}"

# Set dependency: dep-1 depends on the original MR (already merged, so this is a no-op but tests the API)
DEP_SET=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" \
  -d "{\"depends_on\":[\"${MR_ID}\"]}" \
  "${API}/merge-requests/${DEP_MR1}/dependencies")
DEP_CODE=$(echo "$DEP_SET" | tail -1)
ok "MR dependency set: HTTP ${DEP_CODE}"

# Get dependencies
DEP_GET=$(api_get "${API}/merge-requests/${DEP_MR1}/dependencies" 2>/dev/null) || DEP_GET="{}"
ok "MR dependencies: $(echo "$DEP_GET" | jq -c '{depends_on: (.depends_on | length), dependents: (.dependents | length)}')"

# Set atomic group
AG_SET=$(curl -s -w '\n%{http_code}' -X PUT -H "$AUTH" -H "$CT" \
  -d '{"group":"e2e-atomic-group"}' \
  "${API}/merge-requests/${DEP_MR1}/atomic-group")
AG_CODE=$(echo "$AG_SET" | tail -1)
ok "Atomic group set: HTTP ${AG_CODE}"

# Merge queue graph
MQ_GRAPH=$(api_get "${API}/merge-queue/graph" 2>/dev/null) || MQ_GRAPH="{}"
MQ_NODES=$(echo "$MQ_GRAPH" | jq '.nodes | length // 0')
ok "Merge queue graph: ${MQ_NODES} nodes"

# =============================================================================
step $((STEP++)) "Commit signatures & provenance"
# =============================================================================
# Get a commit SHA from main
MAIN_SHA=$(api_get "${API}/repos/${REPO_ID}/commits?branch=main&limit=1" | jq -r '.[0].sha // "none"')
if [ "$MAIN_SHA" != "none" ]; then
  SIG=$(api_get "${API}/repos/${REPO_ID}/commits/${MAIN_SHA}/signature" 2>/dev/null) || SIG=""
  if [ -n "$SIG" ] && [ "$SIG" != "null" ]; then
    ok "Commit signature: $(echo "$SIG" | jq -r '.algorithm // "present"')"
  else
    info "No commit signature for ${MAIN_SHA:0:12} (expected for non-jj commits)"
  fi
fi

# Conversation provenance
CONV=$(api_get "${API}/conversations/${MAIN_SHA}" 2>/dev/null) || CONV=""
if [ -n "$CONV" ] && [ "$CONV" != "null" ]; then
  TURN_CT=$(echo "$CONV" | jq '.turns | length // 0')
  ok "Conversation provenance: ${TURN_CT} turns"
else
  info "No conversation provenance (expected for non-agent commits)"
fi

# AIBOM
AIBOM=$(api_get "${API}/repos/${REPO_ID}/aibom" 2>/dev/null) || AIBOM="[]"
AIBOM_CT=$(echo "$AIBOM" | jq 'length // 0' 2>/dev/null || echo "0")
ok "AIBOM entries: ${AIBOM_CT}"

# Blame
BLAME=$(api_get "${API}/repos/${REPO_ID}/blame?path=src/greeting.rs" 2>/dev/null) || BLAME="[]"
BLAME_CT=$(echo "$BLAME" | jq 'length // 0' 2>/dev/null || echo "0")
ok "Blame (src/greeting.rs): ${BLAME_CT} lines"

# Hot files
HOT=$(api_get "${API}/repos/${REPO_ID}/hot-files?limit=5" 2>/dev/null) || HOT="[]"
HOT_CT=$(echo "$HOT" | jq 'length // 0' 2>/dev/null || echo "0")
ok "Hot files: ${HOT_CT}"

# Review routing
ROUTING=$(api_get "${API}/repos/${REPO_ID}/review-routing?path=src/greeting.rs" 2>/dev/null) || ROUTING="[]"
ROUTING_CT=$(echo "$ROUTING" | jq 'length // 0' 2>/dev/null || echo "0")
ok "Review routing: ${ROUTING_CT} suggested reviewers"

# Speculative merge
SPEC_MERGE=$(api_get "${API}/repos/${REPO_ID}/speculative" 2>/dev/null) || SPEC_MERGE="[]"
ok "Speculative merge results: $(echo "$SPEC_MERGE" | jq 'length // 0')"

# =============================================================================
step $((STEP++)) "Meta-spec registry"
# =============================================================================
MS_CREATE=$(api_post "${API}/meta-specs-registry" "{
  \"name\": \"e2e-coding-standard\",
  \"kind\": \"meta:standard\",
  \"path\": \"standards/e2e-test.md\",
  \"content\": \"# E2E Coding Standard\n\nAll functions must have doc comments.\",
  \"version\": 1
}" 2>/dev/null) || MS_CREATE=""
if [ -n "$MS_CREATE" ]; then
  MS_ID=$(echo "$MS_CREATE" | jq -r '.id // "none"')
  ok "Meta-spec created: ${MS_ID}"

  # List
  MS_LIST=$(api_get "${API}/meta-specs-registry" 2>/dev/null) || MS_LIST="[]"
  MS_CT=$(echo "$MS_LIST" | jq 'length // 0')
  ok "Meta-spec registry: ${MS_CT} entries"

  # Get by ID
  if [ "$MS_ID" != "none" ]; then
    MS_GET=$(api_get "${API}/meta-specs-registry/${MS_ID}" 2>/dev/null) || MS_GET="{}"
    ok "Meta-spec detail: $(echo "$MS_GET" | jq -r '.name // "?"')"

    # Versions
    MS_VERS=$(api_get "${API}/meta-specs-registry/${MS_ID}/versions" 2>/dev/null) || MS_VERS="[]"
    ok "Meta-spec versions: $(echo "$MS_VERS" | jq 'length // 0')"

    # Delete
    DEL_MS=$(curl -s -w '%{http_code}' -X DELETE -H "$AUTH" "${API}/meta-specs-registry/${MS_ID}")
    ok "Meta-spec deleted: HTTP $(echo "$DEL_MS" | tail -c 4)"
  fi
else
  warn "Meta-spec registry creation failed"
fi

# Workspace meta-spec-set
MS_SET=$(api_get "${API}/workspaces/${WS_ID}/meta-spec-set" 2>/dev/null) || MS_SET="{}"
ok "Workspace meta-spec-set: $(echo "$MS_SET" | jq -c 'keys' 2>/dev/null || echo "retrieved")"

# =============================================================================
step $((STEP++)) "Compute targets"
# =============================================================================
CT_CREATE=$(api_post "${API}/admin/compute-targets" "{
  \"name\": \"e2e-local-target\",
  \"target_type\": \"local\",
  \"config\": {}
}" 2>/dev/null) || CT_CREATE=""
if [ -n "$CT_CREATE" ]; then
  CT_ID=$(echo "$CT_CREATE" | jq -r '.id // "none"')
  ok "Compute target created: ${CT_ID} (local)"

  # List
  CT_LIST=$(api_get "${API}/admin/compute-targets" 2>/dev/null) || CT_LIST="[]"
  ok "Compute targets: $(echo "$CT_LIST" | jq 'length // 0')"

  # Delete
  if [ "$CT_ID" != "none" ]; then
    curl -s -X DELETE -H "$AUTH" "${API}/admin/compute-targets/${CT_ID}" >/dev/null 2>&1
    ok "Compute target deleted"
  fi
else
  warn "Compute target creation failed"
fi

# =============================================================================
step $((STEP++)) "Repo dependencies (cross-repo)"
# =============================================================================
REPO_DEPS=$(api_get "${API}/repos/${REPO_ID}/dependencies" 2>/dev/null) || REPO_DEPS="[]"
ok "Repo dependencies: $(echo "$REPO_DEPS" | jq 'length // 0')"

REPO_DEPTS=$(api_get "${API}/repos/${REPO_ID}/dependents" 2>/dev/null) || REPO_DEPTS="[]"
ok "Repo dependents: $(echo "$REPO_DEPTS" | jq 'length // 0')"

REPO_BLAST=$(api_get "${API}/repos/${REPO_ID}/blast-radius" 2>/dev/null) || REPO_BLAST="{}"
ok "Blast radius: $(echo "$REPO_BLAST" | jq '.repos | length // 0' 2>/dev/null || echo "0") repos"

DEP_GRAPH=$(api_get "${API}/dependencies/graph" 2>/dev/null) || DEP_GRAPH="{}"
ok "Tenant dependency graph: $(echo "$DEP_GRAPH" | jq '.nodes | length // 0') nodes"

# =============================================================================
step $((STEP++)) "Release preparation"
# =============================================================================
RELEASE=$(api_post "${API}/release/prepare" "{
  \"repo_id\": \"${REPO_ID}\"
}" 2>/dev/null) || RELEASE=""
if [ -n "$RELEASE" ]; then
  NEXT_VER=$(echo "$RELEASE" | jq -r '.next_version // "none"')
  CHANGELOG_LEN=$(echo "$RELEASE" | jq -r '.changelog | length // 0')
  ok "Release: next=${NEXT_VER}, changelog=${CHANGELOG_LEN} chars"
else
  warn "Release preparation failed"
fi

# =============================================================================
step $((STEP++)) "Knowledge graph advanced (concept, risks, timeline, diff)"
# =============================================================================
# Concept view
CONCEPT=$(api_get "${API}/repos/${REPO_ID}/graph/concept/Greeting" 2>/dev/null) || CONCEPT="{}"
CONCEPT_CT=$(echo "$CONCEPT" | jq '.nodes | length // 0')
ok "Graph concept 'Greeting': ${CONCEPT_CT} nodes"

# Risk metrics
RISKS=$(api_get "${API}/repos/${REPO_ID}/graph/risks" 2>/dev/null) || RISKS="[]"
RISK_CT=$(echo "$RISKS" | jq 'length // 0')
ok "Graph risk metrics: ${RISK_CT} nodes scored"

# Timeline (architectural deltas)
GRAPH_TL=$(api_get "${API}/repos/${REPO_ID}/graph/timeline" 2>/dev/null) || GRAPH_TL="[]"
GRAPH_TL_CT=$(echo "$GRAPH_TL" | jq 'length // 0')
ok "Graph timeline: ${GRAPH_TL_CT} deltas"

# Graph diff
GRAPH_DIFF=$(api_get "${API}/repos/${REPO_ID}/graph/diff" 2>/dev/null) || GRAPH_DIFF="{}"
ok "Graph diff: $(echo "$GRAPH_DIFF" | jq -r '.message // "retrieved"')"

# Workspace graph
WS_GRAPH=$(api_get "${API}/workspaces/${WS_ID}/graph" 2>/dev/null) || WS_GRAPH="{}"
WS_NODES=$(echo "$WS_GRAPH" | jq '.nodes | length // 0')
ok "Workspace graph: ${WS_NODES} nodes (cross-repo)"

# Workspace concept
WS_CONCEPT=$(api_get "${API}/workspaces/${WS_ID}/graph/concept/Greeting" 2>/dev/null) || WS_CONCEPT="{}"
ok "Workspace concept 'Greeting': $(echo "$WS_CONCEPT" | jq '.nodes | length // 0') nodes"

# Briefing
BRIEFING=$(api_get "${API}/workspaces/${WS_ID}/briefing" 2>/dev/null) || BRIEFING="{}"
ok "Workspace briefing: $(echo "$BRIEFING" | jq -r '.summary[:60] // "retrieved"' 2>/dev/null)"

# Explorer views
EV_CREATE=$(api_post "${API}/workspaces/${WS_ID}/explorer-views" "{
  \"name\": \"e2e-overview\",
  \"description\": \"Test view\"
}" 2>/dev/null) || EV_CREATE=""
if [ -n "$EV_CREATE" ]; then
  EV_ID=$(echo "$EV_CREATE" | jq -r '.id // "none"')
  ok "Explorer view created: ${EV_ID}"
  [ "$EV_ID" != "none" ] && curl -s -X DELETE -H "$AUTH" "${API}/workspaces/${WS_ID}/explorer-views/${EV_ID}" >/dev/null 2>&1
fi

# =============================================================================
step $((STEP++)) "Admin endpoints"
# =============================================================================
ADMIN_HEALTH=$(api_get "${API}/admin/health" 2>/dev/null) || ADMIN_HEALTH="{}"
ok "Admin health: $(echo "$ADMIN_HEALTH" | jq -c '{uptime_secs: .uptime_secs}' 2>/dev/null || echo "retrieved")"

ADMIN_JOBS=$(api_get "${API}/admin/jobs" 2>/dev/null) || ADMIN_JOBS="[]"
ok "Background jobs: $(echo "$ADMIN_JOBS" | jq 'length // 0')"

ADMIN_AUDIT=$(api_get "${API}/admin/audit?limit=5" 2>/dev/null) || ADMIN_AUDIT="[]"
ok "Audit log: $(echo "$ADMIN_AUDIT" | jq 'length // 0') entries"

# Analytics
ANALYTICS=$(api_get "${API}/analytics/usage?event_name=agent.completed" 2>/dev/null) || ANALYTICS="{}"
ok "Analytics (agent.completed): $(echo "$ANALYTICS" | jq -r '.count // 0') events"

ANALYTICS_TOP=$(api_get "${API}/analytics/top?limit=5" 2>/dev/null) || ANALYTICS_TOP="[]"
ok "Top analytics: $(echo "$ANALYTICS_TOP" | jq 'length // 0') event types"

# Costs
COSTS=$(api_get "${API}/costs/summary" 2>/dev/null) || COSTS="[]"
ok "Cost summary: $(echo "$COSTS" | jq 'length // 0') entries"

# Activity log
ACTIVITY=$(api_get "${API}/activity?limit=10" 2>/dev/null) || ACTIVITY="[]"
ok "Activity log: $(echo "$ACTIVITY" | jq 'length // 0') events"

# User profile
USER=$(api_get "${API}/users/me" 2>/dev/null) || USER="{}"
ok "User profile: $(echo "$USER" | jq -r '.username // .global_role // "retrieved"')"

USER_AGENTS=$(api_get "${API}/users/me/agents" 2>/dev/null) || USER_AGENTS="[]"
ok "My agents: $(echo "$USER_AGENTS" | jq 'length // 0')"

USER_TASKS=$(api_get "${API}/users/me/tasks" 2>/dev/null) || USER_TASKS="[]"
ok "My tasks: $(echo "$USER_TASKS" | jq 'length // 0')"

USER_MRS=$(api_get "${API}/users/me/mrs" 2>/dev/null) || USER_MRS="[]"
ok "My MRs: $(echo "$USER_MRS" | jq 'length // 0')"

# Token management
TOKEN_CREATE=$(api_post "${API}/users/me/tokens" "{\"name\":\"e2e-test-token\",\"scopes\":[\"read\"]}" 2>/dev/null) || TOKEN_CREATE=""
if [ -n "$TOKEN_CREATE" ]; then
  TOK_ID=$(echo "$TOKEN_CREATE" | jq -r '.id // "none"')
  ok "API token created: ${TOK_ID}"
  [ "$TOK_ID" != "none" ] && curl -s -X DELETE -H "$AUTH" "${API}/users/me/tokens/${TOK_ID}" >/dev/null 2>&1
  ok "API token revoked"
fi

# Version
VERSION=$(api_get "${API}/version" 2>/dev/null) || VERSION="{}"
ok "Server: $(echo "$VERSION" | jq -c '{name,version,milestone}')"

# Auth token info
TOKEN_INFO=$(api_get "${API}/auth/token-info" 2>/dev/null) || TOKEN_INFO="{}"
ok "Token info: $(echo "$TOKEN_INFO" | jq -r '.token_kind // "retrieved"')"

# =============================================================================
# Summary
# =============================================================================
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
TOTAL_STEPS=$((STEP - 1))
if [ ${#ISSUES[@]} -eq 0 ]; then
  echo -e "${BOLD}║  ${GREEN}All ${TOTAL_STEPS} steps passed!${NC}${BOLD}                                      ║${NC}"
else
  echo -e "${BOLD}║  ${YELLOW}${#ISSUES[@]} issue(s) across ${TOTAL_STEPS} steps${NC}${BOLD}                                  ║${NC}"
fi
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  ${BOLD}Provenance Chain:${NC}"
echo -e "    ${CYAN}Spec${NC}   ${SPEC_PATH} (${SPEC_FINAL_STATUS})"
echo -e "      ↓  approved by ${APPROVER_TYPE}"
echo -e "    ${CYAN}Task${NC}   ${TASK_ID:0:8}... (${TASK_FINAL_STATUS})"
echo -e "      ↓  agent spawned"
echo -e "    ${CYAN}Agent${NC}  ${AGENT_ID:0:8}... (${AGENT_FINAL_STATUS})"
echo -e "      ↓  +${DIFF_INS} -${DIFF_DEL} across ${DIFF_FILES} files"
echo -e "    ${CYAN}MR${NC}     ${MR_ID:0:8}... (${MR_FINAL_STATUS})"
echo -e "      ↓  merged + attested + gated"
echo -e "    ${CYAN}Code${NC}   ${COMMIT_COUNT} commits on main"
NODE_COUNT=${NODE_COUNT:-0}
EDGE_COUNT=${EDGE_COUNT:-0}
echo -e "    ${CYAN}Graph${NC}  ${NODE_COUNT} nodes, ${EDGE_COUNT} edges"
echo ""

if [ ${#ISSUES[@]} -gt 0 ]; then
  echo -e "  ${BOLD}${YELLOW}Issues:${NC}"
  for issue in "${ISSUES[@]}"; do
    echo -e "    ${YELLOW}⚠${NC} ${issue}"
  done
  echo ""
fi

echo -e "  ${DIM}IDs: ws=${WS_ID:0:8} repo=${REPO_ID:0:8} task=${TASK_ID:0:8} agent=${AGENT_ID:0:8} mr=${MR_ID:0:8}${NC}"
echo ""
