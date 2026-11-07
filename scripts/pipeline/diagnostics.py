"""Bounded, structured runner diagnostics; never copy model reasoning or credentials."""
import json
import re

DEFAULT_MODEL_TIMEOUT = 4 * 60 * 60


def diagnose(directory, runner_exit=None):
    errors, pending = [], {}
    terminal = None
    runner_message = ''
    agent_exit = None
    incomplete = False
    for path in sorted((directory / 'runs').glob('*/run-telemetry.jsonl')):
        with path.open(errors='replace') as source:
            for line in source:
                try:
                    frame = json.loads(line)
                except ValueError:
                    continue
                for resource in frame.get('resourceSpans', []):
                    for scope in resource.get('scopeSpans', []):
                        for span in scope.get('spans', []):
                            if span.get('name') == 'run':
                                runner_message = span.get('status', {}).get('message', '')
                            if span.get('name') == 'agent':
                                if 'subtype: incomplete' in span.get('status', {}).get('message', ''):
                                    incomplete = True
                                for attribute in span.get('attributes', []):
                                    if attribute.get('key') == 'exit_code':
                                        try:
                                            agent_exit = int(attribute.get('value', {}).get('intValue'))
                                        except (TypeError, ValueError):
                                            pass
    for path in sorted((directory / 'runs').glob('*/iteration-*/output.jsonl')):
        pending = {}
        with path.open(errors='replace') as source:
            for line in source:
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                kind = event.get('type')
                if kind == 'tool_execution_start':
                    # Names only: tool arguments can contain credentials.
                    pending[event.get('toolCallId')] = event.get('toolName', 'unknown')
                elif kind == 'tool_execution_end':
                    pending.pop(event.get('toolCallId'), None)
                elif kind == 'message_end' and isinstance(event.get('message'), dict):
                    message = event['message']
                    if message.get('role') == 'assistant':
                        terminal = message.get('stopReason')
                        if terminal == 'error':
                            errors.append(str(message.get('errorMessage', '')))
    timeout = None
    pattern = r'agent timed out after ([\dhms.]+) without completing \(timeout: ([\dhms.]+)\)'
    match = re.search(pattern, runner_message)
    if match:
        timeout = match.groups()
    # Match only the runner's fixed terminal diagnostics, not generic mentions
    # of timeouts/429s inside task text, tool output, or model discussion.
    path = directory / 'output.log'
    if path.exists():
        with path.open(errors='replace') as source:
            for line in source:
                match = re.search(pattern, line)
                if match:
                    timeout = match.groups()
    details = {'runner_exit': runner_exit, 'agent_exit': agent_exit, 'last_stop_reason': terminal,
               'unfinished_tools': sorted(set(pending.values())), 'api_error_count': len(errors),
               'transcript_incomplete': incomplete}
    outcome = directory / 'outcome.json'
    try:
        receipt = json.loads(outcome.read_text()) if outcome.exists() else {}
    except ValueError:
        receipt = {}
    transport_lost=False
    for path in directory.glob('agent-transport-*-state.json'):
        try:
            state=json.loads(path.read_text())
            transport_lost=transport_lost or state.get('phase')=='lost'
        except (OSError,ValueError):pass
    if receipt.get('source_valid') is False:
        category, summary = 'review_source_guard', 'Review changed protected source; discard its checkpoint and start a clean review'
    elif timeout:
        category, summary = 'model_timeout', f'Agent exceeded {timeout[1]} deadline (elapsed {timeout[0]})'
        details.update(elapsed=timeout[0], budget=timeout[1])
    elif runner_message.startswith('creating sandbox:'):
        category, summary = 'sandbox_provisioning', 'Sandbox provisioning failed before the model started; retry infrastructure with backoff'
    elif 'transport error' in runner_message or 'deadline has elapsed' in runner_message:
        category, summary = 'gateway_transport', 'Gateway transport failed while preparing or collecting the assignment'
    elif transport_lost:
        category, summary = 'gateway_transport', 'Gateway reconnection budget exhausted; retained agent output is incomplete'
    elif terminal == 'error' and errors and re.search(r'\b429\b|rate.?limit', errors[-1], re.I):
        category, summary = 'inference_rate_limit', 'Inference returned HTTP 429; retry with backoff'
    elif terminal == 'error' and errors and re.search(r'\b(?:401|403)\b', errors[-1]):
        category, summary = 'inference_auth', 'Inference rejected authentication (HTTP 401/403)'
    elif terminal == 'error' and errors:
        category, summary = 'inference_error', 'Inference request failed; structured agent error retained locally'
    elif incomplete:
        category, summary = 'agent_interrupted', 'Agent stream ended before completing; resume saved evidence (no verdict accepted)'
    elif runner_message.startswith('validation failed'):
        category, summary = 'assignment_validation', 'Assignment validator failed; inspect source guard and verdict evidence'
    else:
        category, summary = 'runner_failure', 'Runner exited without a completed assignment'
    details.update(category=category, message=summary)
    artifact = directory / 'diagnostic.json'
    artifact.write_text(json.dumps(details))
    artifact.chmod(0o600)
    return details | {'artifact': str(artifact)}
