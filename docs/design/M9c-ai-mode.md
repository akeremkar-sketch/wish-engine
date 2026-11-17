# M9c — AI mode ("By hand ⇄ With AI")

**Status:** WP1–WP7 code side done, 17 November 2026 (§7). Waiting for the user's trial. At the user's request; scope approved ("really work hard on this part").
**Recommended model · effort:** Opus 5.5 · xhigh.

## 1. Problem

Today the editor shows a single face: the panels of manual editing and a small Agent panel beside them. For someone who builds a game with AI, this puts the wrong part of the work in front. The hierarchy and the inspector fill the screen; what the game **is**, what has been **decided** and what is **next** are written nowhere.

Agents make bad games mostly because of missing context:
- every request starts from zero;
- decisions that were made are forgotten;
- choices that belong to the person (difficulty, color, feel) are filled in by the agent with guesses;
- how the game feels when played never reaches the agent.

## 2. Decision: two modes, one project

There is a switch always visible in the top bar: **Manual | With AI**.
- Same project, same history, same files. The mode only changes the screen; invariant 9 is kept.
- **By hand:** today's editor.
- **With AI:** the manual editing panels are hidden; in their place opens a workspace that describes the game, steers the agent and judges the result by playing.

### Parts of AI mode and their reasons

| Part | Place | What it is for (why it makes a better game) |
|---|---|---|
| **Game card** | Left | The identity of the game. Fields: name, one sentence, genre, the player's goal, core loop ("what does the player do every 10 seconds"), controls, look and feel, target audience, **must-haves**, **left for later**. The agent reads this in every task; scope drift and inconsistency go down. The person fills it in, or says "Let the agent draft it". |
| **Decisions** | Left (tab) | Pinned rules such as "jump is 2.5 m", "pastel colors", "no violence". It shows who set each one. The agent obeys them in every task; when the agent proposes a decision, it arrives as a card and the person approves. This is the memory across sessions. |
| **Chat and task cards** | Middle | The conversation itself. Every task is a card: the request, the agent's steps (tool calls, short), the result, a **before/after picture**, "Undo" and "Continue". Chat memory is kept (the Claude Code session is resumed); "New chat" resets it. |
| **Questions** | Middle, above the input box | For choices that belong to the person, the agent does not guess; it asks a **question with options** ("Number of enemies?" 3 / 5 / 10). The person taps an option or writes their own answer; if wanted, the answer is saved as a decision; the agent continues from where it stopped. |
| **Ready-made requests** | Below the input box | Guided requests for common jobs: add a level, add an enemy, add sound, "game feel" (polish), fix bugs, balance the game. Each sends a well-built request that uses the game card and the decisions. |
| **Live game** | Top right | The game view; Play, Stop, Restart. While the agent works, the scene changes live. |
| **Play-test notes** | Right, under the live game | While playing, "Take note": the tick, the scene and the picture of that moment are saved with the note ("this part is too hard"). "Send to agent" turns the notes into a single task. This is how the feel of the game reaches the agent; it is the most missing part of making games with AI. |
| **Plan** | Right | A list of steps; the agent writes, the person adds, orders and ticks. "Do the next one" starts the next step as a task. Big jobs are split into small steps that can be reviewed. |
| **Health** | Bottom right | Problems, tests, the last build. If it is red, "Let the agent fix". The person sees at once what the agent broke. |
| **AI settings** | Button in the top bar | The helper (is Claude Code found), the **model**, the permission mode, chat memory, a checkpoint before every task, what the agent may touch (scenes / game code), the spending of this session. |

## 3. Control plane (invariants 4, 8, 9)

**File:** `design/game.json`. Canonical JSON, git friendly, part of the project; not an asset, not a document either (`design/` does not enter the asset index).

```json
{
  "format": 1,
  "brief": {
    "title": "", "pitch": "", "genre": "", "goal": "", "loop": "", "controls": "",
    "look": "", "audience": "", "must": [], "later": []
  },
  "decisions": [{ "id": 1, "text": "...", "by": "human" }],
  "plan": [{ "id": 1, "text": "...", "status": "todo", "by": "agent:claude-code" }],
  "notes": [{ "id": 1, "text": "...", "scene": "scenes/main.scene.json", "tick": 120, "picture": "design/notes/1.png", "status": "open" }],
  "questions": [{ "id": 1, "text": "...", "options": ["3", "5", "10"], "answer": null }],
  "next_id": 2
}
```

