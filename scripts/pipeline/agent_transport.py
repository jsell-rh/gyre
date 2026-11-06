"""Keep the agent process independent of an individual gateway RPC stream."""
import base64
import hashlib
import json
import subprocess
import sys
import time


def pi_command(args):
    if args[:2] != ['sandbox','exec'] or '--' not in args:return None
    index=args.index('--');command=args[index+1:]
    if (len(command)!=3 or command[:2]!=['sh','-c'] or 'FULLSEND_PI_BIN' not in command[2]
            or '--print' not in command[2] or '--mode json' not in command[2]):return None
    return command


def stream(execution, name, argv, timeout, invoke):
    # Identity is per claim and command. Lost launch acknowledgements and poll
    # retries reconnect to this run; they never launch a second coding process.
    key=hashlib.sha256(json.dumps(argv).encode()).hexdigest()[:24]
    root='/tmp/stage/agent-'+key
    path=execution.directory/('agent-command-'+key+'.json')
    path.write_text(json.dumps({'argv':argv,'timeout':timeout,'cwd':'/sandbox/workspace'}));path.chmod(0o600)
    def call(*arguments):
        result=invoke(['sandbox','exec','-n',name,'--no-login-shell','--workdir','/tmp','--env','HOME=/tmp',
                       '--','python3','/tmp/stage/pipeline-agent.py',*arguments],capture_output=True,text=True,timeout=30)
        if result.returncode:raise RuntimeError('agent transport RPC failed')
        return result.stdout
    result=invoke(['sandbox','exec','-n',name,'--no-login-shell','--','mkdir','-p',root],capture_output=True,text=True,timeout=30)
    if result.returncode:raise RuntimeError('agent transport directory unavailable')
    result=invoke(['sandbox','upload',name,str(path),root+'/command.json','--no-git-ignore'],capture_output=True,text=True,timeout=60)
    if result.returncode:raise RuntimeError('agent transport launch input unavailable')
    launched=False;terminal=False;connected=False;long_poll=False;offset=err_offset=0;unavailable=None;failures=0
    state_path=execution.directory/('agent-transport-'+key+'-state.json')
    def state(phase,**extra):
        temporary=state_path.with_suffix('.tmp')
        temporary.write_text(json.dumps({'phase':phase,'offset':offset,'stderr_offset':err_offset,**extra}))
        temporary.chmod(0o600);temporary.replace(state_path)
    deadline=time.monotonic()+timeout+60
    try:
        while time.monotonic()<deadline:
            execution.current()
            try:
                if not launched:call('launch',root);launched=True
                output=call('poll',root,'--offset',str(offset),'--stderr-offset',str(err_offset),*(['--wait','8'] if long_poll else []))
                frames=[line.removeprefix('GYRE_AGENT_POLL ') for line in output.splitlines() if line.startswith('GYRE_AGENT_POLL ')]
                if len(frames)!=1:raise ValueError('invalid agent transport frame')
                frame=json.loads(frames[0]);long_poll=frame.get('long_poll',False);out=base64.b64decode(frame['stdout'],validate=True);err=base64.b64decode(frame['stderr'],validate=True)
                if frame['offset']!=offset+len(out) or frame['stderr_offset']!=err_offset+len(err):raise ValueError('agent transport offset mismatch')
                # Advance only after the reader received the exact bytes.
                sys.stdout.buffer.write(out);sys.stdout.buffer.flush()
                sys.stderr.buffer.write(err);sys.stderr.buffer.flush()
                offset,err_offset=frame['offset'],frame['stderr_offset']
                if not connected or failures:
                    execution.phase('AgentRunning',sandbox=name,engine='fullsend',transport='durable')
                    connected=True
                failures=0;unavailable=None
                state('running')
                if frame['result'] is not None:
                    terminal=True
                    state('exited',exit_code=int(frame['result']['exit_code']))
                    return int(frame['result']['exit_code'])
                if not frame['alive']:
                    state('lost',exit_code=74)
                    return 74
            except BrokenPipeError:
                raise  # A vanished consumer is not a gateway reconnection.
            except (RuntimeError,OSError,ValueError,KeyError,subprocess.TimeoutExpired):
                failures+=1
                if unavailable is None:
                    unavailable=time.monotonic()
                    execution.phase('ReconnectingAgent',sandbox=name,engine='fullsend',transport='durable')
                    execution.store.event('agent_transport_reconnecting',{'run':key},execution.claim['task'],execution.claim['id'])
                state('reconnecting')
                if time.monotonic()-unavailable>execution.store.setting('agent_reconnect_timeout',300):
                    state('lost',exit_code=74)
                    return 74
            time.sleep(min(15,2**min(failures,4)) if failures else 1)
        state('timeout',exit_code=124)
        return 124
    finally:
        try:
            if not terminal:call('cancel',root)
        except Exception:pass  # The normal owned sandbox cleanup remains mandatory.
