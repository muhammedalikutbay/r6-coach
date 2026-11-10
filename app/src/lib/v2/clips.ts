// Kill clips (v4): front-end wrappers of the Rust `clips` commands and events.
// Inside the Tauri window every call goes to the backend; in a plain browser (npm run dev) there
// is no recording and no clips, so the mock answers describe an idle, empty feature.
import type { ClipRow, ClipSettings, ClipsStatus, ClipsUpdated, ClipsWarning } from './types';
import { toast } from '$lib/toast.svelte';

const IN_TAURI = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// Same as `ClipSettings::default()` in Rust: 10 s before, 0 s after, offset -5 s (the recommended values).
export const DEFAULT_CLIP_SETTINGS: ClipSettings = {
  enabled: false,
  kinds: 'kills',
  keepMatches: 10,
  diskCapGb: 0,
  offsetS: -5,
  beforeS: 10,
  afterS: 0,
  recordAudio: true
};

const MOCK_STATUS: ClipsStatus = {
  recording: { state: 'off', gameRunning: false, blackFrames: false, message: '', audio: 'off' },
  bufferBytes: 0,
  clipsBytes: 0,
  clipsCount: 0,
  starredCount: 0,
  clipsDir: ''
};

let mockSettings: ClipSettings = { ...DEFAULT_CLIP_SETTINGS };

async function call<T>(cmd: string, args: Record<string, unknown> | undefined, fake: () => T): Promise<T> {
  if (!IN_TAURI) return fake();
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    console.error(`[clips] command "${cmd}" failed`, e);
    throw e;
  }
}

export const getClipSettings = () => call<ClipSettings>('get_clip_settings', undefined, () => ({ ...mockSettings }));

/** Saves the settings; the backend clamps them and returns what it stored. */
export const setClipSettings = (settings: ClipSettings) =>
  call<ClipSettings>('set_clip_settings', { settings }, () => (mockSettings = { ...settings }));

export const clipsStatus = () => call<ClipsStatus>('clips_status', undefined, () => ({ ...MOCK_STATUS }));

/** The clips of one match, newest rounds last. Empty when none were recorded. */
export const listClips = (matchId: string) => call<ClipRow[]>('list_clips', { matchId }, () => []);

export const setClipStarred = (id: number, starred: boolean) => call<void>('set_clip_starred', { id, starred }, () => undefined);

/** Deletes every clip, starred ones included. Returns how many were deleted. */
export const deleteAllClips = () => call<number>('delete_all_clips', undefined, () => 0);

export const showClipInFolder = (id: number) => call<void>('show_clip_in_folder', { id }, () => undefined);

export const openClipsFolder = () => call<void>('open_clips_folder', undefined, () => undefined);

/** URL the `<video>` element can play (Tauri asset protocol; the clips folder is in its scope). */
export async function clipUrl(path: string): Promise<string> {
  if (!IN_TAURI) return path;
  const { convertFileSrc } = await import('@tauri-apps/api/core');
  return convertFileSrc(path);
}

/**
 * A `blob:` URL of the clip, read through the backend. Fallback for when the webview refuses to
 * stream the file through the asset protocol. The caller must `URL.revokeObjectURL` it.
 */
export async function clipBlobUrl(id: number): Promise<string> {
  if (!IN_TAURI) throw new Error('Clips are only available in the app.');
  const { invoke } = await import('@tauri-apps/api/core');
  const bytes = await invoke<ArrayBuffer>('read_clip_file', { id });
  return URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
}

/** Plays the clip in the default video player of the PC. */
export const openClipExternal = (id: number) => call<void>('open_clip_external', { id }, () => undefined);

/** Human text for the `MediaError` of a `<video>` element. */
export function videoErrorText(err: MediaError | null): string {
  if (!err) return 'unknown error';
  const names: Record<number, string> = {
    1: 'MEDIA_ERR_ABORTED',
    2: 'MEDIA_ERR_NETWORK',
    3: 'MEDIA_ERR_DECODE',
    4: 'MEDIA_ERR_SRC_NOT_SUPPORTED'
  };
  return `${names[err.code] ?? 'code ' + err.code} (${err.code})${err.message ? ': ' + err.message : ''}`;
}

async function listen<T>(event: string, onEvent: (p: T) => void): Promise<() => void> {
  if (!IN_TAURI) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<T>(event, (e) => onEvent(e.payload));
}

/** Calls `cb` after clips were cut for a round. Returns the unlisten function. */
export const onClipsUpdated = (cb: (u: ClipsUpdated) => void) => listen<ClipsUpdated>('clips-updated', cb);

/** Calls `cb` for every `clips-warning` event. Returns the unlisten function. */
export const onClipsWarning = (cb: (w: ClipsWarning) => void) => listen<ClipsWarning>('clips-warning', cb);

let warnedBlack = false;

/**
 * Shows the "use borderless/windowed mode" notice once per app run, whichever page is open.
 * Call it once from a component that lives as long as the app window (the layout); the Settings
 * section shows the same hint permanently. Returns the unlisten function.
 */
export function watchClipWarnings(): Promise<() => void> {
  return onClipsWarning((w) => {
    if (w.kind === 'blackFrames' && !warnedBlack) {
      warnedBlack = true;
      toast('error', 'Clips: the game looks black. Use borderless or windowed mode in Siege.', 12000);
    }
  });
}

export const fmtBytes = (n: number): string => {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`;
  return `${n} B`;
};
