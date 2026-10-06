# The Bruiser (Enforcer): the Goons' cyborg big brother. Same dark hide, a small head under a
# riveted helmet with a glowing visor slit, massive pauldrons with worn hazard stripes, a riot
# plate chest and a rusty power pack. His right forearm is a triple-barrel cannon (the `Muzzle`
# bone sits at its end), his left hand a huge hydraulic fist.
#
# Rotation axes as in goon.py: limbs hang down, the spine points up; X swings (negative =
# forwards), Z swings sideways (negative = towards +X), Y twists. For spine bones +X leans in.
#
# Usage: blender -b --factory-startup --python scripts/models/bruiser.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402

HEIGHT = 1.9
HIP_X, HIP_Z, KNEE_Z, ANKLE_Z = 0.19, 0.82, 0.46, 0.12
WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z = 0.98, 1.18, 1.56, 1.60
SHOULDER_X, SHOULDER_Z = 0.58, 1.46
ELBOW_Z, WRIST_Z, HAND_Z = 1.16, 0.86, 0.72
# Where the cannon's barrels end, at rest (the arm hanging, barrels down).
MUZZLE_Z = 0.47


def bones():
    b = rrkit.biped(HIP_X, HIP_Z, KNEE_Z, ANKLE_Z, WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z, HEAD_Z + 0.24,
                    SHOULDER_X, SHOULDER_Z, ELBOW_Z, WRIST_Z, HAND_Z, toe=(-0.18, 0.04))
    x = -SHOULDER_X
    b.append(("Muzzle", (x, 0, MUZZLE_Z), (x, 0, MUZZLE_Z - 0.05), "LowerArm.R", False))
    return b


def materials():
    return {
        "hide": rrkit.material("BruiserHide", (74, 82, 52)),
        "steel": rrkit.material("BruiserSteel", (64, 66, 72), rough=0.55, metal=0.7),
        "worn": rrkit.material("BruiserSteelWorn", (104, 100, 92), rough=0.7, metal=0.6),
        "rust": rrkit.material("BruiserRust", (104, 58, 34), rough=0.9),
        "rivets": rrkit.material("BruiserRivets", (150, 148, 140), rough=0.45, metal=0.9),
        "hazard": rrkit.material("BruiserHazard", (170, 128, 30), rough=0.8),
        "visor": rrkit.material("BruiserVisor", (255, 50, 20), rough=0.2, emit=5.0),
        "rubber": rrkit.material("BruiserRubber", (28, 27, 29), rough=0.9),
        "chrome": rrkit.material("BruiserChrome", (170, 172, 178), rough=0.3, metal=0.9),
        "ivory": rrkit.material("BruiserTusk", (214, 200, 160)),
    }


def rivets(body, bone, m, points, size=0.03):
    for p in points:
        body.box(bone, m["rivets"], p, (size, size, size), bevel=size * 0.25)


