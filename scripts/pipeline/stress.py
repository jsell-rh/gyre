"""Opt-in experiment guard: pause new admissions, never kill active work."""
import json
import time


def observe(store,now=None):
    if not store.setting('stress_watch_enabled',False):return
    now=time.time() if now is None else now
    start=store.setting('stress_watch_started_at',now)
    if not store.setting('stress_watch_started_at'):
        store.set_setting('stress_watch_started_at',start)
    if store.setting('admission_paused',False):return
    cutoff=max(start,now-600)
    reason=None;tasks=[]
    overlaps=store.db.execute("""SELECT w.task FROM work w JOIN tasks t ON t.name=w.task AND t.generation=w.generation
        WHERE w.state='claimed' AND w.expires>? AND w.stage IN ('triage','assess','implement','review','verify')
        GROUP BY w.task HAVING count(*)>1""",(now,)).fetchall()
    if overlaps:reason='Overlapping live compute claims for the same task';tasks=[r['task'] for r in overlaps]
    groups={'provisioning':set(),'leases':set(),'memory_ceiling':set(),'ledger':set()}
    for row in store.db.execute("SELECT * FROM events WHERE at>=? AND kind IN ('retry_scheduled','lease_expired','compute_oom','bootstrap_connect_exhausted') ORDER BY id",(cutoff,)):
        d=json.loads(row['detail'])
        if row['kind']=='compute_oom':
            if d.get('observed_gib',0)>=6:groups['memory_ceiling'].add(row['task'])
            continue
        if row['work']:
            claim=store.db.execute("SELECT at FROM events WHERE work=? AND kind='claimed' AND at<=? ORDER BY id DESC LIMIT 1",(row['work'],row['at'])).fetchone()
            if not claim or claim['at']<start:continue
        if row['kind']=='bootstrap_connect_exhausted':groups['provisioning'].add(row['task']);continue
        if row['kind']=='lease_expired':groups['leases'].add(row['task']);continue
        failure=d.get('reason') or {}
        if failure.get('category')=='sandbox_provisioning':groups['provisioning'].add(row['task'])
        if 'database is locked' in failure.get('message','').lower():groups['ledger'].add(row['task'])
    for key,message in [('provisioning','Repeated sandbox provisioning faults after quota-aware admission'),
                        ('leases','Multiple worker leases expired'),
                        ('memory_ceiling','Multiple tasks exhausted the 6 GiB memory ceiling'),
                        ('ledger','Repeated ledger lock failures')]:
        if not reason and len(groups[key])>=3:reason=message;tasks=sorted(groups[key])
    if not reason:return
    alert={'at':now,'reason':reason,'tasks':tasks,'desired_slots':store.setting('slots')}
    with store.transaction():
        store.db.execute('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value',('admission_paused','true'))
        store.db.execute('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value',('stress_watch_alert',json.dumps(alert)))
        store.event('stress_guard_paused',alert)
    return alert
