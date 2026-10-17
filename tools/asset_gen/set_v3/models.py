"""Staalfront neutral asset set v2 — procedural low-poly models per art brief 17-10-2026."""
import math, json, sys, os
import numpy as np
sys.path.insert(0, os.path.dirname(__file__))
from glbkit import *
from figure_v3 import figure as figure_v3
from figure_v4 import build_unit, UNITS

rng_global = np.random.default_rng(7)

# ============================================================================ people
def figure(d, kind):
    """Rigid-part human figure. Nodes: torso, head, arm_l, arm_r, leg_l, leg_r. 0.6x0.6x1.7, front +X."""
    civ = kind == "civilian"
    root = Node(kind)
    top = "coat" if civ else "overall"
    legs = "trousers" if civ else "overall"
    cap_mat = "cap" if civ else "hair"

    # ---- legs (z = -0.1 left? Right side is +Z per brief, so leg_r at +Z)
    for side, z in (("l", -0.105), ("r", 0.105)):
        n = root.child(Node(f"leg_{side}"))
        if d == 2:
            if side == "l":
                n.add(box(0.26, 0.86, 0.38, base_at=0.0, c=(0.0, 0, 0)), legs)
            continue
        # shoe
        if d == 0:
            n.add(box(0.29, 0.07, 0.13, base_at=0.0, c=(0.04, 0, z), taper_x=0.85), "dark")
            n.add(box(0.15, 0.40, 0.14, base_at=0.07, c=(0.0, 0, z), taper=1.12), legs)
        else:
            n.add(box(0.16, 0.47, 0.14, base_at=0.0, c=(0.0, 0, z), taper=1.1), legs)
        n.add(box(0.17, 0.42, 0.16, base_at=0.46, c=(0.0, 0, z), taper=1.05), legs)
        if civ and d == 0:
            # trouser cuff
            n.add(box(0.17, 0.05, 0.16, base_at=0.07, c=(0.0, 0, z)), "dark")

    # ---- torso
    t = root.child(Node("torso"))
    if d == 2:
        t.add(box(0.30, 0.62, 0.42, base_at=0.86, taper_z=1.12, taper_x=0.9), top)
    else:
        # hips / belt
        if d == 0:
            t.add(box(0.26, 0.10, 0.36, base_at=0.86), "dark" if civ else "leather")
        # chest, widening to shoulders
        t.add(box(0.26, 0.56 if d else 0.46, 0.36, base_at=0.86 if d else 0.96, taper_z=1.22, taper_x=0.95), top)
        if civ:
            # coat skirt hanging over the hips
            t.add(box(0.30, 0.18, 0.40, base_at=0.74, taper=0.9), top)
            if d == 0:
                # lapels / collar
                t.add(box(0.06, 0.16, 0.26, base_at=1.27, c=(0.14, 0, 0)), "dark")
                # scarf wrap at the neck
                t.add(cyl(0.11, 0.10, 0.07, n=8, base_at=1.41), "trousers")
        else:
            # bib + shoulder straps of the overall, chest pocket, tool belt with pouches
            t.add(box(0.05, 0.20, 0.24, base_at=1.18, c=(0.14, 0, 0)), "overall")
            t.add(box(0.28, 0.06, 0.40, base_at=0.92), "leather")
            if d == 0:
                t.add(box(0.10, 0.12, 0.10, base_at=0.86, c=(0.08, 0, -0.17)), "leather")
                t.add(box(0.10, 0.12, 0.10, base_at=0.86, c=(0.08, 0, 0.17)), "leather")
                t.add(box(0.04, 0.10, 0.12, base_at=1.30, c=(0.15, 0, -0.10)), "overall")
                # rolled collar of a shirt
                t.add(cyl(0.10, 0.09, 0.05, n=8, base_at=1.41), "skin")

    # ---- head
    h = root.child(Node("head"))
    if d == 2:
        h.add(box(0.20, 0.26, 0.20, base_at=1.46), "skin")
    else:
        sub = 1 if d == 0 else 0
        h.add(cyl(0.06, 0.06, 0.06, n=6, base_at=1.42), "skin")             # neck
        h.add(ico(0.115, sub, c=(0.0, 1.59, 0), s=(1.0, 1.15, 0.95)), "skin")
        # nose / brow bump so the face direction reads
        if d == 0:
            h.add(box(0.05, 0.05, 0.04, base_at=1.56, c=(0.12, 0, 0)), "skin")
        if civ:
            # flat cap: shallow dome + brim forward
            h.add(cyl(0.125, 0.09, 0.07, n=10 if d == 0 else 7, base_at=1.66), cap_mat)
            if d == 0:
                h.add(box(0.11, 0.025, 0.20, base_at=1.665, c=(0.13, 0, 0), taper_z=0.8), cap_mat)
        else:
            # hair cap + wool beanie-ish band
            h.add(cyl(0.12, 0.10, 0.06, n=10 if d == 0 else 7, base_at=1.65), "hair")
            if d == 0:
                h.add(cyl(0.125, 0.125, 0.035, n=10, base_at=1.62), "overall")

    # ---- arms (shoulder pivots at y 1.36, z ±0.22)
    for side, z in (("l", -0.22), ("r", 0.22)):
        a = root.child(Node(f"arm_{side}"))
        sz = 1 if z > 0 else -1
        if d == 2:
            if side == "l":
                a.add(box(0.14, 0.62, 0.12, base_at=0.78, c=(0.02, 0, z + sz * 0.02)), top)
            elif civ:
                a.add(box(0.46, 0.34, 0.13, base_at=0.36, c=(0.06, 0, z + sz * 0.11)), "leather")
            else:
                a.add(box(0.44, 0.22, 0.18, base_at=0.44, c=(0.06, 0, z + sz * 0.13)), "steel")
            continue
        # upper arm hanging, slightly outward
        a.add(box(0.13, 0.30, 0.12, base_at=1.08, c=(0.0, 0, z + sz * 0.01), taper=1.1), top)
        sleeve = top if civ else "skin"   # technician: rolled sleeves
        a.add(box(0.12, 0.26, 0.11, base_at=0.83, c=(0.02, 0, z + sz * 0.03)), sleeve)
        a.add(box(0.11, 0.09, 0.09, base_at=0.75, c=(0.03, 0, z + sz * 0.03)), "skin")
        if civ and side == "r":
            # suitcase in the right hand
            a.add(box(0.46, 0.34, 0.13, base_at=0.36, c=(0.06, 0, z + sz * 0.11)), "leather")
            if d == 0:
                a.add(box(0.48, 0.04, 0.15, base_at=0.51, c=(0.06, 0, z + sz * 0.11)), "dark")
                a.add(box(0.48, 0.04, 0.15, base_at=0.36, c=(0.06, 0, z + sz * 0.11)), "dark")
                a.add(box(0.12, 0.05, 0.03, base_at=0.70, c=(0.06, 0, z + sz * 0.11)), "dark")
        if (not civ) and side == "r":
            # steel toolbox in the right hand
            a.add(box(0.44, 0.22, 0.18, base_at=0.44, c=(0.06, 0, z + sz * 0.13)), "steel")
            if d == 0:
                a.add(box(0.46, 0.03, 0.20, base_at=0.66, c=(0.06, 0, z + sz * 0.13)), "dark")
                a.add(box(0.14, 0.06, 0.03, base_at=0.69, c=(0.06, 0, z + sz * 0.13)), "dark")
                a.add(box(0.03, 0.22, 0.19, base_at=0.44, c=(0.0, 0, z + sz * 0.13)), "dark")
        if (not civ) and side == "l" and d == 0:
            # wrench in the left hand
            a.add(box(0.36, 0.035, 0.035, base_at=0.78, c=(0.12, 0, z + sz * 0.03)), "steel")
            a.add(box(0.07, 0.07, 0.035, base_at=0.765, c=(0.32, 0, z + sz * 0.03)), "steel")
    return root

