// Builds an anonymized copy of an R6 Coach database for screenshots and demos.
//
//   node --no-warnings tools/demo-data/sanitize.mjs <input.db> <output.db> [replay-dir]
//
// Every player name (matches.full_json, the zlib raw JSON, clips) is replaced by a stable fake name,
// every player profile id by a fake UUID. The tracked player (the replay's recording player,
// `me_profile_id`) becomes `Coach.Demo`. The script reads the real names from the input database
// itself, so no name is stored in this file. After writing, it searches the output for every
// original name and profile id and exits with an error if one is left.
//
// Needs Node 22.5+ (built-in `node:sqlite`). Never commit the input or the output database.
import { DatabaseSync } from 'node:sqlite';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const [inPath, outPath, replayDirArg] = process.argv.slice(2);
if (!inPath || !outPath) {
  console.error('usage: node --no-warnings sanitize.mjs <input.db> <output.db> [replay-dir]');
  process.exit(2);
}
if (path.resolve(inPath) === path.resolve(outPath)) {
  console.error('input and output must differ');
  process.exit(2);
}

fs.mkdirSync(path.dirname(path.resolve(outPath)), { recursive: true });
fs.copyFileSync(inPath, outPath);
const db = new DatabaseSync(outPath);

// ---------- collect names and ids ----------
const inflate = (buf) => {
  const b = Buffer.from(buf);
  try {
    return zlib.inflateSync(b).toString('utf8');
  } catch {
    return zlib.inflateRawSync(b).toString('utf8');
  }
};

const rows = db.prepare('SELECT match_id, raw_json, full_json FROM matches').all();
const names = new Set();
const profileIds = new Set();
const meIds = new Set();
const playerNumericIds = new Set();
for (const r of rows) {
  const full = JSON.parse(r.full_json);
  if (full.me_profile_id) meIds.add(full.me_profile_id);
  for (const p of full.players ?? []) {
    if (p.username) names.add(p.username);
    if (p.profile_id) profileIds.add(p.profile_id);
  }
  const rawText = inflate(r.raw_json);
  for (const m of rawText.matchAll(/"recordingPlayerID":(\d+)/g)) playerNumericIds.add(m[1]);
  for (const round of JSON.parse(rawText).rounds ?? []) {
    for (const p of round.players ?? []) {
      if (p.username) names.add(p.username);
      if (p.profileID) profileIds.add(p.profileID);
    }
    for (const s of round.stats ?? []) if (s.username) names.add(s.username);
    for (const f of round.matchFeedback ?? []) {
      if (f.username) names.add(f.username);
      if (f.target) names.add(f.target);
    }
    if (round.recordingProfileID) profileIds.add(round.recordingProfileID);
  }
}
for (const t of db.prepare("SELECT killer, victim FROM clips").all()) {
  if (t.killer) names.add(t.killer);
  if (t.victim) names.add(t.victim);
}

// Names of the tracked player: every username that carries one of the tracked profile ids.
const meNames = new Set();
for (const r of rows) {
  const full = JSON.parse(r.full_json);
  for (const p of full.players ?? []) if (meIds.has(p.profile_id) && p.username) meNames.add(p.username);
}

// ---------- stable fake values ----------
const ADJ = ['Quiet', 'Rapid', 'Amber', 'Silent', 'Iron', 'Lunar', 'Frost', 'Crimson', 'Hollow', 'Swift',
  'Rusty', 'Neon', 'Dusty', 'Gentle', 'Wild', 'Cobalt', 'Velvet', 'Ghost', 'Brave', 'Misty', 'Plain',
  'Sharp', 'Cosmic', 'Lucky', 'Static', 'Pale', 'Bitter', 'Golden', 'Steady', 'Wired'];
const NOUN = ['Falcon', 'Pixel', 'Otter', 'Comet', 'Anvil', 'Badger', 'Cipher', 'Harbor', 'Lynx', 'Mantis',
  'Nomad', 'Orbit', 'Pilot', 'Quartz', 'Raven', 'Signal', 'Tundra', 'Vector', 'Willow', 'Yonder',
  'Zephyr', 'Bishop', 'Cactus', 'Drifter', 'Ember', 'Fable', 'Glider', 'Hornet', 'Island', 'Jigsaw'];
const hash = (s) => crypto.createHash('sha256').update('r6coach-demo:' + s).digest();

