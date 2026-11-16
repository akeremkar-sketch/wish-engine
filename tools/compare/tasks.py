"""The comparison tasks (M7, exit criterion 4): eight engine-neutral scene
tasks, given in the same words to an agent driving Wish Engine and one
driving Unity, each over its engine's own MCP server. Both engines start
from the same scene (Unity is left-handed, so its Z is mirrored); each check
reads what the engine saved or does.

Wish Engine checks go through `wen do`; Unity checks read the saved scene
file (`unity_scene.py`)."""

import json
import subprocess

from unity_scene import (BOX_COLLIDER, BUILTIN_CUBE, CAMERA, COLLIDERS, LIGHT, MESH_FILTER,
                         RIGIDBODY, Scene, in_view)

NOTE = ("Sahne motorda açık; yalnızca motorun MCP araçlarını kullan. İşin bitince sahneyi kaydet. "
        "Koordinatlar metre; +Y yukarı.")

TASKS = [
    ("place_cube", "'crate' adında 1 metrelik bir küp ekle; merkezi (2, 0.5, 0) noktasında olsun."),
    ("paint_red", "'barrel' nesnesini kırmızı yap."),
    ("falling_box", "'box' nesnesi oyun başlayınca yerçekimiyle düşsün ve 'floor' zemininin üstünde dursun."),
    ("point_light", "(0, 3, 0) noktasına 'lamp' adında bir nokta ışığı (point light) ekle."),
    ("camera_view", "Kamera 'target' nesnesini görmüyor. Kamerayı ayarla ki 'target' görüntüde olsun; 'target'ı oynatma."),
    ("reparent", "'wheel' nesnesini 'car' nesnesinin çocuğu yap; 'wheel' dünyadaki yerinden oynamasın."),
    ("delete", "'old' nesnesini sil; başka bir şeye dokunma."),
    ("scale_floor", "'floor' zeminini X ve Z yönünde iki katına büyüt; kalınlığı (Y) aynı kalsın."),
]

# The shared starting scene, in Wish Engine coordinates (right-handed, the
# camera looks along -Z). Unity's copy (CompareSetup.cs) mirrors Z.
START = {
    "floor": {"at": [0, -0.1, 0], "scale": [20, 0.2, 20], "shape": "Cube", "collider": True},
    "barrel": {"at": [-3, 0.5, 0], "shape": "Cube"},
    "box": {"at": [0, 4, 0], "shape": "Cube", "collider": True},
    "car": {"at": [5, 0.5, 0], "shape": "Cube"},
    "wheel": {"at": [5.5, 0.3, -0.6], "scale": [0.6, 0.6, 0.6], "shape": "Sphere"},
    "old": {"at": [-6, 0.5, -3], "shape": "Cube"},
    "target": {"at": [0, 0.5, 25], "shape": "Sphere"},
}
CAMERA_AT = [0, 1, 10]


def wish_start_ops():
    ops = [{"op": "despawn", "id": "ground"},
           {"op": "replace", "id": "camera", "component": "Transform", "value": {"translation": CAMERA_AT}}]
    for name, spec in START.items():
        components = {"Transform": {"translation": spec["at"], "scale": spec.get("scale", [1, 1, 1])},
                      "Primitive": {"shape": spec["shape"]}}
        if spec.get("collider"):
            components["Collider"] = {"shape": "Box"}
        ops.append({"op": "spawn", "id": name, "components": components})
    return ops


# --------------------------------------------------------------------------
# Wish Engine.

def wen_do(wen, root, calls):
    lines = "".join(json.dumps(c) + "\n" for c in calls)
    done = subprocess.run([wen, "do", "-", "--root", root], input=lines.encode("utf-8"),
                          capture_output=True, timeout=600)
    return [json.loads(l) for l in done.stdout.decode("utf-8").splitlines() if l.strip()]


def wish_entity(wen, root, name, session=None):
    args = {"ids": [name]}
    if session:
        args["session"] = session
    answer = wen_do(wen, root, [{"tool": "entity_get", "args": args}])[0]
    if not answer.get("ok"):
        return None
    return answer["entities"][0]


def wish_world(wen, root, name):
    answer = wen_do(wen, root, [{"tool": "entity_get", "args": {"ids": [name], "components": ["GlobalTransform"]}}])[0]
    m = answer["entities"][0]["components"]["GlobalTransform"]["matrix"]
    return m[12], m[13], m[14]


def near(a, b, tolerance=0.05):
    return all(abs(x - y) <= tolerance for x, y in zip(a, b))


def wish_check(task, wen, root):
    """(passed, reason) for one task in a Wish Engine project."""
    try:
        return _wish_check(task, wen, root)
    except (AttributeError, KeyError, IndexError, TypeError) as error:
        return False, f"not there ({error!r})"


