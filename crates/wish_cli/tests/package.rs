//! `wen package` end to end (M12 release pipeline,
//! `docs/design/M12-release-pipeline.md`): the package folder for this host,
//! and beside it the release archive and `SHA256SUMS`; packaged twice, the
//! archives are the same bytes.
//!
//! It packs the compiler (about 560 MB with everything) and compresses it
//! twice, so it is not part of the everyday run. Build in release (the
//! compressor is slow unoptimized):
//! `cargo test --release -p wish_cli --test package -- --ignored --nocapture`.
//! The release workflow runs the same checks on the shipped programs.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use wish_control::archive::{self, Format};
use wish_control::release;

#[test]
#[ignore = "packs about 560 MB and compresses it twice; run by hand in release"]
fn wen_package_writes_the_same_archive_twice_with_its_licences() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wen-package");
    let _ = std::fs::remove_dir_all(&tmp);
    let windows = cfg!(windows);
    let exe = if windows { ".exe" } else { "" };
    let stem = release::archive_stem(env!("CARGO_PKG_VERSION"));
    let name = format!("{stem}.{}", Format::for_host().extension());
    let mut archives = Vec::new();
    for run in ["first", "second"] {
        let out = tmp.join(run).join("package");
        let started = std::time::Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_wen"))
            .arg("package")
            .arg("--out")
            .arg(&out)
            .arg("--json")
            .env("SOURCE_DATE_EPOCH", "1795005296")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
        println!(
            "{run}: {:.0} s, diagnostics {}",
            started.elapsed().as_secs_f64(),
            answer["diagnostics"]
        );

        // The folder, for this host.
        for file in [
            format!("wen{exe}"),
            "LICENSE-MIT".to_owned(),
            "LICENSE-APACHE".to_owned(),
            "THIRD_PARTY_LICENSES.md".to_owned(),
            "wish-engine.json".to_owned(),
            "sdk/sdk.json".to_owned(),
            "toolchain/toolchain.json".to_owned(),
        ] {
            assert!(out.join(&file).is_file(), "{file} is missing");
        }
        assert!(out.join("toolchain/share/doc/rust").is_dir());
        // The package's SDK names no path of the machine that made it (it
        // finds the packed compiler beside wen).
        let sdk = std::fs::read_to_string(out.join("sdk/sdk.json")).unwrap();
        assert!(!sdk.contains("rustc_path"), "{sdk}");
        assert!(
            out.join(format!("toolchain/bin/rustc{exe}")).is_file(),
            "the packed compiler"
        );
        assert!(out.join("templates").is_dir());
        assert!(out.join("packages").is_dir());
        for windows_only in ["wish-engine.ico", "clean-test.wsb", "clean-test.ps1"] {
            assert_eq!(out.join(windows_only).is_file(), windows, "{windows_only}");
        }

        // Beside it: the archive and its checksum.
        let archive_path = tmp.join(run).join(&name);
        assert_eq!(
            PathBuf::from(answer["archive"]["file"].as_str().unwrap()),
            archive_path
        );
        let bytes = std::fs::read(&archive_path).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join(run).join(archive::SUMS)).unwrap(),
            format!("{}  {name}\n", archive::sha256_hex(&bytes))
        );
        let listed = archive::read(&archive_path, &|path| path.ends_with("/LICENSE-MIT")).unwrap();
        let paths: Vec<&str> = listed.iter().map(|entry| entry.path.as_str()).collect();
        assert!(
            paths
                .iter()
                .all(|path| path.starts_with(&format!("{stem}/")))
        );
        for file in [
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "THIRD_PARTY_LICENSES.md",
            "wish-engine.json",
            "toolchain/share/doc/rust/",
        ] {
            assert!(paths.contains(&format!("{stem}/{file}").as_str()), "{file}");
        }
        assert!(!paths.iter().any(|path| path.contains("clean-test")));
        let mit = listed
            .iter()
            .find(|entry| entry.path.ends_with("/LICENSE-MIT"))
            .unwrap();
        assert_eq!(
            mit.contents.as_deref(),
            Some(wish_control::licenses::MIT.as_bytes())
        );
        archives.push(bytes);
    }
    assert!(
        archives[0] == archives[1],
        "two packages of the same sources gave different archives"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
