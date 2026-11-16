//! Regression test for the M5 closing review's finding T5: `wen replay
//! <name>` and `wen test <files>` take their names as positional arguments.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

fn wen(arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args(arguments)
        .output()
        .expect("wen runs");
    assert!(
        !output.status.success(),
        "these calls name things that do not exist"
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn t5_replay_and_test_take_their_names_as_arguments() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("review-m5-cli");
    let _ = std::fs::remove_dir_all(&dir);
    wish_control::scaffold::create_project(&dir, "Game", "wen").unwrap();
    let root = dir.to_string_lossy().to_string();
    let replay = wen(&["replay", "nope", "--root", &root]);
    assert!(replay.contains("replay_not_found"), "{replay}");
    assert!(!replay.contains("does not accept"), "{replay}");
    let test = wen(&["test", "tests/nope.test.json", "--root", &root]);
    assert!(test.contains("test_not_found"), "{test}");
    assert!(!test.contains("does not accept"), "{test}");
}
