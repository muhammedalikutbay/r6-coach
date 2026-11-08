<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import { toast, toastError } from '$lib/toast.svelte';
  import { USE_MOCK_V2 } from '../api';

  let busy = $state(false);

  /** Writes the unknown map, operator and weapon ids of the stored matches to a file the user picks. */
  async function exportIds() {
    busy = true;
    try {
      if (USE_MOCK_V2) {
        toast('success', 'Saved the unknown ids (mock).');
        return;
      }
      const { invoke } = await import('@tauri-apps/api/core');
      const path = await invoke<string | null>('export_unknown_ids');
      if (path) toast('success', 'Saved the unknown ids. Send that file to the developers.');
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Card>
  <h3 class="font-semibold">Unknown ids</h3>
  <p class="mt-1 mb-3 text-muted">
    After a game update some maps, operators or weapons can show up without a name. This saves
    those ids, with counts and bomb site names, to a file. It holds no player names.
  </p>
  <Button onclick={exportIds} disabled={busy}>Export unknown ids</Button>
</Card>
