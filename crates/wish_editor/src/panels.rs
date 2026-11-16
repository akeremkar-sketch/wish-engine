//! The bottom panel's other tabs (M8a WP8): the console (what the engine and
//! the game logged, from `logs_get`), the problems (`check`) and how fast
//! the editor and the game run.

use std::collections::VecDeque;

use crate::i18n::{t, tf};
use egui::{Color32, RichText, Sense, Ui, vec2};
use serde_json::Value;

/// Which tab of the bottom panel is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Assets,
    Console,
    Problems,
    Performance,
    History,
}

/// How many log entries are kept for the console.
pub(crate) const MAX_LOG: usize = 2000;
/// How many frames the performance graph shows.
pub(crate) const HISTORY: usize = 240;

/// Frame times, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Timing {
    /// Laying out the interface and painting it.
    pub interface: f32,
    /// Drawing the scene's picture (0 when it did not change).
    pub scene: f32,
    /// Running game ticks this frame (0 when not playing).
    pub game: f32,
    /// Ticks run this frame.
    pub ticks: u32,
}

/// What the tabs ask for.
#[derive(Default)]
pub(crate) struct PanelActions {
    pub tab: Option<Tab>,
    pub clear_console: bool,
    pub check: bool,
    /// Select the entity a problem is about: its file, its id.
    pub select: Option<(String, String)>,
    pub undo: bool,
    pub redo: bool,
    /// Take back one earlier change on its own (its revision).
    pub revert: Option<u64>,
}

impl PanelActions {
    pub fn any(&self) -> bool {
        self.tab.is_some()
            || self.clear_console
            || self.check
            || self.select.is_some()
            || self.undo
            || self.redo
            || self.revert.is_some()
    }
}

fn severity_color(severity: &str) -> Color32 {
    match severity {
        "error" => Color32::from_rgb(235, 100, 90),
        "warning" => Color32::from_rgb(230, 190, 80),
        _ => Color32::from_rgb(150, 160, 170),
    }
}

/// The tab buttons.
pub(crate) fn tabs(ui: &mut Ui, current: Tab, problems: usize, actions: &mut PanelActions) {
    ui.horizontal(|ui| {
        for (tab, name) in [
            (Tab::Assets, t("Assets").to_owned()),
            (Tab::Console, t("Console").to_owned()),
            (
                Tab::Problems,
                tf("Problems ({problems})", &[("problems", &problems)]),
            ),
            (Tab::Performance, t("Performance").to_owned()),
            (Tab::History, t("History").to_owned()),
        ] {
            if ui.selectable_label(current == tab, name).clicked() && current != tab {
                actions.tab = Some(tab);
            }
        }
    });
}

