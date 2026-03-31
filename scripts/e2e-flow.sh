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
step 5 "Create implementation task"
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
step 6 "Spawn agent"
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
step 7 "Agent implements the spec (real Rust code)"
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
step 8 "Agent completes → MR with diff"
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
  echo "$DIFF" | jq -r '.files[] | "    \(.status) \(.path)"' 2>/dev/null | while read -r line; do
    echo -e "  ${DIM}${line}${NC}"
  done
else
  warn "MR diff empty"
fi

ok "Agent → $(api_get "${API}/agents/${AGENT_ID}" | jq -r '.status')"
ok "Task → $(api_get "${API}/tasks/${TASK_ID}" | jq -r '.status')"

# =============================================================================
step 9 "Merge queue"
# =============================================================================
api_post "${API}/merge-queue/enqueue" "{\"merge_request_id\": \"${MR_ID}\"}" >/dev/null
ok "MR enqueued"

info "Waiting for merge processor..."
MERGED=false
for i in $(seq 1 30); do
  sleep 1
  MR_CURRENT=$(api_get "${API}/merge-requests/${MR_ID}" | jq -r '.status')
  [ "$MR_CURRENT" = "merged" ] && { MERGED=true; break; }
  [ $((i % 5)) -eq 0 ] && info "Waiting... (${i}s, status: ${MR_CURRENT})"
done
[ "$MERGED" = true ] || fail "MR did not merge within 30s (status: ${MR_CURRENT})"
ok "MR merged!"

# =============================================================================
step 10 "Verify attestation"
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
step 11 "Verify knowledge graph"
# =============================================================================
sleep 2  # graph extraction runs async after merge push

GRAPH=$(api_get "${API}/repos/${REPO_ID}/graph" 2>/dev/null) || true
if [ -n "$GRAPH" ] && [ "$GRAPH" != "null" ]; then
  NODE_COUNT=$(echo "$GRAPH" | jq '.nodes | length')
  EDGE_COUNT=$(echo "$GRAPH" | jq '.edges | length')
  if [ "$NODE_COUNT" -gt 0 ]; then
    ok "Knowledge graph: ${NODE_COUNT} nodes, ${EDGE_COUNT} edges"
    # Show node types
    echo "$GRAPH" | jq -r '.nodes[] | "    \(.node_type) \(.qualified_name // .name)"' 2>/dev/null | sort | head -15 | while read -r line; do
      echo -e "  ${DIM}${line}${NC}"
    done
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
step 12 "Verify full provenance chain"
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

# =============================================================================
# Summary
# =============================================================================
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
if [ ${#ISSUES[@]} -eq 0 ]; then
  echo -e "${BOLD}║  ${GREEN}All checks passed!${NC}${BOLD}                                         ║${NC}"
else
  echo -e "${BOLD}║  ${YELLOW}${#ISSUES[@]} issue(s) found${NC}${BOLD}                                           ║${NC}"
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
echo -e "      ↓  merged + attested"
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
