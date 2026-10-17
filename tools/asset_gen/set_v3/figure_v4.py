"""Parametric rounded human figures for Staalfront (rigid parts, 3 LODs).
Nodes: torso, head, arm_l, arm_r, leg_l, leg_r  (two-man units: second figure with suffix _2).
Front = +X, right = +Z, ground Y = 0, metres. LOD0 <= 1500 tris, LOD1 <= 250, LOD2 <= 60.
"""
import numpy as np
from glbkit import *

MATERIALS.update({
    "team":            ("#9C9C9C", 0.0, 1.0, None),   # placeholder, engine assigns player colour
    "meridian_sand":   ("#C8B48A", 0.0, 1.0, None),
    "meridian_olive":  ("#6B7045", 0.0, 1.0, None),
    "meridian_blue":   ("#5C7489", 0.0, 1.0, None),
    "veyran_slate":    ("#5A6470", 0.0, 1.0, None),
    "veyran_char":     ("#33363B", 0.0, 1.0, None),
    "veyran_crimson":  ("#7A3B3B", 0.0, 1.0, None),
    "camo":            ("#4F5A3A", 0.0, 1.0, None),
    "wood":            ("#6A4B30", 0.0, 0.9, None),
})

def elathe(profile, n, sx=1.0, sz=1.0, caps="none", **k):
    V, F = lathe(profile, n=n, caps=caps)
    V = V * [sx, 1.0, sz]
    if k:
        V = xf(V, **k)
    return V, F

def limb(r_top, r_mid, r_bot, length, n, rings=3):
    if rings >= 4:
        prof = [(r_bot, -length), (r_mid * 0.97, -length * 0.72), (r_mid, -length * 0.4), (r_top, 0.0)]
    else:
        prof = [(r_bot, -length), (r_mid, -length * 0.45), (r_top, 0.0)]
    return lathe(prof, n=n, caps="bottom")

def blob(r, sub, s=(1, 1, 1), c=(0, 0, 0)):
    return ico(r, sub, c=c, s=s)

def dir_from(angles_deg_rz, length):
    """Point at the end of a limb hanging from origin, rotated rz degrees (about Z) — in the XY plane."""
    a = np.radians(angles_deg_rz)
    return np.array([length * np.sin(a), -length * np.cos(a), 0.0])

# --------------------------------------------------------------------------------------------- props
def rifle(n, long=False, scope=False, carbine=False):
    """Generic bolt-action style long arm along +X, origin at the grip. Not a replica."""
    parts = []
    L = 1.25 if long else (0.78 if carbine else 1.0)
    parts.append((box(0.32, 0.07, 0.04, c=(-0.16, -0.02, 0), taper_x=1.0), "wood"))              # stock
    parts.append((box(0.40, 0.05, 0.045, c=(0.18, 0.0, 0)), "wood"))                              # fore-end
    g = cyl(0.012, 0.012, L - 0.3, n=max(5, n - 3))
    parts.append(((xf(g[0], rz=-90, t=(0.3 + (L - 0.3) / 2, 0.015, 0)), g[1]), "dark"))
    parts.append((box(0.06, 0.04, 0.03, c=(0.02, 0.035, 0)), "dark"))                            # receiver/bolt
    parts.append((box(0.04, 0.10, 0.03, c=(-0.02, -0.08, 0)), "wood"))                           # grip
    if scope:
        g = cyl(0.018, 0.018, 0.22, n=6)
        parts.append(((xf(g[0], rz=-90, t=(0.08, 0.07, 0)), g[1]), "dark"))
    return parts

def rocket_tube(n, hooded=False, lite=False):
    g = cyl(0.045, 0.045, 1.25, n=n, caps=not lite)
    parts = [((xf(g[0], rz=-90, t=(0.0, 0, 0)), g[1]), "dark")]
    if lite:
        return parts
    if hooded:
        parts.append((box(0.14, 0.09, 0.08, c=(0.35, 0.07, 0), taper_x=0.7), "dark"))            # angular sight hood
    else:
        g2 = cyl(0.055, 0.055, 0.25, n=n)                                                       # wrapped grip band
        parts.append(((xf(g2[0], rz=-90, t=(0.2, 0, 0)), g2[1]), "leather"))
    return parts

