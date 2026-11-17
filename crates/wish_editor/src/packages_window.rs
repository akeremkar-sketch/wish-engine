//! The Packages window (M10a): the project's and the person's packages,
//! installing one from a folder, removing one, writing the lock again. Every
//! button is a `packages` call (the same as an agent's), so the history and
//! the activity feed show who did what.

use std::cell::{Cell, RefCell};

use egui::{RichText, Ui};
use serde_json::Value;

use crate::i18n::{t, tf};
use crate::style::{AMBER, GO, WEAK};

/// What the window shows and what is being written in it.
#[derive(Default)]
pub(crate) struct PackagesState {
    pub open: bool,
    /// The last `packages list` answer.
    pub listing: Value,
    /// The folder to install from.
    pub path: RefCell<String>,
    /// Replace an installed version of the same package.
    pub replace: Cell<bool>,
}

/// What the window asked for.
#[derive(Default)]
pub(crate) struct PackagesActions {
    pub close: bool,
    /// Install from this folder into this scope.
    pub install: Option<(String, &'static str)>,
    /// Remove this package from this scope.
    pub remove: Option<(String, &'static str)>,
    pub lock: bool,
}

impl PackagesActions {
    pub fn any(&self) -> bool {
        self.close || self.install.is_some() || self.remove.is_some() || self.lock
    }
}

fn kinds(package: &Value) -> String {
    package["contents"]
        .as_object()
        .map(|contents| {
            contents
                .iter()
                .map(|(kind, items)| format!("{kind} {}", items.as_array().map_or(0, Vec::len)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn list(ui: &mut Ui, packages: &[Value], scope: &'static str, actions: &mut PackagesActions) {
    if packages.is_empty() {
        ui.weak(t("None yet."));
    }
    for package in packages {
        let id = package["id"].as_str().unwrap_or("?");
        ui.horizontal(|ui| {
            if ui
                .small_button(t("Remove"))
                .on_hover_text(t("Moves it to the trash; nothing is deleted"))
                .clicked()
            {
                actions.remove = Some((id.to_owned(), scope));
            }
            ui.label(RichText::new(package["name"].as_str().unwrap_or(id)).strong());
            ui.weak(format!(
                "{} · {}",
                package["version"].as_str().unwrap_or("?"),
                package["license"].as_str().unwrap_or("?")
            ));
            if package["locked"] == true {
                ui.label(RichText::new(t("locked")).color(GO))
                    .on_hover_text(t("Its files are the ones wish.lock records"));
            } else if package["locked"] == false {
                ui.label(RichText::new(t("not locked")).color(AMBER))
                    .on_hover_text(t("Its files differ from wish.lock: write the lock again if the change is yours"));
            }
        });
        ui.weak(format!("{id} · {}", kinds(package)));
        if let Some(problem) = package["problem"].as_str() {
            ui.label(RichText::new(problem).color(AMBER));
        }
        if let Some(text) = package["description"].as_str().filter(|d| !d.is_empty()) {
            ui.add(egui::Label::new(RichText::new(text).color(WEAK)).wrap());
        }
        ui.add_space(4.0);
    }
}

/// The window.
pub(crate) fn window(ui: &mut Ui, state: &PackagesState, actions: &mut PackagesActions) {
    if !state.open {
        return;
    }
    let mut open = true;
    egui::Window::new(t("Packages"))
        .open(&mut open)
        .default_width(560.0)
        .show(ui.ctx(), |ui| {
            ui.weak(t(
                "Packages bring themes, workspaces, languages, recipes and templates. The project's travel with it (packages/, wish.lock); yours are for every project.",
            ));
            ui.add_space(6.0);
            crate::style::panel_title(ui, t("This project"));
            let project = state.listing["project"].as_array().cloned().unwrap_or_default();
            list(ui, &project, "project", actions);
            if !project.is_empty() && ui.small_button(t("Write the lock again")).clicked() {
                actions.lock = true;
            }
            ui.add_space(6.0);
            crate::style::panel_title(ui, t("Yours"));
            let user = state.listing["user"].as_array().cloned().unwrap_or_default();
            list(ui, &user, "user", actions);
            for problem in state.listing["diagnostics"].as_array().into_iter().flatten() {
                ui.label(
                    RichText::new(problem["message"].as_str().unwrap_or(""))
                        .color(AMBER)
                        .size(12.0),
                );
            }
            ui.separator();
            crate::style::panel_title(ui, t("Install"));
            let mut path = state.path.borrow_mut();
            ui.add(
                egui::TextEdit::singleline(&mut *path)
                    .desired_width(f32::INFINITY)
                    .hint_text(t("The package's folder (with wish-package.json)")),
            );
            let mut replace = state.replace.get();
            ui.checkbox(&mut replace, t("Replace an installed version"));
            state.replace.set(replace);
            let given = path.trim().to_owned();
            drop(path);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!given.is_empty(), egui::Button::new(t("Install into the project")))
                    .on_hover_text(t("It travels with the project and is locked in wish.lock"))
                    .clicked()
                {
                    actions.install = Some((given.clone(), "project"));
                }
                if ui
                    .add_enabled(!given.is_empty(), egui::Button::new(t("Install for me")))
                    .on_hover_text(t("For every project you open, on this computer"))
                    .clicked()
                {
                    actions.install = Some((given.clone(), "user"));
                }
            });
            let count = project.len() + user.len();
            ui.weak(tf("{count} package(s) installed", &[("count", &count)]));
        });
    if !open {
        actions.close = true;
    }
}
