//! The AI mode (M9c): the editor's second face, for making a game with an
//! agent. The manual panels step aside; in their place:
//!
//! - left, **the game**: the game card (what the game is) and the
//!   decisions that hold for every change;
//! - middle, the **live game** and below it the **plan**, the **play-test
//!   notes** and the project's **health**;
//! - right, the **conversation**: task cards (each with its undo and its
//!   before/after), the agent's **questions** with their choices, ready
//!   requests and the request box.
//!
//! The same project, history and files as the manual mode (invariant 9):
//! everything here is a `game_design` call or a task for the agent, and
//! the manual mode does all of it by hand too.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use egui::{Color32, CornerRadius, Margin, RichText, Stroke, Ui};
use serde_json::{Map, Value, json};

use crate::cockpit::{Cockpit, CockpitActions, Who};
use crate::i18n::{t, tf};

/// A text of the interface, in the language shown.
type Text = fn() -> &'static str;

/// The game card's fields: the key, the name, an example, and whether the
/// box has more than one line.
pub(crate) const CARD: &[(&str, Text, Text, bool)] = &[
    (
        "title",
        || t("Name"),
        || t("What the game is called"),
        false,
    ),
    (
        "pitch",
        || t("In one sentence"),
        || t("Hop between floating islands to light every lighthouse"),
        true,
    ),
    (
        "genre",
        || t("Genre"),
        || t("Platformer, puzzle, racing..."),
        false,
    ),
    (
        "goal",
        || t("The player's goal"),
        || t("What the player wants to reach"),
        true,
    ),
    (
        "loop",
        || t("Core loop"),
        || t("What the player does every few seconds"),
        true,
    ),
    (
        "controls",
        || t("Controls"),
        || t("Arrows to move, Space to jump"),
        false,
    ),
    (
        "look",
        || t("Look and feel"),
        || t("Art style, colours, mood, sounds"),
        true,
    ),
    (
        "audience",
        || t("For whom"),
        || t("Who plays it, and for how long"),
        false,
    ),
    (
        "must",
        || t("Must have"),
        || t("One per line: what the game cannot do without"),
        true,
    ),
    (
        "later",
        || t("Later"),
        || t("One per line: good ideas that wait"),
        true,
    ),
];

/// The left panel's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Left {
    #[default]
    Card,
    Decisions,
}

/// The board under the live game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Board {
    #[default]
    Plan,
    Notes,
    Health,
}

/// What the AI mode's panels are writing (kept between frames).
#[derive(Default)]
pub(crate) struct AiState {
    pub left: Left,
    pub board: Board,
    /// The card as being written, and the card it was taken from.
    pub card: BTreeMap<String, String>,
    pub card_from: Option<Value>,
    pub decision: String,
    pub step: String,
    pub note: String,
    /// A question answered in the person's own words, by question id.
    pub own: BTreeMap<u64, String>,
    /// Questions whose answer is not to be kept as a decision.
    pub not_kept: BTreeSet<u64>,
    /// The AI settings window is open.
    pub settings: bool,
    /// The live view shows the scene through the editor's eye instead of
    /// the game's camera.
    pub scene_view: bool,
}

/// What the AI mode's panels asked for.
#[derive(Default)]
pub(crate) struct AiActions {
    pub save_card: Option<Map<String, Value>>,
    /// Use this workspace (`Some(None)`: the default).
    pub workspace: Option<Option<String>>,
    pub draft_card: bool,
    pub decide: Option<String>,
    pub forget: Option<u64>,
    pub plan_add: Option<String>,
    pub plan_status: Option<(u64, &'static str)>,
    pub plan_remove: Option<u64>,
    pub next_step: bool,
    pub note: Option<String>,
    pub note_done: Option<u64>,
    pub send_notes: bool,
    /// A question answered: its id, the answer, whether to keep it as a
    /// decision.
    pub answer: Option<(u64, String, bool)>,
    /// A ready request to put in the request box.
    pub ready: Option<String>,
    pub fix: bool,
    pub run_tests: bool,
    pub build: bool,
    /// Open (true) or close the AI settings.
    pub settings: Option<bool>,
    pub model: Option<Option<String>>,
    pub memory: Option<bool>,
    pub checkpoint_first: Option<bool>,
    pub files: Option<bool>,
    pub new_conversation: bool,
    pub scene_view: Option<bool>,
    /// A task card's "Undo": go back to the checkpoint saved before it.
    pub undo_task: Option<String>,
    /// A task card's "Before / after".
    pub compare_task: Option<String>,
    /// Show the manual mode's Activity tab (changes waiting there).
    pub show_activity: bool,
    /// What the conversation's own buttons asked for (send, stop).
    pub cockpit: CockpitActions,
}

impl AiActions {
    pub fn any(&self) -> bool {
        self.save_card.is_some()
            || self.workspace.is_some()
            || self.draft_card
            || self.decide.is_some()
            || self.forget.is_some()
            || self.plan_add.is_some()
            || self.plan_status.is_some()
            || self.plan_remove.is_some()
            || self.next_step
            || self.note.is_some()
            || self.note_done.is_some()
            || self.send_notes
            || self.answer.is_some()
            || self.ready.is_some()
            || self.fix
            || self.run_tests
            || self.build
            || self.settings.is_some()
            || self.model.is_some()
            || self.memory.is_some()
            || self.checkpoint_first.is_some()
            || self.files.is_some()
            || self.new_conversation
            || self.scene_view.is_some()
            || self.undo_task.is_some()
            || self.compare_task.is_some()
            || self.show_activity
            || self.cockpit.any()
    }
}

/// What the AI mode shows besides its own state.
pub(crate) struct AiView<'a> {
    pub design: &'a Value,
    pub cockpit: &'a Cockpit,
    pub state: &'a RefCell<AiState>,
    /// Errors and warnings, and what `check` found.
    pub problems: (u64, u64),
    pub found: &'a [Value],
    /// The game code is there but not built.
    pub unbuilt: bool,
    /// The last test run: passed, failed.
    pub tests: Option<(usize, usize)>,
    /// The game's tick, while it plays.
    pub tick: Option<u64>,
    /// The permission mode and how many agent changes wait for the person.
    pub permission: &'a str,
    pub waiting: usize,
    /// Where Claude Code is, if it was found.
    pub assistant: Option<&'a str>,
    /// The workspaces there are, and the project's (`None`: the default).
    pub workspaces: &'a [crate::workspaces::WorkspaceEntry],
    pub workspace: Option<&'a crate::workspaces::WorkspaceEntry>,
}

