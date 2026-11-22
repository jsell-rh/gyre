import { cva } from 'class-variance-authority';

/**
 * badgeVariants — cva variant config for the shadcn-svelte Badge.
 *
 * Status variants use the brand status colors at 15%/30% alpha (the former
 * color-mix() recipe from the bespoke badge), rendered with Tailwind
 * opacity modifiers against the theme tokens.
 */
export const badgeVariants = cva(
  'inline-block px-2 py-1 rounded-sm font-sans text-xs font-semibold uppercase tracking-[0.04em] whitespace-nowrap border',
  {
    variants: {
      variant: {
        default: 'border-muted-foreground/30 bg-muted-foreground/15 text-muted-foreground',
        muted: 'border-muted-foreground/30 bg-muted-foreground/15 text-muted-foreground',
        success: 'border-success/30 bg-success/15 text-success',
        warning: 'border-warning/30 bg-warning/15 text-warning',
        danger: 'border-destructive/30 bg-destructive/15 text-destructive',
        destructive: 'border-destructive/30 bg-destructive/15 text-destructive',
        critical: 'border-destructive/30 bg-destructive/15 text-destructive',
        info: 'border-info/30 bg-info/15 text-info',
        purple: 'border-purple/30 bg-purple/15 text-purple',
        blocked: 'border-blocked/30 bg-blocked/15 text-blocked',
        outline: 'border-border-strong text-foreground',
        secondary: 'border-border bg-surface-elevated text-foreground',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);
