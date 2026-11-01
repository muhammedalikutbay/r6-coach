<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';
  let { class: cls, children, ...rest }: HTMLAttributes<HTMLDivElement> & { children?: Snippet } = $props();
</script>

<div class={cn('rounded-xl border border-border bg-surface-1 p-4', cls)} {...rest}>
  {@render children?.()}
</div>