# ============================================================================ ore field
def ore_chunk(seed, r, sub, metal_frac=0.28):
    rng = np.random.default_rng(seed)
    V, F = ico(r, sub)
    V = jitter(V, r * 0.28, seed=seed)
    # angular: push vertices onto a few planes by rounding
    V = np.round(V / (r * 0.22)) * (r * 0.22) * 0.5 + V * 0.5
    V[:, 1] *= 0.62
    V = xf(V, ry=rng.uniform(0, 360), rz=rng.uniform(-15, 15))
    V, F = fix_winding(V, F)
    N = face_normals(V, F)
    # metallic facets: a band of faces with similar normal direction + random pick
    k = rng.normal(size=3); k /= np.linalg.norm(k)
    score = N @ k + rng.uniform(-0.6, 0.6, size=len(F))
    mask = score > np.quantile(score, 1 - metal_frac)
    return (V, F), mask

def ore_field(d, stage):
    """stage 1 = almost depleted ... stage 4 = full. One cell (4x4 m), origin cell centre."""
    root = Node(f"ore_field_{stage}")
    counts = {1: 4, 2: 7, 3: 10, 4: 14}[stage]
    if d == 1:
        counts = max(3, int(counts * 0.7))
    if d == 2:
        counts = max(2, int(counts * 0.4))
    sub = 1 if d == 0 else 0
    rng = np.random.default_rng(100 + stage)
    soil = root.child(Node("soil"))
    chunks = root.child(Node("ore"))
    # disturbed-soil patch, sized with the stage
    pr = 0.9 + 0.3 * stage
    V, F = cyl(pr, pr * 0.9, 0.08, n=12 if d == 0 else 8, base_at=-0.03)
    V = jitter(V, 0.18, seed=stage, keep_bottom=True); V[:, 1] = np.clip(V[:, 1], -0.03, 0.06)
    soil.add((V, F), "soil")
    # chunk placement: poisson-ish ring + centre
    cand = []
    for gx in np.arange(-1.5, 1.51, 0.6):
        for gz in np.arange(-1.5, 1.51, 0.6):
            cand.append(np.array([gx, gz]) + rng.uniform(-0.22, 0.22, 2))
    cand.sort(key=lambda p: np.linalg.norm(p) + rng.uniform(0, 0.5))
    pts = [p for p in cand if np.linalg.norm(p, ord=np.inf) < 1.6][:counts]
    for i, (x, z) in enumerate(pts):
        r = rng.uniform(0.28, 0.52) * (0.85 + 0.08 * stage)
        if i == 0 and stage >= 3:
            r *= 1.35
        g, mask = ore_chunk(1000 * stage + i, r, sub)
        chunks.add_split(g, "ore", "steel", mask, t=(x, r * 0.45, z))
    return root

