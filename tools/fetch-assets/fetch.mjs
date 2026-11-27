// One-time download of operator badges and map images for the R6Koc app.
//
// Usage (from the repo root):  node tools/fetch-assets/fetch.mjs
//
// 1. Operator badges and map images listed in the Tracker JSON captured by the
//    user (`metadata.operatorImageUrl`, `metadata.mapImageUrl`) are downloaded.
// 2. Keys from data/operators.json and data/maps.json that the Tracker JSON does
//    not cover are tried against the Tracker CDN with the same key-based pattern.
// 3. Files already on disk are skipped, so re-running only fills gaps.
// 4. app/static/assets/manifest.json is rewritten from what is on disk, and the
//    keys without an image are printed and listed under "missing".
//
// The app never calls this script or the CDN at runtime. Node built-ins only.
// Override the data folder with the R6K_VERI environment variable.

import { readdir, readFile, writeFile, mkdir, stat } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..', '..');
const VERI = process.env.R6K_VERI ?? 'R6Koc-veri';
const TRACKER_DIR = join(VERI, 'tracker');
const ASSETS_DIR = join(ROOT, 'app', 'static', 'assets');
const OP_DIR = join(ASSETS_DIR, 'operators');
const MAP_DIR = join(ASSETS_DIR, 'maps');

const CDN = 'https://trackercdn.com/cdn/r6.tracker.network';
const DELAY_MS = 150;
const USER_AGENT = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) R6Koc-fetch-assets/1.0';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Matching ignores case and punctuation: "kafe-dostoyevsky" == "kafedostoyevsky".
const norm = (s) => String(s).toLowerCase().replace(/[^a-z0-9]/g, '');

async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

// Collects { slug -> imageUrl } from every Tracker segment file of one kind.
async function trackerImages(kind) {
  const files = (await readdir(TRACKER_DIR)).filter(
    (f) => f.startsWith('profile_ubi_') && f.includes(`_segments_${kind}-`) && f.endsWith('.json'),
  );
  const out = new Map();
  for (const f of files) {
    const json = await readJson(join(TRACKER_DIR, f));
    for (const seg of json.data ?? []) {
      const slug = seg.attributes?.[kind];
      const url = seg.metadata?.[kind === 'operator' ? 'operatorImageUrl' : 'mapImageUrl'];
      if (slug && url && !out.has(norm(slug))) out.set(norm(slug), url);
    }
  }
  console.log(`tracker ${kind}: ${files.length} file(s), ${out.size} image URL(s)`);
  return out;
}

// Downloads one image. Returns { ok, reason? }. Never throws.
async function download(url, dest) {
  try {
    const res = await fetch(url, { headers: { 'User-Agent': USER_AGENT } });
    if (!res.ok) return { ok: false, reason: `HTTP ${res.status}` };
    const type = res.headers.get('content-type') ?? '';
    if (!type.startsWith('image/')) return { ok: false, reason: `not an image (${type || 'no content-type'})` };
    const bytes = Buffer.from(await res.arrayBuffer());
    await writeFile(dest, bytes);
    return { ok: true };
  } catch (err) {
    return { ok: false, reason: err.message };
  }
}

// Fetches every key of one kind. Returns the keys that still have no file.
async function fetchKind({ label, keys, dir, ext, trackerMap, cdnPath }) {
  await mkdir(dir, { recursive: true });
  const missing = [];
  let fromTracker = 0;
  let fromCdn = 0;
  let skipped = 0;

  for (const key of keys) {
    const dest = join(dir, `${key}.${ext}`);
    if (await exists(dest)) {
      skipped++;
      continue;
    }

    const trackerUrl = trackerMap.get(norm(key));
    const candidates = trackerUrl
      ? [{ url: trackerUrl, source: 'tracker' }]
      : [{ url: `${CDN}/${cdnPath}/${key}.${ext}`, source: 'cdn' }];

    let done = false;
    for (const c of candidates) {
      await sleep(DELAY_MS);
      const r = await download(c.url, dest);
      if (r.ok) {
        console.log(`  [${label}] ${key}: saved (${c.source})`);
        if (c.source === 'tracker') fromTracker++;
        else fromCdn++;
        done = true;
        break;
      }
      console.log(`  [${label}] ${key}: ${c.source} failed (${r.reason}) ${c.url}`);
    }
    if (!done) missing.push(key);
  }

  console.log(
    `${label}: ${keys.length} key(s), ${fromTracker} from Tracker JSON, ${fromCdn} from CDN fallback, ` +
      `${skipped} already present, ${missing.length} missing`,
  );
  return missing;
}

// Lists files on disk for the manifest: key -> relative path under assets/.
async function filesOnDisk(dir, subdir, ext, keys) {
  const out = {};
  for (const key of keys) {
    if (await exists(join(dir, `${key}.${ext}`))) out[key] = `${subdir}/${key}.${ext}`;
  }
  return out;
}

async function main() {
  const operators = (await readJson(join(ROOT, 'data', 'operators.json'))).operators.map((o) => o.key);
  const opImages = await trackerImages('operator');
  const mapImages = await trackerImages('map');
  // Normalised keys only; every Tracker map is included, even if data/maps.json lacks it.
  const maps = [
    ...new Set([
      ...(await readJson(join(ROOT, 'data', 'maps.json'))).maps.map((m) => norm(m.key)),
      ...mapImages.keys(),
    ]),
  ].sort();

  console.log('--- operators');
  const missingOps = await fetchKind({
    label: 'operator',
    keys: operators,
    dir: OP_DIR,
    ext: 'png',
    trackerMap: opImages,
    cdnPath: 'operators/badges',
  });

  console.log('--- maps');
  const missingMaps = await fetchKind({
    label: 'map',
    keys: maps,
    dir: MAP_DIR,
    ext: 'jpg',
    trackerMap: mapImages,
    cdnPath: 'images/maps',
  });

  const manifest = {
    operators: await filesOnDisk(OP_DIR, 'operators', 'png', operators),
    maps: await filesOnDisk(MAP_DIR, 'maps', 'jpg', maps),
    missing: {
      operators: missingOps,
      maps: missingMaps,
    },
  };
  await writeFile(join(ASSETS_DIR, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', 'utf8');

  console.log('--- summary');
  console.log(`operators with image: ${Object.keys(manifest.operators).length}/${operators.length}`);
  console.log(`maps with image: ${Object.keys(manifest.maps).length}/${maps.length}`);
  if (missingOps.length) console.log(`missing operators: ${missingOps.join(', ')}`);
  if (missingMaps.length) console.log(`missing maps: ${missingMaps.join(', ')}`);
  console.log(`manifest: ${join(ASSETS_DIR, 'manifest.json')}`);
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
