//! `wish-evals`: agent evaluations (ADR-017 layer 4, design M4 §8).
//!
//! ```text
//! wish-evals selftest                    every check fails on the prepared project and passes
//!                                        after the reference solution (no AI, free)
//! wish-evals run [--tasks 01,03] [--tries 3] [--model sonnet] [--effort medium]
//!                                        each try: a fresh project, `claude -p` with only the
//!                                        project's MCP tools, then the check; results in
//!                                        evals/results/
//! ```

mod game_tasks;
mod m6_tasks;
mod probe;
mod tasks;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::probe::Probe;
use crate::tasks::Task;

/// Where the throwaway projects go (outside the repository).
const WORK: &str = "C:/wish-build/evals";
/// The longest one try may take.
const TRY_LIMIT: Duration = Duration::from_secs(15 * 60);

fn wen_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(if cfg!(windows) { "wen.exe" } else { "wen" })
}

/// A fresh project for a task: `wen new`, MCP on the software adapter, setup.
fn prepare(task: &Task, dir: &Path) -> Result<Value, String> {
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
    (task.setup)(dir)?;
    let mut probe = Probe::open(dir)?;
    (task.baseline)(&mut probe)
}

fn check(task: &Task, dir: &Path, baseline: &Value) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    (task.check)(&mut probe, dir, baseline)
}

fn selftest(tasks: &[Task]) -> bool {
    let mut all_good = true;
    for task in tasks {
        let dir = Path::new(WORK).join("selftest").join(task.id);
        let outcome = (|| -> Result<(), String> {
            let baseline = prepare(task, &dir)?;
            if check(task, &dir, &baseline).is_ok() {
                return Err("the check passes before anything is done".to_owned());
            }
            let mut probe = Probe::open(&dir)?;
            (task.reference)(&mut probe)?;
            drop(probe);
            check(task, &dir, &baseline)
                .map_err(|reason| format!("the reference solution fails: {reason}"))
        })();
        match outcome {
            Ok(()) => println!("ok    {}", task.id),
            Err(reason) => {
                all_good = false;
                println!("FAIL  {}: {reason}", task.id);
            }
        }
    }
    all_good
}

/// What one try of a task did.
#[derive(Default)]
struct Try {
    passed: bool,
    reason: String,
    tool_calls: u64,
    tool_errors: u64,
    first_call_error: bool,
    turns: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    cost_usd: f64,
    seconds: f64,
    agent_error: Option<String>,
}

fn claude_command() -> Command {
    Command::new(if cfg!(windows) {
        "claude.cmd"
    } else {
        "claude"
    })
}

