<script>
  /**
   * Skeleton — compatibility facade over the shadcn-svelte Skeleton
   * (lib/components/ui/skeleton/). Consumed contract unchanged:
   * width/height/radius inline-style dims, `lines` multi-line mode with
   * the last line shortened to 70%, role=status + localized loading
   * label. Styling is the Tailwind shimmer; the bespoke scoped class
   * layer is retired (`.skeleton`/`.skeleton-lines` remain as inert DOM
   * hooks).
   */
  import { t } from 'svelte-i18n';
  import { Skeleton } from './components/ui/skeleton/index.js';

  let {
    width = '100%',
    height = '1rem',
    radius = 'var(--radius)',
    lines = 1,
  } = $props();
</script>

{#if lines > 1}
  <div class="skeleton-lines flex flex-col gap-2" role="status" aria-label={$t('common.loading')}>
    {#each Array(lines) as _, i (i)}
      <Skeleton
        class="skeleton"
        style="width:{i === lines - 1 ? '70%' : '100%'}; height:{height}; border-radius:{radius}"
        aria-hidden="true"
      ></Skeleton>
    {/each}
  </div>
{:else}
  <Skeleton
    class="skeleton"
    role="status"
    aria-label={$t('common.loading')}
    style="width:{width}; height:{height}; border-radius:{radius}"
  ></Skeleton>
{/if}
