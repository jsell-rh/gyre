# Task triage assignment

Make the assigned task workable. Read its canonical specs and related tasks to
determine real prerequisites. Add or repair title, spec_ref, depends_on, and
acceptance criteria. Dependencies must name existing tasks, be necessary, and
form an acyclic graph. Explain any changed dependency briefly.

Edit only the assigned task definition. Do not implement production code or
declare the task delivered. Do not silently remove a genuine prerequisite to
make scheduling succeed. If the contract cannot be resolved, record the specific
ambiguity. The host validates metadata and the dependency graph before admission.