const nameMap = new Map();
const used = new Set(['Coach.Demo']);
for (const n of meNames) {
  nameMap.set(n, 'Coach.Demo');
}
for (const n of [...names].sort()) {
  if (nameMap.has(n)) continue;
  const h = hash(n);
  let k = 0;
  let fake;
  do {
    const a = ADJ[(h[0] + k) % ADJ.length];
    const b = NOUN[(h[1] + k * 7) % NOUN.length];
    const num = String(h[2] % 100).padStart(2, '0');
    fake = k < 3 ? `${a}${b}${num}` : `${a}_${b}${num}${k}`;
    k++;
  } while (used.has(fake));
  used.add(fake);
  nameMap.set(n, fake);
}

const fakeUuid = (id) => {
  const h = hash('uuid:' + id);
  const hex = h.subarray(0, 16).toString('hex').split('');
  hex[12] = '4';
  hex[16] = '89ab'[h[16] % 4];
  const s = hex.join('');
  return `${s.slice(0, 8)}-${s.slice(8, 12)}-${s.slice(12, 16)}-${s.slice(16, 20)}-${s.slice(20, 32)}`;
};
const idMap = new Map([...profileIds, ...meIds].map((id) => [id, fakeUuid(id)]));
const numMap = new Map([...playerNumericIds].map((id) => [id, String(2000000000000000000n + BigInt('0x' + hash('num:' + id).subarray(0, 6).toString('hex')))]));

// ---------- text replacement ----------
// Names appear only as whole JSON string values, so replace the quoted form. Go escapes < > & as
// \u003c \u003e \u0026 in raw_json, so every name is replaced in both spellings.
const quoted = (s) => JSON.stringify(s);
const goQuoted = (s) =>
  quoted(s).replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026');
const pairs = [];
for (const [n, f] of nameMap) {
  pairs.push([quoted(n), quoted(f)]);
  if (goQuoted(n) !== quoted(n)) pairs.push([goQuoted(n), quoted(f)]);
}
// Longest first, so a quoted name never partly matches another.
pairs.sort((a, b) => b[0].length - a[0].length);
const nameByQuoted = new Map(pairs);
const nameRe = new RegExp(pairs.map(([q]) => q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'), 'g');
const idRe = new RegExp([...idMap.keys()].join('|'), 'g');
const numRe = playerNumericIds.size ? new RegExp([...playerNumericIds].join('|'), 'g') : null;

const scrub = (text) => {
  let t = text.replace(nameRe, (m) => nameByQuoted.get(m)).replace(idRe, (m) => idMap.get(m));
  if (numRe) t = t.replace(numRe, (m) => numMap.get(m));
  return t;
};

// ---------- rewrite ----------
db.exec('BEGIN');
const upd = db.prepare('UPDATE matches SET raw_json = ?, full_json = ? WHERE match_id = ?');
for (const r of rows) {
  const raw = zlib.deflateSync(Buffer.from(scrub(inflate(r.raw_json)), 'utf8'));
  upd.run(raw, scrub(r.full_json), r.match_id);
}
const clipUpd = db.prepare('UPDATE clips SET killer = ?, victim = ? WHERE id = ?');
for (const c of db.prepare('SELECT id, killer, victim FROM clips').all()) {
  clipUpd.run(nameMap.get(c.killer) ?? c.killer, nameMap.get(c.victim) ?? c.victim, c.id);
}
// Settings: keep none of the real folder paths.
const replayDir = replayDirArg ?? path.join(path.dirname(path.resolve(outPath)), 'replays');
fs.mkdirSync(replayDir, { recursive: true });
db.prepare("UPDATE settings SET value = ? WHERE key = 'replay_dir'").run(replayDir);
db.exec('COMMIT');
db.exec('VACUUM');

// ---------- verify ----------
const verifyText = [];
for (const r of db.prepare('SELECT raw_json, full_json FROM matches').all()) {
  verifyText.push(inflate(r.raw_json), r.full_json);
}
for (const c of db.prepare('SELECT killer, victim FROM clips').all()) verifyText.push(c.killer, c.victim);
for (const s of db.prepare('SELECT key, value FROM settings').all()) verifyText.push(s.key, s.value);
const hay = verifyText.join('\n');
const hayLower = hay.toLowerCase();
const left = [];
for (const n of names) {
  if (n.length >= 4 ? hayLower.includes(n.toLowerCase()) : hay.includes(quoted(n))) left.push('name');
}
for (const id of idMap.keys()) if (hay.includes(id)) left.push('profile id');
for (const id of playerNumericIds) if (hay.includes(id)) left.push('player id');
db.close();
if (left.length > 0) {
  console.error(`FAILED: ${left.length} original value(s) still present in ${outPath}`);
  process.exit(1);
}
console.log(`ok: ${rows.length} matches, ${nameMap.size} names, ${idMap.size} profile ids replaced -> ${outPath}`);
