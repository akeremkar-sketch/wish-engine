# M12: the editor redesign before 1.0

**Status:** mockups drawn, waiting for the user's choice of direction
(20 November 2026).

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

Three mockups on one canvas (an Artifact, private to the user):

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

## 4. Next

The user picks a direction (or a mix). Then: a package plan (the theme
tokens, the menu bar and command ids, the scene camera, the inspector, the
dock, the AI surface), the recommended model, and the code, with the
editor's rules kept (every action a `Workspace::call`, `t("...")` texts in
all 11 languages, commands in `COMMANDS`, tests that find widgets by name,
golden images refreshed and looked at).
