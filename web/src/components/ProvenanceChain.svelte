<script>
  /**
   * ProvenanceChain.svelte — Attestation chain visualization (§7.6)
   *
   * Displays a directed graph of the attestation chain for a commit.
   * Each node shows signer identity, constraint count, verification status.
   * Failed constraints highlighted with the failing expression and value.
   */

  let { repoId = '', commitSha = '', token = '' } = $props();

  let chain = $state(null);
  let loading = $state(false);
  let error = $state(null);
  let selectedNode = $state(null);

  async function fetchChain() {
    if (!repoId || !commitSha) return;
    loading = true;
    error = null;
    try {
      const resp = await fetch(
        `/api/v1/repos/${repoId}/attestations/${commitSha}/chain`,
        { headers: { Authorization: `Bearer ${token}` } }
      );
      if (!resp.ok) {
        const text = await resp.text();
        error = `Failed to load chain: ${resp.status} ${text}`;
        return;
      }
      chain = await resp.json();
    } catch (e) {
      error = `Network error: ${e.message}`;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    fetchChain();
  });

  function nodeColor(node) {
    if (!node.valid) return '#ef4444'; // red
    if (node.input_type === 'signed') return '#3b82f6'; // blue
    return '#22c55e'; // green
  }

  function nodeIcon(node) {
    if (node.input_type === 'signed') return 'key';
    return 'git-branch';
  }

  function formatTimestamp(ts) {
    if (!ts) return '';
    return new Date(ts * 1000).toISOString().replace('T', ' ').slice(0, 19) + 'Z';
  }
</script>

