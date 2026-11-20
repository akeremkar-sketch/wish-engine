//! What one baseline campaign of the scorecard would cost, estimated offline
//! from the runs recorded in `evals/results/` (no model runs). The basis and
//! every assumption are printed with the numbers (`wish-evals estimate`).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// List prices in dollars per million tokens: (input, output, cache read,
/// cache write). From Anthropic's model table as cached on 3 November 2026
/// (Sonnet 5.5: $2 / $10, cache reads $0.10; Haiku 5.5: $0.10 / $0.50 up to
/// 100K-token prompts). Cache writes are taken at 1.25 times the input price
/// and Haiku's cache reads at a tenth of it: assumptions, not published
/// numbers.
pub const SONNET: [f64; 4] = [2.0, 10.0, 0.10, 2.5];
pub const HAIKU: [f64; 4] = [0.10, 0.50, 0.01, 0.125];

/// The list-price cost of a run whose client report is missing (a run cut
/// off by the time limit): Haiku's prices for a model alias that names
/// Haiku, Sonnet's otherwise.
pub fn list_cost(model: &str, t: &crate::transcript::Transcript) -> f64 {
    let price = if model.to_ascii_lowercase().contains("haiku") {
        HAIKU
    } else {
        SONNET
    };
    (t.input_tokens as f64 * price[0]
        + t.output_tokens as f64 * price[1]
        + t.cache_read_tokens as f64 * price[2]
        + t.cache_write_tokens as f64 * price[3])
        / 1e6
}

#[derive(Clone, Copy, Debug, Default)]
struct Usage {
    cost: f64,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    seconds: f64,
    turns: f64,
}

impl Usage {
    fn tokens(&self) -> f64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
    fn priced(&self, price: [f64; 4]) -> f64 {
        (self.input * price[0]
            + self.output * price[1]
            + self.cache_read * price[2]
            + self.cache_write * price[3])
            / 1e6
    }
    fn add(&mut self, other: &Usage, times: f64) {
        self.cost += other.cost * times;
        self.input += other.input * times;
        self.output += other.output * times;
        self.cache_read += other.cache_read * times;
        self.cache_write += other.cache_write * times;
        self.seconds += other.seconds * times;
        self.turns += other.turns * times;
    }
    fn plus(&self, other: &Usage) -> Usage {
        let mut out = *self;
        out.add(other, 1.0);
        out
    }
    fn scaled(&self, times: f64) -> Usage {
        let mut out = Usage::default();
        out.add(self, times);
        out
    }
}

fn usage(row: &Value) -> Usage {
    let n = |k: &str| row[k].as_f64().unwrap_or(0.0);
    Usage {
        cost: n("cost_usd"),
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_tokens"),
        cache_write: n("cache_write_tokens"),
        seconds: n("seconds"),
        turns: n("turns"),
    }
}

/// Every recorded try of `model`, from result files of either format (the
/// old `tries: [row]` or the scorecard's `tries: [{ model, rows }]`).
fn recorded(results: &Path, model: &str) -> (Vec<String>, Vec<Value>) {
    let mut files: Vec<_> = std::fs::read_dir(results)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let mut used = Vec::new();
    let mut rows = Vec::new();
    for file in files {
        let Some(value) = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        let before = rows.len();
        for entry in value["tries"].as_array().into_iter().flatten() {
            if entry["rows"].is_array() {
                if entry["model"] == model {
                    rows.extend(entry["rows"].as_array().into_iter().flatten().cloned());
                }
            } else if value["model"] == model && entry["task"].is_string() {
                rows.push(entry.clone());
            }
        }
        if rows.len() > before {
            used.push(format!(
                "{} ({} tries)",
                file.file_name().unwrap_or_default().to_string_lossy(),
                rows.len() - before
            ));
        }
    }
    (used, rows)
}

fn money(x: f64) -> String {
    format!("${x:.2}")
}

fn millions(x: f64) -> String {
    format!("{:.1} M", x / 1e6)
}

fn hours(seconds: f64) -> String {
    format!("{:.1} h", seconds / 3600.0)
}

