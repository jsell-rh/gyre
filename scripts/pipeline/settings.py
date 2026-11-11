"""Validated operator settings for the shared Gyre ledger; never credentials."""
import json
from datetime import datetime, timezone

from .diagnostics import DEFAULT_MODEL_TIMEOUT


def configuration(store):
    rows = []

    def add(key, label, value, effect, kind='integer', help='', choices=None):
        rows.append(dict(key=key, label=label, value=str(value).lower() if isinstance(value, bool) else str(value),
                         effect=effect, kind=kind, help=help, choices=choices or []))

    live = 'Next admission'
    add('slots', 'Sandbox capacity', store.setting('slots', 8), live,
        help='0–1000. Zero drains new compute. Running workers and pending deletions keep their reservations.')
    add('admission_paused', 'Pause new model work', store.setting('admission_paused', False), live, 'choice',
        'Pauses triage, implementation, review and verification; cleanup and PR observation continue.', ['false', 'true'])
    add('verification_slots', 'Reserved verification slots', store.setting('verification_slots', 1), live,
        help='0–1000, within total capacity. Effective reserve clamps to capacity minus one; existing workers finish.')
    limits = store.setting('limits', {})
    for stage, label in [('triage', 'Triage'), ('assess', 'Spec assessment'), ('implement', 'Implementation'), ('review', 'Review'), ('verify', 'Verification')]:
        add('limit.' + stage, label + ' concurrency', limits.get(stage, ''), live,
            help='1–1000, or empty to inherit total capacity. Global sandbox admission still applies. Running work is not canceled.')
    add('model_timeout', 'Agent deadline (seconds)', store.setting('model_timeout', DEFAULT_MODEL_TIMEOUT), 'New assignments',
        help='60–86400 seconds. Default four hours; running assignments keep their existing deadline.')
    add('verification_timeout', 'Verification deadline (seconds)', store.setting('verification_timeout', 14400), 'New assignments',
        help='60–86400 seconds. Bounds remote verification; running verifiers retain their deadline.')
    add('retry_base_seconds', 'Retry base (seconds)', store.setting('retry_base_seconds', 5), 'New retry schedules',
        help='1–3600 seconds. Exponential backoff with jitter; already scheduled retries keep their wake time.')
    add('retry_max_seconds', 'Retry ceiling (seconds)', store.setting('retry_max_seconds', 900), 'New retry schedules',
        help='1–86400 seconds, at least the base. Does not alter explicit stage-specific delays.')
    checked = store.setting('dispatcher_health_at', 0)
    auth_status = datetime.fromtimestamp(checked, timezone.utc).isoformat(timespec='seconds') if checked else 'Not checked yet'
    for key, label, value, help in [
        ('model', 'Pinned model', 'gateway/rits/zai-org/glm-5-3', 'Read-only: model selection must agree with the gateway route and inference admission probe.'),
        ('gateway', 'Gateway registration', store.setting('gateway', 'gyre-gyre'), 'Read-only: gateway identity and credentials are managed by gateway setup and automatic login refresh.'),
        ('gateway_auth', 'Last successful gateway check', auth_status, 'Read-only: last successful provider/authentication health check, not a guarantee of current connectivity. Automatic refresh remains enabled.'),
        ('repository', 'Repository', store.setting('repository', 'jsell-rh/gyre'), 'Read-only: changing repository would invalidate the task ledger and source identities.'),
        ('ledger', 'Settings ledger', str(store.directory / 'pipeline.sqlite3'), 'Shared with the browser and CLI; separate from every other dispatcher instance.'),
    ]:
        add(key, label, value, 'Read only', 'readonly', help)
    return rows


def save_configuration(store, request):
    if request.get('scope') != 'gyre/' + store.setting('owner'):
        raise ValueError('configuration belongs to another dispatcher scope')
    key, text = request.get('key'), request.get('value')
    if not isinstance(text, str):
        raise ValueError('setting value must be text')
    with store.transaction():
        row = next((r for r in configuration(store) if r['key'] == key), None)
        if row is None or row['kind'] == 'readonly':
            raise ValueError('unknown or read-only setting')
        if 'expected' in request and request['expected'] != row['value']:
            raise ValueError('Setting changed in another controller; refresh and edit again.')
        text = text.strip()
        if key == 'admission_paused':
            if text not in ('true', 'false'):
                raise ValueError('pause must be true or false')
            value = text == 'true'
        elif key.startswith('limit.') and not text:
            value = None
        else:
            if not text.isascii() or not text.isdecimal():
                raise ValueError('enter a whole number')
            value = int(text)
            low, high = (0, 1000)
            if key.startswith('limit.'):
                low = 1
            elif key in ('model_timeout', 'verification_timeout'):
                low, high = 60, 86400
            elif key == 'retry_base_seconds':
                low, high = 1, 3600
            elif key == 'retry_max_seconds':
                low, high = 1, 86400
            if not low <= value <= high:
                raise ValueError(f'value must be {low}–{high}')
        if key in ('retry_base_seconds', 'retry_max_seconds'):
            base = value if key == 'retry_base_seconds' else store.setting('retry_base_seconds', 5)
            ceiling = value if key == 'retry_max_seconds' else store.setting('retry_max_seconds', 900)
            if base > ceiling:
                raise ValueError('retry ceiling must be at least the retry base')
        storage_key = key
        if key.startswith('limit.'):
            storage_key = 'limits'
            limits = dict(store.setting('limits', {}))
            if value is None:
                limits.pop(key.split('.')[1], None)
            else:
                limits[key.split('.')[1]] = value
            stored = limits
        else:
            stored = value
        store.db.execute('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value',
                         (storage_key, json.dumps(stored)))
        store.event('configuration_changed', {'key': key, 'value': value, 'effect': row['effect']})
        return {'settings': configuration(store), 'value': value, 'message': 'Saved in Gyre ledger. ' + row['effect'] + '.'}
