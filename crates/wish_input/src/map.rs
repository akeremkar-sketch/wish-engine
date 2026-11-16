//! The action map: `input.json`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_types::{MAX_ACTIONS, action_id};

/// The project file that holds the action map.
pub const INPUT_FILE: &str = "input.json";

/// How an action reads the keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Binding {
    /// Down (1) or up (0).
    Button {
        /// Any of these keys held makes it active.
        #[serde(default)]
        keys: Vec<String>,
    },
    /// One axis, from -1 to 1.
    Axis {
        /// Keys that push toward +1.
        #[serde(default)]
        positive: Vec<String>,
        /// Keys that push toward -1.
        #[serde(default)]
        negative: Vec<String>,
    },
    /// Two axes (x right, y up); diagonals are normalized.
    Axis2 {
        /// Keys that push y toward +1.
        #[serde(default)]
        up: Vec<String>,
        /// Keys that push y toward -1.
        #[serde(default)]
        down: Vec<String>,
        /// Keys that push x toward -1.
        #[serde(default)]
        left: Vec<String>,
        /// Keys that push x toward +1.
        #[serde(default)]
        right: Vec<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    actions: BTreeMap<String, Binding>,
}

/// Every key name an action can use.
pub fn key_names() -> Vec<String> {
    let mut names: Vec<String> = ('A'..='Z').map(String::from).collect();
    names.extend((0..=9).map(|n| n.to_string()));
    names.extend((1..=12).map(|n| format!("F{n}")));
    names.extend(
        [
            "Space",
            "Enter",
            "Escape",
            "Tab",
            "Shift",
            "Ctrl",
            "Alt",
            "Up",
            "Down",
            "Left",
            "Right",
            "MouseLeft",
            "MouseRight",
            "MouseMiddle",
        ]
        .map(str::to_owned),
    );
    names
}

fn invalid(file: &str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("invalid_input_file", message, hint).at_file(file)
}

const SHAPE: &str = "The file looks like { \"format\": 1, \"actions\": { \"jump\": { \"kind\": \"button\", \"keys\": [\"Space\"] }, \"move\": { \"kind\": \"axis2\", \"up\": [\"W\"], \"down\": [\"S\"], \"left\": [\"A\"], \"right\": [\"D\"] } } }; kinds are button, axis (positive/negative) and axis2 (up/down/left/right).";

/// The project's actions and the keys that drive them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputMap {
    actions: BTreeMap<String, Binding>,
}

impl InputMap {
    /// Reads and checks `input.json`'s text; `file` names it in diagnostics.
    pub fn parse(text: &str, file: &str) -> Result<Self> {
        let parsed: File = serde_json::from_str(text).map_err(|error| {
            let mut problem = invalid(file, format!("{file} is not valid: {error}."), SHAPE);
            problem = problem.at_line(
                u32::try_from(error.line()).unwrap_or(0),
                u32::try_from(error.column()).ok(),
            );
            problem
        })?;
        if parsed.format != 1 {
            return Err(invalid(
                file,
                format!(
                    "{file} has format {}; the engine reads format 1.",
                    parsed.format
                ),
                "Set \"format\": 1.",
            ));
        }
        if parsed.actions.len() > MAX_ACTIONS {
            return Err(invalid(
                file,
                format!(
                    "{file} has {} actions; a project can have {MAX_ACTIONS}.",
                    parsed.actions.len()
                ),
                "Merge actions that mean the same, or remove the ones the game does not use.",
            ));
        }
        let all = key_names();
        let known: Vec<&str> = all.iter().map(String::as_str).collect();
        let mut ids: BTreeMap<u32, &str> = BTreeMap::new();
        for (name, binding) in &parsed.actions {
            let valid_name = !name.is_empty()
                && name.chars().count() <= 40
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid_name {
                return Err(invalid(
                    file,
                    format!("'{name}' is not an action name."),
                    "Action names are 1 to 40 letters, digits or underscores, such as \"jump\" or \"move_left\".",
                ));
            }
            if let Some(other) = ids.insert(action_id(name), name) {
                return Err(invalid(
                    file,
                    format!("The actions '{other}' and '{name}' have the same id."),
                    "Rename one of them (this is a one-in-four-billion coincidence).",
                ));
            }
            for key in keys_of(binding) {
                if !known.contains(&key.as_str()) {
                    return Err(invalid(
                        file,
                        format!("Action '{name}' uses '{key}', which is not a key name."),
                        &unknown_name_hint(key, &known),
                    )
                    .with_candidates(known.iter().copied()));
                }
            }
        }
        Ok(Self {
            actions: parsed.actions,
        })
    }

    /// The map in canonical text (sorted keys, one trailing newline).
    pub fn to_canonical(&self) -> Result<String> {
        let file = File {
            format: 1,
            actions: self.actions.clone(),
        };
        let mut text = serde_json::to_string_pretty(&file).map_err(|error| {
            Diagnostic::error(
                "internal_error",
                format!("The input map could not be written: {error}."),
                "This is a bug in the engine. Please report it.",
            )
        })?;
        text.push('\n');
        Ok(text)
    }

    /// The map a new project starts with: move and jump.
    pub fn starter() -> Self {
        let keys = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        Self {
            actions: BTreeMap::from([
                (
                    "jump".to_owned(),
                    Binding::Button {
                        keys: keys(&["Space"]),
                    },
                ),
                (
                    "move".to_owned(),
                    Binding::Axis2 {
                        up: keys(&["W", "Up"]),
                        down: keys(&["S", "Down"]),
                        left: keys(&["A", "Left"]),
                        right: keys(&["D", "Right"]),
                    },
                ),
            ]),
        }
    }

    /// The action names, in the order of their slots.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.actions.keys().map(String::as_str)
    }

    /// Whether the map has no actions.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// How an action is bound.
    pub fn binding(&self, name: &str) -> Option<&Binding> {
        self.actions.get(name)
    }

    /// The value of every action when exactly the keys in `down` are held:
    /// `[x, y]` for each (a button or an axis uses `x` only).
    pub fn evaluate(&self, down: &BTreeSet<String>) -> BTreeMap<String, [f32; 2]> {
        let any = |keys: &[String]| keys.iter().any(|key| down.contains(key));
        let sign = |positive: bool, negative: bool| match (positive, negative) {
            (true, false) => 1.0,
            (false, true) => -1.0,
            _ => 0.0,
        };
        self.actions
            .iter()
            .map(|(name, binding)| {
                let value = match binding {
                    Binding::Button { keys } => [f32::from(u8::from(any(keys))), 0.0],
                    Binding::Axis { positive, negative } => {
                        [sign(any(positive), any(negative)), 0.0]
                    }
                    Binding::Axis2 {
                        up,
                        down: down_keys,
                        left,
                        right,
                    } => {
                        let x = sign(any(right), any(left));
                        let y = sign(any(up), any(down_keys));
                        if x != 0.0 && y != 0.0 {
                            let diagonal = std::f32::consts::FRAC_1_SQRT_2;
                            [x * diagonal, y * diagonal]
                        } else {
                            [x, y]
                        }
                    }
                };
                (name.clone(), value)
            })
            .collect()
    }
}

fn keys_of(binding: &Binding) -> Vec<&String> {
    match binding {
        Binding::Button { keys } => keys.iter().collect(),
        Binding::Axis { positive, negative } => positive.iter().chain(negative).collect(),
        Binding::Axis2 {
            up,
            down,
            left,
            right,
        } => up.iter().chain(down).chain(left).chain(right).collect(),
    }
}
