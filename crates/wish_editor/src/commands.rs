//! Commands (M10a WP4): every editor action has an id (`game.play`,
//! `mode.switch`), a name and a default shortcut. The command palette
//! (Ctrl+K) finds and runs them by name; the person remaps shortcuts in the
//! preferences (`"keys": { "mode.switch": "Ctrl+J" }`); packages and agents
//! can name them.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};

use crate::i18n::t;

/// A command.
pub(crate) struct Command {
    /// Its id, which never changes.
    pub id: &'static str,
    /// Its name, in the interface's language.
    pub name: fn() -> &'static str,
    /// Its default shortcuts, comma-separated (`""`: none).
    pub keys: &'static str,
}

/// Every command, in the order the palette lists them.
pub(crate) const COMMANDS: &[Command] = &[
    Command {
        id: "app.palette",
        name: || t("Command palette"),
        keys: "Ctrl+K",
    },
    Command {
        id: "game.play",
        name: || t("Play or stop the game"),
        keys: "Ctrl+P",
    },
    Command {
        id: "editor.undo",
        name: || t("Undo"),
        keys: "Ctrl+Z",
    },
    Command {
        id: "editor.redo",
        name: || t("Redo"),
        keys: "Ctrl+Y, Ctrl+Shift+Z",
    },
    Command {
        id: "edit.duplicate",
        name: || t("Duplicate"),
        keys: "Ctrl+D",
    },
    Command {
        id: "scene.new",
        name: || t("New scene"),
        keys: "Ctrl+N",
    },
    Command {
        id: "create.empty",
        name: || t("Create an empty entity"),
        keys: "Ctrl+Shift+N",
    },
    Command {
        id: "create.cube",
        name: || t("Create a cube"),
        keys: "",
    },
    Command {
        id: "create.sphere",
        name: || t("Create a sphere"),
        keys: "",
    },
    Command {
        id: "create.plane",
        name: || t("Create a plane"),
        keys: "",
    },
    Command {
        id: "create.camera",
        name: || t("Create a camera"),
        keys: "",
    },
    Command {
        id: "create.directional_light",
        name: || t("Create a directional light"),
        keys: "",
    },
    Command {
        id: "create.point_light",
        name: || t("Create a point light"),
        keys: "",
    },
    Command {
        id: "create.spot_light",
        name: || t("Create a spot light"),
        keys: "",
    },
    Command {
        id: "create.sprite",
        name: || t("Create a sprite"),
        keys: "",
    },
    Command {
        id: "create.particles",
        name: || t("Create particles"),
        keys: "",
    },
    Command {
        id: "mode.switch",
        name: || t("Switch between manual and AI mode"),
        keys: "Ctrl+M",
    },
    Command {
        id: "mode.manual",
        name: || t("Manual"),
        keys: "",
    },
    Command {
        id: "mode.ai",
        name: || t("With AI"),
        keys: "",
    },
    Command {
        id: "ai.next_step",
        name: || t("Do the next step"),
        keys: "",
    },
    Command {
        id: "ai.settings",
        name: || t("AI settings"),
        keys: "",
    },
    Command {
        id: "agent.panel",
        name: || t("Show or hide the agent panel"),
        keys: "",
    },
    Command {
        id: "view.game_view",
        name: || t("Show or hide the game view"),
        keys: "",
    },
    Command {
        id: "game.network",
        name: || t("Show or hide the network panel"),
        keys: "",
    },
    Command {
        id: "view.reset_camera",
        name: || t("Reset camera"),
        keys: "",
    },
    Command {
        id: "view.maximize",
        name: || t("Maximize or restore the view"),
        keys: "Shift+Space",
    },
    Command {
        id: "view.frame",
        name: || t("Frame the selection"),
        keys: "F",
    },
    Command {
        id: "project.check",
        name: || t("Check the project"),
        keys: "",
    },
    Command {
        id: "project.build_code",
        name: || t("Build code"),
        keys: "Ctrl+B",
    },
    Command {
        id: "project.run_tests",
        name: || t("Run tests"),
        keys: "",
    },
    Command {
        id: "project.save_checkpoint",
        name: || t("Save checkpoint"),
        keys: "",
    },
    Command {
        id: "project.save_picture",
        name: || t("Save picture"),
        keys: "",
    },
    Command {
        id: "project.export",
        name: || t("Export game"),
        keys: "",
    },
    Command {
        id: "project.settings",
        name: || t("Settings..."),
        keys: "",
    },
    Command {
        id: "project.packages",
        name: || t("Packages..."),
        keys: "",
    },
    Command {
        id: "project.connect_agent",
        name: || t("Connect an outside agent..."),
        keys: "",
    },
    Command {
        id: "app.quit",
        name: || t("Quit"),
        keys: "",
    },
];

