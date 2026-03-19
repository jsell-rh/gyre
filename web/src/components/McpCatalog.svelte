<script>
  import { api } from '../lib/api.js';

  let tools = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let selected = $state(null);

  $effect(() => {
    api.mcpTools()
      .then((data) => { tools = data; loading = false; })
      .catch((e) => { error = e.message; loading = false; });
  });
</script>

<div class="panel">
  <div class="panel-header">
    <h2>MCP Tool Catalog</h2>
    <span class="count">{tools.length} tools</span>
  </div>

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else if tools.length === 0}
    <p class="state-msg muted">No tools registered.</p>
  {:else}
    <div class="scroll">
      <ul class="tool-list">
        {#each tools as tool (tool.name)}
          <li>
            <button
              class="tool-card"
              class:active={selected?.name === tool.name}
              onclick={() => (selected = selected?.name === tool.name ? null : tool)}
            >
              <span class="tool-name">{tool.name}</span>
              <span class="tool-desc">{tool.description ?? ''}</span>
            </button>

            {#if selected?.name === tool.name}
              <div class="tool-detail">
                <h4>Input Schema</h4>
                {#if tool.inputSchema?.properties}
                  <table class="schema-table">
                    <thead>
                      <tr><th>Parameter</th><th>Type</th><th>Required</th></tr>
                    </thead>
                    <tbody>
                      {#each Object.entries(tool.inputSchema.properties) as [param, def]}
                        <tr>
                          <td class="param-name">{param}</td>
                          <td class="param-type">{def.type ?? '—'}</td>
                          <td class="param-req">
                            {tool.inputSchema.required?.includes(param) ? '✓' : '—'}
                          </td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                {:else}
                  <p class="no-schema">No parameters.</p>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }
  h4 { margin: 0 0 0.5rem; font-size: 0.82rem; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.04em; }

  .count { font-size: 0.78rem; color: var(--text-dim); }

  .scroll { flex: 1; overflow-y: auto; padding: 0.75rem 1.25rem; }

  .tool-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.35rem; }

  .tool-card {
    display: flex; flex-direction: column; gap: 0.2rem; width: 100%;
    padding: 0.65rem 0.85rem; border-radius: 5px; text-align: left;
    background: var(--surface); border: 1px solid var(--border-subtle);
    cursor: pointer; transition: background 0.1s, border-color 0.1s;
  }
  .tool-card:hover { background: var(--surface-hover); border-color: var(--border); }
  .tool-card.active { border-color: var(--accent); background: var(--accent-muted); }

  .tool-name { font-size: 0.88rem; font-weight: 600; color: var(--text); font-family: 'Courier New', monospace; }
  .tool-desc { font-size: 0.8rem; color: var(--text-muted); }

  .tool-detail {
    margin: 0.35rem 0 0; padding: 0.75rem; background: var(--bg);
    border: 1px solid var(--border-subtle); border-radius: 0 0 5px 5px; border-top: none;
  }

  .schema-table { width: 100%; border-collapse: collapse; font-size: 0.82rem; }
  .schema-table th {
    text-align: left; padding: 0.25rem 0.5rem; color: var(--text-dim);
    font-weight: 500; border-bottom: 1px solid var(--border);
  }
  .schema-table td { padding: 0.3rem 0.5rem; border-bottom: 1px solid var(--border-subtle); }
  .param-name { color: var(--accent); font-family: monospace; }
  .param-type { color: var(--text-muted); }
  .param-req { color: #4ade80; text-align: center; }

  .no-schema { font-size: 0.82rem; color: var(--text-dim); margin: 0; font-style: italic; }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
