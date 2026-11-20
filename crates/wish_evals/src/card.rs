//! The AI scorecard (M12 plan, "The AI scorecard"): the tries of a campaign
//! turned into the card's seven criteria, in the card's order, per model,
//! with the gap between the first two models; written as JSON and Markdown
//! with every input and the command that reproduces it (ADR-027, rule 4).
//! Everything here is a pure function of the recorded tries, so a card can
//! be made again from its JSON (`wish-evals card --from <file>`).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

/// The frame budget the tick time is compared with (60 ticks a second).
pub const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;

fn num(row: &Value, key: &str) -> f64 {
    row[key].as_f64().unwrap_or(0.0)
}

fn yes(row: &Value, key: &str) -> bool {
    row[key] == true
}

fn round(x: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (x * scale).round() / scale
}

fn rate(count: usize, of: usize) -> Value {
    if of == 0 {
        Value::Null
    } else {
        json!(round(count as f64 / of as f64, 3))
    }
}

fn per(total: f64, of: usize) -> Value {
    if of == 0 {
        Value::Null
    } else {
        json!(round(total / of as f64, 3))
    }
}

/// The value at quantile `q` (nearest rank) of sorted numbers.
fn quantile(sorted: &[f64], q: f64) -> Value {
    if sorted.is_empty() {
        return Value::Null;
    }
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    json!(round(sorted[rank - 1], 3))
}

fn sorted(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    values
}

/// Coefficient of variation (standard deviation over mean) of a task's tries.
fn variation(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean <= 0.0 {
        return None;
    }
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    Some(var.sqrt() / mean)
}

/// Whether the agent said it was done (see `transcript::Claim`).
fn claims(row: &Value, key: &str) -> bool {
    crate::transcript::Claim::from_name(row[key].as_str().unwrap_or_default()).claims_success()
}

