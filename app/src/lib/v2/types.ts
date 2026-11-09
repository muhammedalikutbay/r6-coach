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

export interface ArenyzeQuota {
  plan: string | null;
  used: number | null;
  limit: number | null;
  remaining: number | null;
  /** ISO UTC. */
  checkedAt: string;
}

export interface ArenyzeStatus {
  hasKey: boolean;
  /** Tracked player (newest stored match); null until a replay is imported. */
  username: string | null;
  quota: ArenyzeQuota | null;
  /** ISO UTC of the last successful online fetch. */
  lastFetchedAt: string | null;
  /** A raw answer is in memory and can be saved for diagnostics. */
  hasRawResponse: boolean;
}

export interface RankPoint {
  label: string | null;
  rankPoints: number;
  /** Rank at that point, for example 'Platinum V'. */
  rankName?: string | null;
}

/** Official per-operator numbers (Operators V2) for one season and playlist. */
export interface OfficialOperator {
  name: string;
  side: 'attack' | 'defense' | '';
  rounds: number;
  kd: number;
  kills: number;
  deaths: number;
  /** Round win percentage, 0-100. */
  winPct?: number | null;
  hsPct?: number | null;
  firstBloods?: number | null;
  firstDeaths?: number | null;
  matches?: number | null;
}

export type PlaylistKey = 'ranked' | 'casual' | 'unranked' | 'event' | 'warmup';

/** One playlist inside one season, or the career line (`playlist: 'career'`). */
export interface PlaylistStats {
  playlist: PlaylistKey | 'career';
  rankName: string | null;
  /** Local asset key under /assets/ranks/<key>.png. */
  rankIcon: string | null;
  rankPoints: number | null;
  maxRankName: string | null;
  maxRankIcon: string | null;
  maxRankPoints: number | null;
  /** Matchmaking rating of the seasons before RP. */
  mmr: number | null;
  maxMmr: number | null;
  kd: number | null;
  kills: number | null;
  deaths: number | null;
  matches: number | null;
  wins: number | null;
  losses: number | null;
  abandons: number | null;
  /** 0-100. */
  winPct: number | null;
  /** Text as Arenyze sends it, for example '597h'. */
  timePlayed: string | null;
}

export interface SeasonEntry {
  seasonId: number;
  /** For example 'Y11S3'. */
  shortName: string;
  /** For example 'Split Fire'. */
  name: string | null;
  /** '#rrggbb'. */
  color: string | null;
  playlists: PlaylistStats[];
}

export interface PeakEntry {
  seasonId: number;
  shortName: string;
  name: string | null;
  color: string | null;
  value: number;
  unit: 'RP' | 'MMR' | 'K/D';
  rankName: string | null;
  rankIcon: string | null;
  matches: number | null;
}

export interface SeasonPeaks {
  bestRank: PeakEntry | null;
  bestKd: PeakEntry | null;
}

/** One operator table: a (season, playlist) pair. */
export interface OperatorsView {
  /** 'Y11S3' or 'all'. */
  season: string;
  mode: OperatorMode;
  operators: OfficialOperator[];
  fetchedAt: string;
}

export type OperatorMode = 'all' | 'ranked' | 'standard' | 'quick-match' | 'dual-front' | 'siege-cup';

/** The tracked player's online profile. Fields Arenyze did not return are null. */
export interface OnlineProfile {
  source: 'arenyze';
  username: string;
  season: string | null;
  rankName: string | null;
  rankPoints: number | null;
  maxRankName: string | null;
  maxRankPoints: number | null;
  topRankPosition: number | null;
  /** Local asset key under /assets/ranks/<key>.png, or null. */
  rankIcon: string | null;
  seasonKd: number | null;
  kills: number | null;
  deaths: number | null;
  wins: number | null;
  losses: number | null;
  abandons: number | null;
  matches: number | null;
  level: number | null;
  profilePictureUrl: string | null;
  rankHistory: RankPoint[];
  operators: OfficialOperator[];
  /** ISO UTC. */
  fetchedAt: string;
  official: { season: OfficialTotals | null; all: OfficialTotals | null };
  /** 'PC', 'PlayStation' or 'Xbox'. */
  platform?: string | null;
  currentSeasonId?: number | null;
  seasonName?: string | null;
  seasonColor?: string | null;
  /** Newest season first. */
  seasons?: SeasonEntry[];
  peaks?: SeasonPeaks;
  /** All-time line. */
  career?: PlaylistStats | null;
}

