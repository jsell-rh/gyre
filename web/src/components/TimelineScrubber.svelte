<script>
  /**
   * TimelineScrubber — horizontal time-scrubber at the bottom of the Explorer canvas.
   * Drag to see the system at any point in history. Key moments are marked on the timeline.
   * system-explorer.md §6: Architectural Timeline.
   */
  let {
    timeline = [],            // Array of ArchitecturalDelta records from graph/timeline endpoint
    active = false,           // Whether time travel mode is active
    scrubIndex = -1,          // Current index into timeline array (-1 = present)
    nodes = [],               // Current graph nodes (for key moment marker extraction)
    onToggle = () => {},      // Callback to toggle time travel mode
    onScrub = (_idx) => {},   // Callback when scrubber position changes
    onMarkerClick = (_delta) => {}, // Callback when a key moment marker is clicked
    deltaStats = null,        // { added: N, removed: N, modified: N, byType: {} }
  } = $props();

  // Extract key moments from timeline deltas and node metadata
  let keyMoments = $derived.by(() => {
    if (!timeline?.length) return [];
    const moments = [];

    // Key moments from timeline deltas with significant changes
    for (let i = 0; i < timeline.length; i++) {
      const delta = timeline[i];
      if (!delta?.timestamp) continue;

      let parsed = null;
      try { parsed = typeof delta.delta_json === 'string' ? JSON.parse(delta.delta_json) : delta.delta_json; } catch { /* skip */ }

      const addedCount = parsed?.nodes_added?.length ?? delta.added_count ?? 0;
      const removedCount = parsed?.nodes_removed?.length ?? delta.removed_count ?? 0;

      // Determine event type
      let eventType = 'extraction';
      if (delta.spec_ref) eventType = 'spec_approval';
      else if (addedCount > 5 || removedCount > 5) eventType = 'structural_change';

      // Only mark significant events as key moments
      if (eventType !== 'extraction' || addedCount > 3 || removedCount > 3) {
        moments.push({ index: i, delta, eventType, addedCount, removedCount });
      }
    }

    // Add spec approval moments from nodes (spec_approved_at, milestone_completed_at)
    const nodeApprovals = new Map();
    for (const n of nodes) {
      if (n.spec_approved_at) {
        const ts = typeof n.spec_approved_at === 'number' ? n.spec_approved_at : Math.floor(new Date(n.spec_approved_at).getTime() / 1000);
        // Find closest timeline index
        const idx = findClosestIndex(ts);
        if (idx >= 0 && !nodeApprovals.has(ts)) {
          nodeApprovals.set(ts, true);
          moments.push({ index: idx, delta: { timestamp: ts, spec_ref: n.spec_path }, eventType: 'spec_approval', addedCount: 0, removedCount: 0 });
        }
      }
      if (n.milestone_completed_at) {
        const ts = typeof n.milestone_completed_at === 'number' ? n.milestone_completed_at : Math.floor(new Date(n.milestone_completed_at).getTime() / 1000);
        const idx = findClosestIndex(ts);
        if (idx >= 0 && !nodeApprovals.has(ts + 1)) {
          nodeApprovals.set(ts + 1, true);
          moments.push({ index: idx, delta: { timestamp: ts }, eventType: 'milestone', addedCount: 0, removedCount: 0 });
        }
      }
    }

    // Deduplicate by index proximity (merge moments within 1 index)
    moments.sort((a, b) => a.index - b.index);
    const deduped = [];
    for (const m of moments) {
      if (deduped.length > 0 && Math.abs(deduped[deduped.length - 1].index - m.index) < 2) continue;
      deduped.push(m);
    }
    return deduped;
  });

  function findClosestIndex(ts) {
    if (!timeline?.length) return -1;
    let best = 0;
    let bestDist = Infinity;
    for (let i = 0; i < timeline.length; i++) {
      const t = typeof timeline[i].timestamp === 'number' ? timeline[i].timestamp : 0;
      const d = Math.abs(t - ts);
      if (d < bestDist) { bestDist = d; best = i; }
    }
    return best;
  }

  // Current delta at the scrubber position
  let currentDelta = $derived(
    scrubIndex >= 0 && timeline?.[scrubIndex] ? timeline[scrubIndex] : null
  );

  // Format timestamp for display
  function formatDate(ts) {
    if (!ts) return '\u2014';
    const ms = typeof ts === 'number' ? ts * 1000 : new Date(ts).getTime();
    const d = new Date(ms);
    return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  }

  function formatDateTime(ts) {
    if (!ts) return '\u2014';
    const ms = typeof ts === 'number' ? ts * 1000 : new Date(ts).getTime();
    const d = new Date(ms);
    return d.toLocaleString(undefined, { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  // Marker position as percentage
  function markerPosition(idx) {
    if (!timeline?.length || timeline.length <= 1) return 0;
    return (idx / (timeline.length - 1)) * 100;
  }

  // Event type display properties
  function eventTypeLabel(type) {
    switch (type) {
      case 'spec_approval': return 'Spec Approval';
      case 'milestone': return 'Milestone';
      case 'reconciliation': return 'Reconciliation';
      case 'structural_change': return 'Structural Change';
      default: return 'Extraction';
    }
  }

  function eventTypeColor(type) {
    switch (type) {
      case 'spec_approval': return '#22c55e';
      case 'milestone': return '#3b82f6';
      case 'reconciliation': return '#a855f7';
      case 'structural_change': return '#f59e0b';
      default: return '#64748b';
    }
  }

  // Key moment detail popover state
  let selectedMoment = $state(null);
  let popoverX = $state(0);
  let popoverY = $state(0);

  function handleMarkerClick(moment, event) {
    event.stopPropagation();
    const rect = event.target.getBoundingClientRect();
    popoverX = rect.left + rect.width / 2;
    popoverY = rect.top;
    selectedMoment = moment;
    onMarkerClick(moment.delta);
  }

  function closePopover() {
    selectedMoment = null;
  }

  function handleScrubInput(e) {
    onScrub(parseInt(e.target.value));
  }
</script>

{#if timeline?.length > 0}
  <div class="timeline-scrubber-bar" role="toolbar" aria-label="Architectural timeline scrubber">
    <!-- Toggle button -->
    <button
      class="tl-toggle-btn"
      class:active
      onclick={() => onToggle()}
      type="button"
      title={active ? 'Exit time travel' : 'Time travel — scrub through architectural history'}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
        <circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>
      </svg>
      {#if !active}
        <span class="tl-toggle-label">Timeline</span>
      {/if}
    </button>

    {#if active}
      <!-- Date label at current position -->
      <span class="tl-date">{currentDelta ? formatDate(currentDelta.timestamp) : 'Present'}</span>

      <!-- Slider with markers -->
      <div class="tl-slider-container">
        <input
          type="range"
          min="0"
          max={timeline.length - 1}
          value={scrubIndex >= 0 ? scrubIndex : timeline.length - 1}
          oninput={handleScrubInput}
          class="tl-slider"
          aria-label="Timeline position"
        />
        <!-- Key moment markers -->
        <div class="tl-markers" aria-hidden="true">
          {#each keyMoments as moment}
            <button
              class="tl-marker"
              style="left: {markerPosition(moment.index)}%; background: {eventTypeColor(moment.eventType)}"
              onclick={(e) => handleMarkerClick(moment, e)}
              type="button"
              title="{eventTypeLabel(moment.eventType)} — {formatDate(moment.delta?.timestamp)}"
            ></button>
          {/each}
        </div>
      </div>

      <!-- Delta stats summary -->
      {#if deltaStats}
        <div class="tl-delta-stats">
          {#if deltaStats.added > 0}
            <span class="tl-stat-add">+{deltaStats.added}</span>
          {/if}
          {#if deltaStats.removed > 0}
            <span class="tl-stat-remove">-{deltaStats.removed}</span>
          {/if}
          {#if deltaStats.modified > 0}
            <span class="tl-stat-modify">{'\u0394'}{deltaStats.modified}</span>
          {/if}
        </div>
      {/if}

      <!-- Present label -->
      <span class="tl-present-label">
        {scrubIndex >= timeline.length - 1 || scrubIndex < 0 ? 'Now' : ''}
      </span>
    {:else}
      <!-- Inactive: show timeline range summary -->
      <span class="tl-range-summary">
        {timeline.length} change{timeline.length !== 1 ? 's' : ''}
        {#if timeline[0]?.timestamp && timeline[timeline.length - 1]?.timestamp}
          <span class="tl-range-dates">
            {formatDate(timeline[0].timestamp)} — {formatDate(timeline[timeline.length - 1].timestamp)}
          </span>
        {/if}
      </span>
      {#if keyMoments.length > 0}
        <span class="tl-key-count">{keyMoments.length} key moment{keyMoments.length !== 1 ? 's' : ''}</span>
      {/if}
    {/if}
  </div>
{/if}

<!-- Key moment detail popover -->
{#if selectedMoment}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="tl-popover-backdrop" onclick={closePopover}>
    <div
      class="tl-popover"
      style="left: {popoverX}px; top: {popoverY - 8}px"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="tl-popover-header">
        <span class="tl-popover-type" style="color: {eventTypeColor(selectedMoment.eventType)}">
          {eventTypeLabel(selectedMoment.eventType)}
        </span>
        <button class="tl-popover-close" onclick={closePopover} type="button" aria-label="Close">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10">
            <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>
      <div class="tl-popover-body">
        <div class="tl-popover-field">
          <span class="tl-popover-label">Time</span>
          <span class="tl-popover-value">{formatDateTime(selectedMoment.delta?.timestamp)}</span>
        </div>
        {#if selectedMoment.delta?.spec_ref}
          <div class="tl-popover-field">
            <span class="tl-popover-label">Spec</span>
            <span class="tl-popover-value mono">{selectedMoment.delta.spec_ref}</span>
          </div>
        {/if}
        {#if selectedMoment.delta?.agent_id}
          <div class="tl-popover-field">
            <span class="tl-popover-label">Agent</span>
            <span class="tl-popover-value mono">{selectedMoment.delta.agent_id}</span>
          </div>
        {/if}
        {#if selectedMoment.delta?.commit_sha}
          <div class="tl-popover-field">
            <span class="tl-popover-label">Commit</span>
            <code class="tl-popover-value mono">{selectedMoment.delta.commit_sha.slice(0, 7)}</code>
          </div>
        {/if}
        {#if selectedMoment.addedCount || selectedMoment.removedCount}
          <div class="tl-popover-field">
            <span class="tl-popover-label">Delta</span>
            <span class="tl-popover-value">
              {#if selectedMoment.addedCount}<span class="tl-stat-add">+{selectedMoment.addedCount} added</span>{/if}
              {#if selectedMoment.addedCount && selectedMoment.removedCount}, {/if}
              {#if selectedMoment.removedCount}<span class="tl-stat-remove">-{selectedMoment.removedCount} removed</span>{/if}
            </span>
          </div>
        {/if}
      </div>
      <div class="tl-popover-footer">
        <button
          class="tl-popover-view-btn"
          onclick={() => { onScrub(selectedMoment.index); closePopover(); }}
          type="button"
        >View at this point</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .timeline-scrubber-bar {
    position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); z-index: 35;
    display: flex; align-items: center; gap: 8px;
    padding: 6px 14px; background: rgba(15, 15, 26, 0.95); border: 1px solid #334155;
    border-radius: 8px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px); max-width: 90%;
  }

  .tl-toggle-btn {
    display: flex; align-items: center; gap: 4px;
    padding: 4px 8px; border: none; border-radius: 6px;
    background: #1e293b; color: #94a3b8; cursor: pointer;
    font-size: 11px; font-weight: 500; transition: all 0.15s;
    font-family: system-ui, -apple-system, sans-serif; white-space: nowrap;
  }
  .tl-toggle-btn:hover { background: #334155; color: #e2e8f0; }
  .tl-toggle-btn.active { background: #1d4ed8; color: #e2e8f0; }
  .tl-toggle-label { font-size: 11px; }

  .tl-date {
    font-size: 11px; color: #e2e8f0; font-family: 'SF Mono', Menlo, monospace;
    white-space: nowrap; min-width: 80px;
  }

  .tl-slider-container {
    position: relative; flex: 1; min-width: 200px; height: 20px;
    display: flex; align-items: center;
  }
  .tl-slider {
    width: 100%; height: 4px; accent-color: #3b82f6; cursor: pointer;
    position: relative; z-index: 2;
  }

  .tl-markers {
    position: absolute; top: 0; left: 0; right: 0; bottom: 0;
    pointer-events: none; z-index: 3;
  }
  .tl-marker {
    position: absolute; top: 50%; transform: translate(-50%, -50%);
    width: 8px; height: 8px; border-radius: 50%; border: 1px solid rgba(255,255,255,0.3);
    cursor: pointer; pointer-events: all; transition: transform 0.15s;
    padding: 0;
  }
  .tl-marker:hover { transform: translate(-50%, -50%) scale(1.5); }

  .tl-delta-stats {
    display: flex; gap: 6px; font-size: 11px;
    font-family: 'SF Mono', Menlo, monospace; white-space: nowrap;
  }
  .tl-stat-add { color: #4ade80; }
  .tl-stat-remove { color: #f87171; }
  .tl-stat-modify { color: #fbbf24; }

  .tl-present-label {
    font-size: 10px; color: #64748b; white-space: nowrap; min-width: 24px;
    font-family: 'SF Mono', Menlo, monospace;
  }

  .tl-range-summary {
    font-size: 11px; color: #94a3b8; white-space: nowrap;
  }
  .tl-range-dates {
    font-size: 10px; color: #64748b; margin-left: 4px;
    font-family: 'SF Mono', Menlo, monospace;
  }
  .tl-key-count {
    font-size: 10px; color: #64748b; white-space: nowrap;
    padding: 2px 6px; background: #1e293b; border-radius: 4px;
  }

  /* Key moment popover */
  .tl-popover-backdrop {
    position: fixed; top: 0; left: 0; right: 0; bottom: 0; z-index: 200;
  }
  .tl-popover {
    position: fixed; transform: translate(-50%, -100%);
    background: rgba(15, 15, 26, 0.98); border: 1px solid #334155;
    border-radius: 8px; padding: 12px; min-width: 240px; max-width: 320px;
    box-shadow: 0 12px 48px rgba(0, 0, 0, 0.8); backdrop-filter: blur(16px);
    z-index: 201;
  }
  .tl-popover-header {
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 8px; padding-bottom: 6px; border-bottom: 1px solid #1e293b;
  }
  .tl-popover-type {
    font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px;
  }
  .tl-popover-close {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 20px; background: transparent; border: none;
    border-radius: 4px; color: #64748b; cursor: pointer;
  }
  .tl-popover-close:hover { background: #1e293b; color: #e2e8f0; }
  .tl-popover-body { display: flex; flex-direction: column; gap: 4px; }
  .tl-popover-field { display: flex; gap: 8px; align-items: baseline; }
  .tl-popover-label {
    font-size: 10px; color: #64748b; text-transform: uppercase; letter-spacing: 0.3px;
    min-width: 48px; flex-shrink: 0;
  }
  .tl-popover-value { font-size: 12px; color: #e2e8f0; }
  .mono { font-family: 'SF Mono', Menlo, monospace; font-size: 11px; }
  .tl-popover-footer {
    margin-top: 8px; padding-top: 6px; border-top: 1px solid #1e293b;
  }
  .tl-popover-view-btn {
    width: 100%; padding: 5px 0; border: none; border-radius: 4px;
    background: #1d4ed8; color: #e2e8f0; font-size: 11px; font-weight: 500;
    cursor: pointer; transition: background 0.15s;
  }
  .tl-popover-view-btn:hover { background: #2563eb; }

  @media (prefers-reduced-motion: reduce) {
    .tl-toggle-btn, .tl-marker, .tl-popover-view-btn { transition: none; }
  }
</style>