/// The estimate as Markdown. `open` are the open set's task ids, `game` the
/// game-code ones among them, `follow_ups` the tasks with a second-agent
/// step, `long` the long task's id (never run yet).
pub fn estimate(
    results: &Path,
    open: &[String],
    game: &[String],
    follow_ups: &[String],
    long: &str,
) -> String {
    let (used, rows) = recorded(results, "sonnet");
    let mut by_task: BTreeMap<String, Vec<Usage>> = BTreeMap::new();
    for row in &rows {
        if let Some(task) = row["task"].as_str() {
            by_task.entry(task.to_owned()).or_default().push(usage(row));
        }
    }
    let mean = |task: &str| -> Option<Usage> {
        let tries = by_task.get(task)?;
        let mut sum = Usage::default();
        for t in tries {
            sum.add(t, 1.0 / tries.len() as f64);
        }
        Some(sum)
    };
    let mut open_once = Usage::default();
    let mut missing = Vec::new();
    let mut game_sum = Usage::default();
    let mut game_count: f64 = 0.0;
    for task in open {
        match mean(task) {
            Some(u) => {
                open_once.add(&u, 1.0);
                if game.contains(task) {
                    game_sum.add(&u, 1.0);
                    game_count += 1.0;
                }
            }
            None => missing.push(task.clone()),
        }
    }
    let game_mean = game_sum.scaled(1.0 / game_count.max(1.0));
    // The long task: three to six game-code tasks' worth.
    let long_low = game_mean.scaled(3.0);
    let long_high = game_mean.scaled(6.0);
    let follow_low = {
        let mut u = Usage::default();
        for t in follow_ups {
            if t == long {
                u.add(&long_low, 0.5);
            } else if let Some(m) = mean(t) {
                u.add(&m, 0.7);
            }
        }
        u
    };
    let follow_high = {
        let mut u = Usage::default();
        for t in follow_ups {
            if t == long {
                u.add(&long_high, 1.0);
            } else if let Some(m) = mean(t) {
                u.add(&m, 1.2);
            }
        }
        u
    };
    let tries = 3.0;
    // Repair rounds: a failed first attempt gets one more round, about half
    // a try. Sonnet passed 41/41 and 30/30 in the recorded runs: 0 to 10 %
    // fail first; Haiku is assumed 20 to 40 %.
    let (sonnet_repair_low, sonnet_repair_high) = (1.0, 1.05);
    let (haiku_repair_low, haiku_repair_high) = (1.10, 1.20);
    // Haiku is assumed to use 1 to 2 times Sonnet's tokens and 0.6 to 1.2
    // times its time (more turns, faster ones).
    let (haiku_tokens_low, haiku_tokens_high) = (1.0, 2.0);
    let (haiku_time_low, haiku_time_high) = (0.6, 1.2);

    let sonnet_low = open_once.plus(&long_low).scaled(tries * sonnet_repair_low);
    let sonnet_high = open_once
        .plus(&long_high)
        .scaled(tries * sonnet_repair_high);
    let haiku_low = sonnet_low.scaled(haiku_tokens_low * haiku_repair_low / sonnet_repair_low);
    let haiku_high = sonnet_high.scaled(haiku_tokens_high * haiku_repair_high / sonnet_repair_high);
    let second_s = (follow_low.scaled(tries), follow_high.scaled(tries));
    let second_h = (
        follow_low.scaled(tries * haiku_tokens_low),
        follow_high.scaled(tries * haiku_tokens_high),
    );

    // The harness's own time per try (checks, hidden tests, the replay, the
    // measurement at 10,000 entities, the editor): about 20 s for a game
    // task and 8 s for a scene task, from the self-test on this machine.
    let game_tries = (game.len() + 1) as f64;
    let scene_tries = open.len() as f64 - game.len() as f64;
    let harness = tries * (game_tries * 20.0 + scene_tries * 8.0);

    let sonnet_cost = |u: &Usage| u.priced(SONNET);
    let haiku_cost = |u: &Usage| u.priced(HAIKU);
    let recorded_cost = |u: &Usage| u.cost;

    let mut out = String::new();
    out.push_str("Basis: Sonnet 5.5 · medium tries recorded in evals/results/: ");
    out.push_str(&used.join(", "));
    out.push_str(&format!(
        ". {} recorded tries over {} tasks; every open task has at least one ({} without: {}).\n\n",
        rows.len(),
        by_task.len(),
        missing.len(),
        if missing.is_empty() {
            "none".to_owned()
        } else {
            missing.join(", ")
        }
    ));
    out.push_str(&format!(
        "One try of each of the {} open tasks, Sonnet 5.5 · medium (task means): {} recorded by the client, {} at today's list price, {} tokens ({} output), {} of agent time, {:.1} turns a task.\n\n",
        open.len(),
        money(open_once.cost),
        money(open_once.priced(SONNET)),
        millions(open_once.tokens()),
        millions(open_once.output),
        hours(open_once.seconds),
        open_once.turns / open.len().max(1) as f64,
    ));
    out.push_str(&format!(
        "A game-code task averages {} ({} tokens, {:.0} s); the long task is taken as 3 to 6 of them: {} to {}.\n\n",
        money(game_mean.cost),
        millions(game_mean.tokens()),
        game_mean.seconds,
        money(long_low.cost),
        money(long_high.cost)
    ));
    out.push_str("| Campaign part (3 tries per task) | Tokens | $ recorded-price basis | $ list price | Agent time |\n|---|---|---|---|---|\n");
    let line = |out: &mut String,
                name: &str,
                lo: &Usage,
                hi: &Usage,
                cost: &dyn Fn(&Usage) -> f64,
                list: &dyn Fn(&Usage) -> f64,
                time: (f64, f64)| {
        out.push_str(&format!(
            "| {name} | {} – {} | {} – {} | {} – {} | {} – {} |\n",
            millions(lo.tokens()),
            millions(hi.tokens()),
            money(cost(lo)),
            money(cost(hi)),
            money(list(lo)),
            money(list(hi)),
            hours(time.0),
            hours(time.1)
        ));
    };
    line(
        &mut out,
        "Sonnet 5.5 · medium, 41 open + long task",
        &sonnet_low,
        &sonnet_high,
        &recorded_cost,
        &sonnet_cost,
        (sonnet_low.seconds, sonnet_high.seconds),
    );
    // Haiku has no recorded run: its "recorded" column is its list price.
    line(
        &mut out,
        "Haiku 5.5 · medium, 41 open + long task",
        &haiku_low,
        &haiku_high,
        &haiku_cost,
        &haiku_cost,
        (
            sonnet_low.seconds * haiku_time_low,
            sonnet_high.seconds * haiku_time_high,
        ),
    );
    line(
        &mut out,
        "Second-agent step, Sonnet (7 follow-ups)",
        &second_s.0,
        &second_s.1,
        &recorded_cost,
        &sonnet_cost,
        (second_s.0.seconds, second_s.1.seconds),
    );
    line(
        &mut out,
        "Second-agent step, Haiku (7 follow-ups)",
        &second_h.0,
        &second_h.1,
        &haiku_cost,
        &haiku_cost,
        (
            second_s.0.seconds * haiku_time_low,
            second_s.1.seconds * haiku_time_high,
        ),
    );
    let without = (
        recorded_cost(&sonnet_low) + haiku_cost(&haiku_low),
        recorded_cost(&sonnet_high) + haiku_cost(&haiku_high),
    );
    let with = (
        without.0 + recorded_cost(&second_s.0) + haiku_cost(&second_h.0),
        without.1 + recorded_cost(&second_s.1) + haiku_cost(&second_h.1),
    );
    let tokens_without = (
        sonnet_low.tokens() + haiku_low.tokens(),
        sonnet_high.tokens() + haiku_high.tokens(),
    );
    let tokens_with = (
        tokens_without.0 + second_s.0.tokens() + second_h.0.tokens(),
        tokens_without.1 + second_s.1.tokens() + second_h.1.tokens(),
    );
    let time_without = (
        sonnet_low.seconds * (1.0 + haiku_time_low) + 2.0 * harness,
        sonnet_high.seconds * (1.0 + haiku_time_high) + 2.0 * harness,
    );
    let time_with = (
        time_without.0 + second_s.0.seconds * (1.0 + haiku_time_low),
        time_without.1 + second_s.1.seconds * (1.0 + haiku_time_high),
    );
    out.push_str(&format!(
        "\n**Baseline campaign, both models, 3 tries, one repair round:** {} – {} ({} – {} tokens), about {} – {} of wall time (harness included, one try at a time).\n\n",
        money(without.0), money(without.1), millions(tokens_without.0), millions(tokens_without.1), hours(time_without.0), hours(time_without.1)
    ));
    out.push_str(&format!(
        "**With the second-agent step:** {} – {} ({} – {} tokens), about {} – {}.\n",
        money(with.0),
        money(with.1),
        millions(tokens_with.0),
        millions(tokens_with.1),
        hours(time_with.0),
        hours(time_with.1)
    ));
    out
}
