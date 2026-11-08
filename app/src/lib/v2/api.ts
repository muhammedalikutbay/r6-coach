// v2 front-end API. Mock data in a plain browser; real Tauri `invoke` calls inside the app.
// Set USE_MOCK_V2 to `true` to force mock data even inside Tauri (dev switch).
import type {
  CoachReport,
  Filter,
  MatchDetail,
  MatchRow,
  OperatorListRow,
  OperatorScreen,
  QueryResult,
  WeaponListRow,
  WeaponScreen
} from './types';
import {
  mockCoach,
  mockListMatches,
  mockMatchDetail,
  mockOperatorList,
  mockOperatorScreen,
  mockQuery,
  mockWeaponList,
  mockWeaponScreen
} from './mock';

const FORCE_MOCK = false;
export const USE_MOCK_V2 =
  FORCE_MOCK || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window);

/** Adds the local UTC offset so the backend can apply date and hour filters in local time. */
const withTz = (filter: Filter): Filter => ({ ...filter, tzOffsetMin: -new Date().getTimezoneOffset() });

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

export const query = (filter: Filter) => call<QueryResult>('query', { filter: withTz(filter) }, () => mockQuery(filter));

export const PAGE_SIZE = 20;

export const listMatches = (filter: Filter, page: number) =>
  call<MatchRow[]>('list_matches', { filter: withTz(filter), page, pageSize: PAGE_SIZE }, () => mockListMatches(filter, page));

export const matchDetail = (matchId: string) =>
  call<MatchDetail>('match_detail', { matchId }, () => mockMatchDetail(matchId));

export const coach = (filter: Filter) => call<CoachReport>('coach', { filter: withTz(filter) }, () => mockCoach(filter));

// ---- v5 operator and weapon screens

export const operatorList = (filter: Filter) =>
  call<OperatorListRow[]>('operator_list', { filter: withTz(filter) }, () => mockOperatorList(filter));

export const weaponList = (filter: Filter) =>
  call<WeaponListRow[]>('weapon_list', { filter: withTz(filter) }, () => mockWeaponList(filter));

export const operatorDetail = (filter: Filter, key: string) =>
  call<OperatorScreen>('operator_detail', { filter: withTz(filter), key }, () => mockOperatorScreen(filter, key));

export const weaponDetail = (filter: Filter, key: string) =>
  call<WeaponScreen>('weapon_detail', { filter: withTz(filter), key }, () => mockWeaponScreen(filter, key));
