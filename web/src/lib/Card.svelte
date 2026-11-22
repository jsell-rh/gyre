<script>
  /**
   * Card — compatibility facade composing the shadcn-svelte Card parts
   * (lib/components/ui/card/). Consumed contract unchanged: `class`
   * (cls), `header`/`footer` snippets, children snippet. The legacy
   * `card` / `card-header` / `card-body` / `card-footer` class names are
   * kept as inert DOM hooks (parent-scoped CSS targets them); styling
   * comes from the Tailwind utilities in the ui parts, and the bespoke
   * `.card*` scoped class layer is retired.
   */
  import { Card, CardHeader, CardContent, CardFooter } from './components/ui/card/index.js';

  let { class: cls = '', header = undefined, footer = undefined, children } = $props();
</script>

<Card class="card {cls}">
  {#if header}
    <CardHeader class="card-header">{@render header()}</CardHeader>
  {/if}
  <CardContent class="card-body">
    {@render children?.()}
  </CardContent>
  {#if footer}
    <CardFooter class="card-footer">{@render footer()}</CardFooter>
  {/if}
</Card>
