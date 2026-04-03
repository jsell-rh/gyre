/**
 * Shared status tooltip functions — provide "WHY" context for status indicators.
 *
 * Extracted from WorkspaceHome.svelte for reuse across DetailPanel, RepoMode,
 * RepoCard, and any component that displays entity status.
 */
import { entityName } from './entityNames.svelte.js';

export const SPEC_STATUS_ICONS = {
  draft: '~',
  pending: '?',
  approved: '✓',
  rejected: '✗',
  implemented: '✓',
  merged: '✓',
};

export function specStatusTooltip(status) {
  switch (status) {
    case 'draft': return 'Synced from repo — not yet submitted for approval';
    case 'pending': return 'Awaiting YOUR approval before agents can work on it';
    case 'approved': return 'Approved — agents can create tasks and implement';
    case 'rejected': return 'Rejected — implementation blocked';
    case 'implemented': return 'All linked tasks completed';
    default: return '';
  }
}

export function taskStatusTooltip(task) {
  const status = typeof task === 'string' ? task : task?.status;
  const specName = typeof task === 'object' ? task?.spec_path?.split('/').pop()?.replace(/\.md$/, '') : null;
  const agentName = typeof task === 'object' && task?.assigned_to ? entityName('agent', task.assigned_to) : null;
  switch (status) {
    case 'backlog': return `Waiting to be assigned${specName ? ` (from spec: ${specName})` : ''}`;
    case 'in_progress': return `${agentName ?? 'Agent'} is working on this${specName ? ` (spec: ${specName})` : ''}`;
    case 'review': return `Agent completed — awaiting review${specName ? ` of ${specName}` : ''}`;
    case 'done': return `Completed${specName ? ` — implemented ${specName}` : ''}`;
    case 'blocked': return 'Blocked by a dependency or external factor';
    case 'cancelled': return 'Cancelled — linked spec may have been rejected';
    default: return '';
  }
}

export function mrStatusTooltip(mr) {
  if (mr.queue_position != null) return `MR is position ${mr.queue_position + 1} in the merge queue — gates will run before merge`;
  switch (mr.status) {
    case 'open': {
      if (mr._gates?.failed > 0) return `MR blocked — ${mr._gates.failed} gate(s) failed: ${mr._gates.details?.filter(g => g.status === 'failed').map(g => g.name).join(', ') ?? 'unknown'}`;
      if (mr.has_conflicts) return 'MR has merge conflicts with the target branch';
      return 'MR is open and ready to be enqueued for merge';
    }
    case 'merged': {
      const parts = ['MR passed all required gates and was merged'];
      if (mr.merge_commit_sha) parts.push(`commit ${mr.merge_commit_sha.slice(0, 7)}`);
      if (mr._gates?.total > 0) parts.push(`${mr._gates.passed}/${mr._gates.total} gates passed`);
      return parts.join(' — ');
    }
    case 'closed': return 'MR was closed without merging — may have failed gates or been superseded';
    default: return '';
  }
}

export function agentStatusTooltip(agent) {
  const status = typeof agent === 'string' ? agent : agent?.status;
  const spawned = typeof agent === 'object' ? agent?.spawned_at ?? agent?.created_at : null;
  const completed = typeof agent === 'object' ? agent?.completed_at : null;
  let duration = '';
  if (spawned) {
    const start = new Date(typeof spawned === 'number' ? spawned * 1000 : spawned).getTime();
    const end = completed ? new Date(typeof completed === 'number' ? completed * 1000 : completed).getTime() : Date.now();
    const secs = Math.round((end - start) / 1000);
    if (secs < 60) duration = ` (${secs}s)`;
    else if (secs < 3600) duration = ` (${Math.round(secs / 60)}m)`;
    else duration = ` (${Math.round(secs / 3600)}h)`;
  }
  switch (status) {
    case 'active': return `Agent is currently running${duration}`;
    case 'idle': return `Agent completed its work${duration} — MR should have been created`;
    case 'completed': return `Agent finished successfully${duration}`;
    case 'failed': return `Agent encountered an error${duration}`;
    case 'dead': return 'Agent was killed by an administrator';
    case 'stopped': return 'Agent was stopped gracefully';
    default: return '';
  }
}

/**
 * Build a status journey array from MR timeline events.
 * Returns [{step, status, timestamp, detail}] for rendering a stepper.
 */
export function mrStatusJourney(mr, timelineEvents) {
  const steps = [
    { step: 'Created', status: 'done', timestamp: mr?.created_at },
  ];

  // Gates
  const gates = mr?._gates ?? {};
  if (gates.total > 0) {
    if (gates.failed > 0) {
      steps.push({ step: 'Gates', status: 'failed', detail: `${gates.failed} failed` });
    } else if (gates.passed === gates.total) {
      steps.push({ step: 'Gates', status: 'done', detail: `${gates.passed} passed` });
    } else {
      steps.push({ step: 'Gates', status: 'pending', detail: `${gates.passed}/${gates.total}` });
    }
  }

  // Queue
  if (mr?.queue_position != null) {
    steps.push({ step: 'Queued', status: 'active', detail: `#${mr.queue_position + 1}` });
  } else if (mr?.status === 'merged') {
    steps.push({ step: 'Queued', status: 'done' });
  }

  // Merged
  if (mr?.status === 'merged') {
    steps.push({ step: 'Merged', status: 'done', timestamp: mr?.merged_at });
  } else if (mr?.status === 'closed') {
    steps.push({ step: 'Closed', status: 'failed' });
  }

  return steps;
}
