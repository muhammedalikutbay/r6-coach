# R6 Coach v7 — coaching Home, tray app, clip timing fix, public release

Status: user feedback 2026-11-27 after testing v6. Builds on `docs/v6-spec.md`.

## User feedback (2026-11-27)
1. Clips settings: do not disable the inputs while the Clips switch is off. (They stay locked only
   while the game is being recorded, v6.)
2. Remove the sidebar **Update** button: imports are automatic now.
3. Arenyze data updates automatically only once, when the app starts (the free plan has a monthly
   call limit). A manual refresh on the Profile page stays.
4. Closing the window keeps the app running in the system tray, like Discord. To quit fully: tray
   icon > right click > **Quit**.
5. Clips still drift in the real game after alt-tab. The user suspects the game's frame rate drop.
   Study the code and fix it if possible.
6. The current Home page depends on Arenyze: without a key the first screen is an empty Arenyze page
   that asks for a key. Rename it to **Profile**. The new **Home** is a coaching-focused page that
   serves the app's purpose (R6 *Coach*) with local replay data only.
7. A first-run **tutorial**.
8. Remove the "R6 COACH" text from the left sidebar.
9. Bilingual README (English + Turkish) with in-app screenshots, like the Baytar repository.
10. Contributor: `yavuzhanalbayrak` (tests and ideas).
11. Commit, push, and make the repository public.

## Clip drift: cause and fix
`windows-capture`'s `VideoEncoder::send_frame` stamps each frame with its capture time and hands it
to a Media Foundation transcoder that expects a constant frame rate (30 fps). When fewer frames
arrive (the game's frame rate drops after alt-tab), Media Foundation re-times the stream (the v5
note in `RecorderConfig::fps` measured content up to 1 s ahead). The cutter maps the encoder's
timeline to the wall clock through the capture times, so every clip cut after such a stretch in the
same segment is shifted.

Fix: the encoder must never see an irregular stream. The recorder rate-limits frames to the clip
frame rate itself and stamps the n-th sent frame at `n / fps` (a perfect constant-rate stream, so
Media Foundation keeps one sample per frame). The `.clock` sidecar keeps the real wall time of every
sent frame, and the cutter builds the clip's own timeline (video and audio) from those wall times.

## Decisions
- Arenyze key: never stored in the repository, docs, logs or test fixtures.
- Screenshots in the README use anonymized player names (no real gamer tags of other players).
- Not affiliated with Ubisoft: the README carries a disclaimer for game names, operator and map art.
