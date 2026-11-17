//! The third-party license file shipped with exported games names every
//! library in Cargo.lock (run `python tools/third_party.py` after changing
//! dependencies).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[test]
fn every_library_in_the_lock_file_is_in_the_license_file() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let file = wish_control::licenses::THIRD_PARTY;
    let mut missing = Vec::new();
    let mut name = None;
    for line in lock.lines() {
        if let Some(value) = line.strip_prefix("name = ") {
            name = Some(value.trim_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("version = ")
            && let Some(package) = name.take()
        {
            let version = value.trim_matches('"');
            // The engine's own crates are under its own license.
            if package.starts_with("wish_") {
                continue;
            }
            if !file.contains(&format!("| {package} | {version} |")) {
                missing.push(format!("{package} {version}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "THIRD_PARTY_LICENSES.md lacks these (run python tools/third_party.py): {missing:?}"
    );
}
