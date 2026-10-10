<script>
  /**
   * CanvasProbeStub.svelte — test stub for ExplorerCanvas used by
   * WorkspaceHomeScopeDrill.test.js. Captures the callback props the
   * real parent passes and renders trigger buttons so tests can fire
   * single-click (onNodeDetail) and double-click (onScopeDrill)
   * through the real parent wiring.
   */
  let {
    nodes = [],
    edges = [],
    onNodeDetail = () => {},
    onScopeDrill = null,
  } = $props();

  // Fixed probe node: a workspace-scope repo node (repo_id set).
  const node = { id: 'node-a', repo_id: 'repo-123', name: 'payment-api', node_type: 'module' };
</script>

<div data-testid="canvas-probe">
  <button
    type="button"
    data-testid="probe-single-click"
    onclick={() => onNodeDetail?.(node)}
  >single</button>
  <button
    type="button"
    data-testid="probe-scope-drill"
    onclick={() => onScopeDrill?.(node)}
  >drill</button>
</div>
