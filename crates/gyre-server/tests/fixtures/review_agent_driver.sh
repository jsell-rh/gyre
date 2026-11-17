#!/bin/sh
# Review agent driver for gate_executor end-to-end tests.
# Simulates a real single-minded review agent: reads its scoped identity and
# MR context from the environment, submits its verdict via the Review API
# (POST /api/v1/merge-requests/:id/reviews) with the scoped JWT.
#
# Usage: review_agent_driver.sh <approved|changes_requested>
#
# Emits the received context to stdout so the test can assert delivery
# (diff file, spec file, persona prompt, task description).
set -u

VERDICT="${1:-approved}"
MR_ID="$GYRE_MR_ID"
SERVER="$GYRE_SERVER_URL"
TOKEN="$GYRE_REVIEW_TOKEN"

echo "persona_prompt=$GYRE_PERSONA_PROMPT"
echo "task_description=$GYRE_TASK_DESCRIPTION"
echo "spec_ref=$GYRE_SPEC_REF"
echo "spec_content=$GYRE_SPEC_CONTENT"
echo "mr_title=$GYRE_MR_TITLE"
if [ -n "${GYRE_DIFF_FILE:-}" ]; then
    echo "diff_first_line=$(head -n 1 "$GYRE_DIFF_FILE")"
fi
if [ -n "${GYRE_SPEC_FILE:-}" ]; then
    echo "spec_file_first_line=$(head -n 1 "$GYRE_SPEC_FILE")"
fi

if [ "$VERDICT" = "approved" ]; then
    BODY="LGTM: satisfies the acceptance criteria."
else
    BODY="The diff violates the persona criteria."
fi

HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST "$SERVER/api/v1/merge-requests/$MR_ID/reviews" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"reviewer_agent_id\": \"forged-id-attempt\", \"decision\": \"$VERDICT\", \"body\": \"$BODY\"}")
echo "http_code=$HTTP_CODE"

# A single-minded agent succeeds only when its verdict was accepted.
[ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "201" ]
