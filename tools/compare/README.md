# Engine comparison (M7)

The same eight scene tasks (`tasks.py`), in the same words, for an agent
driving Wish Engine and one driving Unity, each through its own MCP server.
Measured: passed, turns, tool calls, tool errors, tokens, cost.

- `python tools/compare/run.py selftest` — free; no model, no download.
- `python tools/compare/run.py run --engine wish --model sonnet --effort medium` — **spends Claude usage; ask the user first.**
- `python tools/compare/run.py run --engine unity --unity-project <dir> ...` — the same, against Unity.

## Preparing Unity (once, at measurement time)

Disk: a Unity project with its `Library` folder takes roughly 2–3 GB. Use one
throwaway project and delete it after the run. Unreal is not part of the
comparison (user decision, 16 November 2026: not enough disk for a 50+ GB
install).

1. Create an empty 3D project (Unity Hub, or
   `Unity.exe -batchmode -createProject C:\wish-build\compare\unity -quit`)
   with the editor that has the Unity MCP (AI Assistant) package; the relay is
   `~/.unity/relay/relay_win.exe --mcp`.
2. Copy `unity/CompareSetup.cs` to `Assets/Editor/` and run
   `Unity.exe -batchmode -projectPath <project> -executeMethod CompareSetup.Build -quit`:
   it writes `Assets/Scenes/CompareBase.unity` (the start scene, Z mirrored,
   text serialization).
3. `python tools/compare/run.py prepare-unity --unity-project <project>`
   while the editor is **closed**: each task gets its own copy of the start
   scene, which the editor imports when it opens (a scene copied in while
   the editor runs in the background is not imported, and the agent cannot
   open it).
4. Close every other Unity project (the relay talks to the open editor),
   and open this project in the editor.
5. Run the comparison. Before the first task the runner checks that every
   task *fails* on the real start scene and that every task scene is an
   untouched copy (proof that the scene reader reads
   Unity's files); each task gets its own copy of the start scene.

## How the checks differ

Wish Engine checks ask the engine (`describe_view` for what the camera sees,
a 150-tick simulation for the falling box). Unity checks read the saved scene:
the camera check is geometric (inside the view cone), and the falling box is
a static check (a Rigidbody with gravity, colliders on the box and the
floor). Both are documented in each result row's `why`.
