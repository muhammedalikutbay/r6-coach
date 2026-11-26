<script lang="ts">
  // Rank icon from the bundled assets; a letter badge when the key is unknown or the file is missing.
  let { icon, name = 'Rank', size = 48 }: { icon: string | null | undefined; name?: string; size?: number } = $props();
  let failed = $state(false);
  $effect(() => {
    void icon;
    failed = false;
  });
</script>

{#if icon && !failed}
  <img
    src="/assets/ranks/{icon}.png"
    alt={name}
    class="shrink-0 object-contain"
    style="width:{size}px;height:{size}px"
    onerror={() => (failed = true)}
  />
{:else}
  <div
    class="flex shrink-0 items-center justify-center rounded-full border border-border bg-surface-2 font-bold text-muted/70"
    style="width:{size}px;height:{size}px;font-size:{size * 0.4}px"
    aria-hidden="true"
  >
    {name.slice(0, 1).toUpperCase() || '–'}
  </div>
{/if}
