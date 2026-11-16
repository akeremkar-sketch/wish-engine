//! The inspector (M8a WP4): every component of the selection, edited with
//! widgets made from its schema, with no code per component. A component a
//! game defines in Wasm is edited the same way as the engine's own, because
//! both come out of `schema_get`.
//!
//! How a field is shown follows the JSON Schema `schema_get` gives (with the
//! engine's `x-wish-type`, `x-unit`, ranges and documentation):
//!
//! | Schema | Widget |
//! |---|---|
//! | `number` / `integer` | a drag box with the unit and the range |
//! | `boolean` | a check box |
//! | `oneOf` of constants | a drop-down list, each choice with its documentation |
//! | an array of numbers or booleans of fixed length | one box per item (`Color`: and a colour picker; `Quat`: as degrees) |
//! | `string` (`Entity`) | a drop-down of the scene's entities |
//! | `string` (`AssetRef`) | a text box and a drop-down of the project's files |
//! | `string` | a text box |
//! | `object` with properties | its fields, nested |
//! | an array of objects | its items, nested, folded |
//! | anything else | the raw JSON, editable (and a note that it is raw) |
//!
//! A change is sent when it is finished: a drag when the mouse button is let
//! go, typing on Enter or when the box loses focus, a click at once. So one
//! change is one entry of the history, however long the drag.

use std::cell::RefCell;
use std::collections::BTreeMap;

use egui::{DragValue, Ui};
use serde_json::{Map, Value, json};

/// What the inspector keeps between frames.
#[derive(Default)]
pub(crate) struct InspectorState {
    /// Values being dragged, by field key: shown, not sent yet.
    editing: BTreeMap<String, Value>,
    /// Text being typed, by field key.
    texts: BTreeMap<String, String>,
    /// What is typed in the "Add component" search box.
    add_filter: String,
}

/// One selected entity as the inspector shows it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Inspected {
    /// Its id (`instance/part` for a part of a prefab instance).
    pub id: String,
    /// Its components' values (effective: defaults filled in).
    pub components: Map<String, Value>,
    /// The prefab it is an instance of.
    pub prefab: Option<String>,
    /// Whether it is a part of a prefab instance (changes are overrides).
    pub part: bool,
}

/// What the inspector asks for.
#[derive(Default)]
pub(crate) struct InspectorActions {
    /// Set one field of one component on every selected entity that has it.
    pub set: Option<(String, String, Value)>,
    /// Add a component (with its default values).
    pub add: Option<String>,
    /// Remove a component.
    pub remove: Option<String>,
}

impl InspectorActions {
    pub fn any(&self) -> bool {
        self.set.is_some() || self.add.is_some() || self.remove.is_some()
    }
}

/// What the panel needs to know.
pub(crate) struct InspectorView<'a> {
    pub inspected: &'a [Inspected],
    /// Component schemas, by name.
    pub schemas: &'a BTreeMap<String, Value>,
    /// The scene's entity ids, for entity fields.
    pub entities: Vec<&'a str>,
    /// The project's files, for asset fields.
    pub assets: &'a [String],
    pub state: &'a RefCell<InspectorState>,
}

/// The components every inspected entity has, in the first one's order.
fn common(inspected: &[Inspected]) -> Vec<String> {
    let Some(first) = inspected.first() else {
        return Vec::new();
    };
    first
        .components
        .keys()
        .filter(|name| {
            inspected
                .iter()
                .all(|entity| entity.components.contains_key(*name))
        })
        .cloned()
        .collect()
}

