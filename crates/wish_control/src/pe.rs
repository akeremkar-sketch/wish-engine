//! Reading a Windows program's import table: whether a program needs the
//! Visual C++ runtime DLL, which a clean Windows may lack (`wen package`
//! and `project_export` warn about it).

/// The DLLs a Windows program imports, read from its PE import table (an
/// empty list for anything that is not a PE file).
pub fn imported_dlls(bytes: &[u8]) -> Vec<String> {
    let u16_at = |at: usize| -> Option<usize> {
        Some(usize::from(u16::from_le_bytes(
            bytes.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let u32_at = |at: usize| -> Option<usize> {
        usize::try_from(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()
    };
    let read = || -> Option<Vec<String>> {
        let pe = u32_at(0x3c)?;
        if bytes.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let sections = u16_at(pe + 6)?;
        let optional = pe + 24;
        let optional_size = u16_at(pe + 20)?;
        // PE32+ (0x20b) keeps the data directories 16 bytes further on.
        let directories = optional + if u16_at(optional)? == 0x20b { 112 } else { 96 };
        let import_rva = u32_at(directories + 8)?;
        let table = optional + optional_size;
        let to_offset = |rva: usize| -> Option<usize> {
            (0..sections).find_map(|i| {
                let header = table + i * 40;
                let start = u32_at(header + 12)?;
                let size = u32_at(header + 8)?.max(u32_at(header + 16)?);
                let raw = u32_at(header + 20)?;
                (rva >= start && rva < start + size).then(|| raw + rva - start)
            })
        };
        let mut names = Vec::new();
        let mut descriptor = to_offset(import_rva)?;
        while names.len() < 256 {
            let name_rva = u32_at(descriptor + 12)?;
            if name_rva == 0 {
                break;
            }
            let at = to_offset(name_rva)?;
            let end = bytes.get(at..)?.iter().position(|b| *b == 0)?;
            names.push(String::from_utf8_lossy(&bytes[at..at + end]).to_string());
            descriptor += 20;
        }
        Some(names)
    };
    read().unwrap_or_default()
}

/// Whether the program imports the Visual C++ runtime DLL (`vcruntime*`).
pub fn needs_c_runtime_dll(bytes: &[u8]) -> bool {
    imported_dlls(bytes)
        .iter()
        .any(|dll| dll.to_ascii_lowercase().starts_with("vcruntime"))
}
