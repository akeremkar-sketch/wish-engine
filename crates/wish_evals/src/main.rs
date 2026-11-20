//! `wish-evals`: agent evaluations (ADR-017 layer 4, design M4 §8) and the
//! AI scorecard (M12 package 10, design `docs/design/M12-ai-scorecard.md`).
//!
//! ```text
//! wish-evals selftest [--tasks 01,42]   free: every check fails before and passes after the
//!                                       reference solution; every measure of the scorecard
//!                                       runs on the references; deliberately wrong results
//!                                       are refuted
//! wish-evals selftest --held-out [dir]  free: validates the held-out set, prints counts only
//! wish-evals selftest --task-file <f>   free: validates one task file, with details
//! wish-evals campaign --label <name> [--models sonnet:medium,haiku:medium] [--tries 3]
//!            [--repairs 1] [--second-agent] [--max-cost <dollars>] [--overwrite]
//!            [--held-out [dir]] [--tasks 01,42]
//!                                       SPENDS MODEL QUOTA (ask the user first): runs every
//!                                       task with every model and writes the scorecard to
//!                                       evals/results/<name>.json and .md
//! wish-evals run --label <name> [--model sonnet] [--effort medium] [--tries 3] ...
//!                                       SPENDS MODEL QUOTA: a campaign of one model
//! wish-evals card --from <result.json>  free: writes the scorecard's Markdown again from its JSON
//! wish-evals rescore --label <name>     free: reads a finished run's transcripts again
//! wish-evals recheck --label <name>     free: runs the current checks on a finished run
//! wish-evals estimate                   free: what a baseline campaign costs, from evals/results/
//! ```

mod card;
mod estimate;
mod followups;
mod game_tasks;
mod held_out;
mod long_task;
mod m6_tasks;
mod m7_tasks;
mod measures;
mod people;
mod probe;
mod tasks;
mod transcript;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::followups::FollowUpTask;
use crate::held_out::DeclaredTask;
use crate::probe::Probe;
use crate::tasks::{EvalTask, Task};
use crate::transcript::{CLAIM_SUFFIX, Transcript, claim};

/// The harness's own folder, outside the repository and the person's home
/// (`C:/wish-build` on Windows; the system's temporary folder elsewhere).
fn work_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:/wish-build")
    } else {
        std::env::temp_dir().join("wish-build")
    }
}

/// Where the projects of finished tries are kept, with their transcripts
/// (`<label>/<model>/<task>-<n>` and `<task>-<n>.logs`).
fn work() -> PathBuf {
    work_root().join("evals")
}

/// Where a try's project lives while an agent works in it: a folder of its
/// own that holds nothing else, outside every folder the agent may not read
/// (`deny_rules`). The project moves to `work()` when the try is over.
fn live_root() -> PathBuf {
    work_root().join("evals-live")
}

/// The longest one agent run may take (the long task needs more).
const TRY_LIMIT: Duration = Duration::from_secs(15 * 60);
const LONG_TRY_LIMIT: Duration = Duration::from_secs(40 * 60);

fn wen_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(if cfg!(windows) { "wen.exe" } else { "wen" })
}

/// The open set: the 41 tasks of M4 to M7 and the long task.
fn open_set() -> Vec<Task> {
    let mut all = tasks::all();
    all.extend(m6_tasks::all());
    all.extend(m7_tasks::all());
    all.push(long_task::task());
    all
}

/// A fresh project for a task: `wen new`, MCP on the software adapter, setup.
fn prepare(task: &dyn EvalTask, dir: &Path) -> Result<Value, String> {
    let _ = std::fs::remove_dir_all(dir);
    let wen = wen_path();
    wish_control::scaffold::create_project(dir, "Eval", &wen.to_string_lossy())
        .map_err(|problem| problem.message.clone())?;
    let mcp =
        json!({ "mcpServers": { "wish": { "command": wen, "args": ["mcp", "--software"] } } });
    std::fs::write(
        dir.join(".mcp.json"),
        serde_json::to_string_pretty(&mcp).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    task.setup(dir)?;
    let mut probe = Probe::open(dir)?;
    task.baseline(&mut probe)
}

fn check(task: &dyn EvalTask, dir: &Path, baseline: &Value) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    task.check(&mut probe, dir, baseline)
}

fn follow_up_check(follow: &dyn FollowUpTask, dir: &Path) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    follow.check(&mut probe, dir)
}

// --------------------------------------------------------------------------
// The self-test (free).

/// What the self-test found, for its summary line.
#[derive(Default)]
struct Tally {
    tasks: (u32, u32),
    follow_ups: (u32, u32),
    replays: (u32, u32),
    editor: (u32, u32),
    clean: (u32, u32),
    speed: Vec<(String, f64)>,
    refutations: (u32, u32),
}

fn count(slot: &mut (u32, u32), ok: bool) {
    slot.1 += 1;
    slot.0 += u32::from(ok);
}

/// The task's checks, and every measure of the scorecard, on its reference
/// solution. Returns notes for the report line, or what failed.
fn selftest_task(task: &dyn EvalTask, dir: &Path, tally: &mut Tally) -> Result<String, String> {
    let baseline = prepare(task, dir)?;
    if check(task, dir, &baseline).is_ok() {
        return Err("the check passes before anything is done".to_owned());
    }
    let before = people::scene_entities(dir);
    let mut probe = Probe::open(dir)?;
    task.reference(&mut probe)?;
    drop(probe);
    check(task, dir, &baseline)
        .map_err(|reason| format!("the reference solution fails: {reason}"))?;
    let mut notes = Vec::new();
    // Quality: `check` on the result.
    let quality = measures::quality(dir)?;
    if quality.errors > 0 {
        return Err(format!(
            "check finds {} errors in the reference",
            quality.errors
        ));
    }
    count(&mut tally.clean, quality.clean());
    if !quality.clean() {
        notes.push(format!("{} warnings", quality.warnings));
    }
    // Replay, speed and the editor on a copy, as in a campaign.
    let copy = dir.with_extension("measures");
    people::copy_project(dir, &copy)?;
    let replay = measures::replay_holds(&copy)?;
    if task.files() {
        let held = replay == Some(true);
        count(&mut tally.replays, held);
        if !held {
            let _ = std::fs::remove_dir_all(&copy);
            return Err(format!(
                "a replay of the reference does not hold ({replay:?})"
            ));
        }
        notes.push("replay holds".to_owned());
    }
    match measures::speed(&copy)? {
        measures::Speed::Measured { ms_per_tick, .. } => {
            tally.speed.push((task.id().to_owned(), ms_per_tick));
            notes.push(format!("10k: {ms_per_tick:.2} ms/tick"));
        }
        measures::Speed::NotMeaningful(_) if task.files() => notes.push("10k: n/a".to_owned()),
        measures::Speed::NotMeaningful(_) => {}
    }
    let edited = people::edit_by_hand(&copy, &before);
    let _ = std::fs::remove_dir_all(&copy);
    count(&mut tally.editor, edited.is_ok());
    let edited =
        edited.map_err(|reason| format!("the editor cannot change it by hand: {reason}"))?;
    notes.push(format!("editor: {edited}"));
    // The second agent's change.
    if let Some(follow) = task.follow_up() {
        let outcome = (|| -> Result<(), String> {
            if follow_up_check(follow, dir).is_ok() {
                return Err("the follow-up's check passes before the change".to_owned());
            }
            let mut probe = Probe::open(dir)?;
            follow.reference(&mut probe)?;
            drop(probe);
            follow_up_check(follow, dir)
                .map_err(|reason| format!("the follow-up's reference fails: {reason}"))
        })();
        count(&mut tally.follow_ups, outcome.is_ok());
        outcome?;
        notes.push("follow-up ok".to_owned());
    }
    Ok(notes.join("; "))
}

