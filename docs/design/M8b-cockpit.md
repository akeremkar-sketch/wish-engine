# M8b — AI cockpit

**Status:** started (17 November 2026), branch `m8b-cockpit`. Recommended model · effort: Opus 5.5 · high (adapter and selective-undo architecture decisions); interface work Sonnet 5.5 · high.

## 1. Goal and boundary

The cockpit is **not** a chat window added into the editor. The parts for managing the AI are part of the design from the start (VISION, ARCHITECTURE "AI cockpit elements"). Each part is the visual face of a mechanism that already exists in the core:
- activity feed and per-card undo,
- checkpoints,
- before/after comparison,
- permission modes,
- show and tell,
- live observation,
- in-cockpit agent.

**Exit criteria (ROADMAP):**
1. A user who has never seen the engine builds a playable mini game in 30 minutes, only by talking (with at least 3 people). **Needs a user test.**
2. Every change the agent makes is visible in the activity feed and can be undone with one click.
3. Hand-over: the person edits by hand, the agent sees it and continues on top; while the agent works, the person edits another entity by hand and no change is lost.
4. Changes made by hand and by the agent appear in the same history, with who made them.
5. When an outside agent (Claude Code) and the in-cockpit agent work at the same time, a conflict does not cause data loss.

**Owned by the user (not done overnight):**
- T1 cockpit mock-ups and user trial;
- the 3-person user test;
- a run with a real model (uses quota).

This note and the first work packages cover what can be done without waiting for these. The agent is tested in the tests with a **fake model** (scripted replies).

## 2. Decisions

### 2.1 Per-card undo = selective undo

A linear undo stack is not enough for per-card undo: undoing a card must not mean undoing everything done after it.

- The inverse of every operation is already in the history (`wish_scene::edit`: each operation computes its own inverse while it runs).
- **Selective undo:** only that operation's inverse is applied, and only if later operations did not touch the same place. "The same place" means prefix overlap of JSON pointers: `/entities/x` overlaps with `/entities/x/components/T`.
- If a later change touches it, the undo is **rejected**. The diagnostic says which revisions touched it and who made them. Hint: "undo those first" or "fix by hand".
- The undo itself is a new operation, enters the history and can be undone. The same card cannot be undone twice.
- An outside file change (`replace_content`) invalidates older inverses: cards before that moment are marked "can no longer be undone".

**Why:** "One-click undo per card" and "no change is lost" (exit criterion 3) hold together only this way. The overlap check stops us from silently breaking the person's later work while undoing the agent's work.

### 2.2 Activity feed

It is fed from two sources:

1. **Document history** (`edit_history list`): each edit is a card.
   - What a card carries: who, what (operation summary, the entities it touched), **why** (new optional `reason` parameter of `edit_apply`), whether it can be undone.
2. **Workspace activity log** (new):
   - Every tool call that changes something (any actor) and the agents' reading calls are recorded: who, tool, short summary, result.
   - Reading calls are, for example, taking a picture, tests, simulation; these are for "live observation".
   - Tool: `activity_get`, with `since` (its counterpart in the parity table: Activity panel).
   - Determinism is not affected: the log is in the tool layer, not in the simulation, and keeps no wall clock (sequence number).

### 2.3 Checkpoints and before/after

- The `checkpoint` tool:
  - **save:** the canonical text of the project's scene and prefab documents, plus `wish.json` and `input.json`, into `.wish/checkpoints/<name>/`.
  - **list.**
  - **restore:** each document is written back as a `replace_content` operation. So it enters the history and can be undone; no change is lost.
- The cockpit takes a checkpoint by itself before every agent task.
- **Before/after:** `capture_frame` + `checkpoint` (a picture from the document in a checkpoint). The editor shows the two pictures side by side.

### 2.4 Permission modes

A policy for agent actors in the workspace (the person always has full authority):
- **Suggest only:** the agent's calls that change something run as `dry_run` and are held as a **proposal**; the agent gets the reply "proposed, waiting for approval". When the person accepts, the real call is applied under the agent's name; when the person rejects, it is dropped.
- **Apply, I review:** today's behavior; cards in the feed.
- **Fully autonomous:** the same application; in addition, the cockpit does not ask for approval between agent tasks.

The policy is a control plane setting (invariant 4). Outside agents are subject to it too.

### 2.5 In-cockpit agent (decision on the temporary part of ADR-024)

- **Adapter interface:** the `wish_agent` crate, independent of the provider. A task is started; the adapter streams events (text, tool call, tool result, done/error).
- **First adapter: Claude Code as a child process** (`claude -p --output-format stream-json`), with the user's own session. No API key; the engine takes no commission (ADR-024).
  - The agent connects to the editor's own workspace; for this the editor opens its own MCP endpoint (Streamable HTTP, random token, local address).
  - So the agent's calls run in the editor's workspace, in the same history and the same feed.
- **Fake adapter:** for tests. It makes scripted tool calls directly on the workspace.
- **Why a child process (not the API directly):**
  - It adds no network/TLS dependency.
  - It uses the user's existing subscription.
  - The tools are already in MCP.
  - Claude Code's own agent loop (context management, retry) comes for free.

  A direct API adapter can be added later with the same interface.

### 2.6 The editor's own MCP endpoint

- The loop of `wish_mcp::http` that runs the workspace on its own thread is split out into an **endpoint** that can be embedded in its owner's loop.
- Every frame the editor runs the pending jobs in its own workspace and answers.
- One workspace = one history, instant visibility. An outside agent can connect to the same endpoint.