/// Reads a shortcut such as `Ctrl+Shift+Z` (`Ctrl` is the platform's
/// command key; `Alt`, `Shift`; the key by egui's name).
pub(crate) fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => modifiers |= Modifiers::COMMAND,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = Some(Key::from_name(part)?);
            }
        }
    }
    Some(KeyboardShortcut::new(modifiers, key?))
}

/// A shortcut as people read it (`Ctrl+Shift+Z`).
pub(crate) fn shortcut_text(shortcut: &KeyboardShortcut) -> String {
    let mut parts = Vec::new();
    if shortcut.modifiers.command || shortcut.modifiers.ctrl {
        parts.push("Ctrl".to_owned());
    }
    if shortcut.modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if shortcut.modifiers.shift {
        parts.push("Shift".to_owned());
    }
    parts.push(shortcut.logical_key.name().to_owned());
    parts.join("+")
}

/// Each command's shortcuts: its defaults, or what the person set
/// (`overrides`, by id; `""` takes a command's shortcut away). Longer
/// shortcuts come first, so Ctrl+Shift+Z is seen before Ctrl+Z.
pub(crate) fn bindings(
    overrides: &BTreeMap<String, String>,
) -> Vec<(&'static str, KeyboardShortcut)> {
    let mut found = Vec::new();
    for command in COMMANDS {
        for text in keys_of(command, overrides)
            .split(',')
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            if let Some(shortcut) = parse_shortcut(text) {
                found.push((command.id, shortcut));
            }
        }
    }
    let weight = |s: &KeyboardShortcut| {
        u8::from(s.modifiers.shift) + u8::from(s.modifiers.alt) + u8::from(s.modifiers.command)
    };
    found.sort_by_key(|(_, shortcut)| std::cmp::Reverse(weight(shortcut)));
    found
}

/// The shortcuts of `command` as the person has them (their override, or
/// the default).
pub(crate) fn keys_of<'a>(
    command: &'a Command,
    overrides: &'a BTreeMap<String, String>,
) -> &'a str {
    overrides
        .get(command.id)
        .map_or(command.keys, String::as_str)
}

/// The first shortcut of the command `id` as the person has it, as people
/// read it (for a menu item); none when it has none.
pub(crate) fn first_key(overrides: &BTreeMap<String, String>, id: &str) -> Option<String> {
    let command = COMMANDS.iter().find(|command| command.id == id)?;
    keys_of(command, overrides)
        .split(',')
        .map(str::trim)
        .find_map(parse_shortcut)
        .map(|shortcut| shortcut_text(&shortcut))
}

/// The `keys` preference: the person's shortcuts by command id.
pub(crate) fn overrides_from(
    prefs: &serde_json::Map<String, serde_json::Value>,
) -> BTreeMap<String, String> {
    prefs
        .get("keys")
        .and_then(serde_json::Value::as_object)
        .map(|keys| {
            keys.iter()
                .filter_map(|(id, keys)| Some((id.clone(), keys.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// The commands whose name has every word of `filter` (any case), in
/// their order.
pub(crate) fn matching(filter: &str) -> Vec<&'static Command> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    COMMANDS
        .iter()
        .filter(|command| {
            let name = (command.name)().to_lowercase();
            words
                .iter()
                .all(|word| name.contains(word.as_str()) || command.id.contains(word.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_read_and_written_back() {
        for text in ["Ctrl+K", "Ctrl+Shift+Z", "Alt+F4", "F5"] {
            let shortcut = parse_shortcut(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(shortcut_text(&shortcut), text);
        }
        assert!(parse_shortcut("Ctrl+Nope").is_none());
        assert!(parse_shortcut("Ctrl+A+B").is_none());
    }

    #[test]
    fn every_command_has_a_unique_id_and_valid_defaults() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        for command in COMMANDS {
            for text in command
                .keys
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                assert!(parse_shortcut(text).is_some(), "{}: {text}", command.id);
            }
        }
        // The person's choice replaces the default; "" takes it away.
        let mut overrides = BTreeMap::new();
        overrides.insert("mode.switch".to_owned(), "Ctrl+J".to_owned());
        overrides.insert("game.play".to_owned(), String::new());
        let bound = bindings(&overrides);
        assert!(
            bound
                .iter()
                .any(|(id, s)| *id == "mode.switch" && shortcut_text(s) == "Ctrl+J")
        );
        assert!(!bound.iter().any(|(id, _)| *id == "game.play"));
        assert_eq!(bound[0].0, "editor.redo", "Ctrl+Shift+Z is seen first");
    }
}
