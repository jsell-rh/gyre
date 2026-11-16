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
    from .runtime import ensure_workspace
    ensure_workspace(execution,name,invoke)
    root='/sandbox/workspace/.gyre-agent-'+key
    path=execution.directory/('agent-command-'+key+'.json')
    path.write_text(json.dumps({'argv':argv,'timeout':timeout,'cwd':'/sandbox/workspace'}));path.chmod(0o600)
    def call(*arguments):
        result=invoke(['sandbox','exec','-n',name,'--no-login-shell','--workdir','/tmp','--env','HOME=/tmp',
                       '--','python3','/tmp/stage/pipeline-agent.py',*arguments],capture_output=True,text=True,timeout=30)
        if result.returncode:
            message=(result.stderr or result.stdout or '')[-1200:]
            import os
            for field in ('OPENSHELL_OIDC_CLIENT_SECRET','GH_TOKEN','GITHUB_TOKEN','ENMAAS_API_KEY'):
                if os.environ.get(field):message=message.replace(os.environ[field],'[redacted]')
            raise RuntimeError('agent transport RPC failed: '+message)
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
                if frame.get('usage'):
                    (execution.directory/'remote-usage.json').write_text(json.dumps(frame['usage']))
                # Advance only after the reader received the exact bytes.
                sys.stdout.buffer.write(out);sys.stdout.buffer.flush()
                sys.stderr.buffer.write(err);sys.stderr.buffer.flush()
                offset,err_offset=frame['offset'],frame['stderr_offset']
                from .compute import from_usage
                oom=from_usage(execution,name,frame.get('usage') or {})
                if oom:
                    from .runtime import save_failure
                    save_failure(execution,oom)
                    try:
                        call('cancel',root)
                        if oom.category=='sandbox_storage':
                            from .runtime import reclaim_build_cache
                            reclaim_build_cache(execution,name,invoke)
                        from .fullsend import collect
                        collect(execution,name,success=False)
                    except Exception:pass  # Original resource evidence remains actionable.
                    return 74

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
            except (RuntimeError,OSError,ValueError,KeyError,subprocess.TimeoutExpired) as exc:
                failures+=1
                if unavailable is None:
                    unavailable=time.monotonic()
                    execution.phase('ReconnectingAgent',sandbox=name,engine='fullsend',transport='durable')
                    execution.store.event('agent_transport_reconnecting',{'run':key},execution.claim['task'],execution.claim['id'])
                state('reconnecting',error_kind=type(exc).__name__,error=str(exc)[-1200:])
                # A failed poll is not evidence that the process died. Inspect
                # the owned runtime before canceling anything or spending more inference.
                if failures>=2 and time.monotonic()-unavailable>=10:
                    from . import runtime
                    value=runtime.snapshot(execution,name,invoke)
                    phase=(value or {}).get('phase','').lower()
                    if phase=='stopped':
                        oom=runtime.failure(execution,name)
                        if runtime.restart(execution,name,invoke):
                            # Workspace survives; /tmp helpers do not necessarily survive.
                            runtime.restore_inputs(execution,name,invoke)
                            if oom:
                                try:
                                    from .fullsend import collect
                                    collect(execution,name,success=False)
                                except Exception:pass  # OOM evidence remains actionable even if salvage fails.
                                runtime.save_failure(execution,oom)
                                return 74
                            call('launch',root,'--resume-after-stop')
                            unavailable=None;failures=0
                            execution.phase('AgentRunning',sandbox=name,engine='fullsend',transport='resumed-after-stop')
                    elif phase in ('failed','error','deleted','deleting'):
                        from .execution import Attention
                        error=Attention('Sandbox reached terminal phase '+phase+'; inspect runtime evidence before retrying')
                        error.category='sandbox_stopped';runtime.save_failure(execution,error);return 74
                    # A Ready sandbox keeps its process and output offsets. No
                    # five-minute blind cancellation; the assignment deadline still applies.
                    if unavailable and time.monotonic()-unavailable>execution.store.setting('agent_reconnect_timeout',300) and phase!='ready':
                        from .execution import Attention
                        error=Attention('Cannot establish owned sandbox health after the reconnect budget; no blind re-execution')
                        error.category='gateway_transport';runtime.save_failure(execution,error);return 74
            time.sleep(min(15,2**min(failures,4)) if failures else 1)
        state('timeout',exit_code=124)
        return 124
    except (RuntimeError,ValueError) as error:
        from .runtime import save_failure
        save_failure(execution,error)
        raise
    finally:
        try:
            if not terminal:call('cancel',root)
        except Exception:pass  # The normal owned sandbox cleanup remains mandatory.
