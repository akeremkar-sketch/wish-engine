//! M5 WP1 proof work: how game code gets compiled on a machine with no
//! developer tools (design note §5, ADR-023). Measures path A (cargo with a
//! procedural macro) and path C (one `rustc` call against an SDK built in
//! advance), and builds path C with a copied, minimal toolchain in an empty
//! environment — no PATH, no Visual Studio, no rustup.
//!
//! `cargo test --release -p wish_script --test build_paths -- --ignored --nocapture`

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

use wish_core::Rng;
use wish_script::{Limits, ScriptEngine, ScriptModule};

fn guests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guests")
}

fn rustc_says(args: &[&str]) -> String {
    let out = Command::new("rustc").args(args).output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn copy_dir(from: &Path, to: &Path) -> u64 {
    std::fs::create_dir_all(to).unwrap();
    let mut bytes = 0;
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            bytes += copy_dir(&entry.path(), &target);
        } else {
            bytes += std::fs::copy(entry.path(), &target).unwrap();
        }
    }
    bytes
}

fn timed(command: &mut Command) -> (f64, Output) {
    let start = Instant::now();
    let out = command.output().unwrap();
    (start.elapsed().as_secs_f64(), out)
}

fn ok(what: &str, out: &Output) {
    assert!(
        out.status.success(),
        "{what} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Changes the marker line so the next build has real work to do.
fn edit(file: &Path, n: u32) {
    let text = std::fs::read_to_string(file).unwrap();
    let start = text.find("let _ = count + ").unwrap();
    let end = start + text[start..].find(';').unwrap();
    let changed = format!("{}let _ = count + {n}{}", &text[..start], &text[end..]);
    std::fs::write(file, changed).unwrap();
}

fn runs(bytes: &[u8]) {
    let engine = ScriptEngine::new(Limits::default()).unwrap();
    let module = ScriptModule::load(&engine, "game.wasm", bytes).unwrap();
    let mut frame = vec![0u8; 8 + 24];
    frame[..4].copy_from_slice(&1u32.to_le_bytes());
    frame[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    frame[20..24].copy_from_slice(&3.0f32.to_le_bytes());
    module.run(0, &mut frame, &mut Rng::from_seed(1)).unwrap();
    assert_eq!(f32::from_le_bytes(frame[8..12].try_into().unwrap()), 3.0);
}

#[test]
#[ignore = "measurement; run by hand in release"]
fn build_paths() {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("build-paths");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let host = rustc_says(&["-vV"])
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
        .unwrap();
    let sysroot = PathBuf::from(rustc_says(&["--print", "sysroot"]));
    let linker_var = format!(
        "CARGO_TARGET_{}_LINKER",
        host.to_uppercase().replace('-', "_")
    );
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    // Path A: cargo, with the SDK's procedural macro.
    let a = work.join("a");
    for name in ["sdk_spike", "macros_spike", "game_spike"] {
        copy_dir(&guests().join(name), &a.join(name));
    }
    let a_build = |extra: &[(&str, &str)]| {
        let mut command = Command::new(&cargo);
        command
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .arg("--manifest-path")
            .arg(a.join("game_spike/Cargo.toml"))
            .arg("--target-dir")
            .arg(a.join("target"))
            .env("RUSTFLAGS", "");
        for (key, value) in extra {
            command.env(key, value);
        }
        timed(&mut command)
    };
    let (_, poisoned) = a_build(&[(&linker_var, "no-such-linker.exe")]);
    let a_needs_host_linker = !poisoned.status.success();
    let _ = std::fs::remove_dir_all(a.join("target"));
    let (a_cold, out) = a_build(&[]);
    ok("path A", &out);
    edit(&a.join("game_spike/src/lib.rs"), 2);
    let (a_edit, out) = a_build(&[]);
    ok("path A after an edit", &out);
    runs(&std::fs::read(a.join("target/wasm32-unknown-unknown/release/game_spike.wasm")).unwrap());

    // Path C, step 1 (the engine's own build, where a linker exists): the
    // SDK for wasm32 and its macro for the host.
    let c = work.join("c");
    let deps = c.join("deps");
    std::fs::create_dir_all(&deps).unwrap();
    let rustc = |args: &[&str]| {
        let mut command = Command::new("rustc");
        command.args(args);
        command
    };
    let macros = guests().join("macros_spike/src/lib.rs");
    let (_, out) = timed(
        rustc(&[
            "--edition",
            "2024",
            "--crate-type",
            "proc-macro",
            "--extern",
            "proc_macro",
            "--crate-name",
            "macros_spike",
            "-C",
            "opt-level=2",
            "--out-dir",
        ])
        .arg(&deps)
        .arg(&macros),
    );
    ok("building the macro", &out);
    let sdk = guests().join("sdk_spike/src/lib.rs");
    let (_, out) = timed(
        rustc(&[
            "--edition",
            "2024",
            "--crate-type",
            "rlib",
            "--crate-name",
            "sdk_spike",
            "--target",
            "wasm32-unknown-unknown",
            "-C",
            "opt-level=2",
            "-C",
            "panic=abort",
            "-C",
            "debuginfo=line-tables-only",
            "--out-dir",
        ])
        .arg(&deps)
        .arg(&sdk),
    );
    ok("building the SDK", &out);
    let macro_file = std::fs::read_dir(&deps)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            name.contains("macros_spike")
                && (name.ends_with(".dll") || name.ends_with(".so") || name.ends_with(".dylib"))
        })
        .unwrap();

    // Path C, step 2: a minimal copy of the toolchain, as the engine would
    // ship it.
    let toolchain = c.join("toolchain");
    let mut size = 0u64;
    std::fs::create_dir_all(toolchain.join("bin")).unwrap();
    std::fs::create_dir_all(toolchain.join("lib")).unwrap();
    for (dir, keep) in [("bin", "bin"), ("lib", "lib")] {
        for entry in std::fs::read_dir(sysroot.join(dir)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            let wanted = name == "rustc.exe"
                || name == "rustc"
                || name.ends_with(".dll")
                || name.contains(".so");
            if entry.file_type().unwrap().is_file() && wanted {
                size += std::fs::copy(entry.path(), toolchain.join(keep).join(&name)).unwrap();
            }
        }
    }
    let rustlib = Path::new("lib/rustlib");
    size += copy_dir(
        &sysroot.join(rustlib).join("wasm32-unknown-unknown"),
        &toolchain.join(rustlib).join("wasm32-unknown-unknown"),
    );
    size += copy_dir(
        &sysroot.join(rustlib).join(&host).join("bin"),
        &toolchain.join(rustlib).join(&host).join("bin"),
    );
    let exe = if cfg!(windows) { "rustc.exe" } else { "rustc" };

    // Path C, step 3: the game's build, in an empty environment.
    let game = c.join("game");
    std::fs::create_dir_all(game.join("src")).unwrap();
    std::fs::copy(
        guests().join("game_spike/src/lib.rs"),
        game.join("src/lib.rs"),
    )
    .unwrap();
    let c_build = |print_link: bool| {
        let mut command = Command::new(toolchain.join("bin").join(exe));
        command.env_clear().env("PATH", toolchain.join("bin"));
        for keep in ["SystemRoot", "SYSTEMROOT", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(keep) {
                command.env(keep, value);
            }
        }
        command.current_dir(&game).args([
            "--edition",
            "2024",
            "--crate-type",
            "cdylib",
            "--crate-name",
            "game",
            "--target",
            "wasm32-unknown-unknown",
            "-C",
            "opt-level=1",
            "-C",
            "panic=abort",
            "-C",
            "debuginfo=line-tables-only",
            "-C",
            &format!("incremental={}", c.join("incremental").display()),
            "--extern",
            &format!("sdk_spike={}", deps.join("libsdk_spike.rlib").display()),
            "--extern",
            &format!("macros_spike={}", macro_file.display()),
            "-L",
            &format!("dependency={}", deps.display()),
            "-o",
            "game.wasm",
            "src/lib.rs",
        ]);
        if print_link {
            command.args(["--print", "link-args"]);
        }
        timed(&mut command)
    };
    let (c_cold, out) = c_build(true);
    ok("path C (empty environment, copied toolchain)", &out);
    let link_line = String::from_utf8_lossy(&out.stdout).to_string();
    edit(&game.join("src/lib.rs"), 2);
    let (c_edit, out) = c_build(false);
    ok("path C after an edit", &out);
    let (c_same, out) = c_build(false);
    ok("path C, nothing changed", &out);
    let wasm = std::fs::read(game.join("game.wasm")).unwrap();
    runs(&wasm);

    println!("host: {host}");
    println!(
        "path A (cargo + proc macro): needs the host linker: {a_needs_host_linker}; cold {a_cold:.2} s, after an edit {a_edit:.2} s"
    );
    println!(
        "path C (rustc, prebuilt SDK, empty env): cold {c_cold:.2} s, after an edit {c_edit:.2} s, unchanged {c_same:.2} s; module {} KiB",
        wasm.len() / 1024
    );
    println!(
        "minimal toolchain: {:.0} MiB",
        size as f64 / (1024.0 * 1024.0)
    );
    let linker = link_line
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('"')
        .to_owned();
    println!("path C linker: {linker}");
    assert!(
        linker.contains("rust-lld") || linker.contains("wasm-ld"),
        "{link_line}"
    );
}
