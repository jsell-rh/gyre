import { cva } from 'class-variance-authority';

/**
 * buttonVariants — cva variant config for the shadcn-svelte Button.
 *
 * The variant names carry the consumed contract of the former bespoke
 * library (primary/secondary/ghost/danger, sm/md/lg) plus the standard
 * shadcn-svelte names (default/destructive/outline/link, icon) as aliases,
 * so both naming families resolve to the Red Hat brand tokens.
 */
export const buttonVariants = cva(
  // base — mirrors the former .btn rules (design-system.css scale values)
  'inline-flex items-center justify-center gap-1 font-sans font-medium border rounded-md cursor-pointer whitespace-nowrap select-none transition-[background-color,border-color,color,transform] duration-150 ease-out focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:opacity-50 disabled:cursor-not-allowed active:scale-[0.98] motion-reduce:transition-none motion-reduce:active:scale-100',
  {
    variants: {
      variant: {
        primary:
          'bg-primary text-primary-foreground border-primary hover:bg-primary-hover hover:border-primary-hover',
        default:
          'bg-primary text-primary-foreground border-primary hover:bg-primary-hover hover:border-primary-hover',
        secondary:
          'bg-secondary text-secondary-foreground border-border-strong hover:bg-border hover:border-muted-foreground',
        ghost:
          'bg-transparent text-muted-foreground border-transparent hover:bg-surface-elevated hover:text-foreground',
        danger:
          'bg-destructive text-destructive-foreground border-destructive hover:bg-destructive-hover hover:border-destructive-hover',
        destructive:
          'bg-destructive text-destructive-foreground border-destructive hover:bg-destructive-hover hover:border-destructive-hover',
        outline:
          'bg-transparent text-foreground border-border-strong hover:bg-surface-elevated',
        link: 'bg-transparent border-transparent text-link underline-offset-4 hover:underline',
      },
      size: {
        sm: 'px-3 py-1 text-xs',
        md: 'px-4 py-2 text-sm',
        default: 'px-4 py-2 text-sm',
        lg: 'px-5 py-3 text-base',
        icon: 'size-8 p-0',
      },
    },
    defaultVariants: {
      variant: 'primary',
      size: 'md',
    },
  }
);
