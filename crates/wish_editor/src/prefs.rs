//! The person's editor preferences (`editor.json`): the language, the
//! projects opened lately and whether an AI assistant helps. One small JSON
//! object; each writer changes its own keys and keeps the others.

use std::path::Path;

use serde_json::{Map, Value};

/// How many recent projects are kept.
pub(crate) const MAX_RECENT: usize = 8;

/// The preferences in `path` (empty if the file is missing or broken).
pub(crate) fn read(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// Reads the preferences, lets `change` change them and writes them back.
pub(crate) fn update(
    path: &Path,
    change: impl FnOnce(&mut Map<String, Value>),
) -> std::io::Result<()> {
    // A file that is there but cannot be read is kept aside, not lost.
    if let Ok(text) = std::fs::read_to_string(path)
        && serde_json::from_str::<Value>(&text).map_or(true, |value| !value.is_object())
    {
        std::fs::copy(path, path.with_extension("json.broken"))?;
    }
    let mut prefs = read(path);
    change(&mut prefs);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    // Written beside and moved over: a crash never leaves half a file.
    let writing = path.with_extension("json.writing");
    std::fs::write(&writing, format!("{:#}\n", Value::Object(prefs)))?;
    std::fs::rename(&writing, path)
}

/// The recent projects, newest first.
pub(crate) fn recent(prefs: &Map<String, Value>) -> Vec<String> {
    prefs
        .get("recent")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Puts `root` first among the recent projects.
pub(crate) fn remember(prefs: &mut Map<String, Value>, root: &Path) {
    let root = std::path::absolute(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .display()
        .to_string();
    let mut recent = recent(prefs);
    recent.retain(|known| *known != root);
    recent.insert(0, root);
    recent.truncate(MAX_RECENT);
    prefs.insert(
        "recent".to_owned(),
        Value::Array(recent.into_iter().map(Value::String).collect()),
    );
}