/// A field of the card as the panel shows it.
pub(crate) struct Field {
    pub key: String,
    pub label: String,
    pub hint: String,
    pub list: bool,
    pub tall: bool,
}

/// The card's fields: the workspace's, else the ones the design file names
/// (its package is not installed here), else the default card.
pub(crate) fn fields(view: &AiView) -> Vec<Field> {
    if let Some(workspace) = view.workspace {
        return workspace
            .card
            .iter()
            .map(|field| Field {
                key: field.key.clone(),
                label: field.label.clone(),
                hint: field.hint.clone(),
                list: field.list,
                tall: field.tall,
            })
            .collect();
    }
    if view.design["card"].is_array() {
        return wish_control::card_fields(view.design)
            .into_iter()
            .map(|field| Field {
                tall: field.list,
                list: field.list,
                hint: String::new(),
                label: field.label,
                key: field.key,
            })
            .collect();
    }
    CARD.iter()
        .map(|(key, name, hint, tall)| Field {
            key: (*key).to_owned(),
            label: name().to_owned(),
            hint: hint().to_owned(),
            list: matches!(*key, "must" | "later"),
            tall: *tall,
        })
        .collect()
}

/// The switch between the two modes, in the menu bar: returns the mode
/// asked for (true: with AI).
pub(crate) fn switch(ui: &mut Ui, ai: bool) -> Option<bool> {
    let mut asked = None;
    egui::Frame::new()
        .fill(crate::style::pal().raised)
        .stroke(Stroke::new(1.0, crate::style::pal().line))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for (on, icon, name, hover) in [
                    (
                        false,
                        crate::style::Icon::Hand,
                        t("Manual"),
                        t("Make the game by hand: hierarchy, inspector, assets"),
                    ),
                    (
                        true,
                        crate::style::Icon::Wish,
                        t("With AI"),
                        t("Make the game with an agent: the game card, decisions, plan, play-test notes and the conversation"),
                    ),
                ] {
                    let chosen = ai == on;
                    let tint = if chosen { Some(if on { crate::style::accent_text() } else { crate::style::pal().select }) } else { None };
                    if crate::style::tool_button(ui, icon, name, true, chosen, true, tint)
                        .on_hover_text(hover)
                        .clicked()
                        && !chosen
                    {
                        asked = Some(on);
                    }
                }
            });
        });
    asked
}

fn card_frame(ui: &mut Ui, fill: Color32, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, crate::style::pal().line))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn tab(ui: &mut Ui, selected: bool, name: &str) -> bool {
    let text = RichText::new(name).color(if selected {
        crate::style::pal().text
    } else {
        crate::style::pal().weak
    });
    let response = ui.selectable_label(selected, text);
    response.clicked() && !selected
}

fn who(by: &str) -> String {
    if by == "human" {
        t("you").to_owned()
    } else {
        t("agent").to_owned()
    }
}

fn items<'a>(design: &'a Value, list: &str) -> impl Iterator<Item = &'a Value> {
    design[list].as_array().into_iter().flatten()
}

/// The card's text for a field, as the box shows it (lists one per line).
pub(crate) fn field_text(value: &Value) -> String {
    match value {
        Value::Array(lines) => lines
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        Value::String(text) => text.clone(),
        _ => String::new(),
    }
}

