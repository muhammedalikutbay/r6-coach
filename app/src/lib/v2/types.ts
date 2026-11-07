// v2 data contract (docs/v2-spec.md §3-§5, docs/v2-contract.md). The Rust side serializes the
// same shapes with serde `rename_all = "camelCase"`.

export type Range = 'last1' | 'last5' | 'last20' | 'season' | 'all';
export type Mode = 'all' | 'ranked' | 'unranked' | 'quick';
export type Side = 'both' | 'attack' | 'defense';
export type TeamSide = 'attack' | 'defense';

/** Global filter shared by every view. `operators` and `maps` hold `key`s (lowercase ids). */
export interface Filter {
  range: Range;
  mode: Mode;
  side: Side;
  operators: string[];
  maps: string[];
  /** Local date 'YYYY-MM-DD', inclusive. */
  dateFrom: string | null;
  dateTo: string | null;
  /** Local hour 0-23, inclusive. hourFrom > hourTo wraps past midnight (22 -> 2). */
  hourFrom: number | null;
  hourTo: number | null;
  /** Minutes to add to UTC to get local time. Set by api.ts on every call, not by the UI. */
  tzOffsetMin: number;
}

export const DEFAULT_FILTER: Filter = {
  range: 'all',
  mode: 'all',
  side: 'both',
  operators: [],
  maps: [],
  dateFrom: null,
  dateTo: null,
  hourFrom: null,
  hourTo: null,
  tzOffsetMin: 0
};

export interface Kpis {
  kd: number;
  winPct: number;
  hsPct: number;
  killsPerRound: number;
  /** First kills divided by first deaths. */
  fkFdRatio: number;
  clutchPct: number;
  survivalPct: number;
  rounds: number;
  matches: number;
}

export interface TrendPoint {
  /** Match number within the filtered set, 1 = oldest. */
  index: number;
  /** ISO 8601 UTC. */
  timestamp: string;
  matchId: string;
  kd: number;
  won: boolean;
}

export interface OperatorCard {
  key: string;
  name: string;
  side: TeamSide;
  rounds: number;
  winPct: number;
  kd: number;
  hsPct: number;
  /** K/D minus the player's overall K/D. */
  deltaKd?: number;
}

export interface MapCard {
  key: string;
  name: string;
  matches: number;
  winPct: number;
  attackWinPct: number;
  defenseWinPct: number;
  kd: number;
}

export interface MatchRow {
  matchId: string;
  /** ISO 8601 UTC. */
  timestamp: string;
  mapKey: string;
  mapName: string;
  mode: string;
  won: boolean;
  scoreUs: number;
  scoreThem: number;
  kills: number;
  deaths: number;
  hsPct: number;
  /** For example '3K', '1v2 Won'. */
  badges: string[];
}

export interface OfficialTotals {
  scope: 'season' | 'all';
  kd: number;
  winPct: number;
  kills: number;
  deaths: number;
  wins: number;
  losses: number;
  matches: number;
  fetchedAt: string;
}

export interface UbiStatus {
  signedIn: boolean;
  username: string | null;
  profileId: string | null;
  matchesReplayPlayer: boolean | null;
}

export interface UbiLoginResult {
  status: 'ok' | 'needs2fa' | 'error';
  message: string | null;
  user: UbiStatus | null;
}

export interface RankedProfile {
  season: string;
  rankName: string;
  rankPoints: number;
  maxRankName: string;
  maxRankPoints: number;
  topRankPosition: number | null;
  kills: number;
  deaths: number;
  wins: number;
  losses: number;
  abandons: number;
  /** Local asset key under /assets/ranks/<key>.png, or null. */
  rankIcon: string | null;
  /** ISO UTC. */
  fetchedAt: string;
  official: { season: OfficialTotals | null; all: OfficialTotals | null };
}

export interface QueryResult {
  /** Always computed from the stored local replays. */
  source: 'replays';
  /** Official Ubisoft totals; set by the app shell when signed in and the filter is plain season/all. */
  official?: OfficialTotals | null;
  matchCount: number;
  kpis: Kpis;
  trend: TrendPoint[];
  operators: OperatorCard[];
  maps: MapCard[];
  recentMatches: MatchRow[];
}

export interface ScoreboardRow {
  username: string;
  isMe: boolean;
  /** Operator key per round, in round order. Unknown ids are kept raw. */
  operatorsByRound: string[];
  kills: number;
  deaths: number;
  hsPct: number;
  /** First kills / first deaths. */
  fk: number;
  fd: number;
  clutchesWon: number;
  clutchesLost: number;
  /** Rounds with 2+ kills. */
  multiKills: number;
  kostPct: number;
}

export interface KillEventView {
  timeS: number;
  killer: string;
  killerOperator: string;
  victim: string;
  victimOperator: string;
  headshot: boolean;
  /** 0 = my team, 1 = other team. */
  killerTeam: 0 | 1;
  /** Weapon display name, e.g. 'R4-C'. Null when unknown. */
  weapon: string | null;
  /** 'primary' | 'secondary' | 'gadget' | 'ability', or null when unknown. */
  weaponSlot: string | null;
}

export interface RoundDetail {
  number: number;
  /** 0 = my team, 1 = other team. */
  winnerTeam: 0 | 1;
  /** For example 'Eliminated enemies', 'Defused bomb', 'Planted bomb', 'Time ran out'. */
  winCondition: string;
  site: string;
  mySide: TeamSide;
  kills: KillEventView[];
}

export interface MatchDetail {
  match: MatchRow;
  /** teams[0] = my team, teams[1] = opponents. */
  teams: [ScoreboardRow[], ScoreboardRow[]];
  rounds: RoundDetail[];
}

export type InsightKind = 'praise' | 'improve' | 'info';

export interface Insight {
  kind: InsightKind;
  title: string;
  text: string;
  evidence: string;
  operatorKey?: string;
  mapKey?: string;
}

export interface Comparison {
  metric: string;
  focus: number;
  reference: number;
  delta: number;
  /** True when the focus value is better than the reference (accounts for metrics where lower is better). */
  better: boolean;
}

export interface CoachReport {
  summary: string;
  insights: Insight[];
  strengths: string[];
  workOn: string[];
  comparisons: Comparison[];
  focusLabel: string;
  referenceLabel: string;
}
