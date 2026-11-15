//! Hostile and broken guests: whatever a module does, the call returns a
//! diagnostic, the engine keeps running, and the same input fails the same
//! way every time (design note §6, M5 exit criterion).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::{Rng, Severity};
use wish_script::{Limits, ScriptEngine, ScriptModule};

/// One module, many behaviours: `wish_run(system, frame)` picks by `system`.
const GUEST: &str = r#"
(module
  (import "wish" "log" (func $log (param i32 i32 i32)))
  (import "wish" "panic" (func $panic (param i32 i32 i32 i32 i32 i32)))
  (import "wish" "rng_next" (func $rng (result i64)))
  (memory (export "memory") 1)
  (global $calls (mut i32) (i32.const 0))
  (data (i32.const 16) "boom at src/lib.rs:7:5")
  (func (export "wish_abi_version") (result i32) (i32.const 1))
  (func (export "wish_alloc") (param i32) (result i32) (i32.const 1024))
  (func $recurse (param i32) (result i32)
    (call $recurse (i32.add (local.get 0) (i32.const 1))))
  (func (export "wish_run") (param $system i32) (param $frame i32) (result i32)
    (local $i i32)
    ;; 0: a well-behaved system: frame[0] += 1, and it counts its calls.
    (if (i32.eq (local.get $system) (i32.const 0)) (then
      (i32.store8 (local.get $frame) (i32.add (i32.load8_u (local.get $frame)) (i32.const 1)))
      (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
      (return (global.get $calls))))
    ;; 1: a loop that never ends.
    (if (i32.eq (local.get $system) (i32.const 1)) (then
      (loop $forever (br $forever))))
    ;; 2: unreachable.
    (if (i32.eq (local.get $system) (i32.const 2)) (then unreachable))
    ;; 3: recursion without a base case.
    (if (i32.eq (local.get $system) (i32.const 3)) (then
      (return (call $recurse (i32.const 0)))))
    ;; 4: grows memory by 4 GiB worth of pages, then gives up the way a
    ;;    Rust allocator does.
    (if (i32.eq (local.get $system) (i32.const 4)) (then
      (drop (memory.grow (i32.const 2000)))
      unreachable))
    ;; 5: division by zero.
    (if (i32.eq (local.get $system) (i32.const 5)) (then
      (return (i32.div_u (i32.const 1) (i32.load8_u (i32.add (local.get $frame) (i32.const 1)))))))
    ;; 6: a panic, reported through the SDK's hook.
    (if (i32.eq (local.get $system) (i32.const 6)) (then
      (call $panic (i32.const 16) (i32.const 4) (i32.const 24) (i32.const 10) (i32.const 7) (i32.const 5))
      unreachable))
    ;; 7: logs a thousand times.
    (if (i32.eq (local.get $system) (i32.const 7)) (then
      (loop $again
        (call $log (i32.const 1) (i32.const 16) (i32.const 4))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $again (i32.lt_u (local.get $i) (i32.const 1000))))
      (return (i32.const 0))))
    ;; 8: draws a random number into the frame.
    (if (i32.eq (local.get $system) (i32.const 8)) (then
      (i64.store (local.get $frame) (call $rng))
      (return (i32.const 0))))
    ;; 9: writes far outside its memory.
    (if (i32.eq (local.get $system) (i32.const 9)) (then
      (i32.store (i32.const 0x7fff0000) (i32.const 1))))
    ;; 10: logs with a pointer and length that make no sense.
    (if (i32.eq (local.get $system) (i32.const 10)) (then
      (call $log (i32.const 2) (i32.const -5) (i32.const 999999999))
      (return (i32.const 0))))
    (i32.const -1))
)
"#;

fn engine() -> ScriptEngine {
    ScriptEngine::new(Limits {
        fuel_per_call: 1_000_000,
        memory_bytes: 16 * 1024 * 1024,
    })
    .unwrap()
}

fn guest(engine: &ScriptEngine) -> ScriptModule {
    ScriptModule::load(engine, "scripts/game.wasm", &wat::parse_str(GUEST).unwrap()).unwrap()
}

fn fail(module: &ScriptModule, system: i32) -> wish_core::Diagnostic {
    let mut frame = [0u8; 16];
    module
        .run(system, &mut frame, &mut Rng::from_seed(1))
        .unwrap_err()
}

#[test]
fn a_well_behaved_call_changes_the_frame_and_keeps_no_state() {
    let engine = engine();
    let module = guest(&engine);
    let mut frame = [41u8; 16];
    let mut rng = Rng::from_seed(1);
    let first = module.run(0, &mut frame, &mut rng).unwrap();
    assert_eq!(frame[0], 42, "the guest's change comes back");
    assert_eq!(frame[1], 41, "the rest is untouched");
    // The module counts its calls in a global; every call is a fresh
    // instance, so it always sees its first call.
    let second = module.run(0, &mut frame, &mut rng).unwrap();
    assert_eq!((first.returned, second.returned), (1, 1));
    assert_eq!(frame[0], 43);
    assert_eq!(first.fuel_used, second.fuel_used, "fuel is deterministic");
    assert!(first.fuel_used > 0);
}

