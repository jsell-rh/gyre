Prepare a decision for a human operator. This is read-only triage of a task/spec
conflict, not implementation. Do not change any tracked files, requirements,
progress, coverage, guards or code. Do not run builds or tests. The task is held
until the operator chooses a direction and a reviewed spec amendment lands.

Read the supplied current requirement inputs, the conflict evidence and the
referenced canonical spec/task documents. Explain the problem in simple technical
English, expanding uncommon terms. State what the old task asks for, what the
newer spec says, and why blindly implementing either would have consequences.

Write /tmp/stage/decision.json as valid JSON with exactly these fields:
{
  "question":"One concrete decision the operator can make",
  "context":"Enough factual context to understand the conflict and its stakes",
  "options":[
    {"id":"option_a","title":"Short clear choice","summary":"What changes","tradeoffs":"Benefits, costs and risks","next_step":"What the loop will do if selected"},
    {"id":"option_b","title":"A different direction","summary":"What changes","tradeoffs":"Benefits, costs and risks","next_step":"What the loop will do if selected"},
    {"id":"option_c","title":"A third viable direction","summary":"What changes","tradeoffs":"Benefits, costs and risks","next_step":"What the loop will do if selected"}
  ],
  "recommendation":"option_a",
  "sources":["specs/system/relevant-spec.md"]
}

Offer three materially different options, not three phrasings of the same choice.
Recommend one with a reason in its tradeoffs. Do not manufacture evidence or claim
industry consensus. Cite the repository documents supporting the context. A choice
will create a requirements-amendment task with a versioned decision record; code
implementation resumes only after independent review and the amendment's merge.
The recommendation is not an automatic selection. Do not claim the task resolved.
