#!/usr/bin/env node
// Generates data/operators.json and data/maps.json so the Rust side can resolve operator and
// map names from its own tables (a new season only needs a data update, no Go rebuild).
// Re-run after a new season or after capturing more replays / Tracker data:
//
//   node tools/gen-name-tables/gen.mjs [--veri <R6Koc-veri dir>] [--exe <r6-dissect exe>] [--out <data dir>]
//
// Sources (nothing here talks to the network):
//   1. tools/r6-dissect/dissect/{operator_string,operator_roles,map_string}.go
//      (known ids, names, sides)
//   2. the local replays, parsed with the sidecar exe (map ids, operator ids per player and round)
//   3. one-time Tracker Network JSON in <veri>/tracker (map names/slugs by matchID, operator
//      slugs by profile + round, display names, sides)
// The output contains no player names or profile ids: only ids, slugs, names, sides, match ids.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '..', '..');
const argv = process.argv.slice(2);
const arg = (name, dflt) => {
  const i = argv.indexOf('--' + name);
  return i >= 0 ? argv[i + 1] : dflt;
};
const veri = arg('veri', 'C:\\Users\\Kutbay\\Desktop\\R6Koc-veri');
const exe = arg('exe', path.join(repo, 'app', 'src-tauri', 'binaries', 'r6-dissect-x86_64-pc-windows-msvc.exe'));
const outDir = arg('out', path.join(repo, 'data'));
const dissectDir = path.join(repo, 'tools', 'r6-dissect', 'dissect');
const trackerDir = path.join(veri, 'tracker');
const replayDir = path.join(veri, 'MatchReplay');
const TRACKED_PLAYER = 'H.A.N-01';
const MATCH_TIME_TOLERANCE_MS = 10 * 60 * 1000; // timestamp fallback only
const warnings = [];
const warn = (m) => {
  warnings.push(m);
  console.warn('WARN:', m);
};

const read = (p) => fs.readFileSync(p, 'utf8');
const readJson = (p) => JSON.parse(read(p));
const slugOf = (s) =>
  s
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '');

// ---- 1. upstream Go tables -------------------------------------------------
function goIds(file) {
  // `_ = x[Name-12345]` lines of the stringer files
  return [...read(path.join(dissectDir, file)).matchAll(/_ = x\[(\w+)-(\d+)\]/g)].map((m) => ({
    ident: m[1],
    id: Number(m[2]),
  }));
}
const upstreamOps = goIds('operator_string.go');
const upstreamMaps = goIds('map_string.go');
const roles = new Map(
  [...read(path.join(dissectDir, 'operator_roles.go')).matchAll(/(\d+):\s*(Attack|Defense)/g)].map((m) => [
    Number(m[1]),
    m[2] === 'Attack' ? 'attacker' : 'defender',
  ]),
);

// ---- 2. replays ------------------------------------------------------------
function collectRounds() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'r6gen-'));
  const out = [];
  let n = 0;
  for (const folder of fs.readdirSync(replayDir, { withFileTypes: true }).filter((d) => d.isDirectory())) {
    for (const f of fs.readdirSync(path.join(replayDir, folder.name)).filter((x) => x.endsWith('.rec'))) {
      const outFile = path.join(tmp, `r${++n}.json`);
      spawnSync(exe, [path.join(replayDir, folder.name, f), '-o', outFile], { stdio: 'ignore' });
      if (!fs.existsSync(outFile)) {
        warn(`parser failed on ${folder.name}/${f}`);
        continue;
      }
      out.push(JSON.parse(read(outFile)));
    }
  }
  fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
const rounds = collectRounds();
console.log(`parsed ${rounds.length} rounds`);

const matches = new Map(); // matchID -> {id, ts, map, rounds}
for (const r of rounds) {
  const m = matches.get(r.matchID) ?? { id: r.matchID, ts: Date.parse(r.timestamp), map: r.map, rounds: [] };
  m.ts = Math.min(m.ts, Date.parse(r.timestamp));
  m.rounds.push(r);
  matches.set(r.matchID, m);
}