## 3. Work packages

| WP | Work | Model · effort |
|---|---|---|
| WP1 | Selective undo (`wish_scene`), `edit_apply reason`, `edit_history revert`, undoability of cards | Opus 5.5 · high |
| WP2 | Activity log + `activity_get`, Activity panel in the editor (cards, "Undo this") | Sonnet 5.5 · high |
| WP3 | Checkpoints + before/after | Sonnet 5.5 · high |
| WP4 | Permission modes (proposals, approval gate) | Opus 5.5 · high |
| WP5 | Editor MCP endpoint; `wish_agent` (interface, fake, Claude Code child process); conversation panel; show-and-tell context | Opus 5.5 · high |
| WP6 | User tests (3 people), real model run, closing | User + Opus 5.5 · high |

## 4. Test plan

- Selective undo:
  - a later change that does not overlap is kept;
  - an overlapping change is rejected, and who and which revision is reported;
  - an undo can be undone;
  - the same card cannot be undone twice;
  - an outside change closes older cards.
- Hand-over (exit criterion 3): a fake agent changes one entity while the person changes another. Then the agent's card is undone: the person's change stays in place.
- Feed: every card carries who/what/why; an outside agent (MCP) and the cockpit agent are in the same feed.

## 5. Implementation notes (17 November 2026, night; Opus 5.5 · high)

### WP1 — selective undo
- `wish_scene`: `SceneDocument::revert / revertible / is_reverted`.
- **Criterion:** value-based. A card can be undone if everything it changed (resource, component, override, parent, entity) still has the value the card left. The first attempt was history-based: when an undo itself was undone, that counted as a conflict too; the value-based criterion behaves correctly.
- When rejected, `revert_conflict` names the later changes and who made them.
- An undo is a new operation; it can be undone and redone. Undoing an undo puts the card back in force.
- `already_reverted`, `not_revertible`.
- `edit_apply reason`; `edit_history revert`; the list gives each card's reason, its undoability (or why not) and whether it has been undone.
- Editor: "Undo this" in the History tab.

### WP2 — activity feed
- `wish_control::activity`, with sequence numbers, at most 2000 records. What is recorded:
  - every call that changes something (whoever makes it),
  - every call of the agents, reading ones too.
- The person's reads and the list/get actions of tools that change things are not recorded.
- `activity_get` (the 27th tool).
- Editor: Activity tab.
  - Agents are shown in color; there is an "Agents only" filter.
  - The reason is under the card, errors in red.
  - An edit card has "Undo this".

### WP3 — checkpoints and before/after
- `checkpoint` (the 28th tool): save, list, restore, delete.
- Restore maps every document with a single operation (diff operations); it enters the history and can be undone.
  - Scenes made later are kept.
  - Deleted documents are written back.
  - Settings files are written back.
- `capture_frame checkpoint` and `PictureSource::Checkpoint`.
- Editor: Checkpoints in the project menu; a "Before / Now" window.

### WP4 — permission modes
- The `permissions` (29th) and `proposals` (30th) tools.
- **suggest:** an agent's call that changes the project is held; a dry-run preview is shown when possible.
- **apply:** the default.
- **autonomous.**
- The person is never held. Running the game, testing, building and looking are never held.
- An accepted proposal is applied under the agent's name.
- Editor: the Permissions submenu; a "Waiting for you" list in the Activity tab.

### WP5 — cockpit
- **Endpoint:** `wish_mcp::http::Endpoint`, split from the loop of the HTTP transport. Its owner runs the pending calls in its own workspace.
- **`wish_agent`** (new crate):
  - `Adapter` / `Run` / `Event`.
  - `ClaudeCode` (child process, the person's own account; the `stream-json` parser has unit tests).
  - `Scripted` (tests).
  - An MCP client that uses only std.
- **Editor:**
  - "Agent" panel: conversation transcript; show and tell (add the selection, mark in the view); Send and Stop.
  - A checkpoint before every task.
  - The agent's calls run in the editor's frame, changes are visible at once.
  - "Connect an outside agent" (address, token, `claude mcp add` command).
- **Tests:** `tests/cockpit.rs`.
  - Agent from the cockpit; the person's simultaneous edit is kept.
  - Outside agent, cockpit agent and person at the same time: nothing is lost, every change carries its maker's name (exit criterion 5).

### Exit criteria — status

| # | Criterion | Status |
|---|---|---|
| 1 | Mini game by talking in 30 minutes (3 people) | **Owned by the user:** must be tried with a real model (Claude Code, the person's account) and 3 people |
| 2 | Every change of the agent is in the feed, undone with one click | ✅ Activity tab + "Undo this" (WP1–WP2, tested) |
| 3 | Hand-over, no change is lost | ✅ Value-based selective undo; cockpit test |
| 4 | Changes by hand and by agent in the same history, with who made them | ✅ |
| 5 | Outside agent and cockpit agent at the same time, no data loss | ✅ `cockpit.rs::an_outside_agent_and_the_cockpit_agent_work_together` |

**Still open:**
- T1 cockpit mock-ups and user trial.
- A real cockpit run with Claude Code (uses quota). The program path can be given with `WISH_CLAUDE`.
- A long agent call (a very long `sim_step`) holds the editor's frame for that time; progress and thread separation come later.
- The value of "show and tell" with a real model has not been measured.
