# Wish Engine — User guide

Wish Engine is a free, open-source 3D game engine. Everything in it can be done by hand in the
editor, and everything can also be done by an AI assistant through MCP. Both kinds of change land
in the same history, signed with who made them, and each can be undone on its own.

This guide follows a game from the first start to an exported game.

- [1. Install and first start](#1-install-and-first-start)
- [2. The editor](#2-the-editor)
- [3. Editing by hand](#3-editing-by-hand)
- [4. Playing, replays and tests](#4-playing-replays-and-tests)
- [5. Game code](#5-game-code)
- [6. Working with an AI assistant](#6-working-with-an-ai-assistant)
- [7. Exporting a game](#7-exporting-a-game)
- [8. The command line](#8-the-command-line)
- [9. When something goes wrong](#9-when-something-goes-wrong)
- [10. What is in a project folder](#10-what-is-in-a-project-folder)

## 1. Install and first start

Unpack the Wish Engine folder anywhere and run `wish-engine.exe` (for a shortcut, the icon is
`wish-engine.ico` in the folder). Nothing else needs to be installed: the compiler for game code
ships inside the folder. Drop a project folder on `wish-engine.exe` to open the editor on it. On the
command line the same is `wen edit`.

Without a project, the editor opens on the **welcome screen**:

1. **Who helps you?**
   - **Claude Code**, if it is installed: the editor's Agent panel will use it.
   - **Nobody**: you make the game by hand.

   Any other MCP agent can connect later (project menu, *Connect an outside agent...*).
2. **Start from**:
   - an empty project (a camera, a sun, a ground and starter game code), or
   - a template:
     - *Islands*: a 3D platformer, the showcase;
     - *Hop*: 3D jumping;
     - *Collect*: 3D seen from above;
     - *Bricks*: 2D;
     - *Quiz*: interface only.
3. **Name and place.** New projects go into `Documents/Wish Projects` by default.

Press **Create**. The engine makes the project, builds its game code and opens the editor in the
same window. On the right, **Open a game** lists the projects you opened lately, or takes the path
of any project folder.

The language is chosen at the top right of the welcome screen, and later in the project menu
(*Language*). There are 12 languages.

## 2. The editor

| Place | What it shows |
|---|---|
| Top bar | The project menu (named after the project), the scene, *Undo* / *Redo*, *Play* / *Pause* / *Step* / *Stop*, the view, the gizmo mode (*Move*, *Rotate*, *Scale*, *World* / *Local*, *Snap*), *Agent* |
| Left: *Hierarchy* | The entities of the scene, as a tree. *Search* filters it. |
| Middle | **Scene** view: the scene through the editor's own camera. Click to select; drag the gizmo to move, rotate or scale. In a wide window the **Game** view is beside it: the game as the player sees it, through the scene's camera and with the game's interface (top bar, *Game view* shows or hides it). While the game runs, the scene view keeps the editor's camera, so you can look around the running game. |
| Right: *Inspector* | The selected entity's components and their fields. Each field has its unit and range. |
| Bottom tabs | *Assets*, *Console*, *Problems*, *Performance*, *History*, *Activity* |
| Status line | What the last action did, and the number of errors and warnings |

**Moving around the view:**
- Hold the right mouse button and move the mouse to look around.
- While it is held, **W A S D** move, **Q / E** go down / up, and **Shift** is faster.
- *Reset camera* returns to the scene's camera.

**Shortcuts:**

| Keys | Action |
|---|---|
| W / E / R | Move / rotate / scale |
| Ctrl+Z | Undo |
| Ctrl+Y or Ctrl+Shift+Z | Redo |
| Ctrl+P | Play / stop |
| F2 | Rename the selected entity |
| Delete | Delete the selected entities |
| Escape | Cancel a drag |

**Views** (top bar):
- *Shaded*: the picture.
- *Entity colours*: each entity in its own colour.
- *Depth*, *Normals*, *Wireframe*.
- *Colliders*: physics shapes.
- *Lights*: light ranges.

## 3. Editing by hand

- **Entities:**
  - *Add an entity* in the hierarchy, or *Add child* on a selected one.
  - Drag in the hierarchy to change parents.
  - *Rename* (or F2) changes an entity's name.
- **Components:**
  - *Add component...* in the inspector (type to search), *Remove component*.
  - *Copy values* / *Paste values* copy a component between entities.
  - *Reset to defaults* resets a component.
  - The game's own components show up here too, with no editor code: their fields come from
    the game code's schema.
- **Scene resources:** settings that belong to the scene, not to an entity, such as gravity,
  sky and ambient light. They are under *Scene resources*.
- **Assets** (bottom tab):
  - Models, images and sounds of the project, filtered by kind.
  - Drag one into the view to place it.
  - *New...* makes a placeholder sound or picture.
  - *Rename / move...* moves a file and updates every reference to it.
  - *Open outside* opens it in the program your system uses.
- **Prefabs:**
  - *Make prefab* turns the selection into a reusable piece.
  - Instances show *Instance of {prefab}*; their changed fields are marked.
  - *Revert to prefab* undoes an instance's changes.
  - Opening a prefab from *Assets* edits it like a scene.
- **Notes:** each entity can carry free-form notes (the *Notes* box in the inspector). An agent
  reads them too.
- **Project settings** (project menu, *Settings...*):
  - the name;
  - the default scene;
  - the **input map**: actions such as `move` or `jump`, and the keys that drive them.
    - *Add action* / *Remove action*.
    - Key lists are written like `Space, W, Up`.
    - *Save input map*.
- **History** (bottom tab):
  - Every change: who made it (you, an agent, or a file edited outside) and when.
  - *Undo this* takes back one change on its own, even an old one, as long as the value it set
    is still there.
- **Checkpoints** (project menu, *Checkpoints*):
  - *Save checkpoint* keeps the whole project as it is.
  - *Go back to this* returns to it; going back is itself a change in the history, so it can be
    undone.
  - *Compare with now* shows the checkpoint and now side by side.

Files are the source of truth. Editing a scene file in another program is fine: the editor notices,
the change enters the history as *file*, and a broken file is reported, never overwritten.

## 4. Playing, replays and tests

- **Play** runs the game in the view at 60 ticks a second.
  - Keys drive the actions of the input map.
  - **Pause**, **Step** (one tick) and **Stop**: stopping returns the scene to how it was.
  - Changes you make while playing are live and are not kept.
- **Timeline:** while playing, you can go back to an earlier tick. The game is deterministic: the
  same scene and seed always play out the same way.
- **Replays:**
  - *Record* records a run, keys included.
  - Saved replays are listed under *Replays* and can be played back.
  - A replay checks every tick against the recording and says the first tick that differs.
- **Tests:**
  - `tests/*.test.json` are scenario tests: play from a scene, press keys, expect something.
  - *Run tests* runs them all; a failure says the step, the tick and the values it read.
  - Picture steps compare with golden images; *Update golden pictures* saves new ones.
- **A window of its own:** `wen play` plays the game in its own window. Saving the game code
  switches to the new code without stopping.

## 5. Game code

Game logic is Rust in `scripts/src/lib.rs`, built to WebAssembly by the engine.

- **Building:**
  - *Build code* in the project menu, or `wen build`.
  - Errors show with file, line and a hint.
  - A failed build keeps the last good code running.
- **Editing:**
  - *Edit code* opens the file in your editor (set `WISH_EDITOR`, for example to `code`).
  - Saving while playing reloads the code without losing the game's state.
- **What code can do:** declare components, resources and events (`#[component]`, `#[resource]`,
  `#[event]`) and write systems (`#[system]`) that query entities, read actions, send commands and
  events, cast rays, play sounds and set interface text.
- **Safety:** game code runs in a sandbox. It cannot read your files, cannot freeze the engine,
  and a crash in it is reported with the line it happened on.
- **More:** the SDK reference is at `wish://docs/sdk` and worked recipes are at
  `wish://docs/recipes`. Both can be read with `wen call docs_search --query <words>`, or by an
  agent.

## 6. Working with an AI assistant

### The AI mode

The switch in the top bar changes the editor's face: **Manual | With AI**. Same project, same history, same files; only the screen changes. Your choice is remembered.

*With AI* puts the manual panels aside and shows:

| Where | What it is for |
|---|---|
| **The game** (left), *Game card* | What the game is: name, one sentence, genre, the player's goal, core loop, controls, look and feel, for whom, must have, later. The agent reads it before every task. *Let the agent draft it* writes it from the game as it is. |
| **The game** (left), *Decisions* | Rules that hold for every change ("jump height 2.5 m", "pastel colours"). The agent keeps to them. |
| **The live game** (middle) | What the player sees. Switch to *Scene* and click something: it goes with the next request. |
| *Plan* (below) | Small steps. *Do the next step* gives the first one not done to the agent. |
| *Play-test notes* (below) | While playing, *Take a note*: the moment's tick and picture are kept. *Send the notes to the agent* makes them one task. |
| *Health* (below) | Problems, whether the code is built, tests. *Let the agent fix it* sends them as one task. |
| **Conversation** (right) | Each task is a card: what was asked, what the agent said and which tools it called; *Undo this task* and *Before / after*. Ready requests (*Add a level*, *Game feel*, *Find bugs*...) fill the box. |
| *The agent asks* | For choices that are yours (difficulty, look, how many), the agent does not guess: it asks with choices. Click one or write your own; with *Keep the answer as a decision* it becomes a decision, and the agent goes on. |
| *AI settings* | The assistant, the model (Opus / Sonnet / Haiku), remembering the conversation between tasks, a checkpoint before every task, whether the agent may change the game code itself, what this session cost. |

The design is the project's `design/game.json` (in git, not an asset). Agents do the same with the `game_design` tool.

### The Agent panel (in the manual mode)

**The Agent panel** (*Agent* in the top bar):
1. Write what you want in *What should the agent do?* and press *Send*.
2. *Point in the view*, then click entities, to show the agent what you mean.
3. The transcript shows what the agent says and every tool it calls.

A **checkpoint is saved before every task**, so you can always go back.

**Permission modes** (project menu, *Permissions*):

| Mode | What happens |
|---|---|
| *Only suggest* | The agent proposes; nothing changes until you *Accept* (or *Reject*) in the *Activity* tab |
| *Apply, I review* | The agent's changes are made; you review them in the *Activity* tab |
| *Autonomous* | The agent works freely; everything is still in the history |

**Activity** (bottom tab):
- Shows, live, what everyone did.
- *Agents only* hides your own changes.
- Each card can be undone on its own.

**An outside agent:**
- *Connect an outside agent...* shows an address and a token: give both to any MCP client.
- Claude Code in the project folder needs nothing: the project's `.mcp.json` starts `wen mcp`
  itself.
- The agent uses the same tools the editor uses, about 30 of them. Every one of them can also be
  done by hand.

The engine is complete without any AI. An assistant is a faster way to do the same things.

## 7. Packages, themes, workspaces and the command palette

**Packages** bring themes, workspaces, languages, recipes and templates. A package is a folder with a `wish-package.json`.
- *Project menu → Packages...* lists what is installed and installs a package from its folder: *Install into the project* (it travels with the project: `packages/` and `wish.lock` record exactly which files), or *Install for me* (for every project you open on this computer).
- *Remove* moves a package to the trash; nothing is deleted.
- The project's lock is checked: if a package's files change, the window says so; *Write the lock again* if the change is yours.
- **The package index:** under *From the package index*, name the index folder (a folder of packages; it is remembered), type a few words and press *Search*. Each package shows its newest version that works with this engine: *Add to the project*, *Add for me*, or *Update* when a newer compatible version is there. The Hub's *Packages* tab has the same search for your own packages.
- On the command line: `wen pkg search dark theme --index <folder>`, `wen pkg add <id>`, `wen pkg update`, `wen pkg remove <id>`, `wen pkg lock` (`--user` for your own packages; `WISH_PACKAGE_INDEX` saves typing `--index`).

**Engine versions:** several versions of the engine can live side by side. The Hub's *Engine versions* tab lists them; *Add an engine* remembers another engine's folder where it is (the folder with `wen` and `wish-engine.json`; nothing is copied), *Remove* forgets one. A project opens with the version it was made with (its `wish.json` says which); if that one is not on the computer, with the newest compatible one; otherwise this engine opens it and says so. The recent projects list shows when a project was made with another version. On the command line: `wen engines`, `wen engines add <folder>`, `wen engines pick <project>`.

**Themes:** *Project menu → Theme* switches the editor's look at once: Atelier (the default), Midnight, High contrast, and any theme a package brings.

**Workspaces:** in the AI mode, the *Workspace* box above the game card picks what the card is for. *Story game* asks for the setting, characters, tone, choices and endings, and brings its own ready requests; *Default* is the engine's card for any game. Nothing you wrote is lost when you switch.

**Command palette:** Ctrl+K opens it. Type a few letters of what you want (*export*, *ai mode*, *tests*), choose with the arrow keys, Enter runs it. Shortcuts: Ctrl+P play/stop, Ctrl+Z/Ctrl+Y undo/redo, Ctrl+M manual/AI mode, Ctrl+B build code. To change one, add it to your preferences file: `"keys": { "mode.switch": "Ctrl+J" }`.

## 8. Exporting a game

Project menu, *Export game* (or `wen export`):
- The project is checked first.
- The game code must be built.
- The result is `exports/<name>/`:
  - `<name>.exe`: the player;
  - `game/`: scenes, assets and the built code;
  - `LICENSES.txt`.

To share the game, zip that folder. It runs on a machine without the engine, the editor or a
compiler. It plays exactly as it did in the editor.

## 9. The command line

`wen` does everything the editor does. A few useful commands:

```bash
wen new my_game --template islands
```

```bash
wen edit --root my_game
```

```bash
wen play --root my_game
```

```bash
wen check --root my_game
```

```bash
wen test --root my_game
```

```bash
wen export --root my_game
```

`wen call <tool>` runs any control-plane tool and prints its JSON answer, exactly as an MCP client
receives it. For example, `wen call project_info --root my_game`. An unknown tool name lists them
all. `wen help` lists every command.

## 10. When something goes wrong

Every problem the engine reports has:
- a **code** (such as `unknown_component`);
- a message;
- where it is: file, line and field;
- a **hint**: what to do about it.

If the engine itself ever stops on an unexpected error, it writes a crash report on your
computer (in the `crashes` folder next to the editor's settings; an exported game writes it next to
itself) and says where the next time the editor opens. Nothing is sent anywhere: attach the file to
an issue if you want to help fix it.

The *Problems* tab lists them for the whole project. `wen call docs_search --query <code>`
explains a code.

| Seen | Usually |
|---|---|
| *No component named 'X' is registered* | The game code that defines X is not built: *Build code* |
| *the game code was not built* | The compiler said why on the same line; fix the line it names |
| *Claude Code was not found* | Install Claude Code and sign in once, or set `WISH_CLAUDE` to its path |
| *not exported: ... errors* | Fix what *Problems* lists, then export again |
| The view is black | Look in *Problems*: the scene may have no camera |

## 11. What is in a project folder

```text
wish.json             the project: name, default scene
scenes/               scenes (*.scene.json) and prefabs (*.prefab.json)
assets/               models (.glb/.gltf), images (.png), sounds (.wav/.ogg)
scripts/src/lib.rs    game code
input.json            actions and their keys
tests/                scenario tests (*.test.json) and golden pictures
replays/              recorded replays
exports/              exported games (not part of the project)
.wish/                what the engine makes (builds, checkpoints); not shared
.mcp.json, CLAUDE.md  how an AI agent connects and works here
```

Every file is canonical JSON, so git shows clear differences between versions.
