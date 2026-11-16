"""The engine comparison (M7): the same eight tasks for an agent driving
Wish Engine and one driving Unity, each through its own MCP server.

    python tools/compare/run.py selftest
        Free. Wish Engine: every check fails on the start scene and passes
        after a reference solution. Unity: the scene reader on a small
        fixture. Nothing is downloaded, no model is called.

    python tools/compare/run.py run --engine wish|unity [--model sonnet] [--effort medium]
                                    [--tasks place_cube,delete] [--unity-project DIR] [--label NAME]
        Spends Claude usage: ask the user first. Each task runs `claude -p`
        with only that engine's MCP tools; success, turns, tokens, cost and
        tool errors go to evals/results/compare-<label>.md (+ .jsonl).

Disk: every Wish Engine project is deleted after its task. Unity uses one
project the user prepares (see tools/compare/README.md); each task gets its
own copy of the start scene, so no open scene is changed under the editor.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from tasks import NOTE, START, TASKS, unity_check, wen_do, wish_check, wish_start_ops  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
WEN = os.environ.get("WEN", r"C:\wish-build\target\release\wen.exe")
WORK = os.environ.get("COMPARE_WORK", r"C:\wish-build\compare")
UNITY_MCP = {"command": os.path.expanduser(r"~\.unity\relay\relay_win.exe"), "args": ["--mcp"]}

WISH_REFERENCE = {
    "place_cube": [{"op": "spawn", "id": "crate", "components": {
        "Transform": {"translation": [2, 0.5, 0]}, "Primitive": {"shape": "Cube"}}}],
    "paint_red": [{"op": "set", "id": "barrel", "component": "Material", "value": {"base_color": [0.9, 0.1, 0.1, 1]}}],
    "falling_box": [{"op": "set", "id": "box", "component": "RigidBody", "value": {"kind": "Dynamic"}}],
    "point_light": [{"op": "spawn", "id": "lamp", "components": {
        "Transform": {"translation": [0, 3, 0]}, "PointLight": {}}}],
    "camera_view": [{"op": "replace", "id": "camera", "component": "Transform",
                     "value": {"translation": [0, 1, 10], "euler_deg": [0, 180, 0]}}],
    "reparent": [{"op": "reparent", "id": "wheel", "parent": "car"},
                 {"op": "set", "id": "wheel", "component": "Transform", "value": {"translation": [0.5, -0.2, -0.6]}}],
    "delete": [{"op": "despawn", "id": "old"}],
    "scale_floor": [{"op": "set", "id": "floor", "component": "Transform", "value": {"scale": [40, 0.2, 40]}}],
}


def wish_project(task):
    root = os.path.join(WORK, f"wish-{task}")
    shutil.rmtree(root, ignore_errors=True)
    subprocess.run([WEN, "new", root, "--name", f"Compare {task}"], check=True, capture_output=True)
    answer = wen_do(WEN, root, [{"tool": "edit_apply", "args": {"ops": wish_start_ops()}}])[0]
    if not answer.get("ok"):
        raise RuntimeError(f"start scene: {answer}")
    return root


def selftest():
    failures = 0
    for task, _ in TASKS:
        root = wish_project(task)
        try:
            before, why_before = wish_check(task, WEN, root)
            answer = wen_do(WEN, root, [{"tool": "edit_apply", "args": {"ops": WISH_REFERENCE[task]}}])[0]
            if not answer.get("ok"):
                raise RuntimeError(f"reference: {answer}")
            after, why_after = wish_check(task, WEN, root)
        finally:
            shutil.rmtree(root, ignore_errors=True)
        ok = (not before) and after
        failures += not ok
        print(f"{'ok  ' if ok else 'FAIL'} wish  {task:12} before={before} ({why_before}); after={after} ({why_after})")
    failures += unity_selftest()
    print("selftest:", "passed" if failures == 0 else f"{failures} failed")
    return failures == 0


# A small Unity scene in the editor's text format: the start scene's objects
# plus what the reference solutions add, enough to exercise every check.
FIXTURE = """%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &10
GameObject:
  m_Component:
  - component: {fileID: 11}
  - component: {fileID: 12}
  m_Name: Main Camera
--- !u!4 &11
Transform:
  m_GameObject: {fileID: 10}
  m_LocalRotation: {x: 0, y: 1, z: 0, w: 0}
  m_LocalPosition: {x: 0, y: 1, z: -10}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Father: {fileID: 0}
--- !u!20 &12
Camera:
  m_GameObject: {fileID: 10}
  field of view: 60
--- !u!1 &20
GameObject:
  m_Component:
  - component: {fileID: 21}
  - component: {fileID: 22}
  - component: {fileID: 23}
  m_Name: crate
--- !u!4 &21
Transform:
  m_GameObject: {fileID: 20}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 2, y: 0.5, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Father: {fileID: 0}
