// Realistic fake data for the v2 UI (browser/dev only). Values are modeled on the tracked player's
// real stats, but all opponent names are made up.
import type {
  CoachReport,
  DetailClip,
  DetailStats,
  DeepStats,
  Filter,
  Kpis,
  KillEventView,
  MapCard,
  MatchDetail,
  MatchRow,
  OfficialTotals,
  OperatorCard,
  OperatorListRow,
  OperatorScreen,
  QueryResult,
  RoundDetail,
  ScoreboardRow,
  TeamSide,
  TrendPoint,
  WeaponListRow,
  WeaponScreen
} from './types';

const ME = 'H.A.N-01';

// Deterministic PRNG so the mock does not change between reloads.
function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const r1 = (n: number) => Math.round(n * 10) / 10;
const r2 = (n: number) => Math.round(n * 100) / 100;

export const MOCK_OPERATORS: { key: string; name: string; side: TeamSide }[] = [
  { key: 'rook', name: 'Rook', side: 'defense' },
  { key: 'jager', name: 'Jäger', side: 'defense' },
  { key: 'buck', name: 'Buck', side: 'attack' },
  { key: 'zofia', name: 'Zofia', side: 'attack' },
  { key: 'osa', name: 'Osa', side: 'attack' },
  { key: 'kapkan', name: 'Kapkan', side: 'defense' },
  { key: 'ash', name: 'Ash', side: 'attack' },
  { key: 'mute', name: 'Mute', side: 'defense' }
];

export const MOCK_MAPS: { key: string; name: string }[] = [
  { key: 'outback', name: 'Outback' },
  { key: 'oregon', name: 'Oregon' },
  { key: 'kanal', name: 'Kanal' },
  { key: 'bank', name: 'Bank' },
  { key: 'clubhouse', name: 'Clubhouse' }
];

const MODES = ['Ranked', 'Ranked', 'Ranked', 'Unranked', 'Quick Match'];
const MOCK_WEAPONS = ['R4-C', 'MP5', 'AK-12', 'P90', 'SG-CQB'];
const DAY = 86_400_000;
const NOW = Date.UTC(2026, 9, 31, 22, 0, 0);

function badgesFor(i: number): string[] {
  const all: string[][] = [['3K'], [], ['1v2 Won'], ['2K', '1v1 Won'], [], ['4K'], [], ['1v3 Won'], []];
  return all[i % all.length];
}

function buildMatches(): MatchRow[] {
  const rand = rng(7);
  const rows: MatchRow[] = [];
  for (let i = 0; i < 24; i++) {
    const map = MOCK_MAPS[Math.floor(rand() * MOCK_MAPS.length)];
    const won = rand() < 0.5;
    const loserScore = Math.floor(rand() * 4);
    const kills = 4 + Math.floor(rand() * 14);
    const deaths = 3 + Math.floor(rand() * 11);
    rows.push({
      matchId: `Match-mock-${String(i + 1).padStart(2, '0')}`,
      // Newest first: ~3 matches per day, roughly 40 minutes apart.
      timestamp: new Date(NOW - Math.floor(i / 3) * DAY - (i % 3) * 2_400_000).toISOString(),
      mapKey: map.key,
      mapName: map.name,
      mode: MODES[i % MODES.length],
      won,
      scoreUs: won ? 4 : loserScore,
      scoreThem: won ? loserScore : 4,
      kills,
      deaths,
      hsPct: r1(25 + rand() * 30),
      badges: badgesFor(i)
    });
  }
  return rows;
}

const ALL_MATCHES = buildMatches();

function applyFilter(f: Filter): MatchRow[] {
  let rows = ALL_MATCHES.slice();
  if (f.mode !== 'all') {
    const want = f.mode === 'ranked' ? 'Ranked' : f.mode === 'unranked' ? 'Unranked' : 'Quick Match';
    rows = rows.filter((m) => m.mode === want);
  }
  if (f.maps.length) rows = rows.filter((m) => f.maps.includes(m.mapKey));
  if (f.dateFrom || f.dateTo || f.hourFrom !== null || f.hourTo !== null) {
    const off = -new Date().getTimezoneOffset() * 60_000;
    rows = rows.filter((m) => {
      const local = new Date(Date.parse(m.timestamp) + off).toISOString();
      const day = local.slice(0, 10);
      const hour = Number(local.slice(11, 13));
      if (f.dateFrom && day < f.dateFrom) return false;
      if (f.dateTo && day > f.dateTo) return false;
      const hf = f.hourFrom ?? 0;
      const ht = f.hourTo ?? 23;
      return hf <= ht ? hour >= hf && hour <= ht : hour >= hf || hour <= ht;
    });
  }
  const limit = { last1: 1, last5: 5, last20: 20, season: 24, all: Infinity }[f.range];
  return rows.slice(0, limit);
}