/// The inspector panel.
pub(crate) fn panel(ui: &mut Ui, view: &InspectorView<'_>, actions: &mut InspectorActions) {
    ui.heading("Inspector");
    ui.separator();
    let Some(first) = view.inspected.first() else {
        ui.weak("Select an entity to edit its components.");
        return;
    };
    if view.inspected.len() == 1 {
        ui.strong(&first.id);
    } else {
        ui.strong(format!("{} entities", view.inspected.len()));
        ui.weak("Shown: the first one's values. A change applies to all of them.");
    }
    if let Some(prefab) = &first.prefab {
        ui.weak(format!("Instance of {prefab}"));
    }
    let part = view.inspected.iter().any(|entity| entity.part);
    if part {
        ui.weak("Part of a prefab instance: changes are kept as overrides of the instance.");
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for name in common(view.inspected) {
                let value = &first.components[&name];
                let schema = view.schemas.get(&name);
                let header = egui::CollapsingHeader::new(&name)
                    .id_salt(("component", &name))
                    .default_open(true)
                    .show(ui, |ui| {
                        let Some(schema) = schema else {
                            ui.weak("No schema for this component (rebuild the game code?).");
                            return;
                        };
                        if let Some((field, new)) = fields(ui, view, &name, schema, value) {
                            actions.set = Some((name.clone(), field, new));
                        }
                        if !part
                            && ui
                                .small_button("Remove component")
                                .on_hover_text(format!("Takes {name} off the selection."))
                                .clicked()
                        {
                            actions.remove = Some(name.clone());
                        }
                    });
                if let Some(doc) = schema.and_then(|s| s["description"].as_str()) {
                    header.header_response.on_hover_text(doc);
                }
            }
            if !part {
                ui.separator();
                let present = common(view.inspected);
                ui.menu_button("Add component...", |ui| {
                    let mut filter = view.state.borrow().add_filter.clone();
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut filter)
                            .hint_text("Search")
                            .desired_width(200.0),
                    );
                    search.request_focus();
                    view.state.borrow_mut().add_filter.clone_from(&filter);
                    let wanted = filter.to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for name in view.schemas.keys() {
                                if present.contains(name) || !name.to_lowercase().contains(&wanted)
                                {
                                    continue;
                                }
                                let doc = view.schemas[name]["description"]
                                    .as_str()
                                    .unwrap_or_default();
                                if ui.button(name).on_hover_text(doc).clicked() {
                                    actions.add = Some(name.clone());
                                    view.state.borrow_mut().add_filter.clear();
                                    ui.close();
                                }
                            }
                        });
                });
            }
        });
}

/// The fields of a component: a grid of labels and widgets. Returns the
/// top-level field that changed and its whole new value.
fn fields(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    component: &str,
    schema: &Value,
    value: &Value,
) -> Option<(String, Value)> {
    let mut changed = None;
    let properties = schema["properties"].as_object()?;
    egui::Grid::new(("fields", component))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (field, field_schema) in properties {
                let label = ui.label(field);
                if let Some(doc) = field_schema["description"].as_str() {
                    let unit = field_schema["x-unit"]
                        .as_str()
                        .map(|u| format!(" ({u})"))
                        .unwrap_or_default();
                    label.on_hover_text(format!("{doc}{unit}"));
                }
                let current = value.get(field).unwrap_or(&field_schema["default"]);
                let key = format!("{component}.{field}");
                if let Some(new) = widget(ui, view, &key, field_schema, current) {
                    changed = Some((field.clone(), new));
                }
                ui.end_row();
            }
        });
    changed
}

/// Sends `new` when the change is finished; keeps a drag's value shown
/// meanwhile.
fn finish(
    ui: &Ui,
    view: &InspectorView<'_>,
    key: &str,
    changed: bool,
    new: Value,
) -> Option<Value> {
    let down = ui.input(|input| input.pointer.any_down());
    let mut state = view.state.borrow_mut();
    if changed {
        if down {
            state.editing.insert(key.to_owned(), new);
            return None;
        }
        state.editing.remove(key);
        return Some(new);
    }
    if !down && let Some(pending) = state.editing.remove(key) {
        return Some(pending);
    }
    None
}

/// What a field shows now: the value being dragged, or the stored one.
fn shown(view: &InspectorView<'_>, key: &str, stored: &Value) -> Value {
    view.state
        .borrow()
        .editing
        .get(key)
        .cloned()
        .unwrap_or_else(|| stored.clone())
}

