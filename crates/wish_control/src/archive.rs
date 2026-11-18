//! Deterministic archives (M12 release pipeline): a folder written as a
//! `.zip` or a `.tar.gz` that has the same bytes every time it is written
//! from the same files (entries sorted by path, one timestamp for all, no
//! owner names), read back, and checked with SHA-256 in a `SHA256SUMS` list.
//!
//! `wen package` writes the engine's release archives with it
//! ([`crate::release`]). Zip is written with deflate and without Zip64; tar is
//! POSIX ustar, with a `pax` header for a path that does not fit. Design:
//! `docs/design/M12-release-pipeline.md`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use sha2::{Digest, Sha256};
use wish_core::{Diagnostic, Result};

/// The checksum list's file name (the coreutils convention:
/// `sha256sum -c SHA256SUMS` checks it).
pub const SUMS: &str = "SHA256SUMS";

/// The earliest time a zip entry can hold (1980-01-01 00:00:00 UTC), in
/// seconds since 1970. Archives use it when no time is given.
pub const ZIP_EPOCH: u64 = 315_532_800;

/// The last time a zip entry can hold (2107-12-31 23:59:59 UTC).
const ZIP_LAST: u64 = 4_354_819_199;

/// An archive format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Zip with deflate: Windows opens it without help.
    Zip,
    /// A POSIX tar compressed with gzip: keeps the execute bits programs
    /// need on Linux and macOS.
    TarGz,
}

impl Format {
    /// The format of this host's release archives: zip on Windows, tar.gz
    /// elsewhere.
    pub fn for_host() -> Self {
        if cfg!(windows) {
            Self::Zip
        } else {
            Self::TarGz
        }
    }

    /// The file name's ending, without the first dot (`zip`, `tar.gz`).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
        }
    }

    /// The format a file name ends in, if it is one of these.
    pub fn of(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        if name.ends_with(".zip") {
            Some(Self::Zip)
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Self::TarGz)
        } else {
            None
        }
    }
}

/// One entry to write: a folder or a file, by its path inside the archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The path inside the archive, `/`-separated; a folder's ends with `/`.
    pub path: String,
    /// The file its bytes are read from; `None` for a folder.
    pub source: Option<PathBuf>,
    /// A program: tar mode `0755` instead of `0644`.
    pub executable: bool,
}

/// An entry read back from an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The path inside the archive; a folder's ends with `/`.
    pub path: String,
    /// The file's size in bytes (0 for a folder).
    pub size: u64,
    /// The Unix mode (tar only).
    pub mode: Option<u32>,
    /// When the entry says it was changed, in seconds since 1970 (zip: to
    /// two seconds, as its MS-DOS date holds it).
    pub time: u64,
    /// The file's bytes, when they were asked for.
    pub contents: Option<Vec<u8>>,
}

fn failed(message: impl Into<String>, hint: impl Into<String>) -> Diagnostic {
    Diagnostic::error("archive_failed", message, hint)
}

fn io(what: &str, path: &Path, error: &std::io::Error) -> Diagnostic {
    failed(
        format!("Could not {what} '{}': {error}.", path.display()),
        "Check that the folder can be read, that the archive's folder can be written to, and that the disk is not full.",
    )
}

fn not_an_archive(path: &Path, why: &str) -> Diagnostic {
    failed(
        format!("'{}' is not a readable archive: {why}.", path.display()),
        "The file may be cut short or damaged: download or write it again.",
    )
}

/// The entries of `folder` as they go into an archive, sorted by path:
/// every folder and file under it, under the top folder `top` (none when
/// `top` is empty). Names in `leave_out` are skipped in `folder` itself
/// (not deeper). Links are refused (an archive holds the files themselves,
/// and nothing is ever written through a link), and so are names that are
/// not UTF-8.
pub fn entries_of(folder: &Path, top: &str, leave_out: &[&str]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let prefix = if top.is_empty() {
        String::new()
    } else {
        let prefix = format!("{top}/");
        entries.push(Entry {
            path: prefix.clone(),
            source: None,
            executable: false,
        });
        prefix
    };
    walk(folder, &prefix, leave_out, &mut entries)?;
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(entries)
}

