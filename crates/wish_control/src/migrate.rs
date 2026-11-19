//! Migrating a project's documents to the engine's file format (ADR-027,
//! rule 1; M12 package 9). `wen migrate` is this.
//!
//! Scene and prefab files go through [`wish_scene::migrate::upgrade`]: a file
//! of an older format has every value it relied on whose default changed
//! written into it, so it means under the new defaults what it meant under
//! the old ones (its picture to the byte), and its format number goes up.
//! Meta and material files have one format so far and are only checked.
//!
//! Design: `docs/design/M12-look-lock.md`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;
use wish_core::{Diagnostic, Result};
use wish_scene::migrate::{Defaults, Kind, Step};

/// What a migration found and did.
#[derive(Debug, Default)]
pub struct Report {
    /// Documents already at the current format.
    pub up_to_date: usize,
    /// Documents migrated (or, in a dry run, that would be), with the JSON
    /// pointers of the values written into each.
    pub migrated: Vec<(String, Vec<String>)>,
    /// Documents that could not be read or migrated.
    pub diagnostics: Vec<Diagnostic>,
}

/// The engine's own defaults now: those of every type it registers.
pub fn engine_defaults() -> Result<Defaults> {
    let mut world = wish_ecs::World::new();
    crate::types::register_engine_types(&mut world)?;
    Defaults::of_registry(world.registry())
}

fn kind_of(name: &str) -> Option<Kind> {
    if name.ends_with(".scene.json") {
        Some(Kind::Scene)
    } else if name.ends_with(".prefab.json") {
        Some(Kind::Prefab)
    } else {
        None
    }
}

/// Migrates the documents of the project at `root` (all, or only `only`,
/// project-relative, with the prefabs they place) to scene format `to`
/// through `steps`, the newest defaults being `current`. Meta and material
/// files are checked against format 1. With `write` false nothing is
/// written (a dry run). Each file is written atomically inside the
/// project; a file that cannot be written or migrated is a located
/// diagnostic in the report, and the others go on.
pub fn migrate_project(
    root: &Path,
    only: &[String],
    steps: &[Step],
    to: u32,
    current: &Defaults,
    write: bool,
) -> Result<Report> {
    let mut report = Report::default();
    let documents = wish_asset::index::documents(root)?;
    // Every document as JSON, read before anything is written: an instance
    // asks its prefab file as it was.
    let mut read: BTreeMap<String, Value> = BTreeMap::new();
    for (name, path) in &documents {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                if only.is_empty() || only.contains(name) {
                    report.diagnostics.push(
                        Diagnostic::error(
                            "file_unreadable",
                            format!("Could not read '{name}': {error}."),
                            "Check that the file can be read and is UTF-8 text.",
                        )
                        .at_file(name.clone()),
                    );
                }
                continue;
            }
        };
        match wish_scene::json::parse(&source, name) {
            Ok(node) => {
                read.insert(name.clone(), node.to_value());
            }
            Err(diagnostic) => {
                if only.is_empty() || only.contains(name) {
                    report.diagnostics.push(diagnostic);
                }
            }
        }
    }
    // `wen migrate <scene>` alone takes the prefabs the scene places along:
    // an instance's values ride on its prefab's, which must be migrated too.
    let mut selected: Vec<String> = only.to_vec();
    if !only.is_empty() {
        let mut queue = only.to_vec();
        while let Some(name) = queue.pop() {
            for prefab in placed_prefabs(read.get(&name)) {
                if read.contains_key(&prefab) && !selected.contains(&prefab) {
                    selected.push(prefab.clone());
                    queue.push(prefab);
                }
            }
        }
    }
    let wanted = |name: &str| selected.is_empty() || selected.iter().any(|s| s == name);
    for (name, document) in &read {
        if !wanted(name) {
            continue;
        }
        let Some(kind) = kind_of(name) else {
            // Meta and material files: format 1 is their only format.
            match document.get("format").and_then(Value::as_u64) {
                Some(1) => report.up_to_date += 1,
                Some(format) if format > 1 => report.diagnostics.push(
                    Diagnostic::error(
                        "unsupported_format",
                        format!("'{name}' is format {format}, newer than this engine reads (1)."),
                        "Update the engine; a newer file cannot be downgraded.",
                    )
                    .at_file(name.clone()),
                ),
                Some(format) => report.diagnostics.push(
                    Diagnostic::error(
                        "no_migration",
                        format!("'{name}' is format {format}, and there is no migration from it."),
                        "Format 1 is the first format; restore the file from version control or rewrite it.",
                    )
                    .at_file(name.clone()),
                ),
                None => report.diagnostics.push(
                    Diagnostic::error(
                        "missing_format",
                        format!("'{name}' has no numeric \"format\" key."),
                        "Add \"format\": 1 at the top level.",
                    )
                    .at_file(name.clone()),
                ),
            }
            continue;
        };
        let mut file = document.clone();
        if file.get("format").and_then(Value::as_u64) == Some(u64::from(to)) {
            report.up_to_date += 1;
            continue;
        }
        // What a prefab gives an entity, as its file was before anything was
        // written. A prefab that is missing, or a component that comes from
        // a prefab placed inside the prefab, is reported rather than taken
        // silently as "the prefab gives nothing".
        let unresolved: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let prefab_has = |prefab: &str, entity: &str, component: &str| match prefab_gives(
            &read, prefab, entity, component, 0,
        ) {
            Gives::Yes => true,
            Gives::No => false,
            Gives::Unknown(why) => {
                let mut list = unresolved.borrow_mut();
                if !list.iter().any(|d| d.message.contains(&why)) {
                    list.push(
                            Diagnostic::warning(
                                "migration_prefab_unresolved",
                                format!("'{name}': {why}; its value was written as if the prefab gave none."),
                                "Check the result, or restore the prefab and migrate the whole project (wen migrate without file names).",
                            )
                            .at_file(name.clone()),
                        );
                }
                false
            }
        };
        let upgraded =
            wish_scene::migrate::upgrade(&mut file, kind, to, steps, current, &prefab_has);
        report.diagnostics.extend(unresolved.into_inner());
        match upgraded {
            Ok(written) => {
                if write {
                    let saved = wish_scene::json::to_canonical(&file).and_then(|text| {
                        crate::documents::inside_project(root, name)?;
                        crate::documents::write_project_file(root, name, &text)
                    });
                    // A file that cannot be written is reported and the
                    // others go on: the answer lists both.
                    if let Err(problem) = saved {
                        report.diagnostics.push(problem.at_file(name.clone()));
                        continue;
                    }
                }
                report.migrated.push((name.clone(), written));
            }
            Err(problem) => {
                let mut problem = problem.at_file(name.clone());
                problem.message = format!("'{name}': {}", problem.message);
                report.diagnostics.push(problem);
            }
        }
    }
    Ok(report)
}