def build(body, m):
    hi, st, wo, ru = m["hide"], m["steel"], m["worn"], m["rust"]
    # Armoured belt and rubber-clad pelvis.
    body.box("Hips", m["rubber"], (0, 0, 0.88), (0.52, 0.36, 0.20), top=(1.05, 1.0), bevel=0.02)
    body.box("Hips", st, (0, 0, WAIST_Z), (0.58, 0.40, 0.09), bevel=0.015)
    body.box("Hips", wo, (0, -0.20, 0.90), (0.20, 0.05, 0.17), top=(0.75, 1.0), bevel=0.01)
    rivets(body, "Hips", m, [(x, -0.205, WAIST_Z) for x in (-0.22, -0.11, 0.0, 0.11, 0.22)])
    # Ribbed steel abdomen.
    for i, z in enumerate((1.03, 1.09, 1.15)):
        body.box("Spine", st if i % 2 else wo, (0, 0, z), (0.50 + 0.04 * i, 0.36, 0.065),
                 bevel=0.012)
    # Riot-plate chest with a riveted front plate and a worn hazard band.
    body.box("Chest", st, (0, 0, 1.36), (0.80, 0.50, 0.36), top=(1.15, 0.95), bevel=0.04)
    body.box("Chest", wo, (0, -0.255, 1.35), (0.60, 0.04, 0.28), top=(1.12, 1.0), bevel=0.01)
    for i in range(4):
        body.box("Chest", m["hazard"], (-0.2 + 0.13 * i, -0.278, 1.44), (0.04, 0.012, 0.13),
                 rot=(0, 35, 0))
    rivets(body, "Chest", m, [(x, -0.28, z) for x in (-0.27, 0.27) for z in (1.25, 1.33, 1.41)]
           + [(x, -0.28, 1.22) for x in (-0.14, 0.0, 0.14)])
    # Power pack on the back, two rusty exhausts.
    body.box("Chest", st, (0, 0.31, 1.36), (0.46, 0.18, 0.36), bevel=0.02)
    body.box("Chest", ru, (0, 0.40, 1.36), (0.30, 0.02, 0.22))
    for s in (1, -1):
        body.cyl("Chest", ru, (s * 0.15, 0.34, 1.62), 0.045, 0.04, 0.24, seg=6)
        body.cyl("Chest", m["rubber"], (s * 0.15, 0.34, 1.75), 0.05, 0.05, 0.03, seg=6)
    # Thick neck, tiny head: tusked jaw under a riveted helmet with a visor slit.
    body.cyl("Neck", hi, (0, -0.02, NECK_Z + 0.02), 0.13, 0.11, 0.1, seg=8)
    body.box("Head", hi, (0, -0.05, 1.66), (0.21, 0.21, 0.14), bevel=0.02)
    body.box("Head", hi, (0, -0.11, 1.62), (0.25, 0.12, 0.09), front=1.1, bevel=0.015)  # jaw
    for s in (1, -1):
        body.spike("Head", m["ivory"], (s * 0.08, -0.165, 1.62), 0.022, 0.08, rot=(-10, 0, 0))
    body.box("Head", st, (0, -0.02, 1.75), (0.27, 0.28, 0.14), top=(0.85, 0.85), bevel=0.025)
    body.box("Head", st, (0, -0.15, 1.70), (0.25, 0.04, 0.07), bevel=0.01)  # brow guard
    body.box("Head", m["visor"], (0, -0.172, 1.70), (0.19, 0.012, 0.03))
    body.box("Head", ru, (0, -0.02, 1.83), (0.045, 0.26, 0.05), bevel=0.01)  # crest
    rivets(body, "Head", m, [(s * 0.115, -0.15, 1.70) for s in (1, -1)], 0.025)

    def arm(s, sfx):
        x = s * SHOULDER_X
        up, lo, ha = "UpperArm" + sfx, "LowerArm" + sfx, "Hand" + sfx
        # Huge pauldron with rivets and a hazard edge.
        body.ball(up, hi, (x, 0, SHOULDER_Z - 0.02), 0.16)
        body.box(up, st, (x + s * 0.03, 0, 1.56), (0.42, 0.48, 0.20), top=(0.65, 0.75),
                 rot=(0, s * 15, 0), bevel=0.03)
        body.box(up, m["hazard"], (x + s * 0.20, 0, 1.47), (0.04, 0.44, 0.06), rot=(0, s * 15, 0))
        rivets(body, up, m, [(x + s * 0.04, y, 1.665) for y in (-0.14, 0.0, 0.14)], 0.035)
        body.cyl(up, hi, (x, 0, 1.30), 0.13, 0.15, 0.28, seg=8)
        body.box(up, wo, (x, 0, ELBOW_Z), (0.24, 0.26, 0.10), bevel=0.02)  # elbow plate
        if s > 0:
            # Hydraulic forearm and a big steel fist.
            body.cyl(lo, st, (x, 0, 1.01), 0.12, 0.15, 0.26, seg=8, bevel=0.01)
            for a in (-1, 1):
                body.cyl(lo, m["chrome"], (x + a * 0.10, -0.08, 1.0), 0.025, 0.025, 0.28, seg=6)
            body.box(ha, st, (x, -0.01, 0.80), (0.24, 0.26, 0.18), bevel=0.03)
            for i, fx in enumerate((-0.07, 0.0, 0.07)):
                body.box(ha, wo, (x + fx, -0.10, 0.72), (0.06, 0.09, 0.10), bevel=0.012)
            body.box(ha, wo, (x - 0.12, -0.04, 0.78), (0.06, 0.09, 0.10), bevel=0.012)  # thumb
        else:
            # Triple-barrel cannon replacing the forearm, barrels along the bone.
            body.cyl(lo, st, (x, 0, 1.02), 0.15, 0.17, 0.30, seg=8, bevel=0.015)
            body.cyl(lo, ru, (x, 0, 0.86), 0.17, 0.17, 0.05, seg=8)
            body.box(lo, wo, (x - 0.16, 0.0, 0.98), (0.10, 0.20, 0.22), bevel=0.015)  # ammo box
            for a in (0, 120, 240):
                ox = 0.075 * math.cos(math.radians(a))
                oy = 0.075 * math.sin(math.radians(a))
                body.cyl(lo, st, (x + ox, oy, 0.67), 0.05, 0.045, 0.38, seg=8)
                body.cyl(lo, m["rubber"], (x + ox, oy, MUZZLE_Z + 0.02), 0.055, 0.055, 0.04,
                         seg=8)
            body.cyl(lo, wo, (x, 0, 0.70), 0.13, 0.13, 0.05, seg=8)  # barrel clamp

    def leg(s, sfx):
        x = s * HIP_X
        up, lo, ft = "UpperLeg" + sfx, "LowerLeg" + sfx, "Foot" + sfx
        body.cyl(up, m["rubber"], (x, 0, 0.64), 0.15, 0.18, 0.34, seg=8)
        body.box(up, st, (x + s * 0.06, 0, 0.66), (0.10, 0.30, 0.26), bevel=0.015)  # thigh plate
        body.box(lo, wo, (x, -0.12, KNEE_Z), (0.20, 0.08, 0.14), bevel=0.02)  # knee
        body.box(lo, st, (x, 0, 0.29), (0.24, 0.26, 0.30), top=(1.1, 1.05), bevel=0.025)  # greave
        rivets(body, lo, m, [(x, -0.14, z) for z in (0.22, 0.33)])
        body.box(ft, st, (x, -0.07, 0.07), (0.26, 0.40, 0.14), top=(0.9, 0.8), bevel=0.03)
        body.box(ft, m["hazard"], (x, -0.272, 0.06), (0.22, 0.012, 0.05))

    rrkit.both(arm)
    rrkit.both(leg)


