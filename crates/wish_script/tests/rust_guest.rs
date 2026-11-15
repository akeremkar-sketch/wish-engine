//! A real Rust guest (built for wasm32 by this test): the frame round trip,
//! Rust panics with their message, and traps pointing at the source line.
//! The `#[ignore]`d test measures call cost (design note §6, ADR-011).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Instant;

use wish_core::Rng;
use wish_script::{Limits, ScriptEngine, ScriptModule};

const SOURCE: &str = include_str!("../guests/spike/src/lib.rs");

/// Builds the guest once per test process and returns its bytes.
fn guest_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guests/spike/Cargo.toml");
        let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("guests");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .args([
                "build",
                "--release",
                "--quiet",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target)
            // Measurements try other code generation flags this way.
            .env(
                "RUSTFLAGS",
                std::env::var("SPIKE_RUSTFLAGS").unwrap_or_default(),
            )
            .status()
            .unwrap();
        assert!(
            status.success(),
            "the guest builds (rustup target add wasm32-unknown-unknown?)"
        );
        std::fs::read(target.join("wasm32-unknown-unknown/release/spike_guest.wasm")).unwrap()
    })
}

fn module(engine: &ScriptEngine) -> ScriptModule {
    ScriptModule::load(engine, "scripts/spike.wasm", guest_bytes()).unwrap()
}

/// A mover frame: count, dt, positions, velocities.
fn frame(count: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + count * 24);
    bytes.extend((count as u32).to_le_bytes());
    bytes.extend(0.5f32.to_le_bytes());
    for i in 0..count {
        for axis in [i as f32, 0.0, 0.0] {
            bytes.extend(axis.to_le_bytes());
        }
    }
    for _ in 0..count {
        for axis in [2.0f32, 4.0, -2.0] {
            bytes.extend(axis.to_le_bytes());
        }
    }
    bytes
}

fn position(frame: &[u8], i: usize) -> [f32; 3] {
    let at = 8 + i * 12;
    std::array::from_fn(|axis| {
        f32::from_le_bytes(frame[at + axis * 4..at + axis * 4 + 4].try_into().unwrap())
    })
}

#[test]
fn the_guest_moves_what_it_is_given() {
    let engine = ScriptEngine::new(Limits::default()).unwrap();
    let module = module(&engine);
    let mut bytes = frame(3);
    let report = module.run(0, &mut bytes, &mut Rng::from_seed(1)).unwrap();
    assert_eq!(report.returned, 3);
    assert_eq!(position(&bytes, 0), [1.0, 2.0, -1.0]);
    assert_eq!(position(&bytes, 2), [3.0, 2.0, -1.0]);
}

fn line_of(text: &str) -> u32 {
    let index = SOURCE.lines().position(|line| line.contains(text)).unwrap();
    u32::try_from(index).unwrap() + 1
}

#[test]
fn panics_and_traps_point_at_the_game_code() {
    let engine = ScriptEngine::new(Limits::default()).unwrap();
    let module = module(&engine);
    let fail = |system| {
        module
            .run(system, &mut frame(0), &mut Rng::from_seed(1))
            .unwrap_err()
    };
    let index = fail(1);
    assert_eq!(index.code, "script_panicked");
    assert!(index.message.contains("index out of bounds"), "{index:?}");
    assert_eq!(index.location.file.as_deref(), Some("src/lib.rs"));
    assert_eq!(
        index.location.line,
        Some(line_of("values[std::hint::black_box(3)]")),
        "{index:?}"
    );
    let unwrap = fail(2);
    assert_eq!(unwrap.code, "script_panicked");
    assert!(unwrap.message.contains("None"), "{unwrap:?}");
    let trap = fail(3);
    assert_eq!(trap.code, "script_trap");
    let file = trap.location.file.clone().unwrap_or_default();
    assert!(file.ends_with("src/lib.rs"), "{trap:?}");
    assert_eq!(
        trap.location.line,
        Some(line_of("core::arch::wasm32::unreachable()")),
        "{trap:?}"
    );
    assert!(trap.message.contains("trap_here"), "{trap:?}");
}

fn per_call(mut f: impl FnMut(), calls: u32) -> f64 {
    f();
    let start = Instant::now();
    for _ in 0..calls {
        f();
    }
    start.elapsed().as_secs_f64() * 1e6 / f64::from(calls)
}

/// `cargo test --release -p wish_script --test rust_guest -- --ignored --nocapture`
#[test]
#[ignore = "measurement; run by hand in release"]
fn measure_call_cost() {
    let engine = ScriptEngine::new(Limits::default()).unwrap();
    let start = Instant::now();
    let module = module(&engine);
    println!(
        "module: {} bytes, compile + validate {:.1} ms",
        guest_bytes().len(),
        start.elapsed().as_secs_f64() * 1e3
    );
    for count in [0usize, 1, 1_000, 10_000] {
        let template = frame(count);
        let mut bytes = template.clone();
        let mut rng = Rng::from_seed(1);
        let calls = if count >= 10_000 { 200 } else { 2_000 };
        let wasm = per_call(
            || {
                bytes.copy_from_slice(&template);
                module.run(0, &mut bytes, &mut rng).unwrap();
            },
            calls,
        );
        // The same work done natively, on the same bytes.
        let mut native_bytes = template.clone();
        let native = per_call(
            || {
                native_bytes.copy_from_slice(&template);
                let (head, rest) = native_bytes.split_at_mut(8 + count * 12);
                let dt = f32::from_le_bytes(head[4..8].try_into().unwrap());
                for i in 0..count {
                    for axis in 0..3 {
                        let p = 8 + i * 12 + axis * 4;
                        let v = i * 12 + axis * 4;
                        let value = f32::from_le_bytes(head[p..p + 4].try_into().unwrap())
                            + f32::from_le_bytes(rest[v..v + 4].try_into().unwrap()) * dt;
                        head[p..p + 4].copy_from_slice(&value.to_le_bytes());
                    }
                }
                std::hint::black_box(&native_bytes);
            },
            calls,
        );
        println!(
            "{count:>6} entities: wasm {wasm:>9.2} us/call ({:>7.1} ns/entity), native {native:>8.2} us",
            if count == 0 {
                0.0
            } else {
                (wasm * 1e3) / count as f64
            }
        );
    }
}