/// The left panel: the game card and the decisions.
pub(crate) fn game_panel(ui: &mut Ui, view: &AiView, actions: &mut AiActions) {
    let mut state = view.state.borrow_mut();
    // The boxes follow the file when it changed and nothing was being
    // written (an agent wrote the card).
    let brief = &view.design["brief"];
    let card_fields = fields(view);
    if state.card_from.as_ref() != Some(brief) {
        let editing = state.card_from.as_ref().is_some_and(|from| {
            card_fields.iter().any(|field| {
                state.card.get(&field.key).map(String::as_str).unwrap_or("")
                    != field_text(&from[&field.key])
            })
        });
        if !editing {
            state.card = card_fields
                .iter()
                .map(|field| (field.key.clone(), field_text(&brief[&field.key])))
                .collect();
            state.card_from = Some(brief.clone());
        }
    }
    crate::style::panel_title(ui, t("The game"));
    // The game's own name and promise, as a cover.
    let title = brief["title"].as_str().unwrap_or("").trim();
    let pitch = brief["pitch"].as_str().unwrap_or("").trim();
    if !title.is_empty() || !pitch.is_empty() {
        ui.add_space(2.0);
        if !title.is_empty() {
            ui.label(
                RichText::new(title)
                    .size(20.0)
                    .strong()
                    .color(crate::style::pal().text),
            );
        }
        if !pitch.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(pitch)
                        .italics()
                        .color(crate::style::pal().weak),
                )
                .wrap(),
            );
        }
        ui.add_space(4.0);
    }
    // The workspace: what kind of game the card is for.
    let chosen = view.workspace.map_or_else(
        || t("Default").to_owned(),
        |workspace| workspace.name.clone(),
    );
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(t("Workspace"))
                .color(crate::style::pal().weak)
                .size(12.0),
        );
        egui::ComboBox::from_id_salt("ai-workspace")
            .selected_text(chosen)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(view.workspace.is_none(), t("Default"))
                    .on_hover_text(t("The engine's own card, for any game"))
                    .clicked()
                    && view.workspace.is_some()
                {
                    actions.workspace = Some(None);
                }
                for workspace in view.workspaces {
                    let is = view.workspace.is_some_and(|w| w.key == workspace.key);
                    if ui
                        .selectable_label(is, &workspace.name)
                        .on_hover_text(&workspace.description)
                        .clicked()
                        && !is
                    {
                        actions.workspace = Some(Some(workspace.key.clone()));
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        let decisions = items(view.design, "decisions").count();
        if tab(ui, state.left == Left::Card, t("Game card")) {
            state.left = Left::Card;
        }
        if tab(
            ui,
            state.left == Left::Decisions,
            &tf("Decisions ({count})", &[("count", &decisions)]),
        ) {
            state.left = Left::Decisions;
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("ai-left")
        .auto_shrink([false, false])
        .show(ui, |ui| match state.left {
            Left::Card => card(ui, view, &mut state, actions),
            Left::Decisions => decisions(ui, view, &mut state, actions),
        });
}

fn card(ui: &mut Ui, view: &AiView, state: &mut AiState, actions: &mut AiActions) {
    let card_fields = fields(view);
    let filled = card_fields
        .iter()
        .filter(|field| {
            !field_text(&view.design["brief"][&field.key])
                .trim()
                .is_empty()
        })
        .count();
    ui.weak(tf(
        "{filled} of {all} filled. The agent reads the card before every task.",
        &[("filled", &filled), ("all", &card_fields.len())],
    ));
    ui.add_space(4.0);
    for field in &card_fields {
        ui.label(
            RichText::new(&field.label)
                .color(crate::style::pal().weak)
                .size(12.0),
        );
        let text = state.card.entry(field.key.clone()).or_default();
        let edit = if field.tall {
            egui::TextEdit::multiline(text).desired_rows(2)
        } else {
            egui::TextEdit::singleline(text)
        };
        ui.add(
            edit.desired_width(f32::INFINITY)
                .hint_text(&field.hint)
                .id_salt(("card", field.key.as_str())),
        )
        .on_hover_text(&field.label);
        ui.add_space(2.0);
    }
    let changed: Map<String, Value> = card_fields
        .iter()
        .filter_map(|field| {
            let key = &field.key;
            let now = state.card.get(key).cloned().unwrap_or_default();
            let was = field_text(&view.design["brief"][key]);
            (now != was).then(|| {
                let value = if field.list {
                    Value::Array(
                        now.lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(|line| json!(line))
                            .collect(),
                    )
                } else {
                    json!(now.trim())
                };
                (key.clone(), value)
            })
        })
        .collect();
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!changed.is_empty(), egui::Button::new(t("Save card")))
            .clicked()
        {
            actions.save_card = Some(changed.clone());
        }
        if !changed.is_empty() && ui.button(t("Discard")).clicked() {
            state.card_from = None;
            state.card.clear();
        }
        if ui
            .add_enabled(
                view.cockpit.run.is_none(),
                egui::Button::new(t("Let the agent draft it")),
            )
            .on_hover_text(t(
                "The agent writes the card from the game as it is and asks you what it cannot tell",
            ))
            .clicked()
        {
            actions.draft_card = true;
        }
    });
}

