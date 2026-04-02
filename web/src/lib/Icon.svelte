<script>
  /**
   * Icon — lightweight inline SVG icon component.
   *
   * Usage: <Icon name="check" size={14} />
   *
   * All icons are 24x24 viewBox, stroke-based (currentColor).
   */
  let { name = '', size = 14, cls = '' } = $props();

  const ICONS = {
    // Status
    check: 'M20 6L9 17l-5-5',
    x: 'M18 6L6 18M6 6l12 12',
    circle: 'M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0-18 0',
    'circle-dot': 'M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0-18 0M12 12m-1 0a1 1 0 1 0 2 0a1 1 0 1 0-2 0',
    'alert-triangle': 'M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0zM12 9v4M12 17h.01',
    // Entities
    spec: 'M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M16 13H8M16 17H8M10 9H8',
    task: 'M9 11l3 3L22 4M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11',
    agent: 'M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M12 11a4 4 0 100-8 4 4 0 000 8',
    mr: 'M18 21a3 3 0 100-6 3 3 0 000 6zM6 9a3 3 0 100-6 3 3 0 000 6zM6 9v12M18 9a9 9 0 01-9 9',
    repo: 'M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z',
    gate: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z',
    code: 'M16 18l6-6-6-6M8 6l-6 6 6 6',
    // Actions
    plus: 'M12 5v14M5 12h14',
    link: 'M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71',
    edit: 'M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z',
    search: 'M11 19a8 8 0 100-16 8 8 0 000 16zM21 21l-4.35-4.35',
    copy: 'M20 9h-9a2 2 0 00-2 2v9a2 2 0 002 2h9a2 2 0 002-2v-9a2 2 0 00-2-2zM5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1',
    // Navigation / state
    'arrow-right': 'M5 12h14M12 5l7 7-7 7',
    'chevron-right': 'M9 18l6-6-6-6',
    play: 'M5 3l14 9-14 9V3z',
    clock: 'M12 12m-10 0a10 10 0 1 0 20 0a10 10 0 1 0-20 0M12 6v6l4 2',
    refresh: 'M23 4v6h-6M1 20v-6h6M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15',
    lock: 'M19 11H5a2 2 0 00-2 2v7a2 2 0 002 2h14a2 2 0 002-2v-7a2 2 0 00-2-2zM7 11V7a5 5 0 0110 0v4',
    dollar: 'M12 1v22M17 5H9.5a3.5 3.5 0 000 7h5a3.5 3.5 0 010 7H6',
    hash: 'M4 9h16M4 15h16M10 3l-2 18M16 3l-2 18',
    eye: 'M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8zM12 12m-3 0a3 3 0 1 0 6 0a3 3 0 1 0-6 0',
    'git-merge': 'M18 21a3 3 0 100-6 3 3 0 000 6zM6 9a3 3 0 100-6 3 3 0 000 6zM6 9v12',
    activity: 'M22 12h-4l-3 9L9 3l-3 9H2',
  };

  // Support multi-path icons (paths separated by 'M' at top level)
  const pathData = ICONS[name] ?? '';
</script>

<svg
  class="icon {cls}"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={pathData}/>
</svg>

<style>
  .icon {
    display: inline-block;
    vertical-align: middle;
    flex-shrink: 0;
  }
</style>
