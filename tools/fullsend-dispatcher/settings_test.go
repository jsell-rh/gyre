package main

import (
	"context"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"path/filepath"
	"strings"
	"testing"
)

func TestConfigurationTabEditingDisclosureAndTerminalFrame(t *testing.T) {
	m := sampleModel()
	defer m.cancel()
	m.engine.c.SettingsFile = "/home/test/gyre/pipeline.sqlite3"
	m.engine.c.Settings = []Setting{{Key: "concurrency", Label: "Worker limit", Value: "10", Kind: "number", Effect: "Immediate"},
		{Key: "repo", Label: "Repository", Value: "test/repo", Kind: "repo", Effect: "Restart required"},
		{Key: "model", Label: "Pinned model", Value: "Configured (hidden)", Kind: "readonly", Effect: "Read only"}}
	update := func(key tea.KeyMsg) { next, _ := m.Update(key); m = next.(Model) }
	update(tea.KeyMsg{Type: tea.KeyTab})
	update(tea.KeyMsg{Type: tea.KeyTab})
	if m.screen != "configuration" {
		t.Fatal("third tab is missing")
	}
	for _, size := range [][2]int{{100, 28}, {42, 24}} {
		m.width, m.height = size[0], size[1]
		view := m.View()
		if !strings.Contains(view, "SHARED GYRE LEDGER") || !strings.Contains(view, "persist") {
			t.Fatal(view)
		}
		if lipgloss.Height(view) > m.height {
			t.Fatal("configuration frame overflows")
		}
		for _, line := range strings.Split(view, "\n") {
			if lipgloss.Width(line) > m.width {
				t.Fatal("configuration line overflows")
			}
		}
	}
	update(tea.KeyMsg{Type: tea.KeyEnter})
	if !m.configEditing {
		t.Fatal("editor did not open")
	}
	update(tea.KeyMsg{Type: tea.KeyCtrlU})
	update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune("q?")})
	if m.ctx.Err() != nil || m.helpOpen || m.configInput != "q?" {
		t.Fatal("work shortcuts intercepted text input")
	}
	update(tea.KeyMsg{Type: tea.KeyEsc})
	if m.configEditing {
		t.Fatal("discard did not close editor")
	}
	update(tea.KeyMsg{Type: tea.KeyTab})
	if m.screen != "" || m.issueGrouping {
		t.Fatal("tab did not return to agents")
	}
}

// Verifies: SDLC-LOC-12
func TestConfigurationReturnClampsHistorySelection(t *testing.T) {
	m := sampleModel()
	defer m.cancel()
	m.engine.c.Settings = []Setting{{Key: "concurrency", Value: "2", Kind: "number", Effect: "Immediate"}}
	issue := m.state.Rows[0]
	m.historyIssue = &issue
	for i := 0; i < 20; i++ {
		m.historyRows = append(m.historyRows, issue)
	}
	m.index = 19
	updated, _ := m.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune(",")})
	m = updated.(Model)
	updated, _ = m.Update(tea.KeyMsg{Type: tea.KeyTab})
	m = updated.(Model)
	if m.historyIssue != nil || m.index >= len(m.visibleRows()) {
		t.Fatal("configuration retained stale history selection")
	}
	_ = m.View()
}

// Exercises the actual Go -> Python adapter -> SQLite write and refresh path.
func TestConfigurationSaveUsesGyreLedgerAndRefreshesExternalChanges(t *testing.T) {
	t.Setenv("FULLSEND_DISPATCH_PYTHON", "/usr/bin/python3")
	root, err := filepath.Abs("../..")
	if err != nil {
		t.Fatal(err)
	}
	state := t.TempDir()
	a := Adapter{Path: filepath.Join(root, "scripts/dispatch-adapter.py"), Args: []string{"--state", state}}
	var c Config
	if err = a.call(context.Background(), "config", nil, &c); err != nil {
		t.Fatal(err)
	}
	b := &AdapterBackend{adapter: a}
	e := &Engine{c: c, a: a, store: b}
	if _, err = b.rows(); err != nil {
		t.Fatal(err)
	}
	result := e.requestSetting(context.Background(), "slots", "45", "8")
	if result.err != nil {
		t.Fatal(result.err)
	}
	if _, err = b.rows(); err != nil {
		t.Fatal(err)
	}
	if b.currentLimit() != 45 {
		t.Fatal("capacity not applied")
	}
	// A CLI/browser update arrives while an editor holds the previous value.
	var reply any
	if err = a.call(context.Background(), "settings-save", map[string]string{"scope": c.Scope, "key": "slots", "value": "37", "expected": "45"}, &reply); err != nil {
		t.Fatal(err)
	}
	result = e.requestSetting(context.Background(), "slots", "12", "45")
	if result.err == nil || !strings.Contains(result.err.Error(), "another controller") {
		t.Fatal("stale editor overwrote external change")
	}
	if _, err = b.rows(); err != nil {
		t.Fatal(err)
	}
	if b.currentLimit() != 37 {
		t.Fatal("external capacity not refreshed")
	}
	for _, row := range e.config().Settings {
		if row.Key == "slots" && row.Value != "37" {
			t.Fatal("editor does not share effective value")
		}
	}
	result = e.requestSetting(context.Background(), "model", "other-model", "gateway/rits/zai-org/glm-5-3")
	if result.err == nil {
		t.Fatal("pinned route changed through editor")
	}
	var reopened Config
	if err = a.call(context.Background(), "config", nil, &reopened); err != nil {
		t.Fatal(err)
	}
	if reopened.Concurrency != 37 {
		t.Fatal("saved setting did not survive reopening ledger")
	}
}
