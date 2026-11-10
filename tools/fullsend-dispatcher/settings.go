// Settings editor adapted from HyperShell Fullsend; see UPSTREAM.json.
package main

import (
	"context"
	"fmt"
	"github.com/charmbracelet/x/ansi"
	"time"
)

type Setting struct {
	Key     string   `json:"key"`
	Label   string   `json:"label"`
	Value   string   `json:"value"`
	Effect  string   `json:"effect"`
	Kind    string   `json:"kind"`
	Choices []string `json:"choices"`
	Help    string   `json:"help"`
	Saved   *string  `json:"saved"`
}
type settingsResult struct {
	err     error
	message string
}
type settingsDone settingsResult

// The adapter ledger owns live configuration. Never mutate launch config from
// a UI goroutine or create another settings file beside the workflow ledger.
func (e *Engine) config() Config {
	c := Config{SettingsFile: e.c.SettingsFile, Settings: e.c.Settings}
	if b, ok := e.store.(*AdapterBackend); ok {
		if rows := b.currentSettings(); rows != nil {
			c.Settings = rows
		}
	}
	return c
}
func (e *Engine) requestSetting(ctx context.Context, key, value, expected string) settingsResult {
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	b, ok := e.store.(*AdapterBackend)
	if !ok {
		return settingsResult{err: fmt.Errorf("configuration requires the Gyre adapter backend")}
	}
	var reply struct {
		Settings []Setting `json:"settings"`
		Message  string    `json:"message"`
	}
	err := e.a.call(ctx, "settings-save", map[string]string{"scope": e.c.Scope, "key": key, "value": value, "expected": expected}, &reply)
	if err != nil {
		return settingsResult{err: err}
	}
	b.setSettings(reply.Settings)
	return settingsResult{message: reply.Message}
}
func (m Model) configurationView(lines []string, w, h int) string {
	c := m.engine.config()
	lines = append(lines, m.viewTabsFor(2, w)...)
	lines = append(lines, "")
	lines = append(lines, m.style("SHARED GYRE LEDGER", "8", true))
	lines = append(lines, m.style(middle(c.SettingsFile, w), "6", false))
	lines = append(lines, m.wrapped("Shared with browser/CLI. Changes persist.", w-2, "8")...)
	lines = append(lines, "")
	if m.state.Monitoring {
		lines = append(lines, m.wrapped("Monitoring another dispatcher. These are launch settings; edit in its owner terminal.", w-2, "3")...)
	}
	if len(c.Settings) == 0 {
		lines = append(lines, "Configuration is not available for this launcher.", "", m.style("esc back   q detach", "8", false))
		return m.frame(lines, w, h)
	}
	index := min(m.configIndex, len(c.Settings)-1)
	row := c.Settings[index]
	if m.configEditing {
		lines = append(lines, m.style(row.Label, "7", true), m.style(row.Effect, "3", false))
		lines = append(lines, m.wrapped("Active now: "+row.Value, w-2, "8")...)
		if row.Kind == "readonly" {
			lines = append(lines, m.wrapped(row.Value, w-2, "7")...)
			lines = append(lines, "", m.style(row.Help, "8", false), "", m.style("enter / esc back   q detach", "8", false))
			return m.frame(lines, w, h)
		}
		input := m.configInput
		if row.Kind == "choice" {
			for i, choice := range row.Choices {
				prefix := "  "
				if i == m.configChoice {
					prefix = "> "
				}
				line := prefix + choice
				if i == m.configChoice {
					line = m.selection(line, w-2)
				}
				lines = append(lines, line)
			}
		} else {
			cursor := min(m.configCursor, len([]rune(input)))
			r := []rune(input)
			input = string(r[:cursor]) + "▏" + string(r[cursor:])
			start := max(0, ansi.StringWidth(string(r[:cursor]))-max(1, w-5))
			lines = append(lines, m.style("> "+ansi.Cut(input, start, start+max(1, w-4)), "6", true))
		}
		lines = append(lines, "")
		lines = append(lines, m.wrapped(row.Help, w-2, "8")...)
		if m.configError != "" {
			lines = append(lines, m.wrapped(m.configError, w-2, "1")...)
		}
		footer := "enter save   esc discard   ctrl+c detach"
		if m.configSaving {
			footer = "Saving…"
		}
		lines = append(lines, "", m.style(footer, "8", false))
		return m.frame(lines, w, h)
	}
	content := []string{}
	positions := make([]int, len(c.Settings))
	previous := ""
	for i, item := range c.Settings {
		if item.Effect != previous {
			if len(content) > 0 {
				content = append(content, "")
			}
			content = append(content, m.groupHeading(item.Effect, "", w-2))
			previous = item.Effect
		}
		positions[i] = len(content)
		value := item.Value
		if value == "" {
			value = "Not set / default"
		}
		line := "  " + cell(item.Label, max(1, w/2-2)) + " " + middle(value, max(1, w/2-3))
		if i == index {
			line = m.selection("> "+line[2:], w-2)
		}
		content = append(content, line)
		if item.Saved != nil && *item.Saved != item.Value {
			content = append(content, m.style("  Saved: "+middle(*item.Saved, max(1, w-12))+" · inactive", "3", false))
		}
	}
	count := max(1, h-len(lines)-5)
	start := max(0, positions[index]-count+1)
	end := min(len(content), start+count)
	lines = append(lines, m.withScrollbar(content[start:end], start, end, len(content), count, w)...)
	detail := row.Help
	if row.Saved != nil && *row.Saved != row.Value {
		detail = "Saved: " + *row.Saved + " · Active: " + row.Value + ". Restart required."
		if row.Effect != "Restart required" {
			detail = "A launch argument overrides the saved value on restart."
		}
	}
	lines = append(lines, m.style(middle(detail, w), "3", false), m.scrollIndicator(start, end, len(content)), "", m.style("j/k select   enter edit   tab Agents   esc back   ? help   q detach", "8", false))
	return m.frame(lines, w, h)
}
