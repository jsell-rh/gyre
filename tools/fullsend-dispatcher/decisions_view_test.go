package main

import (
	tea "github.com/charmbracelet/bubbletea"
	"strings"
	"testing"
)

func TestDecisionsRequireExplicitChoiceAndConfirmation(t *testing.T) {
	m := sampleModel()
	defer m.cancel()
	m.screen = "decisions"
	m.decisionChoice = -1
	m.decisions = []HumanDecision{{ID: "decision", Task: "task-082", Question: "Which navigation?", Context: "The task and newer spec disagree.", Recommendation: "contextual", Options: []DecisionOption{{ID: "contextual", Title: "Contextual sidebar", Summary: "Scoped navigation", Tradeoffs: "More design work", Next: "Review spec amendment"}, {ID: "none", Title: "No sidebar"}, {ID: "fixed", Title: "Fixed sidebar"}}}}
	_, cmd := m.updateDecisions(tea.KeyMsg{Type: tea.KeyEnter})
	if cmd != nil {
		t.Fatal("Enter selected the recommendation without a choice")
	}
	model, _ := m.updateDecisions(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("2")})
	m = model.(Model)
	if m.decisionChoice != 1 {
		t.Fatal("explicit choice not retained")
	}
	if !strings.Contains(m.View(), "Press Enter to record") {
		t.Fatal("missing confirmation")
	}
	model, _ = m.updateDecisions(tea.KeyMsg{Type: tea.KeyEsc})
	m = model.(Model)
	if m.screen != "" || m.decisionChoice != -1 {
		t.Fatal("escape recorded or retained a choice")
	}
}
