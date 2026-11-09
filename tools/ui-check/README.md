# ui-check

Drives the R6 Coach window without a human, through WebView2's remote debugging port and the
Chrome DevTools Protocol (CDP). Node 24+, no npm packages.

## Run

From PowerShell, at the repo root (the app needs `app/node_modules`; run `npm install` in `app/` once):

```powershell
node tools/ui-check/run.mjs --scenario smoke
node tools/ui-check/run.mjs --scenario smoke --port 9444          # avoid a port clash
node tools/ui-check/run.mjs --scenario smoke --exe C:\path\to\r6koc.exe   # test a built exe
```

Options: `--scenario <name>` (file in `scenarios/`, default `smoke`), `--port` (default 9333),
`--exe <path>` (launch this exe instead of `npm run tauri dev`), `--startup-timeout <seconds>` (default 300;
the first Rust build is slow).

Exit code 0 = every step passed, 1 = a step failed (a `failure.png` is saved), 2 = bad arguments.

The tool sets `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` and a private
`WEBVIEW2_USER_DATA_FOLDER` (`out/webview2-<port>`), so the profile is isolated and starts clean.
When done it kills only the process tree it started (`taskkill /PID <pid> /T /F`), never by name.
Note: `tauri dev` also needs the Vite dev port (1420) free.

## Scenario format

`scenarios/<name>.json`: `{ "steps": [ ... ] }`. Steps:

| type | fields | meaning |
|---|---|---|
| `wait` | `selector` or `text`, `timeout` (ms, default 10000) | wait until a visible match exists |
| `click` | `selector` or `text` | click the first visible match (text = smallest visible element containing it) |
| `eval` | `expr`, optional `expect` | evaluate JS (promises awaited), print the result; fail if it differs from `expect` (JSON equality) |
| `screenshot` | `path` | PNG via `Page.captureScreenshot`; relative paths go to `out/` |
| `reload` | | reload the page and wait for `readyState = complete` |
| `viewport` | `width`, `height` | emulate a viewport size (`width` 0 clears it) |
| `sleep` | `ms` | wait |
| `localStorage.clear` | | clear the page's localStorage |

Screenshots and the WebView profile live in `tools/ui-check/out/`, which is gitignored.
