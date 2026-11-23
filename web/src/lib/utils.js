import { clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

/**
 * cn — shadcn-svelte's class composition helper.
 *
 * Merges conditional class names (clsx) and resolves Tailwind utility
 * conflicts so consumer-supplied classes override variant defaults
 * (tailwind-merge). Every component under lib/components/ui/ composes
 * its classes through this helper.
 */
export function cn(...inputs) {
  return twMerge(clsx(inputs));
}