fn decisions(ui: &mut Ui, view: &AiView, state: &mut AiState, actions: &mut AiActions) {
    ui.weak(t(
        "Rules that hold for every change (\"jump height 2.5 m\", \"pastel colours\"). The agent keeps to them and adds the ones you agree on.",
    ));
    ui.add_space(4.0);
    let mut any = false;
    for decision in items(view.design, "decisions") {
        any = true;
        let id = decision["id"].as_u64().unwrap_or(0);
        card_frame(ui, crate::style::pal().raised, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button("x")
                    .on_hover_text(t("Remove this decision"))
                    .clicked()
                {
                    actions.forget = Some(id);
                }
                ui.add(egui::Label::new(decision["text"].as_str().unwrap_or("")).wrap());
            });
            ui.weak(tf(
                "decided by {who}",
                &[("who", &who(decision["by"].as_str().unwrap_or("")))],
            ));
        });
    }
    if !any {
        ui.weak(t("No decisions yet."));
    }
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(&mut state.decision)
            .desired_width(f32::INFINITY)
            .hint_text(t("A new decision"))
            .id_salt("ai-decision"),
    );
    if ui
        .add_enabled(
            !state.decision.trim().is_empty(),
            egui::Button::new(t("Add decision")),
        )
        .clicked()
    {
        actions.decide = Some(std::mem::take(&mut state.decision));
    }
}

/// The board under the live game: plan, play-test notes, health.
pub(crate) fn board(ui: &mut Ui, view: &AiView, actions: &mut AiActions) {
    let mut state = view.state.borrow_mut();
    let open_notes = items(view.design, "notes")
        .filter(|note| note["status"] == "open")
        .count();
    let (errors, _) = view.problems;
    let failed = view.tests.map_or(0, |(_, failed)| failed);
    let unwell = errors > 0 || failed > 0 || view.unbuilt;
    ui.horizontal(|ui| {
        let steps = items(view.design, "plan")
            .filter(|step| step["status"] != "done")
            .count();
        if tab(
            ui,
            state.board == Board::Plan,
            &tf("Plan ({count})", &[("count", &steps)]),
        ) {
            state.board = Board::Plan;
        }
        if tab(
            ui,
            state.board == Board::Notes,
            &tf("Play-test notes ({count})", &[("count", &open_notes)]),
        ) {
            state.board = Board::Notes;
        }
        let health = if unwell {
            RichText::new(t("Health")).color(crate::style::pal().red)
        } else {
            RichText::new(t("Health")).color(crate::style::pal().go)
        };
        let selected = state.board == Board::Health;
        if ui.selectable_label(selected, health).clicked() && !selected {
            state.board = Board::Health;
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("ai-board")
        .auto_shrink([false, false])
        .show(ui, |ui| match state.board {
            Board::Plan => plan(ui, view, &mut state, actions),
            Board::Notes => notes(ui, view, &mut state, actions),
            Board::Health => health(ui, view, actions),
        });
}

fn status_name(status: &str) -> &'static str {
    match status {
        "doing" => t("doing"),
        "done" => t("done"),
        _ => t("to do"),
    }
}

fn plan(ui: &mut Ui, view: &AiView, state: &mut AiState, actions: &mut AiActions) {
    let next = items(view.design, "plan").find(|step| step["status"] != "done");
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                next.is_some() && view.cockpit.run.is_none(),
                egui::Button::new(
                    RichText::new(t("Do the next step")).color(crate::style::accent_text()),
                ),
            )
            .on_hover_text(t("The agent does the first step that is not done"))
            .clicked()
        {
            actions.next_step = true;
        }
        ui.weak(t(
            "Small steps the agent can do one at a time; it writes them too.",
        ));
    });
    ui.add_space(4.0);
    let mut any = false;
    for step in items(view.design, "plan") {
        any = true;
        let id = step["id"].as_u64().unwrap_or(0);
        let status = step["status"].as_str().unwrap_or("todo");
        ui.horizontal(|ui| {
            let (color, following) = match status {
                "todo" => (crate::style::pal().weak, "doing"),
                "doing" => (crate::style::pal().amber, "done"),
                _ => (crate::style::pal().go, "todo"),
            };
            if ui
                .add(egui::Button::new(RichText::new(status_name(status)).color(color)).small())
                .on_hover_text(tf(
                    "Mark it {status}",
                    &[("status", &status_name(following))],
                ))
                .clicked()
            {
                actions.plan_status = Some((id, following));
            }
            let text = step["text"].as_str().unwrap_or("");
            let text = if status == "done" {
                RichText::new(text)
                    .strikethrough()
                    .color(crate::style::pal().weak)
            } else {
                RichText::new(text)
            };
            ui.add(egui::Label::new(text).wrap());
            if ui
                .small_button("x")
                .on_hover_text(t("Remove this step"))
                .clicked()
            {
                actions.plan_remove = Some(id);
            }
        });
    }
    if !any {
        ui.weak(t(
            "No plan yet: write a step, or ask the agent to plan the game.",
        ));
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.step)
                .desired_width((ui.available_width() - 90.0).max(80.0))
                .hint_text(t("A new step"))
                .id_salt("ai-step"),
        );
        if ui
            .add_enabled(
                !state.step.trim().is_empty(),
                egui::Button::new(t("Add step")),
            )
            .clicked()
        {
            actions.plan_add = Some(std::mem::take(&mut state.step));
        }
    });
}