fn walk(dir: &Path, prefix: &str, leave_out: &[&str], into: &mut Vec<Entry>) -> Result<()> {
    let items = std::fs::read_dir(dir).map_err(|e| io("read the folder", dir, &e))?;
    for item in items {
        let item = item.map_err(|e| io("read the folder", dir, &e))?;
        let path = item.path();
        let Some(name) = item.file_name().to_str().map(str::to_owned) else {
            return Err(failed(
                format!("'{}' has a name that is not UTF-8.", path.display()),
                "Rename it: names inside an archive are UTF-8.",
            ));
        };
        if leave_out.contains(&name.as_str()) {
            continue;
        }
        let kind = item
            .file_type()
            .map_err(|e| io("read the folder", dir, &e))?;
        if kind.is_symlink() {
            return Err(failed(
                format!("'{}' is a link.", path.display()),
                "An archive holds the files themselves: replace the link with a copy of what it points to.",
            ));
        }
        if kind.is_dir() {
            let inner = format!("{prefix}{name}/");
            into.push(Entry {
                path: inner.clone(),
                source: None,
                executable: false,
            });
            walk(&path, &inner, &[], into)?;
        } else if kind.is_file() {
            let meta = item.metadata().map_err(|e| io("read", &path, &e))?;
            into.push(Entry {
                path: format!("{prefix}{name}"),
                executable: is_program(&meta, &name),
                source: Some(path),
            });
        } else {
            return Err(failed(
                format!("'{}' is neither a file nor a folder.", path.display()),
                "Remove it from the folder: an archive holds files and folders only.",
            ));
        }
    }
    Ok(())
}

/// A file with an execute bit (on Unix); on Windows, an `.exe`.
#[cfg(unix)]
fn is_program(meta: &std::fs::Metadata, _name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

/// A file with an execute bit (on Unix); on Windows, an `.exe`.
#[cfg(not(unix))]
fn is_program(_meta: &std::fs::Metadata, name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".exe")
}

/// Writes `entries` (in the order given) as an archive of `format` at
/// `out`, every entry stamped with `time` (seconds since 1970; zip holds
/// 1980 to 2107 and clamps to that). The archive is written beside its
/// place (`<name>.part`) and renamed when complete. Returns its size in
/// bytes.
pub fn write(entries: &[Entry], format: Format, time: u64, out: &Path) -> Result<u64> {
    let part = out.with_file_name(format!(
        "{}.part",
        out.file_name()
            .map_or_else(|| "archive".into(), |n| n.to_string_lossy())
    ));
    let written = (|| -> Result<()> {
        let file = File::create(&part).map_err(|e| io("create", &part, &e))?;
        let mut sink = BufWriter::new(file);
        match format {
            Format::Zip => write_zip(entries, time, &mut sink, out)?,
            Format::TarGz => {
                let gz = flate2::GzBuilder::new()
                    .mtime(0)
                    .operating_system(255)
                    .write(&mut sink, Compression::default());
                write_tar(entries, time, gz, out)?
                    .finish()
                    .map_err(|e| io("write", out, &e))?;
            }
        }
        sink.flush().map_err(|e| io("write", out, &e))
    })();
    if let Err(problem) = written {
        let _ = std::fs::remove_file(&part);
        return Err(problem);
    }
    std::fs::rename(&part, out).map_err(|e| io("put in place", out, &e))?;
    std::fs::metadata(out)
        .map(|meta| meta.len())
        .map_err(|e| io("read", out, &e))
}

fn put(out: &mut impl Write, bytes: &[u8], at: &Path) -> Result<()> {
    out.write_all(bytes).map_err(|e| io("write", at, &e))
}

fn too_large(what: &str, at: &Path) -> Diagnostic {
    failed(
        format!(
            "'{}' cannot be written: {what} is too large for the format.",
            at.display()
        ),
        "Zip entries and archives stay under 4 GiB with at most 65,535 entries, tar entries under 8 GiB: leave large files out of the folder, or write a .tar.gz.",
    )
}

/// MS-DOS time and date (UTC) of `unix` seconds, as zip stores them.
pub fn dos_time_date(unix: u64) -> (u16, u16) {
    let unix = unix.clamp(ZIP_EPOCH, ZIP_LAST);
    let days = unix / 86_400;
    let seconds = unix % 86_400;
    let (year, month, day) = civil_from_days(days);
    let time = ((seconds / 3600) << 11) | (((seconds % 3600) / 60) << 5) | ((seconds % 60) / 2);
    let date = ((year - 1980) << 9) | (month << 5) | day;
    (
        u16::try_from(time).unwrap_or(0),
        u16::try_from(date).unwrap_or(0),
    )
}

