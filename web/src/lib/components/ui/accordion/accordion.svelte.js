import { cva } from 'class-variance-authority';

/**
 * accordionTriggerVariants — cva variant config for the accordion header
 * button. The header is layout-neutral by design: callers compose its look
 * via `headerClass` + scoped :global() rules in their own <style> blocks
 * (the legacy `card-header` / `section-heading` hooks).
 */
export const accordionTriggerVariants = cva(
  // base — layout-neutral button reset (the former .accordion-header rules)
  'accordion-header block w-full flex-1 min-w-0 cursor-pointer font-inherit align-inherit bg-transparent border-none p-0 m-0 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary rounded-sm'
);
