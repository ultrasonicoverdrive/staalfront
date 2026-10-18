"""Minimal low-poly geometry kit + GLB writer (glTF 2.0 binary).
Conventions: metres, Y up, front +X, ground Y = 0. Flat shading (exploded vertices).
"""
import json, math, struct
import numpy as np
import trimesh

# --------------------------------------------------------------------------- materials
MATERIALS = {
    # name: (rgb hex, metallic, roughness, emissive hex or None)
    "steel":      ("#A7AEB4", 0.35, 0.5, None),
    "dark":       ("#26282B", 0.0, 0.9, None),
    "lamp":       ("#FFE9A8", 0.0, 0.6, "#FFD36A"),
    "rust":       ("#8A5433", 0.1, 0.9, None),
    "burnt":      ("#4E4742", 0.0, 1.0, None),
    "charred":    ("#201D1B", 0.0, 1.0, None),
    "soil":       ("#4F4337", 0.0, 1.0, None),
    "ore":        ("#6A4A36", 0.15, 0.8, None),
    "rock":       ("#7D7A72", 0.0, 1.0, None),
    "rock_dark":  ("#5E5B55", 0.0, 1.0, None),
    "moss":       ("#6E7A45", 0.0, 1.0, None),
    "bark":       ("#5A4634", 0.0, 1.0, None),
    "bark_light": ("#8A7256", 0.0, 1.0, None),
    "pine":       ("#3C5A3E", 0.0, 1.0, None),
    "pine_light": ("#4F7049", 0.0, 1.0, None),
    "leaf":       ("#5F8A3F", 0.0, 1.0, None),
    "leaf_light": ("#7FA54D", 0.0, 1.0, None),
    "skin":       ("#D8B48E", 0.0, 0.9, None),
    "hair":       ("#3B3028", 0.0, 1.0, None),
    "coat":       ("#6E5B49", 0.0, 1.0, None),
    "trousers":   ("#4B5058", 0.0, 1.0, None),
    "cap":        ("#5B604B", 0.0, 1.0, None),
    "leather":    ("#7A5A3C", 0.0, 0.9, None),
    "overall":    ("#56687A", 0.0, 1.0, None),
    "hull_grey":  ("#5F6468", 0.2, 0.8, None),
    "water_dark": ("#2E3E48", 0.0, 1.0, None),
    "tarp":       ("#8A8468", 0.0, 1.0, None),
}

def hex2rgb(h):
    h = h.lstrip('#')
    return [int(h[i:i+2], 16) / 255.0 for i in (0, 2, 4)]

def srgb_to_linear(c):
    return [((v / 12.92) if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4) for v in c]

# --------------------------------------------------------------------------- transforms
def rot_x(a):
    a = math.radians(a); c, s = math.cos(a), math.sin(a)
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]])
def rot_y(a):
    a = math.radians(a); c, s = math.cos(a), math.sin(a)
    return np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])
def rot_z(a):
    a = math.radians(a); c, s = math.cos(a), math.sin(a)
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]])

def xf(V, t=(0, 0, 0), rx=0, ry=0, rz=0, s=1.0, pivot=(0, 0, 0)):
    """scale -> rotate (X, then Z, then Y) -> translate, about pivot."""
    V = np.asarray(V, float).copy()
    p = np.asarray(pivot, float)
    V -= p
    V *= np.asarray(s, float)
    R = rot_y(ry) @ rot_z(rz) @ rot_x(rx)
    V = V @ R.T
    V += p + np.asarray(t, float)
    return V

def jitter(V, amount, seed=0, keep_bottom=False):
    rng = np.random.default_rng(seed)
    V = np.asarray(V, float).copy()
    uniq, inv = np.unique(np.round(V, 6), axis=0, return_inverse=True)
    noise = rng.uniform(-amount, amount, size=uniq.shape)
    if keep_bottom:
        noise[uniq[:, 1] < 1e-4, 1] = 0
    return V + noise[inv.ravel()]

