//! The Hub (M10b): the welcome screen's tabs. Projects is the welcome page
//! itself; the others are the person's packages, the look (themes and
//! languages), the AI assistant's settings, the engine versions side by
//! side, things to learn and the engine's own settings. What a tab changes is the person's (their preferences and
//! their packages), never a project's.

use egui::{CornerRadius, RichText, Stroke, Ui};
use serde_json::{Map, Value};

use crate::i18n::{t, tf};
use crate::style::pal;

/// A tab of the Hub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Projects,
    Packages,
    Look,
    Ai,
    Engines,
    Learn,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Projects,
        Tab::Packages,
        Tab::Look,
        Tab::Ai,
        Tab::Engines,
        Tab::Learn,
        Tab::Settings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Projects => t("Projects"),
            Tab::Packages => t("Packages"),
            Tab::Look => t("Look"),
            Tab::Ai => t("AI"),
            Tab::Engines => t("Engine versions"),
            Tab::Learn => t("Learn"),
            Tab::Settings => t("Settings"),
        }
    }

    fn icon(self) -> crate::style::Icon {
        use crate::style::Icon;
        match self {
            Tab::Projects => Icon::Folder,
            Tab::Packages => Icon::Prefab,
            Tab::Look => Icon::Image,
            Tab::Ai => Icon::Wish,
            Tab::Engines => Icon::Cube,
            Tab::Learn => Icon::Text,
            Tab::Settings => Icon::Target,
        }
    }
}

/// What a Hub page asked for (done after the layout).
#[derive(Default)]
pub(crate) struct HubAsked {
    pub tab: Option<Tab>,
    pub theme: Option<String>,
    pub language: Option<&'static str>,
    /// Install the package in this folder for the person.
    pub install: Option<String>,
    /// Remove the person's package of this id.
    pub remove: Option<String>,
    /// Change the agent preferences (`agent` key).
    pub agent: Option<Map<String, Value>>,
    /// Open this folder or address with the system.
    pub open: Option<String>,
    /// Start from this template (its id) on the Projects tab.
    pub template: Option<String>,
    /// What the package index section asked for.
    pub index: Option<crate::packages_window::IndexAsk>,
    /// Add the engine in this folder.
    pub add_engine: Option<String>,
    /// Remove (forget, or move to the trash) the engine in this folder.
    pub remove_engine: Option<std::path::PathBuf>,
}

/// What the Hub pages show.
pub(crate) struct HubView<'a> {
    pub theme_key: &'a str,
    pub language: &'static str,
    pub themes: &'a [crate::themes::ThemeEntry],
    /// The person's packages and the engine's (from `packages::listing`).
    pub user_packages: &'a [Value],
    pub engine_packages: &'a [Value],
    pub agent: &'a Map<String, Value>,
    pub claude: Option<&'a std::path::Path>,
    pub templates: &'a [wish_control::scaffold::Template],
    pub projects_folder: &'a str,
    /// The package index form.
    pub index: &'a crate::packages_window::IndexForm,
    /// The engines on this computer, and what is wrong with the list.
    pub engines: &'a [wish_control::engines::Engine],
    pub engine_warnings: &'a [String],
}

/// The tabs, down the left side.
pub(crate) fn tabs(ui: &mut Ui, current: Tab, asked: &mut HubAsked) {
    ui.add_space(12.0);
    for tab in Tab::ALL {
        let chosen = tab == current;
        let response =
            crate::style::tool_button(ui, tab.icon(), tab.name(), true, chosen, true, None);
        if response.clicked() && !chosen {
            asked.tab = Some(tab);
        }
        ui.add_space(2.0);
    }
}

fn title(ui: &mut Ui, text: &str, about: &str) {
    ui.add_space(18.0);
    ui.label(RichText::new(text).size(24.0).color(pal().text));
    ui.add(egui::Label::new(RichText::new(about).color(pal().weak)).wrap());
    ui.add_space(14.0);
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(pal().raised)
        .stroke(Stroke::new(1.0, pal().line))
        .corner_radius(CornerRadius::same(pal().radius_window))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    ui.add_space(10.0);
}

/// The person's packages, and the engine's.
pub(crate) fn packages(ui: &mut Ui, view: &HubView, path: &mut String, asked: &mut HubAsked) {
    title(
        ui,
        t("Packages"),
        t(
            "Themes, workspaces, languages, recipes and templates for every project you open. A project's own packages are in the editor (Project menu, Packages...).",
        ),
    );
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Yours"));
        if view.user_packages.is_empty() {
            ui.weak(t("None yet."));
        }
        for package in view.user_packages {
            let id = package["id"].as_str().unwrap_or("?");
            ui.horizontal(|ui| {
                if ui
                    .small_button(t("Remove"))
                    .on_hover_text(t("Moves it to the trash; nothing is deleted"))
                    .clicked()
                {
                    asked.remove = Some(id.to_owned());
                }
                ui.label(RichText::new(package["name"].as_str().unwrap_or(id)).strong());
                ui.weak(format!(
                    "{} · {} · {}",
                    package["version"].as_str().unwrap_or("?"),
                    package["license"].as_str().unwrap_or("?"),
                    id
                ));
            });
        }
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(path)
                .desired_width(f32::INFINITY)
                .hint_text(t("The package's folder (with wish-package.json)")),
        );
        let given = path.trim().to_owned();
        if ui
            .add_enabled(!given.is_empty(), egui::Button::new(t("Install for me")))
            .clicked()
        {
            asked.install = Some(given);
        }
    });
    card(ui, |ui| {
        crate::packages_window::index_section(ui, view.index, &["user"], &mut asked.index);
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Built in"));
        for package in view.engine_packages {
            ui.horizontal(|ui| {
                ui.label(RichText::new(package["name"].as_str().unwrap_or("?")).strong());
                ui.weak(package["description"].as_str().unwrap_or(""));
            });
        }
    });
}

