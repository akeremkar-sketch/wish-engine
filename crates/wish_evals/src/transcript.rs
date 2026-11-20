//! Reading what an agent did from its Claude Code transcript
//! (`claude -p --output-format stream-json --verbose`): calls, errors,
//! compile failures, repair turns, tokens, cost, and what the agent said at
//! the end. Everything here is a pure function of the transcript's lines,
//! so a finished run can be read again (`wish-evals rescore`) and the same
//! lines always give the same numbers.

use std::collections::BTreeMap;

use serde_json::Value;

/// Appended to every prompt so that the agent's own verdict can be read
/// without guessing from free text (the scorecard's "believed right").
pub const CLAIM_SUFFIX: &str = "\n\nWhen you are finished, end your last message with one line of its own: `RESULT: DONE` if you believe the task is done and works, or `RESULT: NOT DONE` followed by what is missing.";

/// What the agent said about its own result in its last message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    /// `RESULT: DONE`.
    Done,
    /// `RESULT: NOT DONE ...`.
    NotDone,
    /// A last message without a `RESULT:` line. Counted as a claim of
    /// success: an agent that stops without saying something is missing is
    /// read by a person as done.
    Unstated,
    /// No last message at all (the agent failed, was cut off, or timed out).
    NoAnswer,
}

impl Claim {
    /// The name in result files.
    pub fn name(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::NotDone => "not_done",
            Self::Unstated => "unstated",
            Self::NoAnswer => "no_answer",
        }
    }

    /// Whether this counts as the agent saying the task is done.
    pub fn claims_success(self) -> bool {
        matches!(self, Self::Done | Self::Unstated)
    }

    /// Reads a name back.
    pub fn from_name(name: &str) -> Self {
        match name {
            "done" => Self::Done,
            "not_done" => Self::NotDone,
            "unstated" => Self::Unstated,
            _ => Self::NoAnswer,
        }
    }
}

/// Reads the claim from the agent's last message: the **last** line that
/// starts with `RESULT:` (case, surrounding spaces, backticks, asterisks, a
/// leading `#`/`>` and list markers `-`, `*`, `•`, `1.` are ignored).
/// Exactly `DONE` or `DONE.` is a claim of success; anything starting with
/// `NOT` is not; any other word, or no such line, is `Unstated`.
pub fn claim(final_message: Option<&str>) -> Claim {
    let Some(text) = final_message.filter(|t| !t.trim().is_empty()) else {
        return Claim::NoAnswer;
    };
    let line = text
        .lines()
        .map(|l| {
            let l = l.trim().trim_start_matches(['#', '>']).trim_start();
            // A list marker before the line: "-", "•", or "1." / "1)"
            // ("*" goes with the emphasis below).
            let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let l = l
                .strip_prefix(['-', '•'])
                .or_else(|| {
                    (digits > 0)
                        .then(|| l[digits..].strip_prefix(['.', ')']))
                        .flatten()
                })
                .unwrap_or(l);
            l.trim_matches(|c: char| c == '`' || c == '*' || c == '_' || c.is_whitespace())
                .to_ascii_uppercase()
        })
        .rfind(|l| l.starts_with("RESULT:") || l.starts_with("RESULT :"));
    let Some(line) = line else {
        return Claim::Unstated;
    };
    let verdict = line
        .split_once(':')
        .map(|(_, v)| v)
        .unwrap_or_default()
        .trim_matches(|c: char| c == '`' || c == '*' || c.is_whitespace());
    if verdict.starts_with("NOT") {
        Claim::NotDone
    } else if verdict == "DONE" || verdict == "DONE." {
        Claim::Done
    } else {
        Claim::Unstated
    }
}

/// What one agent run did, read from its transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    /// Tool calls of every kind (MCP, file tools, the client's own).
    pub tool_calls: u64,
    /// Calls that failed: `is_error`, or an engine answer with `ok: false`.
    pub tool_errors: u64,
    /// Whether the very first call failed.
    pub first_call_error: bool,
    /// `script_build` calls that answered `passed: false`.
    pub compile_failures: u64,
    /// Compiler errors over those builds (their `errors` count).
    pub compile_errors: u64,
    /// `test_run` calls that answered `passed: false`.
    pub failed_test_runs: u64,
    /// Failures (a tool error, a failed build or a failed test run) after
    /// which the agent made another call: each is one turn spent repairing.
    pub repair_turns: u64,
    /// Turns, as the client counts them.
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// API-equivalent cost as the client reports it.
    pub cost_usd: f64,
    /// The client's session id (to resume it for a repair round).
    pub session_id: Option<String>,
    /// The agent's last message.
    pub final_message: Option<String>,
    /// The client's own error, if the run ended in one.
    pub agent_error: Option<String>,
    /// Whether the client's closing `result` event was seen. Without it (a
    /// run cut off by the time limit) tokens come from the assistant
    /// messages and the client's cost is unknown (`cost_usd` stays 0).
    pub has_result: bool,
    /// The model ids the client reported using (`modelUsage`), sorted.
    pub model_ids: Vec<String>,
    /// Whether `cost_usd` is the harness's estimate (from the tokens at list
    /// price) rather than the client's report.
    pub cost_estimated: bool,
}

