const API_BASE = '/api/v1';
const AUTH_TOKEN_KEY = 'gyre_auth_token';

function getAuthToken() {
  return localStorage.getItem(AUTH_TOKEN_KEY) || 'test-token';
}

export function setAuthToken(token) {
  localStorage.setItem(AUTH_TOKEN_KEY, token);
}

async function request(path, options = {}) {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${getAuthToken()}`,
      ...options.headers,
    },
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
  // jj VCS operations
  jjInit: (repoId) =>
    request(`/repos/${repoId}/jj/init`, { method: 'POST' }),
  jjLog: (repoId, limit = 20) =>
    request(`/repos/${repoId}/jj/log?limit=${limit}`),
  jjNew: (repoId, description) =>
    request(`/repos/${repoId}/jj/new`, {
      method: 'POST',
      body: JSON.stringify({ description }),
    }),
  jjSquash: (repoId) =>
    request(`/repos/${repoId}/jj/squash`, { method: 'POST' }),
  jjUndo: (repoId) =>
    request(`/repos/${repoId}/jj/undo`, { method: 'POST' }),
  jjBookmark: (repoId, name, change_id) =>
    request(`/repos/${repoId}/jj/bookmark`, {
      method: 'POST',
      body: JSON.stringify({ name, change_id }),
    }),
  // MCP tools catalog (endpoint is /mcp, not under /api/v1/)
  mcpTools: async () => {
    const res = await fetch('/mcp', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${getAuthToken()}` },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'tools/list',
        params: {},
      }),
    });
    if (!res.ok) throw new Error(`MCP tools/list: ${res.status}`);
    const json = await res.json();
    return json.result?.tools ?? [];
  },
  // Agent card
  updateAgentCard: (agentId, card) =>
    request(`/agents/${agentId}/card`, { method: 'PUT', body: JSON.stringify(card) }),
  // Compose
  composeApply: (spec) =>
    request('/compose/apply', { method: 'POST', body: JSON.stringify(spec) }),
  composeStatus: (composeId) =>
    request(`/compose/status?compose_id=${encodeURIComponent(composeId)}`),
  composeTeardown: (composeId) =>
    request('/compose/teardown', { method: 'POST', body: JSON.stringify({ compose_id: composeId }) }),
  // Admin (requires Admin role)
  adminHealth: () => request('/admin/health'),
  adminJobs: () => request('/admin/jobs'),
  adminRunJob: (name) => request(`/admin/jobs/${name}/run`, { method: 'POST' }),
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
  // Snapshots
  adminCreateSnapshot: () => request('/admin/snapshot', { method: 'POST' }),
  adminListSnapshots: () => request('/admin/snapshots'),
  adminRestoreSnapshot: (snapshotId) =>
    request('/admin/restore', {
      method: 'POST',
      body: JSON.stringify({ snapshot_id: snapshotId }),
    }),
  adminDeleteSnapshot: (id) =>
    request(`/admin/snapshots/${id}`, { method: 'DELETE' }),
  // CRUD create methods
  createProject: (data) =>
    request('/projects', { method: 'POST', body: JSON.stringify(data) }),
  createRepo: (data) =>
    request('/repos', { method: 'POST', body: JSON.stringify(data) }),
  createTask: (data) =>
    request('/tasks', { method: 'POST', body: JSON.stringify(data) }),
  createMr: (data) =>
    request('/merge-requests', { method: 'POST', body: JSON.stringify(data) }),
  seedData: () =>
    request('/admin/seed', { method: 'POST' }),
  // Data export
  adminExport: () => request('/admin/export'),
  // Retention
  adminRetention: () => request('/admin/retention'),
  adminUpdateRetention: (policies) =>
    request('/admin/retention', {
      method: 'PUT',
      body: JSON.stringify(policies),
    }),
};
