<script lang="ts">
  // Optional small rank chip (only when an Arenyze key exists). Links to the Profile page.
  import RankIcon from '$lib/v2/components/profile/RankIcon.svelte';
  import type { OnlineProfile } from '$lib/v2/types';

  let { profile }: { profile: OnlineProfile } = $props();
</script>

{#if profile.rankName}
  <a
    href="/profile"
    title="Open your profile"
    class="inline-flex items-center gap-2 rounded-full border border-border-strong bg-surface-2 py-1 pr-3 pl-1.5 text-xs transition-colors hover:bg-surface-3"
  >
    <RankIcon icon={profile.rankIcon} name={profile.rankName} size={24} />
    <span class="font-semibold">{profile.rankName}</span>
    {#if profile.rankPoints !== null}<span class="text-muted tabular-nums">{profile.rankPoints.toLocaleString('en-GB')} RP</span>{/if}
  </a>
{/if}
