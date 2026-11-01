// Front-end API. Flip USE_MOCK to false when the Rust commands exist.
import type { LastMatchReport, MapRow, OperatorRow, RefreshResult } from './types';
import { mockMaps, mockOperators, mockRefresh, mockReplayDir, mockReport } from './mock';

export const USE_MOCK = true;

// Simulated latency so loading states are visible with mock data.
const delay = <T>(v: T, ms = 600) => new Promise<T>((r) => setTimeout(() => r(v), ms));

// Dev switches for mock states: ?mock=error | empty | partial
function mockMode(): string {
  return typeof location === 'undefined' ? '' : (new URLSearchParams(location.search).get('mock') ?? '');
}

async function call<T>(
  cmd: string,
  args: Record<string, unknown> | undefined,
  fake: () => Promise<T>
): Promise<T> {
  if (USE_MOCK) {
    if (mockMode() === 'error') {
      await delay(null, 400);
      throw new Error(`Could not run "${cmd}" (simulated error).`);
    }
    return fake();
  }
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

export const refresh = () =>
  call<RefreshResult>('refresh', undefined, () => {
    const mode = mockMode();
    if (mode === 'empty') return delay({ newMatches: 0, failed: [] }, 1200);
    if (mode === 'partial')
      return delay(
        {
          newMatches: 1,
          failed: [{ folder: 'Match-2026-10-31_02-10-05-11852', error: 'replay file is corrupted' }]
        },
        1200
      );
    return delay(mockRefresh, 1200);
  });

export const lastMatchReport = () =>
  call<LastMatchReport | null>('last_match_report', undefined, () =>
    delay(mockMode() === 'empty' ? null : mockReport)
  );

export const operatorTable = () =>
  call<OperatorRow[]>('operator_table', undefined, () => delay(mockOperators, 300));

export const mapTable = () => call<MapRow[]>('map_table', undefined, () => delay(mockMaps, 300));

export const getReplayDir = () =>
  call<string>('get_replay_dir', undefined, () => delay(mockReplayDir, 150));

export const setReplayDir = (path: string) =>
  call<void>('set_replay_dir', { path }, () => delay(undefined, 300));

export const resetDatabase = () =>
  call<void>('reset_database', undefined, () => delay(undefined, 600));