function kpisFor(rows: MatchRow[]): Kpis {
  const matches = rows.length;
  const kills = rows.reduce((a, m) => a + m.kills, 0);
  const deaths = rows.reduce((a, m) => a + m.deaths, 0);
  const rounds = rows.reduce((a, m) => a + m.scoreUs + m.scoreThem, 0);
  const wins = rows.filter((m) => m.won).length;
  return {
    kd: r2(kills / Math.max(deaths, 1)),
    winPct: r1((wins / Math.max(matches, 1)) * 100),
    hsPct: r1(rows.reduce((a, m) => a + m.hsPct, 0) / Math.max(matches, 1)),
    killsPerRound: r2(kills / Math.max(rounds, 1)),
    fkFdRatio: 0.92,
    clutchPct: 14.3,
    survivalPct: 38.6,
    rounds,
    matches
  };
}

const OPERATOR_STATS: Omit<OperatorCard, 'deltaKd'>[] = [
  { key: 'rook', name: 'Rook', side: 'defense', rounds: 142, winPct: 52.1, kd: 1.12, hsPct: 41.3 },
  { key: 'zofia', name: 'Zofia', side: 'attack', rounds: 118, winPct: 49.2, kd: 1.05, hsPct: 36.8 },
  { key: 'buck', name: 'Buck', side: 'attack', rounds: 96, winPct: 54.2, kd: 1.21, hsPct: 29.5 },
  { key: 'jager', name: 'Jäger', side: 'defense', rounds: 74, winPct: 47.3, kd: 0.94, hsPct: 33.1 },
  { key: 'osa', name: 'Osa', side: 'attack', rounds: 52, winPct: 50.0, kd: 0.88, hsPct: 38.0 },
  { key: 'kapkan', name: 'Kapkan', side: 'defense', rounds: 31, winPct: 45.2, kd: 0.79, hsPct: 27.4 }
];

function operatorsFor(f: Filter, overallKd: number): OperatorCard[] {
  let ops = OPERATOR_STATS.slice();
  if (f.side !== 'both') ops = ops.filter((o) => o.side === f.side);
  if (f.operators.length) ops = ops.filter((o) => f.operators.includes(o.key));
  return ops.map((o) => ({ ...o, deltaKd: r2(o.kd - overallKd) }));
}

const MAP_STATS: MapCard[] = [
  { key: 'outback', name: 'Outback', matches: 9, winPct: 55.6, attackWinPct: 58.2, defenseWinPct: 51.0, kd: 1.11 },
  { key: 'oregon', name: 'Oregon', matches: 7, winPct: 42.9, attackWinPct: 40.0, defenseWinPct: 46.2, kd: 0.93 },
  { key: 'kanal', name: 'Kanal', matches: 6, winPct: 50.0, attackWinPct: 47.1, defenseWinPct: 53.8, kd: 1.02 },
  { key: 'bank', name: 'Bank', matches: 5, winPct: 60.0, attackWinPct: 52.9, defenseWinPct: 66.7, kd: 1.18 },
  { key: 'clubhouse', name: 'Clubhouse', matches: 3, winPct: 33.3, attackWinPct: 35.0, defenseWinPct: 31.6, kd: 0.81 }
];

function trendFor(rows: MatchRow[]): TrendPoint[] {
  // Oldest first, with a rolling K/D over up to 5 matches.
  const asc = rows.slice().reverse();
  return asc.map((m, i) => {
    const win = asc.slice(Math.max(0, i - 4), i + 1);
    const k = win.reduce((a, x) => a + x.kills, 0);
    const d = win.reduce((a, x) => a + x.deaths, 0);
    return { index: i + 1, timestamp: m.timestamp, matchId: m.matchId, kd: r2(k / Math.max(d, 1)), won: m.won };
  });
}

