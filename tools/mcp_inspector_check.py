"""Checks Wish Engine's MCP server with a client that is not Claude: the
official MCP Inspector (CLI mode), M7 exit criterion 3.

Every step starts `wen mcp` afresh through the Inspector, so each call also
proves the handshake (initialize, capabilities, protocol version). The steps
are the basic tasks an agent does: discover the tools, read the docs, change
the scene, look at it, write and build game code, run the tests, read an
error's hint. Writes a report next to the eval results.

Usage (from the repository root; Inspector installed with
`npm install --prefix C:/wish-build/mcp-inspector --ignore-scripts
@modelcontextprotocol/inspector@2.13.0`):

    python tools/mcp_inspector_check.py [--wen PATH] [--inspector DIR] [--project DIR]
"""

import argparse
import base64
import datetime
import json
import os
import shutil
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

parser = argparse.ArgumentParser()
parser.add_argument("--wen", default=r"C:\wish-build\target\release\wen.exe")
parser.add_argument("--inspector", default=r"C:\wish-build\mcp-inspector")
parser.add_argument("--project", default=r"C:\wish-build\inspector-proj")
parser.add_argument("--report", default=os.path.join(REPO, "evals", "results", "m7-mcp-inspector.md"))
options = parser.parse_args()

LAUNCHER = os.path.join(
    options.inspector, "node_modules", "@modelcontextprotocol", "inspector",
    "clients", "launcher", "build", "index.js")
PACKAGE = os.path.join(
    options.inspector, "node_modules", "@modelcontextprotocol", "inspector", "package.json")
VERSION = json.load(open(PACKAGE, encoding="utf-8"))["version"]


def inspector(method, *extra, strict=False):
    """One Inspector CLI run against a fresh `wen mcp`; returns (exit, json, stderr)."""
    command = ["node", LAUNCHER, "--cli"]
    if strict:
        command.append("--strict")
    command += [options.wen, "mcp", "--root", options.project, "--method", method, *extra]
    done = subprocess.run(command, capture_output=True, timeout=600)
    out = done.stdout.decode("utf-8", "replace")
    try:
        value = json.loads(out)
    except ValueError:
        value = None
    return done.returncode, value, done.stderr.decode("utf-8", "replace")


def call(tool, **args):
    pairs = [f"{k}={json.dumps(v) if not isinstance(v, str) else v}" for k, v in args.items()]
    extra = ["--tool-name", tool]
    if pairs:
        extra += ["--tool-arg", *pairs]
    code, value, err = inspector("tools/call", *extra)
    if value is None:
        raise AssertionError(f"{tool}: no JSON answer (exit {code}): {err[:400]}")
    return value


results = []


def step(name):
    def wrap(check):
        try:
            detail = check()
            results.append((name, True, detail or ""))
        except Exception as error:  # noqa: BLE001 - every failure is reported
            results.append((name, False, str(error)[:600]))
        return check
    return wrap


# A fresh project every time.
shutil.rmtree(options.project, ignore_errors=True)
subprocess.run([options.wen, "new", options.project, "--name", "Inspector Check"],
               check=True, capture_output=True)


@step("tools/list, strict schema portability")
def _():
    code, value, err = inspector("tools/list", strict=True)
    assert code == 0, f"exit {code}: {err[:600]}"
    tools = value["tools"]
    assert len(tools) == 25, f"{len(tools)} tools"
    for tool in tools:
        assert tool.get("description") and tool.get("inputSchema", {}).get("type") == "object", tool["name"]
        assert "outputSchema" in tool, f"{tool['name']} has no outputSchema"
    return f"{len(tools)} tools, every one with input and output schema; no portability problem reported"


@step("resources/list and resources/read (guide)")
def _():
    code, value, err = inspector("resources/list")
    assert code == 0, err[:400]
    uris = [r["uri"] for r in value["resources"]]
    assert "wish://docs/guide" in uris, uris
    code, page, err = inspector("resources/read", "--uri", "wish://docs/guide")
    assert code == 0, err[:400]
    text = page["contents"][0]["text"]
    assert "edit_apply" in text
    return f"{len(uris)} resources; guide is {len(text)} characters"


@step("prompts/list and prompts/get")
def _():
    code, value, err = inspector("prompts/list")
    assert code == 0, err[:400]
    names = [p["name"] for p in value["prompts"]]
    assert names, "no prompts"
    code, got, err = inspector("prompts/get", "--prompt-name", names[0])
    assert code == 0 and got["messages"], err[:400]
    return f"prompts: {', '.join(names)}"


