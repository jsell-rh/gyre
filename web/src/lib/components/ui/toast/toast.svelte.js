import { cva } from 'class-variance-authority';

/**
 * toastVariants — cva variant config for the shadcn-svelte Toast item.
 *
 * Each type sets a `--toast-accent` custom property (the brand status
 * color); the item's left border and icon color resolve through it.
 */
export const toastVariants = cva(
  'flex items-start gap-3 rounded-md border border-l-[3px] border-border-strong border-l-(--toast-accent) bg-surface-elevated p-3 pl-4 shadow-md pointer-events-auto [--toast-accent:var(--muted-foreground)]',
  {
    variants: {
      type: {
        info: '[--toast-accent:var(--info)]',
        success: '[--toast-accent:var(--success)]',
        error: '[--toast-accent:var(--destructive)]',
        warning: '[--toast-accent:var(--warning)]',
      },
    },
    defaultVariants: {
      type: 'info',
    },
  }
);