fn notes(ui: &mut Ui, view: &AiView, state: &mut AiState, actions: &mut AiActions) {
    ui.weak(match view.tick {
        Some(tick) => tf(
            "Playing (tick {tick}): a note keeps this moment and its picture.",
            &[("tick", &tick)],
        ),
        None => t("Play the game and write what you feel: too hard, too slow, unclear. A note keeps the moment and its picture.")
            .to_owned(),
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.note)
                .desired_width((ui.available_width() - 100.0).max(80.0))
                .hint_text(t("What did you notice?"))
                .id_salt("ai-note"),
        );
        if ui
            .add_enabled(
                !state.note.trim().is_empty(),
                egui::Button::new(t("Take a note")),
            )
            .clicked()
        {
            actions.note = Some(std::mem::take(&mut state.note));
        }
    });
    ui.add_space(4.0);
    let open: Vec<&Value> = items(view.design, "notes")
        .filter(|note| note["status"] == "open")
        .collect();
    for note in &open {
        let id = note["id"].as_u64().unwrap_or(0);
        card_frame(ui, crate::style::pal().raised, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button(t("Done"))
                    .on_hover_text(t("It is fixed: close the note"))
                    .clicked()
                {
                    actions.note_done = Some(id);
                }
                ui.add(egui::Label::new(note["text"].as_str().unwrap_or("")).wrap());
            });
            let mut where_ = Vec::new();
            if let Some(tick) = note["tick"].as_u64() {
                where_.push(tf("tick {tick}", &[("tick", &tick)]));
            }
            if note["picture"].is_string() {
                where_.push(t("with a picture").to_owned());
            }
            if !where_.is_empty() {
                ui.weak(where_.join(" · "));
            }
        });
    }
    if open.is_empty() {
        ui.weak(t("No open notes."));
    } else if ui
        .add_enabled(
            view.cockpit.run.is_none(),
            egui::Button::new(
                RichText::new(t("Send the notes to the agent")).color(crate::style::accent_text()),
            ),
        )
        .on_hover_text(t(
            "One task: the agent fixes what the notes say and closes them",
        ))
        .clicked()
    {
        actions.send_notes = true;
    }
    let done = items(view.design, "notes")
        .filter(|note| note["status"] != "open")
        .count();
    if done > 0 {
        ui.weak(tf("{count} note(s) done", &[("count", &done)]));
    }
}

fn health(ui: &mut Ui, view: &AiView, actions: &mut AiActions) {
    let (errors, warnings) = view.problems;
    let row = |ui: &mut Ui, good: bool, text: String| {
        ui.horizontal(|ui| {
            let color = if good {
                crate::style::pal().go
            } else {
                crate::style::pal().red
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, color);
            ui.label(text);
        });
    };
    row(
        ui,
        errors == 0,
        tf(
            "{errors} error(s), {warnings} warning(s)",
            &[("errors", &errors), ("warnings", &warnings)],
        ),
    );
    for problem in view.found.iter().take(5) {
        ui.weak(format!("· {}", problem["message"].as_str().unwrap_or("")));
    }
    row(
        ui,
        !view.unbuilt,
        if view.unbuilt {
            t("The game code is not built").to_owned()
        } else {
            t("The game code is built (or there is none)").to_owned()
        },
    );
    match view.tests {
        Some((passed, failed)) => row(
            ui,
            failed == 0,
            tf(
                "tests: {passed} passed, {failed} failed",
                &[("passed", &passed), ("failed", &failed)],
            ),
        ),
        None => {
            ui.weak(t("The tests have not run yet."));
        }
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if view.unbuilt && ui.button(t("Build code")).clicked() {
            actions.build = true;
        }
        if ui.button(t("Run tests")).clicked() {
            actions.run_tests = true;
        }
        let failed = view.tests.is_some_and(|(_, failed)| failed > 0);
        if ui
            .add_enabled(
                (errors > 0 || failed || view.unbuilt) && view.cockpit.run.is_none(),
                egui::Button::new(
                    RichText::new(t("Let the agent fix it")).color(crate::style::accent_text()),
                ),
            )
            .on_hover_text(t(
                "The problems and failing tests go to the agent as one task",
            ))
            .clicked()
        {
            actions.fix = true;
        }
    });
}

