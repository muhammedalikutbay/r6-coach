<script lang="ts">
  import { Brain } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
</script>

<PageHeader title="Coach" />
<EmptyState
  title="Coach is coming soon"
  text="Your coach report will appear here."
  icon={Brain}
/>
