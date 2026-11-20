# M12: the release pipeline

Package 2 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0, item 2; the Phase 0 line "a release pipeline for Windows and
Linux"). Backlog: the platforms area of [M12-area-maps.md](M12-area-maps.md),
gaps 1 and 2 and their quick wins.

Recommended model and effort: Opus 5.5, high (a settled package: ROADMAP,
M12).

Status: built on 18 November 2026; section 5 says what was verified and what
was not. Not committed by the agent that built it (the package's
instruction). **The workflow has never run**: nobody may push a tag yet, and
its dry run starts only when the branch is pushed.

## 1. Problem

Nothing of the engine can be downloaded. `wen package` makes a folder on the
developer's Windows machine (wen, the player, the launcher, the SDK, the
packed compiler, templates, the engine's packages) and stops there:

- there is no archive and no checksum, so a person cannot fetch it and check
  what they got;
- it is Windows-only in what it writes (an `.ico`, a Windows Sandbox trial,
  PE checks), though the code it packs builds and is tested on Linux in CI;
- the package carries **no licence text at all**: the engine's own MIT and
  Apache texts and `THIRD_PARTY_LICENSES.md` go only into exported games, the
  packed Rust compiler goes without its notices, and the engine's font
  (Roboto, Apache-2.0) is named in neither;
- there is no build profile for what ships, and no workflow that builds a
  release from a tag.

## 2. What the code said (map claims, checked)

| Claim (area map, platforms) | What the code says |
|---|---|
| `wen package` writes `.wsb`/`.ps1`/`.ico` and runs PE checks on every host | True (`wish_cli/src/tool_cmds.rs`, `package`). |
| The player and launcher names already have non-Windows branches | True: `wish_player` / `wish-engine` without `.exe`. |
| The launcher is std-only; its Windows parts are under `cfg` | True: `windows_subsystem` and `CREATE_NO_WINDOW` are `cfg(windows)`; the error report uses `xdg-open` elsewhere. It builds and works as the double-click program on Linux. |
| Linux needs `libasound2-dev` in CI | Only once the `output` feature of `wish_audio` is on for Linux. Today Linux CI installs no development package: winit, softbuffer and wgpu open their system libraries at run time (`wayland-dlopen`, `x11-dlopen`, `dl-libxcb`, Vulkan and EGL through their loaders), and arboard speaks X11 in Rust. `cpal` is the one crate that links a system library at build time (`alsa-sys` through `pkg-config`). |
| Emit `.tar.xz` so the executable bits survive | Any tar keeps them. No xz encoder is in `Cargo.lock`; gzip is (`flate2`, through `png`). `.tar.gz` was chosen (section 3.3). |
| The package contains the licences | **False** (section 1). |

Two things the map did not say, found on the way:

- **The SDK's compile-time macros need the Visual C++ runtime DLL.** In the
  package of 17 November 2026 and in the one built for this note, the three
  proc-macro DLLs in `sdk/host` (`serde_derive`, `wish_schema_macros`,
  `wish_sdk_macros`) import `VCRUNTIME140.dll` (`dumpbin -dependents`, and
  the engine's own import reader); `rustc.exe` and `rustc_driver.dll` do
  not. The packed compiler loads those DLLs to build
  game code, so on a Windows without the Visual C++ runtime game code would
  not build. `pack_sdk` builds them with `--target wasm32-unknown-unknown`,
  and with `--target` cargo applies no `RUSTFLAGS` to host artifacts, so
  `+crt-static` cannot reach them on stable cargo (the `[host]` table is
  unstable). Not fixed here (section 6); `wen package` now says so with a
  `package_needs_static_crt` warning instead of shipping it silently.
- `sdk/sdk.json` records the absolute path of the compiler that built the
  pack (`rustc_path`, for engine development). In a package made on a
  person's computer that is a path in their home folder; in CI it is the
  runner's path. A packaged engine never reads it (it prefers
  `toolchain/bin/rustc` beside `wen`). Left as it is; noted for the closing
  review. **Fixed in the closing review of 1.0:** `wen package` writes the
  package's `sdk.json` without it (`wish_script::build::sdk_for_package`,
  unit-tested), and the release smoke checks that, finds the packed
  `toolchain/bin/rustc`, and builds with the runner's rustup and cargo off
  `PATH` and `RUSTUP_HOME`/`CARGO_HOME` pointing at an empty folder.

## 3. Decisions

### 3.1 The `dist` profile

