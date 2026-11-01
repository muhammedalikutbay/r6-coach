// Realistic fake data used until the Rust commands exist. Player names are not included.
import type { LastMatchReport, MapRow, OperatorRow, RefreshResult } from './types';

export const mockReport: LastMatchReport = {
  match: {
    matchId: 'Match-2026-10-31_00-54-47-11844',
    timestamp: '2026-10-31T00:54:47',
    map: 'Outback',
    matchType: 'Ranked',
    won: true,
    scoreUs: 5,
    scoreThem: 3,
    operators: [
      { name: 'Osa', rounds: 3 },
      { name: 'Zofia', rounds: 2 },
      { name: 'Jäger', rounds: 2 },
      { name: 'Kaid', rounds: 1 }
    ],
    kills: 11,
    deaths: 6,
    headshots: 5,
    firstBloods: 2,
    firstDeaths: 1
  },
  advice: [
    {
      kind: 'praise',
      text: 'Great opening duels: you got the first kill twice and died first only once.',
      evidence: '2 first bloods vs 1 first death (your average: 0.8 vs 1.1 per match)'
    },
    {
      kind: 'praise',
      text: 'Your K/D with Osa is well above your usual level.',
      evidence: 'Osa this match: 6 kills / 2 deaths (3.00) vs career 1.21'
    },
    {
      kind: 'improve',
      text: 'Defense on Outback is still your weak side. Try an anchor setup instead of roaming for a few rounds.',
      evidence: 'Defense win rate on Outback: 38% over 13 rounds (attack: 61%)'
    },
    {
      kind: 'info',
      text: 'Kaid was played for only one round. Not enough data for a verdict yet.',
      evidence: '1 round this match, 9 rounds in total'
    }
  ],
  session: {
    matches: 3,
    wins: 2,
    losses: 1,
    rounds: 19,
    kills: 24,
    deaths: 17,
    headshots: 10,
    firstBloods: 6,
    firstDeaths: 4,
    operators: [
      { name: 'Osa', rounds: 8 },
      { name: 'Jäger', rounds: 6 },
      { name: 'Zofia', rounds: 5 }
    ],
    maps: [
      { name: 'Outback', matches: 2, wins: 2 },
      { name: 'Clubhouse', matches: 1, wins: 0 }
    ]
  },
  sessionAdvice: [
    {
      kind: 'praise',
      text: 'Strong session: a 1.41 K/D over 3 matches (2 of 3 won), above your usual 1.10. Keep this rhythm!',
      evidence: '19 rounds, K 24 / D 17, K/D 1.41 (adjusted 1.30); usual K/D 1.10; 2-1 in matches'
    },
    {
      kind: 'praise',
      text: 'You played Outback 2 times and won every one. This map suits you!',
      evidence: 'Outback: 2 of 2 matches won'
    }
  ]
};

export const mockOperators: OperatorRow[] = [
  { name: 'Osa', side: 'attack', rounds: 41, winPct: 56, kd: 1.21, hsPct: 38, trend: 'up' },
  { name: 'Zofia', side: 'attack', rounds: 34, winPct: 50, kd: 1.05, hsPct: 33, trend: 'flat' },
  { name: 'Ash', side: 'attack', rounds: 22, winPct: 45, kd: 0.92, hsPct: 29, trend: 'down' },
  { name: 'Jäger', side: 'defense', rounds: 38, winPct: 53, kd: 1.1, hsPct: 36, trend: 'up' },
  { name: 'Kaid', side: 'defense', rounds: 9, winPct: 44, kd: 0.8, hsPct: 25, trend: 'down' },
  { name: 'Mute', side: 'defense', rounds: 17, winPct: 47, kd: 0.95, hsPct: 31, trend: 'flat' }
];

export const mockMaps: MapRow[] = [
  { name: 'Outback', rounds: 26, winPct: 50, kd: 1.12, attackWinPct: 61, defenseWinPct: 38 },
  { name: 'Clubhouse', rounds: 24, winPct: 58, kd: 1.15, attackWinPct: 55, defenseWinPct: 62 },
  { name: 'Bank', rounds: 22, winPct: 41, kd: 0.94, attackWinPct: 45, defenseWinPct: 36 },
  { name: 'Consulate', rounds: 18, winPct: 56, kd: 1.03, attackWinPct: 50, defenseWinPct: 61 },
  { name: 'Chalet', rounds: 14, winPct: 43, kd: 0.88, attackWinPct: 40, defenseWinPct: 46 }
];

export const mockRefresh: RefreshResult = { newMatches: 1, failed: [] };

export const mockReplayDir =
  'C:\\Users\\Player\\Documents\\My Games\\Rainbow Six - Siege\\0000\\MatchReplay';
