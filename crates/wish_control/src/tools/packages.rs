//! `packages` (M10a; design note M10 §12): what a project and a person
//! have installed, and installing, removing and locking packages.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::packages::{self, Scope};
use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const PACKAGES: Tool = Tool {
    name: "packages",
    title: "Packages: themes, workspaces, languages, recipes, templates",
    description: "Packages bring themes, workspaces (the AI mode's layout, game card and ready requests), languages, recipes and templates. A package is a folder with a wish-package.json. list shows what is installed in the project (packages/, locked by wish.lock) and for the person, and warns when files no longer match the lock. check reads a package folder without installing it. install copies one in (scope project by default: it travels with the project; scope user, a person only: for every project), remove moves one to the trash, lock writes wish.lock again.",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&["list", "check", "install", "remove", "lock"]),
            "What to do (default list).",
        )
        .or("\"list\""),
        Param::optional(
            "path",
            Kind::String,
            "For check and install: the package's folder (absolute, or relative to the project).",
        ),
        Param::optional("id", Kind::String, "For remove: the package's id."),
        Param::optional(
            "scope",
            Kind::Choice(&["project", "user"]),
            "For install and remove: the project's packages (default) or the person's own (a person only).",
        )
        .or("\"project\""),
        Param::optional(
            "replace",
            Kind::Boolean,
            "For install: replace the installed version of the same package (it goes to the trash).",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check the call and say what it would do, without doing it.",
        ),
    ],
    mutates: true,
    run,
};

fn diagnostics(found: &[Diagnostic]) -> Value {
    json!(found.iter().map(|d| json!(**d)).collect::<Vec<_>>())
}

fn scope_of(args: &Args) -> Scope {
    match args.str("scope") {
        Some("user") => Scope::User,
        _ => Scope::Project,
    }
}

fn person_only(actor: &Actor, scope: Scope) -> Result<()> {
    if scope == Scope::User && !matches!(actor, Actor::Human) {
        return Err(Diagnostic::error(
            "permission_denied",
            "Only a person can change their own packages (scope user): they change the person's editor in every project.",
            "Install into the project instead (scope project), or ask the person.",
        ));
    }
    Ok(())
}

fn folder_arg(workspace: &Workspace, args: &Args, action: &str) -> Result<std::path::PathBuf> {
    let Some(path) = args.str("path").filter(|p| !p.trim().is_empty()) else {
        return Err(Diagnostic::error(
            "missing_parameter",
            format!("{action} needs \"path\": the package's folder."),
            "Write it like \"path\": \"C:/Downloads/dusk\" (the folder with wish-package.json).",
        )
        .at_path("/path"));
    };
    let given = std::path::Path::new(path.trim());
    Ok(if given.is_absolute() {
        given.to_path_buf()
    } else {
        workspace.root().join(given)
    })
}

/// Writes `wish.lock` and the requirements in `wish.json` from what the
/// project has installed. Without packages there is no lock file (a project
/// that had one package and lost it is as it was before).
fn relock(workspace: &mut Workspace) -> Result<()> {
    let root = workspace.root().to_path_buf();
    let requirements = packages::requirements(&root);
    if requirements.is_empty() {
        let lock = root.join(packages::LOCK_FILE);
        if lock.is_file() {
            std::fs::remove_file(&lock).map_err(|error| {
                Diagnostic::error(
                    "file_unwritable",
                    format!("Could not remove {}: {error}.", packages::LOCK_FILE),
                    "Close programs that use it, then write the lock again.",
                )
            })?;
        }
    } else {
        let text = packages::lock_text(&root)?;
        crate::documents::write_project_file(&root, packages::LOCK_FILE, &text)?;
    }
    workspace.write_packages(requirements)
}

fn run(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let action = args.str("action").unwrap_or("list");
    let root = workspace.root().to_path_buf();
    let dry_run = args.flag("dry_run");
    let mut out = Map::new();
    match action {
        "check" => {
            let folder = folder_arg(workspace, args, action)?;
            out = packages::describe(&folder)?;
        }
        "install" => {
            let scope = scope_of(args);
            person_only(actor, scope)?;
            let folder = folder_arg(workspace, args, action)?;
            let plan = packages::plan_install(&folder, scope, &root, args.flag("replace"))?;
            out.insert("package".to_owned(), plan.manifest.to_value());
            out.insert("scope".to_owned(), json!(scope.name()));
            out.insert("replaces".to_owned(), json!(plan.replaces));
            out.insert("diagnostics".to_owned(), diagnostics(&plan.warnings));
            if dry_run {
                out.insert("dry_run".to_owned(), json!(true));
                return Ok(out);
            }
            packages::install(&folder, &plan, scope, &root)?;
            if scope == Scope::Project {
                relock(workspace)?;
            }
            out.insert("installed".to_owned(), json!(true));
        }
        "remove" => {
            let scope = scope_of(args);
            person_only(actor, scope)?;
            let Some(id) = args.str("id").map(str::trim).filter(|id| !id.is_empty()) else {
                return Err(Diagnostic::error(
                    "missing_parameter",
                    "remove needs \"id\": which package.",
                    "packages with action list shows them and their ids.",
                )
                .at_path("/id"));
            };
            if dry_run {
                let installed = packages::installed(scope, &root);
                if !installed.iter().any(|package| package.id == id) {
                    // The real call says what is wrong.
                    packages::remove(id, scope, &root)?;
                }
                out.insert("id".to_owned(), json!(id));
                out.insert("dry_run".to_owned(), json!(true));
                return Ok(out);
            }
            let trash = packages::remove(id, scope, &root)?;
            if scope == Scope::Project {
                relock(workspace)?;
            }
            out.insert("id".to_owned(), json!(id));
            out.insert("removed".to_owned(), json!(true));
            out.insert("trash".to_owned(), json!(trash.display().to_string()));
        }
        "lock" => {
            let text = packages::lock_text(&root)?;
            out.insert(
                "lock".to_owned(),
                serde_json::from_str(&text).unwrap_or(Value::Null),
            );
            if dry_run {
                out.insert("dry_run".to_owned(), json!(true));
                return Ok(out);
            }
            relock(workspace)?;
        }
        _ => {
            let (locked, mut warnings) = match packages::read_lock(&root) {
                Ok(locked) => (locked, Vec::new()),
                Err(problem) => (Vec::new(), vec![problem]),
            };
            let (project, project_warnings) = packages::listing(Scope::Project, &root, &locked);
            let (user, user_warnings) = packages::listing(Scope::User, &root, &[]);
            warnings.extend(project_warnings);
            warnings.extend(user_warnings);
            out.insert("project".to_owned(), json!(project));
            out.insert("user".to_owned(), json!(user));
            out.insert("diagnostics".to_owned(), diagnostics(&warnings));
        }
    }
    Ok(out)
}