/// One field's widget, chosen by its schema.
fn widget(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    key: &str,
    schema: &Value,
    value: &Value,
) -> Option<Value> {
    let wish_type = schema["x-wish-type"].as_str().unwrap_or_default();
    if let Some(choices) = schema["oneOf"].as_array() {
        return choice(ui, view, key, choices, value);
    }
    match schema["type"].as_str() {
        Some("number" | "integer") => number(ui, view, key, schema, value),
        Some("boolean") => {
            let mut on = shown(view, key, value).as_bool().unwrap_or(false);
            let response = ui.checkbox(&mut on, "");
            finish(ui, view, key, response.changed(), json!(on))
        }
        Some("array") if wish_type == "Quat" => quaternion(ui, view, key, value),
        Some("array")
            if schema["items"]["type"] == "number" || schema["items"]["type"] == "boolean" =>
        {
            let length = schema["maxItems"].as_u64().unwrap_or(0) as usize;
            numbers(ui, view, key, schema, value, length, wish_type == "Color")
        }
        Some("array") if schema["items"]["properties"].is_object() => {
            objects(ui, view, key, schema, value)
        }
        Some("string") if wish_type == "Entity" => entity(ui, view, key, value),
        Some("string") => text(ui, view, key, value, schema["x-asset"] == true),
        Some("object") if schema["properties"].is_object() => {
            let mut changed = None;
            ui.vertical(|ui| {
                egui::Grid::new(("nested", key))
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (field, field_schema) in
                            schema["properties"].as_object().into_iter().flatten()
                        {
                            ui.label(field);
                            let current = value.get(field).unwrap_or(&field_schema["default"]);
                            if let Some(new) =
                                widget(ui, view, &format!("{key}.{field}"), field_schema, current)
                            {
                                let mut whole = value.clone();
                                whole[field] = new;
                                changed = Some(whole);
                            }
                            ui.end_row();
                        }
                    });
            });
            changed
        }
        _ => raw(ui, view, key, value),
    }
}

fn number(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    key: &str,
    schema: &Value,
    value: &Value,
) -> Option<Value> {
    let integer = schema["type"] == "integer";
    let mut number = shown(view, key, value).as_f64().unwrap_or(0.0);
    let (low, high) = (schema["minimum"].as_f64(), schema["maximum"].as_f64());
    let speed = match (integer, low, high) {
        (true, ..) => 1.0,
        (false, Some(low), Some(high)) => ((high - low) / 400.0).clamp(0.001, 0.1),
        _ => 0.02,
    };
    let mut drag = DragValue::new(&mut number).speed(speed);
    if let (Some(low), Some(high)) = (low, high) {
        drag = drag.range(low..=high);
    }
    if integer {
        drag = drag.fixed_decimals(0);
    }
    if let Some(unit) = schema["x-unit"].as_str() {
        drag = drag.suffix(format!(" {unit}"));
    }
    let response = ui.add(drag);
    let new = if integer {
        if number < 0.0 {
            json!(number.round() as i64)
        } else {
            json!(number.round() as u64)
        }
    } else {
        json!(number)
    };
    finish(ui, view, key, response.changed(), new)
}

