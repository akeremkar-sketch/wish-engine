//! The engine's release archives (M12 release pipeline): the licence files
//! every engine package carries, and the archive and checksum list `wen
//! package` writes beside the package folder. The release workflow
//! (`.github/workflows/release.yml`) attaches both to a draft GitHub
//! Release. Design: `docs/design/M12-release-pipeline.md`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result};

use crate::archive::{self, Format};
use crate::licenses;

/// The licence files at the top of every engine package, with their texts:
/// the engine's own (MIT OR Apache-2.0) and the third-party notices.
pub const LICENSE_FILES: [(&str, &str); 3] = [
    ("LICENSE-MIT", licenses::MIT),
    ("LICENSE-APACHE", licenses::APACHE),
    ("THIRD_PARTY_LICENSES.md", licenses::THIRD_PARTY),
];

/// What a package folder holds that its archive leaves out: the Windows
/// Sandbox trial, which names the build machine's folders.
pub const NOT_ARCHIVED: &[&str] = &["clean-test.ps1", "clean-test.wsb"];

/// The release archive's name without its ending for `version` on this
/// host: `wish-engine-<version>-<os>-<arch>` (`windows`/`linux`,
/// `x86_64`).
pub fn archive_stem(version: &str) -> String {
    stem(version, std::env::consts::OS, std::env::consts::ARCH)
}

/// The release archive's name without its ending for `version`, `os` and
/// `arch`.
pub fn stem(version: &str, os: &str, arch: &str) -> String {
    format!("wish-engine-{version}-{os}-{arch}")
}

fn failed(message: impl Into<String>, hint: impl Into<String>) -> Diagnostic {
    Diagnostic::error("package_failed", message, hint)
}

/// Writes the licence files ([`LICENSE_FILES`]) into the package folder.
pub fn write_licenses(folder: &Path) -> Result<()> {
    for (name, text) in LICENSE_FILES {
        let path = folder.join(name);
        std::fs::write(&path, text).map_err(|error| {
            failed(
                format!("Could not write '{}': {error}.", path.display()),
                "Check that the package folder can be written to.",
            )
        })?;
    }
    Ok(())
}

/// The time stamped on every entry of a release archive, in seconds since
/// 1970: `SOURCE_DATE_EPOCH` when it is set (the reproducible-builds
/// convention; the release workflow sets it to the commit's time), else
/// 1980-01-01 00:00 UTC. Never the clock: the same inputs give the same
/// archive.
pub fn source_date_epoch() -> Result<u64> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(text) if !text.trim().is_empty() => text.trim().parse().map_err(|_| {
            failed(
                format!("SOURCE_DATE_EPOCH is '{text}', not a whole number of seconds."),
                "Set it to the commit's time (git log -1 --format=%ct), or unset it.",
            )
        }),
        _ => Ok(archive::ZIP_EPOCH),
    }
}

/// A release archive [`write`] made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The archive.
    pub archive: PathBuf,
    /// Its size in bytes.
    pub bytes: u64,
    /// Its SHA-256, 64 lowercase hex digits.
    pub sha256: String,
    /// The checksum list it was recorded in (`SHA256SUMS` beside it).
    pub sums: PathBuf,
}

impl Release {
    /// What `wen package --json` says about it.
    pub fn to_json(&self) -> Value {
        json!({
            "file": self.archive.display().to_string(),
            "bytes": self.bytes,
            "sha256": self.sha256,
            "sums": self.sums.display().to_string(),
        })
    }
}

/// Writes the release archive of the finished package folder `package` into
/// the folder `into` as `<stem>.<zip|tar.gz>`, everything under one top
/// folder named `stem`, every entry stamped with `time`, and records its
/// SHA-256 in `into/SHA256SUMS` (other lines there are kept). The package
/// must hold the licence files; [`NOT_ARCHIVED`] stays out.
pub fn write(
    package: &Path,
    into: &Path,
    stem: &str,
    format: Format,
    time: u64,
) -> Result<Release> {
    for (name, _) in LICENSE_FILES {
        if !package.join(name).is_file() {
            return Err(failed(
                format!(
                    "The package '{}' has no {name}, so its archive would ship without it.",
                    package.display()
                ),
                "Package again with `wen package`: it writes the licence files before the archive.",
            ));
        }
    }
    let entries = archive::entries_of(package, stem, NOT_ARCHIVED)?;
    let name = format!("{stem}.{}", format.extension());
    let path = into.join(&name);
    let bytes = archive::write(&entries, format, time, &path)?;
    let sha256 = archive::sha256_of(&path)?;
    let sums = into.join(archive::SUMS);
    archive::record_sum(&sums, &name, &sha256)?;
    Ok(Release {
        archive: path,
        bytes,
        sha256,
        sums,
    })
}
