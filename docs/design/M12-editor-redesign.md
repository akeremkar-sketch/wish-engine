# M12: the editor redesign before 1.0

**Status:** direction A (Workshop) chosen by the user, 20 November 2026;
packages R1 and R2 done; R3 in progress.

## 1. Request and scope

User request, 20 November 2026: redesign the whole editor interface before
1.0. Answers to three questions the same day: both the look and the layout;
the engine's own style (the wishing wand's purple identity, a familiar
editor arrangement); mockups first, code after a direction is chosen.

**This grows the 1.0 scope.** The short path (M12 note, section 0) left the
editor as it is at 1.0 and put "the editor's first hour" (orbit, pan, zoom,
frame; a Create menu; duplicate; new scene; the menu split and HiDPI) in
release 1.1. The redesign pulls that package into 1.0 and adds a new look.
It runs before the beta (package 13), so the beta tests the new editor.

## 2. What is wrong today

Read from the golden image `crates/wish_editor/tests/golden/editor_window.png`
and the README pictures `docs/images/editor.png`, `editor-ai.png`:

- Several accents compete: orange (`accent`), teal (`select`), green, red;
  the brand purple of the logo appears nowhere else.
- No menu bar: the project menu hides behind the project name; there is no
  Create menu, no duplicate, no new scene.
- The scene view has no camera controls a person can find (no orbit, pan,
  zoom or frame) and its tools sit in the top bar, far from the view.
- The inspector shows schema names and raw values (`mask 4294967295`,
  `density 1000.0 kg/m3` before the shape); there are no groups, no
  "advanced" fold, no axis colours on vectors.
- The bottom panel takes a fixed band with mostly empty space; the status
  bar says only the count of problems.
- Manual and With AI are two different screens; the agent is a separate
  panel in the first and the whole right column in the second.

## 3. Directions drawn

Three mockups of the editor on one canvas (an Artifact, private to the user):

- **A · Workshop.** The familiar three columns: a menu bar (File, Edit,
  Create, View, Game, Help) and Play in the middle of the top bar; Scene
  tree with a filter and a Create button; the view with its own floating
  tool strip and camera hints; an inspector of cards with plain labels,
  units, axis colours and an "advanced" fold; assets as tiles in a bottom
  dock; a status bar that says what was saved.
- **B · Focus.** The view first: an icon rail opens one side panel at a
  time; a command bar in the middle of the top (Ctrl+K); the agent is a
  prompt bar inside the view, aware of the selection, so manual work and
  AI share one screen; the console folds into the status bar.
- **C · Studio.** Workspaces as tabs (Build, Look, Play-test, Make with AI,
  Ship) that rearrange the panels; Scene and Game views side by side; a
  **tick timeline** under the views that shows the recorded run, its events
  and a playhead to go back to any tick (determinism made visible:
  snapshots and replays already exist in the control plane).

All three share one palette: a purple accent (`#6F4FF0` for filled
controls with white text, `#9C82FF` for lines and selection, `#B9A8FF` for
accent text), cool dark greys, green only for "saved", red only for
recording and errors, and the axis colours.

## 4. The opening, the start screen and the details

Second user request the same day: the start screen is redesigned too, the
mark gets a beautiful opening animation when the engine starts, and the
interface gets this kind of care throughout. Two more boards on the canvas,
shared by every direction:

- **Opening animation.** The mark is the wishing wand of `vector::draw_wand`
  (an ivory rod, its grip, a five-pointed gold star at the tip; never a
  four-pointed sparkle, never a continuous arc at rest). The animation keeps
  the code's story (the wand swings up, its trail breaks into four motes of
  dust as it stops with a small flick, the star turns into place and its
  light glows once) and adds the name and a progress line under it. The
  grip and the trail move from orange to the purple accent, so the mark and
  the interface share one colour; the star and the motes stay gold. With
  "reduce motion" the still mark shows at once.
- **Start screen (the Hub).** A side bar with the mark and the version;
  "What shall we make?" with New project and Open a folder; a "Make a wish"
  box that starts a project with the AI helper; templates as picture cards;
  recent projects with their path, engine version (a project that opens
  with another engine says so) and an Open button; the AI helper's state.
- **Details everywhere.** One mark in the window icon, the opening, the
  Hub and the top bar; focus rings for the keyboard; tooltips that name the
  shortcut; empty panels that say the next step; a short notice for saves
  and undo; panel changes animated briefly and never with "reduce motion".

## 5. Decision and packages

**The user chose A · Workshop** (20 November 2026), with the opening and the
start screen of section 4. Five packages, each verified, committed and
pushed on its own; the editor's rules hold throughout (every action a
`Workspace::call`; new texts with `t("...")` in all 11 languages; a new
action gets a command id in `COMMANDS`; tests find widgets by name; golden
images refreshed with `UPDATE_GOLDEN=1` and looked at).

1. **R1 · The look.** *Revised the same day after the user's review:* the
   first pass (a vivid purple accent, large rounded cards, pills, roomy rows)
   "looks made by AI; it should look like professional work". A first code
   attempt of that pass was taken back before any commit. The professional
   pass, redrawn as board A: neutral graphite greys with no tint (`#1C1D20`
   ground and fields, `#25262A` panels, `#2C2D32` headers, `#36373D` lines),
   one calm blue for selection and the active tool (`#2F4E85` rows,
   `#3D6FD0` checks), green only on the play icon, red only on record and
   errors, the axis colours; 3 px corners; 22 px rows, 24 px section
   headers, 20 px fields; panels with tab strips, collapsible inspector
   sections instead of cards, numbers in a monospace face; purple only in
   the mark. Atelier and `packages/wish.theme.atelier/atelier.json` change
   together (a test compares them); Midnight and High contrast stay.
2. **R2 · The top bar and menus.** A menu bar (File, Edit, Create, View,
   Game, Help) from `COMMANDS`; Play, Step and Record in the middle; the
   command palette as a search field (Ctrl+K); Manual | With AI on the
   right. The Create menu (empty, shapes, camera, lights, text, sprite,
   sound, particles), Duplicate and New scene are `edit_apply` and
   `asset_create` calls (the editor's first hour).
3. **R3 · The scene view.** The tool strip floats in the view (move, rotate,
   scale, world/local, snap); camera, shading and Scene | Game at its top
   right; an orbit camera (drag, right-drag to pan, wheel to zoom, F to
   frame the selection) and a hint line.
4. **R4 · Panels.** The hierarchy with a filter and counts on groups; the
   inspector as cards with plain labels, units, axis colours on vectors, an
   "advanced" fold and prefab changes marked; assets as picture tiles; a
   status bar that says what was saved.
5. **R5 · Opening, Hub and details.** The opening animation with the name
   and a progress line, the Hub of section 4, tooltips that name shortcuts,
   focus rings, empty states with a next step, notices for saves and undo,
   "reduce motion" respected; README pictures and both guides updated.
