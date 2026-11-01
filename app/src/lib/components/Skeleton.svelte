<script lang="ts">
  import { cn } from '$lib/utils';
  let { class: cls = '' }: { class?: string } = $props();
</script>

<div class={cn('animate-pulse rounded-lg bg-surface-2', cls)} aria-hidden="true"></div>
