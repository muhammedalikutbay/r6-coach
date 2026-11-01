<script lang="ts">
  import { Swords } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
</script>

<PageHeader title="Matches" />
<EmptyState
  title="Matches are coming soon"
  text="The match list and match detail will appear here."
  icon={Swords}
/>
