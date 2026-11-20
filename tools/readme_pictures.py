"""Makes the pictures of the README (docs/images/*.png) again, with the
engine's own tools, from a checkout.

Every picture is drawn by `wen` on the software adapter (WISH_SOFTWARE=1),
so that it does not depend on a graphics card: a rerun on the same adapter
writes the same bytes (WARP on Windows and lavapipe on Linux differ
slightly, so their bytes differ). Nothing sets a look: every picture is the
engine's default look. Each project is made in a scratch folder (a copy of
an example, or `wen new`), so the repository is never written to except
docs/images. The PNGs are then compressed again here (lossless; the
standard library only) to keep the repository light.

The pictures: islands, hop and bricks are the examples from their own
cameras; outdoor is the level of tools/readme_level.json built with one
edit_apply call on the scene of `wen new`; editor and editor-ai are
`wen edit --headless-frame` on the showcase.

Usage (repository root, after `cargo build --release -p wish_cli`):

    python tools/readme_pictures.py [--wen <path to wen>] [--work <folder>]
                                    [--keep] [--only <name> ...]

`--wen` defaults to the release `wen` in cargo's target folder. `--work` is
the scratch folder (default: a new temporary folder, deleted at the end
unless --keep). It runs `wen sdk` first, so that game code is built against
the SDK of that same `wen`.
"""

import argparse
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "docs", "images")
WIDTH, HEIGHT = 960, 540


def capture(name):
    """The last call of a picture: the scene from its own camera."""
    return {"tool": "capture_frame", "args": {
        "width": WIDTH, "height": HEIGHT, "out": f"readme-{name}.png", "overwrite": True}}


def level_ops():
    with open(os.path.join(ROOT, "tools", "readme_level.json"), encoding="utf-8") as file:
        return json.load(file)["ops"]


def editor_script(name):
    """The editor at 1280x720 with the showcase open, after a few idle
    frames so that the views have drawn. "editor": a crystal selected in the
    hierarchy (its fourth row, under the filter), so that the gizmo and the
    inspector show. "editor-ai": the switch in the toolbar set to With AI.
    The places are those of the editor's layout since its redesign (M12)."""
    click = [70, 216] if name == "editor" else [534, 49]
    return {
        "size": [1280, 720],
        "steps": [
            {"idle": 3},
            {"events": [{"click": click}]},
            {"idle": 3},
        ],
    }


# Each picture: the project it is drawn in ("example:<name>" is a copy of
# examples/<name>; "new" is the scene of `wen new`), and either the tool
# calls of one `wen do` (the last one writes readme-<name>.png into the
# project) or "editor". Nothing here sets a look: every picture is the
# engine's default look, Natural.
PICTURES = {
    "islands": ("example:islands", lambda: [capture("islands")]),
    "outdoor": ("new", lambda: [
        {"tool": "edit_apply", "args": {"ops": level_ops()}},
        capture("outdoor"),
    ]),
    "hop": ("example:hop", lambda: [capture("hop")]),
    "bricks": ("example:bricks", lambda: [capture("bricks")]),
    "editor": ("example:islands", "editor"),
    "editor-ai": ("example:islands", "editor"),
}


def run(wen, *args, env=None):
    result = subprocess.run([wen, *args], capture_output=True, text=True,
                            env=env, encoding="utf-8", errors="replace")
    if result.returncode != 0:
        sys.exit(f"`wen {' '.join(args)}` failed:\n{result.stdout}\n{result.stderr}")
    return result.stdout


def default_wen():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT, capture_output=True, check=True).stdout)
    name = "wen.exe" if os.name == "nt" else "wen"
    return os.path.join(meta["target_directory"], "release", name)


def make_project(source, name, wen, work, env):
    """Makes the project a picture is drawn in, in its own folder of the
    scratch folder, and builds its game code."""
    target = os.path.join(work, name)
    if os.path.isdir(target):
        shutil.rmtree(target)
    if source == "new":
        run(wen, "new", target, env=env)
    else:
        # A copy of the example, without what the engine made in it.
        shutil.copytree(os.path.join(ROOT, "examples", source.split(":", 1)[1]), target,
                        ignore=shutil.ignore_patterns(".wish", "exports", "target"))
    run(wen, "build", "--root", target, env=env)
    return target


def run_calls(wen, project, calls, env):
    """Runs the calls in one `wen do`; every answer must be ok."""
    lines = "".join(json.dumps(call) + "\n" for call in calls)
    result = subprocess.run([wen, "do", "-", "--root", project], input=lines,
                            capture_output=True, text=True, env=env,
                            encoding="utf-8", errors="replace")
    answers = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    if result.returncode != 0 or len(answers) != len(calls) or not all(
            answer.get("ok") for answer in answers):
        sys.exit(f"`wen do` in {project} failed:\n{result.stdout[-4000:]}\n{result.stderr}")


