const API_BASE = '/api/v1';

async function request(path, options = {}) {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });
  if (!res.ok) {
    throw new Error(`API ${path}: ${res.status} ${res.statusText}`);
  }
  if (res.status === 204) return null;
  return res.json();
}

export const api = {
  version: () => request('/version'),
  activity: (limit = 100) => request(`/activity?limit=${limit}`),
  agents: () => request('/agents'),
  agent: (id) => request(`/agents/${id}`),
  spawnAgent: (data) =>
    request('/agents/spawn', { method: 'POST', body: JSON.stringify(data) }),
  tasks: () => request('/tasks'),
  task: (id) => request(`/tasks/${id}`),
  projects: () => request('/projects'),
  project: (id) => request(`/projects/${id}`),
  repos: (projectId) => request(`/projects/${projectId}/repos`),
  allRepos: () => request('/repos'),
  repoBranches: (id) => request(`/repos/${id}/branches`),
  repoCommits: (id, branch, limit = 50) =>
    request(`/repos/${id}/commits?branch=${encodeURIComponent(branch)}&limit=${limit}`),
  mergeRequests: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/merge-requests${qs ? '?' + qs : ''}`);
  },
  mergeRequest: (id) => request(`/merge-requests/${id}`),
  mrReviews: (id) => request(`/merge-requests/${id}/reviews`),
  mrComments: (id) => request(`/merge-requests/${id}/comments`),
  mrDiff: (id) => request(`/merge-requests/${id}/diff`),
  submitReview: (mrId, data) =>
    request(`/merge-requests/${mrId}/reviews`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  mergeQueue: () => request('/merge-queue'),
  enqueue: (mrId, priority = 50) =>
    request('/merge-queue/enqueue', {
      method: 'POST',
      body: JSON.stringify({ merge_request_id: mrId, priority }),
    }),
  cancelQueueEntry: (id) =>
    request(`/merge-queue/${id}`, { method: 'DELETE' }),
  // Admin (requires Admin role)
  adminHealth: () => request('/admin/health'),
  adminJobs: () => request('/admin/jobs'),
  adminAudit: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/admin/audit${qs ? '?' + qs : ''}`);
  },
  adminKillAgent: (id) =>
    request(`/admin/agents/${id}/kill`, { method: 'POST' }),
  adminReassignAgent: (id, targetAgentId) =>
    request(`/admin/agents/${id}/reassign`, {
      method: 'POST',
      body: JSON.stringify({ target_agent_id: targetAgentId }),
    }),
};
