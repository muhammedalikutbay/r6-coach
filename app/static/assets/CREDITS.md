# Asset credits

R6 Coach is a free, open-source, non-commercial fan project. It is not affiliated with or endorsed
by Ubisoft. Rainbow Six Siege and everything that comes from the game belong to Ubisoft
Entertainment SA and its affiliates. The code of this repository is under the GPL-3.0 licence
(see `LICENSE` at the repository root); that licence does **not** cover the game artwork below.

## Operator badges (`operators/*.png`) and map images (`maps/*.jpg`)

Game artwork from Rainbow Six Siege, copyright Ubisoft. They are used here only to show the
player's own operators and maps next to their stats, for non-commercial fan purposes.

- Source: the Tracker Network CDN (`trackercdn.com/cdn/r6.tracker.network/...`), the same images the
  Tracker Network site shows. The download script is `tools/fetch-assets/fetch.mjs`. It ran once,
  and the app never fetches these images at runtime.
- `manifest.json` lists which key has which file, and which keys have no image.

## Rank icons (`ranks/*.png`)

The Ranked rank badges (seasons Y7S4 and later: Unranked, Copper V to Diamond I, Champion) are game
artwork from Rainbow Six Siege, copyright Ubisoft.

- Source: Ubisoft's public CDN (`staticctf.akamaized.net/J3yJr34U2pZ2Ieem48Dwy9uqj5PNUQTn/...`, the
  `R6S_RANK_*` / `RANK_L_*` images). The URL list came from the MIT-licensed
  [danielwerg/r6data](https://github.com/danielwerg/r6data) `src/ranks.ts` (`iconOfficial`). They
  were downloaded once on 2026-11-07; the app never fetches them at runtime.
- File names are the rank icon keys the app uses: `<tier>-<division>` (for example `emerald-2`),
  plus `champion` and `unranked`. A missing file falls back to a neutral badge.

## Use and removal

- The images are not sold and not offered as a separate download; they are redistributed only as part
  of this source repository and the app built from it. Do not reuse them commercially.
- If you are the rights holder and want any of them removed, open an issue or contact the maintainer
  (`muhammedalikutbay` on GitHub) and they will be removed promptly. To remove one yourself, delete
  the file and its entry in `manifest.json`; the app then shows a neutral placeholder with initials.