/// The console: log entries, newest last.
pub(crate) fn console(ui: &mut Ui, entries: &VecDeque<Value>, actions: &mut PanelActions) {
    ui.horizontal(|ui| {
        ui.weak(tf("{count} entries", &[("count", &entries.len())]));
        if ui.small_button(t("Clear")).clicked() {
            actions.clear_console = true;
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("console")
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical(|ui| {
                for entry in entries {
                    let severity = entry["severity"].as_str().unwrap_or("info");
                    let tick = entry["tick"]
                        .as_u64()
                        .map(|t| format!("t{t} "))
                        .unwrap_or_default();
                    let kind = entry["kind"].as_str().unwrap_or_default();
                    let line = format!(
                        "{tick}[{kind}] {}: {}",
                        entry["code"].as_str().unwrap_or_default(),
                        entry["message"].as_str().unwrap_or_default()
                    );
                    let response = ui.label(
                        RichText::new(line)
                            .color(severity_color(severity))
                            .monospace(),
                    );
                    if let Some(hint) = entry["hint"].as_str() {
                        response.on_hover_text(hint);
                    }
                }
            });
        });
}

/// The problems `check` found; a click on one about an entity selects it.
pub(crate) fn problems(ui: &mut Ui, found: &[Value], actions: &mut PanelActions) {
    ui.horizontal(|ui| {
        if ui.small_button(t("Check again")).clicked() {
            actions.check = true;
        }
        if found.is_empty() {
            ui.weak(t("No problems."));
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("problems")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical(|ui| {
                for problem in found {
                    let severity = problem["severity"].as_str().unwrap_or("error");
                    let file = problem["location"]["file"].as_str().unwrap_or_default();
                    let path = problem["location"]["path"].as_str().unwrap_or_default();
                    let text = format!(
                        "{}: {} {}",
                        problem["code"].as_str().unwrap_or_default(),
                        problem["message"].as_str().unwrap_or_default(),
                        if file.is_empty() {
                            String::new()
                        } else {
                            format!("({file}{path})")
                        }
                    );
                    let response = ui
                        .add(
                            egui::Label::new(RichText::new(text).color(severity_color(severity)))
                                .sense(Sense::click()),
                        )
                        .on_hover_text(problem["hint"].as_str().unwrap_or_default());
                    if response.clicked()
                        && let Some(id) = path
                            .strip_prefix("/entities/")
                            .map(|rest| rest.split('/').next().unwrap_or(rest))
                    {
                        actions.select = Some((file.to_owned(), id.to_owned()));
                    }
                    if let Some(hint) = problem["hint"].as_str() {
                        ui.weak(format!("    {hint}"));
                    }
                }
            });
        });
}

/// Frame times as bars, newest on the right, and the averages.
pub(crate) fn performance(ui: &mut Ui, history: &VecDeque<Timing>, playing: bool) {
    let count = history.len().max(1) as f32;
    let average = |f: fn(&Timing) -> f32| history.iter().map(f).sum::<f32>() / count;
    ui.horizontal(|ui| {
        let ms = |value: f32, digits: usize| format!("{value:.digits$}");
        ui.label(tf(
            "interface {ms} ms",
            &[("ms", &ms(average(|t| t.interface), 1))],
        ));
        ui.separator();
        ui.label(tf(
            "scene picture {ms} ms",
            &[("ms", &ms(average(|t| t.scene), 1))],
        ));
        if playing {
            ui.separator();
            ui.label(tf("game {ms} ms", &[("ms", &ms(average(|t| t.game), 2))]));
            ui.separator();
            let ticks: u32 = history.iter().map(|t| t.ticks).sum();
            ui.label(tf(
                "{ticks} ticks in {frames} frames",
                &[("ticks", &ticks), ("frames", &history.len())],
            ));
        }
    });
    ui.weak(t("Bars: one per frame (interface grey, scene blue, game green); the line is 16.7 ms (60 frames a second)."));
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), ui.available_height().max(40.0)),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let scale = rect.height() / 33.3;
    let line = rect.max.y - 16.7 * scale;
    painter.hline(rect.x_range(), line, (1.0, Color32::from_gray(90)));
    let width = rect.width() / HISTORY as f32;
    for (index, timing) in history.iter().enumerate() {
        let x = rect.min.x + index as f32 * width;
        let mut y = rect.max.y;
        for (value, color) in [
            (timing.interface, Color32::from_gray(150)),
            (timing.scene, Color32::from_rgb(90, 140, 220)),
            (timing.game, Color32::from_rgb(110, 190, 110)),
        ] {
            let height = (value * scale).min(y - rect.min.y);
            if height > 0.0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x, y - height),
                        egui::pos2(x + width.max(1.0), y),
                    ),
                    0.0,
                    color,
                );
                y -= height;
            }
        }
    }
}

/// The scene's history (`edit_history list`): who changed what, newest
/// first, with undo and redo.
pub(crate) fn history(
    ui: &mut Ui,
    entries: &[Value],
    can: (bool, bool),
    actions: &mut PanelActions,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can.0, egui::Button::new(t("Undo")).small())
            .clicked()
        {
            actions.undo = true;
        }
        if ui
            .add_enabled(can.1, egui::Button::new(t("Redo")).small())
            .clicked()
        {
            actions.redo = true;
        }
        ui.weak(t(
            "Every change to the scene, by anyone (you, an agent, a text editor): newest first.",
        ));
    });
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if entries.is_empty() {
                ui.weak(t("No changes yet."));
            }
            for entry in entries {
                let touched: Vec<&str> = entry["entities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let action = match entry["action"].as_str() {
                    Some("undo") => t("undo"),
                    Some("redo") => t("redo"),
                    Some("revert") => t("taken back"),
                    _ => t("edit"),
                };
                let revision = entry["revision"].as_u64().unwrap_or(0);
                ui.horizontal(|ui| {
                    // A change, on its own: taken back if nothing later
                    // touched what it changed.
                    if entry["revertible"] == true
                        && entry["action"] == "edit"
                        && ui
                            .add(egui::Button::new(t("Undo this")).small())
                            .on_hover_text(t(
                                "Takes back only this change; later changes stay (refused if one touched the same things)",
                            ))
                            .clicked()
                    {
                        actions.revert = Some(revision);
                    }
                    let text = tf(
                        "r{revision}  {actor}  {action}  {entities} ({changes} change(s))",
                        &[
                            ("revision", &revision),
                            ("actor", &entry["actor"].as_str().unwrap_or("?")),
                            ("action", &action),
                            ("entities", &touched.join(", ")),
                            ("changes", &entry["changes"].as_u64().unwrap_or(0)),
                        ],
                    );
                    if entry["reverted"] == true {
                        ui.label(RichText::new(text).strikethrough().weak())
                            .on_hover_text(t("Taken back"));
                    } else {
                        ui.label(text);
                    }
                    if let Some(reason) = entry["reason"].as_str() {
                        ui.weak(format!("— {reason}"));
                    }
                });
            }
        });
}
