#!/usr/bin/env bash
# =============================================================================
# e2e-trace-emitter.sh — Emit realistic OTLP spans for trace capture gate
#
# Called by the TraceCapture gate executor with:
#   OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port>
#   OTEL_EXPORTER_OTLP_PROTOCOL=http/json
#   OTEL_SERVICE_NAME=gyre-gate-test
#
# Emits a realistic integration test trace tree:
#   test_suite (root)
#     ├── test_greeting_endpoint
#     │     ├── POST /api/greet (server)
#     │     │     └── db.query: SELECT greeting (db)
#     │     └── assert_response (internal)
#     ├── test_health_check
#     │     └── GET /api/health (server)
#     └── test_config_validation
#           └── validate_config (internal)
# =============================================================================

set -euo pipefail

ENDPOINT="${OTEL_EXPORTER_OTLP_ENDPOINT:-http://127.0.0.1:4318}"
SVC="${OTEL_SERVICE_NAME:-e2e-test-svc}"

# Unique trace ID (32 hex chars)
TRACE_ID=$(printf '%032x' $((RANDOM * RANDOM * RANDOM)))

# Base time: "now" in nanoseconds
BASE_NS=$(date +%s)000000000

emit_span() {
  local span_id="$1"
  local parent_id="${2:-}"
  local name="$3"
  local kind="${4:-2}"  # 1=client, 2=server, 3=producer, 5=internal
  local start_offset_ms="${5:-0}"
  local duration_ms="${6:-100}"
  local status_code="${7:-1}"  # 1=OK, 2=ERROR
  local attrs="${8:-[]}"

  local start_ns=$((BASE_NS + start_offset_ms * 1000000))
  local end_ns=$((start_ns + duration_ms * 1000000))

  local parent_field=""
  if [ -n "$parent_id" ]; then
    parent_field=",\"parentSpanId\": \"${parent_id}\""
  fi

  local body
  body=$(cat <<ENDJSON
{
  "resourceSpans": [{
    "resource": {
      "attributes": [
        {"key": "service.name", "value": {"stringValue": "${SVC}"}},
        {"key": "service.version", "value": {"stringValue": "1.0.0"}}
      ]
    },
    "scopeSpans": [{
      "scope": {"name": "e2e-test-runner"},
      "spans": [{
        "traceId": "${TRACE_ID}",
        "spanId": "${span_id}"
        ${parent_field},
        "name": "${name}",
        "kind": ${kind},
        "startTimeUnixNano": "${start_ns}",
        "endTimeUnixNano": "${end_ns}",
        "attributes": ${attrs},
        "status": {"code": ${status_code}}
      }]
    }]
  }]
}
ENDJSON
)

  curl -sf -X POST "${ENDPOINT}/v1/traces" \
    -H "Content-Type: application/json" \
    -d "$body" >/dev/null 2>&1 || true
}

echo "Emitting trace spans to ${ENDPOINT}..."

# Root: test_suite
emit_span "aaa0000000000001" "" \
  "test_suite" 5 0 500 1

# Test 1: test_greeting_endpoint
emit_span "aaa0000000000002" "aaa0000000000001" \
  "test_greeting_endpoint" 5 10 200 1

# POST /api/greet (server span)
emit_span "aaa0000000000003" "aaa0000000000002" \
  "POST /api/greet" 2 20 150 1 \
  '[{"key":"http.method","value":{"stringValue":"POST"}},{"key":"http.route","value":{"stringValue":"/api/greet"}},{"key":"http.status_code","value":{"intValue":"200"}}]'

# db.query (client/db span)
emit_span "aaa0000000000004" "aaa0000000000003" \
  "db.query: SELECT greeting" 1 30 50 1 \
  '[{"key":"db.system","value":{"stringValue":"sqlite"}},{"key":"db.statement","value":{"stringValue":"SELECT greeting FROM greetings WHERE lang = ?"}}]'

# assert_response
emit_span "aaa0000000000005" "aaa0000000000002" \
  "assert_response" 5 180 20 1

# Test 2: test_health_check
emit_span "aaa0000000000006" "aaa0000000000001" \
  "test_health_check" 5 220 80 1

# GET /api/health (server span)
emit_span "aaa0000000000007" "aaa0000000000006" \
  "GET /api/health" 2 225 50 1 \
  '[{"key":"http.method","value":{"stringValue":"GET"}},{"key":"http.route","value":{"stringValue":"/api/health"}},{"key":"http.status_code","value":{"intValue":"200"}}]'

# Test 3: test_config_validation
emit_span "aaa0000000000008" "aaa0000000000001" \
  "test_config_validation" 5 310 80 1

# validate_config (internal)
emit_span "aaa0000000000009" "aaa0000000000008" \
  "validate_config" 5 315 60 1 \
  '[{"key":"code.function","value":{"stringValue":"greeting_service::config::validate"}}]'

echo "Emitted 9 spans (trace_id=${TRACE_ID})"
