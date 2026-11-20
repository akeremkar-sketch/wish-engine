//! The held-out set (M12 package 10; the tasks themselves are written in
//! release 1.1). The score that counts comes from tasks that nobody
//! improving the engine has read, so they are kept apart:
//!
//! - **Where:** a folder outside the repository (default
//!   `C:/wish-build/held-out`, or `--held-out <folder>`). A folder inside the
//!   repository is refused: the repository is public, and every agent that
//!   works on the engine searches it.
//! - **Format:** one `*.task.json` per task, data only (no code to compile),
//!   so the tooling runs them without them being part of the source:
//!   setup operations and files, a prompt, hidden scenario tests, an
//!   optional visibility check, a reference solution that proves the check
//!   can pass, and an optional follow-up for the second agent. The open
//!   example `src/example.task.json` shows the format and is run by
//!   `selftest`.
//! - **Guard:** a held-out run prints and records numbers only: no task id,
//!   no failure reason, no transcript (deleted with the try's project as
//!   soon as its numbers are read), and what the Claude Code client kept of
//!   its sessions is removed (`forget_sessions`). `selftest --held-out`
//!   validates the set and prints counts only; `selftest --task-file`
//!   refuses a file anywhere under a held-out folder. A task that is
//!   studied moves to the open set and a new one replaces it (M12 plan,
//!   "The AI scorecard").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::followups::FollowUpTask;
use crate::game_tasks::{hidden_passes, play_check};
use crate::probe::{Probe, Verdict, require_visible};
use crate::tasks::EvalTask;

/// Where the held-out tasks live when no folder is given.
pub const DEFAULT_FOLDER: &str = "C:/wish-build/held-out";

/// The open example of the format, run by the self-test.
pub const EXAMPLE: &str = include_str!("example.task.json");

/// What a declared task's check asks.
#[derive(Clone, Debug, Default)]
struct DeclaredCheck {
    /// A game-code task: the code must build, `check` pass, the project's
    /// own tests (at least one that checks something) pass, then the hidden
    /// tests (as `play_check`).
    game: bool,
    /// `check` must pass (scene tasks; game tasks always).
    check_passes: bool,
    /// Entities that must be visible from the camera, with the percent.
    visible: BTreeMap<String, f64>,
    /// Hidden scenario tests (objects in the `*.test.json` format).
    tests: Vec<Value>,
}

/// A solution written as data: files, a build, operations.
#[derive(Clone, Debug, Default)]
struct DeclaredSolution {
    files: BTreeMap<String, String>,
    build: bool,
    ops: Value,
}

/// The follow-up of a declared task.
#[derive(Clone, Debug)]
pub struct DeclaredFollowUp {
    prompt: String,
    check: DeclaredCheck,
    reference: DeclaredSolution,
}

/// A task read from a task file.
#[derive(Clone, Debug)]
pub struct DeclaredTask {
    id: String,
    prompt: String,
    files: bool,
    setup: DeclaredSolution,
    check: DeclaredCheck,
    reference: DeclaredSolution,
    follow_up: Option<DeclaredFollowUp>,
}

fn strings(value: &Value, what: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (path, text) in value.as_object().into_iter().flatten() {
        let text = text
            .as_str()
            .ok_or(format!("{what}: the file '{path}' must be text"))?;
        if path.contains("..") || Path::new(path).is_absolute() {
            return Err(format!(
                "{what}: '{path}' must be a path inside the project"
            ));
        }
        out.insert(path.clone(), text.to_owned());
    }
    Ok(out)
}

fn solution(value: &Value, what: &str) -> Result<DeclaredSolution, String> {
    Ok(DeclaredSolution {
        files: strings(&value["files"], what)?,
        build: value["build"] == true,
        ops: if value["ops"].is_array() {
            value["ops"].clone()
        } else {
            json!([])
        },
    })
}

fn declared_check(value: &Value, what: &str) -> Result<DeclaredCheck, String> {
    let mut visible = BTreeMap::new();
    for (id, percent) in value["visible"].as_object().into_iter().flatten() {
        visible.insert(
            id.clone(),
            percent
                .as_f64()
                .ok_or(format!("{what}: visible.{id} must be a number"))?,
        );
    }
    let tests: Vec<Value> = value["tests"].as_array().cloned().unwrap_or_default();
    if tests.iter().any(|t| !t["steps"].is_array()) {
        return Err(format!("{what}: every hidden test needs steps"));
    }
    let check = DeclaredCheck {
        game: value["game"] == true,
        check_passes: value["check_passes"] != false,
        visible,
        tests,
    };
    if check.tests.is_empty() && check.visible.is_empty() {
        return Err(format!(
            "{what}: the check checks nothing (no tests, no visible)"
        ));
    }
    if check.game && check.tests.is_empty() {
        return Err(format!("{what}: a game task needs a hidden test"));
    }
    Ok(check)
}