/// The card of one model's tries.
pub fn model_card(model: &str, effort: &str, rows: &[Value]) -> Value {
    let n = rows.len();
    let passed: Vec<&Value> = rows.iter().filter(|r| yes(r, "passed")).collect();
    let p = passed.len();
    let count = |f: &dyn Fn(&Value) -> bool| rows.iter().filter(|r| f(r)).count();
    let sum = |key: &str| rows.iter().map(|r| num(r, key)).sum::<f64>();

    // 1. Correctness.
    let first = count(&|r| yes(r, "passed_first"));
    let replays: Vec<&Value> = rows.iter().filter(|r| r["replay"].is_boolean()).collect();
    let held = replays.iter().filter(|r| r["replay"] == true).count();

    // 2. Errors. A false success: the agent said it was done, the hidden
    // check refuted it.
    let claimed_first = count(&|r| claims(r, "claim_first"));
    let false_first = count(&|r| claims(r, "claim_first") && !yes(r, "passed_first"));
    // The same, split: claims the agent stated (`RESULT: DONE`) and claims
    // read from a last message without a RESULT line.
    let stated = count(&|r| r["claim_first"] == "done");
    let false_stated = count(&|r| r["claim_first"] == "done" && !yes(r, "passed_first"));
    let false_unstated = count(&|r| r["claim_first"] == "unstated" && !yes(r, "passed_first"));
    let claimed_final = count(&|r| claims(r, "claim_final"));
    let false_final = count(&|r| claims(r, "claim_final") && !yes(r, "passed"));
    let unstated = count(&|r| r["claim_first"] == "unstated");

    // 3. Quality.
    // A `check` that could not run is not clean.
    let clean = passed
        .iter()
        .filter(|r| r["check_errors"] == 0 && r["check_warnings"] == 0)
        .count();
    let check_failed = count(&|r| !r["check_errors"].is_number());
    let seconds: Vec<&Value> = rows.iter().filter(|r| r["second"].is_object()).collect();
    let second_passed = seconds
        .iter()
        .filter(|r| r["second"]["passed"] == true)
        .count();
    let second_cost: f64 = seconds.iter().map(|r| num(&r["second"], "cost_usd")).sum();
    let second_tokens: f64 = seconds.iter().map(|r| num(&r["second"], "tokens")).sum();

    // 4. People.
    let people: Vec<&Value> = rows.iter().filter(|r| r["people"].is_boolean()).collect();
    let people_ok = people.iter().filter(|r| r["people"] == true).count();

    // 5. Efficiency.
    let tokens = sum("input_tokens")
        + sum("output_tokens")
        + sum("cache_read_tokens")
        + sum("cache_write_tokens");

    // 6. Speed.
    let to_result = sorted(
        rows.iter()
            .filter_map(|r| r["seconds_to_result"].as_f64())
            .collect(),
    );
    let ticks = sorted(
        rows.iter()
            .filter_map(|r| r["tick_ms_10k"].as_f64())
            .collect(),
    );

    // 7. Consistency.
    let mut by_task: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for row in rows {
        by_task
            .entry(row["task"].as_str().unwrap_or_default())
            .or_default()
            .push(row);
    }
    let (mut all, mut mixed, mut none) = (0, 0, 0);
    let mut cost_cv = Vec::new();
    let mut time_cv = Vec::new();
    for tries in by_task.values() {
        let ok = tries.iter().filter(|r| yes(r, "passed")).count();
        if ok == tries.len() {
            all += 1;
        } else if ok == 0 {
            none += 1;
        } else {
            mixed += 1;
        }
        let costs: Vec<f64> = tries.iter().map(|r| num(r, "cost_usd")).collect();
        let times: Vec<f64> = tries.iter().map(|r| num(r, "seconds")).collect();
        cost_cv.extend(variation(&costs));
        time_cv.extend(variation(&times));
    }
    let tries_per_task = by_task.values().map(Vec::len).max().unwrap_or(0);

    json!({
        "model": model,
        "effort": effort,
        "tries": n,
        "tasks": by_task.len(),
        "tries_per_task": tries_per_task,
        "1_correctness": {
            "first_attempt_rate": rate(first, n),
            "final_rate": rate(p, n),
            "passed_first": first,
            "passed": p,
            "replay_holds": { "held": held, "of": replays.len(), "rate": rate(held, replays.len()) },
        },
        "2_errors": {
            "tool_errors_per_try": per(sum("tool_errors"), n),
            "compile_failures_per_try": per(sum("compile_failures"), n),
            "failed_test_runs_per_try": per(sum("failed_test_runs"), n),
            "repair_turns_per_try": per(sum("repair_turns"), n),
            "repair_rounds_per_try": per(sum("repair_rounds"), n),
            "first_call_errors": count(&|r| yes(r, "first_call_error")),
            "false_successes_first": { "count": false_first, "of_claims": claimed_first, "rate": rate(false_first, claimed_first) },
            "false_successes_first_stated": { "count": false_stated, "of_claims": stated, "rate": rate(false_stated, stated) },
            "false_successes_first_unstated": { "count": false_unstated, "of_claims": unstated, "rate": rate(false_unstated, unstated) },
            "false_successes_final": { "count": false_final, "of_claims": claimed_final, "rate": rate(false_final, claimed_final) },
            "claims_unstated": unstated,
            "agent_errors": count(&|r| r["agent_error"].is_string()),
        },
        "3_quality": {
            "clean_check": { "clean": clean, "of_passed": p, "rate": rate(clean, p) },
            "check_could_not_run": check_failed,
            "second_agent": if seconds.is_empty() { Value::Null } else { json!({
                "passed": second_passed, "of": seconds.len(), "rate": rate(second_passed, seconds.len()),
                "cost_per_pass_usd": per(second_cost, second_passed),
                "tokens_per_pass": per(second_tokens, second_passed),
            }) },
        },
        "4_people": {
            "edited_by_hand": { "ok": people_ok, "of": people.len(), "rate": rate(people_ok, people.len()) },
        },
        "5_efficiency": {
            "cost_usd": round(sum("cost_usd"), 3),
            "tries_with_estimated_cost": count(&|r| yes(r, "cost_estimated")),
            "tokens": tokens as u64,
            "cost_per_pass_usd": per(sum("cost_usd"), p),
            "tokens_per_pass": per(tokens, p).as_f64().map(f64::round),
            "output_tokens_per_pass": per(sum("output_tokens"), p).as_f64().map(f64::round),
        },
        "6_speed": {
            "seconds_to_result_median": quantile(&to_result, 0.5),
            "seconds_to_result_p90": quantile(&to_result, 0.9),
            "tick_ms_10k_median": quantile(&ticks, 0.5),
            "tick_ms_10k_max": ticks.last().map(|t| round(*t, 3)),
            "tick_measured": ticks.len(),
            "tick_over_frame_budget": ticks.iter().filter(|t| **t > FRAME_BUDGET_MS).count(),
            "tick_failed": count(&|r| r["tick_error"].is_string()),
        },
        "7_consistency": {
            "tasks_all_tries": all,
            "tasks_mixed": mixed,
            "tasks_no_try": none,
            "cost_cv_median": quantile(&sorted(cost_cv), 0.5),
            "seconds_cv_median": quantile(&sorted(time_cv), 0.5),
        },
    })
}

