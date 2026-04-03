<script>
  import './lib/design-system.css';
  import { isLoading, t } from 'svelte-i18n';
  import { createWsStore } from './lib/ws.js';
  import WorkspaceHome from './components/WorkspaceHome.svelte';
  import RepoMode from './components/RepoMode.svelte';
  import WorkspaceSettings from './components/WorkspaceSettings.svelte';
  import MetaSpecs from './components/MetaSpecs.svelte';
  import UserProfile from './components/UserProfile.svelte';
  import CrossWorkspaceHome from './components/CrossWorkspaceHome.svelte';
  import TenantSettings from './components/TenantSettings.svelte';
  import Toast from './lib/Toast.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import { entityName as sharedEntityName, shortId, seedEntityName } from './lib/entityNames.svelte.js';
  import Modal from './lib/Modal.svelte';
  import PresenceAvatars from './lib/PresenceAvatars.svelte';
  import DetailPanel from './lib/DetailPanel.svelte';
  import { onMount, setContext, tick } from 'svelte';
  import { setAuthToken, api } from './lib/api.js';
  import { toast as showToast } from './lib/toast.svelte.js';

  // ── Navigation mode ──────────────────────────────────────────────────
  // 'workspace_home' | 'workspace_settings' | 'repo' | 'profile'
  let mode = $state('workspace_home');

  // ── Workspace / repo state ───────────────────────────────────────────
  let workspaces = $state([]);
  let currentWorkspace = $state(null);
  let currentRepo = $state(null); // { id, name } | null
  let repoTab = $state('specs'); // 'specs' | 'architecture' | 'decisions' | 'code' | 'settings'
  // Cross-workspace sub-page: null = dashboard, 'settings' = /all/settings tenant admin
  let crossWorkspaceTab = $state(null);

  // ── Global detail panel ──────────────────────────────────────────────
  let detailPanel = $state({ open: false, entity: null });
  let detailExpanded = $state(false);

  // ── WebSocket ────────────────────────────────────────────────────────
  let wsStore = $state(null);
  let wsStatus = $state('disconnected');

  $effect(() => {
    const store = createWsStore();
    wsStore = store;
    const unsub = store.onStatus((s) => (wsStatus = s));
    return () => {
      unsub();
      store.destroy();
      wsStore = null;
    };
  });

  // ── UI state ─────────────────────────────────────────────────────────
  let searchOpen = $state(false);
  let shortcutsOpen = $state(false);
  let shortcutsModalEl = $state(null);
  let userMenuOpen = $state(false);
  let userMenuEl = $state(null);
  let wsDropdownOpen = $state(false);
  let wsDropdownEl = $state(null);

  let mobileDrawerOpen = $state(false);
  let createWsModalOpen = $state(false);
  let createWsForm = $state({ name: '', description: '' });
  let createWsSaving = $state(false);
  let tokenModalOpen = $state(false);
  let tokenInput = $state(localStorage.getItem('gyre_auth_token') || 'gyre-dev-token');
  let hasToken = $state(true);
  let tokenInfo = $state(null);
  let tokenVisible = $state(false);

  $effect(() => {
    if (shortcutsOpen && shortcutsModalEl) {
      shortcutsModalEl.querySelector('button')?.focus();
    }
  });

  $effect(() => {
    document.body.style.overflow = shortcutsOpen ? 'hidden' : '';
  });

  $effect(() => {
    if (userMenuOpen) {
      tick().then(() => userMenuEl?.querySelector('[role="menuitem"]')?.focus());
    }
  });

  $effect(() => {
    if (wsDropdownOpen) {
      tick().then(() => wsDropdownEl?.querySelector('[role="menuitem"]')?.focus());
    }
  });

  // Content cross-fade
  let contentVisible = $state(true);
  let fadeTimer = null;

  function fadeContent() {
    contentVisible = false;
    clearTimeout(fadeTimer);
    fadeTimer = setTimeout(() => { contentVisible = true; }, 150);
  }

  // ── Budget / decisions ────────────────────────────────────────────────
  let workspaceBudget = $state(null);
  let decisionsCount = $state(0);

  // ── Repo ID cache ─────────────────────────────────────────────────────
  // Cache repo name→id mappings so browser back/forward can restore full repo state.
  // Key: `${workspaceId}:${repoName}`, Value: repo id string
  const repoIdCache = new Map();

  async function loadWorkspaceData(workspaceId) {
    try { workspaceBudget = await api.workspaceBudget(workspaceId); } catch { workspaceBudget = null; }
  }

  async function loadDecisionsCount() {
    try {
      // Note: spec §1 says repo mode badge should show repo-scoped count only.
      // The server's /count endpoint does not yet support repo_id filtering —
      // it only supports workspace_id. Until the server adds repo_id support,
      // we show the workspace-wide count in all modes. This is tracked as a
      // known limitation of Slice 1.
      decisionsCount = await api.notificationCount(currentWorkspace?.id);
    } catch { /* ignore */ }
  }

  // ── Slug helpers ──────────────────────────────────────────────────────
  /** Get URL slug for a workspace. Uses ws.slug if present, else ws.id. */
  function wsSlug(ws) {
    return ws?.slug ?? ws?.id ?? '';
  }

  /** Find a workspace by slug (slug field OR id). */
  function findWorkspaceBySlug(slug) {
    return workspaces.find(w => w.slug === slug || w.id === slug) ?? null;
  }

  // ── URL structure (§7 of ui-navigation.md) ────────────────────────────
  // /                                          → workspace_home (no workspace selected)
  // /workspaces/:slug                          → workspace home
  // /workspaces/:slug/settings                 → workspace settings
  // /workspaces/:slug/agent-rules              → agent rules
  // /workspaces/:slug/r/:repo                  → repo mode, specs tab
  // /workspaces/:slug/r/:repo/specs            → specs tab
  // /workspaces/:slug/r/:repo/mrs/:id          → MR detail page
  // /workspaces/:slug/r/:repo/tasks/:id        → task detail page
  // /workspaces/:slug/r/:repo/agents/:id       → agent detail page
  // /workspaces/:slug/r/:repo/specs/*path      → spec detail page
  // /profile                                   → user profile

  const REPO_TABS = ['specs', 'tasks', 'mrs', 'agents', 'architecture', 'decisions', 'code', 'settings'];
  const ENTITY_TABS = ['mrs', 'tasks', 'agents'];

  // ── Entity detail state ─────────────────────────────────────────────
  // When set, shows a full-page entity detail view instead of the repo tab content.
  // { type: 'mr'|'task'|'agent'|'spec', id: string }
  let entityDetail = $state(null);

  // Use shared entity name resolution (global singleton cache)
  function resolveEntityName(type, id) {
    return sharedEntityName(type, id);
  }

  export function parseUrl(pathname) {
    const raw = pathname.split('/').filter(Boolean).map(p => {
      try { return decodeURIComponent(p); } catch { return p; }
    });

    if (raw.length === 0) return { mode: 'workspace_home', slug: null, repoName: null, tab: null, entityType: null, entityId: null };

    // /profile
    if (raw.length === 1 && raw[0] === 'profile') {
      return { mode: 'profile', slug: null, repoName: null, tab: null, entityType: null, entityId: null };
    }

    // /all  or  /all/settings  or  /all/agent-rules  (cross-workspace view §10)
    if (raw[0] === 'all') {
      const tab = raw[1] ?? null;
      return { mode: 'cross_workspace', slug: 'all', repoName: null, tab, entityType: null, entityId: null };
    }

    // /workspaces/:slug[/...]
    if (raw[0] === 'workspaces' && raw.length >= 2) {
      const slug = raw[1];

      // /workspaces/:slug/r/:repo[/tab[/entityId]]
      if (raw[2] === 'r' && raw.length >= 4) {
        const repoName = raw[3];
        const tabOrType = raw[4];

        // Entity detail routes: /workspaces/:slug/r/:repo/mrs/:id
        if (raw.length >= 6 && ENTITY_TABS.includes(tabOrType)) {
          const entityType = tabOrType === 'mrs' ? 'mr' : tabOrType === 'tasks' ? 'task' : tabOrType === 'agents' ? 'agent' : tabOrType;
          const entityId = raw[5];
          // Tab is the parent list tab (mrs, tasks, agents)
          return { mode: 'repo', slug, repoName, tab: tabOrType, entityType, entityId };
        }

        // Spec detail route: /workspaces/:slug/r/:repo/specs/system/foo.md (spec paths can have multiple segments)
        if (tabOrType === 'specs' && raw.length >= 6) {
          const specPath = raw.slice(5).join('/');
          return { mode: 'repo', slug, repoName, tab: 'specs', entityType: 'spec', entityId: specPath };
        }

        const tab = tabOrType && REPO_TABS.includes(tabOrType) ? tabOrType : 'specs';
        return { mode: 'repo', slug, repoName, tab, entityType: null, entityId: null };
      }

      // /workspaces/:slug/settings
      if (raw[2] === 'settings') {
        return { mode: 'workspace_settings', slug, repoName: null, tab: null, entityType: null, entityId: null };
      }

      // /workspaces/:slug/agent-rules
      if (raw[2] === 'agent-rules') {
        return { mode: 'agent_rules', slug, repoName: null, tab: null, entityType: null, entityId: null };
      }

      // /workspaces/:slug  or  /workspaces/:slug/...  etc.
      return { mode: 'workspace_home', slug, repoName: null, tab: null, entityType: null, entityId: null };
    }

    return null;
  }

  export function urlFor(parsed) {
    if (!parsed) return '/';
    const { mode: m, slug, repoName, tab, entityType, entityId } = parsed;
    if (m === 'profile') return '/profile';
    if (m === 'cross_workspace') {
      return tab ? `/all/${encodeURIComponent(tab)}` : '/all';
    }
    if (!slug) return '/';
    if (m === 'workspace_home') return `/workspaces/${encodeURIComponent(slug)}`;
    if (m === 'workspace_settings') return `/workspaces/${encodeURIComponent(slug)}/settings`;
    if (m === 'agent_rules') return `/workspaces/${encodeURIComponent(slug)}/agent-rules`;
    if (m === 'repo') {
      const base = `/workspaces/${encodeURIComponent(slug)}/r/${encodeURIComponent(repoName)}`;
      // Entity detail routes
      if (entityType && entityId) {
        const tabSegment = entityType === 'mr' ? 'mrs' : entityType === 'task' ? 'tasks' : entityType === 'agent' ? 'agents' : 'specs';
        if (entityType === 'spec') {
          return `${base}/specs/${entityId}`;
        }
        return `${base}/${tabSegment}/${encodeURIComponent(entityId)}`;
      }
      if (tab && tab !== 'specs') return `${base}/${tab}`;
      return base;
    }
    return '/';
  }

  function pushState(parsed) {
    const stateObj = {
      mode: parsed.mode,
      wsId: currentWorkspace?.id ?? null,
      repoName: parsed.repoName ?? null,
      repoTab: parsed.tab ?? 'specs',
      entityType: parsed.entityType ?? null,
      entityId: parsed.entityId ?? null,
    };
    window.history.pushState(stateObj, '', urlFor(parsed));
  }

  // ── Navigation ────────────────────────────────────────────────────────
  function goToWorkspaceHome(ws) {
    if (ws && ws !== currentWorkspace) {
      currentWorkspace = ws;
      try { localStorage.setItem('gyre_workspace_id', ws.id); } catch { /* private browsing */ }
    }
    mode = 'workspace_home';
    currentRepo = null;
    repoTab = 'specs';
    entityDetail = null;
    fadeContent();
    pushState({ mode: 'workspace_home', slug: wsSlug(currentWorkspace), repoName: null, tab: null });
    if (currentWorkspace) loadWorkspaceData(currentWorkspace.id);
    loadDecisionsCount();
  }

  async function loadRepoDetail(repoId) {
    if (!repoId) return;
    try {
      const detail = await api.repo(repoId);
      if (detail && currentRepo?.id === repoId) {
        currentRepo = { ...currentRepo, ...detail };
      }
    } catch { /* keep partial repo data */ }
  }

  function goToRepo(repo, tab = 'specs') {
    currentRepo = repo;
    // Cache the repo ID for use by popstate restoration
    if (repo.id && currentWorkspace?.id) {
      repoIdCache.set(`${currentWorkspace.id}:${repo.name}`, repo.id);
    }
    mode = 'repo';
    repoTab = tab;
    entityDetail = null;
    fadeContent();
    pushState({ mode: 'repo', slug: wsSlug(currentWorkspace), repoName: repo.name, tab });
    // Load full repo details (clone URL, etc.) in the background
    if (repo.id) loadRepoDetail(repo.id);
  }

  function goToRepoTab(tab) {
    repoTab = tab;
    entityDetail = null; // Clear entity detail when switching tabs
    fadeContent();
    pushState({ mode: 'repo', slug: wsSlug(currentWorkspace), repoName: currentRepo?.name, tab });
  }

  /** Navigate to a full-page entity detail view within repo mode.
   *  Works from any mode — if currentRepo is not set, resolves it from data.repo_id / data.repository_id. */
  async function goToEntityDetail(entityType, entityId, data) {
    if (!currentWorkspace) return;
    const d = data ?? {};
    const parentTab = entityType === 'mr' ? 'mrs' : entityType === 'task' ? 'tasks' : entityType === 'agent' ? 'agents' : 'specs';

    // If we're not in repo mode or the entity belongs to a different repo, resolve context
    const entityRepoId = d.repo_id ?? d.repository_id;
    if (!currentRepo && entityRepoId) {
      // Resolve repo name from the ID so we can build the URL
      try {
        const repos = await api.workspaceRepos(currentWorkspace.id);
        const repo = (repos ?? []).find(r => r.id === entityRepoId);
        if (repo) {
          currentRepo = { id: repo.id, name: repo.name };
          repoIdCache.set(`${currentWorkspace.id}:${repo.name}`, repo.id);
        } else {
          // Fallback: try to get repo directly
          const repoDetail = await api.repo(entityRepoId).catch(() => null);
          if (repoDetail?.name) {
            currentRepo = { id: entityRepoId, name: repoDetail.name };
            repoIdCache.set(`${currentWorkspace.id}:${repoDetail.name}`, entityRepoId);
          }
        }
      } catch { /* best effort */ }
    }
    // If we still don't have repo context, try to resolve from the entity itself
    if (!currentRepo && !entityRepoId) {
      try {
        let resolvedRepoId = null;
        if (entityType === 'task') {
          const taskData = await api.task(entityId).catch(() => null);
          resolvedRepoId = taskData?.repo_id;
        } else if (entityType === 'agent') {
          const agentData = await api.agent(entityId).catch(() => null);
          resolvedRepoId = agentData?.repo_id;
        } else if (entityType === 'mr') {
          const mrData = await api.mergeRequest(entityId).catch(() => null);
          resolvedRepoId = mrData?.repository_id ?? mrData?.repo_id;
        }
        if (resolvedRepoId) {
          const repos = await api.workspaceRepos(currentWorkspace.id);
          const repo = (Array.isArray(repos) ? repos : []).find(r => r.id === resolvedRepoId);
          if (repo) {
            currentRepo = { id: repo.id, name: repo.name };
            repoIdCache.set(`${currentWorkspace.id}:${repo.name}`, repo.id);
            d.repo_id = repo.id;
          }
        }
      } catch { /* best effort */ }
    }
    if (!currentRepo) return; // Can't navigate without repo context

    mode = 'repo';
    repoTab = parentTab;
    entityDetail = { type: entityType, id: entityId, data: d };
    // Pre-cache entity name for breadcrumb from provided data
    const name = d.title ?? d.name;
    if (name && entityType !== 'spec') {
      seedEntityName(entityType, entityId, name);
    }
    fadeContent();
    const parsed = {
      mode: 'repo',
      slug: wsSlug(currentWorkspace),
      repoName: currentRepo.name,
      tab: parentTab,
      entityType,
      entityId,
    };
    const stateObj = {
      mode: 'repo',
      wsId: currentWorkspace.id,
      repoName: currentRepo.name,
      repoTab: parentTab,
      entityType,
      entityId,
      entityTitle: name ?? null,
      entityRepoId: currentRepo?.id ?? entityRepoId ?? null,
      previousMode: mode === 'workspace_home' ? 'workspace_home' : null,
    };
    window.history.pushState(stateObj, '', urlFor(parsed));
  }

  function goToWorkspaceSettings() {
    if (!currentWorkspace) return;
    mode = 'workspace_settings';
    currentRepo = null;
    repoTab = 'specs';
    fadeContent();
    pushState({ mode: 'workspace_settings', slug: wsSlug(currentWorkspace), repoName: null, tab: null });
  }

  function goToAgentRules() {
    if (!currentWorkspace) return;
    mode = 'agent_rules';
    currentRepo = null;
    repoTab = 'specs';
    fadeContent();
    pushState({ mode: 'agent_rules', slug: wsSlug(currentWorkspace), repoName: null, tab: null });
  }

  function goToProfile() {
    mode = 'profile';
    fadeContent();
    pushState({ mode: 'profile', slug: null, repoName: null, tab: null });
  }

  function goToCrossWorkspace() {
    mode = 'cross_workspace';
    crossWorkspaceTab = null;
    currentRepo = null;
    repoTab = 'specs';
    fadeContent();
    window.history.pushState(
      { mode: 'cross_workspace', wsId: null, repoName: null, repoTab: null },
      '',
      '/all'
    );
  }

  function goToTenantSettings() {
    mode = 'cross_workspace';
    crossWorkspaceTab = 'settings';
    currentRepo = null;
    repoTab = 'specs';
    fadeContent();
    window.history.pushState(
      { mode: 'cross_workspace', crossWorkspaceTab: 'settings', wsId: null, repoName: null, repoTab: null },
      '',
      '/all/settings'
    );
  }

  function goToTenantAgentRules() {
    mode = 'cross_workspace';
    crossWorkspaceTab = 'agent-rules';
    currentRepo = null;
    repoTab = 'specs';
    fadeContent();
    window.history.pushState(
      { mode: 'cross_workspace', crossWorkspaceTab: 'agent-rules', wsId: null, repoName: null, repoTab: null },
      '',
      '/all/agent-rules'
    );
  }

  function selectWorkspace(ws) {
    wsDropdownOpen = false;
    if (ws === 'all') {
      goToCrossWorkspace();
      return;
    }
    goToWorkspaceHome(ws);
  }

  // Expose via context
  setContext('navigate', (view) => {
    // Legacy compat shim: map old nav items to new navigation
    if (view === 'profile') { goToProfile(); return; }
    // In repo mode, tab names switch the active tab instead of navigating away
    if (mode === 'repo' && REPO_TABS.includes(view)) { goToRepoTab(view); return; }
    // Everything else lands on workspace home
    goToWorkspaceHome(currentWorkspace);
  });
  setContext('getScope', () => ({
    type: mode === 'repo' ? 'repo' : mode === 'workspace_home' ? 'workspace' : 'tenant',
    workspaceId: currentWorkspace?.id,
    repoId: currentRepo?.id,
  }));
  setContext('openDetailPanel', openDetailPanel);
  setContext('goBackDetailPanel', goBackDetailPanel);
  setContext('goToEntityDetail', goToEntityDetail);
  setContext('goToAgentRules', () => goToAgentRules());
  setContext('goToWorkspaceSettings', () => goToWorkspaceSettings());
  setContext('goToWorkspaceHome', (ws) => goToWorkspaceHome(ws ?? currentWorkspace));
  setContext('goToRepoTab', (tab, params) => {
    if (mode !== 'repo') return;
    if (params) {
      const url = new URL(window.location.href);
      for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
      window.history.replaceState(window.history.state, '', url.toString());
    }
    goToRepoTab(tab);
  });

  // ── Detail panel (with navigation history stack) ──────────────────────
  let detailHistory = $state([]);

  function openDetailPanel(entity) {
    // Primary entity types get full-page views instead of the side panel.
    // The side panel is reserved for lightweight previews (graph nodes, commits).
    const primaryTypes = ['spec', 'task', 'mr', 'agent'];
    if (entity?.type && primaryTypes.includes(entity.type)) {
      goToEntityDetail(entity.type, entity.id, entity.data ?? {});
      return;
    }
    // Push current entity to history stack before navigating
    if (detailPanel.open && detailPanel.entity) {
      detailHistory = [...detailHistory, detailPanel.entity];
    } else {
      // First panel open — push a browser history entry so back button closes panel
      window.history.pushState(
        { ...window.history.state, detailOpen: true },
        '',
        window.location.href,
      );
    }
    detailPanel = { open: true, entity };
  }

  function closeDetailPanel() {
    detailPanel = { open: false, entity: null };
    detailExpanded = false;
    detailHistory = [];
  }

  function goBackDetailPanel() {
    if (detailHistory.length > 0) {
      const prev = detailHistory[detailHistory.length - 1];
      detailHistory = detailHistory.slice(0, -1);
      detailPanel = { open: true, entity: prev };
    } else {
      closeDetailPanel();
    }
  }

  // ── Workspace dropdown keyboard navigation ────────────────────────────
  function onWsDropdownKeydown(e) {
    const items = wsDropdownEl?.querySelectorAll('[role="menuitem"]');
    if (!items?.length) return;
    const arr = Array.from(items);
    const current = arr.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') { e.preventDefault(); arr[(current + 1) % arr.length]?.focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); arr[(current - 1 + arr.length) % arr.length]?.focus(); }
    else if (e.key === 'Escape') { e.preventDefault(); wsDropdownOpen = false; document.querySelector('.ws-name-btn')?.focus(); }
    else if (e.key === 'Home') { e.preventDefault(); arr[0]?.focus(); }
    else if (e.key === 'End') { e.preventDefault(); arr[arr.length - 1]?.focus(); }
  }

  // ── User menu keyboard navigation ────────────────────────────────────
  function onUserMenuKeydown(e) {
    const items = userMenuEl?.querySelectorAll('[role="menuitem"]');
    if (!items?.length) return;
    const arr = Array.from(items);
    const current = arr.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') { e.preventDefault(); arr[(current + 1) % arr.length]?.focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); arr[(current - 1 + arr.length) % arr.length]?.focus(); }
    else if (e.key === 'Escape') { e.preventDefault(); userMenuOpen = false; document.querySelector('.user-btn')?.focus(); }
    else if (e.key === 'Home') { e.preventDefault(); arr[0]?.focus(); }
    else if (e.key === 'End') { e.preventDefault(); arr[arr.length - 1]?.focus(); }
  }

  // ── Keyboard shortcuts (§6 of ui-navigation.md) ──────────────────────
  // g-key sequence: press g, then within 500ms press the second key
  let gKeyPending = false;
  let gKeyTimer = null;

  function handleKeydown(e) {
    const inInput = e.target.tagName === 'INPUT'
      || e.target.tagName === 'TEXTAREA'
      || e.target.isContentEditable;

    // ⌘K: global search
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      searchOpen = true;
      gKeyPending = false;
      return;
    }

    // Esc: close overlay / panel / return to workspace home
    if (e.key === 'Escape') {
      if (shortcutsOpen) { shortcutsOpen = false; gKeyPending = false; return; }
      if (mobileDrawerOpen) { mobileDrawerOpen = false; gKeyPending = false; return; }
      if (userMenuOpen) { userMenuOpen = false; gKeyPending = false; return; }
      if (wsDropdownOpen) { wsDropdownOpen = false; gKeyPending = false; return; }
      if (detailPanel.open) { closeDetailPanel(); gKeyPending = false; return; }
      if (mode === 'repo' && entityDetail) { window.history.back(); gKeyPending = false; return; }
      if (mode === 'repo') { goToWorkspaceHome(currentWorkspace); gKeyPending = false; return; }
      if (mode === 'workspace_settings' || mode === 'agent_rules') { window.history.back(); gKeyPending = false; return; }
      if (mode === 'cross_workspace' && crossWorkspaceTab) { window.history.back(); gKeyPending = false; return; }
      gKeyPending = false;
      return;
    }

    // /: focus search (suppressed in text inputs and when already open)
    if (e.key === '/' && !inInput && !searchOpen) {
      e.preventDefault();
      searchOpen = true;
      gKeyPending = false;
      return;
    }

    // ?: keyboard shortcut overlay (suppressed in text inputs)
    if (e.key === '?' && !inInput) {
      e.preventDefault();
      shortcutsOpen = !shortcutsOpen;
      gKeyPending = false;
      return;
    }

    // g-key sequence (suppressed in text inputs)
    if (!inInput) {
      if (e.key === 'g' && !e.metaKey && !e.ctrlKey && !e.altKey) {
        gKeyPending = true;
        clearTimeout(gKeyTimer);
        gKeyTimer = setTimeout(() => { gKeyPending = false; }, 500);
        return;
      }

      if (gKeyPending) {
        gKeyPending = false;
        clearTimeout(gKeyTimer);

        switch (e.key) {
          case 'h': // g h → workspace home
            e.preventDefault();
            goToWorkspaceHome(currentWorkspace);
            return;
          case 's': // g s → workspace settings
            e.preventDefault();
            goToWorkspaceSettings();
            return;
          case 'a': // g a → agent rules
            e.preventDefault();
            goToAgentRules();
            return;
          case '1': // g 1 → Specs tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('specs');
            return;
          case '2': // g 2 → Tasks tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('tasks');
            return;
          case '3': // g 3 → MRs tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('mrs');
            return;
          case '4': // g 4 → Agents tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('agents');
            return;
          case '5': // g 5 → Architecture tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('architecture');
            return;
          case '6': // g 6 → Decisions tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('decisions');
            return;
          case '7': // g 7 → Code tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('code');
            return;
          case '8': // g 8 → Settings tab (repo mode only)
            e.preventDefault();
            if (mode === 'repo') goToRepoTab('settings');
            return;
        }
      }
    }
  }

  // ── User role ─────────────────────────────────────────────────────────
  // Track whether the current user is a tenant admin (for gear icon visibility).
  // Loaded once on mount; false by default (fail closed for security).
  let userIsAdmin = $state(false);

  // ── Token modal ───────────────────────────────────────────────────────
  const TOKEN_KIND_LABELS = {
    global:     'Global admin token',
    agent_jwt:  'Agent JWT (EdDSA, scoped)',
    uuid_token: 'Per-agent UUID token (legacy)',
    api_key:    'API key',
  };

  async function openTokenModal() {
    tokenInput = localStorage.getItem('gyre_auth_token') || 'gyre-dev-token';
    tokenModalOpen = true;
    tokenVisible = false;
    tokenInfo = null;
    try { tokenInfo = await api.tokenInfo(); } catch { /* ignore */ }
  }

  async function handleCreateWorkspaceFromDropdown() {
    const name = createWsForm.name.trim();
    if (!name) return;
    createWsSaving = true;
    try {
      const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
      const newWs = await api.createWorkspace({ ...createWsForm, name, tenant_id: 'default', slug });
      showToast(`Workspace "${name}" created.`, { type: 'success' });
      createWsModalOpen = false;
      createWsForm = { name: '', description: '' };
      // Refresh workspace list and navigate
      try { workspaces = await api.workspaces() ?? []; } catch { /* keep existing */ }
      const ws = workspaces.find(w => w.id === newWs?.id) ?? newWs;
      if (ws) goToWorkspaceHome(ws);
    } catch (e) {
      showToast('Failed to create workspace: ' + (e.message || e), { type: 'error' });
    } finally {
      createWsSaving = false;
    }
  }

  function saveToken() {
    const t = tokenInput.trim() || 'gyre-dev-token';
    setAuthToken(t);
    tokenInput = t;
    hasToken = true;
    tokenModalOpen = false;
    tokenInfo = null;
  }

  // ── Computed ──────────────────────────────────────────────────────────
  let budgetPct = $derived.by(() => {
    if (!workspaceBudget) return null;
    const used = workspaceBudget.used_credits ?? 0;
    const total = workspaceBudget.total_credits ?? 0;
    if (!total) return null;
    return Math.round((used / total) * 100);
  });

  let trustLevel = $derived(currentWorkspace?.trust_level ?? null);

  // ── Page title ────────────────────────────────────────────────────────
  $effect(() => {
    const wsName = currentWorkspace?.name ?? '';
    const repoName = currentRepo?.name ?? '';
    if (mode === 'profile') {
      document.title = 'Profile | Gyre';
    } else if (mode === 'cross_workspace') {
      document.title = 'All Workspaces | Gyre';
    } else if (mode === 'agent_rules') {
      document.title = wsName ? `Agent Rules — ${wsName} | Gyre` : 'Agent Rules | Gyre';
    } else if (mode === 'repo' && repoName) {
      document.title = wsName ? `${repoName} — ${wsName} | Gyre` : `${repoName} | Gyre`;
    } else if (mode === 'workspace_home' && wsName) {
      document.title = `${wsName} | Gyre`;
    } else {
      document.title = 'Gyre';
    }
  });

  // ── Legacy URL redirect handler (§7 of ui-navigation.md) ────────────────
  // Returns a new URL string if redirect needed, otherwise null.
  function handleLegacyUrl(pathname) {
    const raw = pathname.split('/').filter(Boolean);

    // /workspaces/:uuid/inbox → /workspaces/:slug (workspace home)
    if (raw[0] === 'workspaces' && raw[2] === 'inbox' && raw.length === 3) {
      const slug = raw[1];
      // Try to find the workspace and get its slug
      const ws = findWorkspaceBySlug(slug);
      const target = ws ? wsSlug(ws) : slug;
      return `/workspaces/${encodeURIComponent(target)}`;
    }

    // /repos/:uuid/explorer → redirect to /all (best-effort; repo UUID lookup not available client-side)
    // The full redirect (to specific repo's architecture tab) requires a server-side lookup.
    // Client-side best-effort: navigate to cross-workspace view.
    if (raw[0] === 'repos' && raw[2] === 'explorer' && raw.length === 3) {
      return '/all';
    }

    return null;
  }

  // ── Mount: entrypoint flow + URL routing ──────────────────────────────
  onMount(async () => {
    // 1. Load all workspaces
    try { workspaces = await api.workspaces() ?? []; } catch { workspaces = []; }

    // 2. Determine initial state from URL or entrypoint flow
    const parsed = parseUrl(window.location.pathname);

    // Legacy URL redirects (§7 of ui-navigation.md)
    // Old HSI-style URLs are redirected to new structure.
    const legacyRedirect = handleLegacyUrl(window.location.pathname);
    if (legacyRedirect) {
      window.history.replaceState({}, '', legacyRedirect);
      // Re-parse with the new URL
    }

    if (parsed?.mode === 'profile') {
      mode = 'profile';
    } else if (parsed?.mode === 'cross_workspace') {
      mode = 'cross_workspace';
      crossWorkspaceTab = (parsed.tab === 'settings' || parsed.tab === 'agent-rules') ? parsed.tab : null;
    } else if (parsed?.slug) {
      // URL-driven workspace navigation
      const ws = findWorkspaceBySlug(parsed.slug);
      if (ws) {
        currentWorkspace = ws;
        try { localStorage.setItem('gyre_workspace_id', ws.id); } catch { /* private browsing */ }
        loadWorkspaceData(ws.id);

        if (parsed.mode === 'repo' && parsed.repoName) {
          mode = 'repo';
          repoTab = parsed.tab ?? 'specs';
          currentRepo = { id: null, name: parsed.repoName };
          // Restore entity detail from URL
          if (parsed.entityType && parsed.entityId) {
            entityDetail = { type: parsed.entityType, id: parsed.entityId, data: {} };
          }
          // Try to resolve repo ID and populate cache
          try {
            const repos = await api.workspaceRepos(ws.id);
            const repo = (repos ?? []).find(r => r.name === parsed.repoName);
            if (repo) {
              currentRepo = { id: repo.id, name: repo.name };
              repoIdCache.set(`${ws.id}:${repo.name}`, repo.id);
              // Enrich entity detail with repo_id
              if (entityDetail) {
                entityDetail = { ...entityDetail, data: { ...entityDetail.data, repo_id: repo.id } };
              }
            }
          } catch { /* keep name-only ref */ }
        } else if (parsed.mode === 'workspace_settings') {
          mode = 'workspace_settings';
        } else if (parsed.mode === 'agent_rules') {
          mode = 'agent_rules';
        } else {
          mode = 'workspace_home';
        }
      } else {
        showToast('Workspace not found — redirecting to home', { type: 'info' });
        mode = 'workspace_home';
        currentWorkspace = null;
      }
    } else {
      // Entrypoint flow: always show workspace home
      const savedWsId = localStorage.getItem('gyre_workspace_id');
      if (savedWsId) {
        const ws = workspaces.find(w => w.id === savedWsId);
        if (ws) {
          currentWorkspace = ws;
          mode = 'workspace_home';
          loadWorkspaceData(ws.id);
        } else {
          localStorage.removeItem('gyre_workspace_id');
          // Fall through: auto-select if only one workspace exists
          if (workspaces.length === 1) {
            currentWorkspace = workspaces[0];
            try { localStorage.setItem('gyre_workspace_id', currentWorkspace.id); } catch { /* private browsing */ }
            mode = 'workspace_home';
            loadWorkspaceData(currentWorkspace.id);
          } else {
            mode = 'workspace_home';
            currentWorkspace = null;
          }
        }
      } else if (workspaces.length === 1) {
        // Spec §5: "workspace selector (if multiple) or workspace home (if one)"
        currentWorkspace = workspaces[0];
        try { localStorage.setItem('gyre_workspace_id', currentWorkspace.id); } catch { /* private browsing */ }
        mode = 'workspace_home';
        loadWorkspaceData(currentWorkspace.id);
      } else {
        mode = 'workspace_home';
        currentWorkspace = null;
      }
    }

    // Replace history state with canonical URL
    const canon = urlFor({
      mode,
      slug: wsSlug(currentWorkspace),
      repoName: currentRepo?.name ?? null,
      tab: mode === 'cross_workspace' ? crossWorkspaceTab : repoTab,
      entityType: entityDetail?.type ?? null,
      entityId: entityDetail?.id ?? null,
    });
    window.history.replaceState(
      { mode, wsId: currentWorkspace?.id ?? null, repoName: currentRepo?.name ?? null, repoTab, crossWorkspaceTab, entityType: entityDetail?.type ?? null, entityId: entityDetail?.id ?? null },
      '',
      canon
    );

    // 3. Load user role (for tenant admin gear icon) + decisions count
    try {
      const me = await api.me();
      userIsAdmin = me?.global_role === 'Admin' || me?.role === 'Admin' || me?.is_admin === true;
    } catch { /* fail closed — gear icon stays hidden */ }
    loadDecisionsCount();
    const decisionsInterval = setInterval(loadDecisionsCount, 60_000);

    // 4. Popstate (browser back/forward)
    function handlePopstate(e) {
      // Close detail panel on back navigation so back button always takes user
      // one step back in their journey. Since we push a history entry when the
      // panel opens, going back naturally pops it — no need to re-push.
      if (detailPanel.open) {
        if (detailHistory.length > 0) {
          // Navigate back within the detail panel history stack.
          // Replace (not push) so back button doesn't grow history infinitely.
          goBackDetailPanel();
          window.history.replaceState(
            { ...e.state, detailOpen: true },
            '',
            window.location.href,
          );
        } else {
          closeDetailPanel();
        }
        return;
      }
      if (e.state?.mode) {
        const { mode: m, wsId, repoName, repoTab: rt, crossWorkspaceTab: cwt, entityType: et, entityId: eid, entityTitle: eTitle, entityRepoId: eRepoId } = e.state;
        mode = (m === 'workspace_settings' || m === 'workspace_home' || m === 'repo' || m === 'profile' || m === 'cross_workspace' || m === 'agent_rules')
          ? m : 'workspace_home';
        repoTab = rt ?? 'specs';
        crossWorkspaceTab = m === 'cross_workspace' ? (cwt ?? null) : null;
        // Restore entity detail state from history with repo_id for data loading
        entityDetail = (et && eid) ? { type: et, id: eid, data: eRepoId ? { repo_id: eRepoId } : {} } : null;
        if (et && eid && eTitle) {
          seedEntityName(et, eid, eTitle);
        }
        if (wsId) {
          currentWorkspace = workspaces.find(w => w.id === wsId) ?? currentWorkspace;
        } else if (m !== 'cross_workspace' && m !== 'profile') {
          // Try to recover workspace from URL slug if state doesn't have wsId
          const p2 = parseUrl(window.location.pathname);
          if (p2?.slug) {
            const ws = findWorkspaceBySlug(p2.slug);
            if (ws) currentWorkspace = ws;
          }
        }
        if (repoName && m === 'repo') {
          // Restore repo ID from cache if available
          const cwsId = currentWorkspace?.id;
          const cachedId = cwsId ? repoIdCache.get(`${cwsId}:${repoName}`) : null;
          currentRepo = { id: cachedId ?? null, name: repoName };
          // If cache miss, resolve repo ID from API so tabs load properly
          if (!cachedId && cwsId) {
            api.repos({ workspaceId: cwsId }).then(repos => {
              const found = (Array.isArray(repos) ? repos : []).find(r => r.name === repoName);
              if (found) {
                repoIdCache.set(`${cwsId}:${repoName}`, found.id);
                currentRepo = { id: found.id, name: repoName };
              }
            }).catch(() => {});
          }
        } else {
          currentRepo = null;
        }
        fadeContent();
        if (currentWorkspace) loadWorkspaceData(currentWorkspace.id);
        loadDecisionsCount();
        return;
      }
      // Fallback: re-parse URL
      const p = parseUrl(window.location.pathname);
      if (p) {
        if (p.mode === 'profile') {
          mode = 'profile';
        } else if (p.mode === 'cross_workspace') {
          mode = 'cross_workspace';
          crossWorkspaceTab = (p.tab === 'settings' || p.tab === 'agent-rules') ? p.tab : null;
        } else if (p.slug) {
          const ws = findWorkspaceBySlug(p.slug);
          if (ws) currentWorkspace = ws;
          mode = (p.mode === 'workspace_settings' || p.mode === 'workspace_home' || p.mode === 'repo' || p.mode === 'agent_rules')
            ? p.mode
            : 'workspace_home';
          if (p.repoName && p.mode === 'repo') {
            const cwsId2 = currentWorkspace?.id;
            const cachedId2 = cwsId2 ? repoIdCache.get(`${cwsId2}:${p.repoName}`) : null;
            currentRepo = { id: cachedId2 ?? null, name: p.repoName };
            // If cache miss, resolve repo ID from API so tabs load properly
            if (!cachedId2 && cwsId2) {
              api.repos({ workspaceId: cwsId2 }).then(repos => {
                const found = (Array.isArray(repos) ? repos : []).find(r => r.name === p.repoName);
                if (found) {
                  repoIdCache.set(`${cwsId2}:${p.repoName}`, found.id);
                  currentRepo = { id: found.id, name: p.repoName };
                }
              }).catch(() => {});
            }
          } else {
            currentRepo = null;
          }
          repoTab = p.tab ?? 'specs';
          entityDetail = (p.entityType && p.entityId) ? { type: p.entityType, id: p.entityId, data: currentRepo?.id ? { repo_id: currentRepo.id } : {} } : null;
        }
        fadeContent();
      }
    }

    window.addEventListener('popstate', handlePopstate);
    window.addEventListener('keydown', handleKeydown);

    function handleOutsideClick(e) {
      if (!e.target.closest('.user-menu-wrap')) userMenuOpen = false;
      if (!e.target.closest('.ws-selector')) wsDropdownOpen = false;
    }
    window.addEventListener('click', handleOutsideClick, true);

    return () => {
      window.removeEventListener('popstate', handlePopstate);
      window.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('click', handleOutsideClick, true);
      clearInterval(decisionsInterval);
      clearTimeout(fadeTimer);
      clearTimeout(gKeyTimer);
    };
  });