# --------------------------------------------------------------------------- primitives (return (V,F))
def box(sx, sy, sz, c=(0, 0, 0), taper=1.0, taper_x=None, taper_z=None, base_at=None, shear_x=0.0):
    """Axis-aligned box. taper scales the top face (x and z). base_at: y of the bottom face (overrides c[1])."""
    tx = taper if taper_x is None else taper_x
    tz = taper if taper_z is None else taper_z
    hx, hy, hz = sx / 2, sy / 2, sz / 2
    cx, cy, cz = c
    if base_at is not None:
        cy = base_at + hy
    V = np.array([
        [-hx, -hy, -hz], [hx, -hy, -hz], [hx, -hy, hz], [-hx, -hy, hz],
        [-hx * tx + shear_x, hy, -hz * tz], [hx * tx + shear_x, hy, -hz * tz],
        [hx * tx + shear_x, hy, hz * tz], [-hx * tx + shear_x, hy, hz * tz],
    ], float) + [cx, cy, cz]
    F = np.array([
        [0, 2, 1], [0, 3, 2],          # bottom
        [4, 5, 6], [4, 6, 7],          # top
        [0, 1, 5], [0, 5, 4],          # -z
        [1, 2, 6], [1, 6, 5],          # +x
        [2, 3, 7], [2, 7, 6],          # +z
        [3, 0, 4], [3, 4, 7],          # -x
    ])[:, [0, 2, 1]]
    return V, F

def cyl(r0, r1, h, n=8, c=(0, 0, 0), base_at=None, caps=True, rot=0.0):
    """Cylinder/cone frustum along Y. r1 = top radius (0 -> cone)."""
    cx, cy, cz = c
    y0 = base_at if base_at is not None else cy - h / 2
    y1 = y0 + h
    ang = np.linspace(0, 2 * math.pi, n, endpoint=False) + math.radians(rot)
    ring0 = np.stack([r0 * np.cos(ang), np.full(n, y0), r0 * np.sin(ang)], 1)
    V = [ring0]; F = []
    if r1 > 1e-6:
        ring1 = np.stack([r1 * np.cos(ang), np.full(n, y1), r1 * np.sin(ang)], 1)
        V.append(ring1)
        for i in range(n):
            j = (i + 1) % n
            F += [[i, j, n + j], [i, n + j, n + i]]
        if caps:
            ct = 2 * n; V.append(np.array([[0, y1, 0]]))
            for i in range(n):
                j = (i + 1) % n; F.append([n + i, n + j, ct])
            cb = 2 * n + 1; V.append(np.array([[0, y0, 0]]))
            for i in range(n):
                j = (i + 1) % n; F.append([j, i, cb])
    else:
        V.append(np.array([[0, y1, 0]]))
        for i in range(n):
            j = (i + 1) % n; F.append([i, j, n])
        if caps:
            cb = n + 1; V.append(np.array([[0, y0, 0]]))
            for i in range(n):
                j = (i + 1) % n; F.append([j, i, cb])
    V = np.vstack(V) + [cx, 0, cz]
    return V, np.array(F)[:, [0, 2, 1]]

def ico(r=1.0, sub=1, c=(0, 0, 0), s=(1, 1, 1)):
    m = trimesh.creation.icosphere(subdivisions=sub, radius=r)
    V = m.vertices * np.asarray(s, float) + np.asarray(c, float)
    return V, m.faces.copy()

def lathe(profile, n=8, c=(0, 0, 0), rot=0.0, caps="both"):
    """profile: list of (r, y) from bottom to top. Closed with caps when r>0 at ends."""
    ang = np.linspace(0, 2 * math.pi, n, endpoint=False) + math.radians(rot)
    rings = []
    for r, y in profile:
        rings.append(np.stack([r * np.cos(ang), np.full(n, y), r * np.sin(ang)], 1))
    V = list(rings); F = []
    k = len(profile)
    for a in range(k - 1):
        for i in range(n):
            j = (i + 1) % n
            F += [[a * n + i, a * n + j, (a + 1) * n + j], [a * n + i, (a + 1) * n + j, (a + 1) * n + i]]
    extra = []
    if profile[0][0] > 1e-6 and caps in ("both", "bottom"):
        cb = k * n; extra.append([0, profile[0][1], 0])
        for i in range(n):
            j = (i + 1) % n; F.append([j, i, cb])
    if profile[-1][0] > 1e-6 and caps in ("both", "top"):
        ct = k * n + len(extra); extra.append([0, profile[-1][1], 0])
        for i in range(n):
            j = (i + 1) % n; F.append([(k - 1) * n + i, (k - 1) * n + j, ct])
    V = np.vstack(V + ([np.array(extra)] if extra else [])) + np.asarray(c, float)
    return V, np.array(F)[:, [0, 2, 1]]