/// The ready requests: a name and the request it writes.
pub(crate) fn ready_requests() -> [(&'static str, &'static str); 7] {
    [
        (
            t("Add a level"),
            t(
                "Add a new level that builds on what the player has learned so far. Keep to the game card and the decisions, and test that it can be won.",
            ),
        ),
        (
            t("Add an enemy"),
            t(
                "Add an enemy or obstacle that fits the game. Ask me what it should be like if the card does not say.",
            ),
        ),
        (
            t("Add sounds"),
            t(
                "Add sounds for the player's main actions and for winning and losing (asset_create makes simple ones).",
            ),
        ),
        (
            t("Game feel"),
            t(
                "Polish how the game feels: controls that answer at once, feedback when something happens (particles, sound, camera), without changing the rules.",
            ),
        ),
        (
            t("Balance it"),
            t(
                "Play the game with input (sim_step) and balance it: is it too hard or too easy? Tell me what you changed and why.",
            ),
        ),
        (
            t("Find bugs"),
            t(
                "Play the game through and look for bugs. Fix what you find and add a scenario test for each.",
            ),
        ),
        (
            t("Plan the game"),
            t(
                "Compare the game with its card and write the plan: small steps, in order, each one testable (game_design plan_add).",
            ),
        ),
    ]
}

/// The right panel: the conversation, the questions, the request box.
pub(crate) fn conversation(ui: &mut Ui, view: &AiView, actions: &mut AiActions) {
    let cockpit = view.cockpit;
    ui.horizontal(|ui| {
        crate::style::panel_title(ui, t("Conversation"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(t("AI settings"))
                .on_hover_text(t("The assistant, the model, permissions, memory"))
                .clicked()
            {
                actions.settings = Some(true);
            }
            if ui
                .small_button(t("New conversation"))
                .on_hover_text(t(
                    "The agent forgets this conversation (the game card and the decisions stay)",
                ))
                .clicked()
            {
                actions.new_conversation = true;
            }
        });
    });
    ui.weak(tf(
        "{agent} · {mode}{memory}",
        &[
            ("agent", &cockpit.agent.name()),
            ("mode", &crate::app::mode_label(view.permission)),
            (
                "memory",
                &if cockpit.session.is_some() {
                    format!(" · {}", t("remembers this conversation"))
                } else {
                    String::new()
                },
            ),
        ],
    ));
    if view.waiting > 0 {
        card_frame(ui, crate::style::pal().accent_soft, |ui| {
            ui.label(tf(
                "{count} change(s) by the agent wait for you",
                &[("count", &view.waiting)],
            ));
            if ui.button(t("Review them")).clicked() {
                actions.show_activity = true;
            }
        });
    }
    ui.separator();
    // The questions and the request box keep their place at the bottom;
    // the conversation fills what is left above them.
    egui::Panel::bottom("ai-input")
        .frame(egui::Frame::NONE)
        .resizable(false)
        .show(ui, |ui| ask(ui, view, actions));
    egui::ScrollArea::vertical()
        .id_salt("ai-conversation")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| transcript(ui, cockpit, actions));
}

