#!/usr/bin/env node
// Builds data/baseline.json: the tracked player's personal baseline (counters only), taken from the
// one-time Tracker Network JSON in <veri>/tracker. Rates (kdRatio, headshotPct, ...) are NOT stored;
// they are derived from the counters where needed (--show).
//
//   node tools/gen-baseline/gen.mjs [--veri <R6Koc-veri dir>] [--out <baseline.json>] [--show]
//
// Nothing here talks to the network. The output contains only the tracked player's name, slugs,
// sides, game-mode keys and counters: no other player's data.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '..', '..');
const argv = process.argv.slice(2);
const arg = (name, dflt) => {
  const i = argv.indexOf('--' + name);
  return i >= 0 ? argv[i + 1] : dflt;
};
const show = argv.includes('--show');
const veri = arg('veri', process.env.R6K_VERI ?? 'R6Koc-veri');
const outFile = arg('out', path.join(repo, 'data', 'baseline.json'));
const trackerDir = path.join(veri, 'tracker');

const PLAYER = 'H.A.N-01';
// Tracker capture time (from tracker/_manifest.json, minute precision + capture second)
const CAPTURED_AT = '2026-11-01T12:28:49Z';
const COUNTERS = [
  'roundsPlayed',
  'roundsWon',
  'matchesPlayed',
  'matchesWon',
  'kills',
  'deaths',
  'assists',
  'headshots',
  'firstBloods',
  'firstDeaths',
  'clutches',
  'aces',
  'timePlayed',
];

const readData = (name) => JSON.parse(fs.readFileSync(path.join(trackerDir, name), 'utf8')).data;

// Picks the counters that exist in a Tracker `stats` object; a counter that is absent stays absent.
function counters(stats) {
  const out = {};
  for (const name of COUNTERS) {
    const s = stats?.[name];
    if (s && typeof s.value === 'number') out[name] = s.value;
  }
  return out;
}

// Object with keys in sorted order at every level (deterministic output).
function sortKeys(obj) {
  const sortVal = (v) => (v && typeof v === 'object' && !Array.isArray(v) ? sortKeys(v) : v);
  return Object.fromEntries(Object.keys(obj).sort().map((k) => [k, sortVal(obj[k])]));
}

// ---- operators: one segment per operator (side comes from the segment) -------
const operators = {};
for (const seg of readData('profile_ubi_H.A.N-01_segments_operator-season_all-sessionType_all.json')) {
  const slug = seg.attributes.operator;
  if (operators[slug]) throw new Error(`duplicate operator segment: ${slug}`);
  operators[slug] = { side: seg.attributes.side, ...counters(seg.stats) };
}

// ---- maps: 59 segments = 20 maps x (all sides, attacker, defender). ---------
// Top-level counters are the all-sides segment; `sides` holds the attacker / defender segments.
// A side is omitted when Tracker has no segment for it (e.g. one map has no defender segment).
const maps = {};
const mapSides = {};
for (const seg of readData('profile_ubi_H.A.N-01_segments_map-season_all-sessionType_all.json')) {
  const slug = seg.attributes.map;
  const side = seg.attributes.side;
  if (side === null) {
    if (maps[slug]) throw new Error(`duplicate map segment: ${slug}`);
    maps[slug] = counters(seg.stats);
  } else if (side === 'attacker' || side === 'defender') {
    mapSides[slug] ??= {};
    if (mapSides[slug][side]) throw new Error(`duplicate map ${side} segment: ${slug}`);
    mapSides[slug][side] = counters(seg.stats);
  } else {
    throw new Error(`unknown map side "${side}": ${slug}`);
  }
}
for (const slug of Object.keys(mapSides)) {
  if (!maps[slug]) throw new Error(`map side segments without all-sides segment: ${slug}`);
  maps[slug].sides = { ...mapSides[slug] };
}

// ---- profile overview + game-mode segments ----------------------------------
const profileSegments = readData('profile_ubi_H.A.N-01.json').segments;
const overview = profileSegments.find((s) => s.type === 'overview');
if (!overview) throw new Error('overview segment missing in profile');
const gamemodes = {};
for (const seg of profileSegments.filter((s) => s.type === 'gamemode')) {
  const key = seg.attributes.gamemode;
  if (gamemodes[key]) throw new Error(`duplicate gamemode segment: ${key}`);
  gamemodes[key] = counters(seg.stats);
}

// Sorted keys at every level below the top (top-level order follows the spec).
const sortEntries = (o) => Object.fromEntries(Object.keys(o).sort().map((k) => [k, sortKeys(o[k])]));
const baseline = {
  player: PLAYER,
  capturedAt: CAPTURED_AT,
  operators: sortEntries(operators),
  maps: sortEntries(maps),
  gamemodes: sortEntries(gamemodes),
  overall: sortKeys(counters(overview.stats)),
};

fs.mkdirSync(path.dirname(outFile), { recursive: true });
fs.writeFileSync(outFile, JSON.stringify(baseline, null, 1) + '\n');
console.log(
  `baseline: ${Object.keys(baseline.operators).length} operators, ${Object.keys(baseline.maps).length} maps, ` +
    `${Object.keys(baseline.gamemodes).length} gamemodes -> ${outFile}`,
);

// ---- --show: top 10 operators by rounds played ------------------------------
if (show) {
  const ratio = (n, d) => (d > 0 ? n / d : n);
  const top = Object.entries(baseline.operators)
    .sort((a, b) => (b[1].roundsPlayed ?? 0) - (a[1].roundsPlayed ?? 0) || (a[0] < b[0] ? -1 : 1))
    .slice(0, 10);
  console.log('operator         rounds   K/D    HS%');
  for (const [slug, c] of top) {
    const kd = ratio(c.kills, c.deaths).toFixed(2);
    const hs = (ratio(c.headshots, c.kills) * 100).toFixed(1);
    console.log(`${slug.padEnd(16)} ${String(c.roundsPlayed ?? '-').padStart(6)}   ${kd.padStart(5)}  ${hs.padStart(5)}%`);
  }
}
