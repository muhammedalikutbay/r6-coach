// Arenyze online stats (rank + official totals). Real Tauri commands inside the app; in a plain
// browser a fake key flow and fake profile so the UI can be seen without a key. The mock never
// talks to any network. The API key is only ever passed to `arenyze_set_key`, never logged.
import { USE_MOCK_V2 } from './api';
import type { ArenyzeStatus, OnlineProfile } from './types';

const delay = <T>(v: T, ms = 450) => new Promise<T>((r) => setTimeout(() => r(v), ms));

async function invokeCmd<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    // Never log arguments: they may hold the API key.
    console.error(`[arenyze] command "${cmd}" failed`);
    throw e;
  }
}

// ---------- mock state (browser only) ----------

const NO_KEY: ArenyzeStatus = { hasKey: false, username: null, quota: null, lastFetchedAt: null, hasRawResponse: false };
let mockStatus: ArenyzeStatus = NO_KEY;
let mockFetchedAt = Date.now() - 3 * 60_000;

function mockProfile(force: boolean): OnlineProfile {
  if (force) mockFetchedAt = Date.now();
  const stamp = new Date(mockFetchedAt).toISOString();
  return {
    source: 'arenyze',
    username: 'MockPlayer',
    season: 'Y11S3',
    rankName: 'Emerald II',
    rankPoints: 3420,
    maxRankName: 'Diamond V',
    maxRankPoints: 4010,
    topRankPosition: null,
    rankIcon: 'emerald-2',
    seasonKd: 1.12,
    kills: 1840,
    deaths: 1643,
    wins: 71,
    losses: 60,
    abandons: 2,
    matches: 131,
    level: 214,
    profilePictureUrl: null,
    rankHistory: [3050, 3120, 3080, 3190, 3260, 3210, 3340, 3420].map((rankPoints, i) => ({
      label: `Day ${i + 1}`,
      rankPoints
    })),
    operators: [
      { name: 'Twitch', side: 'attack', rounds: 47, kd: 1.08, kills: 38, deaths: 35 },
      { name: 'Mute', side: 'defense', rounds: 30, kd: 0.9, kills: 18, deaths: 20 }
    ],
    fetchedAt: stamp,
    official: {
      season: {
        scope: 'season', kd: 1.12, winPct: 54.2, kills: 1840, deaths: 1643,
        wins: 71, losses: 60, matches: 131, fetchedAt: stamp
      },
      all: {
        scope: 'all', kd: 1.05, winPct: 51.8, kills: 9120, deaths: 8686,
        wins: 342, losses: 318, matches: 660, fetchedAt: stamp
      }
    }
  };
}

// ---------- commands ----------

export const arenyzeStatus = (): Promise<ArenyzeStatus> =>
  USE_MOCK_V2 ? delay(mockStatus, 150) : invokeCmd<ArenyzeStatus>('arenyze_status');

/** Checks the key online (one call), then stores it in Windows Credential Manager. Rejects with an English message. */
export async function arenyzeSetKey(key: string): Promise<ArenyzeStatus> {
  if (!USE_MOCK_V2) return invokeCmd<ArenyzeStatus>('arenyze_set_key', { key });
  await delay(undefined, 600);
  if (key.trim().toLowerCase() === 'bad') throw 'Arenyze did not accept this key. Check that you copied the whole key, or create a new one.';
  mockStatus = {
    hasKey: true,
    username: 'MockPlayer',
    quota: { plan: 'free', used: 120, limit: 2000, remaining: 1880, checkedAt: new Date().toISOString() },
    lastFetchedAt: null,
    hasRawResponse: false
  };
  return mockStatus;
}

export async function arenyzeClearKey(): Promise<ArenyzeStatus> {
  if (!USE_MOCK_V2) return invokeCmd<ArenyzeStatus>('arenyze_clear_key');
  mockStatus = NO_KEY;
  return delay(NO_KEY, 150);
}

/** The tracked player's online profile; null without a key or before the first replay import. */
export const onlineProfile = (force: boolean): Promise<OnlineProfile | null> =>
  USE_MOCK_V2
    ? delay(mockStatus.hasKey ? mockProfile(force) : null, force ? 700 : 250)
    : invokeCmd<OnlineProfile | null>('online_profile', { force });

/** Diagnostics: saves the last raw Arenyze answers (key removed). Resolves with the saved path, or null if cancelled. */
export async function arenyzeSaveLastResponse(): Promise<string | null> {
  if (!USE_MOCK_V2) return invokeCmd<string | null>('arenyze_save_last_response');
  return delay('C:\\Users\\Mock\\Downloads\\arenyze-last-response.json', 300);
}

/** Opens the Arenyze key page in the default browser. */
export async function arenyzeOpenKeyPage(): Promise<void> {
  if (!USE_MOCK_V2) return invokeCmd<void>('arenyze_open_key_page');
  window.open('https://arenyze.com/profile#developer', '_blank', 'noopener');
}
