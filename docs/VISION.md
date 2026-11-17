# Wish Engine — Vision

> **Wish Engine** — "dile, olsun" (wish it, and it is). The command-line tool is `wen` (**W**ish **En**gine), and the crate prefix is `wish_`.

## In one sentence

A 3D game engine that is **fully integrated with AI**; whose interface is built around steering AI as effectively as possible, yet is complete to use without AI (**hybrid**); where MCP is the backbone, not an add-on; and that is deterministic, text-based, modern and **completely free**. Claude comes first; other AIs that speak MCP use the same surface.

## Product vision

- **Open source.** MIT + Apache-2.0. Anyone can read the code and contribute. Public code also helps AI models get to know the engine better over time.
- **Free, with no commission.** The engine is free. No revenue share, license fee or subscription is taken from games made with the engine.
- **Distribution.** Steam and our own website.
- **The AI belongs to the user.** The user connects their own Claude (or another provider) account. The engine does not step in between and takes no share of AI usage.
- **Hybrid.** You can work with AI, or fully by hand like in today's engines; the two can be mixed in the same scene. Connecting an AI is optional.
- **Zero setup.** Download → open → connect your AI (or start by hand without it) → start making a game. The user is not asked to install a compiler or any extra tool.
- **End to end.** Every step from the idea to a distributable game file (export) is inside the engine.
- **"Seamless" is a quality bar.** It is measured with the criteria at the end of this document; a release does not ship until these criteria are met.

## The problem we solve

In today's engines (Unity, Unreal, Godot), MCP integrations are added afterwards on top of an editor that was designed without an agent in mind. The problems the agent has all come from the same roots:

| Problem | Root cause |
|---|---|
| Uncertainty: "did my change happen?" | State is hidden inside the editor and cannot be queried |
| The same bug cannot be reproduced twice | The simulation is not deterministic |
| Guessing field names, wrong API calls | The engine does not publish its own schema in a machine-readable form |
| Scene files cannot be edited | Binary formats, or formats full of GUIDs that cannot be diffed |
| Faulty code written by the agent crashes or freezes the editor | Game code runs in the same process as the engine, unprotected |
| Every attempt takes minutes | The compile → restart loop |
| "Done" is claimed but it does not work | Verification tools (tests, visual comparison) are not open to the agent |

Wish Engine solves these root causes as **core design invariants**, not with patches afterwards.

## Hybrid: with AI or by hand

The engine is complete in both modes, and the user switches between them at any moment:

- **With AI:** the human states their intent, shows, reviews the result, approves or undoes; the agent (Claude and other AIs that speak MCP) does the work.
- **By hand:** like in today's engines — grab the entity, move it, change its value, play. Everything works even with no AI connected.

The two are not rivals but complements: small and visual adjustments are faster by hand, big and repetitive jobs are faster with AI. Changes made by hand go into the same history too; the agent sees them and continues where it left off.

**The AI side.** The interface is **not** a chat window added to a classic editor; the elements for steering AI are part of the design from the start (the **cockpit**):

| Cockpit element | What it provides |
|---|---|
| View + conversation side by side | The scene is visible while a request is written; replies can be watched live in the scene |
| "Show and tell" | Clicking an entity, a region or a time range in the scene adds it to the request as context ("make **this one** a bit bigger") |
| Activity feed | Every change by the agent is a card: what changed (diff), why, and the result. One-click undo per card |
| Automatic checkpoints | A snapshot before every agent task; "go back to the state before the AI started this" |
| Before/after comparison | The visual difference of the scene and the game; accept / reject the change |
| Permission modes | "Only suggest" · "Apply, I will review" · "Fully autonomous" |
| Live observation | The trials the agent runs, the images it takes and the tests can be watched |

**The manual side.** The classic editor tools are complete and first-class: viewport, hierarchy, inspector (generated automatically for every component), move/rotate/scale handles, asset browser, material and prefab editing, play/pause/step/rewind controls, console and performance panels. Game code can also be written by hand with an external code editor.

Both the human and the agent work through the **same control plane**. Everything the agent can do can be done by hand, and everything that can be done by hand can be done from MCP and the command line. The two can edit the same scene at the same time.

## Design principles (invariants)

Each principle is written so that it can be tested. If a feature breaks one of them, the design is wrong.

