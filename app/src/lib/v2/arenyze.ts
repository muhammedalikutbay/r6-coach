// Arenyze online stats (rank, seasons, operators). Real Tauri commands inside the app; in a plain
// browser a fake key flow and a fake profile in the real answer shape, so the UI can be seen without
// a key. The mock never talks to any network. The API key is only ever passed to `arenyze_set_key`,
// never logged.
import { USE_MOCK_V2 } from './api';
import { normKey } from './catalog';
import type {
  ArenyzeStatus,
  OfficialOperator,
  OnlineProfile,
  OperatorMode,
  OperatorsView,
  PlaylistKey,
  PlaylistStats,
  SeasonEntry
} from './types';

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

// ---------- display helpers (pure) ----------

/** Image key of an operator name from Arenyze: accents removed ("Capitão" -> "capitao"), trimmed. */
export const opAssetKey = (name: string): string =>
  normKey(name.trim().normalize('NFD').replace(/[\u0300-\u036f]/g, ''));

/** "just now", "3 min ago", "2 h ago", "4 d ago" */
export function ago(iso: string, now = Date.now()): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return '';
  const min = Math.max(0, Math.round((now - t) / 60_000));
  if (min < 1) return 'just now';
  if (min < 60) return `${min} min ago`;
  if (min < 1440) return `${Math.round(min / 60)} h ago`;
  return `${Math.round(min / 1440)} d ago`;
}

export const PLAYLIST_LABEL: Record<PlaylistKey | 'career', string> = {
  ranked: 'Ranked',
  casual: 'Casual',
  unranked: 'Unranked',
  event: 'Event',
  warmup: 'Warmup',
  career: 'Career'
};

/** Playlist choices of the operators table, in the order of Arenyze's own page. */
export const OPERATOR_MODES: { value: OperatorMode; label: string }[] = [
  { value: 'ranked', label: 'Ranked' },
  { value: 'all', label: 'All playlists' },
  { value: 'quick-match', label: 'Casual' },
  { value: 'standard', label: 'Unranked' },
  { value: 'dual-front', label: 'Dual Front' },
  { value: 'siege-cup', label: 'Siege Cup' }
];

/** "Y11S3" -> 11 (year), or null. */
const seasonYear = (short: string): number | null => {
  const m = /^Y(\d+)S\d+$/i.exec(short.trim());
  return m ? Number(m[1]) : null;
};

/** Season choices of the operators table: all seasons, then the ones Arenyze can answer (Y8 and later). */
export function operatorSeasonOptions(seasons: SeasonEntry[]): { value: string; label: string }[] {
  const list = seasons
    .filter((s) => (seasonYear(s.shortName) ?? 0) >= 8)
    .map((s) => ({ value: s.shortName, label: s.name ? `${s.shortName} · ${s.name}` : s.shortName }));
  return [{ value: 'all', label: 'All seasons' }, ...list];
}

export const playlistOf = (season: SeasonEntry | undefined, key: PlaylistKey): PlaylistStats | undefined =>
  season?.playlists.find((p) => p.playlist === key);

// ---------- mock state (browser only) ----------

const NO_KEY: ArenyzeStatus = { hasKey: false, username: null, quota: null, lastFetchedAt: null, hasRawResponse: false };
let mockStatus: ArenyzeStatus = NO_KEY;
let mockFetchedAt = Date.now() - 3 * 60_000;

const stat = (
  playlist: PlaylistKey | 'career',
  kills: number,
  deaths: number,
  wins: number,
  losses: number,
  extra: Partial<PlaylistStats> = {}
): PlaylistStats => {
  const matches = wins + losses;
  return {
    playlist,
    rankName: null,
    rankIcon: null,
    rankPoints: null,
    maxRankName: null,
    maxRankIcon: null,
    maxRankPoints: null,
    mmr: null,
    maxMmr: null,
    kd: deaths ? Math.round((kills / deaths) * 100) / 100 : kills,
    kills,
    deaths,
    matches,
    wins,
    losses,
    abandons: null,
    winPct: matches ? Math.round((wins / matches) * 1000) / 10 : null,
    timePlayed: null,
    ...extra
  };
};