fn at<'a>(card: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(card, |v, key| &v[key])
}

/// The gap between two models' cards: `a` (the usual model) minus `b` (the
/// smaller one) for rates in percentage points, and `b / a` for costs and
/// times. The smaller the gap, the more of the work the engine carries.
pub fn gap(a: &Value, b: &Value) -> Value {
    let points = |path: &str| match (at(a, path).as_f64(), at(b, path).as_f64()) {
        (Some(x), Some(y)) => json!(round((x - y) * 100.0, 1)),
        _ => Value::Null,
    };
    let ratio = |path: &str| match (at(a, path).as_f64(), at(b, path).as_f64()) {
        (Some(x), Some(y)) if x > 0.0 => json!(round(y / x, 2)),
        _ => Value::Null,
    };
    json!({
        "between": [a["model"], b["model"]],
        "first_attempt_points": points("1_correctness.first_attempt_rate"),
        "final_points": points("1_correctness.final_rate"),
        "false_success_points": points("2_errors.false_successes_first.rate"),
        "clean_check_points": points("3_quality.clean_check.rate"),
        "edited_by_hand_points": points("4_people.edited_by_hand.rate"),
        "cost_per_pass_ratio": ratio("5_efficiency.cost_per_pass_usd"),
        "tokens_per_pass_ratio": ratio("5_efficiency.tokens_per_pass"),
        "seconds_to_result_ratio": ratio("6_speed.seconds_to_result_median"),
    })
}

/// The task ids of some tries.
fn task_ids(rows: &[Value]) -> BTreeSet<&str> {
    rows.iter()
        .map(|row| row["task"].as_str().unwrap_or_default())
        .collect()
}

/// The gap between the first two models over the tasks both ran (a
/// campaign stopped at its cost cap may leave one model on fewer tasks):
/// each model's tries of those tasks are scored again and compared.
/// `tasks` says how many tasks it uses, `tasks_left_out` how many the two
/// models' columns count beyond them. No task in common: no gap.
pub fn paired_gap(a: &(String, String, Vec<Value>), b: &(String, String, Vec<Value>)) -> Value {
    let (ids_a, ids_b) = (task_ids(&a.2), task_ids(&b.2));
    let shared: BTreeSet<&str> = ids_a.intersection(&ids_b).copied().collect();
    if shared.is_empty() {
        return Value::Null;
    }
    let only_shared = |rows: &[Value]| -> Vec<Value> {
        rows.iter()
            .filter(|row| shared.contains(row["task"].as_str().unwrap_or_default()))
            .cloned()
            .collect()
    };
    let mut out = gap(
        &model_card(&a.0, &a.1, &only_shared(&a.2)),
        &model_card(&b.0, &b.1, &only_shared(&b.2)),
    );
    out["tasks"] = json!(shared.len());
    out["tasks_left_out"] = json!(ids_a.len() + ids_b.len() - 2 * shared.len());
    out
}

/// The whole scorecard of a campaign: inputs, one card per model (in the
/// order the models were run), the gap, and (for the open set) the tries.
pub fn scorecard(inputs: &Value, runs: &[(String, String, Vec<Value>)], keep_tries: bool) -> Value {
    let cards: Vec<Value> = runs
        .iter()
        .map(|(model, effort, rows)| model_card(model, effort, rows))
        .collect();
    let gap = match runs {
        [a, b, ..] => paired_gap(a, b),
        _ => Value::Null,
    };
    let mut out = Map::new();
    out.insert("scorecard".to_owned(), json!(1));
    out.insert("inputs".to_owned(), inputs.clone());
    out.insert("cards".to_owned(), json!(cards));
    out.insert("gap".to_owned(), gap);
    if keep_tries {
        out.insert(
            "tries".to_owned(),
            json!(runs
                .iter()
                .map(|(model, effort, rows)| json!({ "model": model, "effort": effort, "rows": rows }))
                .collect::<Vec<_>>()),
        );
    }
    Value::Object(out)
}