# ============================================================================ trees
def tree_pine(d, variant="standing"):
    root = Node("tree_pine")
    H = 12.0
    trunk = root.child(Node("trunk"))
    fol = root.child(Node("foliage"))
    n = {0: 9, 1: 7, 2: 5}[d]
    burnt = variant == "burnt"
    if burnt:
        trunk.add(lathe([(0.45, 0), (0.30, 1.2), (0.22, 5.0), (0.10, 9.5), (0.0, 10.2)], n=n), "charred")
        # a few bare branch stubs
        if d < 2:
            for i, (y, ang) in enumerate(((3.5, 20), (5.2, 140), (6.8, 260), (8.0, 60))):
                trunk.add(cyl(0.07, 0.03, 1.6 if d == 0 else 1.2, n=4, base_at=0), "charred",
                          t=(0, y, 0), rx=0, rz=-70, ry=ang)
        return root
    trunk.add(lathe([(0.42, 0), (0.28, 1.0), (0.22, 3.0), (0.12, 8.0), (0.06, 10.5)], n=n), "bark")
    if d == 0:
        # root flare
        for ang in (0, 120, 240):
            trunk.add(box(0.55, 0.35, 0.22, base_at=0, c=(0.35, 0, 0), taper_x=0.4), "bark", ry=ang + 25)
    tiers = [(2.4, 2.6, 2.9), (4.6, 2.2, 2.8), (6.6, 1.8, 2.7), (8.5, 1.35, 2.6), (10.2, 0.9, 2.2)]
    if d == 1:
        tiers = [(2.4, 2.6, 3.4), (5.4, 2.0, 3.4), (8.3, 1.3, 3.8)]
    if d == 2:
        tiers = [(2.4, 2.6, 9.4)]
    for i, (y, r, h) in enumerate(tiers):
        V, F = cyl(r, 0.0, h, n=n, base_at=y, rot=i * 17)
        if d == 0:
            V = jitter(V, 0.22, seed=i + 1); V[np.abs(V[:, 0]) + np.abs(V[:, 2]) < 0.01, 0] = 0
        mat = "pine" if i % 2 == 0 else "pine_light"
        fol.add((V, F), mat)
        if d == 0:
            # inner darker underside ring (a short frustum) gives depth from 45°
            fol.add(cyl(r * 0.92, r * 0.75, 0.35, n=n, base_at=y - 0.3, rot=i * 17, caps=True), "pine")
    if variant == "fallen":
        fallen(root, H, 0.42)
    return root