function mockOfficial(scope: 'season' | 'all'): OfficialTotals {
  const season = scope === 'season';
  return {
    scope,
    kd: season ? 1.12 : 1.05,
    winPct: season ? 54.2 : 51.8,
    kills: season ? 1840 : 9120,
    deaths: season ? 1643 : 8686,
    wins: season ? 71 : 342,
    losses: season ? 60 : 318,
    matches: season ? 131 : 660,
    fetchedAt: new Date().toISOString()
  };
}

const rate = (x: number, n: number) => ({ x, n, pct: n ? Math.round((1000 * x) / n) / 10 : 0 });

function mockDeep(matches: number): DeepStats {
  const k = Math.max(1, matches);
  const rounds = 20 * k;
  return {
    rounds,
    entry: rate(7 * k, 15 * k),
    entryAttack: rate(3 * k, 8 * k),
    entryDefense: rate(4 * k, 7 * k),
    traded: rate(4 * k, 12 * k),
    tradesGiven: rate(5 * k, 25 * k),
    kost: rate(13 * k, rounds),
    noImpact: rate(7 * k, rounds),
    afterDeathWin: rate(3 * k, 12 * k),
    afterSurviveWin: rate(6 * k, 8 * k),
    lostAttack: { lost: 5 * k, elimination: 2 * k, time: 2 * k, planted: 0, defused: k },
    lostDefense: { lost: 4 * k, elimination: 2 * k, time: 0, planted: 2 * k, defused: 0 },
    slotKills: { primary: 11 * k, secondary: 2 * k, gadget: 0, ability: 0, melee: 0, explosion: 0, unknown: k },
    weapons: [
      { name: 'R4-C', slot: 'primary', kills: 8 * k, hsPct: 50 },
      { name: 'MP5', slot: 'primary', kills: 3 * k, hsPct: 33.3 },
      { name: 'P9', slot: 'secondary', kills: 2 * k, hsPct: 50 }
    ],
    earlyDeath: rate(2 * k, 12 * k),
    lateDeath: rate(3 * k, 12 * k),
    earlyKill: rate(3 * k, 14 * k),
    lateKill: rate(4 * k, 14 * k),
    rivals: [
      { operatorKey: 'jager', name: 'Jäger', deaths: 3 * k, faced: 14 * k, pct: 21.4 },
      { operatorKey: 'ash', name: 'Ash', deaths: 2 * k, faced: 11 * k, pct: 18.2 }
    ],
    clutchByOpponents: [
      { vs: 1, won: k, total: 2 * k, pct: 50 },
      { vs: 2, won: 0, total: 2 * k, pct: 0 },
      { vs: 3, won: 0, total: k, pct: 0 },
      { vs: 4, won: 0, total: 0, pct: 0 },
      { vs: 5, won: 0, total: 0, pct: 0 }
    ]
  };
}

export function mockQuery(f: Filter): QueryResult {
  const rows = applyFilter(f);
  const kpis = kpisFor(rows);
  const plain = (f.range === 'all' || f.range === 'season') && f.maps.length === 0 && f.operators.length === 0;
  return {
    source: 'replays',
    official: plain ? mockOfficial(f.range === 'season' ? 'season' : 'all') : null,
    matchCount: rows.length,
    kpis,
    trend: trendFor(rows),
    operators: operatorsFor(f, kpis.kd),
    maps: f.maps.length ? MAP_STATS.filter((m) => f.maps.includes(m.key)) : MAP_STATS,
    recentMatches: rows.slice(0, 8),
    deep: mockDeep(rows.length)
  };
}

export function mockListMatches(f: Filter, page: number, pageSize = 20): MatchRow[] {
  return applyFilter(f).slice(page * pageSize, (page + 1) * pageSize);
}

// ---------- match detail ----------

const MY_TEAM = [ME, 'Teammate_A', 'Teammate_B', 'Teammate_C', 'Teammate_D'];
const OPP_TEAM = ['Rival_A', 'Rival_B', 'Rival_C', 'Rival_D', 'Rival_E'];