<div class="provenance-chain">
  {#if loading}
    <div class="loading">Loading attestation chain...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else if chain}
    <div class="chain-header">
      <h3>Attestation Chain</h3>
      <span class="chain-status" class:valid={chain.chain_valid} class:invalid={!chain.chain_valid}>
        {chain.chain_valid ? 'Valid' : 'Invalid'}
      </span>
      <span class="chain-info">{chain.nodes.length} node(s)</span>
    </div>

    <div class="chain-graph">
      {#each chain.nodes as node, i (node.id)}
        <div
          class="chain-node"
          class:selected={selectedNode?.id === node.id}
          class:invalid={!node.valid}
          onclick={() => selectedNode = selectedNode?.id === node.id ? null : node}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === 'Enter' && (selectedNode = selectedNode?.id === node.id ? null : node)}
        >
          <div class="node-header">
            <span class="node-type" style="background: {nodeColor(node)}">
              {node.input_type === 'signed' ? 'Root' : `D${node.chain_depth}`}
            </span>
            <span class="node-signer">{node.signer_identity}</span>
            {#if !node.valid}
              <span class="node-invalid-badge">FAILED</span>
            {/if}
          </div>
          <div class="node-meta">
            <span>{node.constraint_count} constraint(s)</span>
            {#if node.gate_count > 0}
              <span>{node.gate_count} gate(s)</span>
            {/if}
            <span class="node-time">{formatTimestamp(node.created_at)}</span>
          </div>
        </div>

        {#if i < chain.nodes.length - 1}
          <div class="chain-edge">
            <div class="edge-line"></div>
            <span class="edge-label">derives from</span>
          </div>
        {/if}
      {/each}
    </div>

    {#if selectedNode}
      <div class="node-detail">
        <h4>Node Details</h4>
        <dl>
          <dt>ID</dt><dd class="mono">{selectedNode.id.slice(0, 16)}...</dd>
          <dt>Type</dt><dd>{selectedNode.input_type}</dd>
          <dt>Signer</dt><dd>{selectedNode.signer_identity}</dd>
          <dt>Agent</dt><dd>{selectedNode.agent_id}</dd>
          <dt>Task</dt><dd>{selectedNode.task_id}</dd>
          <dt>Depth</dt><dd>{selectedNode.chain_depth}</dd>
          <dt>Constraints</dt><dd>{selectedNode.constraint_count}</dd>
          <dt>Gates</dt><dd>{selectedNode.gate_count}</dd>
          <dt>Status</dt>
          <dd class:valid={selectedNode.valid} class:invalid={!selectedNode.valid}>
            {selectedNode.valid ? 'Valid' : 'Invalid'}
          </dd>
          <dt>Message</dt><dd>{selectedNode.message}</dd>
        </dl>

        {#if selectedNode.failed_constraints.length > 0}
          <h4>Failed Constraints</h4>
          <ul class="failed-list">
            {#each selectedNode.failed_constraints as fc}
              <li class="failed-constraint">
                <strong>{fc.name}</strong>
                {#if fc.expression}
                  <code>{fc.expression}</code>
                {/if}
                <span class="fc-message">{fc.message}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  {:else}
    <div class="empty">No attestation chain found for this commit.</div>
  {/if}
</div>

<style>
  .provenance-chain {
    font-family: var(--font-mono, monospace);
    font-size: 0.85rem;
    padding: 1rem;
  }
  .chain-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 1rem;
  }
  .chain-header h3 { margin: 0; font-size: 1rem; }
  .chain-status {
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 0.75rem;
    font-weight: 600;
  }
  .chain-status.valid { background: #dcfce7; color: #166534; }
  .chain-status.invalid { background: #fef2f2; color: #991b1b; }
  .chain-info { color: var(--text-muted, #666); font-size: 0.75rem; }
  .chain-graph { display: flex; flex-direction: column; gap: 0; }
  .chain-node {
    border: 1px solid var(--border, #e5e7eb);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    cursor: pointer;
    transition: border-color 0.15s;
  }
  .chain-node:hover { border-color: var(--primary, #3b82f6); }
  .chain-node.selected { border-color: var(--primary, #3b82f6); background: var(--bg-selected, #eff6ff); }
  .chain-node.invalid { border-color: #ef4444; }
  .node-header { display: flex; align-items: center; gap: 0.5rem; }
  .node-type {
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 0.7rem;
    font-weight: 700;
    color: white;
  }
  .node-signer { font-weight: 500; }
  .node-invalid-badge {
    background: #ef4444;
    color: white;
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 0.65rem;
    font-weight: 700;
  }
  .node-meta {
    display: flex;
    gap: 0.75rem;
    margin-top: 0.25rem;
    font-size: 0.75rem;
    color: var(--text-muted, #666);
  }
  .chain-edge {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 0.25rem 0;
  }
  .edge-line {
    width: 2px;
    height: 16px;
    background: var(--border, #e5e7eb);
  }
  .edge-label {
    font-size: 0.65rem;
    color: var(--text-muted, #999);
  }
  .node-detail {
    margin-top: 1rem;
    padding: 0.75rem;
    border: 1px solid var(--border, #e5e7eb);
    border-radius: 6px;
    background: var(--bg-subtle, #f9fafb);
  }
  .node-detail h4 { margin: 0 0 0.5rem; font-size: 0.85rem; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 0.25rem 0.75rem; margin: 0; }
  dt { font-weight: 600; color: var(--text-muted, #666); }
  dd { margin: 0; }
  .mono { font-family: var(--font-mono, monospace); }
  .valid { color: #166534; }
  .invalid { color: #991b1b; }
  .failed-list { list-style: none; padding: 0; margin: 0.5rem 0 0; }
  .failed-constraint {
    padding: 0.5rem;
    margin-bottom: 0.25rem;
    background: #fef2f2;
    border-radius: 4px;
    border-left: 3px solid #ef4444;
  }
  .failed-constraint code {
    display: block;
    margin: 0.25rem 0;
    padding: 0.25rem;
    background: #fecaca;
    border-radius: 2px;
    font-size: 0.75rem;
  }
  .fc-message { display: block; font-size: 0.75rem; color: #991b1b; }
  .loading, .error, .empty {
    padding: 2rem;
    text-align: center;
    color: var(--text-muted, #666);
  }
  .error { color: #991b1b; }
</style>