def _wish_check(task, wen, root):
    if task == "place_cube":
        e = wish_entity(wen, root, "crate")
        if e is None or e["components"].get("Primitive", {}).get("shape") != "Cube":
            return False, "no cube 'crate'"
        return near(wish_world(wen, root, "crate"), (2, 0.5, 0)), "position"
    if task == "paint_red":
        c = (wish_entity(wen, root, "barrel") or {}).get("components", {}).get("Material", {}).get("base_color", [1, 1, 1])
        return c[0] > 0.6 and c[1] < 0.3 and c[2] < 0.3, f"colour {c}"
    if task == "falling_box":
        answers = wen_do(wen, root, [{"tool": "sim_start", "args": {}},
                                     {"tool": "sim_step", "args": {"session": "s1", "frames": 150}},
                                     {"tool": "entity_get", "args": {"session": "s1", "ids": ["box"]}}])
        y = answers[2]["entities"][0]["components"]["Transform"]["translation"][1]
        return 0.4 < y < 0.6, f"box at y = {y:.2f} after 2.5 s"
    if task == "point_light":
        e = wish_entity(wen, root, "lamp")
        if e is None or "PointLight" not in e["components"]:
            return False, "no point light 'lamp'"
        return near(wish_world(wen, root, "lamp"), (0, 3, 0)), "position"
    if task == "camera_view":
        if not near(wish_world(wen, root, "target"), START["target"]["at"]):
            return False, "target moved"
        seen = wen_do(wen, root, [{"tool": "describe_view", "args": {"width": 640, "height": 360}}])[0]
        return any(v["entity"] == "target" for v in seen.get("visible", [])), "target on screen"
    if task == "reparent":
        e = wish_entity(wen, root, "wheel")
        if e is None or e.get("parent") != "car":
            return False, f"wheel's parent is {e and e.get('parent')}"
        return near(wish_world(wen, root, "wheel"), START["wheel"]["at"]), "world position kept"
    if task == "delete":
        if wish_entity(wen, root, "old") is not None:
            return False, "'old' is still there"
        rest = [n for n in START if n != "old" and wish_entity(wen, root, n) is None]
        return not rest, f"missing: {rest}"
    if task == "scale_floor":
        s = wish_entity(wen, root, "floor")["components"]["Transform"].get("scale", [1, 1, 1])
        return near(s, (40, 0.2, 40), 0.01), f"scale {s}"
    return False, "unknown task"


# --------------------------------------------------------------------------
# Unity (Z mirrored).

def mirror(p):
    return (p[0], p[1], -p[2])


def unity_check(task, scene_path, project_root):
    """(passed, reason) for one task in a saved Unity scene."""
    try:
        return _unity_check(task, scene_path, project_root)
    except (AttributeError, KeyError, IndexError, TypeError) as error:
        return False, f"not there ({error!r})"


def _unity_check(task, scene_path, project_root):
    scene = Scene(scene_path)
    obj = scene.find
    if task == "place_cube":
        o = obj("crate")
        if o is None:
            return False, "no 'crate'"
        mesh = scene.component(o, MESH_FILTER)
        cube = mesh is not None and mesh.ref("m_Mesh")[0] == BUILTIN_CUBE
        return cube and near(scene.world_position(o), (2, 0.5, 0)), "cube at (2, 0.5, 0)"
    if task == "paint_red":
        colors = scene.material_colors(obj("barrel"), project_root) if obj("barrel") else []
        return any(c[0] > 0.6 and c[1] < 0.3 and c[2] < 0.3 for c in colors), f"colours {colors}"
    if task == "falling_box":
        box, floor = obj("box"), obj("floor")
        body = box and scene.component(box, RIGIDBODY)
        if body is None:
            return False, "'box' has no Rigidbody"
        # Falls: gravity, not kinematic, Y position not frozen (constraint bit 4).
        falls = (body.number("m_UseGravity", 1) == 1 and body.number("m_IsKinematic", 0) == 0
                 and int(body.number("m_Constraints", 0)) & 4 == 0)

        def solid(o):
            colliders = [c for c in scene.components(o) if c.cls in COLLIDERS]
            return any(c.number("m_IsTrigger", 0) == 0 and c.number("m_Enabled", 1) == 1 for c in colliders)
        lands = solid(box) and floor is not None and solid(floor)
        return falls and lands, "rigidbody with gravity, solid colliders on box and floor (static check)"
    if task == "point_light":
        o = obj("lamp")
        light = o and scene.component(o, LIGHT)
        if light is None:
            return False, "no light 'lamp'"
        return light.number("m_Type", -1) == 2 and near(scene.world_position(o), (0, 3, 0)), "point light at (0, 3, 0)"
    if task == "camera_view":
        target = obj("target")
        if target is None or not near(scene.world_position(target), mirror(START["target"]["at"])):
            return False, "target moved or gone"
        cameras = [o for o in scene.objects() if scene.component(o, CAMERA)]
        return any(in_view(scene, c, scene.world_position(target)) for c in cameras), "target inside a camera's view (geometry)"
    if task == "reparent":
        o = obj("wheel")
        if o is None or scene.parent_name(o) != "car":
            return False, "wheel is not under car"
        return near(scene.world_position(o), mirror(START["wheel"]["at"])), "world position kept"
    if task == "delete":
        rest = [n for n in START if n != "old" and obj(n) is None]
        return obj("old") is None and not rest, f"old gone, missing: {rest}"
    if task == "scale_floor":
        s = scene.transform(obj("floor")).vector("m_LocalScale")
        return near((s["x"], s["y"], s["z"]), (40, 0.2, 40), 0.01), f"scale {s}"
    return False, "unknown task"