def tree_broadleaf(d, variant="standing"):
    root = Node("tree_broadleaf")
    trunk = root.child(Node("trunk"))
    fol = root.child(Node("foliage"))
    n = {0: 9, 1: 7, 2: 5}[d]
    burnt = variant == "burnt"
    bark = "charred" if burnt else "bark"
    trunk.add(lathe([(0.55, 0), (0.40, 0.8), (0.33, 3.0), (0.28, 4.5), (0.20, 5.5)], n=n), bark)
    branches = [(4.3, 0, 48, 2.6), (4.8, 120, 42, 2.3), (4.5, 240, 55, 2.4), (5.2, 60, 30, 1.8)]
    if d == 2:
        branches = branches[:1]
    for (y, ang, tilt, ln) in branches:
        trunk.add(cyl(0.16, 0.08, ln, n=5 if d < 2 else 4, base_at=0), bark, t=(0, y, 0), rz=-tilt, ry=ang)
    if burnt:
        if d < 2:
            for (y, ang, tilt, ln) in [(6.2, 20, 35, 1.4), (6.4, 200, 40, 1.2), (6.0, 300, 60, 1.6)]:
                trunk.add(cyl(0.08, 0.03, ln, n=4, base_at=0), bark, t=(0, y, 0), rz=-tilt, ry=ang)
        return root
    blobs = [((0, 7.4, 0), 2.6, "leaf"), ((1.9, 6.4, 0.3), 1.9, "leaf_light"), ((-1.1, 6.6, 1.7), 1.8, "leaf"),
             ((-0.9, 6.5, -1.8), 1.7, "leaf_light"), ((0.6, 8.6, -0.4), 1.6, "leaf_light"), ((0.3, 6.1, 0.2), 2.2, "leaf")]
    if d == 1:
        blobs = blobs[:3]
    if d == 2:
        blobs = [((0, 7.0, 0), 3.0, "leaf")]
    sub = 1 if d < 2 else 0
    for i, (c, r, mat) in enumerate(blobs):
        V, F = ico(r, sub, c=c, s=(1.0, 0.82, 1.0))
        V = jitter(V, r * 0.14, seed=50 + i)
        V, F = fix_winding(V, F)
        fol.add((V, F), mat)
    if variant == "fallen":
        fallen(root, 10.0, 0.55)
    return root

def fallen(root, H, r):
    """Tilt a standing tree onto the ground along +X and add a torn root plate. Mutates geometry in place."""
    for n in [root] + root.children:
        for mat, lst in n.prims.items():
            n.prims[mat] = [(xf(V, rz=-84, t=(0.6, r * 0.9, 0)), F) for V, F in lst]
    allv = np.vstack([V for n in [root] + root.children for lst in n.prims.values() for V, F in lst])
    lift = -allv[:, 1].min() - 0.25
    for n in [root] + root.children:
        for mat, lst in n.prims.items():
            n.prims[mat] = [(V + [0, lift, 0], F) for V, F in lst]
    plate = root.child(Node("root_plate"))
    V, F = cyl(r * 2.4, r * 2.1, 0.5, n=9 if len(root.children) < 6 else 6, base_at=0)
    V = jitter(V, 0.2, seed=3)
    plate.add((V, F), "soil", rz=-84 + 90, t=(0.2, r * 1.2 + lift, 0))
    plate.add(cyl(0.9, 0.7, 0.25, n=6, base_at=-0.1), "soil", t=(0.6, 0, 0))

# ============================================================================ rocks
def rock_blob(seed, r, sub, s=(1, 0.65, 1), flat=0.35):
    rng = np.random.default_rng(seed)
    V, F = ico(r, sub, s=s)
    V = jitter(V, r * 0.16, seed=seed)
    # low-frequency bulge
    k = rng.normal(size=3); k /= np.linalg.norm(k)
    V = V * (1 + 0.18 * np.tanh((V @ k) / r))[:, None]
    V[:, 1] -= r * s[1] * flat                      # sink into the ground
    V = xf(V, ry=rng.uniform(0, 360))
    V, F = fix_winding(V, F)
    N = face_normals(V, F)
    moss = N[:, 1] > 0.55
    return (V, F), moss

def rock(d, size):
    root = Node(f"rock_{size}")
    body = root.child(Node("rock"))
    sub = {0: 2, 1: 1, 2: 0}[d]
    if size == "small":
        g, m = rock_blob(11, 0.75, sub, s=(1.15, 0.7, 0.95))
        body.add_split(g, "rock", "moss", m, t=(0, 0.42, 0))
        if d < 2:
            g, m = rock_blob(12, 0.38, max(sub - 1, 0), s=(1, 0.8, 1))
            body.add_split(g, "rock_dark", "moss", m, t=(0.75, 0.22, 0.45))
    else:
        g, m = rock_blob(21, 1.7, sub, s=(1.1, 0.85, 0.95))
        body.add_split(g, "rock", "moss", m, t=(-0.2, 1.05, 0.1))
        g, m = rock_blob(22, 1.05, max(sub - 1, 0) if d else sub, s=(1.2, 0.7, 1.0))
        body.add_split(g, "rock_dark", "moss", m, t=(1.3, 0.55, -0.9))
        if d < 2:
            g, m = rock_blob(23, 0.7, max(sub - 1, 0), s=(1, 0.75, 1.1))
            body.add_split(g, "rock", "moss", m, t=(0.6, 0.35, 1.3))
            g, m = rock_blob(24, 0.45, max(sub - 1, 0))
            body.add_split(g, "rock_dark", "moss", m, t=(-1.7, 0.25, -0.9))
    return root

