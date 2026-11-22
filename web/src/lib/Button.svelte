<script>
  /**
   * Button — compatibility facade over the shadcn-svelte Button
   * (lib/components/ui/button/). The consumed contract is unchanged:
   * variant (primary/secondary/ghost/danger), size (sm/md/lg), disabled,
   * type, onclick, aria-* passthrough, data-testid, children snippet.
   * Styling comes from the cva variant classes + Tailwind pipeline; the
   * bespoke `.btn .btn-{variant}` class layer is retired.
   */
  import { Button } from './components/ui/button/index.js';

  let {
    variant = 'primary',
    size = 'md',
    disabled = false,
    type = 'button',
    onclick = undefined,
    'aria-label': ariaLabel = undefined,
    'aria-describedby': ariaDescribedby = undefined,
    'aria-pressed': ariaPressed = undefined,
    'aria-expanded': ariaExpanded = undefined,
    'aria-busy': ariaBusy = undefined,
    'aria-controls': ariaControls = undefined,
    'data-testid': testId = undefined,
    children,
  } = $props();
</script>

<Button
  {variant}
  {size}
  {disabled}
  {type}
  {onclick}
  aria-label={ariaLabel}
  aria-describedby={ariaDescribedby}
  aria-pressed={ariaPressed}
  aria-expanded={ariaExpanded}
  aria-busy={ariaBusy}
  aria-controls={ariaControls}
  data-testid={testId}
>
  {@render children?.()}
</Button>