```toml
[profile.dist]
inherits = "release"
lto = "thin"
strip = "debuginfo"
```

- **Thin LTO across crates**, not fat: `wen` and `wish_player` each link
  wasmtime, Cranelift, wgpu, naga, egui and Rapier, and a hosted runner has
  4 cores and 16 GB. Thin LTO gives most of the cross-crate inlining at a
  fraction of the time and memory. Codegen units stay at the default; one
  unit is a later measurement, not a guess.
- **Unwinding stays on.** Hostile glTF files are read under `catch_unwind`
  (`wish_import::guarded`) and the crash report needs the panic
  (`wish_control::crash`). `panic = "abort"` would turn both into a dead
  process.
- **Symbol names stay; debug information goes.** On Linux a crash report's
  backtrace then still names functions (on Windows the names are in a `.pdb`
  that is not shipped either way). Shipping split debug information as a
  separate release file is left for later.
- **The static C runtime is not in the profile**: a profile cannot set
  `RUSTFLAGS` on stable cargo (`profile-rustflags` is unstable). The workflow
  sets `RUSTFLAGS=-C target-feature=+crt-static` on Windows and builds with
  `--target x86_64-pc-windows-msvc`, which keeps the flag off build scripts
  and proc macros (as the M5 package was built).
- `dev` and `release` are untouched; tests never use `dist`.

### 3.2 What `wen package` builds on each host

| | Windows | Linux |
|---|---|---|
| `wen`, `wish_player`, launcher | `.exe` | no extension |
| Launcher | `wish-engine.exe` (no console window) | `wish-engine`, the same program built for Linux: the double-click program; when the editor fails it writes `~/.wish/launcher-error.txt` and opens it with `xdg-open` |
| Icon | `wish-engine.ico` | none yet |
| Clean-machine trial | `clean-test.ps1`, `clean-test.wsb` (Windows Sandbox) | none (package 3 runs the package in a bare environment in CI) |
| C runtime checks | wen, player, launcher, and now the SDK's macro DLLs | none (glibc is dynamic on purpose) |
| Licence files | `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD_PARTY_LICENSES.md`, `toolchain/share/doc/rust/` | the same |
| Archive | `wish-engine-<v>-windows-x86_64.zip` | `wish-engine-<v>-linux-x86_64.tar.gz` |

The Linux counterpart of the Windows shortcut is a freedesktop `.desktop`
entry with a PNG icon. It needs absolute paths, so it belongs to an install
step, which does not exist yet: not done (section 6). The decisions above use
`cfg!(windows)` (a constant in an ordinary `if`), not `#[cfg]`, so the Linux
branch is compiled and linted on Windows too.

### 3.3 The archive

- **Format by host**: `.zip` on Windows (Explorer opens it), `.tar.gz`
  elsewhere (keeps the executable bits that `wen`, `toolchain/bin/rustc` and
  `rust-lld` need). Both formats are written on every host, so both are
  tested here.
- **Name**: `wish-engine-<version>-<os>-<arch>`, with `<os>` and `<arch>`
  from the Rust standard library (`windows`/`linux`, `x86_64`). The version
  is the engine's (`Cargo.toml`, three numbers: ADR-027 rule 3).
- **One top folder** of the same name inside, so extracting never spills
  files into the current folder.
- **Contents**: the package folder as it is, except the Windows Sandbox trial
  (`clean-test.ps1`, `clean-test.wsb`): it holds the build machine's absolute
  paths and is a developer's trial, not part of the engine.
- **Deterministic**: entries sorted by path (bytes), directories included
  (empty ones survive), every timestamp the same, no owner names, numeric ids
  0, gzip header without a name and with time 0. The time is
  `SOURCE_DATE_EPOCH` when it is set (the reproducible-builds convention; the
  workflow sets it to the commit's time), else 1980-01-01 00:00 UTC (the
  earliest a zip can hold). Two runs on the same inputs give the same bytes.
- **Modes**: tar entries are `0755` for folders and for files with an
  execute bit, else `0644` (read from the file on Unix; on Windows, `.exe`
  files). Zip entries carry the MS-DOS folder attribute only.
- **Limits, refused with a diagnostic rather than written wrongly**: links
  (the M9 rule: nothing is written through or as a link), names that are not
  UTF-8, zip entries or archives of 4 GiB or more (no Zip64), more than 65,535
  zip entries, tar entries of 8 GiB or more. Long or non-ASCII tar paths get a
  POSIX `pax` header.
- Written to `<name>.part` beside its place and renamed when complete, so a
  half-written archive never has the final name.