# ============================================================================ wrecks
def scorch(root, rx, rz, d, seed=5):
    """Flat irregular scorch mark on the ground (top faces only)."""
    s = root.child(Node("scorch"))
    rng = np.random.default_rng(seed)
    n = 16 if d == 0 else 10
    ang = np.linspace(0, 2 * math.pi, n, endpoint=False)
    rad = 1 + 0.12 * np.sin(3 * ang + rng.uniform(0, 6)) + rng.uniform(-0.08, 0.08, n)
    ring = np.stack([rx * rad * np.cos(ang), np.full(n, 0.012), rz * rad * np.sin(ang)], 1)
    V = np.vstack([ring, [[0, 0.012, 0]]])
    F = np.array([[i, n, (i + 1) % n] for i in range(n)])
    s.add((V, F), "charred")

def wheel(r, w, n):
    V, F = cyl(r, r, w, n=n)
    return xf(V, rx=90), F     # axis along Z

def wreck_light(d):
    """Burnt-out open utility car, ~4.6 x 2.3 m, nose-down on a missing front wheel."""
    root = Node("wreck_light_vehicle")
    if d < 2:
        scorch(root, 3.4, 2.0, d)
    hull = root.child(Node("hull", t=(0, 0.55, 0), rot_deg=(6, 0, -4)))
    n = {0: 10, 1: 7, 2: 5}[d]
    # tub body with sloped bonnet
    hull.add(box(2.4, 0.55, 1.7, c=(-0.5, 0.0, 0)), "burnt")
    hull.add(box(1.5, 0.45, 1.6, c=(1.35, -0.05, 0), taper_x=0.8, taper_z=0.9), "rust")
    if d < 2:
        # bonnet hanging open
        hull.add(box(1.3, 0.06, 1.4, c=(0, 0, 0)), "burnt", t=(1.3, 0.55, 0), rz=35)
        # wheel arches
        for sz in (-1, 1):
            hull.add(box(1.0, 0.25, 0.3, c=(1.45, -0.3, sz * 0.95)), "rust")
            hull.add(box(1.0, 0.25, 0.3, c=(-1.35, -0.3, sz * 0.95)), "burnt")
        # seats / bulkhead remains
        hull.add(box(0.25, 0.5, 1.5, c=(0.4, 0.4, 0)), "burnt")
        hull.add(box(0.5, 0.35, 0.5, c=(-0.4, 0.35, -0.45)), "charred")
        hull.add(box(0.5, 0.35, 0.5, c=(-0.4, 0.35, 0.45)), "charred")
    # bent roll bar
    bar = hull.child(Node("rollbar"))
    bar.add(box(0.08, 1.1, 0.08, base_at=0.2, c=(-0.1, 0, -0.75)), "steel", rz=12, pivot=(-0.1, 0.2, -0.75))
    bar.add(box(0.08, 1.1, 0.08, base_at=0.2, c=(-0.1, 0, 0.75)), "steel", rz=12, pivot=(-0.1, 0.2, 0.75))
    bar.add(box(0.08, 0.08, 1.58, c=(-0.33, 1.27, 0)), "steel", rz=12, pivot=(-0.1, 0.2, 0))
    # wheels: three attached, the front-right one lies on the ground
    wh = root.child(Node("wheels"))
    r = 0.42
    for (x, z) in ((1.45, -1.0), (-1.35, -1.0), (-1.35, 1.0)):
        wh.add(wheel(r, 0.28, n), "dark", t=(x, r - 0.05, z))
        if d == 0:
            wh.add(wheel(r * 0.55, 0.3, n), "rust", t=(x, r - 0.05, z))
    wh.add(wheel(r, 0.28, n), "dark", t=(2.4, 0.14, 1.5), rx=90, ry=20)
    # front axle stub exposed where the wheel came off
    if d < 2:
        wh.add(cyl(0.07, 0.07, 0.6, n=5), "steel", t=(1.45, 0.35, 0.8), rx=90)
        # debris / crate
        deb = root.child(Node("debris"))
        deb.add(box(0.5, 0.35, 0.4, base_at=0, c=(-1.2, 0, 1.6)), "burnt", pivot=(-1.2, 0, 1.6), ry=25)
        deb.add(box(0.9, 0.05, 0.4, base_at=0, c=(1.0, 0, -1.6)), "rust", pivot=(1.0, 0, -1.6), ry=-30)
    root.child(Node("fx_smoke_1", t=(1.2, 1.1, 0)))
    root.child(Node("fx_fire_1", t=(0.2, 0.8, 0)))
    return root

