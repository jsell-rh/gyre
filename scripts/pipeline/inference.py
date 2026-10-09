"""Cached inference admission probe shared by independent model workers."""
import fcntl
import hashlib
import json
import os
import subprocess
import time
import urllib.error
import urllib.request

from .catalog import helper


def credential():
    value = os.environ.get('ENMAAS_API_KEY')
    if value:
        return value
    result = subprocess.run(['secret-tool', 'lookup', 'service', 'pricetag', 'key', 'api-token'],
                            capture_output=True, timeout=15)
    if result.returncode or not result.stdout.strip():
        raise ValueError('local inference credential unavailable')
    return result.stdout.decode('utf-8').strip()


def probe(secret):
    body = json.dumps({'model': 'rits/zai-org/glm-5-3', 'max_tokens': 8,
                       'messages': [{'role': 'user', 'content': 'Reply OK.'}]}).encode()
    request = urllib.request.Request('https://api.enmaas.devshift.net/v1/chat/completions',
        data=body, headers={'Authorization': 'Bearer ' + secret, 'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=20) as response:
        value = json.loads(response.read(1024 * 1024))
    if not isinstance(value, dict) or not value.get('choices'):
        raise ValueError('inference response had no choices')


def configure(secret):
    helper('dev-inference').configure({**os.environ, 'ENMAAS_API_KEY': secret}, secret)


def check(store):
    from .execution import Retry
    with (store.directory / 'inference.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        now = time.time()
        cached = store.setting('inference_health', {})
        try:
            secret = credential()
            fingerprint = hashlib.sha256(secret.encode()).hexdigest()
        except Exception as exc:
            secret, fingerprint = None, None
            reason = 'InferenceUnavailable: local credential could not be read (' + type(exc).__name__ + ')'
        if cached.get('fingerprint') == fingerprint and cached.get('expires', 0) > now:
            if cached['condition'] != 'Ready':
                raise Retry(cached['condition'])
            store.set_setting('admission_condition', 'Ready')
            return
        if secret is not None:
            try:
                probe(secret)
                # A rotated local key must also replace the gateway's stored
                # provider credential before any fresh worker is admitted.
                if cached.get('configured_fingerprint') != fingerprint:
                    configure(secret)
                state = {'fingerprint': fingerprint, 'configured_fingerprint': fingerprint,
                         'condition': 'Ready', 'expires': now + 60, 'failures': 0}
                store.set_setting('inference_health', state)
                store.set_setting('admission_condition', 'Ready')
                return
            except urllib.error.HTTPError as exc:
                reason = f'InferenceUnavailable: pinned endpoint returned HTTP {exc.code}; refresh the local key for 401/403'
                exc.close()
            except Exception as exc:
                # Never log response bodies, credential values, or exception
                # strings from credential configuration.
                reason = 'InferenceUnavailable: pinned endpoint check failed (' + type(exc).__name__ + ')'
        failures = cached.get('failures', 0) + 1 if cached.get('fingerprint') == fingerprint else 1
        state = {'fingerprint': fingerprint, 'configured_fingerprint': cached.get('configured_fingerprint'),
                 'condition': reason, 'expires': now + min(900, 5 * 2 ** min(failures - 1, 8)),
                 'failures': failures}
        store.set_setting('inference_health', state)
        store.set_setting('admission_condition', reason)
        store.event('inference_unavailable', {'condition': reason, 'retry_at': state['expires']})
        raise Retry(reason)
