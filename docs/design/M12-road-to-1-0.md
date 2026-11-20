# M12: the road to a strong 1.0

**Status:** in progress. Approved on 18 November 2026 at full scope, and **cut
the same day to the short path** (user decision: section 0). 1.0 is the short
path. Everything else in this note is done after 1.0; nothing is dropped. The
decisions it needed on paper are in [VISION.md](../VISION.md),
[DECISIONS.md](../DECISIONS.md) (ADR-012, 019, 020, 023, 027) and
[ROADMAP.md](../ROADMAP.md).

## 0. The scope of 1.0: the short path

User decision, 18 November 2026. The full scope below was estimated at 45 to
50 work packages and 2.5 to 3 months, four weeks of beta in it. The user's
time and tokens do not allow that before a first release. So 1.0 is cut to
what the user asked for most, and the rest follows after 1.0 in the same order
of priority, on the way to 2.0.

1.0 contains (about 13 packages; 3 to 4 weeks with a two-week beta; these are
estimates, not measurements):

1. The render foundation: pass structure, pooled targets, `RenderSettings`,
   statistics.
2. A release pipeline for Windows and Linux: a tag builds archives with
   checksums and every licence text; a `dist` profile.
3. Linux as a tested host: the package end to end in CI, audio on Linux.
4. Sky-lit ambient with a sun disc, fog, colour grading, FXAA.
5. HDR bloom.
6. Cascaded shadows with masked casters.
7. Material files and textures on primitives.
8. An alpha-blended pass.
9. The look lock: presets and the one switch of the default look, signed off
   by the user by eye. Every new setting is editable by hand in the editor
   (invariant 9).
10. The AI scorecard and its first baseline (one measurement campaign).
11. The public face made true: the README with pictures, the guides, a link
    test; the code follow-ups of ADR-027; a short stability note (after 1.0 a
    format changes only with a migration or by addition).
12. An independent closing review with a fix budget.
13. A beta of two weeks, release candidates, 1.0.0.

Not in 1.0, all of it after: the presenter, components and the small base,
the format window (gamepad, player-indexed actions, replay format 2, save
games, scene switching, cross-fade), the editor's first hour and its musts,
the 2D kit, cross-export, Steam Deck, macOS, environment maps and the
High-tier render items, code freshness and the verification gate, AI
efficiency round 2, the rules experiment, the quality mode, verified runs, the
ecosystem and documentation packages, the four-week beta.

Two packages are the first candidates to pull back in if the user wants to
make a game by hand at 1.0: the editor's first hour (duplicate, an orbit
camera, a Create menu, new scene: none of these exist today) and scene
switching with save games. The user is asked once more before the 1.0 label
goes on.

**Changed, 20 November 2026 (user request): the editor is redesigned before
1.0**, look and layout, and the editor's first hour comes with it. This grows
the 1.0 scope; it runs before the beta. Design note:
[M12-editor-redesign.md](M12-editor-redesign.md).

Expected scores at 1.0: graphics 3 → 5, platforms 1 → 3, maturity 2 → 3, the
others as today; "AI with small games" 64 → about 67, "commercial release"
36 → about 44. Section 6 is the full scope.

The short path makes three of the user's decisions urgent: the name (2),
downloading (3) and merging to `main` (4) are needed before the release
candidates. The name and downloading are answered (20 November 2026: "Wish Engine"
stays; GitHub Releases).

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

1.0 is the short path of section 0: the first engine in which one person, an
agent, or both together can make a small or medium desktop 3D game **and prove
that it works**, with a default look locked by eye, packaged and tested for
Windows and Linux. It is not yet for cross-export, Steam Deck, macOS, web,
mobile or consoles, 2D is still the subset of M6, and the first page says so.

What this section described on the morning of 18 November 2026 is the full
scope: 3D and 2D, export to Windows, Linux and Steam Deck from one machine,
macOS as a labelled preview, and file formats, the tool surface and the SDK
source API frozen under a written contract. It is still where the engine is
going, and the train of [road-to-2-0.md](road-to-2-0.md) reaches it around
release 1.7 (section 6).

What limits the work is not writing code (M1 to M10b took four days) but the
owner's attention (decisions, pictures reviewed by eye, sound by ear), the
owner's tokens, and what only strangers and calendar time can give (a beta).
The full scope is about three times the size of everything built so far, in
the roadmap's units; that is why it was cut.

## 4. Phases

