<script>
  let {
    visible = false,
    onDismiss = () => {},
  } = $props();
</script>

{#if visible}
  <div class="observable-banner" role="status" aria-live="polite">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="flex-shrink:0">
      <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
    </svg>
    <span>Observable lens requires an OpenTelemetry collector. Configure <code>GYRE_OTLP_ENDPOINT</code> to see live SLIs, error rates, and latency on the architecture canvas.</span>
    <button class="observable-banner-close" onclick={onDismiss} type="button" title="Dismiss">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
    </button>
  </div>
{/if}

<style>
  .observable-banner {
    position: absolute; top: 12px; left: 50%; transform: translateX(-50%); z-index: 45;
    display: flex; align-items: center; gap: 8px;
    padding: 8px 16px; background: rgba(15, 15, 26, 0.95); border: 1px solid #334155;
    border-radius: 8px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px);
    font-size: 12px; color: #94a3b8; font-family: system-ui, -apple-system, sans-serif;
    animation: observable-fade-in 0.2s ease-out;
  }
  .observable-banner-close {
    display: flex; align-items: center; justify-content: center;
    width: 18px; height: 18px; background: transparent; border: none;
    border-radius: 4px; color: #64748b; cursor: pointer; margin-left: 4px;
  }
  .observable-banner-close:hover { background: #1e293b; color: #e2e8f0; }
  @keyframes observable-fade-in { from { opacity: 0; transform: translateX(-50%) translateY(-8px); } to { opacity: 1; transform: translateX(-50%) translateY(0); } }

  @media (prefers-reduced-motion: reduce) {
    .observable-banner { animation: none; }
  }
</style>
