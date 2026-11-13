"""Durable work, fenced claims, findings, and resource reservations.

No Git, model, or gateway calls belong inside these transactions.
"""
from contextlib import contextmanager
import json
import random
from pathlib import Path
import sqlite3
import time
import uuid

STAGES = ('triage', 'assess', 'implement', 'review', 'verify', 'publish', 'observe', 'cleanup')


class StaleClaim(RuntimeError):
    pass


class Store:
    def __init__(self, directory):
        self.directory = Path(directory).resolve()
        self.directory.mkdir(parents=True, exist_ok=True)
        self.directory.chmod(0o700)
        self.db = sqlite3.connect(self.directory / 'pipeline.sqlite3', timeout=10,
                                  isolation_level=None)
        self.db.row_factory = sqlite3.Row
        self.db.executescript('''
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS tasks(
              name TEXT PRIMARY KEY, generation TEXT NOT NULL, body TEXT NOT NULL,
              data TEXT NOT NULL, updated REAL NOT NULL);
            CREATE TABLE IF NOT EXISTS work(
              id TEXT PRIMARY KEY, stage TEXT NOT NULL, task TEXT NOT NULL,
              generation TEXT NOT NULL, input_key TEXT NOT NULL, input TEXT NOT NULL,
              priority INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL DEFAULT 'ready',
              token INTEGER NOT NULL DEFAULT 0, owner TEXT, expires REAL,
              retry_at REAL NOT NULL DEFAULT 0, failures INTEGER NOT NULL DEFAULT 0,
              result TEXT, created REAL NOT NULL, updated REAL NOT NULL,
              UNIQUE(stage,task,generation,input_key));
            CREATE INDEX IF NOT EXISTS ready_work ON work(stage,state,retry_at,priority);
            CREATE TABLE IF NOT EXISTS findings(
              id TEXT PRIMARY KEY, work TEXT NOT NULL REFERENCES work(id),
              task TEXT NOT NULL, generation TEXT NOT NULL, source TEXT,
              category TEXT NOT NULL, detail TEXT NOT NULL, created REAL NOT NULL,
              resolved_by TEXT);
            CREATE TABLE IF NOT EXISTS resources(
              name TEXT PRIMARY KEY, work TEXT REFERENCES work(id), token INTEGER,
              kind TEXT NOT NULL, state TEXT NOT NULL, data TEXT NOT NULL,
              updated REAL NOT NULL);
            CREATE INDEX IF NOT EXISTS priority_findings ON findings(task,generation,resolved_by,category);
            CREATE INDEX IF NOT EXISTS resource_capacity ON resources(kind,state);
            CREATE TABLE IF NOT EXISTS events(
              id INTEGER PRIMARY KEY AUTOINCREMENT, task TEXT, work TEXT,
              kind TEXT NOT NULL, detail TEXT NOT NULL, at REAL NOT NULL);
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS work_events ON events(work,kind);
            CREATE TABLE IF NOT EXISTS task_history(
              id INTEGER PRIMARY KEY AUTOINCREMENT, task TEXT NOT NULL,
              generation TEXT NOT NULL, body TEXT NOT NULL, data TEXT NOT NULL, at REAL NOT NULL);
            CREATE INDEX IF NOT EXISTS history_task ON task_history(task,id);
            CREATE TRIGGER IF NOT EXISTS archive_contract BEFORE UPDATE ON tasks
              WHEN OLD.generation<>NEW.generation OR
                   json_extract(OLD.data,'$.delivered') IS NOT json_extract(NEW.data,'$.delivered')
              BEGIN INSERT INTO task_history(task,generation,body,data,at)
                VALUES(OLD.name,OLD.generation,OLD.body,OLD.data,unixepoch()); END;
            CREATE TRIGGER IF NOT EXISTS history_no_update BEFORE UPDATE ON task_history
              BEGIN SELECT RAISE(ABORT,'task history is immutable'); END;
            CREATE TRIGGER IF NOT EXISTS history_no_delete BEFORE DELETE ON task_history
              BEGIN SELECT RAISE(ABORT,'task history is immutable'); END;

        ''')
        (self.directory / 'pipeline.sqlite3').chmod(0o600)
        # Serialize additive migration with other dispatcher/worker starts.
        with self.transaction():
            if 'ready_since' not in {row['name'] for row in self.db.execute('PRAGMA table_info(work)')}:
                self.db.execute('ALTER TABLE work ADD COLUMN ready_since REAL NOT NULL DEFAULT 0')
            self.db.execute('UPDATE work SET ready_since=? WHERE ready_since=0',(time.time(),))
        self.db.execute('INSERT OR IGNORE INTO settings VALUES(?,?)', ('owner', json.dumps(uuid.uuid4().hex)))

    def close(self):
        self.db.close()

    @contextmanager
    def transaction(self):
        self.db.execute('BEGIN IMMEDIATE')
        try:
            yield
        except BaseException:
            self.db.rollback()
            raise
        else:
            self.db.commit()

    def event(self, kind, detail, task=None, work=None):
        self.db.execute('INSERT INTO events(task,work,kind,detail,at) VALUES(?,?,?,?,?)',
                        (task, work, kind, json.dumps(detail), time.time()))

    def put_task(self, name, generation, body, data, expected=None):
        with self.transaction():
            old = self.db.execute('SELECT * FROM tasks WHERE name=?', (name,)).fetchone()
            if expected is not None and (not old or old['generation'] != expected['generation']
                    or old['body'] != expected['body'] or json.loads(old['data']) != expected['data']):
                return False  # A stage completed while catalog inputs were being read.
            self.db.execute('''INSERT INTO tasks VALUES(?,?,?,?,?) ON CONFLICT(name)
                DO UPDATE SET generation=excluded.generation,body=excluded.body,
                data=excluded.data,updated=excluded.updated''',
                            (name, generation, body, json.dumps(data), time.time()))
            if old and old['generation'] != generation:
                self.db.execute("UPDATE work SET state='obsolete',token=token+1,updated=? WHERE task=? AND generation<>? AND state NOT IN ('succeeded','obsolete')",
                                (time.time(), name, generation))
                self.event('requirements_changed', {'generation': generation}, name)

    def tasks(self):
        return [dict(row) | {'data': json.loads(row['data'])}
                for row in self.db.execute('SELECT * FROM tasks ORDER BY name')]

    def task(self, name):
        row = self.db.execute('SELECT * FROM tasks WHERE name=?', (name,)).fetchone()
        if not row:
            raise KeyError(name)
        return dict(row) | {'data': json.loads(row['data'])}

    def findings(self, task):
        return [dict(row) | {'detail': json.loads(row['detail'])} for row in
                self.db.execute('SELECT * FROM findings WHERE task=? ORDER BY created', (task,))]

    def enqueue(self, stage, task, generation, inputs, priority=0):
        if stage not in STAGES:
            raise ValueError('unknown stage')
        encoded = json.dumps(inputs, sort_keys=True, separators=(',', ':'))
        identity = {key: value for key, value in inputs.items() if key != 'base'} if stage == 'implement' else inputs
        key = json.dumps(identity, sort_keys=True, separators=(',', ':'))
        # The complete input is the identity: a new candidate/base/repair gets
        # new work, while discovery replay adopts the existing outcome.
        with self.transaction():
            self.db.execute('''INSERT OR IGNORE INTO work
                (id,stage,task,generation,input_key,input,priority,created,updated,ready_since)
                VALUES(?,?,?,?,?,?,?,?,?,?)''',
                            (uuid.uuid4().hex, stage, task, generation, key, encoded,
                             priority, time.time(), time.time(),time.time()))
            self.db.execute("UPDATE work SET state='ready',updated=?,ready_since=? WHERE stage=? AND task=? AND generation=? AND input_key=? AND state='obsolete' AND EXISTS(SELECT 1 FROM tasks WHERE name=? AND generation=?)",
                            (time.time(),time.time(), stage, task, generation, key, task, generation))
            self.db.execute("UPDATE work SET input=?,updated=? WHERE stage=? AND task=? AND generation=? AND input_key=? AND state IN ('ready','retry')",
                            (encoded, time.time(), stage, task, generation, key))
            return self.db.execute('SELECT id FROM work WHERE stage=? AND task=? AND generation=? AND input_key=?',
                                   (stage, task, generation, key)).fetchone()['id']

    @staticmethod
    def runtime_hold_key(task, stage, generation):
        return f'runtime_hold:{task}:{stage}:{generation}'

    def runtime_held(self, work):
        return bool(self.setting(self.runtime_hold_key(work['task'],work['stage'],work['generation'])))

    def claim(self, stage, owner, concurrency=1, lease_seconds=90, now=None, task=None, eligible=None,
              ident=None, capacity=None):
        now = time.time() if now is None else now
        if concurrency < 0 or lease_seconds <= 0:
            raise ValueError('invalid claim limits')
        with self.transaction():
            expired = self.db.execute("SELECT id,task FROM work WHERE state='claimed' AND expires<=?", (now,)).fetchall()
            for row in expired:
                self.db.execute("UPDATE work SET state='ready',token=token+1,owner=NULL,expires=NULL,updated=?,ready_since=? WHERE id=?", (now,now,row['id']))
                self.event('lease_expired', {}, row['task'], row['id'])
            active = self.db.execute("SELECT count(*) FROM work WHERE stage=? AND state='claimed'", (stage,)).fetchone()[0]
            if active >= concurrency:
                return None
            compute = stage in ('triage', 'assess', 'implement', 'review', 'verify') and capacity is not None
            if compute and not self.admission_available(stage,capacity):
                return None
            rows = self.db.execute('''SELECT w.* FROM work w JOIN tasks t
                ON t.name=w.task AND t.generation=w.generation
                WHERE w.stage=? AND w.state IN ('ready','retry') AND w.retry_at<=?
                AND (? IS NULL OR w.task=?)
                AND (? IS NULL OR w.id=?)
                AND NOT EXISTS(SELECT 1 FROM work x WHERE x.task=w.task AND x.state='claimed'
                    AND (CASE WHEN w.stage='observe' THEN x.stage IN ('observe','publish')
                         WHEN w.stage='publish' THEN 1 ELSE x.stage<>'observe' END))
                ORDER BY w.priority DESC,w.created,w.id''', (stage, now, task, task, ident, ident)).fetchall()
            from .priority import sort_key, task_priority, PriorityError
            rows=sorted((dict(row) for row in rows),key=lambda row:sort_key(self,row,now))
            def valid_priority(row):
                if row['stage'] not in ('triage','assess','implement','review','verify'):
                    return True
                try:
                    task_priority(self,self.task(row['task']))
                    return True
                except PriorityError:
                    return False
            row = next((row for row in rows
                        if (not compute or not self.db.execute(
                            "SELECT 1 FROM resources r JOIN work w ON w.id=r.work WHERE w.task=? AND r.state<>'absent'", (row['task'],)).fetchone())
                        and not self.runtime_held(row)
                        and (not compute or self.admission_available(stage,capacity,work=row))
                        and self.integration_available(dict(row))
                        and valid_priority(row)
                        and (eligible is None or eligible(dict(row)))), None)
            if not row:
                return None
            self.db.execute("UPDATE work SET state='claimed',token=token+1,owner=?,expires=?,updated=? WHERE id=?",
                            (owner, now + lease_seconds, now, row['id']))
            claimed = dict(self.db.execute('SELECT * FROM work WHERE id=?', (row['id'],)).fetchone())
            claimed['input'] = json.loads(claimed['input'])
            if compute:
                from .compute import profile
                budget=profile(self,claimed['task'],claimed['stage'])
                name = self.admission_name(claimed['id'], claimed['token'])
                self.db.execute('INSERT INTO resources VALUES(?,?,?,?,?,?,?)',
                                (name, claimed['id'], claimed['token'], 'admission', 'intent', json.dumps({'compute':budget}), now))
                self.event('admitted', {'resource': name, 'stage': stage}, claimed['task'], claimed['id'])
            from .priority import info
            self.event('claimed', {'stage': stage, 'token': claimed['token'], 'owner': owner,
                                  'priority':info(self,dict(row),now)}, row['task'], row['id'])
            return claimed

    @staticmethod
    def admission_name(ident, token):
        return f'admission-{ident}-{token}'

    def compute_resources(self):
        """Cleanup debt occupies compute until fresh scoped runtime evidence says otherwise."""
        context,namespace=self.setting('runtime_kube_context'),self.setting('runtime_namespace')
        now=time.time()
        result=[]
        for row in self.db.execute("SELECT * FROM resources WHERE kind IN ('sandbox','admission') AND state<>'absent'"):
            proof=json.loads(row['data']).get('compute_observation') or {}
            released=(row['kind']=='sandbox' and row['state']=='deleting' and context and namespace
                and proof.get('context')==context and proof.get('namespace')==namespace
                and proof.get('work')==row['work'] and proof.get('token')==row['token']
                and proof.get('absent') is True and 0<=now-proof.get('at',0)<180)
            if not released:result.append(row)
        return result

    def compute_used(self):
        return len(self.compute_resources())

    def verification_reserve(self, capacity=None):
        capacity=self.setting('slots',8) if capacity is None else capacity
        return min(self.setting('verification_slots',1),max(0,capacity-1))

    def admission_available(self, stage, capacity=None, work=None):
        if stage not in ('triage','assess','implement','review','verify'):
            return True
        if self.setting('admission_paused',False):
            return False
        capacity=self.setting('slots',8) if capacity is None else capacity
        if self.compute_used() >= capacity:
            return False
        from .capacity import decision
        if not decision(self,work)['allowed']:
            return False
        if stage == 'verify':
            return True
        verification={r['id'] for r in self.db.execute("SELECT id FROM work WHERE stage='verify'")}
        used=sum(row['work'] not in verification for row in self.compute_resources())
        return used < capacity-self.verification_reserve(capacity)

    def ordered_ready(self):
        from .priority import sort_key
        now=time.time()
        rows=[dict(row) for row in self.db.execute("SELECT * FROM work WHERE state IN ('ready','retry') AND retry_at<=?",(now,))]
        return sorted(rows,key=lambda row:sort_key(self,row,now))

    def integration_waiting(self, data):
        """A long external wait yields the lane, never the merge/check gates."""
        ci = data.get('ci') or {}
        # Green CI on an outdated branch needs our own base refresh. Parking
        # it until GitHub becomes CLEAN deadlocks: only integration can do that.
        if (ci.get('checks') == 'green' and ci.get('status') == 'policy_pending'
                and ci.get('merge_state') == 'BEHIND'):
            return False
        return (ci.get('head') == data.get('published_head') and bool(ci.get('waiting_since'))
                and ci.get('status') in ('pending','policy_pending','infrastructure')
                and time.time()-ci['waiting_since'] >= self.setting('integration_wait_timeout',900))

    def integration_lane(self):
        """Only an actual, unresolved upstream merge owns the global permit."""
        row=self.db.execute("""SELECT r.data,r.updated,w.task,w.generation,w.input
            FROM resources r JOIN work w ON w.id=r.work
            WHERE r.kind='merge' AND r.state<>'absent' ORDER BY r.updated LIMIT 1""").fetchone()
        if not row:return None
        data=json.loads(row['data']);inputs=json.loads(row['input'])
        return {'task':row['task'],'generation':row['generation'],
                'candidate':inputs.get('candidate'),'entered':row['updated'],
                'phase':'merge-confirmation','head':data.get('head'),'base':data.get('base')}

    def integration_available(self, work):
        # Per-task safety holds still apply; unrelated verification and PR
        # preparation never wait for a global lane or another PR's CI.
        if work['stage'] not in ('verify','publish'):return True
        data=self.task(work['task'])['data'];ci=data.get('ci') or {}
        for row in self.db.execute("SELECT input FROM work WHERE task=? AND generation=? AND stage='verify' AND state='attention'",(work['task'],work['generation'])):
            if json.loads(row['input']).get('candidate')==data.get('candidate'):return False
        return not (ci.get('status')=='closed' and ci.get('head')==data.get('published_head'))

    def _current(self, ident, token, now=None):
        row = self.db.execute('''SELECT w.* FROM work w JOIN tasks t
            ON t.name=w.task AND t.generation=w.generation
            WHERE w.id=? AND w.token=? AND w.state='claimed' AND w.expires>?''',
                              (ident, token, time.time() if now is None else now)).fetchone()
        if not row:
            raise StaleClaim(ident)
        return row

    def heartbeat(self, ident, token, lease_seconds=90):
        with self.transaction():
            self._current(ident, token)
            self.db.execute('UPDATE work SET expires=?,updated=? WHERE id=?',
                            (time.time() + lease_seconds, time.time(), ident))

    def finish(self, ident, token, result, updates=None, findings=(), body=None, generation=None):
        with self.transaction():
            row = self._current(ident, token)
            if updates:
                task = self.db.execute('SELECT data FROM tasks WHERE name=?', (row['task'],)).fetchone()
                data = json.loads(task['data']) | updates
                self.db.execute('UPDATE tasks SET data=?,updated=? WHERE name=?',
                                (json.dumps(data), time.time(), row['task']))
            if body is not None:
                self.db.execute('UPDATE tasks SET body=?,generation=?,updated=? WHERE name=?',
                                (body, generation or row['generation'], time.time(), row['task']))
                if generation and generation != row['generation']:
                    self.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE task=? AND id<>? AND state NOT IN ('succeeded','obsolete')",
                                    (row['task'], ident))
            for finding in findings:
                self.db.execute('INSERT INTO findings(id,work,task,generation,source,category,detail,created) VALUES(?,?,?,?,?,?,?,?)',
                                (uuid.uuid4().hex, ident, row['task'], row['generation'],
                                 finding.get('source'), finding['category'], json.dumps(finding), time.time()))
            self.db.execute("UPDATE work SET state='succeeded',result=?,owner=NULL,expires=NULL,updated=? WHERE id=?",
                            (json.dumps(result), time.time(), ident))
            if row['stage'] == 'observe' and ((updates or {}).get('repair') or result.get('status') in ('merged','environment_refresh')):
                victims = self.db.execute("SELECT id,input FROM work WHERE task=? AND stage='verify' AND state='claimed'",(row['task'],)).fetchall()
                for victim in victims:
                    if result.get('status')=='environment_refresh' and json.loads(victim['input']).get('base')==result.get('target_base'):
                        continue
                    self.db.execute("UPDATE work SET state='obsolete',token=token+1,owner=NULL,expires=NULL,updated=? WHERE id=?",(time.time(),victim['id']))
                    self.event('verification_superseded_by_ci',{'observation':ident},row['task'],victim['id'])
            if row['stage'] == 'review' and (updates or {}).get('review', {}).get('approved'):
                self.db.execute('UPDATE findings SET resolved_by=? WHERE task=? AND generation=? AND resolved_by IS NULL',
                                ((updates or {})['review']['candidate'], row['task'], row['generation']))
            if (updates or {}).get('delivered'):
                saved = self.task(row['task'])
                self.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,?)',
                    (saved['name'],saved['generation'],saved['body'],json.dumps(saved['data']),time.time()))
            self.event('completed', result, row['task'], ident)

    def retry(self, ident, token, reason, delay=None, count_failure=True, attention=False):
        with self.transaction():
            row = self._current(ident, token)
            failures = row['failures'] + 1 if count_failure else 0
            delay = min(self.setting('retry_max_seconds', 900), self.setting('retry_base_seconds', 5) * 2 ** min(failures - 1, 8) * random.uniform(.8, 1.2)) if delay is None else delay
            retry_at=time.time()+delay
            self.db.execute("UPDATE work SET state=?,failures=?,retry_at=?,ready_since=?,result=?,owner=NULL,expires=NULL,updated=? WHERE id=?",
                            ('attention' if attention else 'retry', failures, retry_at,retry_at, json.dumps(reason), time.time(), ident))
            self.event('needs_attention' if attention else 'retry_scheduled', {'reason': reason, 'delay': delay}, row['task'], ident)
            if attention and reason.get('category') in ('sandbox_oom','sandbox_stopped','gateway_transport','verification_timeout','sandbox_storage','requirements_triage','repair_stalled','ci_infrastructure'):
                self.db.execute('INSERT OR REPLACE INTO settings VALUES(?,?)',
                    (self.runtime_hold_key(row['task'],row['stage'],row['generation']),json.dumps(reason)))

    def reserve(self, name, ident, token, kind, limit, data=None):
        with self.transaction():
            claim=dict(self._current(ident, token))
            admission = self.admission_name(ident, token)
            admitted = kind == 'sandbox' and bool(self.db.execute(
                "SELECT 1 FROM resources WHERE name=? AND kind='admission' AND state='intent'", (admission,)).fetchone())
            def used():
                return self.compute_used() - int(admitted) if kind == 'sandbox' else self.db.execute(
                    "SELECT count(*) FROM resources WHERE kind=? AND state<>'absent'", (kind,)).fetchone()[0]
            def transfer():
                if admitted:
                    self.db.execute("UPDATE resources SET state='absent',updated=? WHERE name=?", (time.time(), admission))
            old = self.db.execute('SELECT * FROM resources WHERE name=?', (name,)).fetchone()
            if old and (old['work'] != ident or old['token'] > token):
                raise StaleClaim('resource belongs to another claim')
            if kind=='sandbox':
                from .compute import assigned_profile
                from .capacity import decision
                budget=assigned_profile(self,claim)
                # Re-adoption retains an existing allocation's size. New
                # reservations replace their admission rather than counting it twice.
                if not old or old['state']=='absent':
                    if not decision(self,claim,budget,replace=(admission,name),name=name)['allowed']:
                        return False
                data={**(data or {}),'compute':budget}
            if old:
                if old['work'] != ident or old['token'] > token:
                    raise StaleClaim('resource belongs to another claim')
                if old['state'] == 'absent':
                    if not admitted and used() >= limit:
                        return False
                    self.db.execute("UPDATE resources SET token=?,state='intent',data=?,updated=? WHERE name=?",
                                    (token, json.dumps(data or {}), time.time(), name))
                    self.event('resource_reserved', {'name': name, 'kind': kind}, work=ident)
                    transfer()
                    return True
                if old['token'] < token:
                    self.db.execute('UPDATE resources SET token=?,updated=? WHERE name=?',
                                    (token, time.time(), name))
                    self.event('resource_adopted', {'name': name, 'token': token}, work=ident)
                if data:
                    self.db.execute('UPDATE resources SET data=? WHERE name=?',
                                    (json.dumps(json.loads(old['data']) | data), name))
                transfer()
                return old['state'] != 'absent'
            if not admitted and used() >= limit:
                return False
            self.db.execute('INSERT INTO resources VALUES(?,?,?,?,?,?,?)',
                            (name, ident, token, kind, 'intent', json.dumps(data or {}), time.time()))
            self.event('resource_reserved', {'name': name, 'kind': kind}, work=ident)
            transfer()
            return True

    def resource_state(self, name, state, data=None):
        if state not in ('intent', 'present', 'deleting', 'absent'):
            raise ValueError('invalid resource state')
        with self.transaction():
            if data is None:
                self.db.execute('UPDATE resources SET state=?,updated=? WHERE name=?', (state, time.time(), name))
            else:
                self.db.execute('UPDATE resources SET state=?,data=?,updated=? WHERE name=?',
                                (state, json.dumps(data), time.time(), name))

    def setting(self, key, default=None):
        row = self.db.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
        return json.loads(row['value']) if row else default

    def set_setting(self, key, value):
        with self.transaction():
            self.db.execute('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value',
                            (key, json.dumps(value)))
