#!/usr/bin/env python3
"""Read-only workflow efficiency report; compute time includes provisioning."""
import argparse
import collections
import json
from pathlib import Path
import sqlite3
import time


def report(db, hours=24, now=None):
    now=time.time() if now is None else now;start=now-hours*3600
    rows=db.execute('SELECT e.id,e.work,e.kind,e.at,e.detail,w.stage FROM events e LEFT JOIN work w ON w.id=e.work WHERE e.at<=? ORDER BY e.id',(now,)).fetchall()
    timing={row[0]:(row[1],row[2]) for row in db.execute('SELECT id,state,updated FROM work')}
    def end(work,at):
        state,updated=timing[work]
        return min(at,updated) if state!='claimed' else at
    active={};seconds=collections.Counter();attempts=collections.Counter();merges=set();events=collections.Counter();cache=collections.Counter()
    compute={'implement','review','verify','assess','triage'}
    for _,work,kind,at,detail,stage in rows:
        if at>=start:events[kind]+=1
        if kind=='claimed' and stage in compute:
            if work in active:
                old,oldstage=active.pop(work);seconds[oldstage]+=max(0,end(work,at)-max(start,old))
            active[work]=(at,stage)
            if at>=start:attempts[stage]+=1
        if kind in ('completed','retry_scheduled','needs_attention','lease_expired') and work in active:
            began,which=active.pop(work);seconds[which]+=max(0,end(work,at)-max(start,began))
        if at<start:continue
        data=json.loads(detail)
        if kind=='completed' and (data.get('status')=='merged' or data.get('merged')):
            merges.add(data.get('sha') or data.get('merged'))
        if kind=='build_cache_offered':cache['offers' if data.get('hit') else 'misses']+=1
        if kind=='completed' and isinstance(data.get('cache'),dict):cache['restores' if data['cache'].get('hit') else 'cold_builds']+=1
    for work,(began,stage) in active.items():seconds[stage]+=max(0,end(work,now)-max(start,began))
    minutes={stage:round(value/60,1) for stage,value in seconds.items()}
    return {'hours':hours,'merged_prs':len(merges),'compute_worker_minutes':minutes,
        'worker_minutes_per_merge':round(sum(seconds.values())/60/len(merges),1) if merges else None,
        'compute_attempts':dict(attempts),'observation_claims':sum(1 for _,_,k,at,_,stage in rows if k=='claimed' and at>=start and stage=='observe'),
        'cache':dict(cache),'note':'Worker minutes include setup and transport waits, not measured CPU time.'}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--state',type=Path,default=Path(__file__).resolve().parents[1]/'.gyre-pipeline');parser.add_argument('--hours',type=float,default=24)
    args=parser.parse_args()
    if not 0<args.hours<=24*365:parser.error('--hours must be within (0, 8760]')
    db=sqlite3.connect((args.state/'pipeline.sqlite3').resolve().as_uri()+'?mode=ro',uri=True)
    print(json.dumps(report(db,args.hours),indent=2));db.close()

if __name__=='__main__':main()
