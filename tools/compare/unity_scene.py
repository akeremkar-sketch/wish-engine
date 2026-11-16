"""Reads a saved Unity scene (.unity, text YAML) far enough to check the
comparison tasks: game objects by name, their transforms, parents and
components (camera, light, rigidbody, collider, mesh, materials).

Unity's YAML is a list of documents headed `--- !u!<class> &<fileID>`; this
reads the few keys the checks need with plain line parsing (no YAML library,
nothing to install)."""

import math
import os
import re

# Unity class ids.
GAME_OBJECT, TRANSFORM, CAMERA, MESH_RENDERER, MESH_FILTER = 1, 4, 20, 23, 33
RIGIDBODY, BOX_COLLIDER, LIGHT = 54, 65, 108
COLLIDERS = {64, 65, 135, 136}  # mesh, box, sphere, capsule
BUILTIN_CUBE = "10202"

HEADER = re.compile(r"^--- !u!(\d+) &(-?\d+)")
VECTOR = re.compile(r"(\w+): (-?[\d.eE+-]+)")
REF = re.compile(r"fileID: (-?\d+)(?:, guid: (\w+))?")


class Doc:
    def __init__(self, cls, file_id):
        self.cls, self.id, self.lines = cls, file_id, []

    def value(self, key):
        for line in self.lines:
            stripped = line.strip()
            if stripped.startswith(key + ":"):
                return stripped[len(key) + 1:].strip()
        return None

    def vector(self, key):
        raw = self.value(key)
        if raw is None:
            return None
        return {k: float(v) for k, v in VECTOR.findall(raw)}

    def ref(self, key):
        raw = self.value(key)
        match = REF.search(raw or "")
        return (match.group(1), match.group(2)) if match else (None, None)

    def number(self, key, default=0.0):
        raw = self.value(key)
        try:
            return float(raw)
        except (TypeError, ValueError):
            return default

    def list_refs(self, key):
        """`key:` followed by `- {fileID: .., guid: ..}` or `- component: {fileID: ..}` lines."""
        out, inside = [], False
        for line in self.lines:
            stripped = line.strip()
            if stripped.startswith(key + ":"):
                inside = True
                continue
            if inside:
                if not stripped.startswith("-"):
                    break
                match = REF.search(stripped)
                if match:
                    out.append((match.group(1), match.group(2)))
        return out


class Scene:
    def __init__(self, path):
        self.docs = {}
        current = None
        with open(path, encoding="utf-8") as f:
            for line in f:
                head = HEADER.match(line)
                if head:
                    current = Doc(int(head.group(1)), head.group(2))
                    self.docs[current.id] = current
                elif current is not None:
                    current.lines.append(line.rstrip("\n"))
        self.assets = os.path.join(os.path.dirname(path), "..")

    def objects(self):
        return [d for d in self.docs.values() if d.cls == GAME_OBJECT]

    def find(self, name):
        for d in self.objects():
            if d.value("m_Name") == name:
                return d
        return None

    def components(self, obj):
        return [self.docs[i] for i, _ in obj.list_refs("m_Component") if i in self.docs]

    def component(self, obj, cls):
        for c in self.components(obj):
            if c.cls == cls:
                return c
        return None

    def has(self, obj, classes):
        return any(c.cls in classes for c in self.components(obj))

    def transform(self, obj):
        return self.component(obj, TRANSFORM)

    def parent_name(self, obj):
        father, _ = self.transform(obj).ref("m_Father")
        if father in (None, "0") or father not in self.docs:
            return None
        parent_obj, _ = self.docs[father].ref("m_GameObject")
        return self.docs[parent_obj].value("m_Name") if parent_obj in self.docs else None

    def world_position(self, obj):
        """Position in the world (rotation of parents ignored; the tasks keep parents unrotated)."""
        x = y = z = 0.0
        t = self.transform(obj)
        while t is not None:
            p = t.vector("m_LocalPosition") or {}
            x, y, z = x + p.get("x", 0), y + p.get("y", 0), z + p.get("z", 0)
            father, _ = t.ref("m_Father")
            t = self.docs.get(father) if father not in (None, "0") else None
        return (x, y, z)

    def material_colors(self, obj, project_root):
        """The base colours of the materials on an object's renderer."""
        renderer = self.component(obj, MESH_RENDERER)
        if renderer is None:
            return []
        colors = []
        for file_id, guid in renderer.list_refs("m_Materials"):
            if guid:
                mat = find_asset(project_root, guid)
                if mat is None:
                    continue
                text = open(mat, encoding="utf-8").read()
            else:
                # A material kept inside the scene (not saved as an asset).
                inline = self.docs.get(file_id)
                if inline is None or inline.cls != 21:
                    continue
                text = "\n".join(inline.lines)
            for key in ("_BaseColor", "_Color"):
                match = re.search(r"- " + key + r": \{r: ([\d.e-]+), g: ([\d.e-]+), b: ([\d.e-]+)", text)
                if match:
                    colors.append(tuple(float(v) for v in match.groups()))
                    break
        return colors


def find_asset(project_root, guid):
    for folder, _, files in os.walk(os.path.join(project_root, "Assets")):
        for name in files:
            if name.endswith(".meta"):
                path = os.path.join(folder, name)
                with open(path, encoding="utf-8", errors="replace") as f:
                    if f"guid: {guid}" in f.read(400):
                        return path[: -len(".meta")]
    return None


def rotate(rotation, v):
    """A vector turned by a quaternion {x,y,z,w}."""
    x, y, z, w = (rotation.get(k, 0.0) for k in "xyzw")
    if x == y == z == 0 and w == 0:
        w = 1.0
    # v + 2w (q x v) + 2 q x (q x v)
    qv = (x, y, z)

    def cross(a, b):
        return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])
    t = tuple(2 * c for c in cross(qv, v))
    u = cross(qv, t)
    return tuple(v[i] + w * t[i] + u[i] for i in range(3))


def forward(rotation):
    """Unity looks along +Z: the rotated +Z axis."""
    return rotate(rotation, (0.0, 0.0, 1.0))


def in_view(scene, camera_obj, point, aspect=16 / 9):
    """Inside the camera's view: the vertical half-angle is fov/2, the
    horizontal one follows from the aspect (16:9, as Wish Engine's check
    renders). Occlusion is not looked at."""
    t = scene.transform(camera_obj)
    rotation = t.vector("m_LocalRotation") or {}
    eye = scene.world_position(camera_obj)
    to = tuple(p - e for p, e in zip(point, eye))
    ahead = sum(a * b for a, b in zip(rotate(rotation, (0, 0, 1)), to))
    if ahead <= 0.3:  # Unity's default near plane
        return False
    side = sum(a * b for a, b in zip(rotate(rotation, (1, 0, 0)), to))
    up = sum(a * b for a, b in zip(rotate(rotation, (0, 1, 0)), to))
    fov = scene.component(camera_obj, CAMERA).number("field of view", 60.0)
    half_v = math.radians(fov) / 2
    half_h = math.atan(math.tan(half_v) * aspect)
    return abs(math.atan2(up, ahead)) < half_v and abs(math.atan2(side, ahead)) < half_h