@step("project_info")
def _():
    answer = call("project_info")["structuredContent"]
    assert answer["project"]["name"] == "Inspector Check", answer
    return f"scene {answer['project']['scene']}, {answer['types']['components']} component types"


@step("schema_get (search)")
def _():
    answer = call("schema_get", search="Collider")["structuredContent"]
    assert "Collider" in json.dumps(answer)
    return "finds Collider"


@step("edit_apply: a falling crate")
def _():
    ops = [{"op": "spawn", "id": "crate", "components": {
        "Transform": {"translation": [0, 3, 0]}, "Primitive": {"shape": "Cube"},
        "Material": {"base_color": [0.9, 0.5, 0.1, 1]},
        "RigidBody": {"kind": "Dynamic"}, "Collider": {"shape": "Box"}}},
        {"op": "set", "id": "ground", "component": "Collider",
         "value": {"shape": "Box", "size": [1, 0.1, 1], "offset": [0, -0.05, 0]}}]
    answer = call("edit_apply", ops=ops)
    assert answer["isError"] is False and answer["structuredContent"]["ok"], answer
    got = call("entity_get", ids=["crate"])["structuredContent"]
    assert got["entities"][0]["components"]["RigidBody"]["kind"] == "Dynamic"
    return "spawned and read back"


@step("an error carries code and hint")
def _():
    answer = call("edit_apply", ops=[{"op": "set", "id": "crate", "component": "Colider", "value": {}}])
    assert answer["isError"] is True, answer
    error = answer["structuredContent"]["error"]
    assert error["code"] and error["hint"], error
    return f"{error['code']}: {error['hint'][:90]}"


@step("describe_view and capture_frame")
def _():
    seen = call("describe_view", width=320, height=180)["structuredContent"]
    ids = [v["entity"] for v in seen["visible"]]
    assert "crate" in ids, ids
    shot = call("capture_frame", width=320, height=180)
    images = [c for c in shot["content"] if c["type"] == "image"]
    assert images and images[0]["mimeType"] == "image/png", shot["content"][:1]
    png = base64.b64decode(images[0]["data"])
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    return f"crate visible; PNG {len(png)} bytes"


@step("game code: script_build")
def _():
    code = """use wish_sdk::prelude::*;

/// Counts ticks.
#[resource]
pub struct Counter {
    /// Ticks so far.
    pub ticks: u32,
}

#[system]
fn count(mut counter: ResMut<Counter>) {
    counter.ticks += 1;
}

wish_sdk::export! {
    resources: [Counter],
    systems: [count],
}
"""
    with open(os.path.join(options.project, "scripts", "src", "lib.rs"), "w", encoding="utf-8", newline="\n") as f:
        f.write(code)
    answer = call("script_build")["structuredContent"]
    assert answer["passed"] is True, answer
    return "built"


@step("test_run: a scenario test of the falling crate and the counter")
def _():
    test = {"format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": [
        {"frames": 120},
        {"expect": "Transform.translation.y < 0.6 && Transform.translation.y > 0.4", "entity": "crate",
         "message": "the crate fell and rests on the ground"},
        {"expect": "Counter.ticks == 120", "message": "game code ran every tick"}]}
    with open(os.path.join(options.project, "tests", "fall.test.json"), "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(test, indent=2) + "\n")
    answer = call("test_run")["structuredContent"]
    assert answer["passed"] is True, answer
    return f"{len(answer['results'])} tests passed"


@step("asset_create and check")
def _():
    made = call("asset_create", path="assets/sounds/coin.wav",
                sound={"wave": "square", "frequency": 880, "seconds": 0.15})["structuredContent"]
    assert made.get("ok", True), made
    answer = call("check")["structuredContent"]
    assert answer["passed"] is True, answer
    return "sound made; project passes check"


passed = sum(1 for _, ok, _ in results if ok)
lines = [
    "# M7: Wish Engine through MCP Inspector",
    "",
    f"- Date: {datetime.date.today().isoformat()}",
    f"- Client: MCP Inspector {VERSION} (CLI mode, `--strict` for tools/list); every step a fresh `wen mcp` over stdio",
    f"- Result: **{passed}/{len(results)}**",
    "",
    "| Step | Result | Detail |",
    "|---|---|---|",
]
for name, ok, detail in results:
    lines.append(f"| {name} | {'ok' if ok else 'FAIL'} | {detail.replace('|', '/')} |")
with open(options.report, "w", encoding="utf-8", newline="\n") as f:
    f.write("\n".join(lines) + "\n")
print("\n".join(lines))
sys.exit(0 if passed == len(results) else 1)