/// Year, month and day of a day count since 1970-01-01 (days ≥ 0).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Days since 1970-01-01 of a date from 1970 on.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let yoe = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * shifted + 2) / 5 + day.max(1) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe).saturating_sub(719_468)
}

fn unix_from_dos(time: u16, date: u16) -> u64 {
    let year = 1980 + u64::from(date >> 9);
    let month = u64::from((date >> 5) & 0xF).clamp(1, 12);
    let day = u64::from(date & 0x1F);
    days_from_civil(year, month, day) * 86_400
        + u64::from(time >> 11) * 3600
        + u64::from((time >> 5) & 0x3F) * 60
        + u64::from(time & 0x1F) * 2
}

const ZIP_LOCAL: u32 = 0x0403_4b50;
const ZIP_CENTRAL: u32 = 0x0201_4b50;
const ZIP_END: u32 = 0x0605_4b50;
/// General purpose flag 11: names are UTF-8.
const ZIP_UTF8: u16 = 1 << 11;
const ZIP_STORED: u16 = 0;
const ZIP_DEFLATED: u16 = 8;
/// Version 2.0 (deflate, folders), made on MS-DOS (attributes are MS-DOS's).
const ZIP_VERSION: u16 = 20;
/// The MS-DOS folder attribute.
const ZIP_FOLDER: u32 = 0x10;

struct ZipWritten {
    name: Vec<u8>,
    method: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
    folder: bool,
}

fn position(out: &mut impl Seek, at: &Path) -> Result<u64> {
    out.stream_position().map_err(|e| io("write", at, &e))
}

fn write_zip<W: Write + Seek>(entries: &[Entry], time: u64, out: &mut W, at: &Path) -> Result<()> {
    let count = u16::try_from(entries.len()).map_err(|_| too_large("the number of entries", at))?;
    let (dos_time, dos_date) = dos_time_date(time);
    let mut written = Vec::with_capacity(entries.len());
    for entry in entries {
        let start = position(out, at)?;
        let offset = u32::try_from(start).map_err(|_| too_large("the archive", at))?;
        let name = entry.path.as_bytes().to_vec();
        let name_len = u16::try_from(name.len()).map_err(|_| too_large("an entry's name", at))?;
        let method = if entry.source.is_some() {
            ZIP_DEFLATED
        } else {
            ZIP_STORED
        };
        let mut header = Vec::with_capacity(30 + name.len());
        header.extend_from_slice(&ZIP_LOCAL.to_le_bytes());
        header.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        header.extend_from_slice(&ZIP_UTF8.to_le_bytes());
        header.extend_from_slice(&method.to_le_bytes());
        header.extend_from_slice(&dos_time.to_le_bytes());
        header.extend_from_slice(&dos_date.to_le_bytes());
        // CRC-32 and both sizes: filled in once the data is written.
        header.extend_from_slice(&[0; 12]);
        header.extend_from_slice(&name_len.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&name);
        put(out, &header, at)?;
        let (crc, compressed, size) = match &entry.source {
            None => (0, 0, 0),
            Some(source) => {
                let data = position(out, at)?;
                let (crc, size) = deflate_file(source, out, at)?;
                let end = position(out, at)?;
                let compressed =
                    u32::try_from(end - data).map_err(|_| too_large(&entry.path, at))?;
                let size = u32::try_from(size).map_err(|_| too_large(&entry.path, at))?;
                let mut sizes = Vec::with_capacity(12);
                sizes.extend_from_slice(&crc.to_le_bytes());
                sizes.extend_from_slice(&compressed.to_le_bytes());
                sizes.extend_from_slice(&size.to_le_bytes());
                out.seek(SeekFrom::Start(start + 14))
                    .map_err(|e| io("write", at, &e))?;
                put(out, &sizes, at)?;
                out.seek(SeekFrom::Start(end))
                    .map_err(|e| io("write", at, &e))?;
                (crc, compressed, size)
            }
        };
        written.push(ZipWritten {
            name,
            method,
            crc,
            compressed,
            size,
            offset,
            folder: entry.source.is_none(),
        });
    }
    let central = position(out, at)?;
    for entry in &written {
        let mut header = Vec::with_capacity(46 + entry.name.len());
        header.extend_from_slice(&ZIP_CENTRAL.to_le_bytes());
        header.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        header.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        header.extend_from_slice(&ZIP_UTF8.to_le_bytes());
        header.extend_from_slice(&entry.method.to_le_bytes());
        header.extend_from_slice(&dos_time.to_le_bytes());
        header.extend_from_slice(&dos_date.to_le_bytes());
        header.extend_from_slice(&entry.crc.to_le_bytes());
        header.extend_from_slice(&entry.compressed.to_le_bytes());
        header.extend_from_slice(&entry.size.to_le_bytes());
        header.extend_from_slice(&u16::try_from(entry.name.len()).unwrap_or(0).to_le_bytes());
        // Extra field, comment, first disk, internal attributes.
        header.extend_from_slice(&[0; 8]);
        let attributes = if entry.folder { ZIP_FOLDER } else { 0 };
        header.extend_from_slice(&attributes.to_le_bytes());
        header.extend_from_slice(&entry.offset.to_le_bytes());
        header.extend_from_slice(&entry.name);
        put(out, &header, at)?;
    }
    let end = position(out, at)?;
    let central_at = u32::try_from(central).map_err(|_| too_large("the archive", at))?;
    let central_size = u32::try_from(end - central).map_err(|_| too_large("the archive", at))?;
    let mut record = Vec::with_capacity(22);
    record.extend_from_slice(&ZIP_END.to_le_bytes());
    // This disk, the disk the directory starts on.
    record.extend_from_slice(&[0; 4]);
    record.extend_from_slice(&count.to_le_bytes());
    record.extend_from_slice(&count.to_le_bytes());
    record.extend_from_slice(&central_size.to_le_bytes());
    record.extend_from_slice(&central_at.to_le_bytes());
    record.extend_from_slice(&0u16.to_le_bytes());
    put(out, &record, at)
}