export interface QueryResult {
  /** Always computed from the stored local replays. */
  source: 'replays';
  /** Official Arenyze totals (cached, no network); set when a key is saved and the filter is plain season/all. */
  official?: OfficialTotals | null;
  matchCount: number;
  kpis: Kpis;
  trend: TrendPoint[];
  operators: OperatorCard[];
  maps: MapCard[];
  recentMatches: MatchRow[];
  deep: DeepStats;
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
  /** Importance, 0 to 1.5 (info cards: 0). */
  score: number;
  /** Card family; one card per family is shown. */
  family: string;
}

export interface Comparison {
  metric: string;
  focus: number;
  reference: number;
  delta: number;
  /** True when the focus value is better than the reference (accounts for metrics where lower is better). */
  better: boolean;
  /** Importance of the difference; 0 below the noise gate. */
  score: number;
  /** Sample size behind the focus value. */
  n: number;
  unit: 'pct' | 'ratio';
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

// ---------------------------------------------------------------- kill clips (v4)

/** Which events get a clip. */
export type ClipKinds = 'kills' | 'deaths' | 'both';

export interface ClipSettings {
  /** Recording is off until the user turns it on. */
  enabled: boolean;
  kinds: ClipKinds;
  /** Keep the clips of this many newest matches; 0 = all. */
  keepMatches: number;
  /** Disk cap for all clips in GB; 0 = none. */
  diskCapGb: number;
  /** Seconds added to the kill time before the window is placed (negative = earlier). */
  offsetS: number;
  /** Clip window: seconds before the (offset) kill time ... */
  beforeS: number;
  /** ... and after it. */
  afterS: number;
  /** Record the game's sound into the clips. */
  recordAudio: boolean;
}

export type RecState = 'off' | 'waiting' | 'recording' | 'minimized' | 'error';

/** Where the sound of the current recording comes from. */
export type RecAudio = 'off' | 'game' | 'system' | 'failed';

export interface RecStatus {
  state: RecState;
  gameRunning: boolean;
  /** The capture is black: the game is probably in exclusive fullscreen. */
  blackFrames: boolean;
  message: string;
  audio: RecAudio;
}

export interface ClipsStatus {
  recording: RecStatus;
  bufferBytes: number;
  clipsBytes: number;
  clipsCount: number;
  starredCount: number;
  clipsDir: string;
}

export interface ClipRow {
  id: number;
  matchId: string;
  /** 1-based, like `RoundDetail.number`. */
  round: number;
  kind: 'kill' | 'death';
  /** Round clock value of the event: the same number as `KillEventView.timeS`. */
  timeS: number;
  killer: string;
  victim: string;
  /** Absolute path of the MP4 file. */
  path: string;
  starred: boolean;
  size: number;
  /** Where playback starts inside the file (the file begins at a keyframe before it). */
  startMs: number;
  durationMs: number;
  createdMs: number;
}

/** Payload of the `clips-updated` event. */
export interface ClipsUpdated {
  matchId: string;
  added: number;
}

/** Payload of the `clips-warning` event. */
export interface ClipsWarning {
  kind: 'blackFrames';
  message: string;
}

/** A count out of a total plus the percentage (0 when the total is 0). */
export interface RateView {
  x: number;
  n: number;
  pct: number;
}

/** How lost rounds ended on one side. `defused` also holds rounds with an unknown end. */
export interface EndsView {
  lost: number;
  elimination: number;
  time: number;
  planted: number;
  defused: number;
}

export interface WeaponView {
  name: string;
  /** 'primary' | 'secondary' | 'gadget' | 'ability', or null when unknown. */
  slot: string | null;
  kills: number;
  hsPct: number;
}

export interface RivalView {
  operatorKey: string;
  name: string;
  /** Times this enemy operator killed you. */
  deaths: number;
  /** Rounds this operator was on the enemy team. */
  faced: number;
  pct: number;
}

export interface ClutchView {
  /** Opponents alive (5 = five or more). */
  vs: number;
  won: number;
  total: number;
  pct: number;
}

export interface SlotView {
  primary: number;
  secondary: number;
  gadget: number;
  ability: number;
  melee: number;
  explosion: number;
  unknown: number;
}

/** Deeper numbers for the Overview sections "Round impact", "Weapons", "Timing" and "Rivals". */
export interface DeepStats {
  rounds: number;
  /** Opening duels won (x) out of all opening duels (n). */
  entry: RateView;
  entryAttack: RateView;
  entryDefense: RateView;
  /** Your deaths that a teammate traded. */
  traded: RateView;
  /** Teammate deaths you traded. */
  tradesGiven: RateView;
  kost: RateView;
  /** Rounds you died with no kill and no trade. */
  noImpact: RateView;
  afterDeathWin: RateView;
  afterSurviveWin: RateView;
  lostAttack: EndsView;
  lostDefense: EndsView;
  slotKills: SlotView;
  weapons: WeaponView[];
  /** Deaths in the first 30 s of the round clock. */
  earlyDeath: RateView;
  /** Deaths in the last 30 s. */
  lateDeath: RateView;
  earlyKill: RateView;
  lateKill: RateView;
  rivals: RivalView[];
  clutchByOpponents: ClutchView[];
}

// ---- v5 operator and weapon screens (core: `r6k_core::details`, app: `details.rs`)

/** Numbers of one operator under the current filter. */
export interface DetailStats {
  matches: number;
  rounds: number;
  roundsWon: number;
  winPct: number;
  kills: number;
  deaths: number;
  kd: number;
  killsPerRound: number;
  hsPct: number;
  firstKills: number;
  firstDeaths: number;
  kostPct: number;
  survivalPct: number;
  clutchWon: number;
  clutchLost: number;
  clutchPct: number;
}

export interface DetailSideRow {
  side: TeamSide;
  rounds: number;
  winPct: number;
  kd: number;
}

export interface DetailMapRow {
  key: string;
  name: string;
  rounds: number;
  winPct: number;
  kills: number;
  deaths: number;
  kd: number;
}

export interface DetailWeaponUse {
  key: string;
  name: string;
  /** Not a named item (an operator-and-slot label such as "Noor primary"). */
  unnamed?: boolean;
  slot: string | null;
  kills: number;
  hsPct: number;
}

export interface DetailRival {
  operatorKey: string;
  name: string;
  count: number;
}

export interface DetailOperatorMatch {
  matchId: string;
  timestamp: string;
  mapKey: string;
  mapName: string;
  won: boolean;
  scoreUs: number;
  scoreThem: number;
  rounds: number;
  kills: number;
  deaths: number;
}

export interface OperatorDetail {
  key: string;
  name: string;
  /** `null` when the operator has no rounds under the filter. */
  side: TeamSide | null;
  stats: DetailStats;
  sides: DetailSideRow[];
  maps: DetailMapRow[];
  weapons: DetailWeaponUse[];
  mostKilled: DetailRival[];
  mostKilledBy: DetailRival[];
  /** Newest first. */
  matches: DetailOperatorMatch[];
  /** Rolling K/D, oldest first. */
  trend: TrendPoint[];
}

export interface WeaponOperatorRow {
  key: string;
  name: string;
  kills: number;
  hsPct: number;
}

export interface WeaponMapRow {
  key: string;
  name: string;
  kills: number;
}

export interface WeaponTiming {
  early: number;
  mid: number;
  late: number;
}

export interface WeaponRecentKill {
  matchId: string;
  timestamp: string;
  mapKey: string;
  mapName: string;
  round: number;
  timeS: number;
  operatorKey: string;
  operatorName: string;
  victimOperatorKey: string;
  victimOperatorName: string;
  headshot: boolean;
}

export interface WeaponDetail {
  key: string;
  name: string;
  unnamed?: boolean;
  slot: string | null;
  kills: number;
  headshots: number;
  hsPct: number;
  /** Rounds spent on operators that made a kill with this weapon. */
  rounds: number;
  killsPerRound: number;
  operators: WeaponOperatorRow[];
  maps: WeaponMapRow[];
  timing: WeaponTiming;
  recentKills: WeaponRecentKill[];
}

export interface OperatorListRow {
  key: string;
  name: string;
  side: TeamSide;
  matches: number;
  rounds: number;
  winPct: number;
  kills: number;
  deaths: number;
  kd: number;
  killsPerRound: number;
  hsPct: number;
  kostPct: number;
}

export interface WeaponListRow {
  key: string;
  name: string;
  unnamed?: boolean;
  slot: string | null;
  kills: number;
  headshots: number;
  hsPct: number;
  operators: number;
}

/** A recorded clip that belongs to a kill or death shown on a detail screen. */
export interface DetailClip {
  clip: ClipRow;
  headshot: boolean;
  weapon: string | null;
  mapKey: string;
  /** Operator you used. */
  operatorKey: string;
  /** Operator of the other player; empty when unknown. */
  enemyOperatorKey: string;
}

export interface OperatorScreen {
  detail: OperatorDetail;
  clips: DetailClip[];
}

export interface WeaponScreen {
  detail: WeaponDetail;
  clips: DetailClip[];
}