/// A transcript that says `RESULT: DONE` for a try the check failed is a
/// false success on the card.
fn false_success_on_the_card() -> Result<(), String> {
    let lines: Vec<String> = [
        json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "All works.\nRESULT: DONE" }] } }),
        json!({ "type": "result", "is_error": false, "num_turns": 1, "result": "All works.\nRESULT: DONE", "usage": {} }),
    ]
    .iter()
    .map(Value::to_string)
    .collect();
    let said = claim(transcript::read(&lines).final_message.as_deref());
    let row = json!({ "task": "x", "passed_first": false, "passed": false,
                      "claim_first": said.name(), "claim_final": said.name() });
    let card = card::model_card("m", "e", &[row]);
    if card["2_errors"]["false_successes_first"]["count"] == 1 {
        Ok(())
    } else {
        Err(format!("the card shows {}", card["2_errors"]))
    }
}

/// Deliberately wrong results that the scorecard's checks must refute.
fn refutations(tally: &mut Tally) {
    let base = work().join("selftest").join("refute");
    let mut run = |name: &str, outcome: Result<(), String>| {
        count(&mut tally.refutations, outcome.is_ok());
        match outcome {
            Ok(()) => println!("ok    refuted: {name}"),
            Err(reason) => println!("FAIL  refuted: {name}: {reason}"),
        }
    };
    // The long task's mutants: plausible wrong games.
    let long = long_task::task();
    let mutant_dirs: Vec<PathBuf> = (0..long_task::MUTANTS.len())
        .map(|n| base.join(format!("long-mutant-{n}")))
        .collect();
    for ((name, make), dir) in long_task::MUTANTS.iter().zip(&mutant_dirs) {
        let outcome = (|| {
            let baseline = prepare(&long, dir)?;
            let mut probe = Probe::open(dir)?;
            make(&mut probe)?;
            drop(probe);
            match check(&long, dir, &baseline) {
                Ok(()) => Err("the hidden checks accept it".to_owned()),
                Err(_) => Ok(()),
            }
        })();
        run(&format!("the long task: {name}"), outcome);
    }
    // The first mutant's project has game code and a replay to tamper with.
    let dir = mutant_dirs[0].clone();
    // A replay whose recording was changed.
    // The wrong long-task project has game code; its replay is fine until
    // a hash is changed.
    run(
        "a replay whose recorded hash was changed",
        measures::tampered_replay_is_refuted(&dir),
    );
    // A project whose scene file is broken cannot be edited by hand.
    let broken = base.join("broken-scene");
    run(
        "a project with a broken scene file in the editor",
        (|| {
            people::copy_project(&dir, &broken)?;
            std::fs::write(broken.join("scenes/main.scene.json"), "{ not json")
                .map_err(|e| e.to_string())?;
            match people::edit_by_hand(&broken, &Value::Null) {
                Ok(done) => Err(format!("the editor changed it anyway: {done}")),
                Err(_) => Ok(()),
            }
        })(),
    );
    // `check` errors are seen.
    let unclean = base.join("bad-input");
    run(
        "a project with a broken input.json in check",
        (|| {
            people::copy_project(&dir, &unclean)?;
            std::fs::write(
                unclean.join("input.json"),
                "{ \"format\": 1, \"actions\": 3 }",
            )
            .map_err(|e| e.to_string())?;
            let quality = measures::quality(&unclean)?;
            if quality.errors > 0 {
                Ok(())
            } else {
                Err("check finds no error".to_owned())
            }
        })(),
    );
    // A claim of success that the check refutes is a false success on the card.
    run(
        "a claimed success that the check refuted, on the card",
        false_success_on_the_card(),
    );
    for folder in mutant_dirs.iter().chain([&broken, &unclean]) {
        let _ = std::fs::remove_dir_all(folder);
    }
}

fn selftest(tasks: &[&dyn EvalTask], with_refutations: bool) -> bool {
    let mut tally = Tally::default();
    for task in tasks {
        let dir = work().join("selftest").join(task.id());
        let outcome = selftest_task(*task, &dir, &mut tally);
        count(&mut tally.tasks, outcome.is_ok());
        match outcome {
            Ok(notes) => println!("ok    {}  [{notes}]", task.id()),
            Err(reason) => println!("FAIL  {}: {reason}", task.id()),
        }
    }
    // The format of the held-out set, on its open example.
    let example = DeclaredTask::parse(held_out::EXAMPLE);
    let format_ok = match example {
        Ok(example) => {
            let dir = work().join("selftest").join("held-out-format");
            let outcome = selftest_task(&example, &dir, &mut tally);
            count(&mut tally.tasks, outcome.is_ok());
            match outcome {
                Ok(notes) => {
                    println!("ok    held-out format (the open example)  [{notes}]");
                    true
                }
                Err(reason) => {
                    println!("FAIL  held-out format (the open example): {reason}");
                    false
                }
            }
        }
        Err(reason) => {
            println!("FAIL  held-out format: the example does not parse: {reason}");
            false
        }
    };
    let guard = held_out::load(&held_out::repository_or_here().join("evals")).is_err();
    println!(
        "{}  held-out guard: a folder inside the repository is refused",
        if guard { "ok   " } else { "FAIL " }
    );
    if with_refutations {
        refutations(&mut tally);
    }
    let show = |(a, b): (u32, u32)| format!("{a}/{b}");
    println!(
        "\nselftest: tasks {}, follow-ups {}, replays hold {}, editor by hand {}, clean check {}, refutations {}",
        show(tally.tasks),
        show(tally.follow_ups),
        show(tally.replays),
        show(tally.editor),
        show(tally.clean),
        show(tally.refutations),
    );
    if !tally.speed.is_empty() {
        let list: Vec<String> = tally
            .speed
            .iter()
            .map(|(id, ms)| format!("{id} {ms:.2}"))
            .collect();
        println!("tick at 10,000 entities (ms): {}", list.join(", "));
    }
    tally.tasks.0 == tally.tasks.1
        && tally.refutations.0 == tally.refutations.1
        && format_ok
        && guard
}

/// The held-out set's self-test: counts only.
fn selftest_held_out(folder: &Path) -> bool {
    let tasks = match held_out::load(folder) {
        Ok(tasks) => tasks,
        Err(reason) => {
            println!("held-out: {reason}");
            return false;
        }
    };
    let mut tally = Tally::default();
    for (n, task) in tasks.iter().enumerate() {
        let dir = work().join("held-out-selftest").join(format!("{n}"));
        let ok = selftest_task(task, &dir, &mut tally).is_ok();
        count(&mut tally.refutations, ok);
        let _ = std::fs::remove_dir_all(&dir);
    }
    let _ = std::fs::remove_dir_all(work().join("held-out-selftest"));
    println!(
        "held-out: {} tasks, {} valid; follow-ups {}/{}",
        tasks.len(),
        tally.refutations.0,
        tally.follow_ups.0,
        tally.follow_ups.1
    );
    tally.refutations.0 as usize == tasks.len()
}

