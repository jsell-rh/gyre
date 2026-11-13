"""Qualify capacity retries using scoped scheduler evidence, never exit text alone."""
import datetime
import json
import subprocess


def classify(execution,reason):
    if reason.get('category')!='sandbox_provisioning':return reason
    store=execution.store
    context=store.setting('runtime_kube_context');namespace=store.setting('runtime_namespace')
    if not context or not namespace:return reason
    claim=execution.claim
    started=store.db.execute("SELECT at FROM events WHERE work=? AND kind='claimed' AND json_extract(detail,'$.token')=? ORDER BY id DESC LIMIT 1",
                             (claim['id'],claim['token'])).fetchone()
    if not started:return reason
    for resource in store.db.execute("SELECT name FROM resources WHERE work=? AND token=? AND kind='sandbox'",(claim['id'],claim['token'])):
        try:
            result=subprocess.run(['kubectl','--context',context,'--request-timeout=5s','-n',namespace,'get','events',
                '--field-selector','involvedObject.name='+resource['name'],'-o','json'],capture_output=True,text=True,timeout=8,check=True)
            scheduling=[]
            for event in json.loads(result.stdout)['items']:
                obj=event.get('involvedObject') or {};message=event.get('message','')
                at=event.get('lastTimestamp') or event.get('eventTime') or event.get('metadata',{}).get('creationTimestamp')
                at=datetime.datetime.fromisoformat(at.replace('Z','+00:00')).timestamp()
                if (obj.get('kind')!='Pod' or obj.get('name')!=resource['name']
                        or obj.get('namespace')!=namespace or at<started['at']
                        or event.get('reason') not in ('FailedScheduling','Scheduled')):continue
                scheduling.append((at,event))
            if scheduling:
                at,event=max(scheduling,key=lambda item:item[0]);message=event.get('message','')
                if event['reason']!='FailedScheduling' or not any(x in message for x in ('Insufficient memory','Insufficient cpu')):continue
                evidence={'sandbox':resource['name'],'namespace':namespace,'context':context,'at':at,'message':message}
                return reason | {'category':'backend_capacity','diagnostic':reason.get('diagnostic'),
                                 'capacity':evidence,'message':'Backend CPU/memory unavailable; retain task source and retry with stable backoff'}
        except (OSError,ValueError,KeyError,TypeError,AttributeError,subprocess.SubprocessError):
            continue  # Without scoped evidence this remains a provisioning fault.
    return reason
