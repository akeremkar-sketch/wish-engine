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
- [7. Packages, themes, workspaces and the command palette](#7-packages-themes-workspaces-and-the-command-palette)
- [8. Exporting a game](#8-exporting-a-game)
- [9. The command line](#9-the-command-line)
- [10. When something goes wrong](#10-when-something-goes-wrong)
- [11. What is in a project folder](#11-what-is-in-a-project-folder)
- [12. How the claims in this guide are checked](#12-how-the-claims-in-this-guide-are-checked)

## 1. Install and first start

Download the archive for your system from the repository's
[Releases page](https://github.com/akeremkar-sketch/wish-engine/releases) (the first is the beta
0.9.0), or build the engine from source as the [README](../../../README.md#getting-started) says.

Unpack the Wish Engine archive anywhere and run `wish-engine.exe` on Windows (for a shortcut, the
icon is `wish-engine.ico` in the folder), or `wish-engine` on Linux. The compiler for game code
ships inside the folder, so no developer tool needs to be installed. Two things the computer itself
must have:
- **Windows:** building game code currently needs the Visual C++ runtime (`VCRUNTIME140.dll`),
  which most Windows computers have. The engine, the editor and exported games do not need it.
- **Linux:** glibc 2.35 or newer (Ubuntu 22.04, Debian 12), the ALSA library (`libasound2`), a
  Vulkan driver or OpenGL, and the X11 or Wayland libraries a desktop has.

Drop a project folder on `wish-engine.exe` to open the editor on it. On the command line the same is
`wen edit`.

Without a project, the editor opens on the **welcome screen**:

1. **Who helps you?**
   - **Claude Code**, if it is installed: the editor's Agent panel will use it.
   - **Nobody**: you make the game by hand.

   Any other MCP agent can connect later (*AI* menu, *Connect an outside agent...*).
2. **What kind of game?** **3D** (models, lights and a camera in space) or **2D** (sprites on a
   flat screen). The kind only sets where you start: a 2D project begins with an orthographic
   camera and no ground, and the *Create* menu lists sprites first; a 2D project can still hold 3D,
   and the other way round. The kind is written in `wish.json` (`"kind": "2d"`); a project without
   it is a 3D project.
3. **Start from** the templates of that kind:
   - 3D: an empty project (a camera, a sun, a ground and starter game code), or a template:
     *Islands* (a 3D platformer, the showcase), *Hop* (3D jumping), *Collect* (3D seen from
     above), *Quiz* (interface only);
   - 2D: an empty project (an orthographic camera and starter game code), or *Bricks*.
4. **Name and place.** New projects go into `Documents/Wish Projects` by default.

Press **Create**. The engine makes the project, builds its game code and opens the editor in the
same window. On the right, **Open a game** lists the projects you opened lately, or takes the path
of any project folder.

The language is chosen at the top right of the welcome screen, and later in the *View* menu
(*Language*). There are 12 languages.

## 2. The editor

| Place | What it shows |
|---|---|
| Menu bar | *File*, *Edit*, *Create*, *View*, *Game*, *AI*, *Help*; the project and the scene; *Search everything* (Ctrl+K, the command palette) |
| Toolbar | *Undo* / *Redo*, the scene, *Play* / *Pause* / *Step* / *Stop*, *Record*, *Manual* / *With AI*, *Agent* |
| Left: *Hierarchy* | The entities of the scene, as a tree. *Filter entities* narrows it; a row with children shows how many. |
| Middle | **Scene** view: the scene through the editor's own camera. Click to select; drag the gizmo to move, rotate or scale. In a wide window the **Game** view is beside it: the game as the player sees it, through the scene's camera and with the game's interface (*Game view* at the view's top right shows it; the game view's own corner closes it). Drag the line between the two views to share the middle (a double click centres it); the corner button, *View > Maximize the view* or **Shift+Space** over a view makes it fill the editor, and the same again brings the panels back. While the game runs, the scene view keeps the editor's camera, so you can look around the running game. The gizmo's tools (*Move*, *Rotate*, *Scale*, *World* / *Local*, *Snap*, *Reset camera*) float at the view's top left. |
| Right: *Inspector* | The selected entity's components and their fields. Each field has its unit and range. |
| Bottom tabs | *Assets*, *Console*, *Problems*, *Performance*, *History*, *Activity* |
| Status line | What the last action did, the number of errors and warnings, and how many entities are selected |

**Moving around the view:**
- Hold the right mouse button and move the mouse to look around.
- While it is held, **W A S D** move, **Q / E** go down / up, and **Shift** is faster.
- **Alt** and the left button turn around the point ahead (orbit); the middle button slides; the
  wheel zooms; **F** frames the selection (*View → Frame the selection*).
- *Reset camera* returns to the scene's camera.
- **The 2D view:** a 2D project's scene view looks straight down on the flat world (the *2D*
  button among the view's tools, or *View > Switch the scene view between 2D and 3D*, switches).
  There the wheel zooms toward the pointer, the middle button (or **Space** or **Alt** with the
  left button) drags the world, and there is no orbit. Click a sprite to select it (a clear part
  of its picture is not the sprite); drag from an empty place to select everything a rectangle
  touches. The gizmo moves in X and Y (its centre square moves freely in the plane) and turns
  about Z; with *Snap* a move lands on the grid: the game's pixels when the camera has a
  `PixelCamera`, else the first tilemap's cells. *Create* puts things in the middle of the view.

**Shortcuts:**

| Keys | Action |
|---|---|
| W / E / R | Move / rotate / scale |
| Ctrl+Z | Undo |
| Ctrl+Y or Ctrl+Shift+Z | Redo |
| Ctrl+P | Play / stop |
| Ctrl+D | Duplicate the selection (with its children) |
| Ctrl+N | New scene (a camera, the ground and the sun) |
| Ctrl+Shift+N | Create an empty entity (the *Create* menu has the rest) |
| F | Frame the selection |
| Ctrl+K | Command palette |
| F2 | Rename the selected entity |
| Delete | Delete the selected entities |
| Escape | Cancel a drag |

**Views** (the box at the view's top right, or *View → Shading*):
- *Shaded*: the picture.
- *Entity colours*: each entity in its own colour.
- *Depth*, *Normals*, *Wireframe*.
- *Colliders*: physics shapes.
- *Lights*: light ranges.
- *Shadow cascades*: which shadow map each surface uses (grey: no shadow).

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
- **The look of a scene:**
  - A new scene has the engine's default look, *Natural*: the sky lights the scene, a small sun
    disc, a faint haze far away, a soft glow on bright light, gentle colour grading, smoothed
    edges and sharp shadows near the camera.
  - *Scene resources*, *Add resource...*, *Apply a look...* sets a whole look in one step:
    *Plain* (no effects at all), *Natural* or *Vivid* (Natural, stronger). It writes the values
    into the scene, and one undo takes it back.
  - Each part can be changed by hand afterwards: add `Fog`, `Bloom`, `ColorGrading`, `Sky`,
    `Ambient` or `RenderSettings` under *Scene resources* and edit its fields.
  - The in-game interface, sprites and particles are drawn over the finished picture: the look
    does not change them. A camera set to *Orthographic* (2D) never shows the sun's disc.
  - A scene made with an older version of the engine keeps the look it had (see section 10).
- **Assets** (bottom tab):
  - Models, images and sounds of the project, filtered by kind.
  - Drag one into the view to place it.
  - *New...* makes a placeholder sound or picture, a *Material file*, a *Sprite sheet* or a
    *Tileset* of the selected picture, or a *Tilemap* of the selected (or first) tileset.
  - *Rename / move...* moves a file and updates every reference to it.
  - *Open outside* opens it in the program your system uses.
- **Looks (materials and textures):**
  - A shape's look is its `Material` component in the inspector: colour, metal, roughness and
    glow, for a plain look.
  - A picture on a surface comes from a *Material file*. Make one (*Assets*, *New...*, *Material
    file*), select it, and set its fields where the browser shows the file: `base_color_texture`
    takes a PNG of the project (type its path, pick it from the list beside the box, or drag the
    file onto it), `uv_scale` repeats the picture, `alpha_mode` *Mask* cuts holes where the
    picture is transparent, *Blend* makes the surface see-through by its alpha (glass, water),
    `double_sided` shows a surface's back.
  - A plain `Material` is see-through when the alpha of its `base_color` (the last box, `w`, or
    the colour picker) is below 1: 0.5 is half, 0 is invisible. See-through surfaces cast no
    shadow.
  - Then name the file in the `file` field of an entity's `Material`. Every entity that names it
    shares the look and changes with it; the colour and numbers of such an entity's own
    `Material` are then not used.
  - On a model, a material file replaces all of the model's own materials. Without `file`, a
    model keeps its own.
  - Naming a file on an entity is in the history and can be undone. A change to the material
    file itself is recorded with who made it (*Activity*), but has no undo of its own yet: save
    a checkpoint before a large change.
  - Textures are PNG files of at most 4096 pixels a side. A missing or broken one is reported and
    the surface is drawn without it.
- **Sprites and sprite sheets (2D):**
  - A `Sprite` shows a picture flat in the scene. Its `color` tints it (a white character in many
    colours); `size` draws it at another size, and with `slice` borders it is a nine-slice: the
    corners keep their size and the rest stretches (panels and frames).
  - A walk cycle comes from a *Sprite sheet*: select a picture, then *Assets*, *New...*, *Sprite
    sheet*. Where the browser shows the sheet, set how the picture is cut (`grid`: a frame's
    `frame` size, `columns`, `rows`; or *Switch to rectangles*), type a name and *Add clip*, and
    give each clip its `frames` (`[0, 1, 2, 3]`, numbered left to right, then down), `fps` and
    `loop`.
  - Then add a `SpriteAnimator` to the entity with the `Sprite`, and type the sheet in `sheet`
    and a clip in `clip`. While the game runs the clip plays; game code switches clips with
    `animator.play(text("walk"))`.
- **Tilemaps (2D rooms, walls, floors):**
  - Select a picture of tiles, then *Assets*, *New...*, *Tileset* (16-pixel tiles), and *New...*,
    *Tilemap* (a map of it with a `floor` and a `walls` layer).
  - Add a `Tilemap` to an entity and type the map in `map` (`cell_size` is a cell's size in
    meters). With the entity selected, the inspector shows the *Tile palette* under the component:
    choose the layer (*Paint on layer*), a tool (*Brush*, *Rectangle*, *Fill*, *Erase*, *Picker*)
    and a tile, then press and drag in the view. Each stroke is one change in the map's own
    history: Ctrl+Z while painting takes the last one back. *Solid tile* makes the chosen tile a
    wall: solid tiles stop characters (in a layer whose `collide` is on).
  - The map is drawn under the sprites of its `layer`; `y_sort` draws those sprites lower over
    higher, for games seen from above.
- **Drawing order and pixel art (2D):**
  - A higher `layer` is drawn over a lower one, and within a layer a higher `order`. An entity
    with a `SortingLayer` gives a layer number a name, and with `y_sort` sorts its sprites lower
    over higher.
  - A `PixelCamera` beside the `Camera` draws a game made for a fixed number of pixels
    (`resolution`, `pixels_per_unit`) scaled up by a whole number, with bars around it
    (`letterbox`), and keeps sprites on the game's pixels (`snap`), so pixel art stays sharp.
    Game code reads where the pointer is in the world in `Pointer.world`.
- **Prefabs:**
  - *Make prefab* turns the selection into a reusable piece.
  - Instances show *Instance of {prefab}*; their changed fields are marked.
  - *Revert to prefab* undoes an instance's changes.
  - Opening a prefab from *Assets* edits it like a scene.
- **Notes:** each entity can carry free-form notes (the *Notes* box in the inspector). An agent
  reads them too.
- **Project settings** (*File → Settings...*):
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
- **Checkpoints** (*Edit → Checkpoints*):
  - *Save checkpoint* keeps the project's scenes, prefabs, material files, meta files and
    settings as they are (not pictures or sounds).
  - *Go back to this* returns to it; going back is itself a change in the history, so it can be
    undone. Material, meta and settings files it writes over are kept in
    `.wish/trash/checkpoints/`.
  - *Compare with now* shows the checkpoint and now side by side (with today's textures).

Files are the source of truth. Editing a scene file in another program is fine: the editor notices,
the change enters the history as *file*, and a broken file is reported, never overwritten.

## 4. Playing, replays and tests

- **Play** runs the game in the view at 60 ticks a second.
  - Keys drive the actions of the input map.
  - **Pause**, **Step** (one tick) and **Stop**: stopping returns the scene to how it was.
  - Changes you make while playing are live and are not kept.
- **Timeline:** while playing, you can go back to an earlier tick. The game is deterministic: the
  same scene, seed and keys always play out the same way on the same engine version.
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

### Playing over the internet

A host at home sits behind a router that does not let players in from outside. A **relay** carries
their packets instead: a small program you run yourself on a small server with a public address (a
cheap Linux server is plenty). There is no account, no fee and no service of anybody else's.

1. Unpack the Linux archive of the engine on the server.
2. Run `./wen relay`. It listens on UDP port 47620 (`--port` picks another) and prints a line for
   every room and player that comes or goes.
3. Open that UDP port in the server's firewall, for example `sudo ufw allow 47620/udp`.
4. The host plays through the relay at the server's address; the players join with the same address
   and the room code.

The relay carries packets that are sealed between each player and the host and cannot open them. It
never receives the room code itself, only a hash of it, writes nothing to disk and logs no
payloads; it does see which addresses play together and how much they send. Limits: `--max-rooms`
(256), `--max-peers` (32 players a room), `--max-clients` (4096 in all). A host that goes quiet
loses its room after 15 seconds. To keep the relay running after you log out, run it in `tmux` or
as a service.

## 5. Game code

Game logic is Rust in `scripts/src/lib.rs`, built to WebAssembly by the engine.

- **Building:**
  - *Game → Build code* (Ctrl+B), or `wen build`.
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

The switch in the toolbar changes the editor's face: **Manual | With AI**. Same project, same history, same files; only the screen changes. Your choice is remembered.

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

**The Agent panel** (*Agent* in the toolbar, or *AI → Show or hide the agent panel*):
1. Write what you want in *What should the agent do?* and press *Send*.
2. *Point in the view*, then click entities, to show the agent what you mean.
3. The transcript shows what the agent says and every tool it calls.

A **checkpoint is saved before every task**, so you can always go back.

**Permission modes** (*AI → Permissions*):

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
- The agent uses the same tools the editor uses, 34 of them. Every one of them can also be done by
  hand.

The engine is complete without any AI. An assistant is a faster way to do the same things.

## 7. Packages, themes, workspaces and the command palette

**Packages** bring themes, workspaces, languages, recipes and templates. A package is a folder with a `wish-package.json`.
- A package names the engine versions it works with in `engine`. While the engine's version starts with 0, write a lower bound such as `">=0.2"`: a bare `"0.2"` means 0.2.x only, and every newer 0.x engine refuses the package (`package_engine_mismatch`).
- *File → Packages...* lists what is installed and installs a package from its folder: *Install into the project* (it travels with the project: `packages/` and `wish.lock` record exactly which files), or *Install for me* (for every project you open on this computer).
- *Remove* moves a package to the trash; nothing is deleted.
- The project's lock is checked: if a package's files change, the window says so; *Write the lock again* if the change is yours.
- **The package index:** under *From the package index*, name the index folder (a folder of packages; it is remembered), type a few words and press *Search*. Each package shows its newest version that works with this engine: *Add to the project*, *Add for me*, or *Update* when a newer compatible version is there. The Hub's *Packages* tab has the same search for your own packages.
- On the command line: `wen pkg search dark theme --index <folder>`, `wen pkg add <id>`, `wen pkg update`, `wen pkg remove <id>`, `wen pkg lock` (`--user` for your own packages; `WISH_PACKAGE_INDEX` saves typing `--index`).

**Engine versions:** several versions of the engine can live side by side. The Hub's *Engine versions* tab lists them; *Add an engine* remembers another engine's folder where it is (the folder with `wen` and `wish-engine.json`; nothing is copied), *Remove* forgets one. A project opens with the version it was made with (its `wish.json` says which); if that one is not on the computer, with the newest one that opens it unchanged (each engine knows from which version on it opens projects without changing their files); otherwise this engine opens it and says why: a newer project needs its own version, and an older one made before a file format changed will have its files changed when they are saved. The recent projects list shows when a project was made with another version. On the command line: `wen engines`, `wen engines add <folder>`, `wen engines pick <project>`.

**Themes:** *View → Theme* switches the editor's look at once: Atelier (the default), Midnight, High contrast, and any theme a package brings.

**Workspaces:** in the AI mode, the *Workspace* box above the game card picks what the card is for. *Story game* asks for the setting, characters, tone, choices and endings, and brings its own ready requests; *Default* is the engine's card for any game. Nothing you wrote is lost when you switch.

**Command palette:** Ctrl+K opens it. Type a few letters of what you want (*export*, *ai mode*, *tests*), choose with the arrow keys, Enter runs it. Shortcuts: Ctrl+P play/stop, Ctrl+Z/Ctrl+Y undo/redo, Ctrl+M manual/AI mode, Ctrl+B build code. To change one, add it to your preferences file: `"keys": { "mode.switch": "Ctrl+J" }`.

## 8. Exporting a game

*File → Export game* (or `wen export`):
- The project is checked first.
- The game code must be built.
- The result is `exports/<name>/`:
  - `<name>.exe` on Windows, `<name>` on Linux: the player;
  - `game/`: scenes, assets and the built code;
  - `LICENSES.txt`.

To share the game, zip that folder. The player is in it, so it needs neither the engine, the editor
nor a compiler. It plays exactly as it did in the editor: after the same ticks, the same world. The
game is exported for the system it is made on (Windows or Linux); exporting for the other one from
the same computer comes in a later release.

## 9. The command line

`wen` does everything the editor does. A few useful commands:

```bash
wen new my_game --template islands
```

A 2D project: `wen new my_game --kind 2d` (an empty one) or `wen new my_game --template bricks`.
Without `--kind`, a template's own kind is used, and an empty project is 3D.

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
| *This file is format 1; this engine writes format 2* | Nothing is wrong: see below |

**Projects from an older version.** Scene and prefab files carry a format number. This version
writes format 2; the default look of a scene changed with it. A file of format 1 opens as it is and
looks and plays as it did: the engine keeps the old values for it. *Problems* (and `wen check`)
lists such files with a warning. The file is rewritten as format 2, with those values written into
it, the first time you change the scene in the editor, or all at once with:

```bash
wen migrate --root my_game
```

To give such a scene the new look afterwards, use *Apply a look...* and pick *Natural*. Replays
recorded before the files were rewritten no longer match and have to be recorded again.

## 11. What is in a project folder

```text
wish.json             the project: name, kind (2D or 3D), default scene
scenes/               scenes (*.scene.json) and prefabs (*.prefab.json)
assets/               models (.glb/.gltf), images (.png), sounds (.wav/.ogg),
                      material files (*.mat.json), sprite sheets (*.sheet.json),
                      tilesets (*.tileset.json), tilemaps (*.tilemap.json)
scripts/src/lib.rs    game code
input.json            actions and their keys
tests/                scenario tests (*.test.json) and golden pictures
replays/              recorded replays
exports/              exported games (not part of the project)
.wish/                what the engine makes (builds, checkpoints); not shared
.mcp.json, CLAUDE.md  how an AI agent connects and works here
```

Every file is canonical JSON, so git shows clear differences between versions.

## 12. How the claims in this guide are checked

Each of these commands runs from a checkout of the engine's source (the `cargo test` ones also run
in CI on Windows and Linux).

| Claim | Command |
|---|---|
| 12 languages | `wen edit --lang xx` names them in its error |
| 34 tools, each with a way to do it by hand | `wen call` with an unknown tool name lists them; `cargo test -p wish_editor --test parity` |
| The same scene, seed and keys play out the same way; replays of 10,000 ticks play back exactly | `cargo test -p wish_control --test script_tools` |
| An exported game plays as it did in the editor | `cargo test -p wish_player --test export` (the same world hash, in a separate process) |
| A file of format 1 looks and plays as it did; `wen migrate` keeps its meaning | `cargo test -p wish_control --test look_lock` |
| What a release archive needs | the release workflow builds each archive (the Linux one on Ubuntu 22.04, failing if a program asks for a newer glibc than 2.35) and uses it from the extracted copy, without the runner's Rust tools, on a full GitHub runner image: that proves it needs no developer tools, not that a bare system has every library it loads ([.github/workflows/release.yml](../../../.github/workflows/release.yml)); the Visual C++ runtime note: [docs/design/M12-release-pipeline.md](../../design/M12-release-pipeline.md), section 6 |
