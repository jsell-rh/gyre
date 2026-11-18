Decompose the confirmed gaps in Planning context into a bounded, dependency-aware plan.
This is read-only planning: do not edit tracked files, specs, coverage or code and do
not run tests/builds. Read the canonical spec and evidence; preserve its full behavior.

First inspect the supplied task catalog and relevant task documents. Reuse an unfinished
existing task only if its actual contract covers the gap. A finished task is historical
context, not proof the behavior works. If it missed behavior, propose a narrowly scoped
delta task rather than replaying the entire original implementation. Avoid overlapping
active work. Partition genuinely independent capabilities; dependencies must express
real requirements, not arbitrary serialization. No speculative features or test-only
busywork. A negative probe must describe a meaningful failure that proves enforcement
or real behavior, rather than asserting that a symbol exists.

Write /tmp/stage/planning.json:
{"audit":"SHA256 DIGEST OF SUPPLIED AUDIT (provided in Exact assignment)",
 "tasks":[{"key":"stable-short-key", "title":"Concrete deliverable",
 "covers":["gap heading IDs, each owned exactly once across tasks"],
 "required_behavior":"Precise behavior faithful to the canonical spec, including enforcement",
 "acceptance":["Concrete observable requirement"],
 "negative_probe":"A targeted probe and the real defect that must make it fail",
 "dependencies":["another local key or existing task-NNN"],
 "existing_task":null}]}

Use 1–8 tasks. For reused tasks set existing_task to its name; otherwise null. Every
confirmed gap must be covered exactly once. Do not mark tasks complete, amend specs,
weaken acceptance criteria or make production/coverage changes.