fn numbers(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    key: &str,
    schema: &Value,
    value: &Value,
    length: usize,
    color: bool,
) -> Option<Value> {
    let current = shown(view, key, value);
    let mut items: Vec<Value> = current.as_array().cloned().unwrap_or_default();
    items.resize(length.max(items.len()), Value::Null);
    let item_schema = &schema["items"];
    let mut changed = false;
    ui.horizontal(|ui| {
        if color && length >= 3 {
            let mut rgba = [0.0f32; 4];
            for (slot, item) in rgba.iter_mut().zip(&items) {
                *slot = item.as_f64().unwrap_or(1.0) as f32;
            }
            if length == 3 {
                rgba[3] = 1.0;
            }
            let mut picked = egui::Color32::from_rgba_unmultiplied(
                (rgba[0].clamp(0.0, 1.0) * 255.0) as u8,
                (rgba[1].clamp(0.0, 1.0) * 255.0) as u8,
                (rgba[2].clamp(0.0, 1.0) * 255.0) as u8,
                (rgba[3].clamp(0.0, 1.0) * 255.0) as u8,
            );
            if ui.color_edit_button_srgba(&mut picked).changed() {
                let [r, g, b, a] = picked.to_srgba_unmultiplied();
                let parts = [r, g, b, a].map(|c| f64::from(c) / 255.0);
                for (item, part) in items.iter_mut().zip(parts) {
                    *item = json!((part * 1000.0).round() / 1000.0);
                }
                changed = true;
            }
        }
        for (index, item) in items.iter_mut().enumerate() {
            if item_schema["type"] == "boolean" {
                let mut on = item.as_bool().unwrap_or(false);
                if ui
                    .checkbox(
                        &mut on,
                        ["x", "y", "z", "w"].get(index).copied().unwrap_or(""),
                    )
                    .changed()
                {
                    *item = json!(on);
                    changed = true;
                }
            } else {
                let mut number = item.as_f64().unwrap_or(0.0);
                let mut drag = DragValue::new(&mut number).speed(0.02).max_decimals(4);
                if let Some(prefix) = ["x: ", "y: ", "z: ", "w: "]
                    .get(index)
                    .filter(|_| length <= 4)
                {
                    drag = drag.prefix(*prefix);
                }
                if ui.add(drag).changed() {
                    *item = json!(number);
                    changed = true;
                }
            }
        }
    });
    finish(ui, view, key, changed, Value::Array(items))
}

/// A rotation, edited as degrees about X, Y, Z (yaw about Y first, as
/// `edit_apply`'s `euler_deg`).
fn quaternion(ui: &mut Ui, view: &InspectorView<'_>, key: &str, value: &Value) -> Option<Value> {
    let current = shown(view, key, value);
    let part = |i: usize| current.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let rotation = glam::Quat::from_xyzw(part(0), part(1), part(2), part(3)).normalize();
    let (y, x, z) = rotation.to_euler(glam::EulerRot::YXZ);
    let mut degrees = [x, y, z].map(|r| f64::from(r.to_degrees()));
    // Round away float noise so a 45 shows as 45.
    for d in &mut degrees {
        *d = (*d * 1000.0).round() / 1000.0;
    }
    let mut changed = false;
    ui.horizontal(|ui| {
        for (degree, prefix) in degrees.iter_mut().zip(["x: ", "y: ", "z: "]) {
            if ui
                .add(
                    DragValue::new(degree)
                        .speed(0.5)
                        .suffix(" deg")
                        .prefix(prefix),
                )
                .changed()
            {
                changed = true;
            }
        }
    });
    let quat = wish_control::ops::euler_deg_to_quat(degrees);
    let new = json!([quat.x, quat.y, quat.z, quat.w]);
    finish(ui, view, key, changed, new)
}

fn choice(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    key: &str,
    choices: &[Value],
    value: &Value,
) -> Option<Value> {
    let current = shown(view, key, value);
    let selected = current.as_str().unwrap_or_default().to_owned();
    let mut picked = None;
    egui::ComboBox::from_id_salt(("choice", key))
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            for option in choices {
                let Some(name) = option["const"].as_str() else {
                    continue;
                };
                let response = ui.selectable_label(name == selected, name);
                let response = match option["description"].as_str() {
                    Some(doc) => response.on_hover_text(doc),
                    None => response,
                };
                if response.clicked() && name != selected {
                    picked = Some(json!(name));
                }
            }
        });
    match picked {
        Some(new) => finish(ui, view, key, true, new),
        None => finish(ui, view, key, false, current),
    }
}

fn entity(ui: &mut Ui, view: &InspectorView<'_>, key: &str, value: &Value) -> Option<Value> {
    let selected = value.as_str().unwrap_or("null").to_owned();
    let mut picked = None;
    egui::ComboBox::from_id_salt(("entity", key))
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            for name in std::iter::once("null").chain(view.entities.iter().copied()) {
                if ui.selectable_label(name == selected, name).clicked() && name != selected {
                    picked = Some(json!(name));
                }
            }
        });
    picked
}