</script>

<a href="#main-content" class="skip-to-content">{$t('common.skip_to_content')}</a>

{#if !$isLoading}
<div class="app">
  <!-- Main column: topbar + content + status bar (no sidebar) -->
  <div class="main">

    <!-- ── Topbar (always visible) ──────────────────────────────────── -->
    <header class="topbar" data-testid="topbar">

      <!-- Mobile hamburger (visible only on mobile) -->
      <button
        class="hamburger-btn"
        onclick={() => (mobileDrawerOpen = !mobileDrawerOpen)}
        aria-label={mobileDrawerOpen ? 'Close navigation drawer' : 'Open navigation drawer'}
        aria-expanded={mobileDrawerOpen}
        aria-controls="mobile-drawer"
        data-testid="hamburger-btn"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18" aria-hidden="true">
          {#if mobileDrawerOpen}
            <path d="M18 6L6 18M6 6l12 12"/>
          {:else}
            <path d="M3 12h18M3 6h18M3 18h18"/>
          {/if}
        </svg>
      </button>

      <!-- Left side: workspace selector or back arrow + breadcrumb -->
      {#if mode === 'agent_rules' || mode === 'workspace_settings'}
        <!-- Sub-page mode: back arrow + WorkspaceName / Page Name -->
        <div class="topbar-left repo-context">
          <button
            class="back-btn"
            onclick={() => window.history.back()}
            aria-label={$t('topbar.back_to_workspace')}
            data-testid="back-btn"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
              <path d="M19 12H5M12 5l-7 7 7 7"/>
            </svg>
          </button>
          <nav class="breadcrumb" aria-label={$t('topbar.location')}>
            <button
              class="breadcrumb-ws"
              onclick={() => goToWorkspaceHome(currentWorkspace)}
              aria-label={$t('topbar.go_to_workspace_home', { values: { name: currentWorkspace?.name ?? 'workspace' } })}
            >
              {currentWorkspace?.name ?? 'Workspace'}
            </button>
            <span class="breadcrumb-sep" aria-hidden="true">/</span>
            <span class="breadcrumb-repo" aria-current="page">{mode === 'workspace_settings' ? $t('topbar.workspace_settings') : $t('topbar.agent_rules_label')}</span>
          </nav>
        </div>
      {:else if mode === 'repo'}
        <!-- Repo mode: back arrow + WorkspaceName / RepoName [/ EntityType / EntityName] -->
        <div class="topbar-left repo-context">
          <button
            class="back-btn"
            onclick={() => {
              if (entityDetail) {
                // Use browser back to retrace the user's actual journey.
                // This correctly handles MR → Agent → Task chains where
                // the user should go back to the previous entity, not the
                // parent list tab.
                window.history.back();
              } else {
                goToWorkspaceHome(currentWorkspace);
              }
            }}
            aria-label={entityDetail ? 'Back to list' : $t('topbar.back_to_workspace')}
            data-testid="back-btn"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
              <path d="M19 12H5M12 5l-7 7 7 7"/>
            </svg>
          </button>
          <nav class="breadcrumb" aria-label={$t('topbar.location')} data-testid="repo-breadcrumb">
            <button
              class="breadcrumb-ws"
              onclick={() => goToWorkspaceHome(currentWorkspace)}
              aria-label={$t('topbar.go_to_workspace_home', { values: { name: currentWorkspace?.name ?? 'workspace' } })}
            >
              {currentWorkspace?.name ?? 'Workspace'}
            </button>
            <span class="breadcrumb-sep" aria-hidden="true">/</span>
            {#if entityDetail}
              {@const parentTab = entityDetail.type === 'mr' ? 'mrs' : entityDetail.type === 'task' ? 'tasks' : entityDetail.type === 'agent' ? 'agents' : entityDetail.type === 'spec' ? 'specs' : repoTab}
              {@const parentTabLabel = parentTab === 'mrs' ? 'Merge Requests' : parentTab === 'tasks' ? 'Tasks' : parentTab === 'agents' ? 'Agents' : parentTab === 'specs' ? 'Specs' : parentTab}
              <button
                class="breadcrumb-ws"
                onclick={() => goToRepoTab(parentTab)}
                aria-label="Back to {currentRepo?.name ?? 'repo'}"
              >
                {currentRepo?.name ?? ''}
              </button>
              <span class="breadcrumb-sep" aria-hidden="true">/</span>
              <button
                class="breadcrumb-ws"
                onclick={() => goToRepoTab(parentTab)}
              >
                {parentTabLabel}
              </button>
              <span class="breadcrumb-sep" aria-hidden="true">/</span>
              <span class="breadcrumb-repo" aria-current="page" title={entityDetail.id}>
                {resolveEntityName(entityDetail.type, entityDetail.id)}
              </span>
            {:else}
              <span class="breadcrumb-repo" aria-current="page">{currentRepo?.name ?? ''}</span>
            {/if}
          </nav>
        </div>
      {:else}
        <!-- Workspace home / profile: workspace selector -->
        <div class="topbar-left ws-selector" data-testid="ws-selector">
          <div class="logo-mark" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" width="20" height="20">
              <circle cx="12" cy="12" r="10" stroke="var(--color-primary)" stroke-width="2"/>
              <path d="M8 12l3 3 5-5" stroke="var(--color-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </div>

          {#if currentWorkspace}
            <div class="ws-name-wrap">
              <button
                class="ws-name-btn"
                onclick={() => goToWorkspaceHome(currentWorkspace)}
                aria-label={$t('topbar.go_to_workspace_home', { values: { name: currentWorkspace.name } })}
                data-testid="ws-name-btn"
              >
                {currentWorkspace.name}
              </button>
              <button
                class="ws-arrow-btn"
                onclick={() => (wsDropdownOpen = !wsDropdownOpen)}
                aria-haspopup="menu"
                aria-expanded={wsDropdownOpen}
                aria-label={$t('topbar.switch_workspace')}
                data-testid="ws-dropdown-toggle"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true">
                  <path d="M6 9l6 6 6-6"/>
                </svg>
              </button>
              <button
                class="ws-gear-btn"
                onclick={() => goToWorkspaceSettings()}
                aria-label={$t('topbar.workspace_settings')}
                title={$t('topbar.workspace_settings') + ' (g s)'}
                data-testid="ws-gear-btn"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true">
                  <circle cx="12" cy="12" r="3"/>
                  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
                </svg>
              </button>
            </div>
          {:else if mode === 'cross_workspace' && crossWorkspaceTab}
            <!-- Cross-workspace sub-page: back arrow + All Workspaces / Sub-page -->
            <div class="ws-name-wrap">
              <button
                class="back-btn"
                onclick={() => window.history.back()}
                aria-label="Back to All Workspaces"
                data-testid="back-btn"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                  <path d="M19 12H5M12 5l-7 7 7 7"/>
                </svg>
              </button>
              <nav class="breadcrumb" aria-label={$t('topbar.location')}>
                <button class="breadcrumb-ws" onclick={() => goToCrossWorkspace()}>{$t('topbar.all_workspaces')}</button>
                <span class="breadcrumb-sep" aria-hidden="true">/</span>
                <span class="breadcrumb-repo" aria-current="page">{crossWorkspaceTab === 'settings' ? $t('topbar.tenant_settings') : $t('topbar.agent_rules_label')}</span>
              </nav>
            </div>
          {:else if mode === 'cross_workspace'}
            <div class="ws-name-wrap">
              <button
                class="ws-name-btn"
                onclick={() => (wsDropdownOpen = !wsDropdownOpen)}
                aria-haspopup="menu"
                aria-expanded={wsDropdownOpen}
                aria-label="{$t('topbar.all_workspaces')} — {$t('topbar.switch_workspace')}"
                data-testid="ws-all-workspaces-btn"
              >
                {$t('topbar.all_workspaces')}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true">
                  <path d="M6 9l6 6 6-6"/>
                </svg>
              </button>
              {#if userIsAdmin}
                <button
                  class="ws-gear-btn"
                  onclick={() => goToTenantSettings()}
                  aria-label={$t('topbar.tenant_settings')}
                  title={$t('topbar.tenant_settings')}
                  data-testid="all-settings-gear-btn"
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true">
                    <circle cx="12" cy="12" r="3"/>
                    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
                  </svg>
                </button>
              {/if}
            </div>
          {:else}
            <button
              class="ws-name-btn ws-name-empty"
              onclick={() => (wsDropdownOpen = !wsDropdownOpen)}
              aria-haspopup="menu"
              aria-expanded={wsDropdownOpen}
              aria-label="{$t('topbar.select_workspace')}"
              data-testid="ws-select-prompt"
            >
              {$t('topbar.select_workspace')}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true">
                <path d="M6 9l6 6 6-6"/>
              </svg>
            </button>
          {/if}

          <!-- Workspace dropdown -->
          {#if wsDropdownOpen}
            <div
              class="ws-dropdown"
              role="menu"
              tabindex="-1"
              aria-label={$t('topbar.all_workspaces')}
              bind:this={wsDropdownEl}
              onkeydown={onWsDropdownKeydown}
              onfocusout={(e) => { if (!e.currentTarget.contains(e.relatedTarget)) wsDropdownOpen = false; }}
              data-testid="ws-dropdown"
            >
              <!-- "All Workspaces" entry (cross-workspace view, §10) -->
              <button
                class="ws-dropdown-item ws-dropdown-all"
                class:active={mode === 'cross_workspace'}
                role="menuitem"
                tabindex="-1"
                onclick={() => selectWorkspace('all')}
                data-testid="ws-all-workspaces"
              >
                <span class="ws-all-icon" aria-hidden="true">◎</span>
                {$t('topbar.all_workspaces')}
                {#if mode === 'cross_workspace'}
                  <span class="ws-check" aria-hidden="true">✓</span>
                {/if}
              </button>
              <div class="ws-dropdown-divider" role="separator"></div>
              {#if workspaces.length === 0}
                <div class="ws-dropdown-empty">{$t('workspace_home.select_workspace_desc')}</div>
              {:else}
                {#each workspaces as ws}
                  <button
                    class="ws-dropdown-item"
                    class:active={ws.id === currentWorkspace?.id}
                    role="menuitem"
                    tabindex="-1"
                    onclick={() => selectWorkspace(ws)}
                  >
                    {ws.name}
                    {#if ws.id === currentWorkspace?.id}
                      <span class="ws-check" aria-hidden="true">✓</span>
                    {/if}
                  </button>
                {/each}
              {/if}
              <div class="ws-dropdown-divider" role="separator"></div>
              <button
                class="ws-dropdown-item ws-dropdown-create"
                role="menuitem"
                tabindex="-1"
                onclick={() => { wsDropdownOpen = false; createWsForm = { name: '', description: '' }; createWsModalOpen = true; }}
                data-testid="ws-dropdown-create"
              >
                {$t('workspace_home.new_workspace')}
              </button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Center: search trigger -->
      <div class="topbar-center">
        <button
          class="search-trigger"
          onclick={() => (searchOpen = true)}
          aria-label={$t('topbar.open_search')}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
          </svg>
          <span>{$t('topbar.search')}</span>
          <kbd aria-hidden="true">{$t('topbar.search_shortcut')}</kbd>
        </button>
      </div>

      <!-- Right: decisions badge + user avatar -->
      <div class="topbar-right">
        <!-- Decisions badge (🔔) -->
        <button
          class="decisions-badge-btn"
          onclick={async () => {
            if (mode === 'repo') {
              goToRepoTab('decisions');
            } else {
              // Ensure workspace home is rendered before attempting scroll
              if (mode !== 'workspace_home') {
                goToWorkspaceHome(currentWorkspace);
                await tick();
              }
              document.querySelector('[data-testid="section-decisions"]')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }
          }}
          aria-label={decisionsCount > 0 ? $t('topbar.decisions_pending', { values: { count: decisionsCount } }) : $t('topbar.no_decisions')}
          title={$t('workspace_home.sections.decisions')}
          data-testid="decisions-badge"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true">
            <path d="M18 8A6 6 0 006 8c0 7-3 9-3 9h18s-3-2-3-9"/>
            <path d="M13.73 21a2 2 0 01-3.46 0"/>
          </svg>
          {#if decisionsCount > 0}
            <span class="decisions-count" aria-hidden="true">{decisionsCount > 99 ? '99+' : decisionsCount}</span>
          {/if}
        </button>
        <span class="sr-only" aria-live="polite" aria-atomic="true">
          {decisionsCount > 0 ? $t('topbar.decisions_pending', { values: { count: decisionsCount } }) : ''}
        </span>

        <!-- User avatar dropdown -->
        <div class="user-menu-wrap">
          <button
            class="user-btn"
            class:auth-active={hasToken}
            onclick={() => (userMenuOpen = !userMenuOpen)}
            aria-haspopup="menu"
            aria-expanded={userMenuOpen}
            aria-label={$t('topbar.user_menu')}
          >
            <div class="user-avatar" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true">
                <circle cx="12" cy="8" r="4"/>
                <path d="M4 20c0-4 3.6-7 8-7s8 3 8 7"/>
              </svg>
            </div>
            <span class="auth-dot" aria-hidden="true"></span>
          </button>

          {#if userMenuOpen}
            <div
              class="user-dropdown"
              role="menu"
              tabindex="-1"
              aria-label={$t('topbar.user_menu')}
              bind:this={userMenuEl}
              onkeydown={onUserMenuKeydown}
              onfocusout={(e) => { if (!e.currentTarget.contains(e.relatedTarget)) userMenuOpen = false; }}
            >
              <button class="user-dropdown-item" role="menuitem" tabindex="-1" onclick={() => { goToProfile(); userMenuOpen = false; }}>
                {$t('topbar.profile')}
              </button>
              <button class="user-dropdown-item" role="menuitem" tabindex="-1" onclick={() => { openTokenModal(); userMenuOpen = false; }}>
                {$t('topbar.api_token')}
              </button>
              <div class="user-dropdown-divider" role="separator"></div>
              <button class="user-dropdown-item" role="menuitem" tabindex="-1" onclick={() => { localStorage.removeItem('gyre_auth_token'); hasToken = false; userMenuOpen = false; }}>
                {$t('topbar.sign_out')}
              </button>
            </div>
          {/if}
        </div>
      </div>
    </header>

    <!-- ── Mobile navigation drawer ─────────────────────────────────── -->
    {#if mobileDrawerOpen}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="mobile-drawer-backdrop"
        role="presentation"
        onclick={() => (mobileDrawerOpen = false)}
      ></div>
      <nav
        id="mobile-drawer"
        class="mobile-drawer"
        aria-label={$t('topbar.navigation_drawer')}
        data-testid="mobile-drawer"
      >
        <div class="drawer-header">
          <span class="drawer-title">{currentWorkspace?.name ?? 'Navigation'}</span>
          <button
            class="drawer-close"
            onclick={() => (mobileDrawerOpen = false)}
            aria-label={$t('topbar.close_drawer')}
          >✕</button>
        </div>
        <ul class="drawer-links" role="list">
          <li>
            <a
              class="drawer-link"
              href="#section-decisions"
              onclick={(e) => {
                e.preventDefault();
                mobileDrawerOpen = false;
                document.querySelector('[data-testid="section-decisions"]')?.scrollIntoView({ behavior: 'smooth' });
              }}
            >{$t('workspace_home.sections.decisions')}</a>
          </li>
          <li>
            <a
              class="drawer-link"
              href="#section-specs"
              onclick={(e) => {
                e.preventDefault();
                mobileDrawerOpen = false;
                document.querySelector('[data-testid="section-specs"]')?.scrollIntoView({ behavior: 'smooth' });
              }}
            >{$t('workspace_home.sections.specs')}</a>
          </li>
          <li>
            <a
              class="drawer-link"
              href="#section-repos"
              onclick={(e) => {
                e.preventDefault();
                mobileDrawerOpen = false;
                document.querySelector('[data-testid="section-repos"]')?.scrollIntoView({ behavior: 'smooth' });
              }}
            >{$t('workspace_home.sections.repos')}</a>
          </li>
          <li>
            <a
              class="drawer-link"
              href="#section-briefing"
              onclick={(e) => {
                e.preventDefault();
                mobileDrawerOpen = false;
                document.querySelector('[data-testid="section-briefing"]')?.scrollIntoView({ behavior: 'smooth' });
              }}
            >{$t('workspace_home.sections.briefing')}</a>
          </li>
          <li>
            <a
              class="drawer-link"
              href="#section-agent-rules"
              onclick={(e) => {
                e.preventDefault();
                mobileDrawerOpen = false;
                document.querySelector('[data-testid="section-agent-rules"]')?.scrollIntoView({ behavior: 'smooth' });
              }}
            >{$t('topbar.agent_rules_label')}</a>
          </li>
        </ul>
      </nav>
    {/if}

    <!-- ── Content area + detail panel ──────────────────────────────── -->
    <div class="content-split">
    <main class="content" id="main-content" tabindex="-1">
      <div class="content-inner" class:faded={!contentVisible}>
        {#if mode === 'workspace_home'}
          <WorkspaceHome
            workspace={currentWorkspace}
            {decisionsCount}
            onSelectRepo={(repo, tab, specPath) => {
              goToRepo(repo, tab);
              if (specPath) {
                // Open the spec's detail panel after navigation
                tick().then(() => {
                  openDetailPanel({ type: 'spec', id: specPath, data: { repo_id: repo.id } });
                });
              }
            }}
            onWorkspaceCreated={async (newWs) => {
              // Refresh workspace list and navigate to the new workspace
              try { workspaces = await api.workspaces() ?? []; } catch { /* keep existing */ }
              const ws = workspaces.find(w => w.id === newWs?.id) ?? newWs;
              if (ws) goToWorkspaceHome(ws);
            }}
          />
        {:else if mode === 'workspace_settings'}
          <WorkspaceSettings
            workspace={currentWorkspace}
            onBack={() => goToWorkspaceHome(currentWorkspace)}
          />
        {:else if mode === 'agent_rules'}
          <MetaSpecs
            scope={currentWorkspace ? 'workspace' : 'tenant'}
            workspaceId={currentWorkspace?.id ?? null}
          />
        {:else if mode === 'repo'}
          {#if entityDetail}
            <!-- Full-page entity detail view -->
            <DetailPanel
              entity={entityDetail}
              fullPage={true}
              onclose={() => window.history.back()}
              onback={() => window.history.back()}
            />
          {:else}
            <RepoMode
              workspace={currentWorkspace}
              repo={currentRepo}
              activeTab={repoTab}
              onTabChange={(tab) => goToRepoTab(tab)}
              workspaceBudget={workspaceBudget}
            />
          {/if}
        {:else if mode === 'cross_workspace'}
          {#if crossWorkspaceTab === 'settings'}
            <TenantSettings onBack={() => goToCrossWorkspace()} />
          {:else if crossWorkspaceTab === 'agent-rules'}
            <MetaSpecs scope="tenant" workspaceId={null} />
          {:else}
            <CrossWorkspaceHome
              onSelectWorkspace={(ws) => goToWorkspaceHome(ws)}
              onSettings={userIsAdmin ? () => goToTenantSettings() : undefined}
              onManageAgentRules={userIsAdmin ? () => goToTenantAgentRules() : undefined}
            />
          {/if}
        {:else if mode === 'profile'}
          <UserProfile workspaceId={currentWorkspace?.id ?? null} repoId={null} scope="tenant" />
        {/if}
      </div>
    </main>

    <DetailPanel
      entity={detailPanel.open ? detailPanel.entity : null}
      bind:expanded={detailExpanded}
      onclose={closeDetailPanel}
      onback={detailHistory.length > 0 ? goBackDetailPanel : undefined}
    />
    </div>

    <!-- ── Status bar (24px) ─────────────────────────────────────────── -->
    <footer class="status-bar" aria-label={$t('status_bar.label')}>
      {#if trustLevel}
        <span class="status-item status-trust" title={$t('status_bar.trust')}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="12" height="12" aria-hidden="true">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
          </svg>
          {$t('status_bar.trust')}: {trustLevel}
        </span>
      {/if}

      {#if budgetPct !== null}
        <span class="status-item status-budget" title={$t('status_bar.budget')}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="12" height="12" aria-hidden="true">
            <rect x="2" y="5" width="20" height="14" rx="2"/><path d="M2 10h20"/>
          </svg>
          {$t('status_bar.budget')}: {budgetPct}%
          <span
            class="budget-bar-track"
            role="progressbar"
            aria-valuenow={budgetPct}
            aria-valuemin="0"
            aria-valuemax="100"
            aria-label="{$t('status_bar.budget')} {budgetPct}%"
          >
            <span
              class="budget-bar-fill"
              class:bar-danger={budgetPct > 90}
              class:bar-warn={budgetPct > 70 && budgetPct <= 90}
              class:bar-ok={budgetPct <= 70}
              style="width: {budgetPct}%"
            ></span>
          </span>
        </span>
      {/if}

      <span class="status-spacer"></span>

      {#if currentWorkspace?.id}
        <span class="status-item status-presence">
          <PresenceAvatars workspaceId={currentWorkspace.id} wsStore={wsStore} />
        </span>
      {/if}

      <span
        class="status-item status-ws"
        class:connected={wsStatus === 'connected'}
        class:error={wsStatus === 'error' || wsStatus === 'auth-failed'}
        role="status"
        aria-label={wsStatus === 'connected' ? $t('status_bar.live_tooltip') : wsStatus === 'error' || wsStatus === 'auth-failed' ? $t('status_bar.offline_tooltip') : $t('status_bar.connecting_tooltip')}
        title={wsStatus === 'connected' ? $t('status_bar.live_tooltip') : wsStatus === 'error' || wsStatus === 'auth-failed' ? $t('status_bar.offline_tooltip') : $t('status_bar.connecting_tooltip')}
      >
        <span class="ws-dot" aria-hidden="true"></span>
        {wsStatus === 'connected' ? $t('status_bar.live') : wsStatus === 'error' || wsStatus === 'auth-failed' ? $t('status_bar.offline') : $t('status_bar.connecting')}
      </span>
    </footer>
  </div>
</div>
{/if}

<!-- ── Global overlays ────────────────────────────────────────────────── -->
<SearchBar bind:open={searchOpen} onnavigate={(v, opts) => {
  if (v === 'profile') { goToProfile(); return; }
  if (v === 'meta-specs') { goToAgentRules(); return; }
  // Entity search results: open full-page detail for the matched entity
  if (opts?.entityType && opts?.entityId) {
    const data = {};
    if (opts.repo_id) data.repo_id = opts.repo_id;
    if (opts.workspace_id) data.workspace_id = opts.workspace_id;
    tick().then(() => openDetailPanel({ type: opts.entityType, id: opts.entityId, data }));
    return;
  }
  // For section-based views, navigate to workspace home and scroll to the section
  if (mode !== 'workspace_home') goToWorkspaceHome(currentWorkspace);
  if (v === 'inbox') tick().then(() => document.querySelector('[data-testid="section-decisions"]')?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  else if (v === 'briefing') tick().then(() => document.querySelector('[data-testid="section-briefing"]')?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  else if (v === 'specs') tick().then(() => document.querySelector('[data-testid="section-specs"]')?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  else goToWorkspaceHome(currentWorkspace);
}} />
<Toast />

<!-- Keyboard shortcut overlay -->
{#if shortcutsOpen}
  <div class="shortcuts-overlay" role="presentation" tabindex="-1" onclick={() => (shortcutsOpen = false)} onkeydown={(e) => {
    if (e.key === 'Escape') { shortcutsOpen = false; return; }
    if (e.key === 'Tab' && shortcutsModalEl) {
      const focusable = Array.from(shortcutsModalEl.querySelectorAll('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'));
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey) {
        if (document.activeElement === first) { e.preventDefault(); last.focus(); }
      } else {
        if (document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    }
  }}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="shortcuts-modal" bind:this={shortcutsModalEl} onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label={$t('shortcuts.title')} tabindex="-1">
      <div class="shortcuts-header">
        <h2>{$t('shortcuts.title')}</h2>
        <button onclick={() => (shortcutsOpen = false)} aria-label={$t('common.close')}>✕</button>
      </div>
      <div class="shortcuts-body">
        <dl class="shortcuts-list">
          <div class="shortcut-row"><dt><kbd>⌘K</kbd></dt><dd>{$t('shortcuts.global_search')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g h</kbd></dt><dd>{$t('shortcuts.workspace_home')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g s</kbd></dt><dd>{$t('shortcuts.workspace_settings')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g a</kbd></dt><dd>{$t('shortcuts.agent_rules')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 1</kbd></dt><dd>{$t('shortcuts.specs_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 2</kbd></dt><dd>{$t('shortcuts.tasks_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 3</kbd></dt><dd>{$t('shortcuts.mrs_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 4</kbd></dt><dd>{$t('shortcuts.agents_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 5</kbd></dt><dd>{$t('shortcuts.architecture_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 6</kbd></dt><dd>{$t('shortcuts.decisions_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 7</kbd></dt><dd>{$t('shortcuts.code_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>g 8</kbd></dt><dd>{$t('shortcuts.settings_tab')}</dd></div>
          <div class="shortcut-row"><dt><kbd>Esc</kbd></dt><dd>{$t('shortcuts.close_panel')}</dd></div>
          <div class="shortcut-row"><dt><kbd>/</kbd></dt><dd>{$t('shortcuts.focus_search')}</dd></div>
          <div class="shortcut-row"><dt><kbd>?</kbd></dt><dd>{$t('shortcuts.toggle_shortcuts')}</dd></div>
        </dl>
      </div>
    </div>
  </div>
{/if}

<!-- Create Workspace modal (from dropdown) -->
<Modal bind:open={createWsModalOpen} title={$t('workspace_home.create_ws_title')} size="sm">
  <div class="token-modal">
    <label class="token-label" for="ws-name-input">{$t('workspace_home.create_ws_name_label')}</label>
    <input
      id="ws-name-input"
      class="token-input"
      type="text"
      bind:value={createWsForm.name}
      placeholder={$t('workspace_home.create_ws_name_placeholder')}
      onkeydown={(e) => e.key === 'Enter' && handleCreateWorkspaceFromDropdown()}
    />
    <label class="token-label" for="ws-desc-input">{$t('workspace_home.create_ws_desc_label')}</label>
    <input
      id="ws-desc-input"
      class="token-input"
      type="text"
      bind:value={createWsForm.description}
      placeholder={$t('workspace_home.create_ws_desc_placeholder')}
      onkeydown={(e) => e.key === 'Enter' && handleCreateWorkspaceFromDropdown()}
    />
    <div class="token-actions">
      <button class="btn-secondary" onclick={() => (createWsModalOpen = false)}>{$t('common.cancel')}</button>
      <button class="btn-primary" onclick={handleCreateWorkspaceFromDropdown} disabled={createWsSaving || !createWsForm.name?.trim()}>
        {createWsSaving ? $t('workspace_home.create_ws_creating') : $t('workspace_home.create_ws_submit')}
      </button>
    </div>
  </div>
</Modal>

<!-- Token configuration modal -->
<Modal bind:open={tokenModalOpen} title={$t('settings.token.title')} size="sm">
  <div class="token-modal">
    <p class="token-desc">{$t('common.token_desc')}</p>
    {#if tokenInfo}
      <div class="token-info-box">
        <div class="token-info-row">
          <span class="token-info-label">{$t('common.token_kind')}</span>
          <span class="token-info-val">{TOKEN_KIND_LABELS[tokenInfo.kind] ?? tokenInfo.kind ?? '—'}</span>
        </div>
        {#if tokenInfo.agent_id}
          <div class="token-info-row">
            <span class="token-info-label">{$t('common.token_agent_id')}</span>
            <span class="token-info-val mono">{sharedEntityName('agent', tokenInfo.agent_id)}</span>
          </div>
        {/if}
        {#if tokenInfo.exp}
          <div class="token-info-row">
            <span class="token-info-label">{$t('common.token_expires')}</span>
            <span class="token-info-val">{new Date(tokenInfo.exp * 1000).toLocaleString()}</span>
          </div>
        {/if}
      </div>
    {/if}
    <label class="token-label" for="token-input">{$t('settings.token.label')}</label>
    <div class="token-input-wrap">
      <input
        id="token-input"
        class="token-input"
        type={tokenVisible ? 'text' : 'password'}
        bind:value={tokenInput}
        placeholder="gyre-dev-token"
        autocomplete="off"
        onkeydown={(e) => e.key === 'Enter' && saveToken()}
      />
      <button
        class="token-toggle"
        type="button"
        onclick={() => tokenVisible = !tokenVisible}
        aria-label={tokenVisible ? $t('settings.token.hide') : $t('settings.token.show')}
      >
        {#if tokenVisible}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true">
            <path d="M17.94 17.94A10.07 10.07 0 0112 20c-7 0-11-8-11-8a18.45 18.45 0 015.06-5.94M9.9 4.24A9.12 9.12 0 0112 4c7 0 11 8 11 8a18.5 18.5 0 01-2.16 3.19m-6.72-1.07a3 3 0 11-4.24-4.24"/>
            <line x1="1" y1="1" x2="23" y2="23"/>
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true">
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
            <circle cx="12" cy="12" r="3"/>
          </svg>
        {/if}
      </button>
    </div>
    <div class="token-actions">
      <button class="btn-secondary" onclick={() => (tokenModalOpen = false)}>{$t('common.cancel')}</button>
      <button class="btn-primary" onclick={saveToken}>{$t('common.save')}</button>
    </div>
  </div>
</Modal>

<style>
  .app {
    display: flex;
    height: 100vh;
    overflow: hidden;
    background: var(--color-bg);
  }

  /* Main column (full width — no sidebar) */
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  /* ── Topbar ─────────────────────────────────────────────────────────── */
  .topbar {
    display: flex;
    align-items: center;
    height: var(--topbar-height);
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-3);
  }

  .topbar-left {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
    min-width: 0;
  }

  /* Repo mode: back arrow + breadcrumb */
  .repo-context { gap: var(--space-2); }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    border-radius: var(--radius);
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .back-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }

  .breadcrumb-ws {
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: 0;
    white-space: nowrap;
    transition: color var(--transition-fast);
  }

  .breadcrumb-ws:hover {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .breadcrumb-ws:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  .breadcrumb-sep {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  .breadcrumb-repo {
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Workspace selector */
  .ws-selector { position: relative; }

  .logo-mark {
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .ws-name-wrap {
    display: flex;
    align-items: center;
    gap: 0;
  }

  .ws-name-btn {
    background: transparent;
    border: none;
    color: var(--color-text);
    cursor: pointer;
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    padding: var(--space-1) var(--space-1);
    white-space: nowrap;
    transition: color var(--transition-fast);
  }

  .ws-name-btn:hover { color: var(--color-primary); }

  .ws-name-btn.ws-name-empty {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-weight: 400;
  }

  .ws-name-btn:focus-visible,
  .ws-arrow-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  .ws-arrow-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: color var(--transition-fast), background var(--transition-fast);
    padding: 0;
  }

  .ws-arrow-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .ws-gear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: color var(--transition-fast), background var(--transition-fast);
    padding: 0;
    margin-left: var(--space-1);
  }

  .ws-gear-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .ws-gear-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ws-dropdown {
    position: absolute;
    top: calc(100% + var(--space-1));
    left: 0;
    z-index: 200;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-md);
    min-width: 200px;
    padding: var(--space-1) 0;
  }

  .ws-dropdown-empty {
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .ws-dropdown-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    text-align: left;
    transition: background var(--transition-fast), color var(--transition-fast);
    white-space: nowrap;
  }

  .ws-dropdown-item:hover {
    background: var(--color-border);
    color: var(--color-text);
  }

  .ws-dropdown-item.active {
    color: var(--color-text);
    font-weight: 500;
  }

  .ws-dropdown-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    background: var(--color-border);
    color: var(--color-text);
  }

  .ws-check {
    color: var(--color-primary);
    font-size: var(--text-xs);
  }

  /* ── Center search ──────────────────────────────────────────────────── */
  .topbar-center {
    flex: 1;
    display: flex;
    justify-content: center;
  }

  .search-trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    transition: border-color var(--transition-fast), color var(--transition-fast);
    max-width: 320px;
    width: 100%;
  }

  .search-trigger:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text-secondary);
  }

  .search-trigger kbd {
    background: var(--color-border);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-1);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .search-trigger:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Right side ─────────────────────────────────────────────────────── */
  .topbar-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .decisions-badge-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    border-radius: var(--radius);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .decisions-badge-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .decisions-badge-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .decisions-count {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    background: var(--color-danger);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }

  .user-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: 50%;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .user-btn:hover {
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }

  .user-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .auth-dot {
    position: absolute;
    bottom: 0;
    right: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-danger);
    border: 2px solid var(--color-surface);
    transition: background var(--transition-fast);
  }

  .user-btn.auth-active .auth-dot {
    background: var(--color-success);
  }

  .user-menu-wrap { position: relative; }

  .user-dropdown {
    position: absolute;
    top: calc(100% + var(--space-1));
    right: 0;
    z-index: 200;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-md);
    min-width: 160px;
    padding: var(--space-1) 0;
  }

  .user-dropdown-item {
    display: block;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    text-align: left;
    transition: background var(--transition-fast), color var(--transition-fast);
    white-space: nowrap;
  }

  .user-dropdown-item:hover {
    background: var(--color-border);
    color: var(--color-text);
  }

  .user-dropdown-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    background: var(--color-border);
    color: var(--color-text);
  }

  .user-dropdown-divider {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-1) 0;
  }

  /* ── Content area + detail panel split ────────────────────────────── */
  .content-split {
    flex: 1;
    display: flex;
    overflow: hidden;
    min-height: 0;
  }

  .content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }

  main:focus { outline: none; }

  .content-inner {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
    transition: opacity var(--transition-fast);
  }

  .content-inner.faded { opacity: 0; }

  /* ── Status bar ─────────────────────────────────────────────────────── */
  .status-bar {
    display: flex;
    align-items: center;
    height: 24px;
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .status-item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    white-space: nowrap;
  }

  .status-spacer { flex: 1; }

  .budget-bar-track {
    display: inline-block;
    width: 40px;
    height: 4px;
    background: var(--color-border-strong);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .budget-bar-fill {
    display: block;
    height: 100%;
    border-radius: var(--radius-sm);
    transition: width var(--transition-normal);
    background: var(--color-success);
  }

  .budget-bar-fill.bar-danger { background: var(--color-danger); }
  .budget-bar-fill.bar-warn { background: var(--color-warning); }
  .budget-bar-fill.bar-ok { background: var(--color-success); }

  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-text-muted);
    transition: background var(--transition-fast);
  }

  .status-ws.connected .ws-dot {
    background: var(--color-success);
    box-shadow: 0 0 4px color-mix(in srgb, var(--color-success) 50%, transparent);
  }

  .status-ws.error .ws-dot { background: var(--color-danger); }

  /* ── Skip to content ────────────────────────────────────────────────── */
  .skip-to-content {
    position: fixed;
    top: -100%;
    left: var(--space-4);
    z-index: 9999;
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    color: var(--color-text-inverse);
    border-radius: 0 0 var(--radius) var(--radius);
    font-size: var(--text-sm);
    text-decoration: none;
    transition: top var(--transition-fast);
  }

  .skip-to-content:focus { top: 0; }

  /* ── Keyboard shortcuts overlay ─────────────────────────────────────── */
  .shortcuts-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: color-mix(in srgb, var(--color-bg) 60%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .shortcuts-modal {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    width: 400px;
    max-width: 90vw;
  }

  .shortcuts-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
  }

  .shortcuts-header h2 {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .shortcuts-header button {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: 0;
  }

  .shortcuts-header button:hover { color: var(--color-text); }

  .shortcuts-header button:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .shortcuts-body { padding: var(--space-4) var(--space-6); }

  .shortcuts-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
  }

  .shortcut-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .shortcut-row dt { flex-shrink: 0; }

  .shortcut-row dd {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    text-align: right;
  }

  .shortcut-row kbd {
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    padding: 2px 6px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
  }

  /* ── Token modal ────────────────────────────────────────────────────── */
  .token-modal {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .token-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .token-info-box {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .token-info-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .token-info-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    width: 70px;
    flex-shrink: 0;
  }

  .token-info-val {
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .token-info-val.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .token-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .token-input-wrap {
    display: flex;
    align-items: center;
    position: relative;
  }

  .token-input {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    padding-right: 36px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    box-sizing: border-box;
  }

  .token-input:focus:not(:focus-visible) { outline: none; }

  .token-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  .token-toggle {
    position: absolute;
    right: var(--space-2);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius-sm);
    padding: 0;
  }

  .token-toggle:hover { color: var(--color-text); }

  .token-toggle:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .token-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .btn-primary {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .btn-primary:hover { background: var(--color-primary-hover); }
  .btn-primary:active { background: var(--color-primary-hover); }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .btn-secondary:hover { border-color: var(--color-text-muted); }

  .btn-primary:focus-visible,
  .btn-secondary:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0,0,0,0);
    white-space: nowrap;
    border: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .content-inner,
    .search-trigger,
    .decisions-badge-btn,
    .user-btn,
    .auth-dot,
    .user-dropdown-item,
    .ws-dropdown-item,
    .back-btn,
    .budget-bar-fill,
    .ws-dot,
    .skip-to-content,
    .btn-primary,
    .btn-secondary { transition: none; }
  }

  /* ── Hamburger button (mobile only) ────────────────────────────────── */
  .hamburger-btn {
    display: none; /* hidden on desktop */
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    border-radius: var(--radius);
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .hamburger-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .hamburger-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Mobile navigation drawer ──────────────────────────────────────── */
  .mobile-drawer-backdrop {
    display: none; /* shown only on mobile, see @media below */
    position: fixed;
    inset: 0;
    z-index: 150;
    background: color-mix(in srgb, var(--color-bg) 50%, transparent);
  }

  .mobile-drawer {
    display: none; /* shown only on mobile */
    position: fixed;
    top: var(--topbar-height);
    left: 0;
    bottom: 0;
    width: 260px;
    z-index: 160;
    background: var(--color-surface);
    border-right: 1px solid var(--color-border-strong);
    box-shadow: var(--shadow-md);
    flex-direction: column;
    overflow-y: auto;
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .drawer-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .drawer-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: var(--space-1);
    border-radius: var(--radius-sm);
  }

  .drawer-close:hover { color: var(--color-text); }

  .drawer-close:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .drawer-links {
    list-style: none;
    margin: 0;
    padding: var(--space-2) 0;
  }

  .drawer-links li { border-bottom: 1px solid var(--color-border); }
  .drawer-links li:last-child { border-bottom: none; }

  .drawer-link {
    display: block;
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    text-decoration: none;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .drawer-link:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .drawer-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Workspace dropdown: All Workspaces entry ─────────────────────── */
  .ws-dropdown-all {
    color: var(--color-primary);
    font-weight: 500;
  }

  .ws-all-icon {
    margin-right: var(--space-2);
    font-size: var(--text-base);
  }

  .ws-dropdown-create {
    color: var(--color-primary);
    font-weight: 500;
  }

  .ws-dropdown-divider {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-1) 0;
  }

  @media (max-width: 768px) {
    /* Show hamburger button on mobile */
    .hamburger-btn { display: flex; }

    /* Show drawer on mobile when open */
    .mobile-drawer-backdrop { display: block; }
    .mobile-drawer { display: flex; }

    .search-trigger kbd { display: none; }
    .search-trigger span { display: none; }
    .topbar { gap: var(--space-2); padding: 0 var(--space-2); }
    .status-bar { gap: var(--space-2); padding: 0 var(--space-2); }
    .status-trust, .status-budget { display: none; }
    .breadcrumb-repo { max-width: 120px; }
  }
</style>
