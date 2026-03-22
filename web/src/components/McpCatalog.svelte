<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Badge from '../lib/Badge.svelte';

  let tools = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let expandedTool = $state(null);

  $effect(() => {
    api.mcpTools()
      .then((data) => { tools = data; loading = false; })
      .catch((e) => { error = e.message; loading = false; });
  });

  function toggle(name) {
    expandedTool = expandedTool === name ? null : name;
  }

  function schemaToJson(schema) {
    try {
      return JSON.stringify(schema, null, 2);
    } catch {
      return '{}';
    }
  }
</script>

<div class="panel">
  <div class="panel-header">
    <div class="header-left">
      <h2>MCP Tool Catalog</h2>
      {#if !loading && tools.length > 0}
        <span class="tool-count">{tools.length} tools</span>
      {/if}
    </div>
  </div>

  <div class="scroll">
    {#if loading}
      <div class="tool-grid">
        {#each Array(6) as _}
          <div class="skeleton-card">
            <Skeleton height="1rem" width="60%" />
            <Skeleton height="0.75rem" lines={2} />
          </div>
        {/each}
      </div>
    {:else if error}
      <div class="error-msg">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
          <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
        </svg>
        {error}
      </div>
    {:else if tools.length === 0}
      <EmptyState
        title="No tools registered"
        description="MCP tools registered with the server will appear here."
      />
    {:else}
      <div class="tool-grid">
        {#each tools as tool (tool.name)}
          <div class="tool-card" class:expanded={expandedTool === tool.name}>
            <button class="tool-header" onclick={() => toggle(tool.name)}>
              <div class="tool-title-row">
                <code class="tool-name">{tool.name}</code>
                <span class="expand-icon" class:rotated={expandedTool === tool.name}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
                    <path d="M6 9l6 6 6-6"/>
                  </svg>
                </span>
              </div>
              {#if tool.description}
                <p class="tool-desc">{tool.description}</p>
              {/if}
              <div class="tool-meta">
                {#if tool.inputSchema?.properties}
                  <Badge value={`${Object.keys(tool.inputSchema.properties).length} params`} variant="info" />
                {:else}
                  <Badge value="no params" variant="muted" />
                {/if}
              </div>
            </button>

            {#if expandedTool === tool.name}
              <div class="tool-detail">
                <h4 class="detail-title">Input Schema</h4>

                {#if tool.inputSchema?.properties}
                  <table class="schema-table">
                    <thead>
                      <tr>
                        <th>Parameter</th>
                        <th>Type</th>
                        <th>Required</th>
                        <th>Description</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each Object.entries(tool.inputSchema.properties) as [param, def]}
                        <tr>
                          <td class="param-name">{param}</td>
                          <td class="param-type">{def.type ?? '—'}</td>
                          <td class="param-req">
                            {#if tool.inputSchema.required?.includes(param)}
                              <span class="req-yes">✓</span>
                            {:else}
                              <span class="req-no">—</span>
                            {/if}
                          </td>
                          <td class="param-desc">{def.description ?? ''}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                {:else}
                  <p class="no-schema">No parameters.</p>
                {/if}

                <details class="raw-schema">
                  <summary class="raw-schema-toggle">Raw JSON Schema</summary>
                  <pre class="schema-code">{schemaToJson(tool.inputSchema)}</pre>
                </details>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .tool-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
  }

  .error-msg {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  /* Tool grid */
  .tool-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: var(--space-4);
    align-items: start;
  }

  .skeleton-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }

  /* Tool cards */
  .tool-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
    transition: border-color var(--transition-fast);
  }

  .tool-card:hover {
    border-color: var(--color-border-strong);
  }

  .tool-card.expanded {
    border-color: var(--color-link);
    grid-column: 1 / -1;
  }

  .tool-header {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-4);
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .tool-header:hover {
    background: var(--color-surface-elevated);
  }

  .tool-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .tool-name {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .expand-icon {
    color: var(--color-text-muted);
    transition: transform var(--transition-fast);
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .expand-icon.rotated {
    transform: rotate(180deg);
  }

  .tool-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
  }

  .tool-meta {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  /* Tool detail */
  .tool-detail {
    border-top: 1px solid var(--color-border);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    background: var(--color-bg);
  }

  .detail-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
  }

  .schema-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .schema-table th {
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .schema-table td {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .schema-table tbody tr:last-child td { border-bottom: none; }

  .param-name {
    font-family: var(--font-mono);
    color: var(--color-link);
    font-size: var(--text-xs);
  }

  .param-type {
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .req-yes { color: var(--color-success); font-weight: 600; }
  .req-no  { color: var(--color-text-muted); }

  .param-desc {
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
  }

  .no-schema {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  /* Raw schema collapsible */
  .raw-schema { }

  .raw-schema-toggle {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    cursor: pointer;
    user-select: none;
    list-style: none;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) 0;
  }

  .raw-schema-toggle:hover { color: var(--color-text-secondary); }

  .schema-code {
    margin-top: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow-x: auto;
    white-space: pre;
    line-height: 1.6;
  }
</style>
