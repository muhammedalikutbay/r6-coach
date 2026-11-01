<script lang="ts">
  import { CircleAlert, CircleCheck, Info, X } from '@lucide/svelte';
  import { toasts, dismissToast } from '$lib/toast.svelte';
  const tone = { error: 'text-bad', success: 'text-good', info: 'text-accent' } as const;
</script>

<div
  class="pointer-events-none fixed right-4 bottom-4 z-[200] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2"
  aria-live="polite"
>
  {#each toasts as t (t.id)}
    <div class="animate-in fade-in slide-in-from-bottom-2 pointer-events-auto flex items-start gap-3 rounded-xl border border-border-strong bg-surface-2 p-3 shadow-2xl">
      <span class={tone[t.kind]}>
        {#if t.kind === 'error'}<CircleAlert class="size-4" />{:else if t.kind === 'success'}<CircleCheck class="size-4" />{:else}<Info class="size-4" />{/if}
      </span>
      <p class="min-w-0 flex-1 text-sm break-words">{t.text}</p>
      <button class="text-muted hover:text-foreground" aria-label="Dismiss" onclick={() => dismissToast(t.id)}>
        <X class="size-4" />
      </button>
    </div>
  {/each}
</div>
