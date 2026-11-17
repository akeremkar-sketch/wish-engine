//! Crash reports (M9): a panic that escapes `main` leaves a report on this
//! computer; a caught one leaves none; nothing is sent anywhere.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_control::crash::{guard, install, reports, told};

// One test: the panic hook and the noted panic are the process's own.
#[test]
fn an_escaped_panic_leaves_a_report_and_a_caught_one_does_not() {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("crash-reports");
    let _ = std::fs::remove_dir_all(&folder);
    install();
    // Caught inside: the program goes on, no report.
    let fine = guard("tester", Some(&folder), || {
        std::panic::catch_unwind(|| panic!("caught on purpose")).is_err()
    });
    assert_eq!(fine, Ok(true));
    assert!(reports(&folder).is_empty());
    // Escaped: a report with what and where.
    let crashed = guard("tester", Some(&folder), || -> u32 {
        panic!("escaped on purpose")
    });
    let path = crashed.unwrap_err().expect("a report was written");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("program: tester"), "{text}");
    assert!(text.contains("error: escaped on purpose"), "{text}");
    assert!(text.contains("crash.rs:"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert_eq!(reports(&folder), std::slice::from_ref(&path));
    assert!(told("wen", Some(&path)).contains("nothing was sent"));
    // No folder to write to: still no crash of its own.
    let nowhere = guard("tester", None, || -> u32 { panic!("again") });
    assert_eq!(nowhere, Err(None));
}