1. **Schema first.** Every data type in the world (component, resource, event) has a schema that can be queried at run time. Data without a schema cannot enter the world. The agent never guesses a field name.
2. **Everything is text.** Scene, prefab, material, input map, test — all are canonically formatted, diffable text files. Files are the single source of truth.
3. **Deterministic simulation.** Same build + same scene + same seed + same inputs = a bit-for-bit identical world. Every tick has a world digest (hash).
4. **Observability.** Every part of the world can be queried: entities, components, events, logs, performance, what is shown on screen (not only as pixels, but semantically too).
5. **Reversibility.** Every edit is an atomic transaction and can be undone. Every simulation moment can be saved and restored.
6. **A loop that does not crash.** Game code written by the agent runs in a sandbox (Wasm). Faulty code does not crash or freeze the engine; it produces a structured diagnostic message.
7. **A fast loop.** The time from change to observation is measured in seconds. The engine is not recompiled when game logic changes.
8. **One control plane.** MCP, the command line and the editor are three faces of the same internal API. None of them has a special capability that the others lack.
9. **Finish with proof.** The engine gives the agent the tools to prove a "it works" claim (scenario tests, visual comparison, hash verification).
10. **Measure, do not assume.** The claim that the engine is agent-friendly is measured continuously by an automatic agent evaluation suite (evals).
11. **Hybrid parity.** Every scene/entity operation the agent can do can also be done by hand. The engine works completely when no AI is connected. Changes made by hand and by the agent are kept in a single history.

## Features that set it apart

- **A built-in MCP server inside the engine** — in every project created with `wen new`, Claude Code connects automatically.
- **Time travel** — take a snapshot, rewind, fork the world ("what if I tried this?"), replay.
- **Semantic vision** — next to the screenshot, the information "which entity is where on the screen, and what percentage of it is visible".
- **World inspector (linter)** — heuristic warnings such as "an entity fell out of the world", "there is no light in the scene", "the camera sees nothing".
- **Input injection** — the agent can really play the game; inputs can be recorded and replayed.
- **Lossless hot reload** — game state lives in the world, not in the Wasm module; when code changes, the game continues where it was.
- **An asset pipeline open to external production tools** — Blender MCP or generative tools drop files into the `assets/` folder; the engine imports, validates and reports them.

## Scope: the 1.0 goal

"The best engine for making small and medium-sized 3D games from start to finish, seamlessly, with AI, by hand, or with both together."

- 3D first (2D as a subset, with an orthographic camera + sprites)
- Windows desktop (Linux/macOS from the same code); web is a later milestone
- PBR rendering, shadows, glTF import, skeletal animation
- Physics, audio, input, in-game UI
- Wasm-based game logic (first language: Rust)
- MCP + command line + hybrid editor (AI cockpit and full manual editing)
- Exporting the game to a distributable package
- Single-package install (Steam and website), Turkish and English interface

## Out of scope (deliberately, for 1.0)

- Unreal-level AAA rendering (virtual geometry, ray-traced global illumination) → a research line after 1.0. The architecture leaves the door open for it, but does not promise it.
- Multiplayer/networking → after 1.0. The determinism and snapshot infrastructure prepares the ground for it.
- Console and mobile platforms.
- Visual (node-based) scripting.
- Expert manual editing tools: animation timeline editor, visual shader graph, terrain sculpting, a full code editor inside the engine → after 1.0. Until then, an external tool (Blender, VS Code), a text file or AI is used.
- Generative AI models embedded inside the engine (we integrate with external tools).

These limits are for honesty: the reason "next-generation engine" projects fail is almost always scope. First narrow but flawless, then wide.

## Success criteria

| Criterion | Target |
|---|---|
| Scene change → observation | < 2 seconds |
| Game code change → running game | < 10 seconds |
| Determinism | Zero hash drift over a 10,000-tick replay |
| Agent evaluation suite | ≥ 90% task success with Sonnet 5.5 · medium effort |
| Playable mini game from scratch | A single agent session, no human intervention |
| Use without AI | With no AI connected, building a scene, playing, testing and exporting all work completely |
| Manual/agent parity | Every scene/entity operation in the control plane has a manual counterpart in the interface (checked automatically) |
| First run | After download: connect the AI → first request → a working scene in < 5 minutes |
| Install | A single package; the user does not install a compiler or any extra tool |
| Agent-caused crash/freeze | Zero (faulty game code only produces a diagnostic) |
| Reversibility | Every change the agent makes can be undone from the interface with one click |
| Export | A standalone game package that works with a single command/click |
