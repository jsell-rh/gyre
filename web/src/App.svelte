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
  import UserProfile from './components/UserProfile.svelte';
  import Toast from './lib/Toast.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import Modal from './lib/Modal.svelte';
  import ScopeBreadcrumb from './lib/ScopeBreadcrumb.svelte';
  import PresenceAvatars from './lib/PresenceAvatars.svelte';
  import { onMount, setContext, tick } from 'svelte';
  import { setAuthToken, api } from './lib/api.js';
  import { toast as showToast } from './lib/toast.svelte.js';

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
      inboxBadge = await api.notificationCount(scope?.type === 'workspace' ? scope.workspaceId : undefined);
    } catch { /* ignore */ }
  }

  // ── URL routing ───────────────────────────────────────────────────────
  const NAV_ITEMS = ['inbox', 'briefing', 'explorer', 'specs', 'meta-specs', 'admin', 'profile'];

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
    window.history.pushState({ nav, scope: { ...s } }, '', urlFor(nav, s));
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

  // ── ScopeBreadcrumb onnavigate handler ───────────────────────────────
  function onBreadcrumbNavigate(view, ctx) {
    if (ctx.scope === 'tenant') {
      setScope({ type: 'tenant' }, view || currentNav);
    } else if (ctx.scope === 'workspace' && ctx.workspace) {
      const ws = workspaces.find(w => w.id === ctx.workspace.id) || ctx.workspace;
      currentWorkspace = ws;
      localStorage.setItem('gyre_workspace_id', ws.id);
      setScope({ type: 'workspace', workspaceId: ws.id }, view || 'inbox');
    }
  }

  // ── Workspace switching (breadcrumb dropdown) ─────────────────────────
  function selectWorkspace(ws) {
    currentWorkspace = ws;
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

  // ── User menu keyboard navigation ───────────────────────────────────
  function onUserMenuKeydown(e) {
    const items = userMenuEl?.querySelectorAll('[role="menuitem"]');
    if (!items?.length) return;
    const arr = Array.from(items);
    const current = arr.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      arr[(current + 1) % arr.length]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      arr[(current - 1 + arr.length) % arr.length]?.focus();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      userMenuOpen = false;
      document.querySelector('.user-btn')?.focus();
    } else if (e.key === 'Home') {
      e.preventDefault();
      arr[0]?.focus();
    } else if (e.key === 'End') {
      e.preventDefault();
      arr[arr.length - 1]?.focus();
    }
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

    // Cmd+1-6: nav shortcuts (check e.key first, fall back to e.code for non-US layouts)
    const shortcutDigit = NAV_SHORTCUTS[e.key]
      ? e.key
      : (e.code?.match(/^Digit([1-6])$/)?.[1] ?? null);
    if ((e.metaKey || e.ctrlKey) && shortcutDigit && NAV_SHORTCUTS[shortcutDigit]) {
      e.preventDefault();
      navigate(NAV_SHORTCUTS[shortcutDigit]);
      return;
    }

    // Esc: close detail panel or go up one scope level
    if (e.key === 'Escape') {
      if (shortcutsOpen) { shortcutsOpen = false; return; }
      if (userMenuOpen) { userMenuOpen = false; return; }
      if (detailPanel.open) { closeDetailPanel(); return; }
      if (scope.type === 'repo') {
        // scope.workspaceId may be absent if navigated directly via URL;
        // fall back to currentWorkspace or tenant scope.
        if (scope.workspaceId || currentWorkspace) {
          setScope({ type: 'workspace', workspaceId: scope.workspaceId ?? currentWorkspace?.id });
        } else {
          setScope({ type: 'tenant' }, 'explorer');
        }
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
    tokenVisible = false;
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

  // ── Page title ─────────────────────────────────────────────────────
  $effect(() => {
    const navLabel = currentNav ? currentNav.charAt(0).toUpperCase() + currentNav.slice(1).replace('-', ' ') : 'Gyre';
    const wsName = scope?.type !== 'tenant' ? (currentWorkspace?.name ?? '') : '';
    const repoName = scope?.type === 'repo' ? (scope.repoName ?? '') : '';
    const parts = [navLabel];
    if (repoName) parts.push(repoName);
    else if (wsName) parts.push(wsName);
    document.title = parts.length > 1 ? `${parts.join(' — ')} | Gyre` : `${parts[0]} | Gyre`;
  });

  let currentLayout = $derived.by(() => {
    if (detailPanel.open) return 'split';
    if (currentNav === 'explorer' && scope.type !== 'tenant') return 'canvas-controls';
    return 'full-width';
  });

  // Breadcrumb segments

  // ── Mount: entrypoint flow + URL routing ──────────────────────────────
  onMount(async () => {
    // 1. Load all workspaces
    try { workspaces = await api.workspaces(); } catch { /* ignore */ }

    // 2. Determine initial scope from URL or entrypoint flow
    const fromUrl = parseUrl(window.location.pathname);

    if (!fromUrl && window.location.pathname !== '/') {
      showToast('Page not found \u2014 redirecting to home', { type: 'info' });
    }

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
    window.history.replaceState({ nav: currentNav, scope: { ...scope } }, '', urlFor(currentNav, scope));

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

    function handleOutsideClick(e) {
      if (!e.target.closest('.user-menu-wrap')) userMenuOpen = false;
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
  <Sidebar bind:currentNav onnavigate={(v) => navigate(v)} {inboxBadge} {wsStatus} />

  <!-- Main area: topbar + content + status bar -->
  <div class="main">
    <!-- Topbar (48px) -->
    <header class="topbar">
      <!-- Left: scope breadcrumb (S4.8 ScopeBreadcrumb) -->
      <ScopeBreadcrumb
        tenant={{ id: 'default', name: 'Gyre' }}
        workspace={scope.type !== 'tenant' ? currentWorkspace : null}
        repo={scope.type === 'repo' && scope.repoId ? { id: scope.repoId, name: scope.repoName ?? scope.repoId } : null}
        {workspaces}
        onnavigate={onBreadcrumbNavigate}
      />

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
          aria-label={inboxBadge > 0 ? `${inboxBadge} unresolved inbox items` : 'Inbox'}
          aria-current={currentNav === 'inbox' ? 'page' : undefined}
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
        <span class="sr-only" aria-live="polite" aria-atomic="true">{inboxBadge > 0 ? `${inboxBadge} unresolved inbox items` : ''}</span>

        <!-- User avatar dropdown -->
        <div class="user-menu-wrap">
          <button
            class="user-btn"
            class:auth-active={hasToken}
            onclick={() => (userMenuOpen = !userMenuOpen)}
            aria-haspopup="menu"
            aria-expanded={userMenuOpen}
            aria-label="User menu ({hasToken ? 'authenticated' : 'not authenticated'})"
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
              aria-label="User menu"
              bind:this={userMenuEl}
              onkeydown={onUserMenuKeydown}
              onfocusout={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget)) {
                  userMenuOpen = false;
                }
              }}
            >
              <button class="user-dropdown-item" role="menuitem" tabindex="-1" onclick={() => { navigate('profile'); userMenuOpen = false; }}>
                Profile
              </button>
              <button class="user-dropdown-item" role="menuitem" tabindex="-1" onclick={() => { openTokenModal(); userMenuOpen = false; }}>
                API Token
              </button>
              <div class="user-dropdown-divider" role="separator"></div>
              <button class="user-dropdown-item" role="menuitem" tabindex="-1" onclick={() => { localStorage.removeItem('gyre_auth_token'); hasToken = false; userMenuOpen = false; }}>
                Sign out
              </button>
            </div>
          {/if}
        </div>
      </div>
    </header>

    <!-- Content area (adapts to nav + scope) -->
    <main class="content" id="main-content" tabindex="-1">
      <ContentArea layout={currentLayout} {detailPanel} onclosePanel={closeDetailPanel}>
        <div class="content-inner" class:faded={!contentVisible}>
          {#if currentNav === 'inbox'}
            <Inbox workspaceId={scope.workspaceId} scope={scope.type} />
          {:else if currentNav === 'briefing'}
            <Briefing workspaceId={scope.workspaceId} workspaceName={currentWorkspace?.name} {trustLevel} scope={scope.type} />
          {:else if currentNav === 'explorer'}
            <ExplorerView {scope} workspaceName={currentWorkspace?.name ?? null} />
          {:else if currentNav === 'specs'}
            <SpecDashboard workspaceId={scope.workspaceId ?? null} repoId={scope.repoId ?? null} scope={scope.type} />
          {:else if currentNav === 'meta-specs'}
            <MetaSpecs workspaceId={scope.workspaceId ?? null} repoId={scope.repoId ?? null} scope={scope.type} />
          {:else if currentNav === 'profile'}
            <UserProfile workspaceId={scope.workspaceId ?? null} repoId={scope.repoId ?? null} scope={scope.type} />
          {:else if currentNav === 'admin'}
            <AdminPanel workspaceId={scope.workspaceId ?? null} repoId={scope.repoId ?? null} scope={scope.type} />
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

      <!-- Spacer -->
      <span class="status-spacer"></span>

      <!-- Presence avatars (S4.8 PresenceAvatars) -->
      {#if scope.type === 'workspace' && scope.workspaceId}
        <span class="status-item status-presence">
          <PresenceAvatars workspaceId={scope.workspaceId} wsStore={wsStore} />
        </span>
      {/if}

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
        WS: {wsStatus}
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
    <div class="shortcuts-modal" bind:this={shortcutsModalEl} onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
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
        aria-label={tokenVisible ? 'Hide token' : 'Show token'}
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

  /* Topbar — height synced with --topbar-height (48px) */
  .topbar {
    display: flex;
    align-items: center;
    height: var(--topbar-height);
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
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
    padding: var(--space-1) var(--space-1);
    font-size: var(--text-xs);
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

  .search-trigger:focus-visible,
  .inbox-badge-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .inbox-count {
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

  /* User menu dropdown */
  .user-menu-wrap {
    position: relative;
  }

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

  .user-dropdown-divider {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-1) 0;
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
    transition: opacity var(--transition-fast);
  }

  .content-inner.faded {
    opacity: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .content-inner,
    .search-trigger,
    .inbox-badge-btn,
    .user-btn,
    .auth-dot,
    .user-dropdown-item,
    .budget-bar-fill,
    .ws-dot,
    .skip-to-content,
    .btn-primary,
    .btn-secondary { transition: none; }
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
    font-size: var(--text-xs);
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
    box-shadow: 0 0 4px color-mix(in srgb, var(--color-success) 50%, transparent);
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
    color: var(--color-text-inverse);
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

  .shortcuts-header button:hover {
    color: var(--color-text);
    background: color-mix(in srgb, var(--color-focus) 8%, transparent);
  }

  .shortcuts-header button:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .token-input:focus:not(:focus-visible) {
    outline: none;
  }

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

  .token-toggle:hover {
    color: var(--color-text);
  }

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
  .btn-primary:active {
    background: var(--color-primary-hover);
  }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  .user-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .user-dropdown-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    background: var(--color-border);
    color: var(--color-text);
  }
</style>