export function mockMatchDetail(matchId: string): MatchDetail {
  const match = ALL_MATCHES.find((m) => m.matchId === matchId) ?? ALL_MATCHES[0];
  const rand = rng(match.matchId.length * 31 + match.kills * 7 + match.deaths);
  const total = match.scoreUs + match.scoreThem;
  const pool = MOCK_OPERATORS;

  // Round outcomes: my team wins `scoreUs` of them, in a shuffled order.
  const outcomes: (0 | 1)[] = [
    ...Array<0>(match.scoreUs).fill(0),
    ...Array<1>(match.scoreThem).fill(1)
  ];
  for (let i = outcomes.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [outcomes[i], outcomes[j]] = [outcomes[j], outcomes[i]];
  }

  const opPick = (side: TeamSide) => {
    const c = pool.filter((o) => o.side === side);
    return c[Math.floor(rand() * c.length)].key;
  };

  const rows = [...MY_TEAM, ...OPP_TEAM].map((username, idx) => ({
    username,
    team: idx < 5 ? 0 : 1,
    kills: 0,
    deaths: 0,
    hs: 0,
    fk: 0,
    fd: 0,
    survived: 0,
    ops: [] as string[],
    roundKills: [] as number[]
  }));

  const rounds: RoundDetail[] = outcomes.map((winnerTeam, ri) => {
    const mySide: TeamSide = ri < Math.ceil(total / 2) ? 'attack' : 'defense';
    const sideOf = (team: number): TeamSide =>
      team === 0 ? mySide : mySide === 'attack' ? 'defense' : 'attack';
    rows.forEach((p) => {
      p.ops.push(p.username === ME && ri % 3 === 0 ? (mySide === 'attack' ? 'buck' : 'rook') : opPick(sideOf(p.team)));
      p.roundKills.push(0);
    });
    const alive = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    const kills: KillEventView[] = [];
    let t = 150;
    // Kill until the losing team is wiped out (or a few kills for objective wins).
    const loser = winnerTeam === 0 ? 1 : 0;
    const winCondition = rand() < 0.7 ? 'Eliminated enemies' : mySide === 'attack' === (winnerTeam === 0) ? 'Planted bomb' : 'Defused bomb';
    const killsNeeded = winCondition === 'Eliminated enemies' ? 5 : 2 + Math.floor(rand() * 3);
    for (let k = 0; k < killsNeeded + Math.floor(rand() * 2); k++) {
      const forced = k < killsNeeded;
      const killerTeam = forced ? winnerTeam : rand() < 0.5 ? 0 : 1;
      const victimTeam = killerTeam === 0 ? 1 : 0;
      const killers = alive.filter((i) => rows[i].team === killerTeam);
      const victims = alive.filter((i) => rows[i].team === victimTeam);
      if (!killers.length || !victims.length || (victimTeam !== loser && forced)) continue;
      const ki = killers[Math.floor(rand() * killers.length)];
      const vi = victims[Math.floor(rand() * victims.length)];
      alive.splice(alive.indexOf(vi), 1);
      const hs = rand() < 0.35;
      rows[ki].kills++;
      rows[ki].roundKills[ri]++;
      rows[vi].deaths++;
      if (hs) rows[ki].hs++;
      if (k === 0) {
        rows[ki].fk++;
        rows[vi].fd++;
      }
      t -= 6 + Math.floor(rand() * 25);
      kills.push({
        timeS: Math.max(t, 0),
        killer: rows[ki].username,
        killerOperator: rows[ki].ops[ri],
        victim: rows[vi].username,
        victimOperator: rows[vi].ops[ri],
        headshot: hs,
        killerTeam: killerTeam as 0 | 1,
        // Mock: some kills carry no weapon, like replays where it could not be decoded.
        weapon: k % 3 === 2 ? null : MOCK_WEAPONS[(ri + k) % MOCK_WEAPONS.length],
        weaponSlot: k % 3 === 2 ? null : 'primary'
      });
    }
    alive.forEach((i) => rows[i].survived++);
    return {
      number: ri + 1,
      winnerTeam,
      winCondition,
      site: ['2F Armory / Lounge', '1F Kitchen / Dining', 'B Garage'][ri % 3],
      mySide,
      kills
    };
  });

  const toRow = (p: (typeof rows)[number]): ScoreboardRow => ({
    username: p.username,
    isMe: p.username === ME,
    operatorsByRound: p.ops,
    kills: p.kills,
    deaths: p.deaths,
    hsPct: p.kills ? r1((p.hs / p.kills) * 100) : 0,
    fk: p.fk,
    fd: p.fd,
    clutchesWon: p.username === ME ? match.badges.filter((b) => /Won/.test(b)).length : 0,
    clutchesLost: 0,
    multiKills: p.roundKills.filter((n) => n >= 2).length,
    kostPct: total ? r1(((p.survived + p.roundKills.filter((n) => n > 0).length) / (2 * total)) * 100) : 0
  });

  return {
    match,
    teams: [rows.filter((p) => p.team === 0).map(toRow), rows.filter((p) => p.team === 1).map(toRow)],
    rounds
  };
}

