const API_BASE = '/api/v1';
const AUTH_TOKEN_KEY = 'gyre_auth_token';

function getAuthToken() {
  return localStorage.getItem(AUTH_TOKEN_KEY) || 'gyre-dev-token';
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
  agents: ({ workspaceId, status } = {}) => {
    const p = new URLSearchParams();
    if (status) p.set('status', status);
    if (workspaceId) p.set('workspace_id', workspaceId);
    const qs = p.toString();
    return request(`/agents${qs ? '?' + qs : ''}`);
  },
  agent: (id) => request(`/agents/${id}`),
  spawnAgent: (data) =>
    request('/agents/spawn', { method: 'POST', body: JSON.stringify(data) }),
  tasks: ({ workspaceId, status, assigned_to, parent_task_id } = {}) => {
    const p = new URLSearchParams();
    if (status) p.set('status', status);
    if (assigned_to) p.set('assigned_to', assigned_to);
    if (parent_task_id) p.set('parent_task_id', parent_task_id);
    if (workspaceId) p.set('workspace_id', workspaceId);
    const qs = p.toString();
    return request(`/tasks${qs ? '?' + qs : ''}`);
  },
  task: (id) => request(`/tasks/${id}`),
  projects: ({ workspaceId } = {}) => {
    const qs = workspaceId ? `?workspace_id=${encodeURIComponent(workspaceId)}` : '';
    return request(`/projects${qs}`);
  },
  project: (id) => request(`/projects/${id}`),
  repos: (projectId) => request(`/repos?project_id=${projectId}`),
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
  mrGates: (id) => request(`/merge-requests/${id}/gates`),
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
  // ABAC policy — server returns {repo_id, policies: [...]}; unwrap to array
  repoAbacPolicy: (id) => request(`/repos/${id}/abac-policy`).then(r => r?.policies ?? []),
  setRepoAbacPolicy: (id, policies) =>
    request(`/repos/${id}/abac-policy`, { method: 'PUT', body: JSON.stringify({ policies }) }),
  // Spec policy
  repoSpecPolicy: (id) => request(`/repos/${id}/spec-policy`),
  setRepoSpecPolicy: (id, policy) =>
    request(`/repos/${id}/spec-policy`, { method: 'PUT', body: JSON.stringify(policy) }),
  // Hot files & blame
  repoHotFiles: (id, limit = 20) => request(`/repos/${id}/hot-files?limit=${limit}`),
  repoBlame: (id, path) => request(`/repos/${id}/blame?path=${encodeURIComponent(path)}`),
  // Speculative merges
  repoSpeculative: (id) => request(`/repos/${id}/speculative`),
  // Agent commits
  repoAgentCommits: (id) => request(`/repos/${id}/agent-commits`),
  // Commit signature
  commitSignature: (id, sha) => request(`/repos/${id}/commits/${sha}/signature`),
  // AIBOM (AI Bill of Materials) — M14.3
  repoAibom: (id, from, to) => {
    const qs = from ? `?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to || '')}` : '';
    return request(`/repos/${id}/aibom${qs}`);
  },
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
  createMirrorRepo: (data) =>
    request('/repos/mirror', { method: 'POST', body: JSON.stringify(data) }),
  syncMirror: (id) =>
    request(`/repos/${id}/mirror/sync`, { method: 'POST' }),
  createTask: (data) =>
    request('/tasks', { method: 'POST', body: JSON.stringify(data) }),
  createMr: (data) =>
    request('/merge-requests', { method: 'POST', body: JSON.stringify(data) }),
  seedData: () =>
    request('/admin/seed', { method: 'POST' }),
  // Agent TTY WebSocket URL
  agentTtyUrl: (id) =>
    `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws/agents/${id}/tty`,
  // Agent logs
  agentLogs: (id, limit = 100, offset = 0) =>
    request(`/agents/${id}/logs?limit=${limit}&offset=${offset}`),
  appendAgentLog: (id, message) =>
    request(`/agents/${id}/logs`, { method: 'POST', body: JSON.stringify({ message }) }),
  // MR dependencies
  mrDependencies: (id) => request(`/merge-requests/${id}/dependencies`),
  setMrDependencies: (id, data) =>
    request(`/merge-requests/${id}/dependencies`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),
  removeMrDependency: (id, depId) =>
    request(`/merge-requests/${id}/dependencies/${depId}`, { method: 'DELETE' }),
  setMrAtomicGroup: (id, group) =>
    request(`/merge-requests/${id}/atomic-group`, {
      method: 'PUT',
      body: JSON.stringify({ group }),
    }),
  // Repo gates
  repoGates: (id) => request(`/repos/${id}/gates`),
  createRepoGate: (id, data) =>
    request(`/repos/${id}/gates`, { method: 'POST', body: JSON.stringify(data) }),
  deleteRepoGate: (id, gateId) =>
    request(`/repos/${id}/gates/${gateId}`, { method: 'DELETE' }),
  repoPushGates: (id) => request(`/repos/${id}/push-gates`),
  setRepoPushGates: (id, data) =>
    request(`/repos/${id}/push-gates`, { method: 'PUT', body: JSON.stringify(data) }),
  // Merge queue graph
  mergeQueueGraph: () => request('/merge-queue/graph'),
  // Data export
  adminExport: () => request('/admin/export'),
  // Retention
  adminRetention: () => request('/admin/retention'),
  adminUpdateRetention: (policies) =>
    request('/admin/retention', {
      method: 'PUT',
      body: JSON.stringify(policies),
    }),
  // SIEM forwarding targets
  siemList: () => request('/admin/siem'),
  siemCreate: (data) =>
    request('/admin/siem', { method: 'POST', body: JSON.stringify(data) }),
  siemUpdate: (id, data) =>
    request(`/admin/siem/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  siemDelete: (id) =>
    request(`/admin/siem/${id}`, { method: 'DELETE' }),
  // Compute targets
  computeList: () => request('/admin/compute-targets'),
  computeCreate: (data) =>
    request('/admin/compute-targets', { method: 'POST', body: JSON.stringify(data) }),
  computeDelete: (id) =>
    request(`/admin/compute-targets/${id}`, { method: 'DELETE' }),
  // Network / WireGuard peers
  networkPeers: () => request('/network/peers'),
  networkPeerCreate: (data) =>
    request('/network/peers', { method: 'POST', body: JSON.stringify(data) }),
  networkPeerDelete: (id) =>
    request(`/network/peers/${id}`, { method: 'DELETE' }),
  networkDerpMap: () => request('/network/derp-map'),
  // Agent spawn log
  agentSpawnLog: (id) => request(`/admin/agents/${id}/spawn-log`),
  // Container audit record (M19.3) — 404 if agent was not container-spawned
  agentContainer: (id) => request(`/agents/${id}/container`),
  // BCP (M23)
  bcpTargets: () => request('/admin/bcp/targets'),
  bcpDrill: () => request('/admin/bcp/drill', { method: 'POST' }),
  // M23 analytics (usage, compare, top)
  analyticsUsage: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/analytics/usage${qs ? '?' + qs : ''}`);
  },
  analyticsCompare: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/analytics/compare${qs ? '?' + qs : ''}`);
  },
  analyticsTop: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/analytics/top${qs ? '?' + qs : ''}`);
  },
  // Token introspection (M18)
  tokenInfo: () => request('/auth/token-info'),
  // Spec approvals ledger (M12.3)
  specsApprovals: (path) => {
    const qs = path ? `?path=${encodeURIComponent(path)}` : '';
    return request(`/specs/approvals${qs}`);
  },
  specsApprove: (data) =>
    request('/specs/approve', { method: 'POST', body: JSON.stringify(data) }),
  specsRevoke: (data) =>
    request('/specs/revoke', { method: 'POST', body: JSON.stringify(data) }),
  // Audit events (M7.1)
  auditEvents: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/audit/events${qs ? '?' + qs : ''}`);
  },
  auditStats: () => request('/audit/stats'),
  auditStreamUrl: () => `${API_BASE}/audit/stream`,
  // Spec registry (M21.1)
  getSpecs: () => request('/specs'),
  getSpec: (path) => request(`/specs/${encodeURIComponent(path)}`),
  approveSpec: (path, sha) =>
    request(`/specs/${encodeURIComponent(path)}/approve`, {
      method: 'POST',
      body: JSON.stringify({ sha }),
    }),
  revokeSpec: (path, reason) =>
    request(`/specs/${encodeURIComponent(path)}/revoke`, {
      method: 'POST',
      body: JSON.stringify({ reason }),
    }),
  getSpecHistory: (path) => request(`/specs/${encodeURIComponent(path)}/history`),
  getPendingSpecs: () => request('/specs/pending'),
  getDriftedSpecs: () => request('/specs/drifted'),
  // Search (M22.7)
  search: ({ q, entity_type, workspace_id, limit = 20 } = {}) => {
    const params = new URLSearchParams({ q: q || '' });
    if (entity_type) params.set('entity_type', entity_type);
    if (workspace_id) params.set('workspace_id', workspace_id);
    params.set('limit', String(limit));
    return request(`/search?${params.toString()}`);
  },
  // Spec graph (M22.3)
  specsGraph: () => request('/specs/graph'),
  // Workspaces (M22.5)
  workspaces: () => request('/workspaces'),
  workspace: (id) => request(`/workspaces/${id}`),
  createWorkspace: (data) =>
    request('/workspaces', { method: 'POST', body: JSON.stringify(data) }),
  workspaceBudget: (id) => request(`/workspaces/${id}/budget`),
  setWorkspaceBudget: (id, data) =>
    request(`/workspaces/${id}/budget`, { method: 'PUT', body: JSON.stringify(data) }),
  budgetSummary: () => request('/budget/summary'),
  workspaceRepos: (id) => request(`/workspaces/${id}/repos`),
  workspaceMembers: (id) => request(`/workspaces/${id}/members`),
  workspaceTeams: (id) => request(`/workspaces/${id}/teams`),
  addWorkspaceMember: (id, data) =>
    request(`/workspaces/${id}/members`, { method: 'POST', body: JSON.stringify(data) }),
  removeWorkspaceMember: (id, userId) =>
    request(`/workspaces/${id}/members/${userId}`, { method: 'DELETE' }),
  // Personas (M22.5)
  personas: () => request('/personas'),
  persona: (id) => request(`/personas/${id}`),
  createPersona: (data) =>
    request('/personas', { method: 'POST', body: JSON.stringify(data) }),
  updatePersona: (id, data) =>
    request(`/personas/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  deletePersona: (id) =>
    request(`/personas/${id}`, { method: 'DELETE' }),
  // Dependency graph (M22.5)
  dependencyGraph: () => request('/dependencies/graph'),
  repoDependencies: (id) => request(`/repos/${id}/dependencies`),
  repoDependents: (id) => request(`/repos/${id}/dependents`),
  repoBlastRadius: (id) => request(`/repos/${id}/blast-radius`),
  // User profile (M22.5)
  me: () => request('/users/me'),
  updateMe: (data) =>
    request('/users/me', { method: 'PUT', body: JSON.stringify(data) }),
  myAgents: () => request('/users/me/agents'),
  myTasks: () => request('/users/me/tasks'),
  myMrs: () => request('/users/me/mrs'),
  myNotifications: () => request('/users/me/notifications'),
  markNotificationRead: (id) =>
    request(`/users/me/notifications/${id}/read`, { method: 'PUT' }),
};