def prism(poly_xz, y0, y1, taper=1.0):
    """Extrude a convex polygon (list of (x,z), CCW seen from above) from y0 to y1."""
    P = np.asarray(poly_xz, float); n = len(P)
    cen = P.mean(0)
    bot = np.stack([P[:, 0], np.full(n, y0), P[:, 1]], 1)
    Pt = cen + (P - cen) * taper
    top = np.stack([Pt[:, 0], np.full(n, y1), Pt[:, 1]], 1)
    V = np.vstack([bot, top]); F = []
    for i in range(n):
        j = (i + 1) % n
        F += [[i, n + j, j], [i, n + i, n + j]]
    for i in range(1, n - 1):
        F.append([0, i, i + 1])
        F.append([n, n + i + 1, n + i])
    return fix_winding(V, np.array(F))

def merge(parts):
    """parts: list of (V,F) -> (V,F)."""
    Vs, Fs, off = [], [], 0
    for V, F in parts:
        Vs.append(V); Fs.append(np.asarray(F) + off); off += len(V)
    return np.vstack(Vs), np.vstack(Fs)

def fix_winding(V, F):
    """Make faces face outward relative to the mesh centroid (good enough for convex-ish blobs)."""
    V = np.asarray(V); F = np.asarray(F).copy()
    c = V.mean(0)
    for k, f in enumerate(F):
        a, b, d = V[f]
        nrm = np.cross(b - a, d - a)
        if np.dot(nrm, (a + b + d) / 3 - c) < 0:
            F[k] = [f[0], f[2], f[1]]
    return V, F

def face_normals(V, F):
    a, b, c = V[F[:, 0]], V[F[:, 1]], V[F[:, 2]]
    n = np.cross(b - a, c - a)
    l = np.linalg.norm(n, axis=1, keepdims=True); l[l == 0] = 1
    return n / l

# --------------------------------------------------------------------------- scene graph
class Node:
    def __init__(self, name, t=(0, 0, 0), rot_deg=(0, 0, 0)):
        self.name = name
        self.t = tuple(float(x) for x in t)
        self.rot = tuple(rot_deg)        # euler X,Y,Z degrees (applied X, then Z, then Y like xf)
        self.prims = {}                  # material -> list of (V,F)
        self.children = []

    def add(self, geom, material, **k):
        V, F = geom
        if k:
            V = xf(V, **k)
        self.prims.setdefault(material, []).append((np.asarray(V, float), np.asarray(F, int)))
        return self

    def add_split(self, geom, mat_a, mat_b, mask, **k):
        """Split faces of one geometry over two materials by a boolean face mask."""
        V, F = geom
        if k:
            V = xf(V, **k)
        self.prims.setdefault(mat_a, []).append((V, F[~mask]))
        if mask.any():
            self.prims.setdefault(mat_b, []).append((V, F[mask]))
        return self

    def child(self, node):
        self.children.append(node)
        return node

    def tri_count(self):
        n = sum(len(F) for lst in self.prims.values() for _, F in lst)
        return n + sum(c.tri_count() for c in self.children)

    def materials(self, acc=None):
        acc = set() if acc is None else acc
        acc.update(self.prims.keys())
        for c in self.children:
            c.materials(acc)
        return acc

def euler_to_quat(rx, ry, rz):
    R = rot_y(ry) @ rot_z(rz) @ rot_x(rx)
    m = R
    tr = m[0, 0] + m[1, 1] + m[2, 2]
    if tr > 0:
        S = math.sqrt(tr + 1.0) * 2; w = 0.25 * S
        x = (m[2, 1] - m[1, 2]) / S; y = (m[0, 2] - m[2, 0]) / S; z = (m[1, 0] - m[0, 1]) / S
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        S = math.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2]) * 2
        w = (m[2, 1] - m[1, 2]) / S; x = 0.25 * S; y = (m[0, 1] + m[1, 0]) / S; z = (m[0, 2] + m[2, 0]) / S
    elif m[1, 1] > m[2, 2]:
        S = math.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2]) * 2
        w = (m[0, 2] - m[2, 0]) / S; x = (m[0, 1] + m[1, 0]) / S; y = 0.25 * S; z = (m[1, 2] + m[2, 1]) / S
    else:
        S = math.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1]) * 2
        w = (m[1, 0] - m[0, 1]) / S; x = (m[0, 2] + m[2, 0]) / S; y = (m[1, 2] + m[2, 1]) / S; z = 0.25 * S
    return [x, y, z, w]