### 3.4 Checksums

`SHA256SUMS` beside the archive, in the coreutils format (`<hex>  <name>`,
LF): `sha256sum -c SHA256SUMS` checks it. If the file exists, its line for
this archive is replaced and the others are kept (sorted by name), so two
hosts packaging into one folder share one file; a `SHA256SUMS` that is not a
checksum list is refused, not overwritten. The release job joins the two
platforms' lines the same way.

### 3.5 Licence texts

- The engine's `LICENSE-MIT`, `LICENSE-APACHE` and `THIRD_PARTY_LICENSES.md`
  are written into the package from the copies compiled into `wen`
  (`wish_control::licenses`), so they are the texts of the program that made
  the package. Exported games use the same constants.
- `tools/third_party.py` now also lists the third-party works that are not
  crates: Roboto (the engine's font, built into every program and exported
  game) with its licence text, and a line pointing to the compiler's notices.
  `THIRD_PARTY_LICENSES.md` was regenerated.
- `pack_toolchain` copies the notices that the rustc component installs
  (`share/doc/rust/COPYRIGHT*.html`, `share/doc/rust/licenses/`) into
  `toolchain/share/doc/rust/`. A compiler without them (a distribution's
  rustc) still packs, and `wen package` warns
  (`package_without_compiler_notices`).

### 3.6 Where the code lives

Writing archives and checksum lists is a capability other parts will need
(cross-export writes a tar for Unix players, gap 3 of the map; the Hub will
read engine archives once downloading exists), so it is in the control plane:
`wish_control::archive` (deterministic zip and tar.gz, reading them back,
SHA-256, the checksum list) and `wish_control::release` (the engine archive's
name, its licence files, `SOURCE_DATE_EPOCH`, archive plus sums). `wen
package` calls them. It is not a tool: packaging the engine needs the
engine's sources and cargo, and no project; it stays engine development, like
`wen sdk`. No tool answer changes.

Libraries: `flate2` (deflate, gzip; MIT OR Apache-2.0) and `sha2`
(MIT OR Apache-2.0) were already in `Cargo.lock` (through `png` and
Cranelift). They are now direct dependencies of `wish_control`; the lock gains
no package. Zip, tar and the checksum list are written by our own code.

### 3.7 The workflow (`.github/workflows/release.yml`)

- **Triggers**: a tag of three numbers (`[0-9]+.[0-9]+.[0-9]+`; no `v`, no
  suffix: ADR-027); a push to `main` or a milestone branch that touches the
  workflow or the packaging code (the dry run); by hand
  (`workflow_dispatch`). Path filters do not apply to tags.
- **`version`** (Ubuntu): reads the workspace version from `Cargo.toml`,
  checks it is three numbers, and for a tag that the tag is that version.
- **`package`** (Windows and Ubuntu): builds `wen`, the player and the
  launcher with `--profile dist` (static C runtime on Windows); runs `wen
  package` with `SOURCE_DATE_EPOCH` at the commit's time; checks
  `SHA256SUMS`; packages again into another folder and compares the archives
  byte for byte; extracts the archive with a different tool (PowerShell's
  `Expand-Archive`, GNU tar) and, from the extracted copy only, runs `wen
  version`, `new`, `build` (the packed compiler), `test`, `export` and the
  exported game headless. Uploads the archive and `SHA256SUMS` as artifacts
  of the run (kept 7 days).
- **`release`** (tags only, Ubuntu): downloads both artifacts, joins the two
  `SHA256SUMS`, checks them, and creates a **draft** GitHub Release with the
  two archives and the joined `SHA256SUMS`. A person writes the notes and
  publishes it.
- **Least privilege**: the workflow's default token permissions are none;
  `version` and `package` get `contents: read`, `release` gets
  `contents: write`; checkouts do not keep the token; no secret but the
  default token; actions pinned to a major version (`checkout@v5`,
  `upload-artifact@v4`, `download-artifact@v4`); refs reach scripts through
  environment variables, never pasted into the script. No build cache: a
  release is built from the sources alone.

### 3.8 Linux: sound, system packages

Sound out of the speakers was Windows-only (`wen play`, exported games). The
change is small: the `output` feature of `wish_audio` is now on for Windows
and Linux (`wish_cli` and `wish_player`), and the four `#[cfg(windows)]`
around the speaker in `wish_player` became `cfg(any(windows, target_os =
"linux"))`. macOS stays silent until it is built anywhere. Consequence:
`cpal` links ALSA, so every Linux `wen` and `wish_player`, even headless,
needs `libasound.so.2` to start. Desktop distributions and the Steam Runtime
have it; a bare container does not. CI's Linux job installs `libasound2-dev`.

