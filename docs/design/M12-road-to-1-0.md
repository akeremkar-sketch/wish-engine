# M12 (proposal): the road to a strong 1.0

**Status:** proposal, 18 November 2026; waits for the user's decision on the
scope (section 7). **This grows the 1.0 scope.**

User request: "Considering these evaluations, how can we make the engine
better: a general plan, and a beautiful and strong entry into 1.0." Highest
priority: graphics quality and platform support. Also behind: ecosystem,
maturity and docs, editor tools, 2D. Good but improvable: code iteration and
lightness (a small base engine, everything else as extras; anti-cheat was the
example). Keep the lead in working with AI; add a quality mode.

## 1. How this plan was made

Two workflows of agents, read-only. First, eight agents each mapped one area
from the code (what exists, gaps with sizes and risks) and two researched the
web (what engines offer in 2026, and how engines and tools launched well):
[M12-area-maps.md](M12-area-maps.md) keeps their full output and is the
backlog every package starts from. Then three agents each wrote a complete
plan from a different lens (first impression, differentiation, risk and
capacity) and three critics judged them (feasibility, the user's priorities,
architecture). All three critics ranked the risk-and-capacity plan first
(7.5 of 10); this note is that plan with what the critics asked to graft and
reorder. Scores and sizes below are judgements, not measurements.

## 2. Is it the API, or have we not built enough?

Not the API. `wgpu` reaches Vulkan, DX12 and Metal (Bevy draws far more on
it), and `winit`, `wgpu` and Rapier run on Linux, macOS and the web. What is
missing was not built: one shadow map for the whole scene, a flat ambient
colour, no post-processing, no anti-aliasing, no transparency for meshes, a
picture that is read back to the CPU every frame; only Windows is packaged.
Ecosystem and maturity are different: code can make them easy to grow, but
users and time make them.

## 3. What 1.0 is

The first engine in which one person, an agent, or both together can make a
small or medium desktop game, 3D or 2D, **and prove that it works**: for
Windows and Linux developers (macOS as a labelled preview) who export to
Windows, Linux and Steam Deck from one machine, with file formats, the tool
surface and the SDK source API frozen under a written contract. Not yet for
web, mobile or consoles, and the first page says so.

What limits the work is not writing code (M1 to M10b took four days) but the
owner's attention (decisions, pictures reviewed by eye, sound by ear) and what
only strangers and calendar time can give (a beta). The plan is about three
times the size of everything built so far, in the roadmap's units.

## 4. Phases

Every phase ends with a quiet numbered pre-release (0.2.0, 0.3.0, ...; versions
are three numbers) and an independent closing review with a fix budget.
At most two code lanes at once, split by crate, plus a docs lane.

**Phase 0: ground truth.** Make what the public sees true and turn cheap
unknowns into facts.
- Decisions on paper before code: the VISION scope, ADR-012 against the M11
  list, ADR-023 (a small base and components), and four rules: schema
  defaults are part of the format; no wall-clock numbers in tool answers; the
  version scheme across 0.x; every public claim has a command that reproduces
  it.
- The public face: `main` shows the current README with pictures, the guide,
  community files; a test that every documentation link resolves.
- A release pipeline for **Windows and Linux**: a tag builds base and full
  archives with checksums and every licence text; a separate `dist` profile.
- Probes: committed replays run on Windows and Linux in CI; macOS arm64 and
  wasm32 determinism as informational jobs; a window smoke test.
- Code freshness (sim, test and replay never run stale game code), the lean
  play path, and the verification gate (`check` at level full) that later
  packages exit through.
- Measure first: render statistics, `wen bench --render`, fuel per system.
- AI efficiency round 2 (small scenes answer values by default, start-run-report
  in one call, `keep_world`, `look_at`), measured with three tries.

**Phase 1: foundations, built once.** Each structure several areas share, and
the single hash-and-format break.
- Render pass structure, pooled targets, one `RenderSettings`; the MSAA
  measurement on two adapters right after.
- The presenter: swapchain, vsync, interpolation between ticks, fullscreen,
  HiDPI, automatic software fallback; a forced device-loss test.
- Components: the compiler, SDK and export players become optional
  components from a local archive first; `--base` (about 70 MiB) and `--full`
  packages; templates ship with built modules, so a new project plays without
  the compiler.
- The format window: gamepad (with focus navigation for the interface),
  player-indexed actions, replay format 2 (build id, strict, by path),
  animation cross-fade, save games, scene switching at run time.
- Editor first hour: orbit, pan, zoom, frame; a Create menu; duplicate; new
  scene; the menu split and HiDPI.
- First-run robustness (a `doctor` action), then the first outside hands: one
  or two informal first runs.

**Phase 2: the look and the desktop** (the user's two first priorities, two
lanes).
- Render lane, must tier: sky-lit ambient with a sun disc, fog, colour
  grading and FXAA; HDR bloom and particles that glow; cascaded shadows with
  masked casters; material files and textures on primitives; an alpha-blended
  pass; environment maps. Then **the look lock**: presets, Low/Medium/High
  tiers, a render scale, and the one switch of the default look, signed off
  by the user by eye. After it, as non-default High-tier settings: culling and
  instancing, ambient occlusion, spot-light shadows, GPU skinning, MSAA if the
  measurement said yes.