# --- clips ---------------------------------------------------------------------------------


def pose(*parts, **extra):
    out = {}
    for p in parts + (extra,):
        for bone, k in p.items():
            out[bone] = {**out.get(bone, {}), **k}
    return out


STANCE = {
    "UpperLeg.L": {"r": (0, 0, -8)},
    "UpperLeg.R": {"r": (0, 0, 8)},
    "Foot.L": {"r": (0, 0, 8)},
    "Foot.R": {"r": (0, 0, -8)},
}
READY = {
    "UpperArm.R": {"r": (-18, 0, 10)},
    "LowerArm.R": {"r": (-12, 0, 0)},
    "UpperArm.L": {"r": (-8, 0, -10)},
    "LowerArm.L": {"r": (-20, 0, 0)},
}
AIM = {
    "UpperArm.R": {"r": (-88, -24, 0)},
    "LowerArm.R": {"r": (0, 0, 0)},
    "UpperArm.L": {"r": (-35, 0, -8)},
    "LowerArm.L": {"r": (-60, 0, 0)},
    "Chest": {"r": (0, -10, 0)},
}


def clips(arm):
    acts = []
    a = pose(STANCE, READY, Chest={"r": (-3, 0, 0)}, Head={"r": (4, 8, 0)})
    b = pose(STANCE, READY, Hips={"l": (0, -0.02, 0)}, Chest={"r": (2, 0, 0)},
             Head={"r": (8, -10, 0)}, **{"UpperArm.L": {"r": (-12, 0, -14)},
                                          "Hand.L": {"r": (-20, 0, 0)}})
    acts.append(rrkit.clip(arm, "Idle", [(0, a), (40, b)]))

    def step(lf, rf, lk, rk, roll, bob):
        return pose(READY, **{
            "Hips": {"r": (0, roll * 0.6, roll), "l": (0, bob, 0)},
            "Spine": {"r": (5, 0, -roll * 0.5)},
            "Head": {"r": (0, 0, -roll * 0.6)},
            "UpperLeg.L": {"r": (lf, 0, -6)},
            "UpperLeg.R": {"r": (rf, 0, 6)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "Foot.L": {"r": (-lf * 0.4, 0, 0)},
            "Foot.R": {"r": (-rf * 0.4, 0, 0)},
            "UpperArm.L": {"r": (-rf * 0.4 - 8, 0, -10)},
        })

    acts.append(rrkit.clip(arm, "Walk", [
        (0, step(-22, 18, 6, 15, 5, -0.04)),
        (9, step(0, -4, 4, 45, 0, 0.02)),
        (18, step(18, -22, 15, 6, -5, -0.04)),
        (27, step(-4, 0, 45, 4, 0, 0.02)),
    ]))

    def charge(lf, rf, lk, rk, bob):
        return pose(**{
            "Hips": {"l": (0, bob, 0)},
            "Spine": {"r": (16, 0, 0)},
            "Head": {"r": (-14, 0, 0)},
            "UpperLeg.L": {"r": (lf, 0, -6)},
            "UpperLeg.R": {"r": (rf, 0, 6)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "UpperArm.R": {"r": (-60, -8, 6)},
            "UpperArm.L": {"r": (rf * 0.8 - 10, 0, -12)},
            "LowerArm.L": {"r": (-55, 0, 0)},
        })

    acts.append(rrkit.clip(arm, "Run", [
        (0, charge(-35, 26, 10, 35, -0.04)),
        (6, charge(-4, -8, 15, 80, 0.03)),
        (12, charge(26, -35, 35, 10, -0.04)),
        (18, charge(-8, -4, 80, 15, 0.03)),
    ]))

    aim = pose(STANCE, AIM, Spine={"r": (-3, 0, 0)})
    kick = pose(STANCE, AIM, Spine={"r": (-12, 0, 0)}, Head={"r": (-10, 0, 0)},
                Hips={"l": (0, -0.03, 0)}, **{"UpperArm.R": {"r": (-104, -24, 0)},
                                              "LowerArm.R": {"r": (-10, 0, 0)}})
    acts.append(rrkit.clip(arm, "Attack", [(0, aim), (3, kick), (12, aim), (24, aim)],
                           loop=False))

    hurt = pose(STANCE, READY, Spine={"r": (-14, 8, 0)}, Head={"r": (-20, -15, 0)},
                **{"UpperArm.L": {"r": (-30, 0, -40)}, "UpperArm.R": {"r": (-30, 0, 30)}})
    acts.append(rrkit.clip(arm, "Pain", [(0, a), (4, hurt), (14, a)], loop=False))

    stagger = pose(STANCE, Spine={"r": (-15, 0, 10)}, Head={"r": (-25, 0, 0)},
                   **{"UpperArm.R": {"r": (-40, 0, 40)}, "UpperArm.L": {"r": (-20, 0, -50)}})
    kneel = pose(**{
        "Hips": {"r": (10, 0, 0), "l": (0, -0.34, 0)},
        "Spine": {"r": (20, 0, 0)},
        "Head": {"r": (25, 0, 0)},
        "UpperLeg.L": {"r": (-85, 0, -8)},
        "UpperLeg.R": {"r": (-30, 0, 8)},
        "LowerLeg.L": {"r": (95, 0, 0)},
        "LowerLeg.R": {"r": (110, 0, 0)},
        "UpperArm.R": {"r": (-20, 0, 15)},
        "UpperArm.L": {"r": (-10, 0, -15)},
    })
    face = pose(**{
        "Hips": {"r": (86, 0, 0), "l": (0, -0.62, -0.2)},
        "Spine": {"r": (4, 0, 0)},
        "Head": {"r": (-20, 25, 0)},
        "UpperLeg.L": {"r": (-6, 0, -10)},
        "UpperLeg.R": {"r": (0, 0, 12)},
        "LowerLeg.L": {"r": (20, 0, 0)},
        "UpperArm.R": {"r": (-170, 0, 30)},
        "UpperArm.L": {"r": (-20, 0, -60)},
    })
    bounce = pose(face, Hips={"r": (80, 0, 0), "l": (0, -0.56, -0.2)})
    acts.append(rrkit.clip(arm, "Death", [(0, a), (6, stagger), (16, kneel), (22, kneel),
                                          (32, face), (36, bounce), (42, face)], loop=False))
    return acts


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Bruiser")
    build(body, materials())
    obj = body.object()
    arm = rrkit.armature("BruiserArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        rrkit.preview(prev, "bruiser", arm, HEIGHT, acts)


if __name__ == "__main__":
    main()
