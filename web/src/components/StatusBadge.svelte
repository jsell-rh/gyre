<script>
  import Badge from '../lib/Badge.svelte';
  // Re-export Badge with backwards-compatible API
  let { value, type = 'status' } = $props();
</script>

<Badge {value} />
