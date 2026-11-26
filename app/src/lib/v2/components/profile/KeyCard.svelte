<script lang="ts">
  // What Home shows when the online profile is not there: no key, no replay yet, or a failed load.
  import { KeyRound, RefreshCw, ShieldQuestion, TriangleAlert } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';

  let { kind, error = null, refreshing = false, onretry }: {
    kind: 'no-key' | 'no-player' | 'failed';
    error?: string | null;
    refreshing?: boolean;
    onretry?: () => void;
  } = $props();
</script>

{#if kind === 'no-key'}
  <Card class="flex flex-wrap items-center gap-4">
    <KeyRound class="size-9 shrink-0 text-accent" />
    <div class="min-w-0 flex-1">
      <div class="font-semibold">Add your free Arenyze key</div>
      <p class="text-xs text-muted">
        Your rank, RP, season history and official operator stats appear here. The key is free, takes a minute to
        create, and stays on this PC. Everything below works without it.
      </p>
    </div>
    <a href="/settings"><Button variant="primary">Open Settings</Button></a>
  </Card>
{:else if kind === 'no-player'}
  <Card class="flex items-center gap-3">
    <ShieldQuestion class="size-8 shrink-0 text-muted" />
    <div class="min-w-0">
      <div class="font-semibold">Import a match to see your online stats</div>
      <div class="text-xs text-muted">The app reads your Ubisoft name from your newest replay.</div>
    </div>
  </Card>
{:else}
  <Card class="flex items-center gap-3">
    <TriangleAlert class="size-8 shrink-0 text-accent" />
    <div class="min-w-0 flex-1">
      <div class="font-semibold">Could not load your online stats</div>
      <div class="text-xs text-muted">{error ?? 'No answer from Arenyze yet.'}</div>
    </div>
    <button
      type="button"
      onclick={onretry}
      disabled={refreshing}
      title="Try again"
      aria-label="Try again"
      class="rounded-lg p-1.5 text-muted transition-colors hover:bg-surface-3 hover:text-foreground disabled:opacity-50"
    >
      <RefreshCw class="size-4 {refreshing ? 'animate-spin' : ''}" />
    </button>
  </Card>
{/if}
