"""Writes THIRD_PARTY_LICENSES.md: every library in the dependency tree (not
the engine's own crates), its license and, once each, the license texts the
library ships (LICENSE*, COPYING*, NOTICE*), then the third-party works that
are not crates (EXTRA: the engine's font). Exported games and engine packages
carry this file (M9, M12); a test checks that it names every library in
Cargo.lock.

Usage (repository root): python tools/third_party.py
"""

import hashlib
import json
import os
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "THIRD_PARTY_LICENSES.md")
TEXT_FILES = ("license", "licence", "copying", "notice")
# Third-party works the engine ships that are not crates (so not in
# Cargo.lock): name, version, license, license files (from the root).
EXTRA = [
    ("Roboto (the engine's font)", "2.138", "Apache-2.0",
     ["crates/wish_text/fonts/LICENSE-Roboto.txt"]),
]


def main():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT, capture_output=True, check=True).stdout)
    members = set(meta["workspace_members"])
    packages = sorted(
        (p for p in meta["packages"] if p["id"] not in members),
        key=lambda p: (p["name"], p["version"]))
    texts = {}  # hash -> (text, [package names])
    rows = []
    for package in packages:
        folder = os.path.dirname(package["manifest_path"])
        found = []
        try:
            names = sorted(os.listdir(folder))
        except OSError:
            names = []
        for name in names:
            path = os.path.join(folder, name)
            if not os.path.isfile(path) or not name.lower().startswith(TEXT_FILES):
                continue
            try:
                text = open(path, encoding="utf-8", errors="replace").read().strip()
            except OSError:
                continue
            key = hashlib.sha256(text.encode()).hexdigest()[:12]
            texts.setdefault(key, [text, []])[1].append(f"{package['name']} {package['version']}")
            found.append(key)
        rows.append((package, found))
    extra_rows = []
    for name, version, license, files in EXTRA:
        found = []
        for file in files:
            text = open(os.path.join(ROOT, file), encoding="utf-8", errors="replace").read().strip()
            key = hashlib.sha256(text.encode()).hexdigest()[:12]
            texts.setdefault(key, [text, []])[1].append(f"{name} {version}")
            found.append(key)
        extra_rows.append((name, version, license, found))
    lines = [
        "# Third-party libraries",
        "",
        "Wish Engine and the games made with it use these libraries. Each is listed with its license;",
        "the license texts the libraries ship follow, each text once, with the libraries it belongs to.",
        "Made by tools/third_party.py from Cargo.lock, with the works that are not libraries last.",
        "Engine packages also carry the Rust compiler (toolchain/): its own notices are in",
        "toolchain/share/doc/rust.",
        "",
        "| Library | Version | License | Texts |",
        "|---|---|---|---|",
    ]
    for package, found in rows:
        license = (package.get("license") or "see its texts").replace("|", "/")
        lines.append(f"| {package['name']} | {package['version']} | {license} | {', '.join(found) or '-'} |")
    for name, version, license, found in extra_rows:
        lines.append(f"| {name} | {version} | {license} | {', '.join(found)} |")
    lines.append("")
    for key, (text, owners) in sorted(texts.items(), key=lambda kv: kv[1][1][0]):
        lines.append(f"## Text {key}")
        lines.append("")
        lines.append("Used by: " + ", ".join(owners))
        lines.append("")
        lines.append("```text")
        lines.append(text.replace("```", "'''"))
        lines.append("```")
        lines.append("")
    open(OUT, "w", encoding="utf-8", newline="\n").write("\n".join(lines))
    print(f"{len(rows)} libraries, {len(texts)} distinct texts, {os.path.getsize(OUT)} bytes")


main()