impl Transcript {
    /// Every token read and written.
    pub fn tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }

    /// Adds another run of the same try (a repair round) to this one.
    pub fn absorb(&mut self, other: &Transcript) {
        self.tool_calls += other.tool_calls;
        self.tool_errors += other.tool_errors;
        self.compile_failures += other.compile_failures;
        self.compile_errors += other.compile_errors;
        self.failed_test_runs += other.failed_test_runs;
        self.repair_turns += other.repair_turns;
        self.turns += other.turns;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.cost_usd += other.cost_usd;
        if other.session_id.is_some() {
            self.session_id.clone_from(&other.session_id);
        }
        if other.final_message.is_some() {
            self.final_message.clone_from(&other.final_message);
        }
        if other.agent_error.is_some() {
            self.agent_error.clone_from(&other.agent_error);
        }
        self.has_result = self.has_result && other.has_result;
        self.cost_estimated |= other.cost_estimated;
        for id in &other.model_ids {
            if !self.model_ids.contains(id) {
                self.model_ids.push(id.clone());
            }
        }
        self.model_ids.sort();
    }
}

/// The text of a tool result's content (a string, or text blocks).
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Reads a transcript.
pub fn read(lines: &[String]) -> Transcript {
    let mut out = Transcript::default();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut first_id: Option<String> = None;
    // Failures in the order they happened, and how many calls came before
    // each (to tell whether the agent went on after it).
    let mut failures_at: Vec<u64> = Vec::new();
    let mut last_text: Option<String> = None;
    // Usage of each assistant message (by message id; the client repeats a
    // message's usage on every event of it), for runs without a result.
    let mut messages: BTreeMap<String, Value> = BTreeMap::new();
    for line in lines {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["type"].as_str() {
            Some("system") => {
                if out.session_id.is_none() {
                    out.session_id = event["session_id"].as_str().map(str::to_owned);
                }
            }
            Some("assistant") => {
                if let Some(id) = event["message"]["id"].as_str() {
                    messages.insert(id.to_owned(), event["message"]["usage"].clone());
                }
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("tool_use") => {
                            out.tool_calls += 1;
                            let id = block["id"].as_str().unwrap_or_default().to_owned();
                            if first_id.is_none() {
                                first_id = Some(id.clone());
                            }
                            names.insert(id, block["name"].as_str().unwrap_or_default().to_owned());
                        }
                        Some("text") => {
                            if let Some(text) = block["text"].as_str() {
                                last_text = Some(text.to_owned());
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] != "tool_result" {
                        continue;
                    }
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    let name = names.get(id).map(String::as_str).unwrap_or_default();
                    let answer: Value = serde_json::from_str(&result_text(&block["content"]))
                        .unwrap_or(Value::Null);
                    let failed_call = block["is_error"] == true || answer["ok"] == false;
                    let mut failure = failed_call;
                    if failed_call {
                        out.tool_errors += 1;
                        if first_id.as_deref() == Some(id) {
                            out.first_call_error = true;
                        }
                    } else if name.ends_with("script_build") && answer["passed"] == false {
                        out.compile_failures += 1;
                        out.compile_errors += answer["errors"].as_u64().unwrap_or(1).max(1);
                        failure = true;
                    } else if name.ends_with("test_run") && answer["passed"] == false {
                        out.failed_test_runs += 1;
                        failure = true;
                    }
                    if failure {
                        failures_at.push(out.tool_calls);
                    }
                }
            }
            Some("result") => {
                // Summed, so that a file holding several runs adds up.
                out.has_result = true;
                out.turns += event["num_turns"].as_u64().unwrap_or(0);
                out.cost_usd += event["total_cost_usd"].as_f64().unwrap_or(0.0);
                let usage = &event["usage"];
                out.input_tokens += usage["input_tokens"].as_u64().unwrap_or(0);
                out.output_tokens += usage["output_tokens"].as_u64().unwrap_or(0);
                out.cache_read_tokens += usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                out.cache_write_tokens +=
                    usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                for id in event["modelUsage"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, _)| k)
                {
                    if !out.model_ids.contains(id) {
                        out.model_ids.push(id.clone());
                    }
                }
                out.model_ids.sort();
                if let Some(id) = event["session_id"].as_str() {
                    out.session_id = Some(id.to_owned());
                }
                if event["is_error"] == true {
                    out.agent_error = Some(
                        event["result"]
                            .as_str()
                            .unwrap_or("error")
                            .chars()
                            .take(300)
                            .collect(),
                    );
                } else if let Some(text) = event["result"].as_str() {
                    out.final_message = Some(text.to_owned());
                }
            }
            _ => {}
        }
    }
    if out.final_message.is_none() && out.agent_error.is_none() {
        out.final_message = last_text;
    }
    if !out.has_result {
        for usage in messages.values() {
            let n = |k: &str| usage[k].as_u64().unwrap_or(0);
            out.input_tokens += n("input_tokens");
            out.output_tokens += n("output_tokens");
            out.cache_read_tokens += n("cache_read_input_tokens");
            out.cache_write_tokens += n("cache_creation_input_tokens");
        }
    }
    out.repair_turns = failures_at
        .iter()
        .filter(|&&calls_before| out.tool_calls > calls_before)
        .count() as u64;
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use serde_json::json;

    fn lines(events: &[Value]) -> Vec<String> {
        events.iter().map(Value::to_string).collect()
    }

    fn call(id: &str, name: &str) -> Value {
        json!({ "type": "assistant", "message": { "content": [
            { "type": "tool_use", "id": id, "name": name, "input": {} } ] } })
    }

    fn answer(id: &str, body: Value, is_error: bool) -> Value {
        json!({ "type": "user", "message": { "content": [
            { "type": "tool_result", "tool_use_id": id, "is_error": is_error,
              "content": body.to_string() } ] } })
    }

    fn result(text: &str) -> Value {
        json!({ "type": "result", "num_turns": 5, "total_cost_usd": 0.25, "is_error": false,
                "session_id": "s-1", "result": text,
                "usage": { "input_tokens": 10, "output_tokens": 200,
                           "cache_read_input_tokens": 3000, "cache_creation_input_tokens": 400 } })
    }

    #[test]
    fn claims_are_read_from_the_last_result_line() {
        assert_eq!(claim(Some("All good.\nRESULT: DONE")), Claim::Done);
        assert_eq!(claim(Some("**RESULT: DONE**")), Claim::Done);
        assert_eq!(claim(Some("`RESULT: DONE`")), Claim::Done);
        assert_eq!(claim(Some("result: done")), Claim::Done);
        assert_eq!(
            claim(Some("RESULT: DONE\nwait\nRESULT: NOT DONE the test fails")),
            Claim::NotDone
        );
        assert_eq!(
            claim(Some("I built the game and it works.")),
            Claim::Unstated
        );
        assert_eq!(claim(Some("RESULT: maybe")), Claim::Unstated);
        assert_eq!(claim(Some("- RESULT: DONE")), Claim::Done);
        assert_eq!(claim(Some("• RESULT: DONE.")), Claim::Done);
        assert_eq!(claim(Some("3. **RESULT: DONE**")), Claim::Done);
        assert_eq!(
            claim(Some("* RESULT: NOT DONE: tests fail")),
            Claim::NotDone
        );
        assert_eq!(claim(Some("RESULT: DONE, mostly")), Claim::Unstated);
        assert_eq!(claim(Some("RESULT: DONEISH")), Claim::Unstated);
        assert_eq!(claim(Some("  ")), Claim::NoAnswer);
        assert_eq!(claim(None), Claim::NoAnswer);
        assert!(Claim::Done.claims_success() && Claim::Unstated.claims_success());
        assert!(!Claim::NotDone.claims_success() && !Claim::NoAnswer.claims_success());
        for c in [
            Claim::Done,
            Claim::NotDone,
            Claim::Unstated,
            Claim::NoAnswer,
        ] {
            assert_eq!(Claim::from_name(c.name()), c);
        }
    }

    #[test]
    fn errors_builds_tests_and_repairs_are_counted() {
        let t = read(&lines(&[
            json!({ "type": "system", "subtype": "init", "session_id": "s-0" }),
            call("a", "mcp__wish__edit_apply"),
            answer(
                "a",
                json!({ "ok": false, "error": { "message": "bad" } }),
                true,
            ),
            call("b", "mcp__wish__script_build"),
            answer(
                "b",
                json!({ "ok": true, "passed": false, "errors": 3 }),
                false,
            ),
            call("c", "mcp__wish__script_build"),
            answer(
                "c",
                json!({ "ok": true, "passed": true, "errors": 0 }),
                false,
            ),
            call("d", "mcp__wish__test_run"),
            answer("d", json!({ "ok": true, "passed": false }), false),
            call("e", "mcp__wish__test_run"),
            answer("e", json!({ "ok": true, "passed": true }), false),
            call("f", "mcp__wish__check"),
            answer("f", json!({ "ok": false }), false),
            result("Done.\nRESULT: DONE"),
        ]));
        assert_eq!(t.tool_calls, 6);
        assert_eq!(t.tool_errors, 2);
        assert!(t.first_call_error);
        assert_eq!(t.compile_failures, 1);
        assert_eq!(t.compile_errors, 3);
        assert_eq!(t.failed_test_runs, 1);
        // The last failure (check) had no call after it.
        assert_eq!(t.repair_turns, 3);
        assert_eq!(t.turns, 5);
        assert_eq!(t.tokens(), 3610);
        assert_eq!(t.session_id.as_deref(), Some("s-1"));
        assert_eq!(claim(t.final_message.as_deref()), Claim::Done);
    }

    #[test]
    fn an_agent_error_leaves_no_answer() {
        let t = read(&lines(&[
            call("a", "Read"),
            answer("a", json!("text"), false),
            json!({ "type": "result", "is_error": true, "result": "overloaded", "usage": {} }),
        ]));
        assert_eq!(t.agent_error.as_deref(), Some("overloaded"));
        assert_eq!(claim(t.final_message.as_deref()), Claim::NoAnswer);
        assert!(!t.first_call_error);
        assert_eq!(t.repair_turns, 0);
    }

    #[test]
    fn a_cut_off_run_keeps_its_last_text() {
        let t = read(&lines(&[
            json!({ "type": "assistant", "message": { "content": [
            { "type": "text", "text": "Working on it" } ] } }),
        ]));
        assert_eq!(t.final_message.as_deref(), Some("Working on it"));
        assert_eq!(claim(t.final_message.as_deref()), Claim::Unstated);
    }

    #[test]
    fn repair_rounds_add_up() {
        let mut first = read(&lines(&[call("a", "x"), result("RESULT: DONE")]));
        let second = read(&lines(&[
            call("b", "y"),
            call("c", "z"),
            result("RESULT: NOT DONE"),
        ]));
        first.absorb(&second);
        assert_eq!(first.tool_calls, 3);
        assert!((first.cost_usd - 0.5).abs() < 1e-9);
        assert_eq!(claim(first.final_message.as_deref()), Claim::NotDone);
    }

    #[test]
    fn several_runs_in_one_file_add_up_and_name_their_models() {
        let mut a = result("RESULT: DONE");
        a["modelUsage"] = json!({ "claude-sonnet-5-5": {} });
        let mut b = result("RESULT: DONE");
        b["modelUsage"] = json!({ "claude-haiku-5-5": {} });
        let t = read(&lines(&[a, b]));
        assert_eq!(t.turns, 10);
        assert_eq!(t.tokens(), 7220);
        assert!((t.cost_usd - 0.5).abs() < 1e-9);
        assert_eq!(t.model_ids, ["claude-haiku-5-5", "claude-sonnet-5-5"]);
    }

    #[test]
    fn a_run_without_a_result_counts_each_message_once() {
        let usage = json!({ "input_tokens": 5, "output_tokens": 50,
                            "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 100 });
        let message = |id: &str| {
            json!({ "type": "assistant", "message": { "id": id, "usage": usage,
            "content": [{ "type": "text", "text": "..." }] } })
        };
        // The client repeats a message's usage on each of its events.
        let t = read(&lines(&[message("m1"), message("m1"), message("m2")]));
        assert!(!t.has_result);
        assert_eq!(t.tokens(), 2 * 1155);
        assert!(t.cost_usd.abs() < 1e-12);
        let done = read(&lines(&[message("m1"), result("RESULT: DONE")]));
        assert!(done.has_result);
        assert_eq!(done.tokens(), 3610);
    }

    #[test]
    fn unreadable_lines_are_skipped() {
        let t = read(&["not json".to_owned(), String::new()]);
        assert_eq!(t, Transcript::default());
    }
}
