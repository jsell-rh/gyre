const API_BASE = '/api/v1';
const AUTH_TOKEN_KEY = 'gyre_auth_token';

/**
 * Sanitize an API-derived URL for use in href attributes.
 * Only allows http: and https: schemes; rejects javascript:, data:, etc.
 * Returns '#' for unsafe or invalid URLs.
 */
export function safeHref(url) {
  if (!url || typeof url !== 'string') return '#';
  const trimmed = url.trim();
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  // Relative URLs are fine
  if (trimmed.startsWith('/')) return trimmed;
  return '#';
}

function getAuthToken() {
  const token = localStorage.getItem(AUTH_TOKEN_KEY);
  if (!token && typeof window !== 'undefined' && window.location.hostname !== 'localhost' && window.location.hostname !== '127.0.0.1') {
    console.warn('[api] No auth token in localStorage and not on localhost — API calls may fail');
  }
  return token || 'gyre-dev-token';
}

export function setAuthToken(token) {
  localStorage.setItem(AUTH_TOKEN_KEY, token);
}

async function request(path, options = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 15000);
  try {
    const { headers: customHeaders, ...restOptions } = options;
    const res = await fetch(`${API_BASE}${path}`, {
      ...restOptions,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${getAuthToken()}`,
        ...customHeaders,
      },
      signal: controller.signal,
    });
    if (!res.ok) {
      if (res.status === 401) {
        throw new Error('Session expired — please re-authenticate.');
      }
      throw new Error(`API ${path}: ${res.status} ${res.statusText}`);
    }
    if (res.status === 204) return null;
    return res.json();
  } catch (e) {
    if (e.name === 'AbortError') {
      throw new Error('Request timed out. Check your connection.');
    }
    if (e instanceof TypeError && !navigator.onLine) {
      throw new Error('You appear to be offline. Check your network connection.');
    }
    throw e;
  } finally {
    clearTimeout(timeout);
  }
}

export const api = {
  version: () => request('/version'),
  activity: (limit = 100) => request(`/activity?limit=${limit}`),
  agents: ({ workspaceId, repoId, status } = {}) => {
    const p = new URLSearchParams();
    if (status) p.set('status', status);
    if (workspaceId) p.set('workspace_id', workspaceId);
    if (repoId) p.set('repo_id', repoId);
    const qs = p.toString();
    return request(`/agents${qs ? '?' + qs : ''}`);
  },
  agent: (id) => request(`/agents/${id}`),
  repo: (id) => request(`/repos/${id}`),
  spawnAgent: (data) =>
    request('/agents/spawn', { method: 'POST', body: JSON.stringify(data) }),
  tasks: ({ workspaceId, repoId, status, assigned_to, parent_task_id } = {}) => {
    const p = new URLSearchParams();
    if (status) p.set('status', status);
    if (assigned_to) p.set('assigned_to', assigned_to);
    if (parent_task_id) p.set('parent_task_id', parent_task_id);
    if (workspaceId) p.set('workspace_id', workspaceId);
    if (repoId) p.set('repo_id', repoId);
    const qs = p.toString();
    return request(`/tasks${qs ? '?' + qs : ''}`);
  },
  task: (id) => request(`/tasks/${id}`),
  updateTaskStatus: (id, status) => request(`/tasks/${id}/status`, { method: 'PUT', body: JSON.stringify({ status }) }),
  repos: ({ workspaceId } = {}) => {
    const qs = workspaceId ? `?workspace_id=${encodeURIComponent(workspaceId)}` : '';
    return request(`/repos${qs}`);
  },
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
  submitComment: (mrId, data) =>
    request(`/merge-requests/${mrId}/comments`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  mrDiff: (id) => request(`/merge-requests/${id}/diff`),
  mrGates: (id) => request(`/merge-requests/${id}/gates`),
  mrAttestation: (id) => request(`/merge-requests/${id}/attestation`),
  mrTimeline: (id) => request(`/merge-requests/${id}/timeline`),
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
  // Review routing
  repoReviewRouting: (id, path) => request(`/repos/${id}/review-routing${path ? '?path=' + encodeURIComponent(path) : ''}`),
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
  agentCard: (agentId) =>
    request(`/agents/${agentId}/card`),
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
  createRepo: (data) =>
    request('/repos', { method: 'POST', body: JSON.stringify(data) }),
  updateRepo: (id, data) =>
    request(`/repos/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  archiveRepo: (id) =>
    request(`/repos/${id}/archive`, { method: 'POST' }),
  unarchiveRepo: (id) =>
    request(`/repos/${id}/unarchive`, { method: 'POST' }),
  deleteRepo: (id) =>
    request(`/repos/${id}`, { method: 'DELETE' }),
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
  // Agent log SSE stream URL (for live tailing active agents)
  agentLogStreamUrl: (id) =>
    `${API_BASE}/agents/${id}/logs/stream`,
  appendAgentLog: (id, message) =>
    request(`/agents/${id}/logs`, { method: 'POST', body: JSON.stringify({ message }) }),
  // Agent messages (distinct from logs — typed messages: TaskAssignment, ReviewRequest, etc.)
  agentMessages: (id) => request(`/agents/${id}/messages`),
  // Send message via workspace message bus (POST /workspaces/:wsId/messages)
  // Server expects: { to: { agent: "<id>" }, kind: "FreeText", payload: {...} }
  sendAgentMessage: (workspaceId, agentId, data) =>
    request(`/workspaces/${workspaceId}/messages`, {
      method: 'POST',
      body: JSON.stringify({
        to: { agent: agentId },
        kind: data.kind ?? 'FreeText',
        payload: data.payload ?? { content: data.content },
      }),
    }),
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
  // Agent spawn log — endpoint removed; agent logs are available via /agents/:id/logs
  agentSpawnLog: (id) => request(`/agents/${id}/logs?limit=50&offset=0`),
  // Container audit record (M19.3) — 404 if agent was not container-spawned
  agentContainer: (id) => request(`/agents/${id}/container`),
  // Agent workload attestation (G10) — pid, hostname, compute_target, stack_hash, alive
  agentWorkload: (id) => request(`/agents/${id}/workload`),
  // Agent touched paths (M13.4) — all branches and files written by this agent
  agentTouchedPaths: (id) => request(`/agents/${id}/touched-paths`),
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
  // Audit events (M7.1)
  auditEvents: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/audit/events${qs ? '?' + qs : ''}`);
  },
  auditStats: () => request('/audit/stats'),
  auditStreamUrl: () => `${API_BASE}/audit/stream`,
  // Trace capture (HSI §3a)
  mrTrace: (id) => request(`/merge-requests/${id}/trace`),
  // Spec registry (M21.1)
  getSpecs: () => request('/specs'),
  getSpec: (path) => request(`/specs/${encodeURIComponent(path)}`),
  approveSpec: (path, sha, { output_constraints, scope } = {}) =>
    request(`/specs/${encodeURIComponent(path)}/approve`, {
      method: 'POST',
      body: JSON.stringify({
        sha,
        ...(output_constraints?.length ? { output_constraints } : {}),
        ...(scope ? { scope } : {}),
      }),
    }),
  revokeSpec: (path, reason) =>
    request(`/specs/${encodeURIComponent(path)}/revoke`, {
      method: 'POST',
      body: JSON.stringify({ reason }),
    }),
  rejectSpec: (path, reason) =>
    request(`/specs/${encodeURIComponent(path)}/reject`, {
      method: 'POST',
      body: JSON.stringify({ reason }),
    }),
  getSpecHistory: (path) => request(`/specs/${encodeURIComponent(path)}/history`),
  getSpecProgress: (path) => request(`/specs/${encodeURIComponent(path)}/progress`),
  getPendingSpecs: () => request('/specs/pending'),
  getDriftedSpecs: () => request('/specs/drifted'),
  // Constraint validation dry-run (authorization-provenance.md §7.6)
  validateConstraints: (data) =>
    request('/constraints/validate', { method: 'POST', body: JSON.stringify(data) }),
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
  // Meta-spec registry (M32 / agent-runtime §2)
  getMetaSpecs: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/meta-specs-registry${qs ? '?' + qs : ''}`);
  },
  getMetaSpec: (id) => request(`/meta-specs-registry/${id}`),
  createMetaSpec: (data) =>
    request('/meta-specs-registry', { method: 'POST', body: JSON.stringify(data) }),
  updateMetaSpec: (id, data) =>
    request(`/meta-specs-registry/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  deleteMetaSpec: (id) =>
    request(`/meta-specs-registry/${id}`, { method: 'DELETE' }),
  getMetaSpecVersions: (id) => request(`/meta-specs-registry/${id}/versions`),
  getMetaSpecVersion: (id, ver) => request(`/meta-specs-registry/${id}/versions/${ver}`),
  getMetaSpecBlastRadius: (path) => request(`/meta-specs/${encodeURIComponent(path)}/blast-radius`),
  getWorkspaceMetaSpecSet: (id) => request(`/workspaces/${id}/meta-spec-set`),
  setWorkspaceMetaSpecSet: (id, data) =>
    request(`/workspaces/${id}/meta-spec-set`, { method: 'PUT', body: JSON.stringify(data) }),
  // Workspace briefing (TASK-205)
  getWorkspaceBriefing: (id, since) => {
    const params = new URLSearchParams();
    if (since) params.set('since', String(since));
    const qs = params.toString();
    return request(`/workspaces/${id}/briefing${qs ? `?${qs}` : ''}`);
  },
  // Briefing Q&A SSE (S4.3) — returns a fetch Response; caller handles SSE
  briefingAsk: (workspaceId, body) =>
    fetch(`${API_BASE}/workspaces/${workspaceId}/briefing/ask`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${getAuthToken()}`,
      },
      body: JSON.stringify(body),
    }),
  // Tenants (M34)
  tenants: () => request('/tenants'),
  tenant: (id) => request(`/tenants/${id}`),
  createTenant: (data) =>
    request('/tenants', { method: 'POST', body: JSON.stringify(data) }),
  updateTenant: (id, data) =>
    request(`/tenants/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  deleteTenant: (id) =>
    request(`/tenants/${id}`, { method: 'DELETE' }),
  // Workspaces (M22.5)
  workspaces: () => request('/workspaces'),
  workspace: (id) => request(`/workspaces/${id}`),
  createWorkspace: (data) =>
    request('/workspaces', { method: 'POST', body: JSON.stringify(data) }),
  workspaceBudget: (id) => request(`/workspaces/${id}/budget`),
  setWorkspaceBudget: (id, data) =>
    request(`/workspaces/${id}/budget`, { method: 'PUT', body: JSON.stringify(data) }),
  budgetSummary: () => request('/budget/summary'),
  costSummary: (since, until) => {
    const params = new URLSearchParams();
    if (since != null) params.set('since', since);
    if (until != null) params.set('until', until);
    const qs = params.toString();
    return request(`/costs/summary${qs ? '?' + qs : ''}`);
  },
  costsByAgent: (agentId) => request(`/costs?agent_id=${encodeURIComponent(agentId)}`),
  // Per-repo budget endpoint does not exist; use workspace budget instead.
  // Components should prefer workspaceBudget(workspaceId) when a workspace ID is available.
  repoBudget: (_id) => Promise.resolve(null),
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
  approvePersona: (id) =>
    request(`/personas/${id}/approve`, { method: 'POST' }),
  resolvePersona: (slug, scopeKind, scopeId) =>
    request(`/personas/resolve?slug=${encodeURIComponent(slug)}&scope_kind=${encodeURIComponent(scopeKind)}&scope_id=${encodeURIComponent(scopeId)}`),
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
  myNotifications: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/users/me/notifications${qs ? '?' + qs : ''}`);
  },
  notificationCount: (workspaceId) => {
    const qs = workspaceId ? `?workspace_id=${encodeURIComponent(workspaceId)}` : '';
    return request(`/users/me/notifications/count${qs}`).then(r => r?.count ?? 0);
  },
  markNotificationRead: (id) =>
    request(`/notifications/${id}/dismiss`, { method: 'POST' }),
  resolveNotification: (id) =>
    request(`/notifications/${id}/resolve`, { method: 'POST' }),
  // Notification preferences (HSI §12)
  getNotificationPreferences: () => request('/users/me/notification-preferences'),
  updateNotificationPreferences: (prefs) =>
    request('/users/me/notification-preferences', { method: 'PUT', body: JSON.stringify(prefs) }),
  myJudgments: (params) => {
    const qs = params ? new URLSearchParams(params).toString() : '';
    return request(`/users/me/judgments${qs ? '?' + qs : ''}`);
  },
  // Knowledge graph (TASK-174/TASK-175)
  repoGraph: (id) => request(`/repos/${id}/graph`),
  repoGraphNode: (repoId, nodeId) => request(`/repos/${repoId}/graph/node/${nodeId}`),
  repoGraphTypes: (id) => request(`/repos/${id}/graph/types`),
  repoGraphModules: (id) => request(`/repos/${id}/graph/modules`),
  repoGraphRisks: (id) => request(`/repos/${id}/graph/risks`),
  getGraphConcept: (repoId, name) => request(`/repos/${repoId}/graph/concept/${encodeURIComponent(name)}`),
  repoGraphTimeline: (id, since, until) => {
    const params = new URLSearchParams();
    if (since != null) params.set('since', since);
    if (until != null) params.set('until', until);
    const qs = params.toString();
    return request(`/repos/${id}/graph/timeline${qs ? '?' + qs : ''}`);
  },
  workspaceGraph: (id) => request(`/workspaces/${id}/graph`),
  // Meta-spec preview loop (S4.6)
  previewPersona: (workspaceId, data) =>
    request(`/workspaces/${workspaceId}/meta-specs/preview`, { method: 'POST', body: JSON.stringify(data) }),
  previewPersonaStatus: (workspaceId, previewId) =>
    request(`/workspaces/${workspaceId}/meta-specs/preview/${previewId}`),
  publishPersona: (_workspaceId, personaId, data) =>
    request(`/meta-specs-registry/${personaId}`, { method: 'PUT', body: JSON.stringify({ prompt: data.content }) }),
  // Workspace admin (S4.7)
  updateWorkspace: (id, data) =>
    request(`/workspaces/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  // Tenant-level ABAC policies
  policies: () => request('/policies'),
  createPolicy: (data) =>
    request('/policies', { method: 'POST', body: JSON.stringify(data) }),
  deletePolicy: (id) =>
    request(`/policies/${id}`, { method: 'DELETE' }),
  policyDecisions: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/policies/decisions${qs ? '?' + qs : ''}`);
  },
  workspaceAbacPolicies: (id) => request(`/policies?scope=Workspace&scope_id=${id}`),
  createWorkspaceAbacPolicy: (id, data) =>
    request('/policies', { method: 'POST', body: JSON.stringify({ ...data, scope: 'Workspace', scope_id: id }) }),
  deleteWorkspaceAbacPolicy: (id, policyId) =>
    request(`/policies/${policyId}`, { method: 'DELETE' }),
  simulateAbacPolicy: (id, data) =>
    request('/policies/evaluate', { method: 'POST', body: JSON.stringify(data) }),
  // Explorer saved views — repo-scoped (canonical, via WS or REST)
  savedViews: (repoId) => request(`/repos/${repoId}/views`),
  createSavedView: (repoId, data) =>
    request(`/repos/${repoId}/views`, { method: 'POST', body: JSON.stringify(data) }),
  updateSavedView: (repoId, viewId, data) =>
    request(`/repos/${repoId}/views/${viewId}`, { method: 'PUT', body: JSON.stringify(data) }),
  deleteSavedView: (repoId, viewId) =>
    request(`/repos/${repoId}/views/${viewId}`, { method: 'DELETE' }),
  // Deprecated: workspace-scoped explorer views (legacy KV-based, use savedViews instead)
  explorerViews: (workspaceId) => request(`/workspaces/${workspaceId}/explorer-views`),
  saveExplorerView: (workspaceId, data) =>
    request(`/workspaces/${workspaceId}/explorer-views`, { method: 'POST', body: JSON.stringify(data) }),
  deleteExplorerView: (workspaceId, id) =>
    request(`/workspaces/${workspaceId}/explorer-views/${id}`, { method: 'DELETE' }),
  generateExplorerView: (workspaceId, body) =>
    fetch(`${API_BASE}/workspaces/${workspaceId}/explorer-views/generate`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${getAuthToken()}`,
      },
      body: JSON.stringify(body),
    }),
  // Specs View (S4.5)
  specsForWorkspace: (workspaceId) =>
    request(`/specs${workspaceId ? '?workspace_id=' + encodeURIComponent(workspaceId) : ''}`),
  specContent: (path, repoId) =>
    request(`/specs/${encodeURIComponent(path)}${repoId ? '?repo_id=' + encodeURIComponent(repoId) : ''}`),
  updateSpec: (path, repoId, content) =>
    request(`/specs/${encodeURIComponent(path)}${repoId ? '?repo_id=' + encodeURIComponent(repoId) : ''}`, {
      method: 'PUT',
      body: JSON.stringify({ content }),
    }),
  specProgress: (path, repoId) =>
    request(`/specs/${encodeURIComponent(path)}/progress${repoId ? '?repo_id=' + encodeURIComponent(repoId) : ''}`),
  specLinks: (path, repoId) =>
    request(`/specs/${encodeURIComponent(path)}/links${repoId ? '?repo_id=' + encodeURIComponent(repoId) : ''}`),
  specHistoryRepo: (path, repoId) =>
    request(`/specs/${encodeURIComponent(path)}/history${repoId ? '?repo_id=' + encodeURIComponent(repoId) : ''}`),
  checkSpecAssertions: (repoId, specPath, content) =>
    request(`/repos/${repoId}/spec-assertions/check`, {
      method: 'POST',
      body: JSON.stringify({ spec_path: specPath, content }),
    }),
  specsAssist: (repoId, body) =>
    fetch(`${API_BASE}/repos/${repoId}/specs/assist`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${getAuthToken()}`,
      },
      body: JSON.stringify(body),
    }),
  // Server does not have POST /specs/assist — only POST /repos/:id/specs/assist.
  // Return a failed Response so callers fall through gracefully.
  specsAssistGlobal: (_body) =>
    Promise.resolve(new Response(null, { status: 404, statusText: 'Not available without repo context' })),
  specsSave: (repoId, data) =>
    request(`/repos/${repoId}/specs/save`, { method: 'POST', body: JSON.stringify(data) }),
  costs: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/costs${qs ? '?' + qs : ''}`);
  },
  // Analytics events
  analyticsEvents: (params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/analytics/events${qs ? '?' + qs : ''}`);
  },
  // Presence
  workspacePresence: (workspaceId) =>
    request(`/workspaces/${workspaceId}/presence`),
  // Release preparation
  releasePrep: (repoId) =>
    request('/release/prepare', { method: 'POST', body: JSON.stringify({ repo_id: repoId }) }),
  // Conversation provenance
  conversationProvenance: (sha) => request(`/conversations/${sha}`),
  // Agent discovery
  agentDiscovery: (capability) => {
    const qs = capability ? `?capability=${encodeURIComponent(capability)}` : '';
    return request(`/agents/discover${qs}`);
  },
  // ── LLM Configuration (per-workspace overrides) ────────────────────
  llmConfigList: (workspaceId) =>
    request(`/workspaces/${workspaceId}/llm/config`),
  llmConfigGet: (workspaceId, feature) =>
    request(`/workspaces/${workspaceId}/llm/config/${encodeURIComponent(feature)}`),
  llmConfigSet: (workspaceId, feature, data) =>
    request(`/workspaces/${workspaceId}/llm/config/${encodeURIComponent(feature)}`, { method: 'PUT', body: JSON.stringify(data) }),
  llmConfigDelete: (workspaceId, feature) =>
    request(`/workspaces/${workspaceId}/llm/config/${encodeURIComponent(feature)}`, { method: 'DELETE' }),
  llmPromptGet: (workspaceId, feature) =>
    request(`/workspaces/${workspaceId}/llm/prompts/${encodeURIComponent(feature)}`),
  llmPromptSet: (workspaceId, feature, data) =>
    request(`/workspaces/${workspaceId}/llm/prompts/${encodeURIComponent(feature)}`, { method: 'PUT', body: JSON.stringify(data) }),
  llmPromptDelete: (workspaceId, feature) =>
    request(`/workspaces/${workspaceId}/llm/prompts/${encodeURIComponent(feature)}`, { method: 'DELETE' }),
  // Admin LLM defaults
  adminLlmConfigGet: (feature) =>
    request(`/admin/llm/config/${encodeURIComponent(feature)}`),
  adminLlmConfigSet: (feature, data) =>
    request(`/admin/llm/config/${encodeURIComponent(feature)}`, { method: 'PUT', body: JSON.stringify(data) }),
  adminLlmPromptGet: (feature) =>
    request(`/admin/llm/prompts/${encodeURIComponent(feature)}`),
  adminLlmPromptSet: (feature, data) =>
    request(`/admin/llm/prompts/${encodeURIComponent(feature)}`, { method: 'PUT', body: JSON.stringify(data) }),
  // Graph diff (architecture delta between commits/branches)
  repoGraphDiff: (id, params = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request(`/repos/${id}/graph/diff${qs ? '?' + qs : ''}`);
  },
  // Workspace-scope concept search
  workspaceGraphConcept: (wsId, name) =>
    request(`/workspaces/${wsId}/graph/concept/${encodeURIComponent(name)}`),
  // API token management
  createApiToken: (data) =>
    request('/users/me/tokens', { method: 'POST', body: JSON.stringify(data) }),
  deleteApiToken: (id) =>
    request(`/users/me/tokens/${id}`, { method: 'DELETE' }),
  listApiTokens: () => request('/users/me/tokens'),
  // Server version
  serverVersion: () => request('/version'),
  // Activity feed
  activityFeed: (limit = 20) =>
    request(`/activity?limit=${limit}`),
  // Graph predict — LLM-powered structural predictions (TASK-358)
  graphPredict: async (repoId, body) => {
    try {
      return await request(`/repos/${repoId}/graph/predict`, {
        method: 'POST',
        body: JSON.stringify(body ?? {}),
      });
    } catch (e) {
      if (e.message?.includes('503')) {
        throw new Error('LLM unavailable — graph predictions require an LLM to be configured.');
      }
      throw e;
    }
  },
  // Thorough preview: create throwaway branch, run agents, get real impact
  thoroughPreview: async (repoId, body) => {
    return await request(`/repos/${repoId}/graph/thorough-preview`, {
      method: 'POST',
      body: JSON.stringify(body ?? {}),
    });
  },
  // Task status polling
  taskStatus: async (taskId) => {
    return await request(`/tasks/${taskId}`);
  },
  // View query dry-run — resolves a view query server-side, returns node_metrics etc.
  graphQueryDryrun: (repoId, query, selectedNodeId) =>
    request(`/repos/${repoId}/graph/query-dryrun`, {
      method: 'POST',
      body: JSON.stringify({ query, selected_node_id: selectedNodeId ?? null }),
    }),
};