Linux system packages:

| Package (Ubuntu 24.04 names) | Build | Run (end user) | For |
|---|---|---|---|
| `libasound2-dev`, `pkg-config` | yes | no | `alsa-sys` finds and links ALSA |
| `libasound2t64` (`libasound2` before 24.04) | (pulled by the -dev package) | **yes**, linked | sound; the programs do not start without it |
| `libvulkan1` and a Vulkan driver (`mesa-vulkan-drivers` gives lavapipe), or `libegl1` + GL | no | yes, opened at run time | drawing (wgpu) |
| `libxkbcommon0` (and `libxkbcommon-x11-0` on X11) | no | yes, opened at run time | keyboard in windows (winit) |
| `libwayland-client0` (Wayland) or `libxcb1`, `libx11-6`, `libxcursor1`, `libxrandr2`, `libxi6` (X11) | no | yes, opened at run time | windows (winit, softbuffer) |
| `xdg-utils` | no | optional | the launcher opens its error report |
| glibc 2.35 or newer (Ubuntu 22.04, Debian 12) | no | yes | everything (built on `ubuntu-22.04`, section 6) |

The run-time list is read from the crates' features (`cargo tree --target
x86_64-unknown-linux-gnu -e features`), not tried on a Linux desktop.

## 4. Tests

- `wish_control/tests/release.rs` (in the normal run): a small package
  folder (nested and empty folders, an empty file, a long path that needs a
  `pax` header, a non-ASCII name) is written as zip and as tar.gz; the
  entries read back are the folder's, in order, with the given time; the
  licence files are inside with their exact texts; `SHA256SUMS` holds the
  archive's SHA-256; two runs give the same bytes; an existing sums file keeps
  its other lines and a broken one is refused; SHA-256 and the zip date match
  known values; on Unix, links are refused and execute bits kept.
- `wish_cli/tests/package.rs` (`#[ignore]`: it packs about 560 MB and
  compresses it twice): the real `wen package` into two folders; the layout
  for the host, the licence files and compiler notices inside the archive,
  `SHA256SUMS` matching, the two archives byte-identical.
- `wish_script/tests/toolchain.rs` (`#[ignore]`, CI runs it): the packed
  toolchain carries the compiler's notices.
- By hand: the archives read by tools that share no code with ours (Python's
  `zipfile` and `tarfile`, Windows `tar.exe`), and `sha256sum -c`.
- The workflow's own steps (section 3.7) are the Linux test.

## 5. Result

Checked on the development machine (Windows 10, 18 November 2026). Linux
was not run anywhere: everything about it is in "Not verified" below.

**Verified**

