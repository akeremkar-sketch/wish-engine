# M12 package 13: the beta, tested by the agent (report)

**Date:** 21 November 2026. **Why:** the user has no outside testers and
asked the agent to test ([M12-beta-and-release.md](M12-beta-and-release.md)
section 5). The user listened while it ran and reported the first finding.

## 1. How it was tested

1. The Windows archive was built as the release workflow builds it (the
   `dist` profile, `+crt-static`, `wen package`), checked against its
   `SHA256SUMS` and unpacked with PowerShell's `Expand-Archive` into
   `...\tester\Wish Engine\` (a path with a space).
2. **From the unpacked copy alone**, with the developer's Rust taken off
   `PATH` (`RUSTUP_HOME` and `CARGO_HOME` pointed at an empty folder): `wen
   version`, `wen new` into a path with Turkish letters (`İlk Oyun`), `wen
   build` (5 s), `wen test` (1/1), `wen export`, and the exported game
   headless for 60 ticks. All passed.
3. **By hand in the real window** (the desktop seen and clicked through),
   with a fresh settings folder (`WISH_CONFIG_DIR`): the tester's guide's
   hour: the opening, a project from the Islands template, Play, a new
   scene, the Create menu, the gizmo, Duplicate, the wheel and F, a field of
   the inspector, undo and redo, a deliberate compile error and its fix, Game
   → Build code, File → Export game, and the exported game in its window.
   The AI step was looked at, not run (it spends the user's quota).

## 2. Findings

| # | Sev. | Finding | Outcome |
|---|---|---|---|
| S1 | high | **Heard by the user:** the showcase (Islands, also the template) played an ambient "wind" that was white noise in a six-second loop that faded out and in: it sounded like broken audio ("a bad hiss, and a loop"). The mixer was right; the content was a placeholder. | Fixed: the `wind` entity removed with `edit_apply` (its reason recorded), `assets/sounds/wind.wav` deleted; `wen check` clean, the four scenario tests pass. No other example loops a sound. |
| B1 | high | With the game view beside the scene view (the default in a wide window), keys pressed with the pointer over the **game view** went nowhere: the editor gave the game only the keys pressed over the scene view. The first thing a tester does after Play is click the game. | Fixed: keys and buttons go to the game over either view. Test `keys_over_the_game_view_go_to_the_game` (a wide window; the old test only tried the scene view). |
| B2 | medium | The editor used a whole core while idle (92 % measured), and a game window drew as fast as the loop could spin. Both loops asked for a redraw on every wake-up, which woke them again at once, so they never slept. | Fixed in `wish_window`: the editor's shell asks for a frame only when its 16 ms tick is due and sleeps until then; the viewer (games, `wen play`, exported games) draws at most every 16 ms and sleeps between. Measured headless: an idle editor draws no frames; the window loop is checked by its CPU use on the desktop. |
| F6 | medium | A compile error in the Problems tab named its file but not its line ("Fix the line named." with no line named), though the diagnostic carries line and column. | Fixed: `scripts/src/lib.rs:351:22`. In `manual_only.rs` (a real compile error). |
| F8 | medium | After a clean build, the Problems tab still showed the last build's error. | Fixed: a clean build reads the project's problems again (a build with warnings shows its warnings). Same test. |
| F7 | low | The status bar said "0 error(s)" while the Problems tab listed a compile error (it counted the project's check, not the list). | Fixed: it counts what the Problems tab lists. Same test. |
| F11 | low | An exported game's window was titled "Beta Islands - lit": the viewer's debug view name. | Fixed: a game being played is titled by its name alone. |
| F4 | low | A cube or sphere from the Create menu was half sunk into the ground (made at the origin). | Fixed: half a metre up. Test in `menus.rs`. |
| F5 | low | A rotation of nothing read "-0.0 deg" in the inspector. | Fixed. Test in `inspector.rs`. |
| F3 | low | New scene opened the scene but the status bar still showed the previous message. | Fixed: "new scene scenes/scene_1.scene.json" (11 languages). Test in `menus.rs`. |
| F1 | low | The tester's guide did not say that the archive holds one folder with the programs inside. | Fixed in both guides. |
| F10 | low | An exported game's window has the system's plain icon. | Recorded for release 1.1: a game's own icon is a setting the project does not have yet. |
| N1 | note | Under the automation tool, the window lost focus between steps; the first click after that sometimes opened the wrong menu. With the window focused, menus open correctly every time, and a headless probe with the press and release in separate frames is correct. | Not reproduced by hand; to be checked by a person. |
| N2 | note | Duplicate puts the copy exactly on the original (as Unity does), so nothing seems to happen; the wheel zooms toward the view's middle, not the pointer. | Kept for 1.0; noted for the editor's next release. |

## 3. What this test cannot say

The agent knows the code and used one machine (Windows 10, an RTX card).
It cannot stand in for a stranger: the formal tests with outside people
stay at release 1.4. Linux was tested only by the release workflow's
smoke run. Sound was judged by the user's ear on this machine only.
