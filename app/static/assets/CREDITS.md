# Asset credits

The operator badges (`operators/*.png`) and map images (`maps/*.jpg`) in this folder
are game artwork from Rainbow Six Siege. They belong to Ubisoft (Ubisoft Entertainment
SA and its affiliates). R6Koc does not claim any rights in them.

- Source: the Tracker Network CDN (`trackercdn.com/cdn/r6.tracker.network/...`), the
  same images the Tracker Network site shows. The download script is
  `tools/fetch-assets/fetch.mjs`. It ran once, and the app never fetches these at runtime.
- `manifest.json` lists which key has which file, and which keys have no image.

Use: this is a private, personal gift app for one player. It is not distributed,
published or sold, and these images must not be redistributed or used commercially.
If you are the rights holder and want them removed, delete the files and the entries
in `manifest.json`; the app falls back to a neutral placeholder with initials.

## Rank icons (`ranks/*.png`)

The Ranked rank badges (seasons Y7S4 and later: Unranked, Copper V to Diamond I, Champion)
are game artwork from Rainbow Six Siege and belong to Ubisoft.

- Source: Ubisoft's public CDN (`staticctf.akamaized.net/J3yJr34U2pZ2Ieem48Dwy9uqj5PNUQTn/...`,
  the `R6S_RANK_*` / `RANK_L_*` images). The URL list came from the MIT-licensed
  [danielwerg/r6data](https://github.com/danielwerg/r6data) `src/ranks.ts` (`iconOfficial`).
  They were downloaded once on 2026-11-07; the app never fetches them at runtime.
- File names are the rank icon keys the app uses: `<tier>-<division>` (e.g. `emerald-2`),
  plus `champion` and `unranked`. A missing file falls back to a neutral badge.
