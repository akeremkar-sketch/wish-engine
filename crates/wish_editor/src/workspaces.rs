//! Workspaces from packages (M10a WP3): what the AI mode shows for a kind of
//! game. A workspace file brings the game card's fields (with names and
//! examples), ready requests and words for the agent; its texts are plain
//! or one per language (`{ "en": "...", "tr": "..." }`). The default
//! workspace is the engine's own card (in the code, translated with the
//! interface).

use std::path::{Path, PathBuf};

use serde_json::Value;
use wish_control::packages::{self, Scope};

/// A field of a workspace's game card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceField {
    pub key: String,
    /// Its name in the interface's language.
    pub label: String,
    /// Its name in English (for the design file and the agent).
    pub label_en: String,
    /// An example, in the interface's language.
    pub hint: String,
    pub list: bool,
    /// More than one line.
    pub tall: bool,
}

/// A workspace a package brings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceEntry {
    /// `<package id>/<file>`.
    pub key: String,
    pub name: String,
    pub description: String,
    pub card: Vec<WorkspaceField>,
    /// Ready requests: name and request, in the interface's language.
    pub ready: Vec<(String, String)>,
    /// Words for the agent with every task.
    pub instructions: String,
    pub path: PathBuf,
}

/// A text in the language `code`: plain, or a map of languages (English,
/// else the first, when the language is not there).
fn text(value: &Value, code: &str) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(map) => map
            .get(code)
            .or_else(|| map.get("en"))
            .or_else(|| map.values().next())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    }
}

/// Reads a workspace file (`None` when it is not one).
pub(crate) fn read(key: &str, path: &Path, code: &str) -> Option<WorkspaceEntry> {
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    if value["format"] != 1 {
        return None;
    }
    let card: Vec<WorkspaceField> = value["card"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|field| {
            let key = field["key"].as_str()?.to_owned();
            let list = field["list"] == true;
            Some(WorkspaceField {
                label: text(&field["label"], code),
                label_en: text(&field["label"], "en"),
                hint: text(&field["hint"], code),
                tall: list || field["lines"].as_u64().is_some_and(|lines| lines > 1),
                list,
                key,
            })
        })
        .filter(|field| !field.label_en.trim().is_empty())
        .take(20)
        .collect();
    if card.is_empty() {
        return None;
    }
    let ready = value["ready"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| (text(&item["name"], code), text(&item["request"], code)))
        .filter(|(name, request)| !name.trim().is_empty() && !request.trim().is_empty())
        .take(12)
        .collect();
    Some(WorkspaceEntry {
        key: key.to_owned(),
        name: text(&value["name"], code),
        description: text(&value["description"], code),
        card,
        ready,
        instructions: text(&value["agent"]["instructions"], code),
        path: path.to_path_buf(),
    })
}

fn from_folder(folder: &Path, code: &str, found: &mut Vec<WorkspaceEntry>) {
    let Ok(manifest) = packages::read_manifest(folder) else {
        return;
    };
    for file in manifest.contents.get("workspaces").into_iter().flatten() {
        let key = format!("{}/{file}", manifest.id);
        if found.iter().any(|entry| entry.key == key) {
            continue;
        }
        if let Some(entry) = read(&key, &folder.join(file), code) {
            found.push(entry);
        }
    }
}

/// Every workspace there is: the project's packages first, then the
/// person's, then the engine's.
pub(crate) fn available(project: &Path, code: &str) -> Vec<WorkspaceEntry> {
    let mut found = Vec::new();
    for package in packages::installed(Scope::Project, project) {
        from_folder(&package.folder, code, &mut found);
    }
    for package in packages::installed(Scope::User, project) {
        from_folder(&package.folder, code, &mut found);
    }
    if let Some(engine) = packages::engine_packages_folder() {
        let mut folders: Vec<PathBuf> = std::fs::read_dir(&engine)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        folders.sort();
        for folder in folders {
            from_folder(&folder, code, &mut found);
        }
    }
    found.sort_by_key(|entry| entry.name.to_lowercase());
    found
}