/// The engine versions on this computer, side by side.
pub(crate) fn engines(ui: &mut Ui, view: &HubView, path: &mut String, asked: &mut HubAsked) {
    use wish_control::engines::Source;
    title(
        ui,
        t("Engine versions"),
        t(
            "Engine versions live side by side. A project opens with the version it was made with; if that one is not here, with the newest compatible one.",
        ),
    );
    card(ui, |ui| {
        crate::style::panel_title(ui, t("On this computer"));
        for warning in view.engine_warnings {
            ui.add(egui::Label::new(RichText::new(warning).color(pal().amber)).wrap());
        }
        for engine in view.engines {
            ui.horizontal(|ui| {
                if engine.source != Source::Running
                    && ui
                        .small_button(t("Remove"))
                        .on_hover_text(t(
                            "An added engine is only forgotten (its files stay); an installed one goes to the trash",
                        ))
                        .clicked()
                {
                    asked.remove_engine = Some(engine.folder.clone());
                }
                ui.label(
                    RichText::new(tf(
                        "Wish Engine {version}",
                        &[("version", &engine.version)],
                    ))
                    .strong(),
                );
                let source = match engine.source {
                    Source::Running => t("this one"),
                    Source::Installed => t("installed"),
                    Source::Added => t("added"),
                };
                ui.weak(format!("{source} · {}", engine.folder.display()));
            });
        }
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(path)
                .desired_width(f32::INFINITY)
                .hint_text(t("An engine's folder (with wen and wish-engine.json)")),
        );
        let given = path.trim().to_owned();
        if ui
            .add_enabled(!given.is_empty(), egui::Button::new(t("Add an engine")))
            .on_hover_text(t("It stays where it is; nothing is copied"))
            .clicked()
        {
            asked.add_engine = Some(given);
        }
    });
}

/// A small picture of a theme: its panel, a raised card, text, the accent
/// and the chosen colour.
fn swatch(ui: &mut Ui, theme: &crate::style::Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(132.0, 64.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, theme.panel);
    let inner = rect.shrink(8.0);
    painter.rect_filled(
        egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), 22.0)),
        3.0,
        theme.raised,
    );
    painter.rect_filled(
        egui::Rect::from_min_size(inner.min + egui::vec2(6.0, 7.0), egui::vec2(48.0, 8.0)),
        2.0,
        theme.text,
    );
    painter.rect_filled(
        egui::Rect::from_min_size(inner.min + egui::vec2(0.0, 30.0), egui::vec2(40.0, 18.0)),
        3.0,
        theme.accent,
    );
    painter.rect_stroke(
        egui::Rect::from_min_size(inner.min + egui::vec2(46.0, 30.0), egui::vec2(40.0, 18.0)),
        3.0,
        Stroke::new(1.5, theme.select),
        egui::StrokeKind::Inside,
    );
}

/// Themes and languages.
pub(crate) fn look(ui: &mut Ui, view: &HubView, asked: &mut HubAsked) {
    title(
        ui,
        t("Look"),
        t(
            "How the editor looks and speaks, for you. Themes and languages can also come in packages.",
        ),
    );
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Theme"));
        ui.horizontal_wrapped(|ui| {
            for entry in view.themes {
                let Ok(theme) = crate::themes::load(view.themes, &entry.key) else {
                    continue;
                };
                let chosen = entry.key == view.theme_key;
                ui.vertical(|ui| {
                    swatch(ui, &theme);
                    let label = if chosen {
                        RichText::new(&entry.name).color(pal().select)
                    } else {
                        RichText::new(&entry.name)
                    };
                    if ui.selectable_label(chosen, label).clicked() && !chosen {
                        asked.theme = Some(entry.key.clone());
                    }
                });
                ui.add_space(8.0);
            }
        });
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Language"));
        ui.horizontal_wrapped(|ui| {
            for language in crate::i18n::all_languages() {
                let chosen = language.code == view.language;
                if ui.selectable_label(chosen, language.name).clicked() && !chosen {
                    asked.language = Some(language.code);
                }
            }
        });
    });
}

