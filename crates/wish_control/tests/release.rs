//! The engine's release archives (M12 release pipeline,
//! `docs/design/M12-release-pipeline.md`): a small package folder written as
//! zip and as tar.gz and read back: every entry in order under one top
//! folder, the licence texts inside, the checksum list right, the same bytes
//! twice, damage noticed. `wen package` end to end is
//! `wish_cli/tests/package.rs` (ignored: it packs about 560 MB).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use wish_control::archive::{self, Format};
use wish_control::release;

/// 2026-11-18 12:34:56 UTC: an even second, which zip holds exactly.
const TIME: u64 = 1_795_005_296;

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("release-test")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const PROGRAM: &str = if cfg!(windows) { "wen.exe" } else { "wen" };
/// A name too long for ustar's 100-byte name field: needs a pax header.
fn long_name() -> String {
    format!("{}.rlib", "f".repeat(110))
}
/// Folders that make a path longer than 100 bytes with every part shorter:
/// ustar's prefix field.
fn deep() -> String {
    format!("toolchain/{}/{}", "d".repeat(50), "e".repeat(50))
}

/// A package: nested and empty folders, an empty file, long paths, a name
/// that is not ASCII, a program, the licence files, and the Windows Sandbox
/// trial (which stays out of the archive).
fn small_package(root: &Path) -> PathBuf {
    let package = root.join("package");
    std::fs::create_dir_all(package.join("sdk/host")).unwrap();
    std::fs::create_dir_all(package.join("empty")).unwrap();
    std::fs::create_dir_all(package.join(deep())).unwrap();
    std::fs::write(package.join(deep()).join("x.rlib"), vec![7u8; 70_000]).unwrap();
    std::fs::write(package.join("toolchain").join(long_name()), b"long").unwrap();
    std::fs::write(package.join(PROGRAM), b"a program").unwrap();
    std::fs::write(package.join("sdk/host/empty.txt"), b"").unwrap();
    std::fs::write(package.join("sdk/sdk.json"), "{}\n").unwrap();
    std::fs::write(package.join("templates-ğüş.txt"), "Türkçe").unwrap();
    std::fs::write(package.join("clean-test.wsb"), "<Configuration/>").unwrap();
    std::fs::write(package.join("clean-test.ps1"), "# trial").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            package.join(PROGRAM),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    release::write_licenses(&package).unwrap();
    package
}

/// What the archive of [`small_package`] must hold, in byte order.
fn expected(stem: &str) -> Vec<String> {
    let mut paths: Vec<String> = [
        "",
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "THIRD_PARTY_LICENSES.md",
        "empty/",
        "sdk/",
        "sdk/host/",
        "sdk/host/empty.txt",
        "sdk/sdk.json",
        "templates-ğüş.txt",
        "toolchain/",
        PROGRAM,
    ]
    .iter()
    .map(|path| format!("{stem}/{path}"))
    .collect();
    paths.push(format!("{stem}/toolchain/{}/", "d".repeat(50)));
    paths.push(format!("{stem}/{}/", deep()));
    paths.push(format!("{stem}/{}/x.rlib", deep()));
    paths.push(format!("{stem}/toolchain/{}", long_name()));
    paths.sort();
    paths
}

#[test]
fn a_package_is_archived_with_its_licences_and_its_checksum() {
    for format in [Format::Zip, Format::TarGz] {
        let root = fresh(format.extension());
        let package = small_package(&root);
        let stem = release::stem("0.1.0", "test", "x86_64");
        let made = release::write(&package, &root, &stem, format, TIME).unwrap();
        let name = format!("{stem}.{}", format.extension());
        assert_eq!(made.archive, root.join(&name));
        assert!(!root.join(format!("{name}.part")).exists());

        let listed = archive::read(&made.archive, &|_| true).unwrap();
        let paths: Vec<String> = listed.iter().map(|entry| entry.path.clone()).collect();
        assert_eq!(paths, expected(&stem), "{format:?}");
        for (file, text) in release::LICENSE_FILES {
            let entry = listed
                .iter()
                .find(|entry| entry.path == format!("{stem}/{file}"))
                .unwrap();
            assert_eq!(entry.contents.as_deref(), Some(text.as_bytes()), "{file}");
        }
        let rlib = listed
            .iter()
            .find(|entry| entry.path.ends_with("x.rlib"))
            .unwrap();
        assert_eq!(rlib.contents.as_deref(), Some(vec![7u8; 70_000].as_slice()));
        let turkish = listed
            .iter()
            .find(|entry| entry.path.ends_with(".txt") && entry.path.contains("templates"))
            .unwrap();
        assert_eq!(turkish.contents.as_deref(), Some("Türkçe".as_bytes()));
        for entry in &listed {
            assert_eq!(entry.time, TIME, "{}", entry.path);
            if format == Format::TarGz {
                let mode = if entry.path.ends_with('/') || entry.path.ends_with(PROGRAM) {
                    0o755
                } else {
                    0o644
                };
                assert_eq!(entry.mode, Some(mode), "{}", entry.path);
            }
        }

        // The checksum list names the archive with its SHA-256.
        let bytes = std::fs::read(&made.archive).unwrap();
        assert_eq!(made.sha256, archive::sha256_hex(&bytes));
        assert_eq!(made.bytes, bytes.len() as u64);
        assert_eq!(
            std::fs::read_to_string(&made.sums).unwrap(),
            format!("{}  {name}\n", made.sha256)
        );

        // The same inputs give the same bytes, wherever they are written.
        let again = root.join("again");
        std::fs::create_dir_all(&again).unwrap();
        let second = release::write(&package, &again, &stem, format, TIME).unwrap();
        assert_eq!(std::fs::read(&second.archive).unwrap(), bytes, "{format:?}");
        assert_eq!(
            std::fs::read(&second.sums).unwrap(),
            std::fs::read(&made.sums).unwrap()
        );
    }
}