- Platform lane: Linux as a first-class host (package tested end to end in
  CI); cross-export between Windows and Linux through player components;
  Steam Deck checked in the Steam Linux Runtime ("playable" is said only after
  a report from a real device); macOS arm64 as an unsigned preview that never
  gates the release.
- The eleven scores are re-scored here, where the user cares most.

**Phase 3: make games with it.**
- 2D kit: sprites kept on the GPU, sheets and sprite animation, nine-slice, a
  2D physics profile, fixed viewport with a pixel-perfect camera and the
  pointer in the world, tilemaps with painting.
- Editor musts: overlays and picking, gizmo completion, hierarchy at scale,
  asset import and management, console with click-through to file and line,
  the play loop (background build, build on save, sound, profiler, proposal
  preview); inspector depth and a material editor as stretch.

**Phase 4: prove, teach, package, freeze.**
- AI: the game-code loop; tool profiles with a core of at most 25 tools and a
  read-only role; a hard evaluation tier; the quality mode (Fast, Balanced,
  Thorough: plan, build, prove with several checks including views from more
  than one camera, repair; the reviewer stage experimental).
- Determinism as a product: replays and a crash ring in the shipped player;
  **verified runs** as the first optional package (a server re-simulates a
  submitted replay and confirms its result; it is not a kernel anti-cheat).
- Ecosystem: packages carry assets and prefabs, dependencies, author tooling;
  a seeded index with genre templates and starter packs; the showcase games
  re-dressed with the new look; a feature gallery.
- Docs: a reference site generated from the schema with an offline copy and
  in-editor help; tutorials executed in CI (English, and the first lesson in
  Turkish); the stability contract with a compatibility corpus; a launch kit.

**Phase 5: beta, release candidates, 1.0.** No feature merges. A freeze audit;
a beta of at least four weeks with an exposure floor (ten or more outside
first runs on named platforms, three weekly builds); the formal three-person
test with people who have never seen the engine; release candidates; 1.0.0 and
a planned 1.0.1.

## 5. After 1.0, and the cut order

After 1.0: **web export** as the headline of 1.1 (with the lean runtime and a
second executor it needs); macOS as a signed first-class target; code in
packages (kits) and editor plugins (`wish_ext`); how native SDKs attach
(Steamworks, a kernel anti-cheat) as engine-built player variants; global
illumination, reflection probes, TAA; docking and specialist editors; 2D
lights and autotiling; Android and iOS; a scripting language that needs no
compiler; rollback netcode; a public multi-engine benchmark.

If time runs short, cut in this order: first what the user did not ask for
(benchmark runs, extra agent adapters, time-travel depth, Hub missions), then
second-tier depth (inspector depth, half the templates), and last anything in
graphics or platforms. Never cut: the release pipeline, the format window, the
look lock, the stability contract, the beta.

## 6. Where the scores would stand at 1.0

| Area | Now | At 1.0 | Basis |
|---|---|---|---|
| Graphics | 3 | 5.5 to 6 | the must tier; 6 with the High-tier items |
| Platforms | 1 | 5 | Windows, Linux, Deck, cross-export, macOS preview |
| Ecosystem | 1 | 3 | first-party content; 4 needs outside authors |
| Maturity and docs | 2 | 4.5 | needs the beta; 4 without it |
| Editor tools | 4 | 6.5 | the musts of Phase 1 and 3 |
| 2D | 4 | 7 | the 2D kit |
| Determinism | 10 | 10 | becomes a product feature |
| Working with AI | 9 | 9.5 | efficiency, gate, quality mode, hard tier |
| Code iteration | 8 | 9 | freshness, background build, instant first Play |
| Lightness | 7 | 8.5 | a base of about 70 MiB |
| License | 10 | 10 | |

With the report's weights: "AI with small games" 64 → about 78 (Godot 72,
Unity 68); "commercial release" 36 → about 60. Parity in the two top areas
costs more: graphics 6 needs GPU skinning, spot and point shadows and MSAA on
top of the must tier; platforms 6 needs a web preview.

## 7. Decisions that are the user's

1. The scope: is this what 1.0 is (it is about three times what was built so
   far), or a smaller cut?
2. The name: it is frozen by the format freeze (file names carry it), so it is
   needed before Phase 4 ends, not at the announcement.
3. Downloading: the index repository and the HTTP/TLS library (still open);
   GitHub Releases as the download address.
4. Merging to `main` now, so the public sees the current state.
5. Where the art of the showcase games and starter packs comes from.
6. A spending cap per measurement campaign, so runs do not need a yes each
   time.
7. Outside people: one or two for an early informal run, three fresh ones for
   the formal test, ten or more for the beta.
8. What "plugins" means at 1.0: the compiler, SDK, export targets, looks,
   asset packs, templates and verified runs are extras; physics, audio and the
   interface stay compiled in; code plugins come after 1.0.