# --- PNG: read 8-bit RGB/RGBA, write it again smaller (lossless) ---------

def read_png(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit(f"{path} is not a PNG")
    at, idat, header = 8, [], None
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        at += 12 + length
    width, height, depth, colour, _, _, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        sys.exit(f"{path}: only 8-bit RGB or RGBA without interlacing is read here")
    channels = 4 if colour == 6 else 3
    raw = zlib.decompress(b"".join(idat))
    stride = width * channels
    rows, previous, at = [], bytearray(stride), 0
    for _ in range(height):
        kind, line = raw[at], bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        unfilter(kind, line, previous, channels)
        rows.append(line)
        previous = line
    return width, height, channels, rows


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def unfilter(kind, line, up, bpp):
    if kind == 0:
        return
    for i in range(len(line)):
        a = line[i - bpp] if i >= bpp else 0
        b = up[i]
        if kind == 1:
            line[i] = (line[i] + a) & 255
        elif kind == 2:
            line[i] = (line[i] + b) & 255
        elif kind == 3:
            line[i] = (line[i] + ((a + b) >> 1)) & 255
        elif kind == 4:
            c = up[i - bpp] if i >= bpp else 0
            line[i] = (line[i] + paeth(a, b, c)) & 255


def filtered(kind, line, up, bpp):
    if kind == 0:
        return bytes(line)
    out = bytearray(len(line))
    for i in range(len(line)):
        a = line[i - bpp] if i >= bpp else 0
        b = up[i]
        if kind == 1:
            out[i] = (line[i] - a) & 255
        elif kind == 2:
            out[i] = (line[i] - b) & 255
        elif kind == 3:
            out[i] = (line[i] - ((a + b) >> 1)) & 255
        else:
            c = up[i - bpp] if i >= bpp else 0
            out[i] = (line[i] - paeth(a, b, c)) & 255
    return bytes(out)


def chunk(kind, body):
    return (struct.pack(">I", len(body)) + kind + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF))


def write_png(path, width, height, channels, rows):
    """Writes RGB when every pixel is opaque; picks each row's filter by the
    usual smallest-sum rule; compresses at level 9."""
    if channels == 4 and all(line[i] == 255 for line in rows for i in range(3, len(line), 4)):
        rows = [bytearray(b for i, b in enumerate(line) if i % 4 != 3) for line in rows]
        channels = 3
    stride = width * channels
    previous, body = bytearray(stride), bytearray()
    for line in rows:
        best = None
        for kind in range(5):
            candidate = filtered(kind, line, previous, channels)
            score = sum(b if b < 128 else 256 - b for b in candidate)
            if best is None or score < best[0]:
                best = (score, kind, candidate)
        body.append(best[1])
        body += best[2]
        previous = line
    colour = 6 if channels == 4 else 2
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(bytes(body), 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as file:
        file.write(png)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--wen", help="the wen to draw with")
    parser.add_argument("--work", help="scratch folder for project copies")
    parser.add_argument("--keep", action="store_true", help="keep the scratch folder")
    parser.add_argument("--only", nargs="*", choices=sorted(PICTURES), help="only these")
    args = parser.parse_args()
    wen = os.path.abspath(args.wen or default_wen())
    if not os.path.isfile(wen):
        sys.exit(f"No wen at {wen}: build it with `cargo build --release -p wish_cli`.")
    work = os.path.abspath(args.work) if args.work else tempfile.mkdtemp(prefix="readme-pictures-")
    os.makedirs(work, exist_ok=True)
    os.makedirs(OUT, exist_ok=True)
    env = dict(os.environ, WISH_SOFTWARE="1")
    run(wen, "sdk", env=env)
    try:
        for name in args.only or PICTURES:
            source, how = PICTURES[name]
            project = make_project(source, name, wen, work, env)
            shot = os.path.join(project, f"readme-{name}.png")
            if how == "editor":
                script = os.path.join(work, f"{name}.script.json")
                with open(script, "w", encoding="utf-8") as file:
                    json.dump(editor_script(name), file)
                run(wen, "edit", "--root", project, "--software", "--lang", "en",
                    "--headless-frame", script, "--out", shot, env=env)
            else:
                run_calls(wen, project, how(), env)
            target = os.path.join(OUT, f"{name}.png")
            write_png(target, *read_png(shot))
            print(f"docs/images/{name}.png  {os.path.getsize(target) // 1024} KiB")
    finally:
        if not args.keep and not args.work:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
