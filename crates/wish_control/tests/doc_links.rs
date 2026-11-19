//! Every relative link in the repository's Markdown files resolves (M12
//! package 11a; ADR-027, rule 4: a link check is one of the claims CI
//! reproduces for free): links to files and folders, with the letter case
//! the file has (GitHub and Linux are case-sensitive), and `#anchors` to
//! headings of Markdown targets by GitHub's rules. External `http(s)`
//! links are listed (`cargo test -p wish_control --test doc_links --
//! --nocapture`) but not fetched: CI does not depend on other sites.
//!
//! The files: every `*.md` of the repository outside hidden folders and
//! build or export folders: `README.md`, `CONTRIBUTING.md`, `docs/`, the
//! crates' Markdown (the recipes and guides the engine serves), the
//! examples, evaluations and tools.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

fn repository() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::canonicalize(here).unwrap()
}

/// Every Markdown file under `folder`, sorted.
fn markdown_files(folder: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            // Version control and a project's own build folder; other
            // hidden folders (`.github`, `.template`) are read.
            if [".git", ".wish", "target", "exports", "node_modules"].contains(&name.as_str()) {
                continue;
            }
            markdown_files(&path, out);
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
}

/// The line without its inline code spans.
fn without_code(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            let mut run = 0;
            while i + run < chars.len() && chars[i + run] == '`' {
                run += 1;
            }
            // The closing run of the same length, if there is one.
            let mut j = i + run;
            let mut closed = None;
            while j < chars.len() {
                if chars[j] == '`' {
                    let mut other = 0;
                    while j + other < chars.len() && chars[j + other] == '`' {
                        other += 1;
                    }
                    if other == run {
                        closed = Some(j + other);
                        break;
                    }
                    j += other;
                } else {
                    j += 1;
                }
            }
            match closed {
                Some(end) => {
                    // Keep the code's text: a heading's anchor is made of it.
                    out.extend(&chars[i + run..end - run]);
                    i = end;
                }
                None => {
                    out.extend(&chars[i..i + run]);
                    i += run;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The line with its inline code spans removed entirely (for finding links).
fn strip_code(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let run = rest[start..].chars().take_while(|c| *c == '`').count();
        let after = &rest[start + run..];
        let fence = "`".repeat(run);
        match after.find(&fence) {
            Some(end) => rest = &after[end + run..],
            None => {
                out.push_str(&rest[start..start + run]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The lines of a Markdown file outside fenced code blocks, numbered.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fence: Option<char> = None;
    let mut out = Vec::new();
    // Indented code blocks: lines indented by four or more after a blank
    // line, outside a list (in a list, indentation continues the item).
    let mut in_list = false;
    let mut previous_blank = true;
    let mut in_indented = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent: usize = line[..line.len() - trimmed.len()]
            .chars()
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();
        let blank = trimmed.is_empty();
        let marker = trimmed.chars().next();
        // A fence may be indented inside a list item.
        if fence.is_some() || !in_indented {
            if (trimmed.starts_with("```") || trimmed.starts_with("~~~")) && marker.is_some() {
                match fence {
                    None => fence = marker,
                    Some(open) if Some(open) == marker => fence = None,
                    Some(_) => {}
                }
                previous_blank = false;
                continue;
            }
            if fence.is_some() {
                continue;
            }
        }
        if in_indented {
            if blank || indent >= 4 {
                continue;
            }
            in_indented = false;
        }
        if !blank && indent >= 4 && previous_blank && !in_list {
            in_indented = true;
            continue;
        }
        if !blank && indent <= 3 {
            if is_list_item(trimmed) {
                in_list = true;
            } else if previous_blank {
                in_list = false;
            }
        }
        previous_blank = blank;
        out.push((index + 1, line));
    }
    out
}

/// Whether a line (without its indentation) starts a list item.
fn is_list_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") "))
}

/// A reference label as Markdown compares them: case and runs of white
/// space do not matter.
fn label(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The labels a file defines (`[id]: target`).
fn definitions(lines: &[(usize, &str)]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, line) in lines {
        let trimmed = line.trim_start();
        if line.len() - trimmed.len() <= 3
            && let Some(rest) = trimmed.strip_prefix('[')
            && let Some(close) = rest.find("]:")
            && !rest[..close].contains('[')
        {
            out.insert(label(&rest[..close]));
        }
    }
    out
}

/// The labels the reference links of a line use: `[text][id]` uses `id`,
/// `[text][]` uses `text`.
fn references(line: &str) -> Vec<String> {
    let line = strip_code(line);
    let mut found = Vec::new();
    let mut search = 0;
    while let Some(at) = line[search..].find("][").map(|i| search + i) {
        let after = &line[at + 2..];
        let Some(close) = after.find(']') else {
            break;
        };
        let id = &after[..close];
        let used = if id.trim().is_empty() {
            // The text of the link: back to its opening bracket.
            match line[..at].rfind('[') {
                Some(open) => &line[open + 1..at],
                None => "",
            }
        } else {
            id
        };
        // `[x][y](z)` is an inline link whose text has brackets.
        if !used.trim().is_empty() && !after[close + 1..].starts_with('(') {
            found.push(label(used));
        }
        search = at + 2 + close + 1;
    }
    found
}

/// The targets of the links on a line (inline links and images, reference
/// definitions, and `href`/`src` of HTML tags).
fn targets(line: &str) -> Vec<String> {
    let line = strip_code(line);
    let mut found = Vec::new();
    // Inline: ](target "title")
    let mut rest = line.as_str();
    while let Some(at) = rest.find("](") {
        let after = &rest[at + 2..];
        let mut depth = 0usize;
        let mut end = after.len();
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    end = i;
                    break;
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        let inner = after[..end].trim();
        let target = if let Some(angled) = inner.strip_prefix('<') {
            angled.split('>').next().unwrap_or("")
        } else {
            inner.split_whitespace().next().unwrap_or("")
        };
        if !target.is_empty() {
            found.push(target.to_owned());
        }
        rest = &after[end.min(after.len())..];
    }
    // Reference definitions: [id]: target
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() <= 3
        && trimmed.starts_with('[')
        && let Some(close) = trimmed.find("]:")
        && !trimmed[1..close].contains('[')
    {
        let value = trimmed[close + 2..].trim();
        let value = value.trim_start_matches('<');
        if let Some(target) = value.split(['>', ' ']).next()
            && !target.is_empty()
        {
            found.push(target.to_owned());
        }
    }
    // HTML: href="..." and src="..."
    for attribute in ["href=\"", "src=\""] {
        let mut rest = line.as_str();
        while let Some(at) = rest.find(attribute) {
            let after = &rest[at + attribute.len()..];
            let end = after.find('"').unwrap_or(after.len());
            found.push(after[..end].to_owned());
            rest = &after[end..];
        }
    }
    found
}

/// A heading's anchor, as GitHub makes it: the text as shown (code, links
/// and emphasis marks gone), lower case, every character that is not a
/// letter, a number, `-` or `_` dropped, and each space a `-`.
fn slug(heading: &str) -> String {
    let text = without_code(heading);
    // Links and images show their text.
    let mut shown = String::new();
    let mut rest = text.as_str();
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        let before = rest[..open].trim_end_matches('!');
        shown.push_str(before);
        shown.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    shown.push_str(rest);
    // HTML tags show nothing of themselves.
    let mut plain = String::new();
    let mut in_tag = false;
    for c in shown.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let mut out = String::new();
    for c in plain.trim().chars() {
        if c == ' ' {
            out.push('-');
        } else if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Every anchor a Markdown file has: its headings' (repeated ones numbered
/// `-1`, `-2`, ...) and explicit `<a id>`/`<a name>`.
fn anchors(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let lines = prose_lines(text);
    let mut add = |heading: &str, out: &mut BTreeSet<String>| {
        let base = slug(heading);
        let count = seen.entry(base.clone()).or_insert(0);
        out.insert(if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        });
        *count += 1;
    };
    for (position, (number, line)) in lines.iter().enumerate() {
        for attribute in ["<a id=\"", "<a name=\""] {
            let mut rest = *line;
            while let Some(at) = rest.find(attribute) {
                let after = &rest[at + attribute.len()..];
                let end = after.find('"').unwrap_or(after.len());
                out.insert(after[..end].to_owned());
                rest = &after[end..];
            }
        }
        let trimmed = line.trim_start();
        if line.len() - trimmed.len() > 3 {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with([' ', '\t']) {
            let heading = trimmed[hashes..].trim().trim_end_matches('#').trim_end();
            add(heading, &mut out);
            continue;
        }
        // A setext heading: text over a line of = or -.
        if let Some((next_number, next)) = lines.get(position + 1)
            && *next_number == number + 1
            && !trimmed.is_empty()
            && !trimmed.starts_with(['-', '*', '+', '|', '>', '#'])
        {
            let underline = next.trim();
            if !underline.is_empty()
                && (underline.chars().all(|c| c == '=') || underline.chars().all(|c| c == '-'))
                && next.len() - next.trim_start().len() <= 3
            {
                add(trimmed.trim(), &mut out);
            }
        }
    }
    out
}

/// `%XX` escapes decoded.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(value) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The target inside the repository, every part named with the letter case
/// it has on disk; `Err` says what is wrong.
fn resolve(root: &Path, from: &Path, target: &str) -> Result<PathBuf, String> {
    let joined = if let Some(absolute) = target.strip_prefix('/') {
        root.join(absolute)
    } else {
        from.parent().unwrap().join(target)
    };
    // Lexically: `..` and `.` resolved, no file system needed.
    let mut parts: Vec<String> = Vec::new();
    let relative = joined
        .strip_prefix(root)
        .map_err(|_| "it is outside the repository".to_owned())?
        .to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::ParentDir if parts.pop().is_none() => {
                return Err("it leads outside the repository".to_owned());
            }
            _ => {}
        }
    }
    let mut here = root.to_path_buf();
    for part in &parts {
        let names: Vec<String> = std::fs::read_dir(&here)
            .map_err(|_| format!("'{}' is not a folder", here.display()))?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        if !names.contains(part) {
            return Err(match names.iter().find(|n| n.eq_ignore_ascii_case(part)) {
                Some(other) => format!("'{part}' is written '{other}' on disk"),
                None => format!("'{part}' does not exist"),
            });
        }
        here.push(part);
    }
    Ok(here)
}

#[test]
fn every_relative_link_in_the_markdown_files_resolves() {
    let root = repository();
    let mut files = Vec::new();
    markdown_files(&root, &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("README.md"))
            && files.iter().any(|f| f.ends_with("recipes.md"))
            && files.iter().any(|f| f.ends_with("guide.md")),
        "{files:?}"
    );
    let mut cached: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut anchors_of = |path: &Path| -> BTreeSet<String> {
        cached
            .entry(path.to_path_buf())
            .or_insert_with(|| anchors(&std::fs::read_to_string(path).unwrap()))
            .clone()
    };
    let mut broken = Vec::new();
    let mut external = BTreeSet::new();
    let mut checked = 0usize;
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let name = file
            .strip_prefix(&root)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        let lines = prose_lines(&text);
        let defined = definitions(&lines);
        for (number, line) in lines {
            for used in references(line) {
                checked += 1;
                if !defined.contains(&used) {
                    broken.push(format!(
                        "{name}:{number}: [{used}]: the reference link has no definition ([{used}]: target)"
                    ));
                }
            }
            for target in targets(line) {
                let scheme = target
                    .split_once(':')
                    .map(|(scheme, _)| scheme)
                    .filter(|scheme| {
                        !scheme.is_empty()
                            && scheme
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
                    });
                if let Some(scheme) = scheme {
                    if scheme == "http" || scheme == "https" {
                        external.insert(target.clone());
                    }
                    // mailto:, wish:// (the engine's own resources): not files.
                    continue;
                }
                checked += 1;
                let (path, anchor) = match target.split_once('#') {
                    Some((path, anchor)) => (path, Some(anchor)),
                    None => (target.as_str(), None),
                };
                let resolved = if path.is_empty() {
                    file.clone()
                } else {
                    match resolve(&root, file, &percent_decoded(path)) {
                        Ok(resolved) => resolved,
                        Err(why) => {
                            broken.push(format!("{name}:{number}: {target}: {why}"));
                            continue;
                        }
                    }
                };
                if let Some(anchor) = anchor
                    && resolved.is_file()
                    && resolved.extension().is_some_and(|e| e == "md")
                {
                    let anchor = percent_decoded(anchor);
                    if !anchors_of(&resolved).contains(&anchor) {
                        broken.push(format!(
                            "{name}:{number}: {target}: no heading has the anchor '#{anchor}'"
                        ));
                    }
                }
            }
        }
    }
    println!(
        "{} Markdown files, {checked} relative links; {} external links (not fetched):",
        files.len(),
        external.len()
    );
    for link in &external {
        println!("  {link}");
    }
    assert!(checked > 100, "only {checked} relative links were found");
    assert!(
        broken.is_empty(),
        "{} broken links:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

#[test]
fn anchors_follow_githubs_rules() {
    let text = "# M12 — the road to a strong 1.0\n\n## 1. Install and first start\n\n## `wen` & the `engines` tool\n\n## Notes\n\n## Notes\n\nSetext title\n============\n\n```\n# not a heading\n```\n\n## [A link](x.md) and **bold**\n\n## 1. Kurulum ve ilk açılış\n";
    let found = anchors(text);
    for expected in [
        "m12--the-road-to-a-strong-10",
        "1-install-and-first-start",
        "wen--the-engines-tool",
        "notes",
        "notes-1",
        "setext-title",
        "a-link-and-bold",
        "1-kurulum-ve-ilk-açılış",
    ] {
        assert!(found.contains(expected), "{expected} not in {found:?}");
    }
    assert!(!found.contains("not-a-heading"));
    assert_eq!(
        targets("See [a](b.md#c \"title\"), `[x](no.md)`, [d]( <e f.md> ) and <img src=\"g.png\">"),
        ["b.md#c", "e f.md", "g.png"]
    );
    assert_eq!(targets("[ref]: docs/x.md"), ["docs/x.md"]);
    assert_eq!(
        references("See [the guide][Guide  One], [this][] and `[not][code]`."),
        ["guide one", "this"]
    );
    let text =
        "Text\n\n    [in](indented-code.md)\n\n- item\n\n    [in](a-list.md)\n\n[ref]: x.md\n";
    let lines = prose_lines(text);
    let kept: Vec<&str> = lines.iter().map(|(_, line)| line.trim()).collect();
    assert!(!kept.contains(&"[in](indented-code.md)"), "{kept:?}");
    assert!(kept.contains(&"[in](a-list.md)"), "{kept:?}");
    assert_eq!(definitions(&lines), BTreeSet::from(["ref".to_owned()]));
    assert_eq!(percent_decoded("a%20b.md"), "a b.md");
}
