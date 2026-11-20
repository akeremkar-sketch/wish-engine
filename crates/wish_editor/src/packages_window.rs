//! The Packages window (M10a): the project's and the person's packages,
//! installing one from a folder, removing one, writing the lock again. Every
//! button is a `packages` call (the same as an agent's), so the history and
//! the activity feed show who did what. The package index (M10b) is
//! searched here and on the Hub's Packages tab with the same section.

use std::cell::{Cell, RefCell};

use egui::{RichText, Ui};
use serde_json::Value;

use crate::i18n::{t, tf};

/// The package index as a form: its folder, the words searched and the
/// last answer.
#[derive(Default)]
pub(crate) struct IndexForm {
    /// The index's folder (empty: `WISH_PACKAGE_INDEX`).
    pub folder: RefCell<String>,
    /// The words to search for.
    pub query: RefCell<String>,
    /// The last `search` answer's packages (`None`: not searched yet).
    pub results: Option<Vec<Value>>,
    /// Why the last search did not work.
    pub problem: Option<String>,
}

impl IndexForm {
    /// A form whose folder is the `package_index` preference, else empty.
    pub fn from_prefs(prefs: &serde_json::Map<String, Value>) -> Self {
        Self {
            folder: RefCell::new(
                prefs
                    .get("package_index")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            ),
            ..Self::default()
        }
    }

    /// The `index` argument of a `packages` call (absent when empty).
    pub fn index_arg(&self) -> Option<String> {
        Some(self.folder.borrow().trim().to_owned()).filter(|folder| !folder.is_empty())
    }
}

/// What the index section asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndexAsk {
    Search,
    /// Add this package to this scope.
    Add(String, &'static str),
    /// Update this package in this scope.
    Update(String, &'static str),
}

/// Searching the package index and adding from it, for `scopes` (the
/// project's and the person's in the editor, the person's on the Hub).
pub(crate) fn index_section(
    ui: &mut Ui,
    form: &IndexForm,
    scopes: &[&'static str],
    asked: &mut Option<IndexAsk>,
) {
    crate::style::panel_title(ui, t("From the package index"));
    ui.add(
        egui::TextEdit::singleline(&mut *form.folder.borrow_mut())
            .desired_width(f32::INFINITY)
            .hint_text(t(
                "The index folder (a folder of packages; empty: WISH_PACKAGE_INDEX)",
            )),
    );
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut *form.query.borrow_mut())
                .desired_width((ui.available_width() - 90.0).max(120.0))
                .hint_text(t("Words to search for (empty: everything)")),
        );
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(t("Search")).clicked() || entered {
            *asked = Some(IndexAsk::Search);
        }
    });
    if let Some(problem) = &form.problem {
        ui.label(RichText::new(problem).color(crate::style::pal().amber));
    }
    let Some(results) = &form.results else {
        return;
    };
    if results.is_empty() {
        ui.weak(t("No package in the index matches."));
    }
    for package in results {
        let id = package["id"].as_str().unwrap_or("?");
        ui.horizontal(|ui| {
            ui.label(RichText::new(package["name"].as_str().unwrap_or(id)).strong());
            ui.weak(format!(
                "{} · {} · {}",
                package["latest"].as_str().unwrap_or("-"),
                package["license"].as_str().unwrap_or("?"),
                id
            ));
        });
        if let Some(text) = package["description"].as_str().filter(|d| !d.is_empty()) {
            ui.add(egui::Label::new(RichText::new(text).color(crate::style::pal().weak)).wrap());
        }
        ui.horizontal(|ui| {
            let fits = package["latest"].is_string();
            if !fits {
                ui.label(
                    RichText::new(t("No version works with this engine"))
                        .color(crate::style::pal().amber),
                );
            }
            for &scope in scopes {
                let had = package["installed"][scope].as_str();
                match had {
                    None => {
                        let label = if scope == "project" {
                            t("Add to the project")
                        } else {
                            t("Add for me")
                        };
                        if ui.add_enabled(fits, egui::Button::new(label)).clicked() {
                            *asked = Some(IndexAsk::Add(id.to_owned(), scope));
                        }
                    }
                    Some(version) => {
                        let place = if scope == "project" {
                            t("in the project")
                        } else {
                            t("yours")
                        };
                        ui.weak(format!("{version} {place}"));
                        if package["update"][scope] == true
                            && ui
                                .button(t("Update"))
                                .on_hover_text(t(
                                    "To its newest compatible version; the old one goes to the trash",
                                ))
                                .clicked()
                        {
                            *asked = Some(IndexAsk::Update(id.to_owned(), scope));
                        }
                    }
                }
            }
        });
        ui.add_space(4.0);
    }
}

/// What the window shows and what is being written in it.
#[derive(Default)]
pub(crate) struct PackagesState {
    pub open: bool,
    /// The package index.
    pub index: IndexForm,
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
    /// What the index section asked for.
    pub index: Option<IndexAsk>,
}

impl PackagesActions {
    pub fn any(&self) -> bool {
        self.close
            || self.install.is_some()
            || self.remove.is_some()
            || self.lock
            || self.index.is_some()
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
                ui.label(RichText::new(t("locked")).color(crate::style::pal().go))
                    .on_hover_text(t("Its files are the ones wish.lock records"));
            } else if package["locked"] == false {
                ui.label(RichText::new(t("not locked")).color(crate::style::pal().amber))
                    .on_hover_text(t("Its files differ from wish.lock: write the lock again if the change is yours"));
            }
        });
        ui.weak(format!("{id} · {}", kinds(package)));
        if let Some(problem) = package["problem"].as_str() {
            ui.label(RichText::new(problem).color(crate::style::pal().amber));
        }
        if let Some(text) = package["description"].as_str().filter(|d| !d.is_empty()) {
            ui.add(egui::Label::new(RichText::new(text).color(crate::style::pal().weak)).wrap());
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
                        .color(crate::style::pal().amber)
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
            ui.separator();
            index_section(ui, &state.index, &["project", "user"], &mut actions.index);
            let count = project.len() + user.len();
            ui.weak(tf("{count} package(s) installed", &[("count", &count)]));
        });
    if !open {
        actions.close = true;
    }
}

/// The status line for an index `add` or `update` answer (the editor's
/// window and the Hub say the same).
pub(crate) fn index_status(answer: &Value, id: &str) -> String {
    if let Some(change) = answer["updated"].as_array().and_then(|u| u.first()) {
        return tf(
            "{id} updated to {version}",
            &[
                ("id", &change["id"].as_str().unwrap_or(id)),
                ("version", &change["to"].as_str().unwrap_or("?")),
            ],
        );
    }
    if let Some(failed) = answer["failed"].as_array().and_then(|f| f.first()) {
        return format!(
            "{} {}",
            failed["message"].as_str().unwrap_or(""),
            failed["hint"].as_str().unwrap_or("")
        )
        .trim()
        .to_owned();
    }
    if answer.get("updated").is_some() {
        return tf("{id} is up to date", &[("id", &id)]);
    }
    let package = &answer["package"];
    tf(
        "installed {name} {version}",
        &[
            ("name", &package["name"].as_str().unwrap_or(id)),
            ("version", &package["version"].as_str().unwrap_or("?")),
        ],
    )
}
