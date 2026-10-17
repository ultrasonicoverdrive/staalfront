"""Rounded, slightly stylised human figures (rigid parts). Nodes: torso, head, arm_l, arm_r, leg_l, leg_r."""
import numpy as np
from glbkit import *

def elathe(profile, n, sx=1.0, sz=1.0, caps="none", **k):
    """Lathe with elliptical cross-section (sx along X = front/back, sz along Z = sideways)."""
    V, F = lathe(profile, n=n, caps=caps)
    V = V * [sx, 1.0, sz]
    if k:
        V = xf(V, **k)
    return V, F

def limb(r_top, r_mid, r_bot, length, n, bend_mid=0.0):
    """Tapered limb along -Y from the origin (joint at the top), with a slight waist."""
    prof = [(r_bot, -length), (r_mid, -length * 0.45), (r_top, 0.0)]
    return lathe(prof, n=n, caps="bottom")

def blob(r, sub, s=(1, 1, 1), c=(0, 0, 0)):
    return ico(r, sub, c=c, s=s)

def figure(d, kind):
    civ = kind == "civilian"
    root = Node(kind)
    top = "coat" if civ else "overall"
    legs = "trousers" if civ else "overall"
    cap_mat = "cap" if civ else "hair"
    n = {0: 8, 1: 5, 2: 5}[d]
    nl = {0: 7, 1: 5, 2: 5}[d]   # limbs
    sub = 1 if d == 0 else 0

    # ------------------------------------------------------------ LOD2: pure silhouette (<= 60 tris)
    if d == 2:
        root.child(Node("leg_l")).add(box(0.26, 0.86, 0.36, base_at=0.0), legs)
        root.child(Node("leg_r"))
        root.child(Node("torso")).add(box(0.30, 0.60, 0.44, base_at=0.86, taper_z=1.1, taper_x=0.9), top)
        root.child(Node("head")).add(box(0.20, 0.26, 0.20, base_at=1.46), "skin")
        root.child(Node("arm_l")).add(box(0.14, 0.60, 0.12, base_at=0.80, c=(0.02, 0, -0.26)), top)
        ar = root.child(Node("arm_r"))
        if civ:
            ar.add(box(0.46, 0.34, 0.13, base_at=0.36, c=(0.06, 0, 0.34)), "leather")
        else:
            ar.add(box(0.44, 0.22, 0.18, base_at=0.44, c=(0.06, 0, 0.36)), "steel")
        return root

    # ------------------------------------------------------------ legs (hip joints at y 0.88, z ±0.10)
    # a relaxed walking pose: left leg a touch forward, right leg a touch back
    for side, z, swing in (("l", -0.10, 10), ("r", 0.10, -8)):
        L = root.child(Node(f"leg_{side}"))
        hip = (0.0, 0.88, z)
        # thigh + shin as one tapered limb with a knee bulge
        L.add(limb(0.095, 0.075, 0.055, 0.82, nl), legs, t=hip, rz=swing)
        # shoe: rounded wedge pointing +X
        foot_x = 0.82 * np.sin(np.radians(swing)) * -1.0
        if d == 0:
            V, F = elathe([(0.0, 0.0), (0.06, 0.05), (0.035, 0.10)], nl, sx=2.3, sz=1.0, caps="top")
            L.add((V, F), "dark", t=(foot_x + 0.05, 0.0, z))
        else:
            L.add(box(0.24, 0.08, 0.11, base_at=0, c=(foot_x + 0.04, 0, z)), "dark")
        if civ and d == 0:
            L.add(cyl(0.062, 0.058, 0.05, n=n, base_at=0.10, caps=False), "dark", t=(foot_x, 0, z))         # trouser cuff / boot top

    # ------------------------------------------------------------ torso (hips -> chest -> shoulders)
    T = root.child(Node("torso"))
    if civ:
        # long coat: flares from the chest down to mid-thigh
        T.add(elathe([(0.19, 0.62), (0.165, 0.86), (0.155, 1.12), (0.15, 1.40)], n, sx=0.72, sz=1.25, caps="bottom"), top)
        T.add(elathe([(0.16, 1.36), (0.21, 1.42), (0.18, 1.46)] if d == 0 else [(0.21, 1.40), (0.18, 1.46)], n, sx=0.75, sz=1.3, caps="top"), top)           # shoulders
        if d == 0:
            # coat opening + lapels (two thin plates on the front)
            T.add(box(0.03, 0.44, 0.05, base_at=0.80, c=(0.145, 0, -0.045)), "trousers")
            T.add(box(0.03, 0.44, 0.05, base_at=0.80, c=(0.145, 0, 0.045)), "trousers")
            # belt
            T.add(elathe([(0.17, 0.98), (0.175, 1.03)], n, sx=0.75, sz=1.25), "dark")
            # scarf
            T.add(elathe([(0.095, 1.40), (0.13, 1.44), (0.11, 1.50)], n, sx=1.0, sz=1.05), "trousers")
    else:
        # work overall: hips to chest, slightly barrel shaped
        T.add(elathe([(0.165, 0.84), (0.16, 1.00), (0.15, 1.18), (0.16, 1.32), (0.15, 1.40)], n, sx=0.72, sz=1.2), top)
        T.add(elathe([(0.16, 1.36), (0.21, 1.42), (0.18, 1.46)] if d == 0 else [(0.21, 1.40), (0.18, 1.46)], n, sx=0.75, sz=1.3, caps="top"), top)           # shoulders
        # tool belt with pouches
        if d == 0: T.add(elathe([(0.17, 0.90), (0.175, 0.96)], n, sx=0.78, sz=1.22), "leather")
        if d == 0:
            T.add(blob(0.06, 0, s=(0.8, 1.1, 1.0)), "leather", t=(0.08, 0.90, -0.17))
            T.add(blob(0.06, 0, s=(0.8, 1.1, 1.0)), "leather", t=(0.08, 0.90, 0.17))
            T.add(box(0.03, 0.22, 0.20, base_at=1.14, c=(0.125, 0, 0), taper_z=0.8), "overall")      # bib
            T.add(box(0.03, 0.10, 0.10, base_at=1.22, c=(0.14, 0, -0.08)), "dark")               # chest pocket
            T.add(elathe([(0.09, 1.40), (0.115, 1.45), (0.10, 1.49)], n, sx=1.0, sz=1.05), "skin")   # open collar / neck

    # ------------------------------------------------------------ head
    H = root.child(Node("head"))
    if d == 0: H.add(cyl(0.055, 0.06, 0.08, n=n, base_at=1.44, caps=False), "skin")                                          # neck
    V, F = blob(0.11, sub, s=(0.95, 1.18, 0.92), c=(0.0, 1.60, 0))
    H.add((V, F), "skin")
    if d == 0:
        H.add(blob(0.035, 0, s=(1.2, 0.8, 0.8)), "skin", t=(0.10, 1.58, 0))                            # nose
    if civ:
        # flat cap with a forward brim
        H.add(elathe([(0.12, 1.68), (0.135, 1.715), (0.0, 1.76)], n, sx=1.05, sz=1.0), cap_mat)
        H.add(box(0.10, 0.02, 0.17, base_at=1.69, c=(0.14, 0, 0), taper_z=0.75), cap_mat)
    else:
        # knitted cap with a rolled band
        H.add(elathe([(0.12, 1.66), (0.11, 1.73), (0.0, 1.77)], n), "cap")
        H.add(elathe([(0.125, 1.64), (0.132, 1.68)], n), "dark")

    # ------------------------------------------------------------ arms (shoulder joints at y 1.40, z ±0.21)
    for side, z in (("l", -0.21), ("r", 0.21)):
        A = root.child(Node(f"arm_{side}"))
        sz = 1 if z > 0 else -1
        sh = (0.0, 1.40, z)
        carry = side == "r"
        # upper arm: hangs down, slightly outward; carrying arm hangs straight
        out = 10 if not carry else 4
        if d == 1:
            # one straight limb for the whole arm at LOD1
            A.add(limb(0.07, 0.055, 0.045, 0.57, nl), top, t=sh, rx=-sz * out)
        else:
            A.add(limb(0.065, 0.055, 0.05, 0.30, nl), top, t=sh, rx=-sz * out)
        elbow = (sh[0], sh[1] - 0.30 * np.cos(np.radians(out)), z + sz * 0.30 * np.sin(np.radians(out)))
        sleeve = top if civ else "skin"
        if civ and d == 0:
            A.add(cyl(0.06, 0.062, 0.03, n=n, base_at=-0.02, caps=False), top, t=elbow)                          # cuff
        if carry:
            if d == 0: A.add(limb(0.055, 0.05, 0.045, 0.27, nl), sleeve, t=elbow)
            hand = (elbow[0], elbow[1] - 0.27, elbow[2])
            if d == 0: A.add(blob(0.05, 0, s=(1.0, 1.3, 0.8)), "skin", t=(hand[0], hand[1] - 0.03, hand[2]))
            if civ:
                # suitcase hanging from the hand
                c = (hand[0] + 0.02, hand[1] - 0.26, hand[2] + sz * 0.09)
                A.add(box(0.44, 0.32, 0.13, c=c, taper=0.97), "leather")
                if d == 0:
                    A.add(box(0.46, 0.035, 0.14, c=(c[0], c[1] + 0.09, c[2])), "dark")
                    A.add(box(0.46, 0.035, 0.14, c=(c[0], c[1] - 0.09, c[2])), "dark")
                    A.add(box(0.12, 0.05, 0.03, c=(c[0], c[1] + 0.185, c[2])), "dark")
                    A.add(box(0.05, 0.05, 0.02, c=(c[0] + 0.14, c[1], c[2] + sz * 0.07)), "dark")
            else:
                c = (hand[0] + 0.02, hand[1] - 0.20, hand[2] + sz * 0.11)
                A.add(box(0.42, 0.20, 0.18, c=c), "steel")
                if d == 0:
                    A.add(box(0.44, 0.03, 0.20, c=(c[0], c[1] + 0.11, c[2])), "dark")
                    A.add(box(0.14, 0.05, 0.03, c=(c[0], c[1] + 0.14, c[2])), "dark")
                    A.add(box(0.02, 0.20, 0.19, c=(c[0] - 0.21, c[1], c[2])), "dark")
        else:
            # free arm: forearm swings forward a little (walking), hand relaxed
            if d == 0: A.add(limb(0.055, 0.05, 0.045, 0.27, nl), sleeve, t=elbow, rz=25)
            hx = elbow[0] + 0.27 * np.sin(np.radians(25)) * 1.0; hy = elbow[1] - 0.27 * np.cos(np.radians(25))
            if d == 0: A.add(blob(0.05, 0, s=(1.1, 1.2, 0.8)), "skin", t=(hx, hy - 0.03, elbow[2]))
            if (not civ) and d == 0:
                # wrench in the free hand
                A.add(box(0.30, 0.03, 0.03, c=(hx + 0.06, hy - 0.04, elbow[2])), "steel", rz=-70, pivot=(hx, hy - 0.04, elbow[2]))
                A.add(box(0.06, 0.07, 0.03, c=(hx + 0.21, hy - 0.04, elbow[2])), "steel", rz=-70, pivot=(hx, hy - 0.04, elbow[2]))
    return root