impl DeclaredTask {
    /// Reads a task file's text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("not a task file: {e}"))?;
        if value["format"] != 1 {
            return Err("a task file has \"format\": 1".to_owned());
        }
        let id = value["id"]
            .as_str()
            .filter(|id| {
                !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .ok_or("a task needs an id of a-z, 0-9 and _")?
            .to_owned();
        let prompt = value["prompt"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .ok_or("a task needs a prompt")?
            .to_owned();
        let follow_up = match value.get("follow_up") {
            Some(f) if !f.is_null() => Some(DeclaredFollowUp {
                prompt: f["prompt"]
                    .as_str()
                    .ok_or("a follow-up needs a prompt")?
                    .to_owned(),
                check: declared_check(&f["check"], "follow_up.check")?,
                reference: solution(&f["reference"], "follow_up.reference")?,
            }),
            _ => None,
        };
        Ok(Self {
            id,
            prompt,
            files: value["files"] == true,
            setup: solution(&value["setup"], "setup")?,
            check: declared_check(&value["check"], "check")?,
            reference: solution(&value["reference"], "reference")?,
            follow_up,
        })
    }
}

fn apply(probe: &mut Probe, solution: &DeclaredSolution) -> Result<(), String> {
    let root = probe.root();
    for (path, text) in &solution.files {
        let target = root.join(path);
        if let Some(folder) = target.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, text).map_err(|e| e.to_string())?;
    }
    if solution.build {
        let built = probe.call("script_build", json!({}))?;
        if built["passed"] != true {
            return Err(format!(
                "the game code does not build: {}",
                built["diagnostics"]
            ));
        }
    }
    if solution.ops.as_array().is_some_and(|ops| !ops.is_empty()) {
        probe.call("edit_apply", json!({ "ops": solution.ops }))?;
    }
    Ok(())
}

fn run_check(check: &DeclaredCheck, probe: &mut Probe, dir: &Path) -> Verdict {
    if !check.visible.is_empty() {
        let seen = probe.visible(None)?;
        for (id, percent) in &check.visible {
            require_visible(&seen, id, *percent)?;
        }
    }
    let mut tests = check.tests.iter();
    if check.game {
        if let Some(first) = tests.next() {
            play_check(probe, dir, first)?;
        }
    } else if check.check_passes {
        probe.check_passes()?;
    }
    for test in tests {
        hidden_passes(probe, dir, test)?;
    }
    Ok(())
}

impl EvalTask for DeclaredTask {
    fn id(&self) -> &str {
        &self.id
    }
    fn prompt(&self) -> &str {
        &self.prompt
    }
    fn files(&self) -> bool {
        self.files
    }
    fn setup(&self, dir: &Path) -> Result<(), String> {
        let mut probe = Probe::open(dir)?;
        apply(&mut probe, &self.setup)
    }
    fn baseline(&self, _: &mut Probe) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn check(&self, probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
        run_check(&self.check, probe, dir)
    }
    fn reference(&self, probe: &mut Probe) -> Result<(), String> {
        apply(probe, &self.reference)
    }
    fn follow_up(&self) -> Option<&dyn FollowUpTask> {
        self.follow_up.as_ref().map(|f| f as &dyn FollowUpTask)
    }
}

impl FollowUpTask for DeclaredFollowUp {
    fn prompt(&self) -> &str {
        &self.prompt
    }
    fn check(&self, probe: &mut Probe, dir: &Path) -> Verdict {
        run_check(&self.check, probe, dir)
    }
    fn reference(&self, probe: &mut Probe) -> Result<(), String> {
        apply(probe, &self.reference)
    }
}

/// The repository this tool was built from. `CARGO_MANIFEST_DIR` is fixed
/// when the tool is compiled, so a `wish-evals` copied to another machine
/// or a moved checkout has none (`None`), and the guards then refuse.
pub fn repository() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("..").canonicalize().ok()
}

