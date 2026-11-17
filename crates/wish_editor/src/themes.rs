//! Themes from packages (M10a WP2): the engine's own (`packages/wish.theme.*`),
//! the person's and the project's. A theme is named by its package and
//! file (`wish.theme.midnight/midnight.json`); the person's choice is kept
//! in the preferences.

use std::path::{Path, PathBuf};

use wish_control::packages::{self, Scope};

use crate::style::Theme;

/// The engine's default theme.
pub(crate) const DEFAULT_KEY: &str = "wish.theme.atelier/atelier.json";

/// A theme a package brings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThemeEntry {
    /// `<package id>/<file>`.
    pub key: String,
    /// The theme's name (its package's when the file cannot be read).
    pub name: String,
    /// The file.
    pub path: PathBuf,
}

fn from_folder(folder: &Path, found: &mut Vec<ThemeEntry>) {
    let Ok(manifest) = packages::read_manifest(folder) else {
        return;
    };
    for file in manifest.contents.get("themes").into_iter().flatten() {
        let key = format!("{}/{file}", manifest.id);
        if found.iter().any(|entry| entry.key == key) {
            continue;
        }
        let path = folder.join(file);
        let name = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| crate::style::parse_theme(&text).ok())
            .map_or_else(|| manifest.name.clone(), |(name, _)| name);
        found.push(ThemeEntry { key, name, path });
    }
}

/// Every theme the editor can use: the project's packages first, then the
/// person's, then the engine's (an earlier one of the same key wins).
pub(crate) fn available(project: Option<&Path>) -> Vec<ThemeEntry> {
    let mut found = Vec::new();
    if let Some(root) = project {
        for package in packages::installed(Scope::Project, root) {
            from_folder(&package.folder, &mut found);
        }
    }
    for package in packages::installed(Scope::User, project.unwrap_or(Path::new("."))) {
        from_folder(&package.folder, &mut found);
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
            from_folder(&folder, &mut found);
        }
    }
    found.sort_by(|a, b| {
        (a.key != DEFAULT_KEY, a.name.to_lowercase())
            .cmp(&(b.key != DEFAULT_KEY, b.name.to_lowercase()))
    });
    found
}

/// Reads the theme `key` from what is `available`.
pub(crate) fn load(available: &[ThemeEntry], key: &str) -> Result<Theme, String> {
    let entry = available
        .iter()
        .find(|entry| entry.key == key)
        .ok_or_else(|| format!("there is no theme {key}"))?;
    let text = std::fs::read_to_string(&entry.path).map_err(|error| error.to_string())?;
    crate::style::parse_theme(&text)
        .map(|(_, theme)| theme)
        .map_err(|(message, hint)| format!("{message}. {hint}"))
}

/// Adds the languages packages bring (the project's, the person's and the
/// engine's) to the interface's languages; a language file is `{ "format":
/// 1, "code": "sv", "name": "Svenska", "texts": { "English text": "..." } }`.
/// Returns how many were added.
pub(crate) fn load_languages(project: Option<&Path>) -> usize {
    let mut folders: Vec<PathBuf> = Vec::new();
    if let Some(root) = project {
        folders.extend(
            packages::installed(Scope::Project, root)
                .into_iter()
                .map(|package| package.folder),
        );
    }
    folders.extend(
        packages::installed(Scope::User, project.unwrap_or(Path::new(".")))
            .into_iter()
            .map(|package| package.folder),
    );
    if let Some(engine) = packages::engine_packages_folder() {
        let mut engine_folders: Vec<PathBuf> = std::fs::read_dir(&engine)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        engine_folders.sort();
        folders.extend(engine_folders);
    }
    let mut added = 0;
    for folder in folders {
        let Ok(manifest) = packages::read_manifest(&folder) else {
            continue;
        };
        for file in manifest.contents.get("languages").into_iter().flatten() {
            let Ok(text) = std::fs::read_to_string(folder.join(file)) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if value["format"] != 1 {
                continue;
            }
            let texts = value["texts"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(english, translated)| {
                    Some((english.clone(), translated.as_str()?.to_owned()))
                })
                .collect();
            if crate::i18n::register(
                value["code"].as_str().unwrap_or(""),
                value["name"].as_str().unwrap_or(""),
                texts,
            ) {
                added += 1;
            }
        }
    }
    added
}