function mockSeasons(): SeasonEntry[] {
  return [
    {
      seasonId: 43, shortName: 'Y11S3', name: 'Split Fire', color: '#f5d36c',
      playlists: [
        stat('ranked', 1435, 1207, 152, 154, {
          rankName: 'Platinum V', rankIcon: 'platinum-5', rankPoints: 3007,
          maxRankName: 'Platinum III', maxRankIcon: 'platinum-3', maxRankPoints: 3261,
          abandons: 0, timePlayed: '597h'
        }),
        stat('casual', 16, 9, 1, 1, { abandons: 3, timePlayed: '2h 49m' }),
        stat('unranked', 7, 1, 1, 0, { timePlayed: '1h 46m' }),
        stat('event', 0, 3, 0, 1, { timePlayed: '25m 43s' })
      ]
    },
    {
      seasonId: 39, shortName: 'Y10S3', name: 'Silent Hunt', color: '#6fa8dc',
      playlists: [stat('ranked', 34, 51, 0, 2, { maxRankPoints: 1000, timePlayed: '5h 10m' })]
    },
    {
      seasonId: 38, shortName: 'Y10S2', name: 'High Calibre', color: '#c27ba0',
      playlists: [
        stat('ranked', 28, 43, 1, 2, {
          rankPoints: null, maxRankPoints: 1238, maxRankName: 'Copper III', maxRankIcon: 'copper-3', timePlayed: '9h 54m'
        })
      ]
    },
    {
      seasonId: 36, shortName: 'Y9S4', name: 'Collision Point', color: '#93c47d',
      playlists: [
        stat('ranked', 301, 226, 24, 24, {
          rankName: 'Emerald V', rankIcon: 'emerald-5', rankPoints: 3383,
          maxRankName: 'Emerald V', maxRankIcon: 'emerald-5', maxRankPoints: 3383
        })
      ]
    },
    {
      seasonId: 35, shortName: 'Y9S3', name: 'Twin Shells', color: '#e69138',
      playlists: [
        stat('ranked', 2290, 1760, 180, 167, {
          rankName: 'Emerald V', rankIcon: 'emerald-5', rankPoints: 3553,
          maxRankName: 'Emerald III', maxRankIcon: 'emerald-3', maxRankPoints: 3767
        }),
        stat('casual', 51, 64, 1, 3),
        stat('unranked', 190, 114, 8, 5)
      ]
    },
    {
      seasonId: 26, shortName: 'Y7S2', name: 'Heavy Mettle', color: '#8e7cc3',
      playlists: [stat('ranked', 41, 32, 3, 2, { mmr: 2884 })]
    },
    {
      seasonId: 25, shortName: 'Y7S1', name: 'Vector Glare', color: '#76a5af',
      playlists: [stat('ranked', 301, 257, 29, 27, { mmr: 2436, maxMmr: 2898 })]
    }
  ];
}

const platinumTier = (rp: number) =>
  rp < 3000 ? 'Gold I' : ['Platinum V', 'Platinum IV', 'Platinum III', 'Platinum II', 'Platinum I'][Math.min(4, Math.floor((rp - 3000) / 100))];

function mockProfile(force: boolean): OnlineProfile {
  if (force) mockFetchedAt = Date.now();
  const stamp = new Date(mockFetchedAt).toISOString();
  const seasons = mockSeasons();
  const cur = seasons[0].playlists[0];
  const history = [2860, 2905, 2890, 2950, 3010, 3070, 3041, 3099, 3150, 3214, 3261, 3200, 3140, 3098, 3052, 3037, 3007];
  return {
    source: 'arenyze',
    username: 'MockPlayer',
    season: 'Y11S3',
    rankName: cur.rankName,
    rankPoints: cur.rankPoints,
    maxRankName: cur.maxRankName,
    maxRankPoints: cur.maxRankPoints,
    topRankPosition: null,
    rankIcon: cur.rankIcon,
    seasonKd: cur.kd,
    kills: cur.kills,
    deaths: cur.deaths,
    wins: cur.wins,
    losses: cur.losses,
    abandons: 0,
    matches: cur.matches,
    level: 210,
    profilePictureUrl: null,
    rankHistory: history.map((rankPoints, i) => ({
      label: new Date(mockFetchedAt - (history.length - i) * 86_400_000).toISOString(),
      rankPoints,
      rankName: platinumTier(rankPoints)
    })),
    operators: mockOperators('Y11S3', 'ranked'),
    fetchedAt: stamp,
    official: {
      season: {
        scope: 'season', kd: cur.kd ?? 0, winPct: cur.winPct ?? 0, kills: cur.kills ?? 0, deaths: cur.deaths ?? 0,
        wins: cur.wins ?? 0, losses: cur.losses ?? 0, matches: cur.matches ?? 0, fetchedAt: stamp
      },
      all: {
        scope: 'all', kd: 1.28, winPct: 51.5, kills: 8112, deaths: 6337,
        wins: 809, losses: 746, matches: 1572, fetchedAt: stamp
      }
    },
    platform: 'PC',
    currentSeasonId: 43,
    seasonName: 'Split Fire',
    seasonColor: '#f5d36c',
    seasons,
    peaks: {
      bestRank: {
        seasonId: 35, shortName: 'Y9S3', name: 'Twin Shells', color: '#e69138', value: 3767, unit: 'RP',
        rankName: 'Emerald III', rankIcon: 'emerald-3', matches: 347
      },
      bestKd: {
        seasonId: 36, shortName: 'Y9S4', name: 'Collision Point', color: '#93c47d', value: 1.33, unit: 'K/D',
        rankName: null, rankIcon: null, matches: 48
      }
    },
    career: stat('career', 8112, 6337, 809, 746, { matches: 1572, timePlayed: '789h' })
  };
}

