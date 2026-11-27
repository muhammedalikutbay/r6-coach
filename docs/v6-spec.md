# R6 Coach v6 — test feedback on v5, custom window, welcome screen

Status: user feedback 2026-11-10 after testing v5. Builds on `docs/v5-spec.md`.

## User feedback (2026-11-10)
1. Home: the profile photo does not show.
2. Home: picking a season in **Seasons** filters the Season overview but not the Operators table.
   The Operators table must follow the picked season too, and keep its own "All seasons" choice.
3. Overview keeps only **Operators, Maps, Weapons, Recent matches** (in that order). Remove
   Round impact, Timing and Rivals from Overview. (This replaces the v5 "keep all sections" rule.)
4. Replace the "Happy Birthday!" first-run screen with a proper welcome / onboarding screen for any
   player.
5. Operators list: clicking anywhere on a row opens the operator (today only the name cell works).
   Weapons list: the same for weapon rows.
6. Weapons screens: show the icons of the operators that carry the weapon.
7. Match detail scoreboard: the per-round operator icons now wrap into two columns. Show them side by
   side in one row in round order, and clicking an icon opens the Rounds tab at that round.
8. Clip player navigation: opened from Matches > Rounds, step between the clips of the same round;
   opened from an Operator or Weapon screen, step between that screen's clips.
9. App icon: use the maintainer's own icon file (now in `app/src-tauri/icons/`).
10. No standard Windows title bar. A custom title bar that fits the app design, with minimize,
    maximize/restore and close.
11. Clips settings: while the recorder is running ("Recording the game window"), the clip settings
    cannot be changed. Recommended defaults: 10 s before, 0 s after, offset -5 s.

## Decisions
- Avatar: the user asked for the profile photo, so the app may load that one image from the URL
  Arenyze returns in `account.profilePicture` (`https://avatars.ubisoft.com/<profileId>/default_146_146.png`).
  No other Ubisoft request. If the image fails, show the initials fallback.
- All other network rules from v4/v5 stay: Arenyze only, with the user's key; never Tracker.

## Unknown ids from the user's export (2026-11-10, 40 matches, 1459 kills, 1418 named)
Maps (Quick Match, Y11S3) with their bomb sites:
- 430788891316: "1F Armory, 1F Throne Room", "1F Lab, 1F Storage"
- 441842102525: "1F Dining, 1F Kitchen", "2F Administration, 2F CEO Office" (likely Emerald Plains)
- 446546313105: "1F TV Room, 1F Music Room", "2F Car Room, 2F Pink Room", "2F Master Bedroom, 2F Car Room"
Weapons:
- Solid Snake (444310693746): primary asset 1366019244 (3 kills), secondary item 449477444912 (3 kills)
- Noor (456757346397): primary asset 127174503954 (3), asset 168013421941 (1), items 445003709078 (2),
  452023047367 (1)
- Brava: asset 133651069300 (1 kill, no slot); a few kills with unknown operator.