def tracked_hull(hull, L, W, Hh, d, mat_a, mat_b, n):
    """Generic tracked chassis: sloped glacis, track boxes, fenders. No specific real vehicle."""
    hull.add(box(L * 0.62, Hh, W * 0.68, c=(-L * 0.08, 0, 0)), mat_a)
    # glacis (front wedge)
    hull.add(prism([(L * 0.23, -W * 0.34), (L * 0.5, -W * 0.30), (L * 0.5, W * 0.30), (L * 0.23, W * 0.34)], -Hh / 2, Hh * 0.1), mat_a)
    # rear plate step
    hull.add(box(L * 0.14, Hh * 0.7, W * 0.6, c=(-L * 0.46, -Hh * 0.15, 0)), mat_b)
    for sz in (-1, 1):
        hull.add(box(L * 0.92, Hh * 0.55, W * 0.16, c=(0, -Hh * 0.35, sz * W * 0.42), taper_x=0.9), "dark")
        if d < 2:
            # road wheels suggestion: a flat rail
            hull.add(box(L * 0.8, 0.06, W * 0.02, c=(0, -Hh * 0.05, sz * W * 0.51)), "steel")
            # fender
            hull.add(box(L * 0.9, 0.05, W * 0.18, c=(0, -Hh * 0.05 + 0.1, sz * W * 0.42)), mat_b)

def wreck_medium(d):
    """Burnt-out medium tracked vehicle, ~5.8 x 3.0 m, turret knocked askew."""
    root = Node("wreck_medium_vehicle")
    if d < 2:
        scorch(root, 4.2, 2.6, d, seed=6)
    n = {0: 10, 1: 7, 2: 5}[d]
    L, W, Hh = 5.6, 2.9, 1.1
    hull = root.child(Node("hull", t=(0, Hh / 2 + 0.05, 0), rot_deg=(3, 0, 0)))
    tracked_hull(hull, L, W, Hh, d, "burnt", "rust", n)
    if d < 2:
        # thrown track lying beside the vehicle
        tr = root.child(Node("debris"))
        V, F = box(3.2, 0.12, 0.45, base_at=0, c=(-0.8, 0, 2.0))
        tr.add((V, F), "dark", ry=-12)
        tr.add(box(0.5, 0.3, 0.6, base_at=0, c=(3.2, 0, -1.2)), "rust", pivot=(3.2, 0, -1.2), ry=30)
    # turret: a squat, flat, angular drum sitting tilted on the hull
    tur = hull.child(Node("turret", t=(0.2, Hh / 2, 0), rot_deg=(0, 35, 14)))
    tur.add(cyl(1.05, 0.9, 0.55, n=n, base_at=0.0), "burnt")
    tur.add(box(0.9, 0.3, 0.7, c=(0.2, 0.68, 0.0)), "rust")        # hatch block
    if d < 2:
        tur.add(cyl(0.3, 0.3, 0.1, n=n, base_at=0.83, c=(-0.3, 0, 0.3)), "dark")
    bar = tur.child(Node("barrel", t=(0.9, 0.3, 0), rot_deg=(0, 0, -8)))
    bar.add(cyl(0.11, 0.09, 2.6, n=6), "dark", t=(1.3, 0, 0), rz=-90)
    root.child(Node("fx_smoke_1", t=(0.4, Hh + 0.8, 0)))
    root.child(Node("fx_fire_1", t=(-1.4, Hh * 0.6, 0.6)))
    return root

def wreck_heavy(d):
    """Burnt-out heavy tracked vehicle, ~7.8 x 3.8 m. Turret blown clean off, lying upside down behind it."""
    root = Node("wreck_heavy_vehicle")
    if d < 2:
        scorch(root, 5.4, 3.6, d, seed=8)
    n = {0: 12, 1: 8, 2: 6}[d]
    L, W, Hh = 7.4, 3.7, 1.35
    hull = root.child(Node("hull", t=(0, Hh / 2 + 0.05, 0), rot_deg=(0, 0, 2)))
    tracked_hull(hull, L, W, Hh, d, "burnt", "burnt", n)
    hull.add(box(L * 0.3, 0.12, W * 0.6, c=(-L * 0.25, Hh / 2 + 0.06, 0)), "hull_grey")   # engine deck plate
    # open turret ring with charred interior
    hull.add(cyl(1.25, 1.25, 0.18, n=n, base_at=Hh / 2 - 0.02, c=(0.3, 0, 0)), "rust")
    if d < 2:
        hull.add(cyl(1.05, 1.05, 0.02, n=n, base_at=Hh / 2 - 0.1, c=(0.3, 0, 0)), "charred")
    if d < 2:
        # spare plates / stowage remains on the engine deck
        hull.add(box(1.2, 0.18, 1.5, c=(-2.3, Hh / 2 + 0.09, 0.2)), "hull_grey")
        hull.add(box(0.8, 0.4, 0.4, c=(-2.0, Hh / 2 + 0.2, -1.3)), "rust", ry=15)
    # turret upside down on the ground behind the hull (-X)
    tur = root.child(Node("turret", t=(-5.2, 0.0, 1.2), rot_deg=(172, 20, 0)))
    V, F = cyl(1.3, 1.1, 0.75, n=n, base_at=-0.75)
    tur.add((V, F), "burnt")
    if d < 2:
        tur.add(box(1.2, 0.25, 0.9, c=(0.0, -0.87, 0)), "rust")
    bar = tur.child(Node("barrel", t=(1.1, -0.35, 0), rot_deg=(0, 0, 0)))
    bar.add(cyl(0.16, 0.12, 3.4, n=6), "dark", t=(1.7, 0, 0), rz=-90)
    if d == 0:
        bar.add(cyl(0.2, 0.2, 0.5, n=6), "dark", t=(3.1, 0, 0), rz=-90)
    if d < 2:
        deb = root.child(Node("debris"))
        deb.add(box(1.4, 0.1, 0.8, base_at=0, c=(2.0, 0, 2.5)), "hull_grey", pivot=(2.0, 0, 2.5), ry=40)
        deb.add(box(0.6, 0.5, 0.5, base_at=0, c=(4.6, 0, -1.6)), "burnt", pivot=(4.6, 0, -1.6), ry=-20)
    root.child(Node("fx_smoke_1", t=(0.3, Hh + 0.3, 0)))
    root.child(Node("fx_smoke_2", t=(-5.2, 1.0, 1.2)))
    root.child(Node("fx_fire_1", t=(0.3, Hh * 0.8, 0)))
    return root

