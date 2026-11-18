//! Rules of the GPU code that no picture shows when they are broken, checked
//! by reading the source (the way `wish_control/tests/diagnostics_catalog.rs`
//! reads it for diagnostic codes).
//!
//! Each rule was learned from a failure that tests of pictures caught only
//! now and then, or not at all (design notes M3 §14 and M12 render
//! foundation §3.2):
//!
//! - a frame's commands are submitted with `Gpu::finish`, which waits under
//!   the GPU lock. Without the wait, two renderers on the software adapter
//!   draw each other's camera, about once in a dozen runs on a busy machine;
//! - every texture and buffer is created through `Gpu::create_texture` and
//!   `Gpu::create_buffer`, so that `Gpu::created` counts them all and "a
//!   second frame creates nothing" is a claim a test can check;
//! - no buffer is mapped when it is created (`create_buffer_init` and
//!   `mapped_at_creation` panic when the mapping fails).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// The file that wraps the device: the one place the rules are implemented.
const GPU: &str = "gpu.rs";
/// A probe of the adapter itself, run by hand (`#[ignore]`): it measures
/// what the driver does with raw calls, so it makes them.
const PROBE: &str = "renderer/warp_probe.rs";

fn collect(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, root, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

/// Every source file of the crate: (path under `src`, text).
fn sources() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    collect(&root, &root, &mut out);
    out.sort();
    assert!(
        out.iter().any(|(name, _)| name == GPU) && out.iter().any(|(name, _)| name == PROBE),
        "the files the rules name exist"
    );
    out
}

/// Where `needle` is called in the code of `files`, as `file:line`, outside
/// the files in `allowed`; comment lines are not code. `receiver` says which
/// calls are fine anyway, by what the method is called on: the text before
/// the needle on its line or, when the formatter broke the chain there, the
/// line above.
fn calls(
    files: &[(String, String)],
    needle: &str,
    allowed: &[&str],
    receiver: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        if allowed.contains(&file.as_str()) {
            continue;
        }
        let lines: Vec<(usize, &str)> = text
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line))
            .filter(|(_, line)| !line.trim_start().starts_with("//"))
            .collect();
        for (position, (number, line)) in lines.iter().enumerate() {
            let Some(at) = line.find(needle) else {
                continue;
            };
            let before = line[..at].trim();
            let on = if before.is_empty() {
                position
                    .checked_sub(1)
                    .and_then(|above| lines.get(above))
                    .map_or("", |(_, above)| above.trim())
            } else {
                before
            };
            if !receiver(on) {
                found.push(format!("{file}:{number}"));
            }
        }
    }
    found
}

/// `gpu.create_texture(..)` and `gpu.create_buffer(..)` count what they
/// make; a call on the device itself does not.
fn counted(on: &str) -> bool {
    on.ends_with("gpu")
}

#[test]
fn a_frame_is_submitted_only_through_gpu_finish() {
    let files = sources();
    let direct = calls(&files, ".submit(", &[GPU, PROBE], |_| false);
    assert!(
        direct.is_empty(),
        "these submit commands without waiting under the GPU lock; use `gpu.finish(encoder)` as the last thing inside `Gpu::checked`: {direct:?}"
    );
    // The rule is implemented where the scan lets it be: one submit in the
    // crate, followed by the wait.
    assert_eq!(
        calls(&files, ".submit(", &[PROBE], |_| false).len(),
        1,
        "one place submits"
    );
    let gpu = &files.iter().find(|(name, _)| name == GPU).unwrap().1;
    assert!(gpu.contains("self.queue.submit([encoder.finish()]);\n        self.wait()"));
}

#[test]
fn textures_and_buffers_are_created_only_through_the_counted_helpers() {
    let files = sources();
    for needle in [".create_texture(", ".create_buffer("] {
        let direct = calls(&files, needle, &[GPU, PROBE], counted);
        assert!(
            direct.is_empty(),
            "these create a GPU object that `Gpu::created` does not count; call `gpu{needle}..)` instead: {direct:?}"
        );
    }
}

#[test]
fn no_buffer_is_mapped_when_it_is_created() {
    let files = sources();
    for needle in ["create_buffer_init", "mapped_at_creation: true"] {
        let found = calls(&files, needle, &[], |_| false);
        assert!(
            found.is_empty(),
            "`{needle}` can panic when the mapping fails; fill the buffer through the queue (`buffer_with`): {found:?}"
        );
    }
}

#[test]
fn the_scan_finds_the_mistakes_it_is_there_for() {
    // The same scan on a file with each mistake next to its right form.
    let text = "\
fn frame(gpu: &Gpu) {
    // gpu.queue.submit([encoder.finish()]) is how it used to be.
    gpu.queue.submit([encoder.finish()]);
    let a = gpu
        .device
        .create_texture(&descriptor);
    let b = gpu
        .create_texture(&descriptor);
    let c = gpu.device.create_buffer(&descriptor);
    let d = gpu.create_buffer(&descriptor);
    let e = device.create_buffer_init(&descriptor);
}
";
    let files = vec![
        ("pass.rs".to_owned(), text.to_owned()),
        ("gpu.rs".to_owned(), text.to_owned()),
    ];
    assert_eq!(calls(&files, ".submit(", &[GPU], |_| false), ["pass.rs:3"]);
    assert_eq!(
        calls(&files, ".create_texture(", &[GPU], counted),
        ["pass.rs:6"]
    );
    assert_eq!(
        calls(&files, ".create_buffer(", &[GPU], counted),
        ["pass.rs:9"]
    );
    assert_eq!(
        calls(&files, "create_buffer_init", &[], |_| false),
        ["pass.rs:11", "gpu.rs:11"]
    );
}
