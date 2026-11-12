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

func TestDeliveredWorkLeavesPendingListButRemainsVisibleInCompletedAndDetails(t *testing.T) {
	m := sampleModel()
	defer m.cancel()
	for _, state := range []string{"merged", "resolved"} {
		r := Row{Job: Job{Key: "delivered", Scope: m.engine.c.Scope, Agent: "publish", Item: "task-1",
			CodeReadiness: &CodeReadiness{Status: state}}, Status: "succeeded"}
		m.state.Rows = []Row{r}
		m.showCompleted = false
		m.screen = ""
		if len(m.visibleRows()) != 0 {
			t.Fatal("delivered task still appears in pending work", state)
		}
		m.showCompleted = true
		if len(m.visibleRows()) != 1 || strings.Contains(rowStatus(r), "awaiting merge") || rowColor(r) != "2" {
			t.Fatal("completed work lost its delivery outcome or green styling", rowStatus(r))
		}
		m.showCompleted = false
		m.selected, m.screen = r.Key, "details"
		if len(m.visibleRows()) != 1 {
			t.Fatal("delivery confirmation closed the selected detail view")
		}
		r.Activity = "history"
		if rowStatus(r) == "Merged" || rowStatus(r) == "Requirements satisfied upstream" {
			t.Fatal("current delivery rewrote historical stage outcome")
		}
	}
}

func TestBlockedAndRefreshingWorkCannotLookMergeReadyOrDisappear(t *testing.T) {
	m := sampleModel()
	defer m.cancel()
	for state, want := range map[string]string{
		"verification_blocked": "Verification blocked · out of memory",
		"refresh_required":     "Integration refresh required",
		"refresh_queued":       "Integration refresh queued",
		"refresh_running":      "Integration refresh running",
		"repair_pending":       "Implementation repair pending",
		"merge_confirmation":   "Confirming upstream merge",
	} {
		r := Row{Job: Job{Key: "blocked", Scope: m.engine.c.Scope, Agent: "publish", Item: "task-1",
			CodeReadiness: &CodeReadiness{Status: state, Blocker: "sandbox_oom"}}, Status: "succeeded"}
		m.state.Rows = []Row{r}
		if len(m.visibleRows()) != 1 || rowStatus(r) != want {
			t.Fatalf("%s: hidden or wrong status %q", state, rowStatus(r))
		}
	}
}
