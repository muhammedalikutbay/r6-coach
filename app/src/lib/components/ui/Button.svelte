<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  interface Props extends HTMLButtonAttributes {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    size?: 'sm' | 'md' | 'lg';
    ref?: HTMLButtonElement | null;
    children?: Snippet;
  }
  let { variant = 'secondary', size = 'md', class: cls, ref = $bindable(null), children, ...rest }: Props = $props();

  const variants = {
    primary: 'bg-accent text-accent-foreground hover:brightness-110 border-transparent font-semibold',
    secondary: 'bg-surface-2 hover:bg-surface-3 border-border-strong',
    ghost: 'bg-transparent hover:bg-surface-2 border-transparent text-muted hover:text-foreground',
    danger: 'bg-transparent border-bad/40 text-bad hover:bg-bad/10'
  };
  const sizes = { sm: 'h-8 px-3 text-xs', md: 'h-9 px-4', lg: 'h-11 px-7 text-base' };
</script>

<button
  bind:this={ref}
  class={cn(
    'inline-flex items-center justify-center gap-2 rounded-lg border transition disabled:opacity-50',
    variants[variant],
    sizes[size],
    cls
  )}
  {...rest}
>
  {@render children?.()}
</button>