/// Deflates the file at `source` into `out`; its CRC-32 and size.
fn deflate_file(source: &Path, out: &mut impl Write, at: &Path) -> Result<(u32, u64)> {
    let mut file = File::open(source).map_err(|e| io("read", source, &e))?;
    let mut crc = flate2::Crc::new();
    let mut encoder = flate2::write::DeflateEncoder::new(out, Compression::default());
    let mut buffer = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(|e| io("read", source, &e))?;
        if read == 0 {
            break;
        }
        let chunk = buffer.get(..read).unwrap_or_default();
        crc.update(chunk);
        encoder.write_all(chunk).map_err(|e| io("write", at, &e))?;
        size += read as u64;
    }
    encoder.finish().map_err(|e| io("write", at, &e))?;
    Ok((crc.sum(), size))
}

/// Counts what goes through it (the tar stream before compression).
struct Counted<W> {
    inner: W,
    bytes: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// A tar block.
const BLOCK: u64 = 512;
/// Tar archives are written in records of 20 blocks.
const RECORD: u64 = 20 * BLOCK;

fn write_tar<W: Write>(entries: &[Entry], time: u64, out: W, at: &Path) -> Result<W> {
    let mut tar = Counted {
        inner: out,
        bytes: 0,
    };
    for entry in entries {
        match &entry.source {
            None => put(
                &mut tar,
                &tar_header(&entry.path, 0, b'5', 0o755, time, at)?,
                at,
            )?,
            Some(source) => {
                let size = std::fs::metadata(source)
                    .map_err(|e| io("read", source, &e))?
                    .len();
                let mode = if entry.executable { 0o755 } else { 0o644 };
                put(
                    &mut tar,
                    &tar_header(&entry.path, size, b'0', mode, time, at)?,
                    at,
                )?;
                let file = File::open(source).map_err(|e| io("read", source, &e))?;
                let copied = std::io::copy(&mut file.take(size), &mut tar)
                    .map_err(|e| io("write", at, &e))?;
                if copied != size {
                    return Err(failed(
                        format!("'{}' changed while it was archived.", source.display()),
                        "Archive the folder again when nothing is writing to it.",
                    ));
                }
                put(&mut tar, &padding(size), at)?;
            }
        }
    }
    // Two empty blocks end the archive; it is filled up to a whole record.
    put(&mut tar, &[0; 2 * BLOCK as usize], at)?;
    let rest = (RECORD - tar.bytes % RECORD) % RECORD;
    put(&mut tar, &vec![0; usize::try_from(rest).unwrap_or(0)], at)?;
    Ok(tar.inner)
}

/// The zeros that fill `size` bytes up to whole blocks.
fn padding(size: u64) -> Vec<u8> {
    vec![0; usize::try_from((BLOCK - size % BLOCK) % BLOCK).unwrap_or(0)]
}

/// `value` in octal, zero-padded to fill `field` but its last byte (NUL).
fn octal(field: &mut [u8], value: u64) -> Option<()> {
    let digits = field.len().checked_sub(1)?;
    let text = format!("{value:0digits$o}");
    if text.len() > digits {
        return None;
    }
    field.get_mut(..digits)?.copy_from_slice(text.as_bytes());
    *field.get_mut(digits)? = 0;
    Some(())
}

/// Where a ustar header can split `path` into a prefix (≤ 155 bytes) and a
/// name (1 to 100 bytes) at a `/`.
fn ustar_split(path: &str) -> Option<(&str, &str)> {
    if path.len() <= 100 {
        return Some(("", path));
    }
    path.match_indices('/')
        .map(|(at, _)| (&path[..at], &path[at + 1..]))
        .find(|(_, name)| !name.is_empty() && name.len() <= 100)
        .filter(|(prefix, _)| prefix.len() <= 155)
}

/// One `pax` record: `<length> <key>=<value>\n`, the length counting itself.
fn pax_record(key: &str, value: &str) -> String {
    let body = format!(" {key}={value}\n");
    let mut length = body.len() + 1;
    loop {
        let counted = length.to_string().len() + body.len();
        if counted == length {
            return format!("{length}{body}");
        }
        length = counted;
    }
}

fn ustar_block(
    prefix: &[u8],
    name: &[u8],
    size: u64,
    kind: u8,
    mode: u32,
    time: u64,
) -> Option<[u8; BLOCK as usize]> {
    if name.len() > 100 || prefix.len() > 155 {
        return None;
    }
    let mut block = [0u8; BLOCK as usize];
    block.get_mut(..name.len())?.copy_from_slice(name);
    octal(&mut block[100..108], u64::from(mode))?;
    octal(&mut block[108..116], 0)?;
    octal(&mut block[116..124], 0)?;
    octal(&mut block[124..136], size)?;
    octal(&mut block[136..148], time)?;
    block[148..156].fill(b' ');
    block[156] = kind;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    octal(&mut block[329..337], 0)?;
    octal(&mut block[337..345], 0)?;
    block
        .get_mut(345..345 + prefix.len())?
        .copy_from_slice(prefix);
    let sum: u32 = block.iter().map(|&byte| u32::from(byte)).sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    Some(block)
}

/// The header block(s) of one tar entry: a `pax` header first when the path
/// is not ASCII or does not fit ustar's name fields.
fn tar_header(path: &str, size: u64, kind: u8, mode: u32, time: u64, at: &Path) -> Result<Vec<u8>> {
    let oversized = || too_large(path, at);
    let split = ustar_split(path).filter(|_| path.is_ascii());
    let mut header = Vec::with_capacity(3 * BLOCK as usize);
    let (prefix, name) = match split {
        Some(split) => split,
        None => {
            let record = pax_record("path", path);
            let pax = ustar_block(
                b"",
                b"././@PaxHeader",
                record.len() as u64,
                b'x',
                0o644,
                time,
            )
            .ok_or_else(oversized)?;
            header.extend_from_slice(&pax);
            header.extend_from_slice(record.as_bytes());
            header.extend_from_slice(&padding(record.len() as u64));
            // The ustar name is only a fallback for readers without pax:
            // the path's end, cut at a character boundary.
            let mut cut = path.len().saturating_sub(100);
            while !path.is_char_boundary(cut) {
                cut += 1;
            }
            ("", &path[cut..])
        }
    };
    let block = ustar_block(prefix.as_bytes(), name.as_bytes(), size, kind, mode, time)
        .ok_or_else(oversized)?;
    header.extend_from_slice(&block);
    Ok(header)
}

/// The entries of the archive at `archive` (a `.zip` or `.tar.gz`), in
/// their order, with the bytes of the files `wanted` names.
pub fn read(archive: &Path, wanted: &dyn Fn(&str) -> bool) -> Result<Vec<Listed>> {
    match Format::of(archive) {
        Some(Format::Zip) => read_zip(archive, wanted),
        Some(Format::TarGz) => read_tar(archive, wanted),
        None => Err(not_an_archive(
            archive,
            "its name ends in neither .zip nor .tar.gz",
        )),
    }
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_zip(archive: &Path, wanted: &dyn Fn(&str) -> bool) -> Result<Vec<Listed>> {
    let broken = |why: &str| not_an_archive(archive, why);
    let mut file = File::open(archive).map_err(|e| io("read", archive, &e))?;
    let length = file.metadata().map_err(|e| io("read", archive, &e))?.len();
    // The end record is the last 22 bytes, unless a comment follows it.
    let tail_length = length.min(22 + 0xFFFF);
    let mut tail = vec![0; usize::try_from(tail_length).unwrap_or(0)];
    file.seek(SeekFrom::Start(length - tail_length))
        .and_then(|_| file.read_exact(&mut tail))
        .map_err(|e| io("read", archive, &e))?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&at| le32(&tail, at) == Some(ZIP_END))
        .ok_or_else(|| broken("it has no end record"))?;
    let count = le16(&tail, end + 10).ok_or_else(|| broken("its end record is cut short"))?;
    let size = le32(&tail, end + 12).ok_or_else(|| broken("its end record is cut short"))?;
    let start = le32(&tail, end + 16).ok_or_else(|| broken("its end record is cut short"))?;
    if u64::from(start) + u64::from(size) > length {
        return Err(broken("its directory lies outside the file"));
    }
    let mut central = vec![0; size as usize];
    file.seek(SeekFrom::Start(u64::from(start)))
        .and_then(|_| file.read_exact(&mut central))
        .map_err(|_| broken("its directory is cut short"))?;
    let mut listed = Vec::with_capacity(usize::from(count));
    let mut at = 0;
    for _ in 0..count {
        let field = |offset: usize| le32(&central, at + offset);
        let short = |offset: usize| le16(&central, at + offset);
        if field(0) != Some(ZIP_CENTRAL) {
            return Err(broken("an entry of its directory is damaged"));
        }
        let damaged = || broken("an entry of its directory is cut short");
        let method = short(10).ok_or_else(damaged)?;
        let time = unix_from_dos(
            short(12).ok_or_else(damaged)?,
            short(14).ok_or_else(damaged)?,
        );
        let crc = field(16).ok_or_else(damaged)?;
        let compressed = field(20).ok_or_else(damaged)?;
        let size = field(24).ok_or_else(damaged)?;
        let name_length = usize::from(short(28).ok_or_else(damaged)?);
        let skip = usize::from(short(30).ok_or_else(damaged)?)
            + usize::from(short(32).ok_or_else(damaged)?);
        let offset = field(42).ok_or_else(damaged)?;
        let name = central
            .get(at + 46..at + 46 + name_length)
            .ok_or_else(damaged)?;
        let path =
            String::from_utf8(name.to_vec()).map_err(|_| broken("an entry's name is not UTF-8"))?;
        at += 46 + name_length + skip;
        let contents = if !path.ends_with('/') && wanted(&path) {
            let mut local = [0u8; 30];
            file.seek(SeekFrom::Start(u64::from(offset)))
                .and_then(|_| file.read_exact(&mut local))
                .map_err(|_| broken("an entry is cut short"))?;
            if le32(&local, 0) != Some(ZIP_LOCAL) {
                return Err(broken("an entry's header is damaged"));
            }
            let data = u64::from(offset)
                + 30
                + u64::from(le16(&local, 26).unwrap_or(0))
                + u64::from(le16(&local, 28).unwrap_or(0));
            if data + u64::from(compressed) > length {
                return Err(broken("an entry lies outside the file"));
            }
            let mut packed = vec![0; compressed as usize];
            file.seek(SeekFrom::Start(data))
                .and_then(|_| file.read_exact(&mut packed))
                .map_err(|_| broken("an entry is cut short"))?;
            let bytes = match method {
                ZIP_STORED => packed,
                ZIP_DEFLATED => {
                    let mut bytes = Vec::new();
                    // Never more than the directory says (+1 to notice more).
                    flate2::read::DeflateDecoder::new(packed.as_slice())
                        .take(u64::from(size) + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| broken("an entry does not inflate"))?;
                    bytes
                }
                _ => {
                    return Err(broken(
                        "an entry uses a compression method other than deflate",
                    ));
                }
            };
            let mut check = flate2::Crc::new();
            check.update(&bytes);
            if bytes.len() as u64 != u64::from(size) || check.sum() != crc {
                return Err(broken("an entry's bytes do not match its checksum"));
            }
            Some(bytes)
        } else {
            None
        };
        listed.push(Listed {
            path,
            size: u64::from(size),
            mode: None,
            time,
            contents,
        });
    }
    Ok(listed)
}

fn parse_octal(field: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(field).ok()?;
    let text = text.trim_matches(|c: char| c == '\0' || c == ' ');
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

fn text_field(field: &[u8]) -> Option<String> {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8(field.get(..end)?.to_vec()).ok()
}

fn read_tar(archive: &Path, wanted: &dyn Fn(&str) -> bool) -> Result<Vec<Listed>> {
    let broken = |why: &str| not_an_archive(archive, why);
    let file = File::open(archive).map_err(|e| io("read", archive, &e))?;
    let mut tar = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut listed = Vec::new();
    let mut pax_path: Option<String> = None;
    loop {
        let mut block = [0u8; BLOCK as usize];
        tar.read_exact(&mut block)
            .map_err(|_| broken("it ends before its end blocks"))?;
        if block.iter().all(|&b| b == 0) {
            break;
        }
        let stored = parse_octal(&block[148..156]).ok_or_else(|| broken("a header is damaged"))?;
        let mut blank = block;
        blank[148..156].fill(b' ');
        if blank.iter().map(|&b| u64::from(b)).sum::<u64>() != stored {
            return Err(broken("a header's checksum is wrong"));
        }
        let field = |range: std::ops::Range<usize>| block.get(range).unwrap_or_default();
        let size = parse_octal(field(124..136)).ok_or_else(|| broken("a header is damaged"))?;
        let mode = parse_octal(field(100..108)).ok_or_else(|| broken("a header is damaged"))?;
        let time = parse_octal(field(136..148)).ok_or_else(|| broken("a header is damaged"))?;
        let name = text_field(field(0..100)).ok_or_else(|| broken("a name is not UTF-8"))?;
        let prefix = text_field(field(345..500)).ok_or_else(|| broken("a name is not UTF-8"))?;
        let kind = block[156];
        let read_body = |tar: &mut flate2::read::GzDecoder<BufReader<File>>, keep: bool| {
            let mut body = Vec::new();
            let mut taken = tar.by_ref().take(size + (BLOCK - size % BLOCK) % BLOCK);
            if keep {
                taken.read_to_end(&mut body)
            } else {
                std::io::copy(&mut taken, &mut std::io::sink()).map(|n| n as usize)
            }
            .map_err(|_| broken("an entry is cut short"))?;
            body.truncate(usize::try_from(size).unwrap_or(0));
            Ok::<_, Diagnostic>(body)
        };
        if kind == b'x' {
            let body = read_body(&mut tar, true)?;
            let text = String::from_utf8(body).map_err(|_| broken("a pax header is not UTF-8"))?;
            pax_path = text
                .lines()
                .filter_map(|line| line.split_once(' ').map(|(_, record)| record))
                .find_map(|record| record.strip_prefix("path=").map(str::to_owned));
            continue;
        }
        let path = pax_path.take().unwrap_or_else(|| {
            if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            }
        });
        let contents = match kind {
            b'5' => {
                read_body(&mut tar, false)?;
                None
            }
            b'0' | 0 => {
                let keep = wanted(&path);
                let body = read_body(&mut tar, keep)?;
                keep.then_some(body)
            }
            _ => {
                return Err(broken(
                    "it holds an entry that is neither a file nor a folder",
                ));
            }
        };
        listed.push(Listed {
            path,
            size,
            mode: u32::try_from(mode).ok(),
            time,
            contents,
        });
    }
    Ok(listed)
}

/// The SHA-256 of `bytes`, as 64 lowercase hex digits.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The SHA-256 of the file at `path`, as 64 lowercase hex digits.
pub fn sha256_of(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| io("read", path, &e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(|e| io("read", path, &e))?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One line of a checksum list: the hash and the file's name.
fn sum_line(line: &str) -> Option<(&str, &str)> {
    let (hash, rest) = line.split_at_checked(64)?;
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let name = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))?;
    (!name.is_empty()).then_some((hash, name))
}

/// The checksum list in `text`: file name → SHA-256, or `None` when a line
/// is not `<64 hex digits>  <name>`.
pub fn parse_sums(text: &str) -> Option<BTreeMap<String, String>> {
    let mut sums = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (hash, name) = sum_line(line)?;
        sums.insert(name.to_owned(), hash.to_ascii_lowercase());
    }
    Some(sums)
}