# --------------------------------------------------------------------------------------------- figure
def figure(d, spec, offset=(0, 0, 0), suffix=""):
    """spec keys: top, legs, boots, skin, head ('flatcap','beanie','helmet_round','helmet_brim','hood'),
    headgear_mat, faction (None/'meridian'/'veyran'), carry ('suitcase','toolbox','rifle','carbine','rocket','sniper',None),
    back ('pack_roll','toolbag','medbag','charges','spade',None), extras set."""
    S = dict(skin="skin", boots="dark", extras=set(), carry=None, back=None, faction=None, headgear_mat=None)
    S.update(spec)
    ex = S["extras"]
    root = Node("fig")
    top, legs, skin, boots = S["top"], S["legs"], S["skin"], S["boots"]
    hat = S["headgear_mat"] or top
    team = S["faction"] is not None
    n = {0: 10, 1: 5, 2: 5}[d]
    nl = {0: 8, 1: 5, 2: 5}[d]
    sub = 1 if d == 0 else 0
    ox, oy, oz = offset
    def N(name):
        return root.child(Node(name + suffix, t=offset))

    # ---------------------------------------------------------------- LOD2 silhouette
    if d == 2:
        N("leg_l").add(box(0.26, 0.86, 0.36, base_at=0.0), legs)
        N("leg_r")
        t = N("torso"); t.add(box(0.30, 0.56, 0.44, base_at=0.86, taper_z=1.1, taper_x=0.9), top)
        if team:
            t.add(box(0.26, 0.04, 0.42, base_at=1.42), "team")
        h = N("head"); h.add(box(0.20, 0.26, 0.20, base_at=1.46), skin if S["head"] in ("flatcap", "beanie", None) else hat)
        c = S["carry"]
        al = N("arm_l")
        if c not in ("rifle", "carbine", "sniper", "rocket"):
            al.add(box(0.14, 0.60, 0.12, base_at=0.80, c=(0.02, 0, -0.26)), top)
        ar = N("arm_r")
        if c == "suitcase":
            ar.add(box(0.46, 0.34, 0.13, base_at=0.36, c=(0.06, 0, 0.34)), "leather")
        elif c == "toolbox":
            ar.add(box(0.44, 0.22, 0.18, base_at=0.44, c=(0.06, 0, 0.36)), "steel")
        elif c in ("rifle", "carbine", "sniper"):
            ar.add(box(1.0 if c != "sniper" else 1.3, 0.06, 0.06, c=(0.35, 1.0, 0.28)), "dark")
        elif c == "rocket":
            ar.add(box(1.25, 0.09, 0.09, c=(0.1, 1.55, 0.25)), "dark")
        return finish(root, team, boots)

    # ---------------------------------------------------------------- legs
    for side, z, swing in (("l", -0.10, 9), ("r", 0.10, -7)):
        L = N(f"leg_{side}")
        hip = (0.0, 0.88, z)
        L.add(limb(0.095, 0.075, 0.055, 0.82, nl, rings=4 if d == 0 else 3), legs, t=hip, rz=swing)
        foot_x = -0.82 * np.sin(np.radians(swing))
        if d == 0:
            L.add(blob(0.07, 0, s=(1.0, 0.8, 1.0)), legs, t=(foot_x * 0.45, 0.47, z))             # knee
            V, F = elathe([(0.0, 0.0), (0.06, 0.05), (0.035, 0.10)], nl, sx=2.3, sz=1.0, caps="top")
            L.add((V, F), boots, t=(foot_x + 0.05, 0.0, z))
            if "boots_high" in ex or "puttees" in ex:
                L.add(cyl(0.062, 0.07, 0.22, n=nl, base_at=0.09, caps=False), boots, t=(foot_x * 0.9, 0, z))
            elif "cuff" in ex:
                L.add(cyl(0.062, 0.058, 0.05, n=nl, base_at=0.10, caps=False), boots, t=(foot_x, 0, z))
        else:
            L.add(box(0.24, 0.08, 0.11, base_at=0, c=(foot_x + 0.04, 0, z)), boots)

    # ---------------------------------------------------------------- torso
    T = N("torso")
    long_coat = "long_coat" in ex
    if long_coat:
        T.add(elathe([(0.19, 0.60), (0.165, 0.86), (0.155, 1.12), (0.15, 1.40)], n, sx=0.72, sz=1.25, caps="bottom"), top)
    else:
        T.add(elathe([(0.165, 0.84), (0.16, 1.00), (0.15, 1.18), (0.16, 1.32), (0.15, 1.40)] if d == 0 else
                     [(0.165, 0.84), (0.155, 1.12), (0.15, 1.40)], n, sx=0.72, sz=1.2, caps="bottom"), top)
    # shoulders — team colour on faction units (reads from above)
    T.add(elathe([(0.16, 1.36), (0.21, 1.42), (0.18, 1.46)] if d == 0 else [(0.21, 1.40), (0.18, 1.46)],
                 n, sx=0.75, sz=1.3, caps="top"), "team" if team else top)
    if d == 0:
        # belt
        T.add(elathe([(0.17, 0.98), (0.175, 1.03)], n, sx=0.75, sz=1.25), "leather" if "belt_leather" in ex else boots)
        if "webbing" in ex:
            # Y-strap webbing + two pouches on the front
            T.add(box(0.03, 0.36, 0.05, base_at=1.04, c=(0.135, 0, -0.07)), "leather")
            T.add(box(0.03, 0.36, 0.05, base_at=1.04, c=(0.135, 0, 0.07)), "leather")
            T.add(box(0.07, 0.10, 0.09, base_at=0.98, c=(0.13, 0, -0.11)), "leather")
            T.add(box(0.07, 0.10, 0.09, base_at=0.98, c=(0.13, 0, 0.11)), "leather")
        if "lapels" in ex:
            T.add(box(0.03, 0.40, 0.05, base_at=0.80, c=(0.145, 0, -0.045)), boots)
            T.add(box(0.03, 0.40, 0.05, base_at=0.80, c=(0.145, 0, 0.045)), boots)
        if "collar_tab" in ex:
            T.add(box(0.03, 0.05, 0.08, base_at=1.33, c=(0.14, 0, -0.07)), "veyran_crimson")
            T.add(box(0.03, 0.05, 0.08, base_at=1.33, c=(0.14, 0, 0.07)), "veyran_crimson")
        if "scarf" in ex:
            T.add(elathe([(0.095, 1.40), (0.13, 1.44), (0.11, 1.50)], n), legs)
        if "bib" in ex:
            T.add(box(0.03, 0.22, 0.20, base_at=1.14, c=(0.125, 0, 0), taper_z=0.8), top)
            T.add(box(0.03, 0.10, 0.10, base_at=1.22, c=(0.14, 0, -0.08)), boots)
        if "toolbelt" in ex:
            T.add(elathe([(0.17, 0.90), (0.175, 0.96)], n, sx=0.78, sz=1.22), "leather")
            T.add(blob(0.06, 0, s=(0.8, 1.1, 1.0)), "leather", t=(0.08, 0.90, -0.17))
            T.add(blob(0.06, 0, s=(0.8, 1.1, 1.0)), "leather", t=(0.08, 0.90, 0.17))
        if "sandbag" in ex:
            T.add(blob(0.09, 0, s=(1.0, 0.7, 1.4)), "meridian_sand" if S["faction"] == "meridian" else "leather", t=(-0.05, 0.92, 0.22))
        if "armband" in ex:
            pass  # placed on the arm below
        if "camonet" in ex:
            # ragged net over the shoulders
            V, F = elathe([(0.26, 1.10), (0.22, 1.30), (0.19, 1.44)], n, sx=0.8, sz=1.2)
            V = jitter(V, 0.03, seed=9)
            T.add((V, F), "camo")
        if "cape" in ex:
            V, F = elathe([(0.30, 0.55), (0.24, 1.00), (0.20, 1.30), (0.17, 1.46)], n, sx=0.75, sz=1.2)
            V = jitter(V, 0.035, seed=4); V[:, 1] = np.minimum(V[:, 1], 1.47)
            T.add((V, F), "camo")
    # back gear
    b = S["back"]
    if b and d < 2:
        bx = -0.20
        if b == "pack_roll":
            T.add(box(0.16, 0.30, 0.30, base_at=1.00, c=(bx, 0, 0), taper=0.95), "meridian_sand" if S["faction"] == "meridian" else "leather")
            if d == 0:
                g = cyl(0.06, 0.06, 0.34, n=nl); T.add((xf(g[0], rx=90, t=(bx - 0.0, 1.35, 0)), g[1]), "camo" if not team else "meridian_sand")  # bedroll
            if team and d == 0:
                T.add(box(0.03, 0.12, 0.22, base_at=1.14, c=(bx - 0.08, 0, 0)), "team")                            # pack flap
        elif b == "toolbag":
            T.add(box(0.20, 0.34, 0.34, base_at=0.96, c=(bx - 0.02, 0, 0), taper=0.9), "leather")
            if d == 0:
                T.add(box(0.05, 0.05, 0.36, base_at=1.30, c=(bx - 0.06, 0, 0)), boots)
                g = cyl(0.015, 0.015, 0.45, n=5); T.add((xf(g[0], t=(bx - 0.08, 1.05, -0.10)), g[1]), "steel")   # crowbar
        elif b == "medbag":
            T.add(box(0.18, 0.26, 0.30, base_at=1.02, c=(bx, 0, 0)), "leather")
            if d == 0:
                T.add(box(0.03, 0.14, 0.14, base_at=1.08, c=(bx - 0.095, 0, 0)), "veyran_crimson")              # own symbol plate
                T.add(box(0.02, 0.04, 0.10, base_at=1.13, c=(bx - 0.115, 0, 0)), "skin")                        # (bar, not a cross)
        elif b == "charges":
            T.add(box(0.18, 0.30, 0.32, base_at=0.98, c=(bx, 0, 0)), "leather")
            if d == 0:
                g = cyl(0.07, 0.07, 0.10, n=nl); T.add((xf(g[0], rz=90, t=(bx - 0.1, 1.33, 0.0)), g[1]), "dark")  # cable reel
                T.add(box(0.06, 0.12, 0.06, base_at=1.00, c=(bx - 0.1, 0, -0.17)), "dark")
                T.add(box(0.06, 0.12, 0.06, base_at=1.00, c=(bx - 0.1, 0, 0.17)), "dark")
        elif b == "spade":
            T.add(box(0.03, 0.30, 0.16, base_at=1.00, c=(bx - 0.02, 0, 0.10), taper_z=0.7), boots)
            if d == 0:
                g = cyl(0.015, 0.015, 0.40, n=5); T.add((xf(g[0], t=(bx - 0.02, 0.62, 0.10)), g[1]), "wood")
            if d == 0:
                T.add(box(0.03, 0.22, 0.22, base_at=1.06, c=(bx, 0, -0.08)), "leather")                        # small pack

    # ---------------------------------------------------------------- head
    H = N("head")
    if d == 0:
        H.add(cyl(0.055, 0.06, 0.08, n=n, base_at=1.44, caps=False), skin)
    H.add(blob(0.11, sub, s=(0.95, 1.18, 0.92), c=(0.0, 1.60, 0)), skin)
    if d == 0:
        H.add(blob(0.035, 0, s=(1.2, 0.8, 0.8)), skin, t=(0.10, 1.58, 0))                           # nose
        H.add(blob(0.028, 0, s=(0.6, 1.0, 0.6)), skin, t=(0.0, 1.59, -0.105))                       # ears
        H.add(blob(0.028, 0, s=(0.6, 1.0, 0.6)), skin, t=(0.0, 1.59, 0.105))
    kind = S["head"]
    if kind == "flatcap":
        H.add(elathe([(0.12, 1.68), (0.135, 1.715), (0.0, 1.76)], n, sx=1.05), hat)
        H.add(box(0.10, 0.02, 0.17, base_at=1.69, c=(0.14, 0, 0), taper_z=0.75), hat)
    elif kind == "beanie":
        H.add(elathe([(0.12, 1.66), (0.11, 1.73), (0.0, 1.77)], n), hat)
        H.add(elathe([(0.125, 1.64), (0.132, 1.68)], n), boots)
    elif kind == "helmet_round":
        # Meridian: rounded dome, small rolled rim, team patch on top
        H.add(elathe([(0.135, 1.60), (0.14, 1.66), (0.12, 1.73), (0.06, 1.78)], n, sx=1.05), hat)
        if d == 0: H.add(elathe([(0.06, 1.78), (0.0, 1.785)], n, sx=1.05, caps="top"), "team" if team else hat)
        if d == 0:
            H.add(elathe([(0.14, 1.595), (0.145, 1.615)], n, sx=1.05), hat)
    elif kind == "helmet_brim":
        # Veyran: flatter crown, straight brim, team band
        H.add(elathe([(0.13, 1.64), (0.125, 1.72), (0.07, 1.77)], n, sx=1.0), hat)
        if d == 0: H.add(elathe([(0.07, 1.77), (0.0, 1.775)], n, caps="top"), "team" if team else hat)
        H.add(elathe([(0.16, 1.62), (0.165, 1.64), (0.135, 1.65)] if d == 0 else [(0.165, 1.63), (0.135, 1.65)], n, sx=1.15), hat)  # brim
        if d == 0 and team:
            H.add(elathe([(0.131, 1.655), (0.131, 1.685)], n), "team")                             # band
    elif kind == "hood":
        # camo hood: closed at the back and top, open in front so the face shows
        V, F = elathe([(0.15, 1.46), (0.14, 1.60), (0.13, 1.72), (0.0, 1.79)], n, sx=1.05)
        cen = V[F].mean(1)
        keep = (cen[:, 0] < 0.03) | (cen[:, 1] > 1.74)
        H.add((V, F[keep]), "camo")
        if d == 0:
            H.add(box(0.03, 0.03, 0.22, c=(0.09, 1.70, 0), taper_z=0.8), "camo")              # hood rim
    elif kind == "cap_peak":
        H.add(elathe([(0.12, 1.66), (0.125, 1.72), (0.0, 1.76)], n), hat)
        H.add(box(0.09, 0.02, 0.16, base_at=1.66, c=(0.14, 0, 0), taper_z=0.7), boots)
    if "headlamp" in ex and d < 2:
        H.add(box(0.04, 0.05, 0.06, c=(0.12, 1.69, 0)), "lamp")
        H.add(elathe([(0.125, 1.66), (0.125, 1.70)], n), boots)
    if "goggles" in ex and d == 0:
        H.add(box(0.03, 0.04, 0.16, c=(0.10, 1.64, 0)), boots)

    # ---------------------------------------------------------------- arms
    carry = S["carry"]
    two_hand = carry in ("rifle", "carbine", "sniper")
    hands = {}
    for side, z in (("l", -0.21), ("r", 0.21)):
        A = N(f"arm_{side}")
        sz = 1 if z > 0 else -1
        sh = np.array([0.0, 1.40, z])
        if d == 0:
            A.add(blob(0.075, 0), "team" if team else top, t=sh)
        is_r = side == "r"
        sleeve = top if "rolled" not in ex else skin
        if "armband" in ex and side == "l" and d == 0:
            A.add(elathe([(0.068, 1.18), (0.07, 1.25)], nl), "veyran_crimson", t=(0, 0, z - 0.0))
        # pose per carry type
        if two_hand:
            # both arms forward holding a long arm low across the front
            up_rz = 28 if is_r else 50; out = 2 if is_r else -14
            fore_rz = 75 if is_r else 45
        elif carry == "rocket":
            up_rz = 70 if is_r else 10; out = 6; fore_rz = 150 if is_r else 25
        elif carry in ("suitcase", "toolbox"):
            up_rz = 3 if is_r else -8; out = 3 if is_r else 9; fore_rz = 3 if is_r else 25
        else:
            up_rz = -6 if is_r else 8; out = 8; fore_rz = 20 if is_r else 25
        R_up = rot_y(0) @ rot_z(up_rz) @ rot_x(-sz * out)
        if d == 1:
            A.add(limb(0.07, 0.055, 0.045, 0.57, nl), top, t=sh, rz=up_rz * 0.6, rx=-sz * out)
            el = sh + R_up @ np.array([0, -0.30, 0])
        else:
            A.add(limb(0.065, 0.055, 0.05, 0.30, nl), top, t=sh, rz=up_rz, rx=-sz * out)
            el = sh + R_up @ np.array([0, -0.30, 0])
            A.add(blob(0.05, 0), top, t=el)                                                      # elbow
            A.add(limb(0.055, 0.05, 0.045, 0.27, nl), sleeve, t=el, rz=fore_rz, rx=-sz * out * 0.5)
            if sleeve == top:
                A.add(cyl(0.052, 0.054, 0.03, n=nl, base_at=-0.03, caps=False), boots if "cuff_dark" in ex else top,
                      t=el + rot_z(fore_rz) @ np.array([0, -0.27, 0]) + [0, 0.03, 0])
        hand = el + (rot_z(fore_rz) @ rot_x(-sz * out * 0.5)) @ np.array([0, -0.27, 0])
        if d == 0:
            A.add(blob(0.05, 0, s=(1.1, 1.25, 0.8)), skin, t=hand - [0, 0.03, 0])
        # carried object attached to the right hand
        if is_r and d < 2:
            if carry == "suitcase":
                c = hand + [0.02, -0.26, sz * 0.09]
                A.add(box(0.44, 0.32, 0.13, c=c, taper=0.97), "leather")
                if d == 0:
                    A.add(box(0.46, 0.035, 0.14, c=c + [0, 0.09, 0]), "dark")
                    A.add(box(0.46, 0.035, 0.14, c=c - [0, 0.09, 0]), "dark")
                    A.add(box(0.12, 0.05, 0.03, c=c + [0, 0.185, 0]), "dark")
            elif carry == "toolbox":
                c = hand + [0.02, -0.20, sz * 0.11]
                A.add(box(0.42, 0.20, 0.18, c=c), "steel")
                if d == 0:
                    A.add(box(0.44, 0.03, 0.20, c=c + [0, 0.11, 0]), "dark")
                    A.add(box(0.14, 0.05, 0.03, c=c + [0, 0.14, 0]), "dark")
            elif two_hand:
                hands[side] = (A, hand)
            elif carry == "rocket":
                for (V, F), m in rocket_tube(n, hooded=(S["faction"] == "veyran"), lite=(d == 1)):
                    A.add((xf(V, rz=10, t=(ox - 0.25, oy + 1.52, oz + z + 0.02)), F), m)
        if (not is_r) and d < 2 and two_hand:
            hands[side] = (A, hand)
        if (not is_r) and d == 0 and carry == "wrench":
            A.add(box(0.30, 0.03, 0.03, c=hand + [0.06, -0.04, 0]), "steel", rz=-70, pivot=hand - [0, 0.04, 0])
            A.add(box(0.06, 0.07, 0.03, c=hand + [0.21, -0.04, 0]), "steel", rz=-70, pivot=hand - [0, 0.04, 0])
        if (not is_r) and d == 0 and "binoculars" in ex:
            g = cyl(0.025, 0.025, 0.07, n=6)
            A.add((xf(g[0], rz=90, t=hand + [0.05, 0.0, -0.02]), g[1]), "dark")
            A.add((xf(g[0], rz=90, t=hand + [0.05, 0.0, 0.03]), g[1]), "dark")
    if two_hand and d < 2 and "r" in hands and "l" in hands:
        A, hr = hands["r"]; _, hl = hands["l"]
        dv = hl - hr; dist = float(np.linalg.norm(dv))
        ry = float(np.degrees(np.arctan2(-dv[2], dv[0])))
        rz = float(np.degrees(np.arctan2(dv[1], np.hypot(dv[0], dv[2]))))
        if d == 0:
            parts = rifle(n, long=(carry == "sniper"), scope=(carry == "sniper"), carbine=(carry == "carbine"))
        else:
            L = 1.2 if carry == "sniper" else (0.78 if carry == "carbine" else 1.0)
            parts = [(box(L - 0.2, 0.06, 0.045, c=((L - 0.2) / 2 - 0.2, 0, 0)), "wood")]
        # fore-end should sit under the left hand: scale grip->fore distance (0.35) to the hand distance
        for (V, F), m in parts:
            V = V.copy(); V[:, 0] *= max(0.8, min(1.25, dist / 0.38))
            A.add((xf(V, rz=rz, ry=ry, t=hr + [0, -0.01, 0]), F), m)
    return finish(root, team, boots)

