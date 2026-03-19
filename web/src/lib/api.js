const API_BASE = '/api/v1';

async function request(path, options = {}) {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });
  if (!res.ok) {
    throw new Error(`API ${path}: ${res.status} ${res.statusText}`);
  }
  return res.json();
}

export const api = {
  version: () => request('/version'),
  activity: (limit = 100) => request(`/activity?limit=${limit}`),
  agents: () => request('/agents'),
  agent: (id) => request(`/agents/${id}`),
  tasks: () => request('/tasks'),
  task: (id) => request(`/tasks/${id}`),
  projects: () => request('/projects'),
  project: (id) => request(`/projects/${id}`),
  repos: (projectId) => request(`/projects/${projectId}/repos`),
};