// ---- 3. Tracker data -------------------------------------------------------
const listEntries = new Map(); // tracker match id -> {ts, mapSlug, mapName}
const trackerRounds = new Map(); // `${matchID}|${profile}|${roundId}` -> {slug, name, side}
const opMeta = new Map(); // slug -> {name, sides:Set}
for (const f of fs.readdirSync(trackerDir).filter((x) => x.endsWith('.json'))) {
  let j;
  try {
    j = readJson(path.join(trackerDir, f));
  } catch {
    continue;
  }
  const d = j.data;
  if (d?.matches) {
    for (const m of d.matches) {
      if (m.metadata?.sessionMapName) {
        listEntries.set(m.attributes.id, {
          ts: Date.parse(m.metadata.timestamp),
          mapSlug: m.attributes.sessionMap,
          mapName: m.metadata.sessionMapName,
        });
      }
    }
  } else if (d?.attributes?.id && d?.metadata?.sessionMapName && d?.segments) {
    const id = d.attributes.id;
    listEntries.set(id, {
      ts: Date.parse(d.metadata.timestamp),
      mapSlug: d.attributes.sessionMap,
      mapName: d.metadata.sessionMapName,
    });
    for (const s of d.segments) {
      if (s.type === 'player-round' && s.attributes.operatorId && s.attributes.operatorId !== 'unknown') {
        const a = s.attributes;
        trackerRounds.set(`${id}|${a.playerId}|${a.roundId}`, {
          slug: a.operatorId,
          name: s.metadata?.operatorName,
          side: a.sideId,
        });
        const meta = opMeta.get(a.operatorId) ?? { name: s.metadata?.operatorName, sides: new Set() };
        meta.sides.add(a.sideId);
        opMeta.set(a.operatorId, meta);
      }
    }
  }
}
console.log(`tracker: ${listEntries.size} matches with map names, ${trackerRounds.size} player-round records`);

// ---- maps ------------------------------------------------------------------
const mapSeen = new Map(); // id -> {matches, votes, upstreamName}
let byId = 0;
let byTime = 0;
for (const m of matches.values()) {
  let t = listEntries.get(m.id);
  let how = 'matchID';
  if (t) byId++;
  else {
    const near = [...listEntries.values()]
      .map((e) => ({ e, d: Math.abs(e.ts - m.ts) }))
      .sort((a, b) => a.d - b.d)[0];
    if (near && near.d <= MATCH_TIME_TOLERANCE_MS) {
      t = near.e;
      how = 'timestamp';
      byTime++;
    }
  }
  const s = mapSeen.get(m.map.id) ?? { matches: [], votes: new Map(), upstreamName: m.map.name };
  s.matches.push(m.id);
  if (t) {
    const k = `${t.mapSlug}|${t.mapName}`;
    s.votes.set(k, (s.votes.get(k) ?? 0) + 1);
  }
  mapSeen.set(m.map.id, s);
}
console.log(`map evidence: ${byId} matches by matchID, ${byTime} by timestamp, ${matches.size - byId - byTime} none`);

const prettyIdent = (s) =>
  s
    .replace(/Y\d+$/, '')
    .replace(/([a-z])([A-Z0-9])/g, '$1 $2');
// Tracker map slugs by normalized name (e.g. kafedostoyevsky -> kafe-dostoyevsky)
const trackerSlugs = new Map();
for (const e of listEntries.values()) trackerSlugs.set(slugOf(e.mapSlug), e.mapSlug);
trackerSlugs.set('kafedostoyevsky', 'kafe-dostoyevsky'); // slug seen in the Tracker map-season segments (no match list entry)
const mapsOut = [];
const mapsUnresolved = [];
for (const [id, s] of [...mapSeen].sort((a, b) => a[0] - b[0])) {
  const votes = [...s.votes].sort((a, b) => b[1] - a[1]);
  if (votes.length > 1) warn(`map ${id}: conflicting Tracker names ${votes.map((v) => v[0]).join(' vs ')}`);
  if (votes.length) {
    const [slug, name] = votes[0][0].split('|');
    mapsOut.push({ id, key: slug, name });
  } else if (!/^Map\(\d+\)$/.test(s.upstreamName)) {
    // No Tracker evidence, but the parser itself knows the map (newer upstream names).
    // Reuse the Tracker slug/name of the same map when another id of it was seen.
    const base = slugOf(s.upstreamName.replace(/Y\d+$/, ''));
    const sibling = mapsOut.find((m) => slugOf(m.key) === base);
    mapsOut.push(
      sibling
        ? { id, key: sibling.key, name: sibling.name }
        : { id, key: trackerSlugs.get(base) ?? base, name: prettyIdent(s.upstreamName) },
    );
    console.log(`map ${id} resolved from parser name ${s.upstreamName} (matches ${s.matches.join(', ')})`);
  } else {
    mapsUnresolved.push({ id, seenIn: s.matches });
  }
}
// Upstream-known ids that no replay of ours used (older maps): keep upstream names.
const have = new Set([...mapsOut, ...mapsUnresolved].map((m) => m.id));
for (const { ident, id } of upstreamMaps) {
  if (have.has(id)) continue;
  mapsOut.push({ id, key: slugOf(ident.replace(/Y\d+$/, '')), name: prettyIdent(ident) });
}
mapsOut.sort((a, b) => a.id - b.id);