/// The AI assistant: whether it is there, and how it works by default.
pub(crate) fn ai(ui: &mut Ui, view: &HubView, asked: &mut HubAsked) {
    title(
        ui,
        t("AI"),
        t(
            "The assistant that works with you in the AI mode. The engine needs none: everything can be done by hand.",
        ),
    );
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Assistant"));
        match view.claude {
            Some(path) => {
                ui.label(RichText::new("Claude Code").strong());
                ui.weak(tf("found: {path}", &[("path", &path.display())]));
            }
            None => {
                ui.label(RichText::new(t("Claude Code was not found.")).color(pal().amber));
                ui.weak(t(
                    "Install it and sign in once (claude), or set WISH_CLAUDE to where it is. The engine works without it too: the manual mode does everything by hand.",
                ));
            }
        }
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Model"));
        let model = view.agent.get("model").and_then(Value::as_str);
        ui.horizontal_wrapped(|ui| {
            for (value, name) in [
                (None, t("Default")),
                (Some("opus"), "Opus"),
                (Some("sonnet"), "Sonnet"),
                (Some("haiku"), "Haiku"),
            ] {
                let chosen = model == value;
                if ui.selectable_label(chosen, name).clicked() && !chosen {
                    let mut agent = view.agent.clone();
                    agent.insert(
                        "model".to_owned(),
                        value.map_or(Value::Null, |m| Value::String(m.to_owned())),
                    );
                    asked.agent = Some(agent);
                }
            }
        });
        ui.add_space(6.0);
        crate::style::panel_title(ui, t("How it works"));
        for (key, name) in [
            ("memory", t("Remember the conversation between tasks")),
            ("checkpoint_first", t("Save a checkpoint before every task")),
            ("files", t("The agent may change the game code itself")),
        ] {
            let mut on = view.agent.get(key).and_then(Value::as_bool).unwrap_or(true);
            if ui.checkbox(&mut on, name).changed() {
                let mut agent = view.agent.clone();
                agent.insert(key.to_owned(), Value::Bool(on));
                asked.agent = Some(agent);
            }
        }
    });
}

/// Where to learn: the guide, the shortcuts, the templates.
pub(crate) fn learn(ui: &mut Ui, view: &HubView, asked: &mut HubAsked) {
    title(
        ui,
        t("Learn"),
        t("The user guide, the shortcuts and the example games to start from."),
    );
    card(ui, |ui| {
        crate::style::panel_title(ui, t("User guide"));
        ui.weak(t(
            "Everything the editor does, step by step, in English and Turkish.",
        ));
        let page = if view.language == "tr" {
            "https://github.com/akeremkar-sketch/wish-engine/blob/main/docs/user/tr/guide.md"
        } else {
            "https://github.com/akeremkar-sketch/wish-engine/blob/main/docs/user/en/guide.md"
        };
        if ui.button(t("Open the guide")).on_hover_text(page).clicked() {
            asked.open = Some(page.to_owned());
        }
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Shortcuts"));
        egui::Grid::new("hub-keys")
            .num_columns(2)
            .spacing([24.0, 4.0])
            .show(ui, |ui| {
                for command in crate::commands::COMMANDS {
                    if command.keys.is_empty() {
                        continue;
                    }
                    ui.label((command.name)());
                    ui.weak(command.keys);
                    ui.end_row();
                }
            });
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Example games"));
        for template in view.templates {
            ui.horizontal(|ui| {
                if ui.small_button(t("Start from it")).clicked() {
                    asked.template = Some(template.id.clone());
                }
                ui.label(&template.name);
            });
        }
    });
}

/// The engine's own settings: folders, version, privacy.
pub(crate) fn settings(ui: &mut Ui, view: &HubView, asked: &mut HubAsked) {
    title(
        ui,
        t("Settings"),
        t("Where the engine keeps things, and what it is."),
    );
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Folders"));
        ui.label(tf(
            "New projects: {folder}",
            &[("folder", &view.projects_folder)],
        ));
        if let Some(config) = wish_control::crash::config_folder() {
            ui.horizontal(|ui| {
                ui.label(tf(
                    "Your settings and packages: {folder}",
                    &[("folder", &config.display())],
                ));
                if ui.small_button(t("Open")).clicked() {
                    asked.open = Some(config.display().to_string());
                }
            });
        }
        if let Some(crashes) = wish_control::crash::folder() {
            ui.horizontal(|ui| {
                ui.label(tf(
                    "Crash reports: {folder}",
                    &[("folder", &crashes.display())],
                ));
                if ui.small_button(t("Open")).clicked() {
                    asked.open = Some(crashes.display().to_string());
                }
            });
        }
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Engine"));
        ui.label(tf(
            "Wish Engine {version}",
            &[("version", &env!("CARGO_PKG_VERSION"))],
        ));
        if let Ok(exe) = std::env::current_exe() {
            ui.weak(exe.display().to_string());
        }
    });
    card(ui, |ui| {
        crate::style::panel_title(ui, t("Privacy"));
        ui.label(t(
            "Nothing is ever sent anywhere: no telemetry, no account. Crash reports stay on this computer until you send one yourself.",
        ));
    });
}
