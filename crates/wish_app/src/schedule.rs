//! Systems and the order they run in.

use std::collections::{BTreeMap, BTreeSet};

use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result, Rng};
use wish_ecs::{ChangeTick, World};

use crate::time::Time;

/// What a system receives besides the world.
pub struct SystemContext<'a> {
    /// The system's name.
    pub name: &'a str,
    /// The current simulation time (also in the world as the [`Time`] resource).
    pub time: Time,
    /// The system's own random stream. Use it for every random decision: it
    /// is seeded from the app's seed and the system's name, so results do
    /// not change when other systems are added or reordered.
    pub rng: &'a mut Rng,
    /// The change tick at the end of this system's previous run, or 0 on its
    /// first run. Pass it to `World::query_since` to see only what changed
    /// since then.
    pub last_run: ChangeTick,
}

/// The signature of a system: it reads and changes the world and may fail
/// with a diagnostic. A failing system is disabled; the others keep running.
pub type SystemFn = fn(&mut World, &mut SystemContext<'_>) -> Result<()>;

/// A named unit of game logic and its ordering constraints.
#[derive(Clone, Debug)]
pub struct System {
    pub(crate) name: String,
    pub(crate) run: SystemFn,
    pub(crate) before: BTreeSet<String>,
    pub(crate) after: BTreeSet<String>,
}

impl System {
    /// A system with no ordering constraints. Names must be unique within
    /// an app; they appear in diagnostics, and they seed the system's
    /// random stream.
    pub fn new(name: &str, run: SystemFn) -> Self {
        Self {
            name: name.to_owned(),
            run,
            before: BTreeSet::new(),
            after: BTreeSet::new(),
        }
    }

    /// Requires this system to run before the system named `other`.
    #[must_use]
    pub fn before(mut self, other: &str) -> Self {
        self.before.insert(other.to_owned());
        self
    }

    /// Requires this system to run after the system named `other`.
    #[must_use]
    pub fn after(mut self, other: &str) -> Self {
        self.after.insert(other.to_owned());
        self
    }

    /// The system's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Computes the order systems run in.
///
/// Constraints form a graph; the order is a topological sort that, whenever
/// several systems are free to go next, picks the one whose name sorts
/// first. The result depends only on names and constraints.
pub(crate) fn order(systems: &[System]) -> Result<Vec<usize>> {
    let index_of: BTreeMap<&str, usize> = systems
        .iter()
        .enumerate()
        .map(|(index, system)| (system.name.as_str(), index))
        .collect();
    let names: Vec<&str> = index_of.keys().copied().collect();

    // edges[a] holds the systems that must run after `a`.
    let mut edges: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); systems.len()];
    let mut incoming = vec![0usize; systems.len()];
    for (index, system) in systems.iter().enumerate() {
        for (other, earlier_is_self) in system
            .before
            .iter()
            .map(|other| (other, true))
            .chain(system.after.iter().map(|other| (other, false)))
        {
            let Some(&other_index) = index_of.get(other.as_str()) else {
                let relation = if earlier_is_self { "before" } else { "after" };
                return Err(Diagnostic::error(
                    "unknown_system",
                    format!(
                        "System '{}' is ordered {relation} '{other}', but no system has that name.",
                        system.name
                    ),
                    unknown_name_hint(other, &names),
                )
                .with_candidates(names.iter().copied()));
            };
            let (from, to) = if earlier_is_self {
                (index, other_index)
            } else {
                (other_index, index)
            };
            if edges[from].insert(to) {
                incoming[to] += 1;
            }
        }
    }

    // Ready systems, keyed by name so the smallest name comes out first.
    let mut ready: BTreeMap<&str, usize> = systems
        .iter()
        .enumerate()
        .filter(|(index, _)| incoming[*index] == 0)
        .map(|(index, system)| (system.name.as_str(), index))
        .collect();
    let mut sorted = Vec::with_capacity(systems.len());
    while let Some((_, index)) = ready.pop_first() {
        sorted.push(index);
        for &next in &edges[index] {
            incoming[next] -= 1;
            if incoming[next] == 0 {
                ready.insert(systems[next].name.as_str(), next);
            }
        }
    }

    if sorted.len() < systems.len() {
        let mut stuck: Vec<&str> = systems
            .iter()
            .enumerate()
            .filter(|(index, _)| incoming[*index] > 0)
            .map(|(_, system)| system.name.as_str())
            .collect();
        stuck.sort_unstable();
        return Err(Diagnostic::error(
            "system_order_cycle",
            format!(
                "The before/after constraints of these systems contradict each other: {}.",
                stuck.join(", ")
            ),
            "Each constraint says one system runs before another; somewhere among these they form a loop. Remove one of the constraints in the loop.",
        )
        .with_candidates(stuck));
    }
    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
        Ok(())
    }

    fn names(systems: &[System]) -> Vec<&str> {
        order(systems)
            .unwrap()
            .into_iter()
            .map(|index| systems[index].name.as_str())
            .collect()
    }

    #[test]
    fn unconstrained_systems_run_in_name_order() {
        let systems = [
            System::new("c", noop),
            System::new("a", noop),
            System::new("b", noop),
        ];
        assert_eq!(names(&systems), ["a", "b", "c"]);
    }

    #[test]
    fn constraints_win_over_names_and_insertion_order_does_not_matter() {
        let make = || {
            vec![
                System::new("render", noop).after("physics"),
                System::new("input", noop).before("movement"),
                System::new("movement", noop).before("physics"),
                System::new("physics", noop),
                System::new("audio", noop),
            ]
        };
        let expected = ["audio", "input", "movement", "physics", "render"];
        assert_eq!(names(&make()), expected);
        let mut reversed = make();
        reversed.reverse();
        assert_eq!(names(&reversed), expected);
    }

    #[test]
    fn cycles_and_unknown_names_are_explained() {
        let cycle = [
            System::new("a", noop).before("b"),
            System::new("b", noop).before("c"),
            System::new("c", noop).before("a"),
            System::new("d", noop),
        ];
        let error = order(&cycle).unwrap_err();
        assert_eq!(error.code, "system_order_cycle");
        assert_eq!(error.candidates, ["a", "b", "c"]);

        let unknown = [
            System::new("move", noop).after("inptu"),
            System::new("input", noop),
        ];
        let error = order(&unknown).unwrap_err();
        assert_eq!(error.code, "unknown_system");
        assert_eq!(error.hint, "Did you mean 'input'?");
    }
}