#[test]
fn a_damaged_archive_is_reported_not_read() {
    for format in [Format::Zip, Format::TarGz] {
        let root = fresh(&format!("damaged-{}", format.extension()));
        let package = small_package(&root);
        let made = release::write(&package, &root, "damaged", format, TIME).unwrap();
        let bytes = std::fs::read(&made.archive).unwrap();
        std::fs::write(&made.archive, &bytes[..bytes.len() / 2]).unwrap();
        let problem = archive::read(&made.archive, &|_| true).unwrap_err();
        assert_eq!(problem.code, "archive_failed", "{format:?}");
        assert!(!problem.hint.is_empty());
    }
}

#[test]
fn a_package_without_its_licences_is_not_archived() {
    let root = fresh("unlicensed");
    let package = root.join("package");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join(PROGRAM), b"a program").unwrap();
    let problem = release::write(&package, &root, "unlicensed", Format::Zip, TIME).unwrap_err();
    assert_eq!(problem.code, "package_failed");
    assert!(
        problem.message.contains("LICENSE-MIT"),
        "{}",
        problem.message
    );
    assert!(!root.join("unlicensed.zip").exists());
    assert!(!root.join(archive::SUMS).exists());
}

#[test]
fn the_checksum_list_keeps_other_files_and_refuses_what_is_not_one() {
    let root = fresh("sums");
    let sums = root.join(archive::SUMS);
    let windows = "wish-engine-0.1.0-windows-x86_64.zip";
    let linux = "wish-engine-0.1.0-linux-x86_64.tar.gz";
    let other = "b".repeat(64);
    std::fs::write(&sums, format!("{other}  {windows}\n")).unwrap();
    archive::record_sum(&sums, linux, &"A".repeat(64)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&sums).unwrap(),
        format!("{}  {linux}\n{other}  {windows}\n", "a".repeat(64))
    );
    // A new archive of the same name replaces its line.
    archive::record_sum(&sums, linux, &"c".repeat(64)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&sums).unwrap(),
        format!("{}  {linux}\n{other}  {windows}\n", "c".repeat(64))
    );
    // Something that is not a checksum list is left alone.
    std::fs::write(&sums, "my notes\n").unwrap();
    let problem = archive::record_sum(&sums, linux, &"d".repeat(64)).unwrap_err();
    assert_eq!(problem.code, "archive_failed");
    assert!(!problem.hint.is_empty());
    assert_eq!(std::fs::read_to_string(&sums).unwrap(), "my notes\n");
}

#[test]
fn the_archive_is_named_for_the_version_and_the_host() {
    assert_eq!(
        release::archive_stem("1.2.3"),
        format!(
            "wish-engine-1.2.3-{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    );
    assert_eq!(
        Format::for_host(),
        if cfg!(windows) {
            Format::Zip
        } else {
            Format::TarGz
        }
    );
    assert_eq!(Format::of(Path::new("a/b.tar.gz")), Some(Format::TarGz));
    assert_eq!(Format::of(Path::new("B.ZIP")), Some(Format::Zip));
    assert_eq!(Format::of(Path::new("b.tar")), None);
}

#[cfg(unix)]
#[test]
fn links_are_refused() {
    let root = fresh("links");
    let package = small_package(&root);
    std::os::unix::fs::symlink(package.join(PROGRAM), package.join("link")).unwrap();
    let problem = archive::entries_of(&package, "top", &[]).unwrap_err();
    assert_eq!(problem.code, "archive_failed");
    assert!(problem.message.contains("link"), "{}", problem.message);
}