# --------------------------------------------------------------------------- GLB writer
class GLBWriter:
    def __init__(self):
        self.buf = bytearray()
        self.views, self.accs, self.meshes, self.nodes, self.mats = [], [], [], [], []
        self.mat_index = {}

    def _pad(self):
        while len(self.buf) % 4:
            self.buf.append(0)

    def _view(self, data, target):
        self._pad()
        off = len(self.buf); self.buf += data
        self.views.append({"buffer": 0, "byteOffset": off, "byteLength": len(data), "target": target})
        return len(self.views) - 1

    def _acc(self, arr, ctype, atype, target, minmax=False):
        data = arr.tobytes()
        v = self._view(data, target)
        a = {"bufferView": v, "componentType": ctype, "count": len(arr), "type": atype}
        if minmax:
            a["min"] = arr.min(0).tolist(); a["max"] = arr.max(0).tolist()
        self.accs.append(a)
        return len(self.accs) - 1

    def material(self, name):
        if name in self.mat_index:
            return self.mat_index[name]
        rgb, met, rough, emis = MATERIALS[name]
        m = {"name": name, "doubleSided": False,
             "pbrMetallicRoughness": {"baseColorFactor": srgb_to_linear(hex2rgb(rgb)) + [1.0],
                                      "metallicFactor": met, "roughnessFactor": rough}}
        if emis:
            m["emissiveFactor"] = srgb_to_linear(hex2rgb(emis))
        self.mats.append(m)
        self.mat_index[name] = len(self.mats) - 1
        return self.mat_index[name]

    def _mesh(self, node):
        prims = []
        for mat, lst in node.prims.items():
            V, F = merge(lst)
            if len(F) == 0:
                continue
            # explode for flat shading
            Ve = V[F].reshape(-1, 3).astype(np.float32)
            N = face_normals(V, F)
            Ne = np.repeat(N, 3, axis=0).astype(np.float32)
            idx = np.arange(len(Ve), dtype=np.uint32)
            prims.append({"attributes": {"POSITION": self._acc(Ve, 5126, "VEC3", 34962, True),
                                         "NORMAL": self._acc(Ne, 5126, "VEC3", 34962)},
                          "indices": self._acc(idx, 5125, "SCALAR", 34963),
                          "material": self.material(mat), "mode": 4})
        if not prims:
            return None
        self.meshes.append({"name": node.name, "primitives": prims})
        return len(self.meshes) - 1

    def node(self, n):
        d = {"name": n.name}
        if any(abs(x) > 1e-9 for x in n.t):
            d["translation"] = list(n.t)
        if any(abs(x) > 1e-9 for x in n.rot):
            d["rotation"] = euler_to_quat(*n.rot)
        mi = self._mesh(n)
        if mi is not None:
            d["mesh"] = mi
        ch = [self.node(c) for c in n.children]
        if ch:
            d["children"] = ch
        self.nodes.append(d)
        return len(self.nodes) - 1

    def write(self, path, roots, extras=None):
        root_ids = [self.node(r) for r in roots]
        self._pad()
        gltf = {"asset": {"version": "2.0", "generator": "staalfront-glbkit"},
                "scene": 0, "scenes": [{"name": "scene", "nodes": root_ids}],
                "nodes": self.nodes, "meshes": self.meshes, "materials": self.mats,
                "accessors": self.accs, "bufferViews": self.views,
                "buffers": [{"byteLength": len(self.buf)}]}
        if extras:
            gltf["asset"]["extras"] = extras
        js = json.dumps(gltf, separators=(",", ":")).encode()
        while len(js) % 4:
            js += b" "
        total = 12 + 8 + len(js) + 8 + len(self.buf)
        with open(path, "wb") as f:
            f.write(struct.pack("<III", 0x46546C67, 2, total))
            f.write(struct.pack("<II", len(js), 0x4E4F534A)); f.write(js)
            f.write(struct.pack("<II", len(self.buf), 0x004E4942)); f.write(bytes(self.buf))

def bounds(roots):
    allv = []
    def walk(n, T):
        R = rot_y(n.rot[1]) @ rot_z(n.rot[2]) @ rot_x(n.rot[0])
        for lst in n.prims.values():
            for V, F in lst:
                allv.append((V @ R.T) + np.asarray(n.t) + T)
        for c in n.children:
            walk(c, T + np.asarray(n.t))
    walk(roots[0], np.zeros(3))
    if not allv:
        return None
    A = np.vstack(allv)
    return A.min(0).round(3).tolist(), A.max(0).round(3).tolist()
