<script lang="ts">
  import { LayoutDashboard } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
</script>

<PageHeader title="Overview" />
<EmptyState
  title="Overview is coming soon"
  text="KPIs, trends, top operators and maps will appear here."
  icon={LayoutDashboard}
/>
