//! Hand–agent parity (invariant 9, M8a WP9): every tool and every
//! operation an agent has is in `wish_editor::parity::PARITY`, with the
//! editor's way of doing it and the test that does it by hand.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use wish_editor::parity::PARITY;

#[test]
fn every_tool_and_operation_has_a_way_by_hand_and_a_test() {
    let mut wanted: BTreeSet<String> = wish_control::catalog()
        .iter()
        .map(|tool| tool.name.to_owned())
        .collect();
    // These two are listed by their parts.
    wanted.remove("edit_apply");
    wanted.remove("edit_history");
    for op in wish_control::ops::OPS {
        wanted.insert(format!("edit_apply:{op}"));
    }
    wanted.insert("edit_apply:session".to_owned());
    for action in ["list", "undo", "redo", "revert"] {
        wanted.insert(format!("edit_history:{action}"));
    }
    let listed: BTreeSet<String> = PARITY.iter().map(|row| row.name.to_owned()).collect();
    assert_eq!(listed.len(), PARITY.len(), "a capability is listed twice");
    let missing: Vec<_> = wanted.difference(&listed).collect();
    let extra: Vec<_> = listed.difference(&wanted).collect();
    assert!(
        missing.is_empty(),
        "an agent can do these, but the parity table does not say how a person does them in the editor \
         (add a way by hand and a row in wish_editor/src/parity.rs): {missing:?}"
    );
    assert!(extra.is_empty(), "rows for nothing: {extra:?}");
    // Every row points at a test that exists.
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut absent = Vec::new();
    for row in PARITY {
        assert!(!row.by_hand.trim().is_empty(), "{}: say how", row.name);
        let (file, function) = row
            .test
            .split_once("::")
            .unwrap_or_else(|| panic!("{}: test is file::function", row.name));
        let source = std::fs::read_to_string(tests.join(format!("{file}.rs"))).unwrap_or_default();
        if !source.contains(&format!("#[test]\nfn {function}()"))
            && !source.contains(&format!("#[test]\r\nfn {function}()"))
        {
            absent.push(format!("{} -> {}", row.name, row.test));
        }
    }
    assert!(absent.is_empty(), "tests that do not exist: {absent:#?}");
    // Operations (not questions) are what invariant 9 is about.
    assert!(PARITY.iter().filter(|row| row.changes).count() >= 25);
}