// ---------- coach ----------

export function mockCoach(f: Filter): CoachReport {
  const last5 = f.range === 'last1';
  return {
    summary: last5
      ? 'Solid match: your K/D was well above your average, but your first-death rate on attack is still high.'
      : 'Your last 5 matches are trending up: K/D and win rate are both above the previous 20.',
    insights: [
      {
        kind: 'praise',
        title: 'Rook is paying off',
        text: 'Your K/D on Rook is clearly above your overall K/D.',
        evidence: 'Rook K/D 1.12 vs overall 1.01 over 142 rounds',
        operatorKey: 'rook',
        score: 0.62,
        family: 'operator'
      },
      {
        kind: 'improve',
        title: 'Oregon needs work',
        text: 'You win fewer than half your Oregon matches, mostly on attack.',
        evidence: 'Oregon attack win 40.0% over 7 matches',
        mapKey: 'oregon',
        score: 0.55,
        family: 'map'
      },
      {
        kind: 'info',
        title: 'Headshots are steady',
        text: 'Your headshot rate matches your usual level.',
        evidence: 'HS 36.4% vs 36.1%',
        score: 0,
        family: 'info'
      }
    ],
    strengths: ['Defense on Bank (66.7% win)', 'Buck K/D 1.21', 'Clutch conversion'],
    workOn: ['Attack on Oregon', 'First deaths on attack', 'Jäger K/D 0.94'],
    comparisons: [
      { metric: 'K/D', focus: 1.18, reference: 1.01, delta: 0.17, better: true, score: 0.4, n: 40, unit: 'ratio' },
      { metric: 'Kills / round', focus: 0.71, reference: 0.64, delta: 0.07, better: true, score: 0.4, n: 40, unit: 'ratio' },
      { metric: 'HS %', focus: 36.4, reference: 36.1, delta: 0.3, better: true, score: 0.4, n: 40, unit: 'pct' },
      { metric: 'Win %', focus: 60.0, reference: 50.0, delta: 10.0, better: true, score: 0.4, n: 40, unit: 'pct' },
      { metric: 'FK/FD', focus: 0.85, reference: 0.92, delta: -0.07, better: false, score: 0.4, n: 40, unit: 'ratio' },
      { metric: 'Survival %', focus: 36.0, reference: 38.6, delta: -2.6, better: false, score: 0.4, n: 40, unit: 'pct' }
    ],
    focusLabel: last5 ? 'Last match' : 'Last 5 matches',
    referenceLabel: last5 ? 'Your average' : 'Previous 20 matches'
  };
}

// ---------- v5 operator and weapon screens ----------

const MOCK_WEAPON_ROWS: { name: string; slot: string; kills: number; hs: number; ops: number }[] = [
  { name: 'R4-C', slot: 'primary', kills: 41, hs: 22, ops: 3 },
  { name: 'MP5', slot: 'primary', kills: 27, hs: 12, ops: 2 },
  { name: 'AK-12', slot: 'primary', kills: 19, hs: 9, ops: 2 },
  { name: 'P90', slot: 'primary', kills: 12, hs: 4, ops: 1 },
  { name: 'SG-CQB', slot: 'primary', kills: 8, hs: 1, ops: 1 },
  { name: 'P12', slot: 'secondary', kills: 6, hs: 3, ops: 4 }
];
const weaponKeyOf = (name: string) => name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

