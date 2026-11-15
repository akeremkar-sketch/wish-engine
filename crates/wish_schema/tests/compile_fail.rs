//! The macros' error messages are part of the interface: each mistake must
//! give one clear error at the right place. Expected output lives next to
//! each case in `tests/ui/*.stderr`; regenerate it with
//! `TRYBUILD=overwrite cargo test -p wish_schema --test compile_fail` after
//! a deliberate change, and review the diff.

#![allow(missing_docs)]

#[test]
fn macro_mistakes_give_one_clear_error_each() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