/// The bottom of the conversation: the agent's questions, what was pointed
/// at, the ready requests and the request box.
fn ask(ui: &mut Ui, view: &AiView, actions: &mut AiActions) {
    let cockpit = view.cockpit;
    ui.add_space(4.0);
    let questions: Vec<&Value> = items(view.design, "questions")
        .filter(|q| q["answer"].is_null())
        .collect();
    let mut state = view.state.borrow_mut();
    for question in questions {
        let id = question["id"].as_u64().unwrap_or(0);
        card_frame(ui, crate::style::pal().accent_soft, |ui| {
            ui.label(
                RichText::new(t("The agent asks"))
                    .color(crate::style::accent_text())
                    .size(11.5),
            );
            ui.add(
                egui::Label::new(RichText::new(question["text"].as_str().unwrap_or("")).strong())
                    .wrap(),
            );
            let keep = !state.not_kept.contains(&id);
            ui.horizontal_wrapped(|ui| {
                for option in question["options"].as_array().into_iter().flatten() {
                    let option = option.as_str().unwrap_or("");
                    if ui.button(option).clicked() {
                        actions.answer = Some((id, option.to_owned(), keep));
                    }
                }
            });
            ui.horizontal(|ui| {
                let own = state.own.entry(id).or_default();
                ui.add(
                    egui::TextEdit::singleline(own)
                        .desired_width((ui.available_width() - 70.0).max(60.0))
                        .hint_text(t("Or your own answer"))
                        .id_salt(("own-answer", id)),
                );
                let written = own.trim().to_owned();
                if ui
                    .add_enabled(!written.is_empty(), egui::Button::new(t("Answer")))
                    .clicked()
                {
                    actions.answer = Some((id, written, keep));
                }
            });
            let mut kept = keep;
            if ui
                .checkbox(&mut kept, t("Keep the answer as a decision"))
                .changed()
            {
                if kept {
                    state.not_kept.remove(&id);
                } else {
                    state.not_kept.insert(id);
                }
            }
        });
    }
    drop(state);
    for (index, item) in cockpit.context.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui
                .small_button("x")
                .on_hover_text(t("Leave it out"))
                .clicked()
            {
                actions.cockpit.remove_context = Some(index);
            }
            ui.add(
                egui::Label::new(RichText::new(item).color(crate::style::pal().select)).truncate(),
            );
        });
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let ready: Vec<(String, String)> = match view.workspace {
            Some(workspace) if !workspace.ready.is_empty() => workspace.ready.clone(),
            _ => ready_requests()
                .iter()
                .map(|(name, request)| ((*name).to_owned(), (*request).to_owned()))
                .collect(),
        };
        for (name, request) in ready {
            if ui
                .add(egui::Button::new(RichText::new(&name).size(12.0)).small())
                .on_hover_text(&request)
                .clicked()
            {
                actions.ready = Some(request.clone());
            }
        }
    });
    let mut request = cockpit.request.borrow_mut();
    ui.add(
        egui::TextEdit::multiline(&mut *request)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text(t("Tell the agent what to make or change"))
            .id_salt("ai-request"),
    );
    let empty = request.trim().is_empty();
    drop(request);
    ui.horizontal(|ui| {
        if cockpit.run.is_some() {
            ui.spinner();
            ui.weak(t("working..."));
            if ui.button(t("Stop")).clicked() {
                actions.cockpit.stop = true;
            }
        } else if ui
            .add_enabled(
                !empty,
                egui::Button::new(RichText::new(t("Send")).color(crate::style::on_accent()))
                    .fill(crate::style::pal().accent),
            )
            .on_hover_text(t("A checkpoint is saved first: you can always go back"))
            .clicked()
        {
            actions.cockpit.send = true;
        }
    });
}

/// The transcript as task cards: each request with what the agent said
/// and did, how it ended, its undo and its before/after.
fn transcript(ui: &mut Ui, cockpit: &Cockpit, actions: &mut AiActions) {
    if cockpit.transcript.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(t("Start with what you want to make.")).strong());
        ui.weak(t(
            "Describe the game in a few sentences, or fill in the game card first. The agent builds it while you watch, asks you about the choices that are yours, and every task can be taken back.",
        ));
        return;
    }
    // Lines before the first request (none, usually) and each task.
    let mut starts: Vec<usize> = cockpit
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, line)| line.who == Who::Person)
        .map(|(index, _)| index)
        .collect();
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    for (number, start) in starts.iter().enumerate() {
        let end = starts
            .get(number + 1)
            .copied()
            .unwrap_or(cockpit.transcript.len());
        let lines = &cockpit.transcript[*start..end];
        let running = end == cockpit.transcript.len() && cockpit.run.is_some();
        card_frame(ui, crate::style::pal().raised, |ui| {
            let mut tools: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for line in lines {
                match line.who {
                    Who::Person => {
                        ui.label(
                            RichText::new(&line.text)
                                .strong()
                                .color(crate::style::pal().text),
                        );
                    }
                    Who::Agent => {
                        ui.add(egui::Label::new(&line.text).wrap());
                    }
                    Who::Tool => {
                        let entry = tools.entry(line.text.as_str()).or_default();
                        entry.0 += 1;
                        if line.ok != Some(true) {
                            entry.1 += 1;
                        }
                    }
                    Who::Note => {
                        let color = if line.ok == Some(false) {
                            crate::style::pal().red
                        } else {
                            crate::style::pal().go
                        };
                        ui.label(RichText::new(&line.text).color(color));
                    }
                }
            }
            if !tools.is_empty() {
                let total: usize = tools.values().map(|(n, _)| n).sum();
                let failed: usize = tools.values().map(|(_, f)| f).sum();
                let summary: Vec<String> = tools
                    .iter()
                    .map(|(tool, (n, _))| {
                        if *n > 1 {
                            format!("{tool} ×{n}")
                        } else {
                            (*tool).to_owned()
                        }
                    })
                    .collect();
                let text = if failed > 0 {
                    tf(
                        "{count} tool call(s), {failed} failed: {tools}",
                        &[
                            ("count", &total),
                            ("failed", &failed),
                            ("tools", &summary.join(", ")),
                        ],
                    )
                } else {
                    tf(
                        "{count} tool call(s): {tools}",
                        &[("count", &total), ("tools", &summary.join(", "))],
                    )
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .color(crate::style::pal().weak)
                            .size(11.5),
                    )
                    .wrap(),
                );
            }
            if running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(t("working..."));
                });
            } else if let Some(checkpoint) = cockpit.before.get(start) {
                ui.horizontal(|ui| {
                    if ui
                        .small_button(t("Undo this task"))
                        .on_hover_text(tf(
                            "Go back to checkpoint {name}, saved before the task",
                            &[("name", checkpoint)],
                        ))
                        .clicked()
                    {
                        actions.undo_task = Some(checkpoint.clone());
                    }
                    if ui
                        .small_button(t("Before / after"))
                        .on_hover_text(t("The scene before the task beside the scene now"))
                        .clicked()
                    {
                        actions.compare_task = Some(checkpoint.clone());
                    }
                });
            }
        });
        ui.add_space(4.0);
    }
}

