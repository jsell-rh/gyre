package main

import (
	"strings"
	"testing"
)

// A completed code review is not a completed delivery. This ports the live
// readiness distinction from HyperShell dbe5edc6 without importing its labels.
func TestCIReadinessDoesNotHideCompletedReviewOrRewriteHistory(t *testing.T) {
	m := sampleModel()
	defer m.cancel()
	r := Row{Job: Job{Key: "review", Scope: m.engine.c.Scope, Agent: "code-review", Item: "task-1",
		Title: "Candidate", CodeReadiness: &CodeReadiness{Status: "pending"}}, Status: "succeeded"}
	m.state.Rows = []Row{r}
	if len(m.visibleRows()) != 1 || rowStatus(r) != "Waiting for CI" {
		t.Fatal("completed review disappeared while its CI was pending")
	}
	r.CodeReadiness.Status = "candidate_failed"
	r.CodeReadiness.RepairQueued = true
	if rowStatus(r) != "CI failed · repair queued" {
		t.Fatal(rowStatus(r))
	}
	r.Activity = "history"
	if strings.Contains(rowStatus(r), "CI") {
		t.Fatal("live CI rewrote a historical attempt outcome")
	}
}
