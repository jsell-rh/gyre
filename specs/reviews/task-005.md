# Review: TASK-005 — CEL Constraint Evaluator

**Reviewer:** Verifier  
**Date:** 2026-04-08 (R1)  
**Verdict:** needs-revision

---

## R1 Findings

- [ ] **F1 — Multiple persona constraints generate logically unsatisfiable CEL expressions — all-must-pass semantics make multi-persona authorization impossible.**  
  The spec (authorization-provenance.md §2.2, line 107) defines `persona_constraints: PersonaRef[]` with the note `-- required persona(s) for implementation`, indicating multiple personas are valid. The spec (§3.2, lines 203-207) shows only a single-entry example: `agent.persona == input.persona_constraints[0].name`. The implementation (`constraint_evaluator.rs:272-277`) handles multiple entries by generating one equality constraint per entry:  
  ```rust
  for (i, persona_ref) in content.persona_constraints.iter().enumerate() {
      constraints.push(OutputConstraint {
          name: format!("strategy: agent persona must match '{}'", persona_ref.name),
          expression: format!("agent.persona == input.persona_constraints[{}].name", i),
      });
  }
  ```
  Since constraints are additive and evaluated sequentially with all-must-pass semantics (§3.4), two persona entries (e.g., `["security", "compliance"]`) produce two constraints that require `agent.persona` to simultaneously equal both `"security"` AND `"compliance"`. Since `agent.persona` is a scalar string (§3.3, line 288), this is logically impossible — no agent can ever satisfy multi-persona authorization.  
  The test `derive_multiple_persona_constraints` (line 776) validates the broken behavior: it asserts that two separate index-based constraints are generated, but never evaluates them against a CEL context to discover the contradiction.  
  Fix: Replace the per-entry loop with a single membership constraint: `input.persona_constraints.exists(p, p.name == agent.persona)`. This evaluates to true if the agent's persona matches ANY entry in the list — the only interpretation that is semantically coherent for a scalar `agent.persona` field. Update the test to evaluate the generated constraint against a context with multiple persona entries and a matching agent.  
  **Files:** `crates/gyre-domain/src/constraint_evaluator.rs:272-277` (broken loop), `crates/gyre-domain/src/constraint_evaluator.rs:776-798` (test validates broken behavior), spec §3.2 lines 203-207 (persona constraint definition), spec §2.2 line 107 (`PersonaRef[]` type).