/// The AI settings window.
pub(crate) fn settings(ui: &mut Ui, view: &AiView, actions: &mut AiActions) {
    if !view.state.borrow().settings {
        return;
    }
    let settings = &view.cockpit.settings;
    let mut open = true;
    egui::Window::new(t("AI settings"))
        .open(&mut open)
        .collapsible(false)
        .default_width(460.0)
        .show(ui.ctx(), |ui| {
            crate::style::panel_title(ui, t("Assistant"));
            ui.label(view.cockpit.agent.name());
            match view.assistant {
                Some(path) => {
                    ui.weak(tf("found: {path}", &[("path", &path)]));
                }
                None => {
                    ui.label(RichText::new(t("Claude Code was not found.")).color(crate::style::pal().amber));
                    ui.weak(t(
                        "Install it and sign in once (claude), or set WISH_CLAUDE to where it is. The engine works without it too: the manual mode does everything by hand.",
                    ));
                }
            }
            ui.add_space(6.0);
            crate::style::panel_title(ui, t("Model"));
            ui.horizontal_wrapped(|ui| {
                for (value, name, hover) in [
                    (None, t("Default"), t("The assistant's own choice")),
                    (Some("opus"), "Opus", t("The most capable: big and hard tasks")),
                    (Some("sonnet"), "Sonnet", t("Fast and capable: most tasks")),
                    (Some("haiku"), "Haiku", t("The fastest and cheapest: small tasks")),
                ] {
                    let chosen = settings.model.as_deref() == value;
                    if ui.selectable_label(chosen, name).on_hover_text(hover).clicked() && !chosen {
                        actions.model = Some(value.map(str::to_owned));
                    }
                }
            });
            ui.add_space(6.0);
            crate::style::panel_title(ui, t("Permissions"));
            ui.weak(tf(
                "Now: {mode} (AI > Permissions changes it)",
                &[("mode", &crate::app::mode_label(view.permission))],
            ));
            ui.add_space(6.0);
            crate::style::panel_title(ui, t("How it works"));
            let mut memory = settings.memory;
            if ui
                .checkbox(&mut memory, t("Remember the conversation between tasks"))
                .on_hover_text(t("The agent continues where it left off; New conversation starts over"))
                .changed()
            {
                actions.memory = Some(memory);
            }
            let mut first = settings.checkpoint_first;
            if ui
                .checkbox(&mut first, t("Save a checkpoint before every task"))
                .on_hover_text(t("Every task card can then be undone"))
                .changed()
            {
                actions.checkpoint_first = Some(first);
            }
            let mut files = settings.files;
            if ui
                .checkbox(&mut files, t("The agent may change the game code itself"))
                .on_hover_text(t("Off: it works only through the editor's tools (scenes, assets, settings)"))
                .changed()
            {
                actions.files = Some(files);
            }
            ui.add_space(6.0);
            crate::style::panel_title(ui, t("Spent"));
            ui.label(tf(
                "{dollars} in this session (on a subscription: what it would have cost)",
                &[("dollars", &format!("${:.2}", view.cockpit.cost))],
            ));
        });
    if !open {
        actions.settings = Some(false);
    }
}

/// The live view's switch (game camera or scene), over its top-left corner.
pub(crate) fn view_switch(ui: &mut Ui, area: egui::Rect, scene: bool, actions: &mut AiActions) {
    let at = egui::Rect::from_min_size(area.min + egui::vec2(8.0, 8.0), egui::vec2(220.0, 30.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
        egui::Frame::new()
            .fill(Color32::from_black_alpha(160))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::same(2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(!scene, t("Game"))
                        .on_hover_text(t("What the player sees"))
                        .clicked()
                        && scene
                    {
                        actions.scene_view = Some(false);
                    }
                    if ui
                        .selectable_label(scene, t("Scene"))
                        .on_hover_text(t("The scene through the editor's eye: click to point at things for the agent"))
                        .clicked()
                        && !scene
                    {
                        actions.scene_view = Some(true);
                    }
                });
            });
    });
}
