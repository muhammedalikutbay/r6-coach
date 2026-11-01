// v2 front-end API. Mock data in a plain browser; real Tauri `invoke` calls inside the app.
// Set USE_MOCK_V2 to `true` to force mock data even inside Tauri (dev switch).
import type { Filter, MatchDetail, MatchRow, QueryResult } from './types';
import { mockCoach, mockListMatches, mockMatchDetail, mockQuery } from './mock';

const FORCE_MOCK = false;
export const USE_MOCK_V2 =
  FORCE_MOCK || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window);

const delay = <T>(v: T, ms = 350) => new Promise<T>((r) => setTimeout(() => r(v), ms));

async function call<T>(cmd: string, args: Record<string, unknown>, fake: () => T): Promise<T> {
  if (USE_MOCK_V2) return delay(fake());
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    console.error(`[v2] command "${cmd}" failed`, e);
    throw e;
  }
}

export const query = (filter: Filter) => call<QueryResult>('query', { filter }, () => mockQuery(filter));

export const PAGE_SIZE = 20;

export const listMatches = (filter: Filter, page: number) =>
  call<MatchRow[]>('list_matches', { filter, page, pageSize: PAGE_SIZE }, () => mockListMatches(filter, page));

export const matchDetail = (matchId: string) =>
  call<MatchDetail>('match_detail', { matchId }, () => mockMatchDetail(matchId));

// Coach stays on mock data until the backend command is wired (r6k-aeq.14).
export const coach = (filter: Filter) => delay(mockCoach(filter));