const MOCK_OPS: [string, 'attack' | 'defense', number][] = [
  ['Kali', 'attack', 211], ['Echo', 'defense', 190], ['Jäger', 'defense', 160], ['Lesion', 'defense', 150],
  ['Gridlock', 'attack', 140], ['Fuze', 'attack', 133], ['Zofia', 'attack', 120], ['Kapkan', 'defense', 110],
  ['Vigil', 'defense', 95], ['Azami', 'defense', 90], ['Capitão', 'attack', 62], ['Tubarão', 'defense', 41]
];

/** Deterministic fake operator table; other seasons / playlists scale the rounds so changes are visible. */
function mockOperators(season: string, mode: OperatorMode): OfficialOperator[] {
  const scale = (mode === 'ranked' ? 1 : mode === 'all' ? 1.6 : 0.2) * (season === 'all' ? 3 : season === 'Y11S3' ? 1 : 0.4);
  let seed = [...`${season}|${mode}`].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 9973, 7);
  const rnd = () => ((seed = (seed * 48271) % 2147483647) % 1000) / 1000;
  return MOCK_OPS.map(([name, side, rounds]) => {
    const r = Math.max(1, Math.round(rounds * scale));
    const kills = Math.round(r * (0.6 + rnd() * 0.7));
    const deaths = Math.max(1, Math.round(r * (0.5 + rnd() * 0.3)));
    return {
      name, side, rounds: r, kills, deaths,
      kd: Math.round((kills / deaths) * 100) / 100,
      winPct: Math.round((38 + rnd() * 25) * 10) / 10,
      hsPct: Math.round((22 + rnd() * 25) * 10) / 10,
      firstBloods: Math.round(r * 0.12 * rnd() * 2),
      firstDeaths: Math.round(r * 0.1 * rnd() * 2),
      matches: Math.max(1, Math.round(r / 1.7))
    };
  });
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

// Operator tables already fetched in this session, by (season, mode). Same 15 minutes as the backend
// cache, so picking a combination again costs neither a call nor a round trip.
const OPS_TTL_MS = 15 * 60_000;
const opsCache = new Map<string, OperatorsView>();
const opsKey = (season: string, mode: OperatorMode) => `${season}|${mode}`;

/** Seeds the cache with a table that came with the profile (current season, ranked). */
export function seedOperators(season: string, mode: OperatorMode, operators: OfficialOperator[], fetchedAt: string) {
  opsCache.set(opsKey(season, mode), { season, mode, operators, fetchedAt });
}

/** Cached table, if fresh. Never calls the network. */
export function cachedOperators(season: string, mode: OperatorMode): OperatorsView | null {
  const v = opsCache.get(opsKey(season, mode));
  return v && Date.now() - new Date(v.fetchedAt).getTime() < OPS_TTL_MS ? v : null;
}

/**
 * Operator table for one season and playlist. Only call this after the user picked the combination;
 * a fresh cached table is returned without a request.
 */
export async function onlineOperators(season: string, mode: OperatorMode): Promise<OperatorsView> {
  const hit = cachedOperators(season, mode);
  if (hit) return hit;
  const view = USE_MOCK_V2
    ? await delay<OperatorsView>(
        { season, mode, operators: mockOperators(season, mode), fetchedAt: new Date().toISOString() },
        500
      )
    : await invokeCmd<OperatorsView>('online_operators', { season, mode, force: false });
  opsCache.set(opsKey(season, mode), view);
  return view;
}

/** Forgets the cached tables (a manual profile refresh starts clean). */
export const clearOperatorCache = () => opsCache.clear();

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
