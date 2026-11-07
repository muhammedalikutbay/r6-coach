// Ubisoft account commands (rank + sign-in). Real Tauri commands inside the app; in a plain browser
// a fake profile and a fake sign-in flow (including the 2FA step) so the UI can be seen without an
// account. The mock never talks to any network.
import { USE_MOCK_V2 } from './api';
import type { RankedProfile, UbiLoginResult, UbiStatus } from './types';

const delay = <T>(v: T, ms = 450) => new Promise<T>((r) => setTimeout(() => r(v), ms));

async function invokeCmd<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    // Never log arguments: they may hold an email, password or 2FA code.
    console.error(`[ubisoft] command "${cmd}" failed`);
    throw e;
  }
}

// ---------- mock state (browser only) ----------

const MOCK_USER: UbiStatus = {
  signedIn: true,
  username: 'MockPlayer',
  profileId: '00000000-0000-0000-0000-000000000000',
  matchesReplayPlayer: true
};
const SIGNED_OUT: UbiStatus = { signedIn: false, username: null, profileId: null, matchesReplayPlayer: null };

let mockStatus: UbiStatus = SIGNED_OUT;
let mockFetchedAt = Date.now() - 3 * 60_000;

function mockProfile(force: boolean): RankedProfile {
  if (force) mockFetchedAt = Date.now();
  const stamp = new Date(mockFetchedAt).toISOString();
  return {
    season: 'Y11S3',
    rankName: 'Emerald II',
    rankPoints: 3420,
    maxRankName: 'Diamond V',
    maxRankPoints: 4010,
    topRankPosition: null,
    kills: 1840,
    deaths: 1643,
    wins: 71,
    losses: 60,
    abandons: 2,
    rankIcon: 'emerald-2',
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

const err = (message: string): UbiLoginResult => ({ status: 'error', message, user: null });

// ---------- commands ----------

export const ubiStatus = (): Promise<UbiStatus> =>
  USE_MOCK_V2 ? delay(mockStatus, 150) : invokeCmd<UbiStatus>('ubi_status', {});

/** Mock rules: password "wrong" fails; any other sign-in asks for a 2FA code; code "000000" fails. */
export function ubiLogin(email: string, password: string): Promise<UbiLoginResult> {
  if (!USE_MOCK_V2) return invokeCmd<UbiLoginResult>('ubi_login', { email, password });
  if (!email.trim() || !password) return delay(err('Enter your email and password.'));
  if (password === 'wrong') return delay(err('Email or password is incorrect.'));
  return delay<UbiLoginResult>({ status: 'needs2fa', message: 'Enter the code Ubisoft sent you.', user: null });
}

export function ubiLogin2fa(code: string): Promise<UbiLoginResult> {
  if (!USE_MOCK_V2) return invokeCmd<UbiLoginResult>('ubi_login_2fa', { code });
  if (!/^\d{6}$/.test(code.trim()) || code.trim() === '000000') return delay(err('That code is not valid.'));
  mockStatus = MOCK_USER;
  mockFetchedAt = Date.now();
  return delay<UbiLoginResult>({ status: 'ok', message: null, user: mockStatus });
}

export async function ubiLogout(): Promise<void> {
  if (!USE_MOCK_V2) return invokeCmd<void>('ubi_logout', {});
  mockStatus = SIGNED_OUT;
  await delay(undefined, 150);
}

export const rankedProfile = (force: boolean): Promise<RankedProfile | null> =>
  USE_MOCK_V2
    ? delay(mockStatus.signedIn ? mockProfile(force) : null, force ? 700 : 250)
    : invokeCmd<RankedProfile | null>('ranked_profile', { force });