**Tool `game_design` (32nd):**
- `get`
- `set_brief` (only the given fields)
- `decide`, `forget`
- `plan_add`, `plan_set`, `plan_remove`
- `note`, `note_done`
- `ask`, `answer` (`remember: true` → decision)

Every action that changes something accepts `dry_run`. It enters the history and the activity feed with its `actor`; reading (`get`) does not enter the feed. Ids are deterministic (`next_id`), there are no timestamps.

**Context sent to the agent:** `wish_agent::Task` carries a `design` summary (the game card, the decisions, the open plan steps, the answered questions). `instructions` puts it in front of the request and tells the agent:
- keep the game card and the plan up to date;
- for a choice that belongs to the person, ask a question with options using `game_design ask` and stop;
- propose lasting rules with `decide` and save them.

**Claude Code adapter:**
- the `model` option (`--model`);
- chat memory (`--resume <session>`: the `session_id` in the stream);
- spending (`total_cost_usd` in the result event) arrives as an `Event::Usage`.

## 4. Parity (invariant 9)

Every action of `game_design` can be done by hand from the panels of AI mode: writing the card, adding and removing decisions, the plan, notes, answering a question. A row is added to `wish_editor::parity::PARITY`, with a test that does it by hand.

## 5. Work packages

1. The `game_design` tool, the file, tests, MCP count, parity rows.
2. Agent context: design summary, model, chat memory, spending.
3. The mode switch and the AI mode layout.
4. Game card, decisions, questions, plan, health panels.
5. Play-test notes; ready-made requests; settings window.
6. Mark and opening animation.
7. Translations, guides, tests, golden images, closing review.

## 6. Test plan

- **`wish_control`:**
  - the `game_design` actions;
  - `dry_run`;
  - invalid input gives an error with a hint;
  - the file is canonical and the same after reloading;
  - history and activity.
- **`wish_agent`:**
  - the game card and decisions in the instructions;
  - the `--model` and `--resume` arguments;
  - `session_id` and spending from the stream.
- **`wish_editor`:**
  - the mode switch changes the layout and stays in the preferences;
  - the card is filled in by hand → file;
  - tapping a question option → answer and decision, the fake agent gets a continue task;
  - plan "Do the next one" starts a task;
  - a play-test note is saved with tick and picture, "Send to agent" is a single task;
  - parity tests.

## 7. Result (17 November 2026)

- **Control plane:** `game_design` (32nd tool, `wish_control/src/tools/design.rs`), `design/game.json` canonical, `design/` is not an asset (`wish_asset::NON_ASSET_FOLDERS`), new diagnostics `design_file_invalid`, `design_item_not_found`, `invalid_design`. `wish_control::design_summary` produces the summary the agent reads. Tests: `wish_control/tests/game_design.rs` (7).
- **Agent:** `wish_agent::Task` now carries `design`, `model`, `resume`, `files` (`Task::new`); the instructions give the design and the rule "ask and stop on a choice that belongs to the person" with `DESIGN_RULES`. Claude Code: `--model`, `--resume` (only safe words), `files: false` → only the editor's tools; `Event::Session` and `Event::Cost` from the stream.
- **Editor:** `wish_editor::ai_mode` (game panel, chat + question cards + ready-made requests, plan / notes / health board, settings window, live view Game | Scene). The mode and the agent settings are in the preferences (`ai_mode`, `agent`). If the assistant is chosen on the welcome screen, the editor opens in AI mode. Every task card is undone with its own checkpoint and compared before/after. In the scene view, clicking adds the entity to the request instead of selecting it. Tests: `wish_editor/tests/ai_mode.rs` (6; parity row `ai_mode::the_design_is_made_by_hand`).
- **Mark:** first a wishbone (lades kemiği) was made; at the user's request it became a **wishing wand** (`vector::draw_wand`). A cream wand, orange handle rings and tip; behind it four specks of dust left by the swing, in front of it a golden wish light. A 2-second animation at launch: the wand is swung from below, its tip draws an arc, it stops with a small shake; the arc scatters into the dust, the light ignites with a sparkle (a click or key skips it; `LaunchOptions::splash`). Tried and dropped: a curl (it looked like a cane), three dots (like "loading"), a continuous arc (it looked like a rake). Window and taskbar icon (`wish_editor::window_icon`), shortcut icon `wish-engine.ico`, the icon of AI mode `Icon::Wish`. The four-pointed star/sparkle stayed only for particles (resemblance to Gemini).
- **Translations:** 138 new strings × 11 languages.