def wreck_vessel(d):
    """Half-sunk small vessel, ~18 x 5 m, listing to starboard. Y = 0 is the waterline."""
    root = Node("wreck_vessel")
    n = {0: 12, 1: 8, 2: 6}[d]
    hull = root.child(Node("hull", t=(0, -0.35, 0), rot_deg=(11, 0, 3)))
    L, W, Hh = 18.0, 5.0, 2.4
    # hull: mid box + bow wedge + stern taper
    hull.add(box(L * 0.55, Hh, W, c=(-0.5, 0, 0), taper_z=1.0), "hull_grey")
    hull.add(prism([(L * 0.225, -W / 2), (L * 0.5, -W * 0.08), (L * 0.5, W * 0.08), (L * 0.225, W / 2)], -Hh / 2, Hh / 2), "hull_grey")
    hull.add(box(L * 0.2, Hh, W, c=(-L * 0.375, 0, 0), taper_z=1.0), "hull_grey")
    hull.add(box(L * 0.75, 0.25, W + 0.2, c=(-0.5, Hh / 2 - 0.12, 0)), "burnt")      # charred deck
    hull.add(prism([(L * 0.225, -W / 2 - 0.1), (L * 0.5 + 0.05, -W * 0.08), (L * 0.5 + 0.05, W * 0.08), (L * 0.225, W / 2 + 0.1)], Hh / 2 - 0.25, Hh / 2 + 0.01), "burnt")
    if d < 2:
        # gunwale strips in hull colour, gun tub and bollards
        for sz in (-1, 1):
            hull.add(box(L * 0.74, 0.3, 0.18, c=(-0.5, Hh / 2 + 0.1, sz * (W / 2 + 0.02))), "hull_grey")
        hull.add(box(0.5, 0.5, 0.5, c=(-7.6, Hh / 2 + 0.25, -1.4)), "dark")
        hull.add(box(0.5, 0.5, 0.5, c=(6.0, Hh / 2 + 0.25, 1.6)), "dark")
    # deckhouse (angular bridge), burnt, with a collapsed mast
    hull.add(box(3.2, 1.6, 3.2, c=(-1.5, Hh / 2 + 0.8, 0), taper=0.92), "burnt")
    if d < 2:
        hull.add(box(1.6, 0.9, 2.4, c=(0.9, Hh / 2 + 0.45, 0), taper=0.9), "rust")
        hull.add(box(0.5, 0.6, 1.0, c=(-5.0, Hh / 2 + 0.3, 0.8)), "rust")
        hull.add(cyl(0.12, 0.08, 5.0, n=5), "dark", t=(-3.2, Hh / 2 + 1.0, 0), rz=55)      # fallen mast
        hull.add(cyl(0.4, 0.3, 1.2, n=n), "dark", t=(-4.2, Hh / 2 + 0.6, -0.6))             # funnel stub
    # forward gun mount remains (abstract drum)
    tur = hull.child(Node("turret", t=(3.8, Hh / 2, 0), rot_deg=(0, 50, 0)))
    tur.add(cyl(0.9, 0.8, 0.6, n=n, base_at=0), "burnt")
    bar = tur.child(Node("barrel", t=(0.7, 0.35, 0), rot_deg=(0, 0, 18)))
    bar.add(cyl(0.1, 0.08, 2.4, n=6), "dark", t=(1.2, 0, 0), rz=-90)
    if d < 2:
        deb = root.child(Node("debris"))
        deb.add(box(2.0, 0.1, 0.9, base_at=0.0, c=(-11.0, 0, 1.8)), "burnt", pivot=(-11.0, 0, 1.8), ry=30)
        deb.add(cyl(0.6, 0.6, 0.9, n=6), "rust", t=(5.5, -0.5, -3.3), rx=70)
        deb.add(box(1.2, 0.08, 0.7, base_at=0.02, c=(9.5, 0, 2.6)), "rust", pivot=(9.5, 0, 2.6), ry=-45)
    root.child(Node("fx_smoke_1", t=(-1.5, 2.6, 0)))
    root.child(Node("fx_fire_1", t=(-1.5, 1.8, 0)))
    return root

