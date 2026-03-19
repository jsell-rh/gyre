<script>
  let { current = $bindable('activity') } = $props();

  const nav = [
    { id: 'activity',    label: 'Activity',    icon: '⚡' },
    { id: 'agents',      label: 'Agents',      icon: '🤖' },
    { id: 'tasks',       label: 'Tasks',       icon: '📋' },
    { id: 'projects',    label: 'Projects',    icon: '📁' },
    { id: 'merge-queue', label: 'Merge Queue', icon: '⚙' },
    { id: 'mcp-catalog', label: 'MCP Tools',   icon: '🔧' },
    { id: 'compose',     label: 'Compose',     icon: '🎛' },
    { id: 'analytics',   label: 'Analytics',   icon: '📊' },
    { id: 'costs',       label: 'Costs',       icon: '💰' },
    { id: 'admin',       label: 'Admin',       icon: '🛡' },
    { id: 'settings',    label: 'Settings',    icon: '⚙️' },
  ];
</script>

<nav class="sidebar">
  <div class="logo">
    <span class="logo-text">Gyre</span>
  </div>
  <ul>
    {#each nav as item}
      <li>
        <button
          class="nav-item"
          class:active={current === item.id || (item.id === 'projects' && (current === 'repo-detail' || current === 'mr-detail'))}
          onclick={() => (current = item.id)}
        >
          <span class="icon">{item.icon}</span>
          <span class="label">{item.label}</span>
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .sidebar {
    width: 200px;
    min-width: 200px;
    background: var(--surface);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 0;
  }

  .logo {
    padding: 1.25rem 1rem;
    border-bottom: 1px solid var(--border);
    margin-bottom: 0.5rem;
  }

  .logo-text {
    font-size: 1.25rem;
    font-weight: 700;
    color: var(--accent);
    letter-spacing: 0.05em;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 0.5rem;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    padding: 0.55rem 0.75rem;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    font-size: 0.9rem;
    text-align: left;
    transition: background 0.1s, color 0.1s;
  }

  .nav-item:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .nav-item.active {
    background: var(--accent-muted);
    color: var(--accent);
    font-weight: 500;
  }

  .icon {
    font-size: 1rem;
    width: 1.2rem;
    text-align: center;
  }
</style>