#[test]
fn every_kind_of_failure_is_explained_and_the_engine_keeps_going() {
    let engine = engine();
    let module = guest(&engine);
    let cases = [
        (1, "script_out_of_fuel"),
        (2, "script_trap"),
        (3, "script_stack_overflow"),
        (4, "script_out_of_memory"),
        (5, "script_trap"),
        (6, "script_panicked"),
        (9, "script_trap"),
    ];
    for (system, code) in cases {
        let first = fail(&module, system);
        assert_eq!(first.code, code, "system {system}: {first:?}");
        assert!(!first.hint.is_empty(), "system {system} has a hint");
        // The same input fails the same way: no timing is involved.
        assert_eq!(fail(&module, system), first, "system {system} again");
        // And the module still works afterwards.
        let mut frame = [0u8; 16];
        module.run(0, &mut frame, &mut Rng::from_seed(1)).unwrap();
        assert_eq!(frame[0], 1);
    }
    let panic = fail(&module, 6);
    assert_eq!(panic.message, "Game code panicked: boom");
    assert_eq!(panic.location.file.as_deref(), Some("src/lib.rs"));
    assert_eq!(
        (panic.location.line, panic.location.column),
        (Some(7), Some(5))
    );
    let memory = fail(&module, 4);
    assert!(memory.message.contains("limit is 16 MiB"), "{memory:?}");
}

#[test]
fn logs_are_bounded_and_garbage_pointers_are_harmless() {
    let engine = engine();
    let module = guest(&engine);
    let mut frame = [0u8; 16];
    let report = module.run(7, &mut frame, &mut Rng::from_seed(1)).unwrap();
    assert_eq!(report.logs.len(), 100);
    assert_eq!(report.logs_dropped, 900);
    assert_eq!(report.logs[0].severity, Severity::Warning);
    assert_eq!(report.logs[0].message, "boom");
    let garbage = module.run(10, &mut frame, &mut Rng::from_seed(1)).unwrap();
    assert_eq!(garbage.logs[0].message, "");
}

#[test]
fn random_numbers_come_from_the_systems_own_stream() {
    let engine = engine();
    let module = guest(&engine);
    let draw = |rng: &mut Rng| {
        let mut frame = [0u8; 16];
        module.run(8, &mut frame, rng).unwrap();
        u64::from_ne_bytes(frame[..8].try_into().unwrap())
    };
    let mut rng = Rng::from_seed(7);
    let mut expected = Rng::from_seed(7);
    assert_eq!(draw(&mut rng), expected.next_u64());
    assert_eq!(draw(&mut rng), expected.next_u64(), "the stream advanced");
    assert_eq!(rng, expected);
}

#[test]
fn modules_that_are_not_game_modules_are_refused() {
    let engine = engine();
    let load = |text: &str| {
        ScriptModule::load(&engine, "scripts/game.wasm", &wat::parse_str(text).unwrap())
            .unwrap_err()
    };
    let wasi = load(
        r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#,
    );
    assert_eq!(wasi.code, "script_forbidden_import");
    assert!(wasi.message.contains("wasi_snapshot_preview1.fd_write"));
    let clock = load(r#"(module (import "wish" "clock" (func (result i64))))"#);
    assert_eq!(clock.code, "script_forbidden_import");
    let empty = load("(module)");
    assert_eq!(empty.code, "script_missing_export");
    let old = load(
        r#"(module (memory (export "memory") 1)
            (func (export "wish_abi_version") (result i32) (i32.const 0))
            (func (export "wish_alloc") (param i32) (result i32) (i32.const 0))
            (func (export "wish_run") (param i32 i32) (result i32) (i32.const 0)))"#,
    );
    assert_eq!(old.code, "script_abi_mismatch");
    let wrong_type = load(
        r#"(module (memory (export "memory") 1)
            (func (export "wish_abi_version") (result i64) (i64.const 1))
            (func (export "wish_alloc") (param i32) (result i32) (i32.const 0))
            (func (export "wish_run") (param i32 i32) (result i32) (i32.const 0)))"#,
    );
    assert_eq!(wrong_type.code, "script_call_failed");
    let garbage =
        ScriptModule::load(&engine, "scripts/game.wasm", b"\0asm\x01\0\0\0garbage").unwrap_err();
    assert_eq!(garbage.code, "script_invalid_module");
    let not_wasm = ScriptModule::load(&engine, "scripts/game.wasm", b"hello").unwrap_err();
    assert_eq!(not_wasm.code, "script_invalid_module");
}