// --------------------------------------------------------------------------
// Campaigns (spend model quota).

/// The Claude Code client. `WISH_EVALS_CLIENT` names another program to run
/// in its place (a fake client for testing the harness; it is given the
/// same arguments and standard input).
fn claude_command() -> Command {
    client_command(std::env::var_os("WISH_EVALS_CLIENT"))
}

fn client_command(program: Option<std::ffi::OsString>) -> Command {
    Command::new(program.unwrap_or_else(|| {
        if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        }
        .into()
    }))
}

/// A folder as the client's permission rules write an absolute path:
/// `//c/wish-build/evals` for `C:\wish-build\evals`, `//home/x` for
/// `/home/x`.
fn rule_path(folder: &Path) -> Option<String> {
    let folder = std::path::absolute(folder).ok()?;
    let text = folder.to_string_lossy().replace('\\', "/");
    let text = text
        .strip_prefix("//?/")
        .unwrap_or(&text)
        .trim_end_matches('/')
        .to_owned();
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        Some(format!(
            "//{}{}",
            char::from(bytes[0]).to_ascii_lowercase(),
            &text[2..]
        ))
    } else if text.starts_with('/') {
        Some(format!("/{text}"))
    } else {
        None
    }
}

/// The folders an agent may not read or edit: the kept tries of every
/// campaign (other tries, the other model's solutions, transcripts and the
/// hidden checks' reasons), the held-out folders, the repository (the hidden
/// checks' code) and the person's home folder (the client's own session
/// files, and the repository when it lives there). The live folder of a try
/// is under none of them.
fn denied_folders(held_out: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![work(), PathBuf::from(held_out::DEFAULT_FOLDER)];
    folders.extend(held_out.map(Path::to_path_buf));
    folders.extend(held_out::repository());
    folders.extend(
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from),
    );
    folders.extend(held_out::client_folder());
    folders
}

/// The client's deny rules for [`denied_folders`]: `Read(...)` (which also
/// governs Grep and Glob) and `Edit(...)` (every tool that writes files).
fn deny_rules(held_out: Option<&Path>) -> Vec<String> {
    let mut rules = Vec::new();
    for folder in denied_folders(held_out) {
        if let Some(path) = rule_path(&folder) {
            for tool in ["Read", "Edit"] {
                let rule = format!("{tool}({path}/**)");
                if !rules.contains(&rule) {
                    rules.push(rule);
                }
            }
        }
    }
    rules
}

/// The client's command line for one run. The prompt is never on it (it
/// goes in on standard input; see `agent`).
fn agent_command(
    dir: &Path,
    model: &str,
    effort: &str,
    files: bool,
    resume: Option<&str>,
    deny: &[String],
) -> Command {
    agent_command_with(claude_command(), dir, model, effort, files, resume, deny)
}

fn agent_command_with(
    mut command: Command,
    dir: &Path,
    model: &str,
    effort: &str,
    files: bool,
    resume: Option<&str>,
    deny: &[String],
) -> Command {
    command
        .current_dir(dir)
        .args(["-p", "--model", model, "--effort", effort])
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--mcp-config", ".mcp.json", "--strict-mcp-config"]);
    if let Some(session) = resume {
        command.args(["--resume", session]);
    }
    // Game code tasks may read and edit the project's files; the others
    // have the engine's tools only. Bash is never allowed.
    command
        .args(if files {
            &[
                "--allowedTools",
                "mcp__wish",
                "Read",
                "Edit",
                "Write",
                "Glob",
                "Grep",
            ][..]
        } else {
            &["--allowedTools", "mcp__wish"][..]
        })
        .arg("--disallowedTools")
        .args(if files {
            &["Bash"][..]
        } else {
            &["Bash", "Edit", "Write", "Read", "Glob", "Grep"][..]
        })
        .args(["NotebookEdit", "WebFetch", "WebSearch", "Task", "TodoWrite"])
        // Nothing outside the try's own folder that could give the answer
        // away (`denied_folders`).
        .args(deny)
        .args(["--setting-sources", "project"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group, so that a stop reaches what it started
    // (`stop_tree`).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// One run of the Claude Code CLI in the project: a new session, or the
/// given one resumed (a repair round). The harness's clock times it. The
/// run's transcript goes to `log`, outside the project (a later agent in
/// the same project must not read it).
#[allow(clippy::too_many_arguments)]
fn agent(
    dir: &Path,
    log: &Path,
    prompt: &str,
    model: &str,
    effort: &str,
    files: bool,
    resume: Option<&str>,
    limit: Duration,
    deny: &[String],
) -> (Transcript, f64) {
    drive(
        agent_command(dir, model, effort, files, resume, deny),
        log,
        prompt,
        model,
        limit,
    )
}

/// How long the harness waits for the client's output to end once the
/// client has exited or was stopped.
const READER_GRACE: Duration = Duration::from_secs(10);

/// Stops the client and every process it started. On Windows the client is
/// a batch file whose child is the real client: killing the batch file alone
/// left that child running, spending quota, editing the project and holding
/// the output pipe (closing review of 1.0). `taskkill /T` ends the tree while
/// its root still lives. On Unix the client leads its own process group
/// (`agent_command_with`), and the group is killed.
fn stop_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    if cfg!(windows) {
        let taskkill = std::env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("taskkill.exe"))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("taskkill"));
        let _ = Command::new(taskkill)
            .args(["/PID", &pid, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    } else {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs the client's `command` with `prompt` on standard input, at most
/// `limit` long, and reads its transcript (written to `log`).
fn drive(
    mut command: Command,
    log: &Path,
    prompt: &str,
    model: &str,
    limit: Duration,
) -> (Transcript, f64) {
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return (
                Transcript {
                    agent_error: Some(format!("could not start claude: {error}")),
                    ..Transcript::default()
                },
                0.0,
            );
        }
    };
    // The prompt goes in on standard input: on Windows the client is a
    // batch file, and the standard library refuses a batch file argument
    // with a line break in it (every prompt has one since the claim line).
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = prompt.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }
    // Drained so that a talkative client never blocks on a full pipe.
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
        });
    }
    // The lines are shared, not returned by the thread: a process the
    // client left behind may hold the pipe open, and the harness must not
    // wait for it (see `READER_GRACE`).
    let stdout = child.stdout.take();
    let read_so_far = Arc::new(Mutex::new(Vec::new()));
    let (done, finished) = mpsc::channel::<()>();
    {
        let read_so_far = Arc::clone(&read_so_far);
        std::thread::spawn(move || {
            if let Some(stdout) = stdout {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(mut lines) = read_so_far.lock() {
                        lines.push(line);
                    }
                }
            }
            let _ = done.send(());
        });
    }
    let mut stopped = None;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > limit => {
                stop_tree(&mut child);
                stopped = Some("timed out".to_owned());
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(error) => {
                stop_tree(&mut child);
                stopped = Some(error.to_string());
                break;
            }
        }
    }
    // The client's output ends when every process holding the pipe has
    // gone; past the grace, what was read is the transcript.
    let _ = finished.recv_timeout(READER_GRACE);
    let seconds = started.elapsed().as_secs_f64();
    let lines = read_so_far
        .lock()
        .map(|mut lines| std::mem::take(&mut *lines))
        .unwrap_or_default();
    // One run per file, beside the project (removed again for held-out
    // tasks).
    if let Some(folder) = log.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(log, lines.join("\n") + "\n");
    let mut read = transcript::read(&lines);
    if stopped.is_some() {
        read.agent_error = stopped;
        read.final_message = None;
    }
    if !read.has_result {
        // Cut off before the client's report: the cost is estimated from
        // the tokens seen, and marked so.
        read.cost_usd = estimate::list_cost(model, &read);
        read.cost_estimated = true;
    }
    (read, seconds)
}

