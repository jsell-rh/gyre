"""A broken client connection must not restart or truncate a coding process."""
import io
import importlib.util
import base64
import json
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from types import SimpleNamespace
from unittest import TestCase
from unittest.mock import patch

from pipeline.agent_transport import pi_command, stream
from pipeline.execution import Execution
from pipeline.store import Store

SCRIPT=Path(__file__).with_name('pipeline-agent.py').resolve()


class AgentTransportTest(TestCase):
    def test_cancel_quiesces_child_after_supervisor_crash(self):
        import os,signal
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);marker=root/'running'
            code=f'from pathlib import Path; import subprocess,sys,time; p=subprocess.Popen([sys.executable,"-c","import time;time.sleep(60)"],start_new_session=True); Path({str(marker)!r}).write_text(str(p.pid)); time.sleep(60)'
            (root/'command.json').write_text(json.dumps({'argv':[sys.executable,'-c',code],'cwd':directory,'timeout':60}))
            neighbor=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)'])
            self.addCleanup(lambda:(neighbor.kill(),neighbor.wait()))
            subprocess.run([sys.executable,str(SCRIPT),'launch',directory],check=True,capture_output=True)
            self.addCleanup(lambda:subprocess.run([sys.executable,str(SCRIPT),'cancel',directory],capture_output=True))
            deadline=time.monotonic()+5
            while not marker.exists() and time.monotonic()<deadline:time.sleep(.02)
            self.assertTrue(marker.exists())
            parent=json.loads((root/'identity.json').read_text())
            child=json.loads((root/'child.json').read_text())
            pid=int(marker.read_text());fields=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()
            detached={'pid':pid,'start':fields[19],'boot':child['boot']}
            os.kill(parent['pid'],signal.SIGKILL)
            os.kill(child['pid'],signal.SIGKILL)
            subprocess.run([sys.executable,str(SCRIPT),'cancel',directory],check=True,capture_output=True,timeout=10)
            spec=importlib.util.spec_from_file_location('agent_identity',SCRIPT)
            agent=importlib.util.module_from_spec(spec);spec.loader.exec_module(agent)
            self.assertFalse(agent.alive(child))
            self.assertFalse(agent.alive(detached))
            self.assertIsNone(neighbor.poll())

    def test_launch_ack_loss_and_poll_failure_resume_exact_bytes_once(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);store=Store(root/'state');self.addCleanup(store.close)
            store.put_task('task-001','g','body',{})
            work=store.enqueue('implement','task-001','g',{})
            execution=Execution(store,store.claim('implement','test'))
            marker=root/'executions'
            code=f'from pathlib import Path; import sys,time; p=Path({str(marker)!r}); p.write_text(p.read_text()+"run\\n" if p.exists() else "run\\n"); print("first",flush=True); time.sleep(.3); print("last",flush=True); print("diagnostic",file=sys.stderr); sys.exit(7)'
            launch_lost=False;poll_lost=False
            def invoke(args,**kwargs):
                nonlocal launch_lost,poll_lost
                if args[:2]==['sandbox','upload']:
                    job=json.loads(Path(args[3]).read_text());job['cwd']=str(root)
                    (root/'remote').mkdir(exist_ok=True)
                    (root/'remote/command.json').write_text(json.dumps(job))
                    return subprocess.CompletedProcess(args,0,'','')
                command=args[args.index('--')+1:]
                if command[0]=='mkdir':return subprocess.CompletedProcess(args,0,'','')
                command=[sys.executable,str(SCRIPT),*command[2:]]
                command=[str(root/'remote') if x.startswith(('/tmp/stage/agent-','/sandbox/workspace/.gyre-agent-')) else x for x in command]
                result=subprocess.run(command,capture_output=True,text=True,timeout=5)
                if 'launch' in command and not launch_lost:
                    launch_lost=True
                    raise subprocess.TimeoutExpired('lost launch acknowledgement',1)
                if 'poll' in command and not poll_lost and json.loads(result.stdout.removeprefix('GYRE_AGENT_POLL ')).get('offset',0)>0:
                    poll_lost=True
                    raise subprocess.TimeoutExpired('lost output frame',1)
                return result
            out,err=io.BytesIO(),io.BytesIO()
            with patch('pipeline.agent_transport.sys.stdout',SimpleNamespace(buffer=out)),patch('pipeline.agent_transport.sys.stderr',SimpleNamespace(buffer=err)):
                exit_code=stream(execution,'sandbox',[sys.executable,'-c',code],5,invoke)
            self.assertEqual(exit_code,7)
            self.assertEqual(marker.read_text(),'run\n')
            self.assertEqual(out.getvalue(),b'first\nlast\n')
            self.assertEqual(err.getvalue(),b'diagnostic\n')
            self.assertTrue(launch_lost and poll_lost)
            self.assertEqual(json.loads((root/'remote/result.json').read_text())['exit_code'],7)

    def test_ready_sandbox_survives_more_than_five_minutes_of_poll_loss(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close)
            store.put_task('task-001','g','body',{})
            store.enqueue('implement','task-001','g',{})
            execution=Execution(store,store.claim('implement','test'))
            now=[100.0];polls=[0];launches=[0]
            def invoke(args,**kwargs):
                if args[:2]==['sandbox','upload'] or 'mkdir' in args:return SimpleNamespace(returncode=0,stdout='',stderr='')
                if 'launch' in args:
                    launches[0]+=1;return SimpleNamespace(returncode=0,stdout='',stderr='')
                if 'poll' in args:
                    polls[0]+=1
                    if polls[0]<=7:
                        now[0]+=70;raise subprocess.TimeoutExpired('lost poll',30)
                    frame={'stdout':'','stderr':'','offset':0,'stderr_offset':0,'result':{'exit_code':0},'alive':False}
                    return SimpleNamespace(returncode=0,stdout='GYRE_AGENT_POLL '+json.dumps(frame),stderr='')
                raise AssertionError(args)
            with patch('pipeline.agent_transport.time.monotonic',side_effect=lambda:now[0]),patch('pipeline.agent_transport.time.sleep'),patch('pipeline.runtime.snapshot',return_value={'phase':'Ready'}):
                result=stream(execution,'owned',['dummy'],1800,invoke)
            self.assertEqual(result,0)
            self.assertEqual(launches[0],1)
            self.assertGreater(now[0]-100,300)

    def test_exit_receipt_cannot_overtake_a_terminal_event_written_between_poll_reads(self):
        spec=importlib.util.spec_from_file_location('remote_agent',SCRIPT);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'stdout').write_bytes(b'first\n');(root/'stderr').write_bytes(b'')
            read=module.read_chunk
            def interleave(root,name,offset):
                value=read(root,name,offset)
                if name=='stderr' and not (root/'result.json').exists():
                    with (root/'stdout').open('ab') as out:out.write(b'agent_end\n')
                    (root/'result.json').write_text(json.dumps({'exit_code':0}))
                return value
            with patch.object(module,'read_chunk',side_effect=interleave):first=module.poll(root,0,0)
            self.assertIsNone(first['result'])
            second=module.poll(root,first['offset'],first['stderr_offset'])
            self.assertEqual(base64.b64decode(first['stdout'])+base64.b64decode(second['stdout']),b'first\nagent_end\n')
            self.assertEqual(second['result'],{'exit_code':0})

    def test_finished_producer_drains_every_chunk_before_being_reported_dead(self):
        spec=importlib.util.spec_from_file_location('remote_agent',SCRIPT);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);data=b'x'*200000
            (root/'stdout').write_bytes(data);(root/'stderr').write_bytes(b'')
            (root/'result.json').write_text(json.dumps({'exit_code':0}))
            (root/'identity.json').write_text(json.dumps({'pid':99999999,'start':'0','boot':'gone'}))
            offset=0;received=b''
            for _ in range(5):
                frame=module.poll(root,offset,0)
                received+=base64.b64decode(frame['stdout']);offset=frame['offset']
                if frame['result'] is not None:break
                self.assertTrue(frame['alive'],'finished spool was mistaken for a lost process')
            self.assertEqual(received,data)
            self.assertEqual(frame['result'],{'exit_code':0})

    def test_confirmed_stop_can_resume_a_gracefully_interrupted_supervisor(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);marker=root/'resumed'
            (root/'command.json').write_text(json.dumps({'argv':[sys.executable,'-c',f"from pathlib import Path; Path({str(marker)!r}).write_text('once')"],'cwd':directory,'timeout':5}))
            (root/'identity.json').write_text(json.dumps({'pid':99999999,'start':'0','boot':'gone'}))
            (root/'result.json').write_text(json.dumps({'exit_code':130}))
            subprocess.run([sys.executable,str(SCRIPT),'launch',directory,'--resume-after-stop'],check=True,timeout=5)
            deadline=time.monotonic()+5
            while not (root/'result.json').exists() and time.monotonic()<deadline:time.sleep(.02)
            self.assertEqual(marker.read_text(),'once')
            self.assertEqual(json.loads((root/'result.json').read_text())['exit_code'],0)
            subprocess.run([sys.executable,str(SCRIPT),'launch',directory,'--resume-after-stop'],check=True,timeout=5)
            self.assertEqual(json.loads((root/'result.json').read_text())['exit_code'],0)
            self.assertTrue((root/'result.before-restart.json').exists())

    def test_remote_deadline_kills_the_real_process(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);pid=root/'pid'
            code=f'import os,time; from pathlib import Path; Path({str(pid)!r}).write_text(str(os.getpid())); time.sleep(60)'
            (root/'command.json').write_text(json.dumps({'argv':[sys.executable,'-c',code],'cwd':directory,'timeout':.1}))
            subprocess.run([sys.executable,str(SCRIPT),'launch',directory],check=True,timeout=5)
            deadline=time.monotonic()+5
            while not (root/'result.json').exists() and time.monotonic()<deadline:time.sleep(.02)
            self.assertEqual(json.loads((root/'result.json').read_text())['exit_code'],124)
            self.assertFalse(Path('/proc/'+pid.read_text()).exists())

    def test_only_the_pinned_pi_invocation_is_intercepted(self):
        prefix=['sandbox','exec','--name','assigned','--no-tty','--timeout','20','--']
        command=['sh','-c','FULLSEND_PI_BIN=x; pi --print --mode json']
        self.assertEqual(pi_command(prefix+command),command)
        self.assertIsNone(pi_command(prefix+['sleep','60']))
        self.assertIsNone(pi_command(prefix+['sh','-c','echo pi --print --mode json']))
