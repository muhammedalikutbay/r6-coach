// Types shared with the Rust backend (serde camelCase).

export interface OperatorUse {
  name: string;
  rounds: number;
}

export interface MatchSummary {
  matchId: string;
  timestamp: string; // ISO 8601
  map: string;
  matchType: string;
  won: boolean;
  scoreUs: number;
  scoreThem: number;
  operators: OperatorUse[];
  kills: number;
  deaths: number;
  headshots: number;
  firstBloods: number;
  firstDeaths: number;
}

export interface Advice {
  kind: 'praise' | 'improve' | 'info';
  text: string;
  evidence: string;
}

export interface LastMatchReport {
  match: MatchSummary;
  advice: Advice[];
}

export type Side = 'attack' | 'defense';

export interface OperatorRow {
  name: string;
  side: Side;
  rounds: number;
  winPct: number;
  kd: number;
  hsPct: number;
  trend: 'up' | 'down' | 'flat';
}

export interface MapRow {
  name: string;
  rounds: number;
  winPct: number;
  kd: number;
  attackWinPct: number;
  defenseWinPct: number;
}

export interface RefreshResult {
  newMatches: number;
  failed: { folder: string; error: string }[];
}

export type ReplayDirInfo =
  | { status: 'missing' }
  | { status: 'empty' }
  | { status: 'ready'; matchCount: number };