def finish(root, team, boots):
    """Material budget: faction units fold generic materials into their palette."""
    remap = {"wood": "leather"}
    if team:
        remap.update({"dark": boots, "steel": boots})
    for nd in root.children:
        new = {}
        for m, lst in nd.prims.items():
            new.setdefault(remap.get(m, m), []).extend(lst)
        nd.prims = new
    return root

# --------------------------------------------------------------------------------------------- unit specs
MER = dict(faction="meridian", top="meridian_olive", legs="meridian_olive", boots="dark", headgear_mat="meridian_olive")
VEY = dict(faction="veyran", top="veyran_slate", legs="veyran_char", boots="veyran_char", headgear_mat="veyran_char")

def spec(base, **k):
    s = dict(base); ex = set(s.get("extras", set()))
    ex |= set(k.pop("extras", set()))
    s.update(k); s["extras"] = ex
    return s

UNITS = {
    # neutral
    "neutral_civilian":   spec(dict(top="coat", legs="trousers", head="flatcap", headgear_mat="cap", carry="suitcase",
                                   extras={"long_coat", "lapels", "scarf", "cuff", "belt_leather"})),
    "neutral_technician": spec(dict(top="overall", legs="overall", head="beanie", headgear_mat="cap", carry="toolbox",
                                   extras={"rolled", "bib", "toolbelt"})),
    # shared roles
    "meridian_rifleman":       spec(MER, head="helmet_round", carry="rifle", back="pack_roll", extras={"webbing", "puttees"}),
    "veyran_rifleman":         spec(VEY, head="helmet_brim", carry="rifle", extras={"long_coat", "webbing", "collar_tab", "boots_high"}),
    "meridian_rocket_trooper": spec(MER, head="helmet_round", carry="rocket", back="pack_roll", extras={"webbing", "puttees"}),
    "veyran_rocket_trooper":   spec(VEY, head="helmet_brim", carry="rocket", extras={"long_coat", "webbing", "collar_tab", "boots_high"}),
    "meridian_engineer":       spec(MER, head="cap_peak", carry="wrench", back="toolbag", extras={"headlamp", "rolled", "puttees", "toolbelt"}),
    "veyran_engineer":         spec(VEY, head="cap_peak", carry="wrench", back="toolbag", extras={"headlamp", "boots_high", "toolbelt"}),
    "meridian_scout":          spec(MER, head="hood", carry="carbine", extras={"camonet", "binoculars", "puttees"}),
    "veyran_scout":            spec(VEY, head="hood", carry="carbine", extras={"camonet", "binoculars", "boots_high"}),
    # faction-only roles
    "meridian_sniper":         spec(MER, head="hood", carry="sniper", extras={"cape", "puttees"}),
    "veyran_medic":            spec(VEY, head="helmet_brim", carry=None, back="medbag", extras={"long_coat", "armband", "boots_high"}),
    "veyran_combat_engineer":  spec(VEY, head="helmet_brim", carry="carbine", back="spade", extras={"webbing", "sandbag", "collar_tab", "boots_high"}),
    "veyran_demolition_team":  spec(VEY, head="helmet_brim", carry="carbine", back="charges", extras={"webbing", "boots_high"}),
}

def build_unit(d, name):
    s = UNITS[name]
    if name == "veyran_demolition_team":
        root = Node(name)
        a = figure(d, s, offset=(0, 0, -0.36))
        b = figure(d, spec(s, carry=None, back="charges"), offset=(0.15, 0, 0.36), suffix="_2")
        for c in a.children + b.children:
            root.children.append(c)
        return root
    return figure(d, s)