- `cargo build --locked --profile dist --target x86_64-pc-windows-msvc -p
  wish_cli -p wish_player` with `RUSTFLAGS=-C target-feature=+crt-static`
  (the workflow's command), 4 jobs: 10 min 34 s from nothing. `dumpbin
  -dependents`: `wen.exe`, `wish_player.exe` and `wish-engine.exe` import no
  `VCRUNTIME140.dll`.
- The workflow's package job, step by step, as a script on this machine
  (`SOURCE_DATE_EPOCH` = the commit's time):
  - `wen package` from the dist build: 56 s the first time (the SDK built
    for the first time), 28 s the second. Package folder 578 MB, toolchain
    432 MiB, `wish-engine-0.1.0-windows-x86_64.zip` 188 MiB (197,813,302
    bytes), 230 entries. Its only warning is the SDK macros' C runtime
    (section 2).
  - `sha256sum -c SHA256SUMS`: OK.
  - Packaged again into another folder: the archive and `SHA256SUMS` are the
    same bytes (`cmp`).
  - Python's `zipfile` (`testzip`: every CRC right); PowerShell's
    `Expand-Archive` extracts it; the licence files, `wish-engine.json` and
    `toolchain/share/doc/rust` (both `COPYRIGHT*.html` and the 12 texts of
    `licenses/`) are in it; `clean-test.*` are not.
  - From the extracted copy alone: `wen version` says `wen 0.1.0`; `wen new`,
    `wen build` (the packed compiler, 5.15 s), `wen test` (1 test, 0
    failed), `wen export`, and the exported `smoke.exe --headless --frames
    60` (answer with a world hash).
- The small archives of `wish_control/tests/release.rs` read by tools that
  share no code with ours: Python's `zipfile` and `tarfile` (CRCs, names,
  times, modes, the ustar prefix and the `pax` path of the long names, the
  UTF-8 name), and Windows' `tar.exe` (libarchive), whose extraction of the
  zip and of the tar.gz are identical to each other and to the package
  folder (`diff -r`), the empty folder included.
- Tests: `cargo test -p wish_control --lib --test release --test
  third_party --test diagnostics_catalog` (24 + 5 + 1 + 2 passed; the links
  test runs on Unix only); `cargo test -p wish_cli --bins --test package`
  (7 + 3 passed, the package test ignored there); the ignored test by hand,
  `cargo test --release -p wish_cli --test package -- --ignored` (1 passed:
  two real packages of 34 s and 27 s, byte-identical archives; its warnings
  are the C runtime ones, as the release profile is built without
  `+crt-static`); `cargo test -p wish_script --test toolchain -- --ignored`
  (1 passed: the packed toolchain, 433 MiB, carries the notices and builds
  game code);
  `cargo clippy -p wish_control -p wish_cli -p wish_script -p wish_player
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.
- `Cargo.lock` gains two dependency lines in `wish_control` and no package;
  `THIRD_PARTY_LICENSES.md` regenerated offline: only the header, the
  Roboto row and Roboto added to the owners of the Apache text it shares.

**Not verified**

- **The workflow has never run.** Its YAML parses; nothing else about it is
  checked: not the triggers, the tag pattern, the version job, the
  artifacts, the draft release, nor the Windows runner (the local run above
  is a script that mirrors its steps).
- **Linux, all of it**: that `wen package` builds the Linux layout, that the
  tar.gz extracts with its execute bits and runs, that the Linux programs
  build with sound (`libasound2-dev`), that the CI job still passes with
  the `output` feature on, and the run-time package list of section 3.8.
  This changes when the dry run has passed on `ubuntu-latest`.
- That the zip opens in Windows Explorer (PowerShell's `Expand-Archive`
  did; Explorer itself was not tried) and that a release archive works on a
  clean Windows (the SDK macros say it would not build game code without the
  Visual C++ runtime).
- Sound on Linux by ear (and on Windows: still the user's step from M6).

## 6. Not done, and open

- **The SDK's macro DLLs and the C runtime** (section 2). Options, none
  taken: a `RUSTC_WRAPPER` that adds `+crt-static` to proc-macro crates
  only; cargo's unstable `[host]` table (`-Zhost-config`, nightly or
  `RUSTC_BOOTSTRAP`); or naming the Visual C++ Redistributable as a
  requirement of the Windows package. Until one is chosen, `wen package`
  warns and the release notes should say it. The first is the smallest
  stable fix; it is a package of its own because it touches `pack_sdk`
  and needs the clean-machine trial to prove it.
- **The Linux desktop entry and icon**: a `.desktop` file and a PNG need
  absolute paths, so they wait for an install step (none exists). Until then
  the Linux engine starts with `wen edit` or by double-clicking
  `wish-engine`. The launcher writes its error report to `~/.wish`, while
  crash reports go to `$XDG_CONFIG_HOME/wish` (area map, quick wins): left as
  it is.
- **A Linux clean-environment check** (the counterpart of
  `clean-test.wsb`): package 3 runs the package in a bare container in CI.
- **The glibc baseline** (decided in the closing review of 1.0): the Linux
  archive is built on the pinned `ubuntu-22.04` runner, and a release step
  fails if any ELF file of the package asks for a `GLIBC_` symbol version
  above 2.35. The README and both guides state glibc 2.35 or newer (Ubuntu
  22.04, Debian 12). Not done: a smoke run in a bare container of the
  oldest stated system (the runner image has far more libraries than a
  bare system).
- **Split debug information** as a release file (`.pdb` and `.debug`) to
  read crash reports' backtraces: later.
- **Measured LTO choices** (fat, one codegen unit): later; the dist `wen.exe`
  is 42.3 MB with thin LTO and the static C runtime.
- **The archive's name** carries the engine's name; the name is the user's
  decision (M12 §7, decision 2; answered 20 November 2026: "Wish Engine"
  stays, so the archives keep `wish-engine-...`). Renaming is one constant
  (`wish_control::release::stem`) and the workflow's `STEM`.
- **Zip times are UTC** written as MS-DOS local times (zip has no time
  zone): an extracted file shows the commit's UTC time as local time.
