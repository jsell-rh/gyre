<script>
  import './lib/design-system.css';
  import { isLoading } from 'svelte-i18n';
  import { createWsStore } from './lib/ws.js';
  import Sidebar from './components/Sidebar.svelte';
  import ContentArea from './lib/ContentArea.svelte';
  import Inbox from './components/Inbox.svelte';
  import Briefing from './components/Briefing.svelte';
  import ExplorerView from './components/ExplorerView.svelte';
  import SpecDashboard from './components/SpecDashboard.svelte';
  import MetaSpecs from './components/MetaSpecs.svelte';
  import AdminPanel from './components/AdminPanel.svelte';
  import Toast from './lib/Toast.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import Modal from './lib/Modal.svelte';
  import { onMount, setContext } from 'svelte';
  import { setAuthToken, api } from './lib/api.js';

  // ── Primary navigation state ─────────────────────────────────────────
  // One of: 'inbox' | 'briefing' | 'explorer' | 'specs' | 'meta-specs' | 'admin'
  let currentNav = $state('explorer');

  // ── Scope: which tenant/workspace/repo we're viewing ─────────────────
  // { type: 'tenant' | 'workspace' | 'repo', tenantId?, workspaceId?, repoId? }
  let scope = $state({ type: 'tenant' });

  // ── Global detail panel ──────────────────────────────────────────────
  // Opened by clicking any entity reference (agent, spec, MR, task, node)
  let detailPanel = $state({ open: false, entity: null });

  // ── WebSocket ────────────────────────────────────────────────────────
  let wsStatus = $state('disconnected');

  $effect(() => {
    const store = createWsStore();
    const unsub = store.onStatus((s) => (wsStatus = s));
    return () => {
      unsub();
      store.destroy();
    };
  });

  // ── UI state ─────────────────────────────────────────────────────────
  let searchOpen = $state(false);
  let shortcutsOpen = $state(false);
  let wsDropdownOpen = $state(false);
  let tokenModalOpen = $state(false);
  let tokenInput = $state(localStorage.getItem('gyre_auth_token') || 'gyre-dev-token');
  let hasToken = $state(!!localStorage.getItem('gyre_auth_token'));
  let tokenInfo = $state(null);

  // Content cross-fade key (increment to trigger fade transition)
  let contentVisible = $state(true);

  function fadeContent() {
    contentVisible = false;
    setTimeout(() => { contentVisible = true; }, 150);
  }

  // ── Data ─────────────────────────────────────────────────────────────
  let workspaces = $state([]);
  let currentWorkspace = $state(null);
  let workspaceBudget = $state(null);
  let inboxBadge = $state(0);

  async function loadWorkspaceData(workspaceId) {
    try { workspaceBudget = await api.workspaceBudget(workspaceId); } catch { workspaceBudget = null; }
  }

  async function loadInboxBadge() {
    try {
      const [mrs, specs] = await Promise.allSettled([
        api.mergeRequests({ status: 'review' }),
        api.getPendingSpecs(),
      ]);
      const mrCount = mrs.status === 'fulfilled' ? (mrs.value || []).length : 0;
      const specCount = specs.status === 'fulfilled' ? (specs.value || []).length : 0;
      inboxBadge = mrCount + specCount;
    } catch { /* ignore */ }
  }

  // ── URL routing ───────────────────────────────────────────────────────
  const NAV_ITEMS = ['inbox', 'briefing', 'explorer', 'specs', 'meta-specs', 'admin'];

  export function parseUrl(pathname) {
    const parts = pathname.split('/').filter(Boolean);
    if (parts.length === 0) return null;

    if (parts.length === 1) {
      const [nav] = parts;
      if (NAV_ITEMS.includes(nav)) return { nav, scope: { type: 'tenant' } };
    }

    if (parts.length >= 3) {
      const [seg, id, navRaw] = parts;
      const nav = navRaw || 'inbox';
      if (!NAV_ITEMS.includes(nav)) return null;

      if (seg === 'workspaces') {
        return { nav, scope: { type: 'workspace', workspaceId: id } };
      }
      if (seg === 'repos') {
        return { nav, scope: { type: 'repo', repoId: id } };
      }
    }

    return null;
  }

  export function urlFor(nav, s) {
    if (!s || s.type === 'tenant') return `/${nav}`;
    if (s.type === 'workspace') return `/workspaces/${s.workspaceId}/${nav}`;
    if (s.type === 'repo') return `/repos/${s.repoId}/${nav}`;
    return `/${nav}`;
  }

  function pushUrl(nav, s) {
    window.history.pushState({ nav, scope: s }, '', urlFor(nav, s));
  }

  // ── Navigation ────────────────────────────────────────────────────────
  function navigate(nav, scopeOverride) {
    const newScope = scopeOverride ?? scope;
    currentNav = nav;
    if (scopeOverride) {
      scope = scopeOverride;
      fadeContent();
    }
    pushUrl(nav, newScope);
    if (newScope.type === 'workspace') loadWorkspaceData(newScope.workspaceId);
  }

  function setScope(newScope, nav) {
    scope = newScope;
    if (nav) currentNav = nav;
    fadeContent();
    pushUrl(currentNav, newScope);
    if (newScope.type === 'workspace') loadWorkspaceData(newScope.workspaceId);
  }

  // Expose navigate and scope accessor via context (no prop drilling)
  setContext('navigate', navigate);
  setContext('getScope', () => scope);
  setContext('openDetailPanel', openDetailPanel);

  // ── Workspace switching (breadcrumb dropdown) ─────────────────────────
  function selectWorkspace(ws) {
    currentWorkspace = ws;
    wsDropdownOpen = false;
    localStorage.setItem('gyre_workspace_id', ws.id);
    setScope({ type: 'workspace', workspaceId: ws.id }, 'inbox');
  }

  // ── Detail panel ──────────────────────────────────────────────────────
  function openDetailPanel(entity) {
    detailPanel = { open: true, entity };
  }

  function closeDetailPanel() {
    detailPanel = { open: false, entity: null };
  }

  // ── Keyboard shortcuts ────────────────────────────────────────────────
  const NAV_SHORTCUTS = {
    '1': 'inbox', '2': 'briefing', '3': 'explorer',
    '4': 'specs', '5': 'meta-specs', '6': 'admin',
  };

  function handleKeydown(e) {
    const inInput = e.target.tagName === 'INPUT'
      || e.target.tagName === 'TEXTAREA'
      || e.target.isContentEditable;

    // Cmd+K: global search
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      searchOpen = true;
      return;
    }

    // Cmd+1-6: nav shortcuts
    if ((e.metaKey || e.ctrlKey) && NAV_SHORTCUTS[e.key]) {
      e.preventDefault();
      navigate(NAV_SHORTCUTS[e.key]);
      return;
    }

    // Esc: close detail panel or go up one scope level
    if (e.key === 'Escape') {
      if (shortcutsOpen) { shortcutsOpen = false; return; }
      if (wsDropdownOpen) { wsDropdownOpen = false; return; }
      if (detailPanel.open) { closeDetailPanel(); return; }
      if (scope.type === 'repo') {
        setScope({ type: 'workspace', workspaceId: scope.workspaceId });
      } else if (scope.type === 'workspace') {
        setScope({ type: 'tenant' });
      }
      return;
    }

    // /: focus search in current view (suppressed in text inputs)
    if (e.key === '/' && !inInput) {
      e.preventDefault();
      searchOpen = true;
      return;
    }

    // ?: keyboard shortcut overlay (suppressed in text inputs)
    if (e.key === '?' && !inInput) {
      e.preventDefault();
      shortcutsOpen = !shortcutsOpen;
    }
  }

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
    tokenInfo = null;
    try { tokenInfo = await api.tokenInfo(); } catch { /* ignore */ }
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

  let currentLayout = $derived.by(() => {
    if (detailPanel.open) return 'split';
    if (currentNav === 'explorer' && scope.type !== 'tenant') return 'canvas-controls';
    return 'full-width';
  });

  // Breadcrumb segments
  let scopeCrumbs = $derived.by(() => {
    const crumbs = [{ label: 'Gyre', action: () => setScope({ type: 'tenant' }, 'explorer') }];
    if (scope.type !== 'tenant' && currentWorkspace) {
      crumbs.push({
        label: currentWorkspace.name,
        action: null, // workspace segment handled by dropdown
        isWorkspace: true,
      });
    }
    if (scope.type === 'repo' && scope.repoId) {
      crumbs.push({ label: scope.repoId, action: null });
    }
    return crumbs;
  });

  // ── Mount: entrypoint flow + URL routing ──────────────────────────────
  onMount(async () => {
    // 1. Load all workspaces
    try { workspaces = await api.workspaces(); } catch { /* ignore */ }

    // 2. Determine initial scope from URL or entrypoint flow
    const fromUrl = parseUrl(window.location.pathname);

    if (fromUrl) {
      // URL-driven navigation
      currentNav = fromUrl.nav;
      scope = fromUrl.scope;

      if (fromUrl.scope.type === 'workspace') {
        const ws = workspaces.find(w => w.id === fromUrl.scope.workspaceId) ?? null;
        currentWorkspace = ws;
        if (ws) {
          localStorage.setItem('gyre_workspace_id', ws.id);
          loadWorkspaceData(ws.id);
        }
      }
    } else {
      // Entrypoint flow
      const savedWsId = localStorage.getItem('gyre_workspace_id');
      if (savedWsId) {
        const ws = workspaces.find(w => w.id === savedWsId);
        if (ws) {
          // Subsequent visit: restore workspace, land on inbox
          currentWorkspace = ws;
          scope = { type: 'workspace', workspaceId: ws.id };
          currentNav = 'inbox';
          loadWorkspaceData(ws.id);
        } else {
          // Stored workspace not found: fall back to tenant explorer
          localStorage.removeItem('gyre_workspace_id');
          scope = { type: 'tenant' };
          currentNav = 'explorer';
        }
      } else {
        // First visit: tenant explorer (workspace cards)
        scope = { type: 'tenant' };
        currentNav = 'explorer';
      }
    }

    // Replace history state with canonical URL
    window.history.replaceState({ nav: currentNav, scope }, '', urlFor(currentNav, scope));

    // 3. Load inbox badge, refresh every 60s
    loadInboxBadge();
    const inboxInterval = setInterval(loadInboxBadge, 60_000);

    // 4. Popstate (browser back/forward)
    function handlePopstate(e) {
      if (e.state?.nav) {
        currentNav = e.state.nav;
        scope = e.state.scope ?? { type: 'tenant' };
        fadeContent();
        if (scope.type === 'workspace') {
          currentWorkspace = workspaces.find(w => w.id === scope.workspaceId) ?? null;
          if (currentWorkspace) loadWorkspaceData(scope.workspaceId);
        } else if (scope.type === 'tenant') {
          currentWorkspace = null;
        }
      }
    }

    window.addEventListener('popstate', handlePopstate);
    window.addEventListener('keydown', handleKeydown);

    // Close workspace dropdown on outside click
    function handleOutsideClick(e) {
      if (!e.target.closest('.ws-dropdown-wrap')) {
        wsDropdownOpen = false;
      }
    }
    window.addEventListener('click', handleOutsideClick, true);

    return () => {
      window.removeEventListener('popstate', handlePopstate);
      window.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('click', handleOutsideClick, true);
      clearInterval(inboxInterval);
    };
  });