/// How a campaign runs.
struct Options {
    repairs: u32,
    second_agent: bool,
    held_out: bool,
    /// Stop between tries before the running cost would pass this.
    max_cost: Option<f64>,
    /// The client's deny rules (`deny_rules`).
    deny: Vec<String>,
}

/// Where a try's transcripts and baseline go: beside where its project is
/// kept, never in it, and never under the live folder an agent works in.
fn logs_of(dir: &Path) -> PathBuf {
    dir.with_extension("logs")
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// One try of a task: the agent, the check at the first attempt, repair
/// rounds, then every measure of the card on the result.
///
/// The project is `dir` (the try's live folder: nothing else is in it) and
/// the transcripts and the baseline go to `logs` (beside where the project
/// is kept, never under the live folder).
fn run_try(
    task: &dyn EvalTask,
    dir: &Path,
    logs: &Path,
    model: &str,
    effort: &str,
    opts: &Options,
) -> Value {
    let mut row = Map::new();
    row.insert("task".to_owned(), json!(task.id()));
    row.insert("game".to_owned(), json!(task.files()));
    let baseline = match prepare(task, dir) {
        Ok(baseline) => baseline,
        Err(reason) => {
            row.insert("passed_first".to_owned(), json!(false));
            row.insert("passed".to_owned(), json!(false));
            row.insert(
                "agent_error".to_owned(),
                json!(format!("setup failed: {reason}")),
            );
            return Value::Object(row);
        }
    };
    // Kept beside the try, so that a corrected check can be run again later
    // without running the agent again (`wish-evals recheck`).
    let _ = std::fs::remove_dir_all(logs);
    let _ = std::fs::create_dir_all(logs);
    let _ = std::fs::write(logs.join("baseline.json"), baseline.to_string());
    let before = people::scene_entities(dir);
    let limit = if task.id() == long_task::ID {
        LONG_TRY_LIMIT
    } else {
        TRY_LIMIT
    };
    let prompt = format!("{}{CLAIM_SUFFIX}", task.prompt());
    let (mut t, mut seconds) = agent(
        dir,
        &logs.join("run-1.jsonl"),
        &prompt,
        model,
        effort,
        task.files(),
        None,
        limit,
        &opts.deny,
    );
    let mut sessions: Vec<String> = t.session_id.iter().cloned().collect();
    let mut verdict = check(task, dir, &baseline);
    let passed_first = verdict.is_ok();
    let claim_first = claim(t.final_message.as_deref());
    let mut seconds_to_result = passed_first.then_some(seconds);
    let mut rounds = 0;
    while let Err(reason) = &verdict {
        if rounds >= opts.repairs || t.agent_error.is_some() {
            break;
        }
        let Some(session) = t.session_id.clone() else {
            break;
        };
        rounds += 1;
        let feedback = format!(
            "A check of the result failed: {reason}\nFix the project so that the task is done.{CLAIM_SUFFIX}"
        );
        let (again, more) = agent(
            dir,
            &logs.join(format!("run-{}.jsonl", rounds + 1)),
            &feedback,
            model,
            effort,
            task.files(),
            Some(&session),
            limit,
            &opts.deny,
        );
        sessions.extend(again.session_id.clone());
        t.absorb(&again);
        seconds += more;
        verdict = check(task, dir, &baseline);
        if verdict.is_ok() {
            seconds_to_result = Some(seconds);
        }
    }
    let passed = verdict.is_ok();
    let claim_final = claim(t.final_message.as_deref());
    row.insert("passed_first".to_owned(), json!(passed_first));
    row.insert("passed".to_owned(), json!(passed));
    if !opts.held_out {
        row.insert(
            "reason".to_owned(),
            json!(verdict.err().unwrap_or_default()),
        );
    }
    row.insert("claim_first".to_owned(), json!(claim_first.name()));
    row.insert("claim_final".to_owned(), json!(claim_final.name()));
    row.insert("repair_rounds".to_owned(), json!(rounds));
    for (key, value) in [
        ("tool_calls", t.tool_calls),
        ("tool_errors", t.tool_errors),
        ("compile_failures", t.compile_failures),
        ("compile_errors", t.compile_errors),
        ("failed_test_runs", t.failed_test_runs),
        ("repair_turns", t.repair_turns),
        ("turns", t.turns),
        ("input_tokens", t.input_tokens),
        ("output_tokens", t.output_tokens),
        ("cache_read_tokens", t.cache_read_tokens),
        ("cache_write_tokens", t.cache_write_tokens),
    ] {
        row.insert(key.to_owned(), json!(value));
    }
    row.insert("first_call_error".to_owned(), json!(t.first_call_error));
    row.insert("cost_usd".to_owned(), json!(t.cost_usd));
    row.insert("cost_estimated".to_owned(), json!(t.cost_estimated));
    row.insert("model_ids".to_owned(), json!(t.model_ids));
    row.insert("seconds".to_owned(), json!(round1(seconds)));
    row.insert(
        "seconds_to_result".to_owned(),
        json!(seconds_to_result.map(round1)),
    );
    row.insert("agent_error".to_owned(), json!(t.agent_error));
    // 3. Quality: check on the result as it is.
    match measures::quality(dir) {
        Ok(quality) => {
            row.insert("check_errors".to_owned(), json!(quality.errors));
            row.insert("check_warnings".to_owned(), json!(quality.warnings));
        }
        // Not clean: the card counts a check that could not run.
        Err(reason) => {
            row.insert("check_errors".to_owned(), Value::Null);
            if !opts.held_out {
                row.insert("check_error".to_owned(), json!(reason));
            }
        }
    }
    // 1, 4 and 6 on a copy of a passed result (the measures write files).
    let mut replay = Value::Null;
    let mut people_ok = Value::Null;
    let mut tick = Value::Null;
    if passed {
        let copy = beside(dir, "measures");
        if people::copy_project(dir, &copy).is_ok() {
            if task.files() {
                replay = match measures::replay_holds(&copy) {
                    Ok(Some(held)) => json!(held),
                    Ok(None) => Value::Null,
                    Err(_) => json!(false),
                };
                match measures::speed(&copy) {
                    Ok(measures::Speed::Measured { ms_per_tick, .. }) => tick = json!(ms_per_tick),
                    Ok(measures::Speed::NotMeaningful(why)) => {
                        if !opts.held_out {
                            row.insert("tick_note".to_owned(), json!(why));
                        }
                    }
                    // A trap or a failure at 10,000 entities is a result.
                    Err(reason) => {
                        row.insert(
                            "tick_error".to_owned(),
                            json!(if opts.held_out {
                                "failed".to_owned()
                            } else {
                                reason
                            }),
                        );
                    }
                }
            }
            let edited = people::edit_by_hand(&copy, &before);
            people_ok = json!(edited.is_ok());
            if !opts.held_out {
                row.insert(
                    "people_note".to_owned(),
                    json!(edited.unwrap_or_else(|e| e)),
                );
            }
            let _ = std::fs::remove_dir_all(&copy);
        }
    }
    row.insert("replay".to_owned(), replay);
    row.insert("people".to_owned(), people_ok);
    row.insert("tick_ms_10k".to_owned(), tick);
    // 3. The second agent's change (optional, spends quota).
    let mut second = Value::Null;
    if opts.second_agent
        && passed
        && let Some(follow) = task.follow_up()
    {
        let prompt = format!("{}{CLAIM_SUFFIX}", follow.prompt());
        let (s, secs) = agent(
            dir,
            &logs.join("second.jsonl"),
            &prompt,
            model,
            effort,
            task.files(),
            None,
            limit,
            &opts.deny,
        );
        sessions.extend(s.session_id.clone());
        let ok = follow_up_check(follow, dir);
        second = json!({
            "passed": ok.is_ok(),
            "claim": claim(s.final_message.as_deref()).name(),
            "cost_usd": s.cost_usd, "cost_estimated": s.cost_estimated,
            "tokens": s.tokens(), "output_tokens": s.output_tokens,
            "tool_calls": s.tool_calls, "tool_errors": s.tool_errors,
            "seconds": round1(secs), "agent_error": s.agent_error,
        });
        if !opts.held_out {
            second["reason"] = json!(ok.err().unwrap_or_default());
        }
    }
    row.insert("second".to_owned(), second);
    row.insert("session_ids".to_owned(), json!(sessions));
    Value::Object(row)
}

/// What a try cost in all (its runs and the second agent's).
fn row_cost(row: &Value) -> f64 {
    row["cost_usd"].as_f64().unwrap_or(0.0) + row["second"]["cost_usd"].as_f64().unwrap_or(0.0)
}

/// A label is a plain name: it becomes a folder and a file name.
fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 80
        && !label.contains("..")
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(held_out::repository_or_here())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Today's date (UTC) as YYYY-MM-DD.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The machine, without anything personal (no user or host name).
fn machine() -> String {
    let cpu = if cfg!(windows) {
        std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default()
    } else {
        std::fs::read_to_string("/proc/cpuinfo")
            .unwrap_or_default()
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split(':').nth(1))
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let threads = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    format!(
        "{} {}, {threads} threads, {cpu}, software render adapter",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

fn client_version() -> String {
    claude_command()
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|v| !v.is_empty())
        .map_or("Claude Code CLI (version unknown)".to_owned(), |v| {
            format!("Claude Code CLI {v}")
        })
}

fn results_dir() -> PathBuf {
    held_out::repository_or_here().join("evals").join("results")
}

fn write_card(label: &str, card: &Value) -> Result<(PathBuf, PathBuf), String> {
    let folder = results_dir();
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let json_path = folder.join(format!("{label}.json"));
    let md_path = folder.join(format!("{label}.md"));
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(card).unwrap_or_default() + "\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(&md_path, card::markdown(card)).map_err(|e| e.to_string())?;
    Ok((json_path, md_path))
}

/// The order of a campaign's tries, as (model, task, try): the long task's
/// first try with the first model runs first (it is the largest unknown of
/// the cost; with `--max-cost` a first measured long try shows early whether
/// the cap holds), then task by task, each try with every model in turn, so
/// that a stop at the cap leaves every model on the same tasks (the gap
/// compares like with like).
fn schedule(
    tasks: usize,
    long: Option<usize>,
    models: usize,
    tries: u32,
) -> Vec<(usize, usize, u32)> {
    let mut plan = Vec::new();
    if let Some(long) = long
        && models > 0
        && tries > 0
    {
        plan.push((0, long, 1));
    }
    for t in 0..tasks {
        for attempt in 1..=tries {
            for m in 0..models {
                if !(m == 0 && Some(t) == long && attempt == 1) {
                    plan.push((m, t, attempt));
                }
            }
        }
    }
    plan
}

/// `folder` with `.extension` added to its name (a label may hold dots, so
/// not `with_extension`).
fn beside(folder: &Path, extension: &str) -> PathBuf {
    let name = folder
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    folder.with_file_name(format!("{name}.{extension}"))
}

/// Moves a finished try's project from its live folder to where it is kept.
fn keep(live: &Path, kept: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(kept);
    if let Some(parent) = kept.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if std::fs::rename(live, kept).is_ok() {
        return Ok(());
    }
    // Another volume: copy, then remove the live folder.
    people::copy_project(live, kept)?;
    std::fs::remove_dir_all(live).map_err(|e| e.to_string())
}

/// A campaign: every task, every model, `tries` tries; then the scorecard.
/// The order is [`schedule`]'s. With `--max-cost`, the campaign stops
/// between tries when the cost so far plus the mean cost of a try so far
/// would pass the cap, and the card says so; what was measured is kept.
fn campaign(
    tasks: &[&dyn EvalTask],
    models: &[(String, String)],
    tries: u32,
    label: &str,
    opts: &Options,
    command: &str,
) -> bool {
    let run_dir = work().join(label);
    // A try's project lives in a folder of its own while the agent works.
    let live = live_root();
    let _ = std::fs::remove_dir_all(&live);
    let long = tasks.iter().position(|t| t.id() == long_task::ID);
    let plan = schedule(tasks.len(), long, models.len(), tries);
    let total = plan.len();
    let mut rows: Vec<Vec<Value>> = vec![Vec::new(); models.len()];
    let mut resolved: Vec<Vec<String>> = vec![Vec::new(); models.len()];
    let mut spent = 0.0f64;
    let mut done = 0usize;
    let mut stopped_at_cap = false;
    let client = held_out::client_folder();
    for &(m, t, attempt) in &plan {
        if let Some(cap) = opts.max_cost {
            let mean = if done > 0 { spent / done as f64 } else { 0.0 };
            if spent + mean > cap {
                stopped_at_cap = true;
                println!(
                    "stopped at the cost cap: ${spent:.2} spent after {done} of {total} tries; the next would pass ${cap:.2}"
                );
                break;
            }
        }
        let (model, effort) = &models[m];
        let task = tasks[t];
        let name = format!(
            "{}-{attempt}",
            if opts.held_out {
                format!("h{done}")
            } else {
                task.id().to_owned()
            }
        );
        // Kept here after the try (with its transcripts beside it); worked
        // in at `dir`, which holds nothing else.
        let kept = run_dir.join(model).join(&name);
        let dir = live.join(format!("{label}-{model}-{name}"));
        let logs = logs_of(&kept);
        let mut row = run_try(task, &dir, &logs, model, effort, opts);
        done += 1;
        spent += row_cost(&row);
        for id in row["model_ids"].as_array().into_iter().flatten() {
            if let Some(id) = id.as_str()
                && !resolved[m].iter().any(|r| r == id)
            {
                resolved[m].push(id.to_owned());
            }
        }
        if opts.held_out {
            // Nothing of the task is kept: not its project, not its
            // transcripts, not its failure, and not what the client kept of
            // its sessions (they hold the prompt and the repair message).
            let sessions: Vec<String> = row["session_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect();
            if let Some(client) = &client {
                held_out::forget_sessions(client, &dir, &sessions);
            }
            if let Some(map) = row.as_object_mut() {
                map.remove("session_ids");
                map.remove("model_ids");
                map.insert("task".to_owned(), json!(format!("h{t}")));
            }
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&logs);
            println!("held-out: try {done} of {total} done");
        } else {
            if let Err(reason) = keep(&dir, &kept) {
                println!("could not keep the try's project: {reason}");
            }
            println!(
                "{} {model} {}#{attempt}: first {}, {} calls ({} errors, {} failed builds, {} repair turns), {} rounds, {:.0}s, ${:.3}{}",
                if row["passed"] == true {
                    "PASS"
                } else {
                    "FAIL"
                },
                task.id(),
                if row["passed_first"] == true {
                    "pass"
                } else {
                    "fail"
                },
                row["tool_calls"],
                row["tool_errors"],
                row["compile_failures"],
                row["repair_turns"],
                row["repair_rounds"],
                row["seconds"].as_f64().unwrap_or(0.0),
                row_cost(&row),
                row["reason"]
                    .as_str()
                    .filter(|r| !r.is_empty())
                    .map_or(String::new(), |r| format!(" -- {r}")),
            );
        }
        rows[m].push(row);
    }
    if opts.held_out {
        let _ = std::fs::remove_dir_all(&run_dir);
    }
    let _ = std::fs::remove_dir_all(&live);
    let runs: Vec<(String, String, Vec<Value>)> = models
        .iter()
        .zip(rows)
        .map(|((model, effort), rows)| (model.clone(), effort.clone(), rows))
        .collect();
    let inputs = json!({
        "label": label,
        "set": if opts.held_out { "held-out" } else { "open" },
        "tasks": tasks.len(),
        "tries_per_task": tries,
        "repair_rounds": opts.repairs,
        "second_agent": opts.second_agent,
        "engine_version": env!("CARGO_PKG_VERSION"),
        "engine_commit": git(&["rev-parse", "HEAD"]),
        "engine_dirty": git(&["status", "--porcelain"]).map(|s| !s.is_empty()),
        "date": today(),
        "machine": machine(),
        "client": client_version(),
        "models": models.iter().map(|(m, e)| json!({ "model": m, "effort": e })).collect::<Vec<_>>(),
        "resolved_models": models.iter().zip(&resolved)
            .map(|((m, _), ids)| format!("{m} = {}", if ids.is_empty() { "unknown".to_owned() } else { ids.join(" + ") }))
            .collect::<Vec<_>>().join("; "),
        "command": command,
        "method": {
            "claim_sentence": CLAIM_SUFFIX.trim(),
            "repair_rounds": opts.repairs,
            "repair_feedback": "the hidden check's reason, in the resumed session",
            "second_agent": opts.second_agent,
            "order": if long.is_some() { "the long task's first try with the first model, then task by task in order, each try with every model in turn (a stop at the cost cap leaves the models on the same tasks)" } else { "task by task in order, each try with every model in turn (a stop at the cost cap leaves the models on the same tasks)" },
            "max_cost_usd": opts.max_cost,
            "stopped_at_cap": stopped_at_cap,
            "tries_run": done,
            "tries_planned": total,
            "spent_usd": (spent * 1000.0).round() / 1000.0,
            "client_sessions": if opts.held_out { "deleted after each try" } else { "kept by the client" },
        },
    });
    let card = card::scorecard(&inputs, &runs, !opts.held_out);
    for c in card["cards"].as_array().into_iter().flatten() {
        println!(
            "{} · {}: first attempt {}, in the end {}, believed right but refuted {}, ${} per pass",
            c["model"].as_str().unwrap_or_default(),
            c["effort"].as_str().unwrap_or_default(),
            c["1_correctness"]["first_attempt_rate"],
            c["1_correctness"]["final_rate"],
            c["2_errors"]["false_successes_first"]["count"],
            c["5_efficiency"]["cost_per_pass_usd"],
        );
    }
    match write_card(label, &card) {
        Ok((json_path, md_path)) => {
            println!(
                "scorecard: {} and {}",
                json_path.display(),
                md_path.display()
            );
            true
        }
        Err(reason) => {
            eprintln!("could not write the scorecard: {reason}");
            false
        }
    }
}

// --------------------------------------------------------------------------

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .filter(|v| !v.starts_with("--"))
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// The held-out folder of `--held-out [dir]`, if the flag is given.
fn held_out_folder(args: &[String]) -> Option<PathBuf> {
    has(args, "--held-out").then(|| {
        flag(args, "--held-out")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(held_out::DEFAULT_FOLDER))
    })
}

fn exit(ok: bool) -> std::process::ExitCode {
    if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut open = open_set();
    let filtered = flag(&args, "--tasks").is_some();
    if let Some(wanted) = flag(&args, "--tasks") {
        let wanted: Vec<&str> = wanted.split(',').collect();
        open.retain(|task| wanted.iter().any(|w| task.id.starts_with(w)));
    }
    let open_refs: Vec<&dyn EvalTask> = open.iter().map(|t| t as &dyn EvalTask).collect();
    let held = held_out_folder(&args);
    match args.first().map(String::as_str) {
        Some("selftest") => {
            if let Some(file) = flag(&args, "--task-file") {
                let path = Path::new(file);
                // Never details of a held-out task: the default folder and
                // any folder given with --held-out, at any depth.
                let mut folders = vec![PathBuf::from(held_out::DEFAULT_FOLDER)];
                folders.extend(held.clone());
                if held_out::in_held_out(path, &folders) {
                    println!(
                        "refused: this file is in a held-out folder (its details are never printed)"
                    );
                    return exit(false);
                }
                let task = std::fs::read_to_string(path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| DeclaredTask::parse(&t));
                return match task {
                    Ok(task) => {
                        let dir = work().join("selftest").join("task-file");
                        let outcome = selftest_task(&task, &dir, &mut Tally::default());
                        match &outcome {
                            Ok(notes) => println!("ok    {}  [{notes}]", task.id()),
                            Err(reason) => println!("FAIL  {}: {reason}", task.id()),
                        }
                        exit(outcome.is_ok())
                    }
                    Err(reason) => {
                        println!("FAIL  {file}: {reason}");
                        exit(false)
                    }
                };
            }
            if let Some(folder) = held {
                return exit(selftest_held_out(&folder));
            }
            exit(selftest(
                &open_refs,
                !filtered || open.iter().any(|t| t.id == long_task::ID),
            ))
        }
        Some("recheck") => {
            // Checks the projects a run left behind again (after a check was
            // corrected); the agent is not run.
            let label = flag(&args, "--label").unwrap_or("run");
            let run_dir = work().join(label);
            let mut passed = 0;
            let mut total = 0;
            let mut folders = vec![run_dir.clone()];
            folders.extend(
                std::fs::read_dir(&run_dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir()),
            );
            for task in &open {
                for folder in &folders {
                    for attempt in 1.. {
                        let dir = folder.join(format!("{}-{attempt}", task.id));
                        if !dir.is_dir() {
                            break;
                        }
                        let baseline = std::fs::read_to_string(logs_of(&dir).join("baseline.json"))
                            .or_else(|_| std::fs::read_to_string(dir.join("baseline.json")))
                            .ok()
                            .and_then(|text| serde_json::from_str(&text).ok())
                            .unwrap_or(Value::Null);
                        total += 1;
                        match check(task, &dir, &baseline) {
                            Ok(()) => {
                                passed += 1;
                                println!("PASS {}#{attempt}", task.id);
                            }
                            Err(reason) => println!("FAIL {}#{attempt} -- {reason}", task.id),
                        }
                    }
                }
            }
            println!("\n{passed}/{total} pass the current checks");
            exit(true)
        }
        Some("rescore") => {
            // Reads a finished run's transcripts again (free).
            let label = flag(&args, "--label").unwrap_or("run");
            let run_dir = work().join(label);
            let mut found: Vec<PathBuf> = Vec::new();
            let mut stack = vec![run_dir];
            while let Some(folder) = stack.pop() {
                for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n == "transcript.jsonl"
                            || n == "second.jsonl"
                            || (n.starts_with("run-") && n.ends_with(".jsonl"))
                    }) {
                        // One run per file (the M4 to M7 runs kept one
                        // `transcript.jsonl` per try).
                        found.push(path);
                    }
                }
            }
            found.sort();
            let mut sum = Transcript::default();
            let mut claims = std::collections::BTreeMap::<&str, u32>::new();
            for path in &found {
                let lines: Vec<String> = std::fs::read_to_string(path)
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect();
                let t = transcript::read(&lines);
                let said = claim(t.final_message.as_deref());
                *claims.entry(said.name()).or_default() += 1;
                println!(
                    "{}/{}: {} calls, {} errors, {} failed builds ({} compiler errors), {} failed test runs, {} repair turns, claim {}",
                    path.parent()
                        .and_then(Path::file_name)
                        .unwrap_or_default()
                        .to_string_lossy(),
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    t.tool_calls,
                    t.tool_errors,
                    t.compile_failures,
                    t.compile_errors,
                    t.failed_test_runs,
                    t.repair_turns,
                    said.name()
                );
                sum.absorb(&t);
            }
            println!(
                "\n{} transcripts: {} calls, {} errors, {} failed builds, {} failed test runs, {} repair turns; claims {:?}; ${:.2}",
                found.len(),
                sum.tool_calls,
                sum.tool_errors,
                sum.compile_failures,
                sum.failed_test_runs,
                sum.repair_turns,
                claims,
                sum.cost_usd
            );
            exit(true)
        }
        Some("card") => {
            let Some(from) = flag(&args, "--from") else {
                eprintln!("usage: wish-evals card --from evals/results/<label>.json");
                return exit(false);
            };
            let card = std::fs::read_to_string(from)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok());
            match card {
                Some(card) if card["scorecard"] == 1 => {
                    let md = Path::new(from).with_extension("md");
                    let ok = std::fs::write(&md, card::markdown(&card)).is_ok();
                    println!("{}", md.display());
                    exit(ok)
                }
                _ => {
                    eprintln!("{from} is not a scorecard");
                    exit(false)
                }
            }
        }
        Some("estimate") => {
            let all = open_set();
            let ids: Vec<String> = all
                .iter()
                .filter(|t| t.id != long_task::ID)
                .map(|t| t.id.to_owned())
                .collect();
            let game: Vec<String> = all
                .iter()
                .filter(|t| t.files && t.id != long_task::ID)
                .map(|t| t.id.to_owned())
                .collect();
            let follow: Vec<String> = followups::ALL.iter().map(|f| f.task.to_owned()).collect();
            print!(
                "{}",
                estimate::estimate(&results_dir(), &ids, &game, &follow, long_task::ID)
            );
            exit(true)
        }
        Some(command @ ("campaign" | "run")) => {
            let tries: u32 = flag(&args, "--tries")
                .and_then(|t| t.parse().ok())
                .unwrap_or(3);
            let repairs: u32 = flag(&args, "--repairs")
                .and_then(|t| t.parse().ok())
                .unwrap_or(1);
            let label = flag(&args, "--label").unwrap_or("run");
            if !valid_label(label) {
                eprintln!(
                    "--label must be a plain name (letters, digits, - _ .; no '..', no folders)"
                );
                return exit(false);
            }
            let existing = results_dir().join(format!("{label}.json"));
            if existing.exists() && !has(&args, "--overwrite") {
                eprintln!(
                    "{} exists: choose another --label, or pass --overwrite",
                    existing.display()
                );
                return exit(false);
            }
            let max_cost = match flag(&args, "--max-cost").map(str::parse::<f64>) {
                None => None,
                Some(Ok(cap)) if cap > 0.0 => Some(cap),
                Some(_) => {
                    eprintln!("--max-cost takes a number of dollars, such as 30");
                    return exit(false);
                }
            };
            let models: Vec<(String, String)> = if command == "run" {
                vec![(
                    flag(&args, "--model").unwrap_or("sonnet").to_owned(),
                    flag(&args, "--effort").unwrap_or("medium").to_owned(),
                )]
            } else {
                flag(&args, "--models")
                    .unwrap_or("sonnet:medium,haiku:medium")
                    .split(',')
                    .map(|m| {
                        let (model, effort) = m.split_once(':').unwrap_or((m, "medium"));
                        (model.to_owned(), effort.to_owned())
                    })
                    .collect()
            };
            let opts = Options {
                repairs,
                second_agent: has(&args, "--second-agent"),
                held_out: held.is_some(),
                max_cost,
                deny: deny_rules(held.as_deref()),
            };
            let held_tasks;
            let tasks: Vec<&dyn EvalTask> = match &held {
                Some(folder) => match held_out::load(folder) {
                    Ok(loaded) => {
                        held_tasks = loaded;
                        held_tasks.iter().map(|t| t as &dyn EvalTask).collect()
                    }
                    Err(reason) => {
                        eprintln!("{reason}");
                        return exit(false);
                    }
                },
                None => open_refs,
            };
            let line = format!("wish-evals {}", args.join(" "));
            exit(campaign(&tasks, &models, tries, label, &opts, &line))
        }
        _ => {
            eprintln!(
                "usage: wish-evals selftest [--tasks 01,42] [--held-out [dir]] [--task-file f]\n       wish-evals campaign --label name [--models sonnet:medium,haiku:medium] [--tries 3] [--repairs 1] [--second-agent] [--max-cost 30] [--overwrite] [--held-out [dir]]   (spends quota)\n       wish-evals run --label name [--model sonnet] [--effort medium] [--tries 3]   (spends quota)\n       wish-evals card --from file | rescore --label name | recheck --label name | estimate"
            );
            exit(false)
        }
    }
}