export function mockOperatorList(f: Filter): OperatorListRow[] {
  const rand = rng(11);
  const side = f.side === 'both' ? null : f.side;
  return MOCK_OPERATORS.filter((o) => !side || o.side === side)
    .map((o, i) => {
      const rounds = 90 - i * 9 + Math.floor(rand() * 6);
      const kills = Math.round(rounds * (0.6 + rand() * 0.3));
      const deaths = Math.round(rounds * (0.55 + rand() * 0.3));
      return {
        key: o.key,
        name: o.name,
        side: o.side,
        matches: Math.max(1, Math.round(rounds / 7)),
        rounds,
        winPct: r1(40 + rand() * 25),
        kills,
        deaths,
        kd: r2(kills / Math.max(1, deaths)),
        killsPerRound: r2(kills / rounds),
        hsPct: r1(20 + rand() * 25),
        kostPct: r1(55 + rand() * 20)
      };
    })
    .sort((a, b) => b.rounds - a.rounds);
}

export function mockWeaponList(_f: Filter): WeaponListRow[] {
  return MOCK_WEAPON_ROWS.map((w) => ({
    key: weaponKeyOf(w.name),
    name: w.name,
    slot: w.slot,
    kills: w.kills,
    headshots: w.hs,
    hsPct: r1((100 * w.hs) / w.kills),
    operators: w.ops,
    loadoutOperators: MOCK_OPERATORS.slice(0, w.ops + 2).map((o) => o.key)
  }));
}

function mockClips(opKey: string, weapon: string | null): DetailClip[] {
  return [0, 1, 2].map((i) => ({
    clip: {
      id: 9000 + i,
      matchId: ALL_MATCHES[i].matchId,
      round: 2 + i,
      kind: i === 2 ? ('death' as const) : ('kill' as const),
      timeS: 150 - i * 40,
      killer: i === 2 ? 'Rival_A' : ME,
      victim: i === 2 ? ME : 'Rival_B',
      path: '',
      starred: i === 0,
      size: 4_200_000,
      startMs: 0,
      durationMs: 8000,
      createdMs: 0
    },
    headshot: i === 1,
    weapon: i === 2 ? null : weapon,
    mapKey: ALL_MATCHES[i].mapKey,
    operatorKey: opKey,
    enemyOperatorKey: MOCK_OPERATORS[(i + 3) % MOCK_OPERATORS.length].key
  }));
}

