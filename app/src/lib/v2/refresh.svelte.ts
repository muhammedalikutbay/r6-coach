// Bumped after a successful `refresh`; views read `refreshTick.value` inside their load effect to re-query.
class RefreshTick {
  value = $state(0);
  bump() {
    this.value += 1;
  }
}

export const refreshTick = new RefreshTick();

/** Payload of the backend's `replays-updated` event, sent after every import (manual or automatic). */
export interface ReplaysUpdated {
  newMatches: number;
  updatedMatches: number;
  pending: number;
  failed: { folder: string; error: string }[];
}

/**
 * Calls `onUpdate` on every `replays-updated` event. Returns the unlisten function.
 * Outside the Tauri window (plain browser with mock data) there are no events: a no-op.
 */
export async function onReplaysUpdated(onUpdate: (r: ReplaysUpdated) => void): Promise<() => void> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<ReplaysUpdated>('replays-updated', (e) => onUpdate(e.payload));
}
