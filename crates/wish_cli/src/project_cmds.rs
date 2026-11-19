//! Commands that work on a whole project folder: `check`, `fmt`, `migrate`
//! and `watch`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wish_app::{App, AppConfig};
use wish_asset::{HotScene, load_project};
use wish_core::{Diagnostic, Result};

/// What a command prints and whether it succeeded.
pub struct Outcome {
    pub text: String,
    pub success: bool,
}

fn registry_app(example: &str) -> Result<App> {
    let mut app = App::new(AppConfig::default());
    crate::types::register_scene_types(example, app.world_mut())?;
    Ok(app)
}

fn selected(paths: &[String], file: &str) -> bool {
    paths.is_empty() || paths.iter().any(|path| path == file)
}

/// Turns file arguments into project-relative document paths (`\` becomes
/// `/`, a leading `./` goes) and refuses any that names no document: a typo
/// must never make a check pass by checking nothing.
fn resolve(paths: &[String], root: &Path) -> Result<Vec<String>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let documents: Vec<String> = wish_asset::index::documents(root)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    paths
        .iter()
        .map(|path| {
            let mut path = path.replace('\\', "/");
            while let Some(rest) = path.strip_prefix("./") {
                path = rest.to_owned();
            }
            if documents.contains(&path) {
                return Ok(path);
            }
            let hint = match wish_core::suggest::closest_match(
                &path,
                documents.iter().map(String::as_str),
            ) {
                Some(closest) => format!("Did you mean '{closest}'?"),
                None => "Give a scene, prefab, meta or material file path relative to the project folder (--root).".to_owned(),
            };
            Err(Diagnostic::error(
                "file_not_found",
                format!("'{path}' is not a scene, prefab, meta or material file of the project."),
                hint,
            ))
        })
        .collect()
}

fn render(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| format!("{diagnostic}\n"))
        .collect()
}

/// `wen fmt`: rewrites documents in canonical form (or, with `check_only`,
/// lists the ones that are not).
pub fn format(
    paths: &[String],
    example: Option<&str>,
    root: &Path,
    check_only: bool,
    as_json: bool,
) -> Result<Outcome> {
    let paths = &resolve(paths, root)?;
    // The types: an example's, or the project's own (the engine's, its
    // example's and its built game code's), as the tools see them.
    let project = match example {
        Some(example) => load_project(root, registry_app(example)?.world().registry())?,
        None => {
            let workspace = wish_control::Workspace::open(wish_control::WorkspaceOptions {
                root: root.to_path_buf(),
                example: None,
            })?;
            load_project(root, workspace.registry())?
        }
    };
    let mut changed: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    // Files of an older format are tidied in their own format, never
    // migrated here (that would change what a formatter may: `wen migrate`
    // does it, on purpose); they are named so that nobody is surprised.
    let mut older: Vec<&str> = Vec::new();
    for file in project
        .files
        .iter()
        .filter(|file| selected(paths, &file.path))
    {
        if file.older_format.is_some() {
            older.push(&file.path);
        }
        match &file.canonical {
            Some(canonical) if *canonical != file.source => {
                if !check_only {
                    fs::write(root.join(&file.path), canonical).map_err(|error| {
                        Diagnostic::error(
                            "file_unwritable",
                            format!("Could not write '{}': {error}.", file.path),
                            "Check that the file is not read-only or open elsewhere.",
                        )
                    })?;
                }
                changed.push(&file.path);
            }
            Some(_) => {}
            None => skipped.push(&file.path),
        }
    }
    // Files that could not even be read are skipped too, never passed over.
    for diagnostic in &project.diagnostics {
        if diagnostic.code == "file_unreadable"
            && let Some(file) = diagnostic.location.file.as_deref()
            && selected(paths, file)
        {
            skipped.push(file);
        }
    }
    skipped.sort_unstable();
    let problems: Vec<&Diagnostic> = project
        .diagnostics
        .iter()
        .filter(|d| {
            d.is_error()
                && d.location
                    .file
                    .as_deref()
                    .is_some_and(|f| skipped.contains(&f))
        })
        .collect();
    let success = skipped.is_empty() && !(check_only && !changed.is_empty());
    let text = if as_json {
        let mut answer = json!({
            "ok": success,
            "check": check_only,
            "changed": changed,
            "skipped": skipped,
            "diagnostics": problems,
        });
        if !older.is_empty() {
            answer["older_format"] = json!(older);
        }
        answer.to_string()
    } else {
        let mut text = render(&problems.iter().map(|d| (*d).clone()).collect::<Vec<_>>());
        let verb = if check_only {
            "Not canonical"
        } else {
            "Formatted"
        };
        for path in &changed {
            text.push_str(&format!("{verb}: {path}\n"));
        }
        for path in &skipped {
            text.push_str(&format!("Skipped (has errors): {path}\n"));
        }
        for path in &older {
            text.push_str(&format!(
                "Older format (kept; `wen migrate` upgrades it to format {}): {path}\n",
                wish_scene::FORMAT
            ));
        }
        text.push_str(&format!(
            "{} file(s) {}.",
            changed.len(),
            if check_only {
                "need formatting"
            } else {
                "formatted"
            }
        ));
        text
    };
    Ok(Outcome { text, success })
}

