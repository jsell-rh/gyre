import { cva } from 'class-variance-authority';

/**
 * tabTriggerVariants — cva variant config for the shadcn-svelte Tabs trigger.
 *
 * The tab trigger is a real <button role="tab"> in the WAI-ARIA Tabs
 * (manual activation) pattern; the active state is carried by the
 * `active` variant (aria-selected + data-state are set by the component).
 */
export const tabTriggerVariants = cva(
  // base — mirrors the former .tab-btn rules
  'flex items-center gap-2 px-4 py-3 -mb-px bg-transparent font-sans text-sm font-medium cursor-pointer whitespace-nowrap border border-solid border-x-0 border-t-0 border-b-2 transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:opacity-50 disabled:cursor-not-allowed motion-reduce:transition-none',
  {
    variants: {
      active: {
        true: 'text-foreground border-b-link',
        false: 'text-muted-foreground border-b-transparent hover:text-foreground',
      },
    },
    defaultVariants: {
      active: false,
    },
  }
);