export function mockOperatorScreen(f: Filter, key: string): OperatorScreen {
  const op = MOCK_OPERATORS.find((o) => o.key === key);
  const row = mockOperatorList({ ...f, side: 'both' }).find((o) => o.key === key);
  const rows = applyFilter({ ...f, operators: [] }).slice(0, 10);
  const rand = rng(key.length * 13 + 5);
  const trend: TrendPoint[] = rows
    .slice()
    .reverse()
    .map((m, i) => ({ index: i + 1, timestamp: m.timestamp, matchId: m.matchId, kd: r2(0.7 + rand() * 0.9), won: m.won }));
  const rivals = (shift: number) =>
    MOCK_OPERATORS.filter((o) => o.key !== key)
      .slice(shift, shift + 4)
      .map((o, i) => ({ operatorKey: o.key, name: o.name, count: 6 - i }));
  if (!row || !op) {
    const stats: DetailStats = {
      matches: 0, rounds: 0, roundsWon: 0, winPct: 0, kills: 0, deaths: 0, kd: 0, killsPerRound: 0, hsPct: 0,
      firstKills: 0, firstDeaths: 0, kostPct: 0, survivalPct: 0, clutchWon: 0, clutchLost: 0, clutchPct: 0
    };
    return {
      detail: { key, name: key, side: null, stats, sides: [], maps: [], weapons: [], mostKilled: [], mostKilledBy: [], matches: [], trend: [] },
      clips: []
    };
  }
  return {
    detail: {
      key,
      name: op.name,
      side: op.side,
      stats: {
        matches: row.matches,
        rounds: row.rounds,
        roundsWon: Math.round((row.rounds * row.winPct) / 100),
        winPct: row.winPct,
        kills: row.kills,
        deaths: row.deaths,
        kd: row.kd,
        killsPerRound: row.killsPerRound,
        hsPct: row.hsPct,
        firstKills: Math.round(row.rounds * 0.14),
        firstDeaths: Math.round(row.rounds * 0.1),
        kostPct: row.kostPct,
        survivalPct: r1(35 + rand() * 20),
        clutchWon: 3,
        clutchLost: 5,
        clutchPct: 37.5
      },
      sides: [{ side: op.side, rounds: row.rounds, winPct: row.winPct, kd: row.kd }],
      maps: MOCK_MAPS.slice(0, 4).map((m, i) => ({
        key: m.key,
        name: m.name,
        rounds: Math.max(1, Math.round(row.rounds / (i + 2))),
        winPct: r1(35 + rand() * 40),
        kills: 12 - i * 2,
        deaths: 9 - i,
        kd: r2((12 - i * 2) / (9 - i))
      })),
      weapons: MOCK_WEAPON_ROWS.slice(0, 3).map((w) => ({
        key: weaponKeyOf(w.name),
        name: w.name,
        slot: w.slot,
        kills: Math.round(w.kills / 2),
        hsPct: r1((100 * w.hs) / w.kills)
      })),
      mostKilled: rivals(0),
      mostKilledBy: rivals(2),
      matches: rows.slice(0, 8).map((m) => ({
        matchId: m.matchId,
        timestamp: m.timestamp,
        mapKey: m.mapKey,
        mapName: m.mapName,
        won: m.won,
        scoreUs: m.scoreUs,
        scoreThem: m.scoreThem,
        rounds: 4 + Math.floor(rand() * 4),
        kills: m.kills,
        deaths: m.deaths
      })),
      trend
    },
    clips: mockClips(key, 'R4-C')
  };
}

export function mockWeaponScreen(_f: Filter, key: string): WeaponScreen {
  const w = MOCK_WEAPON_ROWS.find((x) => weaponKeyOf(x.name) === key);
  if (!w) {
    return {
      detail: {
        key, name: key, slot: null, kills: 0, headshots: 0, hsPct: 0, rounds: 0, killsPerRound: 0,
        operators: [], maps: [], timing: { early: 0, mid: 0, late: 0 }, recentKills: []
      },
      clips: []
    };
  }
  const rounds = w.kills * 3;
  return {
    detail: {
      key,
      name: w.name,
      slot: w.slot,
      kills: w.kills,
      headshots: w.hs,
      hsPct: r1((100 * w.hs) / w.kills),
      rounds,
      killsPerRound: r2(w.kills / rounds),
      operators: MOCK_OPERATORS.slice(0, w.ops).map((o, i) => ({
        key: o.key,
        name: o.name,
        kills: Math.max(1, Math.round(w.kills / (i + 1.5))),
        hsPct: r1(20 + i * 5)
      })),
      loadoutOperators: MOCK_OPERATORS.slice(0, w.ops + 2).map((o) => o.key),
      maps: MOCK_MAPS.slice(0, 4).map((m, i) => ({ key: m.key, name: m.name, kills: Math.max(1, Math.round(w.kills / (i + 2))) })),
      timing: { early: Math.round(w.kills * 0.3), mid: Math.round(w.kills * 0.5), late: Math.round(w.kills * 0.2) },
      recentKills: ALL_MATCHES.slice(0, 6).map((m, i) => ({
        matchId: m.matchId,
        timestamp: m.timestamp,
        mapKey: m.mapKey,
        mapName: m.mapName,
        round: 1 + i,
        timeS: 170 - i * 20,
        operatorKey: MOCK_OPERATORS[i % w.ops].key,
        operatorName: MOCK_OPERATORS[i % w.ops].name,
        victimOperatorKey: MOCK_OPERATORS[(i + 4) % MOCK_OPERATORS.length].key,
        victimOperatorName: MOCK_OPERATORS[(i + 4) % MOCK_OPERATORS.length].name,
        headshot: i % 3 === 0
      }))
    },
    clips: mockClips(MOCK_OPERATORS[0].key, w.name)
  };
}
