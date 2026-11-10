package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// This adapter records actual engine launch requests. A running model occupies
// the only slot; cleanup must still run, and the queued model must not launch.
func adapterFixture(t *testing.T) (Adapter, Config) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "adapter.py")
	code := `import json,sys,pathlib
r=pathlib.Path(__file__).parent
a=sys.argv[1]
base={'scope':str(r),'agent':'code-implementation','item':'one','title':'Existing worker','key':'existing','fingerprint':'one'}
if a=='rows':
 print(json.dumps({'capacity':1,'rows':[dict(base,status='running',run_id='owned',started=1,log=''),dict(base,key='priority',item='new/control',status='queued',started=100,log='')]}))
elif a=='discover':
 print(json.dumps({'jobs':[dict(base,key='priority',item='new/control',unmetered=True),dict(base,key='queued',item='two'),dict(base,key='cleanup',item='old/resource',unmetered=True)],'errors':[]}))
elif a=='authorize':
 request=json.load(sys.stdin); print(json.dumps({'allowed':request['scope']==str(r)}))
elif a=='launch':
 request=json.load(sys.stdin)
 with (r/'launches').open('a') as f: f.write(request['key']+'\n')
 print(json.dumps({'started':request['key'] in ('priority','cleanup')}))
else: print('{}')
`
	if err := os.WriteFile(path, []byte(code), 0600); err != nil {
		t.Fatal(err)
	}
	return Adapter{Path: path}, Config{AdapterBackend: true, Scope: dir, Watch: true,
		Concurrency: 1, Poll: 1, AuthRetry: 1, AuthCheck: 60, StateDir: dir,
		SupervisorLock: filepath.Join(dir, "supervisor.lock"), LogDir: filepath.Join(dir, "logs")}
}

func TestAdapterBackendGlobalAdmissionAndIndependentInstances(t *testing.T) {
	t.Setenv("FULLSEND_DISPATCH_PYTHON", "python3")
	a, c := adapterFixture(t)
	otherAdapter, otherConfig := adapterFixture(t)
	contexts := []context.Context{}
	cancels := []context.CancelFunc{}
	done := []chan int{}
	for _, item := range []struct {
		a Adapter
		c Config
	}{{a, c}, {otherAdapter, otherConfig}} {
		ctx, cancel := context.WithCancel(context.Background())
		contexts = append(contexts, ctx)
		cancels = append(cancels, cancel)
		backend := &AdapterBackend{adapter: item.a}
		backend.limit.Store(1)
		engine, err := newEngine(item.c, item.a, backend)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { engine.diagnostic.Close() })
		ch := make(chan int, 1)
		done = append(done, ch)
		go func() { ch <- engine.run(ctx) }()
	}
	defer func() {
		for _, cancel := range cancels {
			cancel()
		}
	}()
	deadline := time.Now().Add(10 * time.Second)
	for _, config := range []Config{c, otherConfig} {
		for {
			data, _ := os.ReadFile(filepath.Join(config.StateDir, "launches"))
			if strings.Contains(string(data), "cleanup\n") {
				break
			}
			if time.Now().After(deadline) {
				t.Fatalf("independent dispatcher did not launch cleanup: %s", config.StateDir)
			}
			time.Sleep(20 * time.Millisecond)
		}
		data, _ := os.ReadFile(filepath.Join(config.StateDir, "launches"))
		if strings.Contains(string(data), "queued") {
			t.Fatal("launched model while global capacity was occupied")
		}
		if !strings.HasPrefix(string(data), "priority\n") {
			t.Fatal("dispatcher overrode the domain adapter's control-work priority")
		}
		pid, err := os.ReadFile(config.SupervisorLock)
		if err != nil || string(pid) != fmt.Sprint(os.Getpid()) {
			t.Fatalf("wrong scoped supervisor lock: %q %v", pid, err)
		}
	}
	// Stopping one instance must not stop the other or hold its supervisor lock.
	cancels[0]()
	select {
	case <-done[0]:
	case <-time.After(5 * time.Second):
		t.Fatal("first dispatcher did not detach")
	}
	select {
	case <-done[1]:
		t.Fatal("stopping one dispatcher stopped its neighbor")
	default:
	}
	cancels[1]()
	select {
	case <-done[1]:
	case <-time.After(5 * time.Second):
		t.Fatal("second dispatcher did not detach")
	}
}