/// The repository, or the current folder when it cannot be found (for
/// where results are written, not for a guard).
pub fn repository_or_here() -> PathBuf {
    repository().unwrap_or_else(|| PathBuf::from("."))
}

/// Whether `folder` is inside the repository, where held-out tasks must
/// never be. When the repository cannot be found the answer is yes: the
/// guard refuses rather than passes.
pub fn inside_repository(folder: &Path) -> bool {
    let Some(repository) = repository() else {
        return true;
    };
    let Ok(folder) = folder.canonicalize() else {
        return false;
    };
    folder.starts_with(repository)
}

/// Whether `file` is in one of the held-out `folders` (at any depth).
/// Both sides are canonicalized, so `..`, links and letter case of the
/// path as written do not get around it. A folder that does not exist
/// holds nothing.
pub fn in_held_out(file: &Path, folders: &[PathBuf]) -> bool {
    let Ok(file) = file.canonicalize() else {
        return false;
    };
    folders
        .iter()
        .filter_map(|f| f.canonicalize().ok())
        .any(|folder| file.ancestors().any(|a| a == folder))
}

// --------------------------------------------------------------------------
// What the Claude Code client keeps of a session.

/// The client's own folder: `CLAUDE_CONFIG_DIR`, else `.claude` in the
/// person's home folder.
pub fn client_folder() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|home| PathBuf::from(home).join(".claude"))
}

/// The name of the client's folder for sessions started in `dir`: every
/// character that is not an ASCII letter or digit becomes `-`.
pub fn project_key(dir: &Path) -> String {
    dir.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Removes what the client kept of the sessions of one held-out try: its
/// project folder (the try's folder is used for nothing else), every file
/// or folder one level below the client's folder that is named after one of
/// the sessions (todos, file history, debug logs), and the lines of its
/// prompt history that name the try's folder or a session. Returns how many
/// entries were removed. The person's sign-in and settings are not touched.
pub fn forget_sessions(client: &Path, dir: &Path, sessions: &[String]) -> usize {
    let mut removed = 0;
    let mut keys = vec![project_key(dir)];
    if let Ok(real) = dir.canonicalize() {
        let real = real
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned();
        keys.push(project_key(Path::new(&real)));
    }
    for key in &keys {
        let folder = client.join("projects").join(key);
        if folder.is_dir() && std::fs::remove_dir_all(&folder).is_ok() {
            removed += 1;
        }
    }
    let named = |name: &str| {
        sessions
            .iter()
            .any(|s| !s.is_empty() && name.contains(s.as_str()))
    };
    for entry in std::fs::read_dir(client).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() || entry.file_name() == "projects" {
            continue;
        }
        for inner in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            if named(&inner.file_name().to_string_lossy()) {
                let gone = if inner.path().is_dir() {
                    std::fs::remove_dir_all(inner.path())
                } else {
                    std::fs::remove_file(inner.path())
                };
                removed += usize::from(gone.is_ok());
            }
        }
    }
    let history = client.join("history.jsonl");
    if let Ok(text) = std::fs::read_to_string(&history) {
        let dir_text = dir.to_string_lossy().replace('\\', "/");
        let mentions = |line: &str| {
            let line = line.replace("\\\\", "/").replace('\\', "/");
            named(&line) || line.contains(&dir_text)
        };
        let kept: Vec<&str> = text.lines().filter(|l| !mentions(l)).collect();
        let dropped = text.lines().count() - kept.len();
        if dropped > 0 {
            let side = client.join("history.jsonl.wish-evals");
            let body = if kept.is_empty() {
                String::new()
            } else {
                kept.join("\n") + "\n"
            };
            if std::fs::write(&side, body).is_ok() && std::fs::rename(&side, &history).is_ok() {
                removed += dropped;
            }
        }
    }
    removed
}

