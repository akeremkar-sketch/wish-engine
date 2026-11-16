//! The project settings window (M8a WP8): the name, the default scene and
//! the input map, edited as a draft and saved with one `project_settings`
//! call (which checks the map and says what is wrong, with a hint).

use std::cell::RefCell;

use egui::Ui;
use serde_json::{Map, Value, json};

/// The settings being edited (read with `project_settings` when the window
/// opens).
#[derive(Clone, Debug, Default)]
pub(crate) struct Draft {
    pub name: String,
    pub scene: String,
    /// The input map's actions: name → binding (`kind` and its key lists).
    pub actions: Map<String, Value>,
    /// The new action's name, being typed.
    pub new_action: String,
    /// The new action's kind.
    pub new_kind: String,
}

impl Draft {
    /// From `project_settings` (action get)'s answer.
    pub fn from_answer(answer: &Value) -> Self {
        Self {
            name: answer["project"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            scene: answer["project"]["scene"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            actions: answer["input"]["actions"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
            new_action: String::new(),
            new_kind: "button".to_owned(),
        }
    }

    /// The whole input map, as `input.json` holds it.
    pub fn input(&self) -> Value {
        json!({ "format": 1, "actions": self.actions })
    }
}

/// The key lists of a binding kind.
fn slots(kind: &str) -> &'static [&'static str] {
    match kind {
        "axis" => &["positive", "negative"],
        "axis2" => &["up", "down", "left", "right"],
        _ => &["keys"],
    }
}

/// What the window asked for.
#[derive(Default)]
pub(crate) struct SettingsActions {
    /// Save the name and the default scene.
    pub save_project: bool,
    /// Save the input map.
    pub save_input: bool,
    pub close: bool,
}

impl SettingsActions {
    pub fn any(&self) -> bool {
        self.save_project || self.save_input || self.close
    }
}

/// The window's contents.
pub(crate) fn window(
    ui: &mut Ui,
    draft: &RefCell<Option<Draft>>,
    scenes: &[String],
    actions: &mut SettingsActions,
) {
    let mut state = draft.borrow_mut();
    let Some(draft) = state.as_mut() else {
        return;
    };
    let mut open = true;
    egui::Window::new("Project settings")

        .open(&mut open)
        .default_width(520.0)
        .default_pos([380.0, 80.0])
        .show(ui.ctx(), |ui| {
            ui.heading("Project");
            egui::Grid::new("project_grid").num_columns(2).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(260.0));
                ui.end_row();
                ui.label("Default scene");
                egui::ComboBox::from_id_salt("default_scene")
                    .selected_text(&draft.scene)
                    .show_ui(ui, |ui| {
                        for scene in scenes {
                            ui.selectable_value(&mut draft.scene, scene.clone(), scene);
                        }
                    });
                ui.end_row();
            });
            if ui.button("Save project").clicked() {
                actions.save_project = true;
            }
            ui.separator();
            ui.heading("Input map");
            ui.weak("Actions the game code reads (actions.pressed(\"jump\"), actions.axis2(\"move\")) and the keys that drive them. Several keys: separate them with commas.");
            let mut remove = None;
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("input_grid").num_columns(3).striped(true).show(ui, |ui| {
                    for (name, binding) in &mut draft.actions {
                        ui.strong(name);
                        let kind = binding["kind"].as_str().unwrap_or("button").to_owned();
                        ui.weak(&kind);
                        ui.vertical(|ui| {
                            for slot in slots(&kind) {
                                ui.horizontal(|ui| {
                                    if slots(&kind).len() > 1 {
                                        ui.weak(*slot);
                                    }
                                    let mut text = binding[*slot]
                                        .as_array()
                                        .map(|keys| {
                                            keys.iter()
                                                .filter_map(Value::as_str)
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        })
                                        .unwrap_or_default();
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut text)
                                            .desired_width(200.0)
                                            .hint_text("Space, W, Up..."),
                                    );
                                    if response.changed() {
                                        let keys: Vec<Value> = text
                                            .split(',')
                                            .map(str::trim)
                                            .filter(|k| !k.is_empty())
                                            .map(|k| json!(k))
                                            .collect();
                                        binding[*slot] = Value::Array(keys);
                                    }
                                });
                            }
                            if ui.small_button("Remove action").clicked() {
                                remove = Some(name.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            if let Some(name) = remove {
                draft.actions.remove(&name);
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.new_action)
                        .hint_text("new action")
                        .desired_width(140.0),
                );
                egui::ComboBox::from_id_salt("new_kind")
                    .selected_text(&draft.new_kind)
                    .show_ui(ui, |ui| {
                        for kind in ["button", "axis", "axis2"] {
                            ui.selectable_value(&mut draft.new_kind, kind.to_owned(), kind);
                        }
                    });
                let name = draft.new_action.trim().to_owned();
                if ui
                    .add_enabled(!name.is_empty() && !draft.actions.contains_key(&name), egui::Button::new("Add action"))
                    .clicked()
                {
                    let mut binding = Map::new();
                    binding.insert("kind".to_owned(), json!(draft.new_kind));
                    for slot in slots(&draft.new_kind) {
                        binding.insert((*slot).to_owned(), json!([]));
                    }
                    draft.actions.insert(name, Value::Object(binding));
                    draft.new_action.clear();
                }
            });
            if ui.button("Save input map").clicked() {
                actions.save_input = true;
            }
        });
    if !open {
        actions.close = true;
    }
}
