<script lang="ts">
  import type { Snippet } from 'svelte';
  import { slide } from 'svelte/transition';
  import { ChevronDown } from '@lucide/svelte';

  let { id, title, hint = '', summary = '', children }: {
    /** localStorage key suffix; the open state is remembered per id. */
    id: string;
    title: string;
    hint?: string;
    /** Short text shown next to the title while collapsed, e.g. "12 operators". */
    summary?: string;
    children: Snippet;
  } = $props();

  const KEY = $derived(`r6koc.section.${id}`);

  function load(): boolean {
    try {
      return localStorage.getItem(KEY) !== 'closed';
    } catch {
      return true;
    }
  }

  let open = $state(load());

  function toggle() {
    open = !open;
    try {
      localStorage.setItem(KEY, open ? 'open' : 'closed');
    } catch {
      /* storage unavailable: keep the in-memory state only */
    }
  }
</script>

<section class="mb-6">
  <button
    type="button"
    onclick={toggle}
    aria-expanded={open}
    class="mb-3 flex w-full items-center gap-2 rounded-lg text-left transition-colors hover:text-accent"
  >
    <ChevronDown class="size-4 shrink-0 text-muted transition-transform duration-200 {open ? '' : '-rotate-90'}" />
    <span class="min-w-0">
      <span class="text-sm font-semibold tracking-wide uppercase">{title}</span>
      {#if open && hint}<span class="ml-2 text-xs text-muted">{hint}</span>{/if}
      {#if !open && summary}<span class="ml-2 text-xs text-muted">{summary}</span>{/if}
    </span>
  </button>
  {#if open}
    <div transition:slide={{ duration: 200 }}>
      {@render children()}
    </div>
  {/if}
</section>