</script>

<a href="#main-content" class="skip-to-content">Skip to main content</a>

{#if !$isLoading}
<div class="app">
  <!-- Sidebar: 6 fixed nav items, always present -->
  <Sidebar bind:currentNav onnavigate={(v) => navigate(v)} {inboxBadge} />

  <!-- Main area: topbar + content + status bar -->
  <div class="main">
    <!-- Topbar (48px) -->
    <header class="topbar">
      <!-- Left: scope breadcrumb -->
      <nav class="breadcrumb" aria-label="Scope breadcrumb">
        {#each scopeCrumbs as crumb, i}
          {#if i > 0}
            <span class="breadcrumb-sep" aria-hidden="true">›</span>
          {/if}

          {#if crumb.isWorkspace}
            <!-- Workspace segment: click opens dropdown -->
            <div class="ws-dropdown-wrap">
              <button
                class="breadcrumb-btn"
                class:active={wsDropdownOpen}
                onclick={() => (wsDropdownOpen = !wsDropdownOpen)}
                aria-haspopup="listbox"
                aria-expanded={wsDropdownOpen}
                aria-label="Switch workspace: {crumb.label}"
              >
                {crumb.label}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10" aria-hidden="true">
                  <path d="M6 9l6 6 6-6"/>
                </svg>
              </button>

              {#if wsDropdownOpen}
                <ul class="ws-dropdown" role="listbox" aria-label="Select workspace">
                  {#each workspaces as ws}
                    <li role="option" aria-selected={ws.id === scope.workspaceId}>
                      <button
                        class="ws-dropdown-item"
                        class:selected={ws.id === scope.workspaceId}
                        onclick={() => selectWorkspace(ws)}
                      >
                        {ws.name}
                      </button>
                    </li>
                  {/each}
                  {#if workspaces.length === 0}
                    <li class="ws-dropdown-empty">No workspaces</li>
                  {/if}
                </ul>
              {/if}
            </div>

          {:else if crumb.action}
            <button class="breadcrumb-btn" onclick={crumb.action}>
              {crumb.label}
            </button>
          {:else}
            <span class="breadcrumb-current">{crumb.label}</span>
          {/if}
        {/each}
      </nav>

      <!-- Center: Cmd+K search trigger -->
      <div class="topbar-center">
        <button
          class="search-trigger"
          onclick={() => (searchOpen = true)}
          aria-label="Open search (Ctrl+K)"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
          </svg>
          <span>Search</span>
          <kbd aria-hidden="true">⌘K</kbd>
        </button>
      </div>

      <!-- Right: inbox badge + user avatar -->
      <div class="topbar-right">
        <!-- Inbox badge shortcut -->
        <button
          class="inbox-badge-btn"
          onclick={() => navigate('inbox')}
          aria-label="{inboxBadge} unresolved inbox items"
          title="Inbox"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true">
            <polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/>
            <path d="M5.45 5.11L2 12v6a2 2 0 002 2h16a2 2 0 002-2v-6l-3.45-6.89A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11z"/>
          </svg>
          {#if inboxBadge > 0}
            <span class="inbox-count" aria-hidden="true">{inboxBadge > 99 ? '99+' : inboxBadge}</span>
          {/if}
        </button>

        <!-- User avatar / token button -->
        <button
          class="user-btn"
          class:auth-active={hasToken}
          onclick={openTokenModal}
          aria-label={hasToken ? 'Authenticated — configure API token' : 'No token — configure API token'}
        >
          <div class="user-avatar" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16">
              <circle cx="12" cy="8" r="4"/>
              <path d="M4 20c0-4 3.6-7 8-7s8 3 8 7"/>
            </svg>
          </div>
          <span class="auth-dot" aria-hidden="true"></span>
        </button>
      </div>
    </header>

    <!-- Content area (adapts to nav + scope) -->
    <main class="content" id="main-content" tabindex="-1">
      <ContentArea layout={currentLayout} {detailPanel} onclosePanel={closeDetailPanel}>
        <div class="content-inner" class:faded={!contentVisible}>
          {#if currentNav === 'inbox'}
            <Inbox workspaceId={scope.workspaceId} />
          {:else if currentNav === 'briefing'}
            <Briefing workspaceId={scope.workspaceId} />
          {:else if currentNav === 'explorer'}
            <ExplorerView {scope} />
          {:else if currentNav === 'specs'}
            <SpecDashboard {scope} />
          {:else if currentNav === 'meta-specs'}
            <MetaSpecs {scope} />
          {:else if currentNav === 'admin'}
            <AdminPanel {scope} />
          {/if}
        </div>
      </ContentArea>
    </main>

    <!-- Status bar (24px) -->
    <footer class="status-bar" aria-label="Status bar">
      <!-- Trust level -->
      {#if trustLevel}
        <span class="status-item status-trust" title="Workspace trust level">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="12" height="12" aria-hidden="true">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
          </svg>
          Trust: {trustLevel}
        </span>
      {/if}

      <!-- Budget usage -->
      {#if budgetPct !== null}
        <span class="status-item status-budget" title="Budget usage">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="12" height="12" aria-hidden="true">
            <rect x="2" y="5" width="20" height="14" rx="2"/><path d="M2 10h20"/>
          </svg>
          Budget: {budgetPct}%
          <span
            class="budget-bar-track"
            role="progressbar"
            aria-valuenow={budgetPct}
            aria-valuemin="0"
            aria-valuemax="100"
            aria-label="Budget {budgetPct}%"
          >
            <span class="budget-bar-fill" style="width: {budgetPct}%; background: {budgetPct > 90 ? 'var(--color-danger)' : budgetPct > 70 ? 'var(--color-warning)' : 'var(--color-success)'}"></span>
          </span>
        </span>
      {/if}

      <!-- Spacer -->
      <span class="status-spacer"></span>

      <!-- Presence avatars stub (replaced by S4.8) -->
      <span class="status-item status-presence" aria-hidden="true" title="Active users (S4.8 pending)">
        <span class="presence-stub"></span>
      </span>

      <!-- WebSocket status -->
      <span
        class="status-item status-ws"
        class:connected={wsStatus === 'connected'}
        class:error={wsStatus === 'error' || wsStatus === 'auth-failed'}
        role="status"
        aria-label="WebSocket: {wsStatus}"
        title="WebSocket: {wsStatus}"
      >
        <span class="ws-dot" aria-hidden="true"></span>
        WS
      </span>
    </footer>
  </div>
</div>
{/if}

<!-- Global overlays -->
<SearchBar bind:open={searchOpen} onnavigate={(v) => navigate(v)} />
<Toast />

<!-- Keyboard shortcut overlay -->
{#if shortcutsOpen}
  <div class="shortcuts-overlay" role="dialog" aria-label="Keyboard shortcuts">
    <div class="shortcuts-modal">
      <div class="shortcuts-header">
        <h2>Keyboard Shortcuts</h2>
        <button onclick={() => (shortcutsOpen = false)} aria-label="Close">✕</button>
      </div>
      <div class="shortcuts-body">
        <dl class="shortcuts-list">
          <div class="shortcut-row"><dt><kbd>⌘K</kbd></dt><dd>Global search</dd></div>
          <div class="shortcut-row"><dt><kbd>⌘1</kbd></dt><dd>Inbox</dd></div>
          <div class="shortcut-row"><dt><kbd>⌘2</kbd></dt><dd>Briefing</dd></div>
          <div class="shortcut-row"><dt><kbd>⌘3</kbd></dt><dd>Explorer</dd></div>
          <div class="shortcut-row"><dt><kbd>⌘4</kbd></dt><dd>Specs</dd></div>
          <div class="shortcut-row"><dt><kbd>⌘5</kbd></dt><dd>Meta-specs</dd></div>
          <div class="shortcut-row"><dt><kbd>⌘6</kbd></dt><dd>Admin</dd></div>
          <div class="shortcut-row"><dt><kbd>Esc</kbd></dt><dd>Close panel / go up scope</dd></div>
          <div class="shortcut-row"><dt><kbd>/</kbd></dt><dd>Focus search</dd></div>
          <div class="shortcut-row"><dt><kbd>?</kbd></dt><dd>Toggle this overlay</dd></div>
        </dl>
      </div>
    </div>
  </div>
{/if}

<!-- Token configuration modal -->
<Modal bind:open={tokenModalOpen} title="API Token" size="sm">
  <div class="token-modal">
    <p class="token-desc">Set the Bearer token for all API and WebSocket requests.</p>
    {#if tokenInfo}
      <div class="token-info-box">
        <div class="token-info-row">
          <span class="token-info-label">Kind</span>
          <span class="token-info-val">{TOKEN_KIND_LABELS[tokenInfo.kind] ?? tokenInfo.kind ?? '—'}</span>
        </div>
        {#if tokenInfo.agent_id}
          <div class="token-info-row">
            <span class="token-info-label">Agent ID</span>
            <span class="token-info-val mono">{tokenInfo.agent_id}</span>
          </div>
        {/if}
        {#if tokenInfo.exp}
          <div class="token-info-row">
            <span class="token-info-label">Expires</span>
            <span class="token-info-val">{new Date(tokenInfo.exp * 1000).toLocaleString()}</span>
          </div>
        {/if}
      </div>
    {/if}
    <label class="token-label" for="token-input">Token</label>
    <input
      id="token-input"
      class="token-input"
      type="text"
      bind:value={tokenInput}
      placeholder="gyre-dev-token"
      onkeydown={(e) => e.key === 'Enter' && saveToken()}
    />
    <div class="token-actions">
      <button class="btn-secondary" onclick={() => (tokenModalOpen = false)}>Cancel</button>
      <button class="btn-primary" onclick={saveToken}>Save</button>
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

  /* Main column: topbar + content + status bar */
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  /* Topbar (48px) */
  .topbar {
    display: flex;
    align-items: center;
    height: 48px;
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
  }

  /* Breadcrumb */
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 0 auto;
  }

  .breadcrumb-sep {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    user-select: none;
  }

  .breadcrumb-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    font-family: var(--font-body);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    transition: color var(--transition-fast), background var(--transition-fast);
    white-space: nowrap;
  }

  .breadcrumb-btn:hover, .breadcrumb-btn.active {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .breadcrumb-current {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    padding: var(--space-1) var(--space-2);
  }

  /* Workspace dropdown */
  .ws-dropdown-wrap {
    position: relative;
  }

  .ws-dropdown {
    position: absolute;
    top: calc(100% + var(--space-1));
    left: 0;
    z-index: 100;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-md);
    list-style: none;
    margin: 0;
    padding: var(--space-1) 0;
    min-width: 180px;
    max-height: 280px;
    overflow-y: auto;
  }

  .ws-dropdown-item {
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

  .ws-dropdown-item:hover {
    background: var(--color-border);
    color: var(--color-text);
  }

  .ws-dropdown-item.selected {
    color: var(--color-primary);
    font-weight: 500;
  }

  .ws-dropdown-empty {
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  /* Center search trigger */
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
    padding: 1px 4px;
    font-size: 0.65rem;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  /* Topbar right */
  .topbar-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .inbox-badge-btn {
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

  .inbox-badge-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .inbox-count {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    background: var(--color-primary);
    color: #fff;
    border-radius: 999px;
    font-size: 0.55rem;
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

  /* Content area */
  .content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  main:focus {
    outline: none;
  }

  .content-inner {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
    transition: opacity 150ms ease;
  }

  .content-inner.faded {
    opacity: 0;
  }

  /* Status bar (24px) */
  .status-bar {
    display: flex;
    align-items: center;
    height: 24px;
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
    font-size: 0.65rem;
    color: var(--color-text-muted);
  }

  .status-item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    white-space: nowrap;
  }

  .status-spacer {
    flex: 1;
  }

  /* Budget bar */
  .budget-bar-track {
    display: inline-block;
    width: 40px;
    height: 4px;
    background: var(--color-border-strong);
    border-radius: 2px;
    overflow: hidden;
  }

  .budget-bar-fill {
    display: block;
    height: 100%;
    border-radius: 2px;
    transition: width var(--transition-normal);
  }

  /* Presence stub */
  .presence-stub {
    display: inline-block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--color-surface-elevated);
    border: 1px dashed var(--color-border-strong);
    opacity: 0.5;
  }

  /* WebSocket status */
  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-text-muted);
    transition: background var(--transition-fast);
  }

  .status-ws.connected .ws-dot {
    background: var(--color-success);
    box-shadow: 0 0 4px rgba(99, 153, 61, 0.5);
  }

  .status-ws.error .ws-dot {
    background: var(--color-danger);
  }

  /* Skip to content */
  .skip-to-content {
    position: fixed;
    top: -100%;
    left: var(--space-4);
    z-index: 9999;
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    color: #fff;
    border-radius: 0 0 var(--radius) var(--radius);
    font-size: var(--text-sm);
    text-decoration: none;
    transition: top var(--transition-fast);
  }

  .skip-to-content:focus {
    top: 0;
  }

  /* Keyboard shortcuts overlay */
  .shortcuts-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .shortcuts-modal {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    width: 340px;
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

  .shortcuts-body {
    padding: var(--space-4) var(--space-6);
  }

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

  .shortcut-row dt {
    flex-shrink: 0;
  }

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

  /* Token modal */
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

  .token-input {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    box-sizing: border-box;
  }

  .token-input:focus {
    outline: none;
    border-color: var(--color-primary);
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
    color: #fff;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .btn-primary:hover { opacity: 0.85; }

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
</style>