#[cfg(test)]
mod launch_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// On Windows the client is a batch file, and the standard library
    /// refuses to start one with a line break in an argument: the first
    /// campaign started no agent at all. The prompt goes in on standard
    /// input, so no argument may carry one.
    #[test]
    fn the_command_line_never_carries_a_line_break() {
        for files in [false, true] {
            for resume in [None, Some("session-id")] {
                let command = agent_command(Path::new("."), "sonnet", "medium", files, resume, &[]);
                for arg in command.get_args() {
                    let arg = arg.to_string_lossy();
                    assert!(
                        !arg.contains(['\n', '\r']),
                        "an argument with a line break: {arg:?}"
                    );
                }
                let args: Vec<String> = command
                    .get_args()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                assert_eq!(args[0], "-p");
                assert_eq!(args[1], "--model", "the prompt is not on the command line");
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wish-evals-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A fake client that prints one line, then starts a child of its own
    /// that keeps the output pipe open and writes `alive.txt` once a second
    /// for about a minute (as the real client's batch file starts node).
    fn fake_client(dir: &Path) -> PathBuf {
        if cfg!(windows) {
            std::fs::write(
                dir.join("loop.cmd"),
                "@echo off\r\nfor /l %%i in (1,1,60) do (\r\n  echo tick>>\"%~dp0alive.txt\"\r\n  ping -n 2 127.0.0.1 >nul\r\n)\r\n",
            )
            .unwrap();
            let client = dir.join("client.cmd");
            std::fs::write(
                &client,
                "@echo off\r\necho {\"type\":\"system\",\"subtype\":\"init\"}\r\ncmd /d /c \"%~dp0loop.cmd\"\r\n",
            )
            .unwrap();
            client
        } else {
            std::fs::write(
                dir.join("loop.sh"),
                "i=0\nwhile [ $i -lt 60 ]; do echo tick >> \"$(dirname \"$0\")/alive.txt\"; sleep 1; i=$((i+1)); done\n",
            )
            .unwrap();
            let client = dir.join("client.sh");
            std::fs::write(
                &client,
                "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\"}'\nsh \"$(dirname \"$0\")/loop.sh\"\n",
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            client
        }
    }

    /// Closing review: on Windows the client is a batch file that starts the
    /// real client as a child. Killing the batch file left the child running
    /// (spending quota, editing the project) and holding the output pipe, so
    /// the harness waited for it far past the try's limit.
    #[test]
    fn a_try_past_its_limit_stops_the_whole_client() {
        let dir = scratch("timeout");
        let client = fake_client(&dir);
        let command = agent_command_with(
            client_command(Some(client.into_os_string())),
            &dir,
            "sonnet",
            "medium",
            false,
            None,
            &[],
        );
        let limit = Duration::from_secs(3);
        let started = Instant::now();
        let (t, seconds) = drive(command, &dir.join("log.jsonl"), "prompt\n", "sonnet", limit);
        let took = started.elapsed();
        assert!(
            took < limit + Duration::from_secs(15),
            "the harness waited {took:?} for a try limited to {limit:?}"
        );
        assert_eq!(t.agent_error.as_deref(), Some("timed out"));
        assert!(seconds >= 3.0, "{seconds}");
        // The client's child is gone: its file stops growing.
        let alive = dir.join("alive.txt");
        let size = || std::fs::metadata(&alive).map_or(0, |m| m.len());
        let before = size();
        std::thread::sleep(Duration::from_secs(4));
        assert_eq!(size(), before, "the client's child still runs");
        let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
        assert!(log.contains("\"init\""), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Closing review of 1.0: an agent with file tools could read other
    /// tries, the other model's solutions, transcripts with the hidden
    /// check's reason, the held-out folder and the repository. Each try now
    /// works in a folder of its own, and the client is told not to read or
    /// edit those places; the try's own folder is under none of them.
    #[test]
    fn the_agent_may_not_read_other_tries_the_checks_or_the_held_out_set() {
        let held = PathBuf::from("D:/somewhere/held");
        let rules = deny_rules(Some(&held));
        for folder in [
            work(),
            PathBuf::from(held_out::DEFAULT_FOLDER),
            held.clone(),
            held_out::repository().unwrap(),
        ] {
            let path = rule_path(&folder).unwrap();
            assert!(path.starts_with("//"), "{path}");
            for tool in ["Read", "Edit"] {
                let rule = format!("{tool}({path}/**)");
                assert!(rules.contains(&rule), "{rule} missing from {rules:?}");
            }
        }
        if cfg!(windows) {
            assert!(rules.contains(&"Read(//c/wish-build/evals/**)".to_owned()));
            assert!(rules.contains(&"Read(//d/somewhere/held/**)".to_owned()));
        }
        // The try's own folder is under no denied folder.
        let live = rule_path(&live_root().join("label-sonnet-01_task-1")).unwrap();
        for rule in &rules {
            let denied = rule
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix("/**)"))
                .unwrap();
            assert!(
                !(live == denied || live.starts_with(&format!("{denied}/"))),
                "the live folder {live} is under {rule}"
            );
        }
        // Every rule reaches the client, after --disallowedTools.
        for files in [false, true] {
            let command = agent_command(Path::new("."), "sonnet", "medium", files, None, &rules);
            let args: Vec<String> = command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let from = args.iter().position(|a| a == "--disallowedTools").unwrap();
            let until = args.iter().position(|a| a == "--setting-sources").unwrap();
            for rule in &rules {
                assert!(args[from..until].contains(rule), "{rule}");
            }
        }
    }

    #[test]
    fn rule_paths_are_the_clients_absolute_form() {
        if cfg!(windows) {
            assert_eq!(
                rule_path(Path::new(r"C:\wish-build\evals")).as_deref(),
                Some("//c/wish-build/evals")
            );
            assert_eq!(
                rule_path(Path::new(r"\\?\D:\x\y\")).as_deref(),
                Some("//d/x/y")
            );
        } else {
            assert_eq!(rule_path(Path::new("/home/x")).as_deref(), Some("//home/x"));
        }
    }

    /// Closing review of 1.0: model by model, a stop at the cost cap left
    /// the second model on fewer tasks than the first, and the gap compared
    /// different task sets. Task by task, every model runs each try in turn.
    #[test]
    fn the_schedule_runs_every_model_on_a_task_before_the_next() {
        let plan = schedule(3, Some(2), 2, 2);
        assert_eq!(plan[0], (0, 2, 1), "the long task's first try first");
        assert_eq!(plan.len(), 3 * 2 * 2);
        assert_eq!(&plan[1..5], &[(0, 0, 1), (1, 0, 1), (0, 0, 2), (1, 0, 2)]);
        let mut sorted = plan.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), plan.len(), "no try twice");
        assert_eq!(schedule(2, None, 1, 1), vec![(0, 0, 1), (0, 1, 1)]);
    }

    #[test]
    fn a_label_with_dots_keeps_its_measures_beside_the_try() {
        let dir = Path::new("C:/x/baseline-1.0-small-sonnet-01_a-1");
        assert_eq!(
            beside(dir, "measures"),
            Path::new("C:/x/baseline-1.0-small-sonnet-01_a-1.measures")
        );
    }
}