fn run_try(task: &Task, dir: &Path, model: &str, effort: &str) -> Try {
    let mut result = Try::default();
    let baseline = match prepare(task, dir) {
        Ok(baseline) => baseline,
        Err(reason) => {
            result.reason = format!("setup failed: {reason}");
            return result;
        }
    };
    // Kept with the try, so that a corrected check can be run again later
    // without running the agent again (`wish-evals recheck`).
    let _ = std::fs::write(dir.join("baseline.json"), baseline.to_string());
    let started = Instant::now();
    let child = claude_command()
        .current_dir(dir)
        .args(["-p", task.prompt, "--model", model, "--effort", effort])
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--mcp-config", ".mcp.json", "--strict-mcp-config"])
        // Game code tasks may read and edit the project's files; the others
        // have the engine's tools only. Bash is never allowed.
        .args(if task.files {
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
        .args(if task.files {
            &["Bash"][..]
        } else {
            &["Bash", "Edit", "Write", "Read", "Glob", "Grep"][..]
        })
        .args(["NotebookEdit", "WebFetch", "WebSearch", "Task", "TodoWrite"])
        .args(["--setting-sources", "project"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            result.reason = format!("could not start claude: {error}");
            return result;
        }
    };
    let stdout = child.stdout.take();
    let transcript_path = dir.join("transcript.jsonl");
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                lines.push(line);
            }
        }
        lines
    });
    // Wait, but not for ever.
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > TRY_LIMIT => {
                let _ = child.kill();
                result.agent_error = Some("timed out".to_owned());
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(error) => {
                result.agent_error = Some(error.to_string());
                break;
            }
        }
    }
    result.seconds = started.elapsed().as_secs_f64();
    let lines = reader.join().unwrap_or_default();
    let _ = std::fs::write(&transcript_path, lines.join("\n"));
    let mut tool_ids: Vec<String> = Vec::new();
    for line in &lines {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["type"].as_str() {
            Some("assistant") => {
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_use" {
                        result.tool_calls += 1;
                        tool_ids.push(block["id"].as_str().unwrap_or_default().to_owned());
                    }
                }
            }
            Some("user") => {
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" && block["is_error"] == true {
                        result.tool_errors += 1;
                        if tool_ids.first().map(String::as_str) == block["tool_use_id"].as_str() {
                            result.first_call_error = true;
                        }
                    }
                }
            }
            Some("result") => {
                result.turns = event["num_turns"].as_u64().unwrap_or(0);
                result.cost_usd = event["total_cost_usd"].as_f64().unwrap_or(0.0);
                let usage = &event["usage"];
                result.input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
                result.output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
                result.cache_read_tokens = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                result.cache_write_tokens =
                    usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                if event["is_error"] == true {
                    result.agent_error = Some(
                        event["result"]
                            .as_str()
                            .unwrap_or("error")
                            .chars()
                            .take(300)
                            .collect(),
                    );
                }
            }
            _ => {}
        }
    }
    match check(task, dir, &baseline) {
        Ok(()) => result.passed = true,
        Err(reason) => result.reason = reason,
    }
    result
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut tasks = tasks::all();
    tasks.extend(m6_tasks::all());
    if let Some(wanted) = flag(&args, "--tasks") {
        let wanted: Vec<&str> = wanted.split(',').collect();
        tasks.retain(|task| wanted.iter().any(|w| task.id.starts_with(w)));
    }
    match args.first().map(String::as_str) {
        Some("selftest") => {
            if selftest(&tasks) {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Some("recheck") => {
            // Checks the projects a run left behind again (after a check was
            // corrected); the agent is not run.
            let label = flag(&args, "--label").unwrap_or("run");
            let run_dir = Path::new(WORK).join(label);
            let mut passed = 0;
            let mut total = 0;
            for task in &tasks {
                for attempt in 1.. {
                    let dir = run_dir.join(format!("{}-{attempt}", task.id));
                    if !dir.is_dir() {
                        break;
                    }
                    let baseline = std::fs::read_to_string(dir.join("baseline.json"))
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
            println!("\n{passed}/{total} pass the current checks");
            std::process::ExitCode::SUCCESS
        }
        Some("run") => {
            let tries: u32 = flag(&args, "--tries")
                .and_then(|t| t.parse().ok())
                .unwrap_or(3);
            let model = flag(&args, "--model").unwrap_or("sonnet");
            let effort = flag(&args, "--effort").unwrap_or("medium");
            let label = flag(&args, "--label").unwrap_or("run");
            let run_dir = Path::new(WORK).join(label);
            let mut rows = Vec::new();
            let (mut passed, mut total, mut cost) = (0u32, 0u32, 0.0f64);
            for task in &tasks {
                for attempt in 1..=tries {
                    let dir = run_dir.join(format!("{}-{attempt}", task.id));
                    let r = run_try(task, &dir, model, effort);
                    total += 1;
                    passed += u32::from(r.passed);
                    cost += r.cost_usd;
                    println!(
                        "{} {}#{attempt}: {} calls ({} errors{}), {} turns, {:.0}s, ${:.3}{}",
                        if r.passed { "PASS" } else { "FAIL" },
                        task.id,
                        r.tool_calls,
                        r.tool_errors,
                        if r.first_call_error {
                            ", first call failed"
                        } else {
                            ""
                        },
                        r.turns,
                        r.seconds,
                        r.cost_usd,
                        if r.passed {
                            String::new()
                        } else {
                            format!(" -- {}", r.reason)
                        }
                    );
                    rows.push(json!({
                        "task": task.id, "try": attempt, "passed": r.passed, "reason": r.reason,
                        "tool_calls": r.tool_calls, "tool_errors": r.tool_errors, "first_call_error": r.first_call_error,
                        "turns": r.turns, "seconds": (r.seconds * 10.0).round() / 10.0,
                        "input_tokens": r.input_tokens, "output_tokens": r.output_tokens,
                        "cache_read_tokens": r.cache_read_tokens, "cache_write_tokens": r.cache_write_tokens,
                        "cost_usd": r.cost_usd, "agent_error": r.agent_error,
                    }));
                }
            }
            let summary = json!({
                "model": model, "effort": effort, "tries_per_task": tries,
                "passed": passed, "total": total,
                "pass_rate": if total > 0 { f64::from(passed) / f64::from(total) } else { 0.0 },
                "cost_usd_equivalent": (cost * 1000.0).round() / 1000.0,
                "tries": rows,
            });
            println!(
                "\n{passed}/{total} passed ({:.0}%), API-equivalent cost ${cost:.2}",
                100.0 * f64::from(passed) / f64::from(total.max(1))
            );
            let out = flag(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(format!("evals/results/{label}.json")));
            if let Some(folder) = out.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = std::fs::write(
                &out,
                serde_json::to_string_pretty(&summary).unwrap_or_default() + "\n",
            );
            println!("results: {}", out.display());
            std::process::ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: wish-evals selftest | run [--tasks 01,02] [--tries 3] [--model sonnet] [--effort medium] [--label name]"
            );
            std::process::ExitCode::FAILURE
        }
    }
}