// ---- operators -------------------------------------------------------------
const nameById = new Map(upstreamOps.map((o) => [o.id, o.ident]));
const slugVotes = new Map(); // id -> Map(slug -> count): Tracker evidence for ids upstream lacks
const opSeen = new Map(); // id -> {matches:Set, tracked:boolean}
let agree = 0;
let disagree = 0;
for (const m of matches.values()) {
  for (const r of m.rounds) {
    for (const p of r.players) {
      const o = p.operator;
      const seen = opSeen.get(o.id) ?? { matches: new Set(), tracked: false };
      seen.matches.add(m.id);
      if (p.username === TRACKED_PLAYER) seen.tracked = true;
      opSeen.set(o.id, seen);
      const t = trackerRounds.get(`${m.id}|${p.profileID}|${r.roundNumber + 1}`);
      if (!t) continue;
      const votes = slugVotes.get(o.id) ?? new Map();
      votes.set(t.slug, (votes.get(t.slug) ?? 0) + 1);
      slugVotes.set(o.id, votes);
    }
  }
}
// Sanity check of the (match, profile, round) matching: for operators upstream already names,
// Tracker's slug must equal the upstream name. A small share of rounds disagrees (the two sources
// do not line up perfectly for every round), so ids are resolved by majority vote.
for (const [id, votes] of slugVotes) {
  const known = nameById.get(id);
  if (!known) continue;
  for (const [slug, n] of votes) if (slugOf(known) === slug) agree += n; else disagree += n;
}
console.log(`round-matching sanity check on upstream-known operators: ${agree} agree, ${disagree} disagree`);

const sideFromMeta = (slug) => {
  const s = opMeta.get(slug)?.sides;
  return s && s.size === 1 ? (s.has('attacker') ? 'attacker' : 'defender') : undefined;
};
const opsOut = [];
const opsUnresolved = [];
for (const { ident, id } of upstreamOps) {
  const key = slugOf(ident);
  const side = roles.get(id) ?? sideFromMeta(key);
  if (!side && ident !== 'Recruit') warn(`no side for ${ident}`); // Recruit plays both sides
  opsOut.push({ id, key, name: (opMeta.get(key)?.name ?? ident).trim(), side: side ?? 'unknown' });
}
const viaTracker = [];
for (const [id, seen] of [...opSeen].sort((a, b) => a[0] - b[0])) {
  if (nameById.has(id)) continue;
  const votes = [...(slugVotes.get(id) ?? [])].sort((a, b) => b[1] - a[1]);
  if (votes.length) {
    if (votes.length > 1) warn(`operator ${id}: conflicting Tracker slugs ${votes.map((v) => `${v[0]}x${v[1]}`).join(', ')}`);
    const slug = votes[0][0];
    const entry = { id, key: slug, name: (opMeta.get(slug)?.name ?? slug).trim(), side: roles.get(id) ?? sideFromMeta(slug) ?? 'unknown' };
    opsOut.push(entry);
    viaTracker.push({ ...entry, votes: votes.map((v) => `${v[0]}x${v[1]}`) });
  } else {
    opsUnresolved.push({ id, seenIn: [...seen.matches] });
  }
}
// ids with a role upstream but neither a name nor any replay of ours
for (const id of roles.keys()) {
  if (nameById.has(id) || opSeen.has(id)) continue;
  opsUnresolved.push({ id, seenIn: [] });
}
opsOut.sort((a, b) => a.id - b.id);
opsUnresolved.sort((a, b) => a.id - b.id);

// ---- write -----------------------------------------------------------------
fs.mkdirSync(outDir, { recursive: true });
const write = (name, data) => fs.writeFileSync(path.join(outDir, name), JSON.stringify(data, null, 2) + '\n');
write('operators.json', { operators: opsOut, unresolved: opsUnresolved });
write('maps.json', { maps: mapsOut, unresolved: mapsUnresolved });
console.log(`operators: ${opsOut.length} (${viaTracker.length} resolved via Tracker), unresolved ${opsUnresolved.length}`);
for (const r of viaTracker) console.log('  Tracker-resolved:', JSON.stringify(r));
console.log(`maps: ${mapsOut.length}, unresolved ${mapsUnresolved.length}`);
const trackedUnresolved = opsUnresolved.filter((u) => opSeen.get(u.id)?.tracked);
// Tracker verification of the ids that were unknown to the original Go tables
for (const id of [374667787937, 444310693746, 456757346397]) {
  const votes = [...(slugVotes.get(id) ?? [])].map(([k, n]) => `${k}x${n}`).join(', ');
  console.log(`  verify ${id}: table=${opsOut.find((o) => o.id === id)?.key ?? '-'} tracker=[${votes}]`);
}
console.log(`operator ids played by ${TRACKED_PLAYER} left unresolved: ${trackedUnresolved.length}`);
console.log(`warnings: ${warnings.length}`);