fn text(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    key: &str,
    value: &Value,
    asset: bool,
) -> Option<Value> {
    let stored = value.as_str().unwrap_or_default().to_owned();
    let mut typed = view
        .state
        .borrow()
        .texts
        .get(key)
        .cloned()
        .unwrap_or_else(|| stored.clone());
    let mut picked = None;
    let response = ui
        .horizontal(|ui| {
            let response = ui.add(egui::TextEdit::singleline(&mut typed).desired_width(150.0));
            if asset {
                egui::ComboBox::from_id_salt(("asset", key))
                    .selected_text("")
                    .width(24.0)
                    .show_ui(ui, |ui| {
                        for path in view.assets {
                            if ui.selectable_label(*path == stored, path).clicked() {
                                picked = Some(json!(path));
                            }
                        }
                    });
            }
            response
        })
        .inner;
    if picked.is_some() {
        view.state.borrow_mut().texts.remove(key);
        return picked;
    }
    if response.has_focus() {
        view.state.borrow_mut().texts.insert(key.to_owned(), typed);
        return None;
    }
    let pending = view.state.borrow_mut().texts.remove(key);
    if response.lost_focus() && pending.as_deref().is_some_and(|text| text != stored) {
        return pending.map(|text| json!(text));
    }
    None
}

/// Arrays of objects: each item nested, folded (rarely edited by hand).
fn objects(
    ui: &mut Ui,
    view: &InspectorView<'_>,
    key: &str,
    schema: &Value,
    value: &Value,
) -> Option<Value> {
    let items = value.as_array().cloned().unwrap_or_default();
    let mut changed = None;
    egui::CollapsingHeader::new(format!("{} items", items.len()))
        .id_salt(("items", key))
        .default_open(false)
        .show(ui, |ui| {
            for (index, item) in items.iter().enumerate() {
                ui.label(format!("[{index}]"));
                if let Some(new) =
                    widget(ui, view, &format!("{key}[{index}]"), &schema["items"], item)
                {
                    let mut whole = items.clone();
                    whole[index] = new;
                    changed = Some(Value::Array(whole));
                }
            }
        });
    changed
}

/// A value of a kind the inspector has no widget for: its JSON, editable.
fn raw(ui: &mut Ui, view: &InspectorView<'_>, key: &str, value: &Value) -> Option<Value> {
    let stored = value.to_string();
    let mut typed = view
        .state
        .borrow()
        .texts
        .get(key)
        .cloned()
        .unwrap_or_else(|| stored.clone());
    let response = ui
        .add(egui::TextEdit::singleline(&mut typed).desired_width(150.0))
        .on_hover_text("Raw JSON: this kind of value has no widget yet.");
    if response.has_focus() {
        view.state.borrow_mut().texts.insert(key.to_owned(), typed);
        return None;
    }
    let pending = view.state.borrow_mut().texts.remove(key);
    if response.lost_focus() {
        return pending
            .and_then(|text| serde_json::from_str(&text).ok())
            .filter(|parsed: &Value| *parsed != *value);
    }
    None
}

/// Whether the inspector has a widget for this schema (not the raw JSON
/// fallback); for the test that no engine type falls back.
pub fn has_widget(schema: &Value) -> bool {
    if schema["oneOf"].is_array() {
        return true;
    }
    match schema["type"].as_str() {
        Some("number" | "integer" | "boolean" | "string") => true,
        Some("array") => {
            schema["x-wish-type"] == "Quat"
                || schema["items"]["type"] == "number"
                || schema["items"]["type"] == "boolean"
                || (schema["items"]["properties"].is_object()
                    && schema["items"]["properties"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .all(|(_, s)| has_widget(s)))
        }
        Some("object") => schema["properties"]
            .as_object()
            .is_some_and(|all| all.values().all(has_widget)),
        _ => false,
    }
}
