// Pure helpers of the coaching Home page: greeting, form verdicts and the "what decided it" line.
import type { MatchDetail, QueryResult } from './types';

/** "Good morning" / "Good afternoon" / "Good evening" for a local hour (0-23). */
export function greeting(hour: number): string {
  if (hour < 5) return 'Still up';
  if (hour < 12) return 'Good morning';
  if (hour < 18) return 'Good afternoon';
  return 'Good evening';
}

export type Verdict = 'better' | 'same' | 'worse';

export interface FormMetric {
  key: string;
  label: string;
  unit: 'ratio' | 'pct';
  /** Smallest difference that counts as "better" or "worse"; smaller is "about usual". */
  noise: number;
  /** Reads the value from a query result; null when there is no data behind it. */
  read: (q: QueryResult) => number | null;
}

const pctOf = (r: { n: number; pct: number }) => (r.n > 0 ? r.pct : null);

/** The metrics of the form card. For all of them, higher is better. */
export const FORM_METRICS: FormMetric[] = [
  { key: 'kd', label: 'K/D', unit: 'ratio', noise: 0.05, read: (q) => (q.kpis.rounds > 0 ? q.kpis.kd : null) },
  { key: 'win', label: 'Win %', unit: 'pct', noise: 2, read: (q) => (q.kpis.matches > 0 ? q.kpis.winPct : null) },
  { key: 'kost', label: 'KOST', unit: 'pct', noise: 1.5, read: (q) => pctOf(q.deep.kost) },
  { key: 'entry', label: 'Opening duels won', unit: 'pct', noise: 2, read: (q) => pctOf(q.deep.entry) },
  { key: 'hs', label: 'Headshot %', unit: 'pct', noise: 1.5, read: (q) => (q.kpis.rounds > 0 ? q.kpis.hsPct : null) }
];

export function verdict(value: number, usual: number, noise: number): Verdict {
  const d = value - usual;
  return Math.abs(d) < noise ? 'same' : d > 0 ? 'better' : 'worse';
}

export const VERDICT_TEXT: Record<Verdict, string> = {
  better: 'Better than usual',
  same: 'About your usual',
  worse: 'Below your usual'
};

export function fmtMetric(v: number, unit: 'ratio' | 'pct'): string {
  return unit === 'pct' ? `${v.toFixed(1)}%` : v.toFixed(2);
}

export function fmtDelta(d: number, unit: 'ratio' | 'pct'): string {
  const sign = d > 0 ? '+' : d < 0 ? '-' : '';
  return `${sign}${Math.abs(d).toFixed(unit === 'pct' ? 1 : 2)}${unit === 'pct' ? ' pts' : ''}`;
}

const plural = (n: number, one: string, many = one + 's') => `${n} ${n === 1 ? one : many}`;

/**
 * One plain sentence on what decided a match, from its scoreboard. It looks at the tracked player's
 * opening duels, clutches and K/D, and at how close the score was. Never throws on thin data.
 */
export function decidedLine(d: MatchDetail): string {
  const me = d.teams[0].find((p) => p.isMe);
  const { won, scoreUs, scoreThem } = d.match;
  const margin = Math.abs(scoreUs - scoreThem);
  if (!me) return won ? 'A team win.' : 'A team loss.';

  const kd = me.deaths > 0 ? me.kills / me.deaths : me.kills;
  const firstDiff = me.fk - me.fd;
  const closeNote = margin <= 1 ? ' It was close, one round decided it.' : '';

  if (won) {
    if (me.clutchesWon > 0) return `You won ${plural(me.clutchesWon, 'clutch', 'clutches')} and that kept the team ahead.${closeNote}`;
    if (firstDiff >= 2) return `Your opening duels set the pace: ${plural(me.fk, 'first kill')} against ${plural(me.fd, 'first death')}.${closeNote}`;
    if (kd >= 1.3) return `A K/D of ${kd.toFixed(2)} carried the team.${closeNote}`;
    if (me.kostPct >= 70) return `You had a kill, a trade or survived in ${me.kostPct.toFixed(0)}% of the rounds.${closeNote}`;
    return `A team win. You finished ${me.kills}/${me.deaths}.${closeNote}`;
  }
  if (firstDiff <= -2) return `Opening duels cost you: ${plural(me.fd, 'first death')} against ${plural(me.fk, 'first kill')}.${closeNote}`;
  if (kd < 0.8) return `A rough K/D of ${kd.toFixed(2)} made it hard for the team to recover.${closeNote}`;
  if (me.clutchesLost >= 2) return `You lost ${plural(me.clutchesLost, 'clutch', 'clutches')}, and those rounds decided it.${closeNote}`;
  if (margin <= 1) return 'A close one: a single round decided it, and your numbers held up.';
  return `You played to your level (${me.kills}/${me.deaths}); the team came up short on rounds.`;
}
