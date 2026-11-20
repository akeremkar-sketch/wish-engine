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

/// Closing review of 1.0: MIT-only crates that ship no license text (taffy,
/// drm-fourcc, the objc2 family) were listed without one, though their
/// license asks for its text in every copy. A library without an
/// Apache-2.0 option (whose text the engine ships as its own) must carry a
/// text, unless its license is BSL-1.0 (which does not ask for one in a
/// program).
#[test]
fn every_library_whose_license_asks_for_its_text_has_one() {
    let file = wish_control::licenses::THIRD_PARTY;
    let mut without = Vec::new();
    for line in file.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // "| name | version | license | texts |" splits into six cells.
        let [_, name, version, license, texts, _] = cells.as_slice() else {
            continue;
        };
        if *name == "Library" || name.starts_with("---") {
            continue;
        }
        if license.contains("Apache-2.0") || *license == "BSL-1.0" {
            continue;
        }
        if *texts == "-" || texts.is_empty() {
            without.push(format!("{name} {version} ({license})"));
        }
    }
    assert!(
        without.is_empty(),
        "these libraries have no license text in THIRD_PARTY_LICENSES.md; keep their upstream text in licenses/ and add them to OVERRIDES in tools/third_party.py: {without:?}"
    );
}
