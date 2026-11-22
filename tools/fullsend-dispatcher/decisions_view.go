package main

import (
	"fmt"
	tea "github.com/charmbracelet/bubbletea"
	"strings"
)

type DecisionOption struct {
	ID        string `json:"id"`
	Title     string `json:"title"`
	Summary   string `json:"summary"`
	Tradeoffs string `json:"tradeoffs"`
	Next      string `json:"next_step"`
}
type HumanDecision struct {
	ID             string           `json:"id"`
	Task           string           `json:"task"`
	Question       string           `json:"question"`
	Context        string           `json:"context"`
	Options        []DecisionOption `json:"options"`
	Recommendation string           `json:"recommendation"`
	Sources        []string         `json:"sources"`
	Selection      *struct {
		Option string `json:"option"`
		Actor  string `json:"actor"`
		Source string `json:"source"`
	} `json:"selection"`
	ApplicationTask string `json:"application_task"`
}
type decisionsLoaded struct {
	rows []HumanDecision
	err  error
}
type decisionRecorded struct {
	task string
	err  error
}

func (m Model) loadDecisions() tea.Cmd {
	return func() tea.Msg {
		b, ok := m.engine.store.(*AdapterBackend)
		if !ok {
			return decisionsLoaded{err: fmt.Errorf("Human decisions require the Gyre adapter")}
		}
		var rows []HumanDecision
		err := b.call("decisions", nil, &rows)
		return decisionsLoaded{rows, err}
	}
}
func (m Model) updateDecisions(v tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch v.String() {
	case "esc":
		m.screen = ""
		m.decisionChoice = -1
	case "q", "ctrl+c":
		m.cancel()
		m.exit = 130
		return m, tea.Quit
	case "r":
		m.decisionError = ""
		return m, m.loadDecisions()
	case "j", "down":
		m.decisionScroll++
	case "k", "up":
		m.decisionScroll = max(0, m.decisionScroll-1)
	case "pgdown":
		m.decisionScroll += max(1, m.height/2)
	case "pgup":
		m.decisionScroll = max(0, m.decisionScroll-max(1, m.height/2))
	case "n", "p":
		if len(m.decisions) > 0 {
			delta := 1
			if v.String() == "p" {
				delta = -1
			}
			m.decisionIndex = (m.decisionIndex + delta + len(m.decisions)) % len(m.decisions)
			m.decisionChoice = -1
			m.decisionScroll = 0
		}
	case "1", "2", "3":
		if len(m.decisions) > 0 && !m.decisionSaving && m.decisions[m.decisionIndex].Selection == nil {
			m.decisionChoice = int(v.String()[0] - '1')
		}
	case "enter":
		if m.decisionSaving || len(m.decisions) == 0 || m.decisionChoice < 0 {
			return m, nil
		}
		d := m.decisions[m.decisionIndex]
		if d.Selection != nil || m.decisionChoice >= len(d.Options) {
			return m, nil
		}
		b, ok := m.engine.store.(*AdapterBackend)
		if !ok {
			return m, nil
		}
		m.decisionSaving = true
		return m, func() tea.Msg {
			var result struct {
				Task string `json:"application_task"`
			}
			err := b.call("choose-decision", map[string]string{"scope": m.engine.c.Scope, "id": d.ID, "option": d.Options[m.decisionChoice].ID}, &result)
			return decisionRecorded{result.Task, err}
		}
	}
	return m, nil
}
func (m Model) decisionsView(lines []string, w, h int) string {
	lines = append(lines, m.style("Decisions · three options, recorded intent", "7", true), "")
	content := []string{}
	confirmation := "Choose 1, 2 or 3. No option is selected automatically."
	if m.decisionError != "" {
		content = append(content, m.wrapped(m.decisionError, w-2, "1")...)
	}
	if len(m.decisions) == 0 {
		content = append(content, "No decisions awaiting you. Press r to refresh.")
	} else {
		d := m.decisions[min(m.decisionIndex, len(m.decisions)-1)]
		content = append(content, m.wrapped(fmt.Sprintf("%s · %d/%d · %s", d.Task, m.decisionIndex+1, len(m.decisions), d.Question), w-2, "7")...)
		content = append(content, "")
		content = append(content, m.wrapped(d.Context, w-2, "8")...)
		for i, o := range d.Options {
			marker := "  "
			if m.decisionChoice == i {
				marker = "> "
			}
			title := fmt.Sprintf("%s%d. %s", marker, i+1, o.Title)
			if d.Recommendation == o.ID {
				title += " (Recommended)"
			}
			content = append(content, "")
			content = append(content, m.wrapped(title, w-2, "6")...)
			for _, text := range []string{o.Summary, "Tradeoffs: " + o.Tradeoffs, "Next: " + o.Next} {
				content = append(content, m.wrapped(text, w-2, "7")...)
			}
		}
		content = append(content, "")
		if d.Selection != nil {
			confirmation = "Recorded " + d.Selection.Option + " by " + d.Selection.Actor + " via " + d.Selection.Source + ". Amendment task: " + d.ApplicationTask
		} else if m.decisionChoice >= 0 {
			confirmation = "Press Enter to record: " + d.Options[m.decisionChoice].Title + ". A reviewed requirements update will be queued."
		}
		for _, source := range d.Sources {
			content = append(content, m.wrapped(source, w-2, "8")...)
		}
	}
	confirmLines := m.wrapped(confirmation, w-2, "3")
	count := max(1, h-len(lines)-len(confirmLines)-4)
	start := min(m.decisionScroll, max(0, len(content)-count))
	lines = append(lines, m.scrollPane(content, start, count, w)...)
	lines = append(lines, "")
	lines = append(lines, confirmLines...)
	lines = append(lines, "", m.style(strings.Join([]string{"1/2/3 choose", "Enter record", "j/k scroll", "n/p decision", "r refresh", "Esc back"}, "  "), "8", false))
	return m.frame(lines, w, h)
}