/// Reads every `*.task.json` of the held-out folder, in name order. Refuses
/// a folder inside the repository.
pub fn load(folder: &Path) -> Result<Vec<DeclaredTask>, String> {
    if inside_repository(folder) {
        return Err(
            "the held-out tasks must live outside the repository (it is public, and agents that improve the engine read it)"
                .to_owned(),
        );
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|_| format!("there is no held-out folder at {}", folder.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".task.json"))
        .collect();
    files.sort();
    let mut tasks = Vec::new();
    let mut broken = 0;
    for file in &files {
        match std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|text| DeclaredTask::parse(&text))
        {
            Ok(task) => tasks.push(task),
            Err(_) => broken += 1,
        }
    }
    if broken > 0 {
        // Numbers only: the reason would show the task.
        return Err(format!(
            "{broken} of {} held-out task files cannot be read (check them with `wish-evals selftest --task-file <file>` outside an improving session)",
            files.len()
        ));
    }
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_example_is_a_valid_task() {
        let task = DeclaredTask::parse(EXAMPLE).unwrap();
        assert_eq!(task.id(), "example_green_ball");
        assert!(!task.files());
        assert!(task.follow_up().is_some());
    }

    #[test]
    fn a_folder_inside_the_repository_is_refused() {
        let inside = repository().unwrap().join("evals");
        assert!(inside_repository(&inside));
        let refused = load(&inside).unwrap_err();
        assert!(refused.contains("outside the repository"), "{refused}");
        assert!(!inside_repository(Path::new("C:/no/such/folder")));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wish-evals-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_task_file_anywhere_under_a_held_out_folder_is_held_out() {
        let root = scratch("guard");
        let held = root.join("held");
        std::fs::create_dir_all(held.join("deep").join("er")).unwrap();
        let inside = held.join("deep").join("er").join("a.task.json");
        let outside = root.join("b.task.json");
        std::fs::write(&inside, "{}").unwrap();
        std::fs::write(&outside, "{}").unwrap();
        let folders = vec![PathBuf::from("C:/no/such/held-out"), held.clone()];
        assert!(in_held_out(&inside, &folders));
        // Written with `..`, it is still the same file.
        let around = held
            .join("deep")
            .join("..")
            .join("deep")
            .join("er")
            .join("a.task.json");
        assert!(in_held_out(&around, &folders));
        assert!(!in_held_out(&outside, &folders));
        assert!(!in_held_out(&inside, &[root.join("other")]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_held_out_trys_sessions_are_forgotten_and_nothing_else() {
        let client = scratch("client");
        let dir = PathBuf::from("C:/wish-build/evals/label/sonnet/h3-1");
        let key = project_key(&dir);
        assert_eq!(key, "C--wish-build-evals-label-sonnet-h3-1");
        let project = client.join("projects").join(&key);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("s-123.jsonl"), "secret").unwrap();
        let other = client.join("projects").join("C--some-other-project");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("s-999.jsonl"), "keep").unwrap();
        std::fs::create_dir_all(client.join("todos")).unwrap();
        std::fs::write(client.join("todos").join("s-123-agent.json"), "x").unwrap();
        std::fs::write(client.join("todos").join("s-999-agent.json"), "x").unwrap();
        std::fs::write(client.join(".credentials.json"), "sign-in").unwrap();
        std::fs::write(
            client.join("history.jsonl"),
            "{\"project\":\"C:\\\\wish-build\\\\evals\\\\label\\\\sonnet\\\\h3-1\"}\n{\"project\":\"C:\\\\elsewhere\"}\n",
        )
        .unwrap();
        let removed = forget_sessions(&client, &dir, &["s-123".to_owned()]);
        assert_eq!(removed, 3, "the project folder, the todo, one history line");
        assert!(!project.exists());
        assert!(other.join("s-999.jsonl").exists());
        assert!(client.join("todos").join("s-999-agent.json").exists());
        assert!(client.join(".credentials.json").exists());
        let history = std::fs::read_to_string(client.join("history.jsonl")).unwrap();
        assert!(
            history.contains("elsewhere") && !history.contains("h3-1"),
            "{history}"
        );
        let _ = std::fs::remove_dir_all(&client);
    }

    #[test]
    fn broken_task_files_are_refused() {
        assert!(DeclaredTask::parse("{}").is_err());
        assert!(DeclaredTask::parse(r#"{ "format": 1, "id": "a b", "prompt": "x" }"#).is_err());
        let nothing_checked = r#"{ "format": 1, "id": "a", "prompt": "x", "check": {} }"#;
        assert!(
            DeclaredTask::parse(nothing_checked)
                .unwrap_err()
                .contains("checks nothing")
        );
        let escape = r#"{ "format": 1, "id": "a", "prompt": "x",
            "setup": { "files": { "../x": "y" } }, "check": { "visible": { "a": 1 } } }"#;
        assert!(
            DeclaredTask::parse(escape)
                .unwrap_err()
                .contains("inside the project")
        );
    }
}