--- !u!33 &22
MeshFilter:
  m_GameObject: {fileID: 20}
  m_Mesh: {fileID: 10202, guid: 0000000000000000e000000000000000, type: 0}
--- !u!65 &23
BoxCollider:
  m_GameObject: {fileID: 20}
--- !u!1 &30
GameObject:
  m_Component:
  - component: {fileID: 31}
  - component: {fileID: 32}
  m_Name: lamp
--- !u!4 &31
Transform:
  m_GameObject: {fileID: 30}
  m_LocalPosition: {x: 0, y: 3, z: 0}
  m_Father: {fileID: 0}
--- !u!108 &32
Light:
  m_GameObject: {fileID: 30}
  m_Type: 2
--- !u!1 &40
GameObject:
  m_Component:
  - component: {fileID: 41}
  m_Name: car
--- !u!4 &41
Transform:
  m_GameObject: {fileID: 40}
  m_LocalPosition: {x: 5, y: 0.5, z: 0}
  m_Father: {fileID: 0}
--- !u!1 &50
GameObject:
  m_Component:
  - component: {fileID: 51}
  m_Name: wheel
--- !u!4 &51
Transform:
  m_GameObject: {fileID: 50}
  m_LocalPosition: {x: 0.5, y: -0.2, z: 0.6}
  m_Father: {fileID: 41}
--- !u!1 &60
GameObject:
  m_Component:
  - component: {fileID: 61}
  m_Name: target
--- !u!4 &61
Transform:
  m_GameObject: {fileID: 60}
  m_LocalPosition: {x: 0, y: 0.5, z: -25}
  m_Father: {fileID: 0}
--- !u!1 &70
GameObject:
  m_Component:
  - component: {fileID: 71}
  - component: {fileID: 72}
  - component: {fileID: 73}
  m_Name: box
--- !u!4 &71
Transform:
  m_GameObject: {fileID: 70}
  m_LocalPosition: {x: 0, y: 4, z: 0}
  m_Father: {fileID: 0}
--- !u!65 &72
BoxCollider:
  m_GameObject: {fileID: 70}
--- !u!54 &73
Rigidbody:
  m_GameObject: {fileID: 70}
  m_UseGravity: 1
  m_IsKinematic: 0
--- !u!1 &80
GameObject:
  m_Component:
  - component: {fileID: 81}
  - component: {fileID: 82}
  m_Name: floor
--- !u!4 &81
Transform:
  m_GameObject: {fileID: 80}
  m_LocalPosition: {x: 0, y: -0.1, z: 0}
  m_LocalScale: {x: 40, y: 0.2, z: 40}
  m_Father: {fileID: 0}
--- !u!65 &82
BoxCollider:
  m_GameObject: {fileID: 80}
--- !u!1 &90
GameObject:
  m_Component:
  - component: {fileID: 91}
  - component: {fileID: 92}
  m_Name: barrel
--- !u!4 &91
Transform:
  m_GameObject: {fileID: 90}
  m_LocalPosition: {x: -3, y: 0.5, z: 0}
  m_Father: {fileID: 0}
--- !u!23 &92
MeshRenderer:
  m_GameObject: {fileID: 90}
  m_Materials:
  - {fileID: 2100000, guid: 5a1b2c3d4e5f60718293a4b5c6d7e8f9, type: 2}
"""

MATERIAL = """%YAML 1.1
--- !u!21 &2100000
Material:
  m_Name: Red
  m_SavedProperties:
    m_Colors:
    - _BaseColor: {r: 0.9, g: 0.1, b: 0.1, a: 1}
