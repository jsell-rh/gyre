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

STAGES = ('triage', 'implement', 'review', 'verify', 'publish', 'cleanup')


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
            CREATE TABLE IF NOT EXISTS events(
              id INTEGER PRIMARY KEY AUTOINCREMENT, task TEXT, work TEXT,
              kind TEXT NOT NULL, detail TEXT NOT NULL, at REAL NOT NULL);
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS work_events ON events(work,kind);
        ''')
        (self.directory / 'pipeline.sqlite3').chmod(0o600)
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

    def put_task(self, name, generation, body, data):
        with self.transaction():
            old = self.db.execute('SELECT generation FROM tasks WHERE name=?', (name,)).fetchone()
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
                (id,stage,task,generation,input_key,input,priority,created,updated)
                VALUES(?,?,?,?,?,?,?,?,?)''',
                            (uuid.uuid4().hex, stage, task, generation, key, encoded,
                             priority, time.time(), time.time()))
            self.db.execute("UPDATE work SET state='ready',updated=? WHERE stage=? AND task=? AND generation=? AND input_key=? AND state='obsolete' AND EXISTS(SELECT 1 FROM tasks WHERE name=? AND generation=?)",
                            (time.time(), stage, task, generation, key, task, generation))
            self.db.execute("UPDATE work SET input=?,updated=? WHERE stage=? AND task=? AND generation=? AND input_key=? AND state IN ('ready','retry')",
                            (encoded, time.time(), stage, task, generation, key))
            return self.db.execute('SELECT id FROM work WHERE stage=? AND task=? AND generation=? AND input_key=?',
                                   (stage, task, generation, key)).fetchone()['id']

    def claim(self, stage, owner, concurrency=1, lease_seconds=90, now=None, task=None, eligible=None):
        now = time.time() if now is None else now
        if concurrency < 1 or lease_seconds <= 0:
            raise ValueError('invalid claim limits')
        with self.transaction():
            expired = self.db.execute("SELECT id,task FROM work WHERE state='claimed' AND expires<=?", (now,)).fetchall()
            for row in expired:
                self.db.execute("UPDATE work SET state='ready',token=token+1,owner=NULL,expires=NULL,updated=? WHERE id=?", (now, row['id']))
                self.event('lease_expired', {}, row['task'], row['id'])
            active = self.db.execute("SELECT count(*) FROM work WHERE stage=? AND state='claimed'", (stage,)).fetchone()[0]
            if active >= concurrency:
                return None
            rows = self.db.execute('''SELECT w.* FROM work w JOIN tasks t
                ON t.name=w.task AND t.generation=w.generation
                WHERE w.stage=? AND w.state IN ('ready','retry') AND w.retry_at<=?
                AND (? IS NULL OR w.task=?)
                AND NOT EXISTS(SELECT 1 FROM work x WHERE x.task=w.task AND x.state='claimed')
                ORDER BY w.priority DESC,w.created,w.id''', (stage, now, task, task)).fetchall()
            row = next((row for row in rows if eligible is None or eligible(dict(row))), None)
            if not row:
                return None
            self.db.execute("UPDATE work SET state='claimed',token=token+1,owner=?,expires=?,updated=? WHERE id=?",
                            (owner, now + lease_seconds, now, row['id']))
            claimed = dict(self.db.execute('SELECT * FROM work WHERE id=?', (row['id'],)).fetchone())
            claimed['input'] = json.loads(claimed['input'])
            self.event('claimed', {'stage': stage, 'token': claimed['token'], 'owner': owner}, row['task'], row['id'])
            return claimed

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
            if row['stage'] == 'review' and (updates or {}).get('review', {}).get('approved'):
                self.db.execute('UPDATE findings SET resolved_by=? WHERE task=? AND generation=? AND resolved_by IS NULL',
                                ((updates or {})['review']['candidate'], row['task'], row['generation']))
            self.event('completed', result, row['task'], ident)

    def retry(self, ident, token, reason, delay=None):
        with self.transaction():
            row = self._current(ident, token)
            failures = row['failures'] + 1
            delay = min(900, 5 * 2 ** min(failures - 1, 8) * random.uniform(.8, 1.2)) if delay is None else delay
            self.db.execute("UPDATE work SET state='retry',failures=?,retry_at=?,result=?,owner=NULL,expires=NULL,updated=? WHERE id=?",
                            (failures, time.time() + delay, json.dumps(reason), time.time(), ident))
            self.event('retry_scheduled', {'reason': reason, 'delay': delay}, row['task'], ident)

    def reserve(self, name, ident, token, kind, limit, data=None):
        with self.transaction():
            self._current(ident, token)
            old = self.db.execute('SELECT * FROM resources WHERE name=?', (name,)).fetchone()
            if old:
                if old['work'] != ident or old['token'] > token:
                    raise StaleClaim('resource belongs to another claim')
                if old['state'] == 'absent':
                    count = self.db.execute("SELECT count(*) FROM resources WHERE kind=? AND state<>'absent'", (kind,)).fetchone()[0]
                    if count >= limit:
                        return False
                    self.db.execute("UPDATE resources SET token=?,state='intent',data=?,updated=? WHERE name=?",
                                    (token, json.dumps(data or {}), time.time(), name))
                    self.event('resource_reserved', {'name': name, 'kind': kind}, work=ident)
                    return True
                if old['token'] < token:
                    self.db.execute('UPDATE resources SET token=?,updated=? WHERE name=?',
                                    (token, time.time(), name))
                    self.event('resource_adopted', {'name': name, 'token': token}, work=ident)
                if data:
                    self.db.execute('UPDATE resources SET data=? WHERE name=?',
                                    (json.dumps(json.loads(old['data']) | data), name))
                return old['state'] != 'absent'
            count = self.db.execute("SELECT count(*) FROM resources WHERE kind=? AND state<>'absent'", (kind,)).fetchone()[0]
            if count >= limit:
                return False
            self.db.execute('INSERT INTO resources VALUES(?,?,?,?,?,?,?)',
                            (name, ident, token, kind, 'intent', json.dumps(data or {}), time.time()))
            self.event('resource_reserved', {'name': name, 'kind': kind}, work=ident)
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
