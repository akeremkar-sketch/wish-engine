//! Crash reports (M9): a panic that escapes `main` or a thread's body
//! leaves a report on this computer; a caught one leaves none; a report
//! never carries another thread's panic; nothing is sent anywhere.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_control::crash::{MAX_REPORTS, guard, install, reports, thread, told};

// One test: the panic hook and the notes are the process's own.
#[test]
fn escaped_panics_leave_reports_and_caught_ones_do_not() {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("crash-reports");
    let _ = std::fs::remove_dir_all(&base);
    // The first folder cannot be written (it is a file): the next is used.
    std::fs::create_dir_all(&base).unwrap();
    let blocked = base.join("blocked");
    std::fs::write(&blocked, "a file, not a folder").unwrap();
    let folder = base.join("reports");
    install("tester", vec![blocked, folder.clone()]);

    // Caught inside: the program goes on, no report.
    let fine = guard(|| std::panic::catch_unwind(|| panic!("caught on purpose")).is_err());
    assert_eq!(fine, Ok(true));
    assert!(reports(&folder).is_empty());

    // Another thread's caught panic is not this one's.
    std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(|| panic!("elsewhere, caught"));
    })
    .join()
    .unwrap();

    // Escaped: a report with what and where, and only this panic in it.
    let crashed = guard(|| -> u32 { panic!("escaped on purpose") });
    let path = crashed.unwrap_err().expect("a report was written");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("program: tester"), "{text}");
    assert!(text.contains("thread: main"), "{text}");
    assert!(text.contains("error: escaped on purpose"), "{text}");
    assert!(text.contains("crash.rs:"), "{text}");
    assert!(!text.contains("elsewhere"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .contains(&std::process::id().to_string())
    );
    assert_eq!(reports(&folder), std::slice::from_ref(&path));
    assert!(told("wen", Some(&path)).contains("nothing was sent"));

    // A thread's body that panics leaves a report naming it; joining still
    // sees the panic.
    let joined = std::thread::Builder::new()
        .name("helper".to_owned())
        .spawn(|| thread(|| -> u32 { panic!("in a helper") }))
        .unwrap()
        .join();
    assert!(joined.is_err());
    let newest = std::fs::read_to_string(&reports(&folder)[0]).unwrap();
    assert!(
        newest.contains("thread: helper") && newest.contains("in a helper"),
        "{newest}"
    );

    // Only the newest reports are kept.
    for n in 0..MAX_REPORTS + 5 {
        std::fs::write(
            folder.join(format!("crash-000000000{n:03}-1-old.txt")),
            "old",
        )
        .unwrap();
    }
    let _ = guard(|| -> u32 { panic!("one more") });
    assert_eq!(reports(&folder).len(), MAX_REPORTS);
}
