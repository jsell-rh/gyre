"""Reconcile historical PR associations without weakening exact-source delivery."""
from .execution import Attention


class PublicationMismatch(Attention):
    category = 'publication_mismatch'


def superseded(task, observation, verified):
    """A confirmed previous-generation delivery cannot deliver a new candidate.

    Preserve its receipt for attribution, clear only the active association, and
    let normal publication open a PR for the newly reviewed/verified source.
    Unknown head changes remain safety holds, never successful deliveries.
    """
    previous = task['data'].get('previous_delivery') or {}
    sha = (observation.get('mergeCommit') or {}).get('oid')
    if (observation.get('state') == 'MERGED' and sha
            and previous.get('sha') == sha and previous.get('pr') == task['data'].get('pr')
            and previous.get('candidate') == observation.get('headRefOid')
            and previous.get('generation') and previous['generation'] != task['generation']
            and observation['headRefOid'] != verified['head']):
        return {'previous_delivery': previous, 'publication_epoch': sha, 'pr': None, 'delivery_branch': f"pipeline/delivery/{task['name']}-{task['generation'][:12]}",
                'published_head': None, 'published_verified': None, 'ci': None,
                'pr_body_hash': None}
    return None