# ============================================================================ registry
MODELS = {
    **{u: ((lambda u: (lambda d: build_unit(d, u)))(u), "infantry_pair" if "team" in u else "infantry", (0.6, 0.6, 1.8)) for u in UNITS},
    "neutral_ore_field_stage_01":  (lambda d: ore_field(d, 1), "decor", (4, 4, 1)),
    "neutral_ore_field_stage_02":  (lambda d: ore_field(d, 2), "decor", (4, 4, 1)),
    "neutral_ore_field_stage_03":  (lambda d: ore_field(d, 3), "decor", (4, 4, 1)),
    "neutral_ore_field_stage_04":  (lambda d: ore_field(d, 4), "decor", (4, 4, 1)),
    "neutral_tree_pine":           (lambda d: tree_pine(d), "decor", (6, 6, 12)),
    "neutral_tree_pine_fallen":    (lambda d: tree_pine(d, "fallen"), "decor", (12, 6, 3)),
    "neutral_tree_pine_burnt":     (lambda d: tree_pine(d, "burnt"), "decor", (3, 3, 10)),
    "neutral_tree_broadleaf":      (lambda d: tree_broadleaf(d), "decor", (8, 8, 10)),
    "neutral_tree_broadleaf_fallen": (lambda d: tree_broadleaf(d, "fallen"), "decor", (10, 8, 4)),
    "neutral_tree_broadleaf_burnt": (lambda d: tree_broadleaf(d, "burnt"), "decor", (5, 5, 7)),
    "neutral_rock_small":          (lambda d: rock(d, "small"), "decor", (2, 2, 1)),
    "neutral_rock_large":          (lambda d: rock(d, "large"), "decor", (4, 4, 2)),
    "neutral_wreck_light_vehicle": (wreck_light, "vehicle", (4.6, 2.3, 1.7)),
    "neutral_wreck_medium_vehicle": (wreck_medium, "vehicle", (5.8, 3.0, 2.2)),
    "neutral_wreck_heavy_vehicle": (wreck_heavy, "vehicle", (7.8, 3.8, 2.4)),
    "neutral_wreck_vessel":        (wreck_vessel, "vehicle", (18, 5, 4)),
}
BUDGET = {"infantry": (1500, 250, 60), "infantry_pair": (3000, 500, 120), "vehicle": (1500, 500, 150), "decor": (1500, 500, 150)}

def build(name, outdir):
    fn, cls, approx = MODELS[name]
    roots = []
    tris = []
    mats = set()
    for d in range(3):
        r = fn(d)
        r.name = f"{name}_LOD{d}"
        roots.append(r); tris.append(r.tri_count()); mats |= r.materials()
    w = GLBWriter()
    path = os.path.join(outdir, name + ".glb")
    w.write(path, roots, extras={"staalfront": {"id": name, "class": cls, "lods": ["LOD0", "LOD1", "LOD2"],
                                                "axes": "Y up, front +X, right +Z, ground Y=0, metres"}})
    b = bounds(roots)
    ok = all(t <= B for t, B in zip(tris, BUDGET[cls]))
    return {"file": name + ".glb", "class": cls, "triangles": {"LOD0": tris[0], "LOD1": tris[1], "LOD2": tris[2]},
            "budget": dict(zip(["LOD0", "LOD1", "LOD2"], BUDGET[cls])), "within_budget": ok,
            "materials": sorted(mats), "material_count": len(mats),
            "bounds_min_max_m": b, "nodes": [c.name for c in roots[0].children],
            "format": "glTF 2.0 binary (.glb)", "size_bytes": os.path.getsize(path)}

if __name__ == "__main__":
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    names = sys.argv[2:] or list(MODELS)
    man = []
    for nme in names:
        m = build(nme, out)
        man.append(m)
        flag = "" if m["within_budget"] and m["material_count"] <= 6 else "  <-- CHECK"
        print(f"{nme:34s} tris {m['triangles']['LOD0']:5d}/{m['triangles']['LOD1']:4d}/{m['triangles']['LOD2']:3d}  mats {m['material_count']}  bounds {m['bounds_min_max_m']}{flag}")
    json.dump(man, open(os.path.join(out, "manifest.json"), "w"), indent=2)