/// `wen migrate`: upgrades scene and prefab files of older format versions
/// (`wish_control::migrate`: a file keeps every value it relied on whose
/// default changed) and checks the rest. Format 1 becomes format 2 (the
/// look lock of 1.0); files already at format 2 are left as they are.
/// (Setting `engine` in `wish.json` after a migration is a follow-up of
/// ADR-027, rule 3, and is not done here.)
pub fn migrate(paths: &[String], root: &Path, as_json: bool) -> Result<Outcome> {
    let paths = resolve(paths, root)?;
    let current = wish_scene::FORMAT;
    let defaults = wish_control::migrate::engine_defaults()?;
    let report = wish_control::migrate::migrate_project(
        root,
        &paths,
        wish_scene::migrate::STEPS,
        current,
        &defaults,
        true,
    )?;
    // Warnings (a prefab that could not be read) do not fail the command.
    let success = !report.diagnostics.iter().any(Diagnostic::is_error);
    let migrated: Vec<&str> = report
        .migrated
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let text = if as_json {
        json!({
            "ok": success,
            "current_format": current,
            "up_to_date": report.up_to_date,
            "migrated": migrated,
            "diagnostics": report.diagnostics,
        })
        .to_string()
    } else {
        let mut text = render(&report.diagnostics);
        for (name, written) in &report.migrated {
            text.push_str(&format!(
                "Migrated: {name} (format {current}; {} value(s) written that the file relied on)\n",
                written.len()
            ));
        }
        if report.migrated.is_empty() {
            text.push_str(&format!(
                "{} file(s) already at format {current}; nothing needed migrating.",
                report.up_to_date
            ));
        } else {
            text.push_str(&format!(
                "{} file(s) migrated to format {current}, {} already at it.",
                report.migrated.len(),
                report.up_to_date
            ));
        }
        text
    };
    Ok(Outcome { text, success })
}

fn event(as_json: bool, name: &str, details: Value, line: &str) {
    if as_json {
        let mut object = json!({ "event": name });
        if let (Some(target), Value::Object(extra)) = (object.as_object_mut(), details) {
            target.extend(extra);
        }
        println!("{object}");
    } else {
        println!("{line}");
    }
    let _ = std::io::stdout().flush();
}

/// `wen watch`: loads a scene and reloads it whenever a project file
/// changes; a change that makes the project invalid is reported and the
/// last good scene stays. With `--json` every event is one JSON line.
pub fn watch(
    example: &str,
    scene: &str,
    root: &Path,
    seconds: u64,
    as_json: bool,
) -> Result<Outcome> {
    let mut app = registry_app(example)?;
    let (mut hot, warnings) = HotScene::start(root, scene, app.world_mut())?;
    event(
        as_json,
        "loaded",
        json!({ "scene": scene, "entities": hot.entities().len(), "warnings": warnings }),
        &format!("Loaded {scene}: {} entities.", hot.entities().len()),
    );
    let started = Instant::now();
    let mut reloads = 0u64;
    let mut rejected = 0u64;
    while seconds == 0 || started.elapsed() < Duration::from_secs(seconds) {
        std::thread::sleep(Duration::from_millis(100));
        // A failure here (such as the project folder being unreadable for a
        // moment) is reported, and watching goes on.
        let reload = match hot.poll(app.world_mut()) {
            Ok(Some(reload)) => reload,
            Ok(None) => continue,
            Err(problem) => {
                rejected += 1;
                event(
                    as_json,
                    "error",
                    json!({ "diagnostics": [problem] }),
                    &format!("Could not reload.\n{problem}"),
                );
                continue;
            }
        };
        if reload.applied {
            reloads += 1;
        } else {
            rejected += 1;
        }
        let name = if reload.applied {
            "reloaded"
        } else {
            "rejected"
        };
        let mut line = format!(
            "{} ({} changed): {} entities.",
            if reload.applied {
                "Reloaded"
            } else {
                "Kept the last good scene"
            },
            reload.changed.len(),
            reload.entities
        );
        for diagnostic in &reload.diagnostics {
            line.push_str(&format!("\n{diagnostic}"));
        }
        event(
            as_json,
            name,
            json!({
                "changed": reload.changed,
                "entities": reload.entities,
                "diagnostics": reload.diagnostics,
            }),
            &line,
        );
    }
    let text = if as_json {
        json!({ "ok": true, "reloads": reloads, "rejected": rejected }).to_string()
    } else {
        format!("Stopped: {reloads} reload(s), {rejected} rejected.")
    };
    Ok(Outcome {
        text,
        success: true,
    })
}
