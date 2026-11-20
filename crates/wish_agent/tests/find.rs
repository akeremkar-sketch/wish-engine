//! Finding Claude Code (M9): on the PATH, in the native installer's place,
//! and in the Claude desktop app's own roaming folder (an npm install made
//! from inside the app, which other programs do not see in %APPDATA%).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::OsStr;
use std::path::PathBuf;

use wish_agent::claude::find_in;

fn base(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wish-find-claude-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn program() -> &'static str {
    if cfg!(windows) {
        "claude.cmd"
    } else {
        "claude"
    }
}

#[test]
fn claude_code_is_found_where_it_is_installed() {
    let dir = base("places");
    // Nowhere: not found.
    assert_eq!(
        find_in(None, Some(dir.as_os_str()), Some(dir.as_os_str())),
        None
    );
    // In the Claude app's own roaming folder.
    let app = dir.join("Packages/Claude_abc123/LocalCache/Roaming/npm");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join(program()), "x").unwrap();
    assert_eq!(
        find_in(None, None, Some(dir.as_os_str())),
        Some(app.join(program()))
    );
    // The PATH comes first.
    let on_path = dir.join("bin");
    std::fs::create_dir_all(&on_path).unwrap();
    std::fs::write(on_path.join(program()), "x").unwrap();
    assert_eq!(
        find_in(Some(on_path.as_os_str()), None, Some(dir.as_os_str())),
        Some(on_path.join(program()))
    );
    // Other packages are not looked into.
    let other = base("other");
    let elsewhere = other.join("Packages/Other_1/LocalCache/Roaming/npm");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join(program()), "x").unwrap();
    assert_eq!(
        find_in(Some(OsStr::new("")), None, Some(other.as_os_str())),
        None
    );
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&other).unwrap();
}
