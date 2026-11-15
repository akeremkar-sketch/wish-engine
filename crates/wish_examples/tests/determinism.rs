//! M1 exit criteria for determinism, run on the `bounce` example:
//!
//! - 10,000 ticks run twice give the same world hash at every checkpoint.
//! - Snapshot at tick 5,000, restore, continue: the same hash at 10,000 as
//!   an uninterrupted run, also when restored into a freshly built app.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_app::{App, AppConfig};
use wish_core::Hash128;
use wish_examples::bounce::Stats;

const TICKS: u64 = 10_000;
const HALF: u64 = TICKS / 2;
const CHECK_EVERY: u64 = 100;

fn app(seed: u64) -> App {
    wish_examples::build(
        "bounce",
        AppConfig {
            seed,
            ..AppConfig::default()
        },
    )
    .unwrap()
}

/// Runs to `until`, recording the app hash every `CHECK_EVERY` ticks.
fn run_recording(app: &mut App, until: u64) -> Vec<(u64, Hash128)> {
    let mut hashes = Vec::new();
    while app.tick() < until {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        if app.tick().is_multiple_of(CHECK_EVERY) {
            hashes.push((app.tick(), app.hash()));
        }
    }
    hashes
}

#[test]
fn ten_thousand_ticks_twice_give_identical_hashes() {
    let mut first = app(7);
    let mut second = app(7);
    let first_hashes = run_recording(&mut first, TICKS);
    let second_hashes = run_recording(&mut second, TICKS);
    assert_eq!(first_hashes.len(), (TICKS / CHECK_EVERY) as usize);
    if let Some(index) = first_hashes
        .iter()
        .zip(&second_hashes)
        .position(|(a, b)| a != b)
    {
        panic!("runs diverged at tick {}", first_hashes[index].0);
    }

    // The example really does exercise the engine over the run.
    let stats = *first.world().resource::<Stats>().unwrap();
    assert!(stats.spawned > 5_000, "{stats:?}");
    assert!(stats.despawned > 3_000, "{stats:?}");
    assert!(stats.bounces > 1_000, "{stats:?}");
    assert!(first.world().entity_count() > 50);
    assert!(first.world().texts().len() > 1, "labels were interned");

    // A different seed gives a different world.
    let mut seven = app(7);
    let mut eight = app(8);
    run_recording(&mut seven, 1_000);
    run_recording(&mut eight, 1_000);
    assert_ne!(seven.world().hash(), eight.world().hash());
}

#[test]
fn snapshot_restore_continue_matches_an_uninterrupted_run() {
    let mut uninterrupted = app(11);
    run_recording(&mut uninterrupted, TICKS);
    let expected = uninterrupted.hash();

    let mut interrupted = app(11);
    run_recording(&mut interrupted, HALF);
    let snapshot = interrupted.snapshot();
    let at_half = interrupted.hash();

    // Continue, then rewind to the snapshot and continue again.
    run_recording(&mut interrupted, TICKS);
    assert_eq!(interrupted.hash(), expected, "the first continuation");
    interrupted.restore(&snapshot).unwrap();
    assert_eq!(interrupted.tick(), HALF);
    assert_eq!(
        interrupted.hash(),
        at_half,
        "restoring gives back the exact state"
    );
    run_recording(&mut interrupted, TICKS);
    assert_eq!(interrupted.hash(), expected, "the replay after restoring");

    // A freshly built app restored from the snapshot continues the same way.
    let mut fresh = app(11);
    fresh.restore(&snapshot).unwrap();
    assert_eq!(fresh.hash(), at_half);
    run_recording(&mut fresh, TICKS);
    assert_eq!(
        fresh.hash(),
        expected,
        "a fresh app restored from the snapshot"
    );
}

#[test]
fn snapshots_refuse_a_different_app() {
    let mut source = app(1);
    source.run(10).unwrap();
    let snapshot = source.snapshot();

    let mut other_seed = app(2);
    assert_eq!(
        other_seed.restore(&snapshot).unwrap_err().code,
        "snapshot_mismatch"
    );

    let mut bare = App::new(AppConfig {
        seed: 1,
        ..AppConfig::default()
    });
    let error = bare.restore(&snapshot).unwrap_err();
    assert_eq!(error.code, "snapshot_mismatch");
    assert!(
        error.hint.contains("bounce") || error.hint.contains("spawn"),
        "{}",
        error.hint
    );
}

#[test]
fn unknown_examples_suggest_a_name() {
    let error = match wish_examples::build("bounse", AppConfig::default()) {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    };
    assert_eq!(error.code, "unknown_example");
    assert_eq!(error.hint, "Did you mean 'bounce'?");
}