/// The prefabs a scene or prefab file places (its instances' `prefab`).
fn placed_prefabs(file: Option<&Value>) -> Vec<String> {
    file.and_then(|file| file.get("entities"))
        .and_then(Value::as_object)
        .map(|entities| {
            entities
                .values()
                .filter_map(|entity| entity.get("prefab").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a prefab gives one of its entities a component.
enum Gives {
    Yes,
    No,
    /// It cannot be told, and why.
    Unknown(String),
}

/// How deep prefabs placed inside prefabs are followed.
const MAX_PREFAB_DEPTH: u32 = 16;

fn prefab_gives(
    read: &BTreeMap<String, Value>,
    prefab: &str,
    entity: &str,
    component: &str,
    depth: u32,
) -> Gives {
    let Some(file) = read.get(prefab) else {
        return Gives::Unknown(format!("the prefab '{prefab}' is missing or unreadable"));
    };
    let pointer = format!("/entities/{}", wish_scene::migrate::escape(entity));
    let Some(part) = file.pointer(&pointer) else {
        return Gives::No;
    };
    if part
        .get("components")
        .and_then(Value::as_object)
        .is_some_and(|components| components.contains_key(component))
    {
        return Gives::Yes;
    }
    // The part is itself an instance of another prefab: what it has comes
    // from that prefab's root.
    match part.get("prefab").and_then(Value::as_str) {
        Some(_) if depth >= MAX_PREFAB_DEPTH => Gives::Unknown(format!(
            "prefabs inside '{prefab}' are nested more than {MAX_PREFAB_DEPTH} deep"
        )),
        Some(inner) => prefab_gives(read, inner, wish_scene::PREFAB_ROOT, component, depth + 1),
        None => Gives::No,
    }
}
