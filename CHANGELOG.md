# Changelog

## 0.7.0 - 2026-11-27

The first public version. The numbers below are not releases: the project went through seven
iterations (v1 to v7) before this one, and 0.7.0 marks the seventh.

### New in v7
- **Home** is now a coaching page that works from your replays alone: your last match, the one thing
  to work on next, what your coach says and how your last matches compare with your own history. The
  old Arenyze page is now **Profile**.
- A first-run **tutorial** (a short tour of nine steps) and a clearer welcome screen. You can replay
  the tour from Settings.
- Closing the window keeps R6 Coach running in the **system tray**, so replays keep being imported and
  clips keep being cut. To quit completely, right-click the tray icon and choose **Quit**.
- Arenyze data refreshes once when the app starts (the free plan has a monthly call limit). The
  refresh button on Profile still works.
- The sidebar **Update** button and the "R6 COACH" text are gone: new matches are imported by
  themselves a few seconds after a match ends.
- Clip settings stay editable while the Clips switch is off (they lock only while the game window is
  being recorded).
- Clips drift fix: the recorder now writes a constant frame rate and keeps the real time of every
  frame, so clips stay on the kill even after the game's frame rate dropped (for example after
  alt-tab).

### Added in v6
- Custom title bar, new app icon, welcome screen for any player.
- Profile photo from Arenyze, season filter that also drives the operators table.
- Operator and weapon rows open on a click anywhere; weapon screens show who carries the weapon.
- Match scoreboard shows each player's operators in round order; click one to jump to that round.
- Clip player steps between the clips of a round, an operator or a weapon.

### Added in v5
- Dedicated operator and weapon screens with stats and their clips.
- Map and weapon names for ids that were unknown before; kill feed shows weapons.
- Clip playback fixes, game audio in clips, centered Settings.

### Added in v4
- Rank and season stats through your own free Arenyze key (stored in Windows Credential Manager).
- Kill clips: the app records the game window and cuts a short clip of each kill (and death, if you
  choose). Off by default.
- A coach that compares everything it can read from the replays, ranked by importance.

### Added in v3
- Works for any player: the tracked player is the one who recorded the replay.
- Automatic import when the replay folder changes; unfinished matches are skipped safely.
- Date and time-of-day filters; collapsible Overview sections; weapon of every kill in the kill feed.

### Earlier
- v2: Tracker-style Overview, Matches, Operators, Weapons, one global filter bar, operator and map
  images, the first coach.
- v1: a command-line prototype that read replays and gave short advice.