/// Puts `sha256` for the file `name` into the checksum list at `sums`
/// (`<hash>  <name>` per line, sorted by name, the format `sha256sum -c`
/// reads). Lines for other files are kept; a file there that is not a
/// checksum list is refused, never overwritten.
pub fn record_sum(sums: &Path, name: &str, sha256: &str) -> Result<()> {
    let mut list = match std::fs::read_to_string(sums) {
        Ok(text) => parse_sums(&text).ok_or_else(|| {
            failed(
                format!("'{}' is not a SHA-256 checksum list.", sums.display()),
                "Move it away or delete it, then run again: the list has one '<64 hex digits>  <file name>' line per file.",
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(io("read", sums, &error)),
    };
    list.insert(name.to_owned(), sha256.to_ascii_lowercase());
    let text: String = list
        .iter()
        .map(|(name, hash)| format!("{hash}  {name}\n"))
        .collect();
    let part = sums.with_file_name(format!("{SUMS}.part"));
    std::fs::write(&part, text).map_err(|e| io("write", &part, &e))?;
    std::fs::rename(&part, sums).map_err(|e| io("put in place", sums, &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_convert_both_ways() {
        assert_eq!(dos_time_date(ZIP_EPOCH), (0, 33));
        assert_eq!(dos_time_date(0), (0, 33));
        // 2026-11-18 12:34:56 UTC.
        assert_eq!(dos_time_date(1_795_005_296), (25_692, 23_922));
        assert_eq!(unix_from_dos(25_692, 23_922), 1_795_005_296);
        assert_eq!(unix_from_dos(0, 33), ZIP_EPOCH);
        // The last day zip can hold, and clamping past it.
        assert_eq!(dos_time_date(ZIP_LAST), dos_time_date(ZIP_LAST + 1_000_000));
        assert_eq!(
            unix_from_dos(dos_time_date(ZIP_LAST).0, dos_time_date(ZIP_LAST).1),
            ZIP_LAST - 1
        );
    }

    #[test]
    fn pax_records_count_their_own_length() {
        assert_eq!(pax_record("path", "x"), "9 path=x\n");
        // 98 bytes without the length: two digits would make 100, so it
        // takes three, and counts 101.
        let long = "a".repeat(91);
        let record = pax_record("path", &long);
        assert_eq!(record.len(), 101);
        assert!(record.starts_with("101 path="));
    }

    #[test]
    fn long_paths_split_or_need_pax() {
        let short = "top/file";
        assert_eq!(ustar_split(short), Some(("", short)));
        let deep = format!("{}/{}", "d".repeat(120), "f".repeat(90));
        assert_eq!(ustar_split(&deep), Some((&deep[..120], &deep[121..])));
        assert_eq!(ustar_split(&"x".repeat(101)), None);
        assert_eq!(ustar_split(&format!("{}/f", "d".repeat(160))), None);
    }

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sum_lines_are_read_in_both_modes() {
        let hash = "a".repeat(64);
        let sums = parse_sums(&format!("{hash}  one.zip\n{hash} *two.tar.gz\n")).unwrap();
        assert_eq!(sums.len(), 2);
        assert!(parse_sums("not a checksum list\n").is_none());
    }
}
