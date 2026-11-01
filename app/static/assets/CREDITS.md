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