fn show(value: &Value) -> String {
    match value {
        Value::Null => "–".to_owned(),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(0.0);
            if n.is_u64() || n.is_i64() {
                n.to_string()
            } else {
                format!("{}", round(x, 3))
            }
        }
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn percent(value: &Value) -> String {
    value
        .as_f64()
        .map_or("–".to_owned(), |x| format!("{:.1} %", x * 100.0))
}

fn fraction(card: &Value, path: &str, part: &str, whole: &str) -> String {
    let v = at(card, path);
    if v[whole].as_u64() == Some(0) || v.is_null() {
        return "–".to_owned();
    }
    format!(
        "{} of {} ({})",
        show(&v[part]),
        show(&v[whole]),
        percent(&v["rate"])
    )
}

/// The scorecard as Markdown, in the card's order.
pub fn markdown(card: &Value) -> String {
    let inputs = &card["inputs"];
    let cards: Vec<&Value> = card["cards"].as_array().into_iter().flatten().collect();
    let gap = &card["gap"];
    let mut out = String::new();
    out.push_str(&format!(
        "# AI scorecard: {}\n\n",
        inputs["label"].as_str().unwrap_or("campaign")
    ));
    out.push_str("The card of the M12 plan (\"The AI scorecard\"), in its order. Generated by `wish-evals`; do not edit by hand.\n\n");
    out.push_str("## Inputs\n\n| | |\n|---|---|\n");
    for key in [
        "set",
        "tasks",
        "tries_per_task",
        "repair_rounds",
        "second_agent",
        "engine_version",
        "engine_commit",
        "engine_dirty",
        "date",
        "machine",
        "client",
        "resolved_models",
    ] {
        out.push_str(&format!("| {key} | {} |\n", show(&inputs[key])));
    }
    let models: Vec<String> = cards
        .iter()
        .map(|c| format!("{} · {}", show(&c["model"]), show(&c["effort"])))
        .collect();
    out.push_str(&format!("| models | {} |\n", models.join(", ")));
    out.push_str(&format!(
        "\nReproduce (spends model quota; needs the user's approval):\n\n```\n{}\n```\n",
        show(&inputs["command"])
    ));
    let method = &inputs["method"];
    if method.is_object() {
        out.push_str("\n## How this card was made\n\n");
        out.push_str(&format!(
            "- Every prompt ends with this sentence, so that the agent's own verdict can be read (runs before M12 package 10 had none and are not comparable on the error rows): \"{}\"\n",
            show(&method["claim_sentence"]).trim()
        ));
        out.push_str(&format!(
            "- Repair rounds: up to {} per try. A round resumes the agent's session with the hidden check's reason, so \"passed in the end\", \"refuted in the end\", cost and tokens per pass and the time to a proven result are partly helped by the harness. The first-attempt rows are not.\n",
            show(&method["repair_rounds"])
        ));
        out.push_str(&format!(
            "- Second agent: {}.\n",
            if method["second_agent"] == true {
                "ran on every passed try that has a follow-up"
            } else {
                "not run"
            }
        ));
        out.push_str(&format!("- Order: {}.\n", show(&method["order"])));
        if let Some(cap) = method["max_cost_usd"].as_f64() {
            out.push_str(&format!("- Cost cap: ${cap:.2}"));
            if method["stopped_at_cap"] == true {
                out.push_str(&format!(
                    "; **the campaign stopped at the cap after {} of {} tries** (the card counts only the tries that ran)",
                    show(&method["tries_run"]),
                    show(&method["tries_planned"])
                ));
            }
            out.push_str(".\n");
        }
        out.push_str("- A try cut off before the client's closing report has its cost estimated from the tokens seen at list price (counted on the card).\n");
    }
    let has_gap = gap.is_object();
    let header = |out: &mut String, title: &str| {
        out.push_str(&format!("\n## {title}\n\n| | "));
        out.push_str(&models.join(" | "));
        if has_gap {
            out.push_str(" | gap");
        }
        out.push_str(" |\n|---|");
        for _ in &models {
            out.push_str("---|");
        }
        if has_gap {
            out.push_str("---|");
        }
        out.push('\n');
    };
    let row = |out: &mut String, name: &str, f: &dyn Fn(&Value) -> String, g: Option<&str>| {
        out.push_str(&format!("| {name} |"));
        for c in &cards {
            out.push_str(&format!(" {} |", f(c)));
        }
        if has_gap {
            out.push_str(&format!(
                " {} |",
                g.map_or(String::new(), |k| show(&gap[k]))
            ));
        }
        out.push('\n');
    };
    header(&mut out, "1. Correctness");
    row(
        &mut out,
        "passed at the first attempt",
        &|c| percent(at(c, "1_correctness.first_attempt_rate")),
        Some("first_attempt_points"),
    );
    row(
        &mut out,
        "passed in the end (harness-assisted)",
        &|c| percent(at(c, "1_correctness.final_rate")),
        Some("final_points"),
    );
    row(
        &mut out,
        "a replay of the result holds (game tasks)",
        &|c| fraction(c, "1_correctness.replay_holds", "held", "of"),
        None,
    );
    header(&mut out, "2. Errors");
    row(
        &mut out,
        "believed right, refuted by the hidden test (first attempt)",
        &|c| fraction(c, "2_errors.false_successes_first", "count", "of_claims"),
        Some("false_success_points"),
    );
    row(
        &mut out,
        "  of them stated (`RESULT: DONE`)",
        &|c| {
            fraction(
                c,
                "2_errors.false_successes_first_stated",
                "count",
                "of_claims",
            )
        },
        None,
    );
    row(
        &mut out,
        "  of them unstated (no RESULT line)",
        &|c| {
            fraction(
                c,
                "2_errors.false_successes_first_unstated",
                "count",
                "of_claims",
            )
        },
        None,
    );
    row(
        &mut out,
        "believed right, refuted (in the end; harness-assisted)",
        &|c| fraction(c, "2_errors.false_successes_final", "count", "of_claims"),
        None,
    );
    row(
        &mut out,
        "tool errors per try",
        &|c| show(at(c, "2_errors.tool_errors_per_try")),
        None,
    );
    row(
        &mut out,
        "compile failures per try",
        &|c| show(at(c, "2_errors.compile_failures_per_try")),
        None,
    );
    row(
        &mut out,
        "repair turns per try",
        &|c| show(at(c, "2_errors.repair_turns_per_try")),
        None,
    );
    row(
        &mut out,
        "repair rounds per try (harness)",
        &|c| show(at(c, "2_errors.repair_rounds_per_try")),
        None,
    );
    row(
        &mut out,
        "claims without a RESULT line",
        &|c| show(at(c, "2_errors.claims_unstated")),
        None,
    );
    header(&mut out, "3. Quality of what was made");
    row(
        &mut out,
        "clean `check` (passed tries)",
        &|c| fraction(c, "3_quality.clean_check", "clean", "of_passed"),
        Some("clean_check_points"),
    );
    row(
        &mut out,
        "a second agent changes it correctly",
        &|c| {
            let s = at(c, "3_quality.second_agent");
            if s.is_null() {
                "not run".to_owned()
            } else {
                format!(
                    "{} of {} ({}), {} $ per pass",
                    show(&s["passed"]),
                    show(&s["of"]),
                    percent(&s["rate"]),
                    show(&s["cost_per_pass_usd"])
                )
            }
        },
        None,
    );
    header(&mut out, "4. People");
    row(
        &mut out,
        "opens in the editor and takes an inspector edit by hand",
        &|c| fraction(c, "4_people.edited_by_hand", "ok", "of"),
        Some("edited_by_hand_points"),
    );
    header(&mut out, "5. Efficiency");
    row(
        &mut out,
        "cost per passed try ($, API equivalent)",
        &|c| show(at(c, "5_efficiency.cost_per_pass_usd")),
        Some("cost_per_pass_ratio"),
    );
    row(
        &mut out,
        "tokens per passed try",
        &|c| show(at(c, "5_efficiency.tokens_per_pass")),
        Some("tokens_per_pass_ratio"),
    );
    row(
        &mut out,
        "output tokens per passed try",
        &|c| show(at(c, "5_efficiency.output_tokens_per_pass")),
        None,
    );
    row(
        &mut out,
        "campaign cost ($)",
        &|c| show(at(c, "5_efficiency.cost_usd")),
        None,
    );
    row(
        &mut out,
        "tries with an estimated cost (cut off)",
        &|c| show(at(c, "5_efficiency.tries_with_estimated_cost")),
        None,
    );
    header(&mut out, "6. Speed");
    row(
        &mut out,
        "seconds to a proven result, median",
        &|c| show(at(c, "6_speed.seconds_to_result_median")),
        Some("seconds_to_result_ratio"),
    );
    row(
        &mut out,
        "seconds to a proven result, 90th percentile",
        &|c| show(at(c, "6_speed.seconds_to_result_p90")),
        None,
    );
    row(
        &mut out,
        "tick at 10,000 entities, median ms",
        &|c| show(at(c, "6_speed.tick_ms_10k_median")),
        None,
    );
    row(
        &mut out,
        "tick at 10,000 entities, worst ms",
        &|c| show(at(c, "6_speed.tick_ms_10k_max")),
        None,
    );
    row(
        &mut out,
        "measurements that failed (a trap at 10,000 entities)",
        &|c| show(at(c, "6_speed.tick_failed")),
        None,
    );
    row(
        &mut out,
        "results over the 16.7 ms frame budget",
        &|c| {
            format!(
                "{} of {}",
                show(at(c, "6_speed.tick_over_frame_budget")),
                show(at(c, "6_speed.tick_measured"))
            )
        },
        None,
    );
    header(&mut out, "7. Consistency");
    // Consistency is across the tries of a task: with one try per task there
    // is nothing to compare (every task "passed in every try" or "in none").
    let consistency = |path: &'static str| {
        move |c: &Value| {
            if c["tries_per_task"].as_u64().unwrap_or(0) < 2 {
                "– (one try per task)".to_owned()
            } else {
                show(at(c, path))
            }
        }
    };
    row(
        &mut out,
        "tasks passed in every try",
        &consistency("7_consistency.tasks_all_tries"),
        None,
    );
    row(
        &mut out,
        "tasks passed in some tries",
        &consistency("7_consistency.tasks_mixed"),
        None,
    );
    row(
        &mut out,
        "tasks passed in no try",
        &consistency("7_consistency.tasks_no_try"),
        None,
    );
    row(
        &mut out,
        "spread of cost across tries (median CV)",
        &consistency("7_consistency.cost_cv_median"),
        None,
    );
    row(
        &mut out,
        "spread of time across tries (median CV)",
        &consistency("7_consistency.seconds_cv_median"),
        None,
    );
    if has_gap {
        out.push_str("\nGap: rates in percentage points (first model minus second); costs and times as the second model's over the first's.");
        if let Some(tasks) = gap["tasks"].as_u64() {
            out.push_str(&format!(
                " It is computed over the {tasks} tasks both models ran"
            ));
            match gap["tasks_left_out"].as_u64() {
                Some(left) if left > 0 => out.push_str(&format!(
                    "; the models' own columns also count {left} task{} only one of them ran, so the gap is not the difference of the columns.",
                    if left == 1 { "" } else { "s" }
                )),
                _ => out.push('.'),
            }
        }
        out.push('\n');
    } else if cards.len() >= 2 {
        out.push_str("\nNo gap: the models ran no task in common.\n");
    }
    if inputs["set"] == "held-out" {
        out.push_str("\nHeld-out set: numbers only. No task, failure or transcript is recorded (M12 plan, \"The AI scorecard\").\n");
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    pub(crate) fn fixture() -> Vec<(String, String, Vec<Value>)> {
        let row = |task: &str,
                   passed_first: bool,
                   passed: bool,
                   claim: &str,
                   cost: f64,
                   secs: f64| {
            json!({ "task": task, "passed_first": passed_first, "passed": passed,
                    "claim_first": claim, "claim_final": if passed { "done" } else { claim },
                    "repair_rounds": u8::from(passed && !passed_first),
                    "tool_errors": 1, "compile_failures": 0, "repair_turns": 1, "failed_test_runs": 0,
                    "input_tokens": 10, "output_tokens": 100, "cache_read_tokens": 1000, "cache_write_tokens": 90,
                    "cost_usd": cost, "seconds": secs,
                    "seconds_to_result": if passed { json!(secs) } else { Value::Null },
                    "check_errors": 0, "check_warnings": if task == "b" { 1 } else { 0 },
                    "replay": if task == "b" { json!(true) } else { Value::Null },
                    "people": if passed { json!(true) } else { Value::Null },
                    "tick_ms_10k": if task == "b" { json!(2.5) } else { Value::Null } })
        };
        vec![
            (
                "sonnet".into(),
                "medium".into(),
                vec![
                    row("a", true, true, "done", 0.10, 20.0),
                    row("a", true, true, "done", 0.12, 22.0),
                    row("b", false, true, "done", 0.30, 60.0),
                    row("b", true, true, "unstated", 0.20, 40.0),
                ],
            ),
            (
                "haiku".into(),
                "medium".into(),
                vec![
                    row("a", true, true, "done", 0.01, 30.0),
                    row("a", false, false, "done", 0.02, 35.0),
                    row("b", false, false, "not_done", 0.03, 90.0),
                    row("b", false, true, "done", 0.04, 80.0),
                ],
            ),
        ]
    }

    #[test]
    fn the_card_counts_what_the_criteria_ask() {
        let runs = fixture();
        let a = model_card("sonnet", "medium", &runs[0].2);
        assert_eq!(at(&a, "1_correctness.first_attempt_rate"), &json!(0.75));
        assert_eq!(at(&a, "1_correctness.final_rate"), &json!(1.0));
        // One claim of success (done) was refuted at the first attempt; the
        // unstated claim that passed counts as a claim too.
        assert_eq!(at(&a, "2_errors.false_successes_first.count"), &json!(1));
        assert_eq!(
            at(&a, "2_errors.false_successes_first.of_claims"),
            &json!(4)
        );
        assert_eq!(at(&a, "2_errors.claims_unstated"), &json!(1));
        assert_eq!(
            at(&a, "2_errors.false_successes_first_stated.count"),
            &json!(1)
        );
        assert_eq!(
            at(&a, "2_errors.false_successes_first_unstated.count"),
            &json!(0)
        );
        assert_eq!(at(&a, "3_quality.clean_check.clean"), &json!(2));
        assert_eq!(at(&a, "4_people.edited_by_hand.ok"), &json!(4));
        assert_eq!(at(&a, "5_efficiency.cost_per_pass_usd"), &json!(0.18));
        assert_eq!(at(&a, "6_speed.seconds_to_result_median"), &json!(22.0));
        assert_eq!(at(&a, "6_speed.tick_ms_10k_median"), &json!(2.5));
        assert_eq!(at(&a, "7_consistency.tasks_all_tries"), &json!(2));
        let b = model_card("haiku", "medium", &runs[1].2);
        assert_eq!(at(&b, "1_correctness.final_rate"), &json!(0.5));
        assert_eq!(at(&b, "2_errors.false_successes_final.count"), &json!(1));
        assert_eq!(at(&b, "7_consistency.tasks_mixed"), &json!(2));
        let g = gap(&a, &b);
        assert_eq!(g["final_points"], json!(50.0));
        assert_eq!(g["first_attempt_points"], json!(50.0));
    }

    #[test]
    fn a_check_that_did_not_run_is_not_clean() {
        let row = json!({ "task": "a", "passed": true, "passed_first": true });
        let card = model_card("m", "e", &[row]);
        assert_eq!(at(&card, "3_quality.clean_check.clean"), &json!(0));
        assert_eq!(at(&card, "3_quality.check_could_not_run"), &json!(1));
    }

    #[test]
    fn the_scorecard_is_the_same_from_the_same_tries() {
        let inputs = json!({ "label": "t", "set": "open", "command": "wish-evals campaign",
            "method": { "claim_sentence": "End with RESULT.", "repair_rounds": 1, "second_agent": false,
                        "order": "long task first", "max_cost_usd": 30.0, "stopped_at_cap": true,
                        "tries_run": 7, "tries_planned": 252 } });
        let one = scorecard(&inputs, &fixture(), true);
        let two = scorecard(&inputs, &fixture(), true);
        assert_eq!(one, two);
        let text = markdown(&one);
        let order = [
            "1. Correctness",
            "2. Errors",
            "3. Quality",
            "4. People",
            "5. Efficiency",
            "6. Speed",
            "7. Consistency",
        ];
        let mut last = 0;
        for title in order {
            let at = text
                .find(title)
                .unwrap_or_else(|| panic!("{title} missing"));
            assert!(at > last, "{title} out of order");
            last = at;
        }
        assert!(text.contains("sonnet · medium | haiku · medium | gap"));
        assert!(text.contains("How this card was made"));
        assert!(text.contains("End with RESULT."));
        assert!(text.contains("stopped at the cap after 7 of 252"));
        let held = scorecard(&json!({ "set": "held-out" }), &fixture(), false);
        assert!(held.get("tries").is_none());
        assert!(markdown(&held).contains("numbers only"));
    }

    fn try_of(task: &str, passed: bool) -> Value {
        json!({ "task": task, "passed_first": passed, "passed": passed,
                "claim_first": "done", "claim_final": "done", "cost_usd": 0.1, "seconds": 10.0,
                "seconds_to_result": if passed { json!(10.0) } else { Value::Null },
                "check_errors": 0, "check_warnings": 0 })
    }

    /// Closing review of 1.0: a campaign stopped at its cost cap left the
    /// second model on fewer tasks, and the gap compared the first model's
    /// rate over every task with the second's over a few.
    #[test]
    fn the_gap_compares_only_the_tasks_both_models_ran() {
        let runs = vec![
            (
                "sonnet".to_owned(),
                "medium".to_owned(),
                vec![try_of("1", true), try_of("2", false), try_of("3", false)],
            ),
            (
                "haiku".to_owned(),
                "medium".to_owned(),
                vec![try_of("1", true)],
            ),
        ];
        let card = scorecard(&json!({ "set": "open" }), &runs, true);
        // Over task 1 alone both passed: no gap. Over the columns it would
        // be 33.3 % against 100 %.
        assert_eq!(card["gap"]["final_points"], json!(0.0), "{}", card["gap"]);
        assert_eq!(card["gap"]["first_attempt_points"], json!(0.0));
        assert_eq!(card["gap"]["tasks"], json!(1));
        assert_eq!(card["gap"]["tasks_left_out"], json!(2));
        let text = markdown(&card);
        assert!(text.contains("over the 1 tasks both models ran"), "{text}");
        assert!(text.contains("also count 2 tasks only one of them ran"));
        // No task in common: no gap at all.
        let apart = vec![
            runs[0].clone(),
            (
                "haiku".to_owned(),
                "medium".to_owned(),
                vec![try_of("9", true)],
            ),
        ];
        let card = scorecard(&json!({ "set": "open" }), &apart, true);
        assert!(card["gap"].is_null());
        assert!(markdown(&card).contains("No gap: the models ran no task in common."));
    }

    /// Closing review of 1.0: with one try per task, "tasks passed in every
    /// try" was the number of passed tasks, which says nothing about
    /// consistency.
    #[test]
    fn consistency_needs_two_tries_of_a_task() {
        let runs = vec![(
            "sonnet".to_owned(),
            "medium".to_owned(),
            vec![try_of("1", true), try_of("2", true)],
        )];
        let text = markdown(&scorecard(&json!({ "set": "open" }), &runs, true));
        let section = &text[text.find("## 7. Consistency").unwrap()..];
        assert!(
            section.contains("| tasks passed in every try | – (one try per task) |"),
            "{section}"
        );
        assert!(!section.contains("| tasks passed in every try | 2 |"));
        // With two tries the counts are shown.
        let text = markdown(&scorecard(&json!({ "set": "open" }), &fixture(), true));
        assert!(
            text.contains("| tasks passed in every try | 2 | 0 |  |"),
            "{text}"
        );
    }
}