**The phases are the full scope, not the plan to 1.0.** Section 0 says which
of their items are in 1.0. Everything else is done after 1.0, in the order of
the train ([road-to-2-0.md](road-to-2-0.md) section 3), which regroups these
items into releases 1.1 to 1.10: the phase text below is kept as the
description the train's packages come from, and where it says "1.0", "the
format window of Phase 1" or a 0.x pre-release at the end of a phase, the
train's release is what holds (the format window is release 1.2; the beta of
at least four weeks is the beta of 2.0; 1.0 has a beta of two weeks). At most
two code lanes at once, split by crate, plus a docs lane; every release ends
with an independent closing review with a fix budget.

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
- The rules experiment (user request, 18 November 2026). It answers, before
  the format window, whether game logic written as data should enter 1.0.
  A prototype puts rules in the scene file (a trigger, a condition, actions;
  the expression language grown; run by an engine system, no compile step).
  Then the same models do the same game-logic tasks two ways, as Rust game
  code and as rules, and the AI scorecard (below, after Phase 5) judges.
  Rules enter 1.0 only if they are no worse on correctness and errors for any
  task and better overall; cheaper or faster but less correct loses, and a
  result that breaks the frame budget at the bench scale loses whatever else
  it wins. Tasks that rules cannot express are recorded: that is where Rust
  stays. A yes grows the 1.0 scope and is asked again with the results.
- The AI scorecard itself: built and given its first baseline here, in one
  measurement campaign shared with AI efficiency round 2, before anything is
  improved against it.

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

### The AI scorecard

The engine's main goal is to be used effectively with AI (user, 18 November
2026), and "effectively" is not "fewest tokens". Every AI package from here
on, the rules experiment and the quality mode included, is judged by this
card, in this order:

1. Correctness: passed on the first attempt, and passed in the end; the
   hidden test includes edge cases, and a replay of the result holds.
2. Errors: tool and compile errors, repair turns, and above all results the
   agent believed right that the hidden test refuted.
3. The quality of what was made: a clean `check`, and a second agent asked
   to change the result does it correctly and cheaply.
4. People: a person can read the result and change it by hand in the editor
   (the editor parity test now, people in the user tests). A result that only
   an agent can maintain loses: the engine works fully without AI.
5. Efficiency: tokens and cost per *passed* task.
6. Speed, two kinds. How long the agent takes: wall time from the request to
   a proven result, with the build and repair time in it (the harness
   measures it; tool answers stay free of wall-clock numbers). How fast the
   result runs: tick time at 10,000 entities, measured the way
   `wen bench --script` measures game code.
7. Consistency across three tries.

How it is measured:

- **Held-out tasks.** A second set of tasks is never read or run while the
  engine is being improved, and the score that counts comes from it. Its
  failures are reported as numbers and not studied; a task that is studied
  moves to the open set and a new one replaces it.
- **Two models.** The usual one and a smaller one (today Sonnet 5.5 medium
  and Haiku 5.5). The gap between them is reported: the smaller the gap, the
  more of the work the engine carries.
- **A long task.** From a one-paragraph brief to a finished mini game that a
  hidden scenario test can win. One long task from the first baseline on; the
  hard tier of Phase 4 grows it.

When:

- The card and its first baseline in 1.0 (package 10 of section 0), after
  the render foundation and the release pipeline, before anything is improved
  against it.
- The rules decision in release 1.1, before the format window of 1.2 opens.
  The held-out task set is written in 1.1.
- With every release of the train the card is measured again and published,
  with the command that reproduces it (ADR-027); a release does not ship if
  correctness or errors got worse on the held-out tasks.

Every campaign spends quota and needs the user's approval, or a spending cap
(decision 6 of section 7).

## 5. After 1.0, and the cut order

After 1.0 the engine follows the train in [road-to-2-0.md](road-to-2-0.md):
releases 1.1 to 1.10, then 2.0. Its section 3 says what each release contains,
section 2 the rules, section 5 what is not in 2.0, section 6 the cut order and
what is never cut. This note does not repeat them. In short, for what this
section used to list: everything in the phases above that is not in section 0
comes first (releases 1.1 to 1.7, with GPU skinning and the material editor
in 1.8, MSAA in 1.9 and the four-week beta with its freeze audit at 2.0);
**web export is a preview in release 1.9**, not the headline of 1.1, and
leaves preview in 1.10 only if the determinism matrix passes; code in packages
(kits) is an experiment in 1.10; a signed macOS target, editor plugins
(`wish_ext`), native SDKs as player variants, global illumination and TAA,
docking and specialist editors, 2D lights, Android and iOS, a scripting
language that needs no compiler and rollback netcode are after 2.0.

Five smaller items of the phases above have their release on the train too:
the menu split with the editor's first hour (1.1), proposal preview in the
play loop (1.4), a launch kit with the honest pages (1.6), inspector depth
beside the material editor (1.8) and particles that glow with the particle
package (1.9).

