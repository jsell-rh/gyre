package main

// Backend separates dispatch and terminal interaction from a project's durable
// workflow. The SQLite backend remains available for the original integration;
// adapter backends can retain an existing ledger and detached workers.
import (
	"context"
	"errors"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"time"
)

var errNotAdmitted = errors.New("work is waiting for admission")

type Backend interface {
	rows() ([]Row, error)
	recover() error
	launch(Config, Adapter, Job) error
	retry(string) error
	retryFailed(string, string) error
	retryConfirmedUnknown(string, string, string) error
	requestCancel(string, string, string) error
	history(Row) ([]Row, error)
	exitLogs(string) error
}

type AdapterBackend struct {
	adapter    Adapter
	limit      atomic.Int64
	settingsMu sync.RWMutex
	settings   []Setting
}

func (b *AdapterBackend) call(action string, input, output any) error {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	return b.adapter.call(ctx, action, input, output)
}
func (b *AdapterBackend) rows() ([]Row, error) {
	var result struct {
		Rows     []Row     `json:"rows"`
		Capacity int       `json:"capacity"`
		Settings []Setting `json:"settings"`
	}
	err := b.call("rows", nil, &result)
	if err == nil {
		b.limit.Store(int64(result.Capacity))
		b.setSettings(result.Settings)
	}
	return result.Rows, err
}
func (b *AdapterBackend) setSettings(rows []Setting) {
	b.settingsMu.Lock()
	defer b.settingsMu.Unlock()
	b.settings = append([]Setting(nil), rows...)
}
func (b *AdapterBackend) currentSettings() []Setting {
	b.settingsMu.RLock()
	defer b.settingsMu.RUnlock()
	return append([]Setting(nil), b.settings...)
}
func (b *AdapterBackend) currentLimit() int { return int(b.limit.Load()) }
func (b *AdapterBackend) recover() error {
	var result struct{}
	return b.call("recover", nil, &result)
}
func (b *AdapterBackend) launch(_ Config, _ Adapter, job Job) error {
	var result struct {
		Started bool `json:"started"`
	}
	if err := b.call("launch", job, &result); err != nil {
		return err
	}
	if !result.Started {
		return errNotAdmitted
	}
	return nil
}
func (b *AdapterBackend) retry(key string) error { return b.retryFailed(key, "") }
func (b *AdapterBackend) retryFailed(key, scope string) error {
	var result struct{}
	return b.call("retry", map[string]string{"key": key, "scope": scope}, &result)
}
func (b *AdapterBackend) retryConfirmedUnknown(key, scope, id string) error {
	var result struct{}
	return b.call("retry", map[string]string{"key": key, "scope": scope, "run_id": id}, &result)
}
func (b *AdapterBackend) requestCancel(key, scope, id string) error {
	var result struct{}
	return b.call("cancel", map[string]string{"key": key, "scope": scope, "run_id": id}, &result)
}
func (b *AdapterBackend) history(row Row) ([]Row, error) {
	var result []Row
	err := b.call("history", row, &result)
	return result, err
}
func (b *AdapterBackend) exitLogs(scope string) error {
	rows, err := b.rows()
	if err != nil {
		return err
	}
	for _, row := range rows {
		if row.Scope == scope && row.Status == "running" {
			fmt.Fprintln(os.Stderr, row.Title, row.Log)
		}
	}
	return nil
}

func (e *Engine) capacity() int {
	if backend, ok := e.store.(interface{ currentLimit() int }); ok {
		return backend.currentLimit()
	}
	return e.c.Concurrency
}

func (e *Engine) checkConnection(ctx context.Context) error {
	if e.c.AdapterBackend {
		var result struct{}
		return e.a.call(ctx, "health", nil, &result)
	}
	cmd := contextCommand(ctx, "openshell", "whoami", "--output", "json")
	data, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("Gateway check failed: %s", string(data))
	}
	return nil
}