"""


def unity_selftest():
    failures = 0
    with tempfile.TemporaryDirectory() as project:
        os.makedirs(os.path.join(project, "Assets", "Scenes"))
        scene = os.path.join(project, "Assets", "Scenes", "Fixture.unity")
        open(scene, "w", encoding="utf-8").write(FIXTURE)
        material = os.path.join(project, "Assets", "Red.mat")
        open(material, "w", encoding="utf-8").write(MATERIAL)
        open(material + ".meta", "w", encoding="utf-8").write("fileFormatVersion: 2\nguid: 5a1b2c3d4e5f60718293a4b5c6d7e8f9\n")
        for task, _ in TASKS:
            passed, why = unity_check(task, scene, project)
            # The fixture has no 'old' but all the rest, so every task passes.
            failures += not passed
            print(f"{'ok  ' if passed else 'FAIL'} unity {task:12} ({why})")
        # And the checks can fail: an empty scene passes none.
        open(scene, "w", encoding="utf-8").write("%YAML 1.1\n")
        empty = [t for t, _ in TASKS if t != "delete" and unity_check(t, scene, project)[0]]
        if empty:
            failures += 1
            print("FAIL unity empty scene passes:", empty)
        else:
            print("ok   unity an empty scene passes no task (but 'delete', which only needs 'old' gone)")
    return failures


def stream_metrics(lines):
    """Turns, tokens, cost and tool errors from `claude -p --output-format stream-json`."""
    metrics = {"turns": 0, "input_tokens": 0, "output_tokens": 0, "cost_usd": 0.0, "tool_errors": 0, "tool_calls": 0}
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("type") == "assistant":
            for part in event.get("message", {}).get("content", []):
                if part.get("type") == "tool_use":
                    metrics["tool_calls"] += 1
        if event.get("type") == "user":
            for part in event.get("message", {}).get("content", []) or []:
                if isinstance(part, dict) and part.get("type") == "tool_result" and part.get("is_error"):
                    metrics["tool_errors"] += 1
        if event.get("type") == "result":
            usage = event.get("usage", {})
            metrics["turns"] = event.get("num_turns", 0)
            metrics["cost_usd"] = event.get("total_cost_usd", 0.0)
            metrics["input_tokens"] = (usage.get("input_tokens", 0) + usage.get("cache_read_input_tokens", 0)
                                       + usage.get("cache_creation_input_tokens", 0))
            metrics["output_tokens"] = usage.get("output_tokens", 0)
    return metrics


def claude(prompt, cwd, server, tools, model, effort):
    exe = "claude.cmd" if os.name == "nt" else "claude"
    with open(os.path.join(cwd, ".compare-mcp.json"), "w", encoding="utf-8") as f:
        json.dump({"mcpServers": {server: tools}}, f)
    done = subprocess.run(
        [exe, "-p", prompt, "--model", model, "--effort", effort, "--output-format", "stream-json", "--verbose",
         "--mcp-config", ".compare-mcp.json", "--strict-mcp-config", "--allowedTools", f"mcp__{server}",
         "--disallowedTools", "Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch"],
        cwd=cwd, capture_output=True, timeout=1800)
    return stream_metrics(done.stdout.decode("utf-8", "replace").splitlines())


def run(args):
    def flag(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    engine = flag("--engine")
    model, effort = flag("--model", "sonnet"), flag("--effort", "medium")
    chosen = flag("--tasks")
    tasks = [(t, p) for t, p in TASKS if not chosen or t in chosen.split(",")]
    label = flag("--label", f"{engine}-{model}-{effort}")
    os.makedirs(WORK, exist_ok=True)
    rows = []
    if engine == "unity":
        project = flag("--unity-project")
        base = os.path.join(project, "Assets", "Scenes", "CompareBase.unity")
        # The real file must fail every check first: proof the reader reads it.
        wrong = [t for t, _ in tasks if t != "delete" and unity_check(t, base, project)[0]]
        if wrong:
            sys.exit(f"the start scene already passes {wrong}: check CompareSetup")
    for task, prompt in tasks:
        if engine == "wish":
            root = wish_project(task)
            full = f"{prompt} {NOTE}"
            metrics = claude(full, root, "wish", {"command": WEN, "args": ["mcp", "--root", root]}, model, effort)
            passed, why = wish_check(task, WEN, root)
            shutil.rmtree(root, ignore_errors=True)
        else:
            scene_rel = f"Assets/Scenes/Compare_{task}.unity"
            shutil.copyfile(base, os.path.join(project, scene_rel))
            full = f"Unity'de {scene_rel} sahnesini aç. {prompt} {NOTE}"
            metrics = claude(full, WORK, "unity-mcp", UNITY_MCP, model, effort)
            passed, why = unity_check(task, os.path.join(project, scene_rel), project)
        row = {"engine": engine, "task": task, "passed": passed, "why": why, **metrics}
        rows.append(row)
        print(json.dumps(row, ensure_ascii=False))
    out = os.path.join(REPO, "evals", "results", f"compare-{label}")
    with open(out + ".jsonl", "w", encoding="utf-8") as f:
        f.writelines(json.dumps(r, ensure_ascii=False) + "\n" for r in rows)
    lines = [f"# Comparison: {engine}, {model} · {effort}", "",
             f"Passed **{sum(r['passed'] for r in rows)}/{len(rows)}**; cost {sum(r['cost_usd'] for r in rows):.2f} $", "",
             "| Task | Passed | Turns | Tool calls | Tool errors | Tokens in/out | Cost $ |", "|---|---|---|---|---|---|---|"]
    for r in rows:
        lines.append(f"| {r['task']} | {'yes' if r['passed'] else 'no'} | {r['turns']} | {r['tool_calls']} | "
                     f"{r['tool_errors']} | {r['input_tokens']}/{r['output_tokens']} | {r['cost_usd']:.3f} |")
    open(out + ".md", "w", encoding="utf-8").write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "selftest":
        sys.exit(0 if selftest() else 1)
    if len(sys.argv) > 1 and sys.argv[1] == "run":
        run(sys.argv[2:])
    else:
        print(__doc__)