For 1.0 itself the cut already happened (section 0). If the short path runs
short too, graphics and platforms are cut last, and the release pipeline, the
look lock, the closing review and the beta are not cut.

## 6. Where the scores would stand at the full scope

This table is the full scope, which the train reaches around release 1.7 (GPU
skinning and MSAA, in the Graphics row, are 1.8 and 1.9; the material editor
is 1.8; the four-week beta, in the Maturity row, is the beta of 2.0). It is not 1.0: the
expected scores at 1.0 are in section 0 (graphics 5, platforms 3, maturity 3,
the others as today), and the scores at 1.0 and at 2.0 side by side, with
Godot and Unity, are in [road-to-2-0.md](road-to-2-0.md) section 4.

| Area | Now | At the full scope | Basis |
|---|---|---|---|
| Graphics | 3 | 5.5 to 6 | the must tier; 6 with the High-tier items |
| Platforms | 1 | 5 | Windows, Linux, Deck, cross-export, macOS preview |
| Ecosystem | 1 | 3 | first-party content; 4 needs outside authors |
| Maturity and docs | 2 | 4.5 | 4 at release 1.7; 4.5 needs the four-week beta, which is the beta of 2.0 |
| Editor tools | 4 | 6.5 | the musts of Phase 1 and 3 |
| 2D | 4 | 7 | the 2D kit |
| Determinism | 10 | 10 | becomes a product feature |
| Working with AI | 9 | 9.5 | efficiency, gate, quality mode, hard tier |
| Code iteration | 8 | 9 | freshness, background build, instant first Play |
| Lightness | 7 | 8.5 | a base of about 70 MiB |
| License | 10 | 10 | |

With the report's weights: "AI with small games" 64 → about 78 (Godot 72,
Unity 68); "commercial release" 36 → about 60. The train's own estimate for
the same point is about 79 and 60 after 1.7. Parity in the two top areas
costs more: graphics 6 needs GPU skinning, spot and point shadows and MSAA on
top of the must tier; platforms 6 needs a web preview.

## 7. Decisions that are the user's

Decision 1 was answered twice on the same day. Decisions 2, 3 and 4 are open
and **urgent now**: the short path needs them before the release candidates of
1.0. Decision 6 is asked now, and decision 7 starts with the testers of the
1.0 beta. Decisions 5 and 8 are open and are asked with the release that
needs them ([road-to-2-0.md](road-to-2-0.md) section 7). Nothing that depends on an open
one is treated as decided.

1. The scope: is this what 1.0 is (it is about three times what was built so
   far), or a smaller cut?
   **Answered, 18 November 2026: yes, this is what 1.0 is, starting with
   graphics and platforms.**
   **Answered again the same day: cut to the short path (section 0). 1.0 is
   the short path; the rest is done after 1.0, and nothing is dropped.**
2. *(open, urgent)* The name: the first release carries it in file names and
   on the download page, so it is needed before the release candidates of
   1.0, not at the format freeze.
   **Answered, 20 November 2026: the name stays "Wish Engine"**, after a
   second round of candidate research ([../release/name-research.md](../release/name-research.md)).
   The known risk (the WISH marks in software classes) is accepted; a
   trademark attorney's check stays the user's.
3. *(open, urgent)* Downloading: GitHub Releases as the download address of
   1.0. The index repository and the HTTP/TLS library (the download layer
   inside the engine) stay open and have no release number.
   **Answered, 20 November 2026: yes, GitHub Releases**
   (https://github.com/akeremkar-sketch/wish-engine/releases). The release workflow already makes a draft
   release there on a three-number tag; a person publishes it.
4. *(open, urgent)* Merging to `main`, so the public sees the current state
   and the release pipeline can tag it.
   **Done, 21 November 2026 (user decision):** `m9-product` merged into
   `main`, with M8b, M9, M10 and M12 up to the editor redesign.
5. *(open; release 1.6)* Where the art of the showcase games and starter
   packs comes from.
6. *(open; asked now)* A spending cap per measurement campaign, so runs do
   not need a yes each time. The first campaign is the scorecard baseline of
   1.0.
7. *(open; 21 November 2026: the user is finding them; the guide, the
   issue forms and the plan of package 13 are ready:
   [M12-beta-and-release.md](M12-beta-and-release.md))* Outside people: testers for the two-week beta of 1.0; after it,
   one or two for an informal run at 1.3, three fresh ones for the formal
   test at 1.4 and again at 2.0, ten or more for the beta of 2.0.
8. *(open; release 1.3)* What "plugins" means: the compiler, SDK, export
   targets, looks, asset packs, templates and verified runs are extras;
   physics, audio and the interface stay compiled in; code plugins come after
   2.0 (kits as an experiment in 1.10).
