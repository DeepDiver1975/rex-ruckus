# The Gnasher (Slasher): a hunched, digitigrade alien that is mostly mouth. A bulging, veined
# cranium with six glowing eyes, a huge toothy maw on its own `Jaw` bone, scythe claws, back
# spines and a whip tail; sickly dark purple hide.
#
# Rotation axes as in goon.py for the biped bones. `Jaw` points forwards: +X opens it.
# `Tail1`/`Tail2` point backwards: Z swings them sideways. The hunch is part of every pose.
#
# Usage: blender -b --factory-startup --python scripts/models/gnasher.py -- OUT.glb [--preview DIR]
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mathutils import Matrix, Vector  # noqa: E402

import rrkit  # noqa: E402

HEIGHT = 1.6
HIP_X, HIP_Z, KNEE_Z, ANKLE_Z = 0.13, 0.72, 0.45, 0.16
WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z = 0.86, 1.04, 1.30, 1.36
SHOULDER_X, SHOULDER_Z = 0.29, 1.24
ELBOW_Z, WRIST_Z, HAND_Z = 0.99, 0.75, 0.67
JAW = (0, -0.03, 1.38)
# Scythe claws: from the back of the hand, curving down and forwards.
CLAW_DIR = Vector((0, -0.42, -0.91))
CLAW_LEN = 0.34
CLAW_ROOT_Z = 0.69


def bones():
    b = rrkit.biped(HIP_X, HIP_Z, KNEE_Z, ANKLE_Z, WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z, HEAD_Z + 0.3,
                    SHOULDER_X, SHOULDER_Z, ELBOW_Z, WRIST_Z, HAND_Z, toe=(-0.18, 0.03))
    b += [
        ("Jaw", JAW, (0, -0.25, 1.36), "Head", True),
        ("Tail1", (0, 0.12, 0.76), (0, 0.38, 0.62), "Hips", True),
        ("Tail2", (0, 0.38, 0.62), (0, 0.66, 0.56), "Tail1", True),
    ]
    # The right claw's tip: where the slash lands (models.ron tip, enemies.ron muzzle).
    tip = Vector((-SHOULDER_X, -0.04, CLAW_ROOT_Z)) + CLAW_DIR * CLAW_LEN
    b.append(("Claw", tuple(tip), tuple(tip + CLAW_DIR * 0.05), "Hand.R", False))
    return b


def materials():
    return {
        "hide": rrkit.material("GnasherHide", (96, 72, 106)),
        "belly": rrkit.material("GnasherBelly", (122, 104, 112)),
        "vein": rrkit.material("GnasherVein", (130, 38, 72)),
        "glow": rrkit.material("GnasherGlow", (130, 255, 90), rough=0.3, emit=5.0),
        "teeth": rrkit.material("GnasherTeeth", (226, 214, 176), rough=0.4),
        "maw": rrkit.material("GnasherMaw", (110, 16, 28)),
        "claw": rrkit.material("GnasherClaw", (60, 50, 46), rough=0.35),
    }


def build(body, m):
    hi, be, ve = m["hide"], m["belly"], m["vein"]
    # Narrow pelvis, whip tail ending in a barb.
    body.box("Hips", hi, (0, 0.02, 0.78), (0.30, 0.24, 0.16), top=(1.1, 1.0), bevel=0.02)
    body.between("Tail1", hi, (0, 0.10, 0.77), (0, 0.38, 0.62), 0.08, 0.055, seg=6)
    body.between("Tail2", hi, (0, 0.38, 0.62), (0, 0.64, 0.56), 0.055, 0.025, seg=6)
    body.spike_to("Tail2", m["claw"], (0, 0.63, 0.56), (0, 0.80, 0.58), 0.035)
    # Ribbed belly and a narrow, sinewy chest with spines down the back.
    body.box("Spine", hi, (0, 0.01, 0.95), (0.32, 0.25, 0.20), top=(1.3, 1.15), bevel=0.02)
    body.box("Spine", be, (0, -0.115, 0.95), (0.20, 0.05, 0.18), top=(1.2, 1.0))
    for s in (1, -1):
        for z in (0.92, 0.98):
            body.box("Spine", be, (s * 0.15, -0.06, z), (0.04, 0.12, 0.02), rot=(0, 0, s * 20))
    body.box("Chest", hi, (0, 0.02, 1.16), (0.46, 0.32, 0.26), top=(1.15, 1.0), bevel=0.025)
    body.box("Chest", be, (0, -0.135, 1.13), (0.24, 0.04, 0.20), top=(1.3, 1.0), bevel=0.01)
    for i, z in enumerate((1.02, 1.12, 1.22, 1.30)):
        body.spike_to("Chest" if z > CHEST_Z else "Spine", m["claw"], (0, 0.15, z),
                      (0, 0.30 + 0.02 * i, z + 0.10), 0.035)
    # Neck and head: a big veined cranium over a snout full of teeth.
    body.cyl("Neck", hi, (0, -0.02, 1.34), 0.09, 0.08, 0.10, seg=6)
    hz = HEAD_Z
    # The head is drawn 1.2x around the neck joint: mostly mouth.
    pivot = Vector((0, 0, HEAD_Z))
    body.xf = Matrix.Translation(pivot) @ Matrix.Scale(1.2, 4) @ Matrix.Translation(-pivot)
    body.ball("Head", hi, (0, 0.05, hz + 0.17), 0.17, scale=(1.0, 1.3, 1.0))  # cranium
    for p, r in (((0.07, -0.02, hz + 0.29), (0, 25, 10)), ((-0.06, 0.08, hz + 0.31), (0, -30, 60)),
                 ((0.12, 0.10, hz + 0.20), (20, 60, 0)), ((-0.12, -0.04, hz + 0.21), (0, -60, -20)),
                 ((0.0, 0.17, hz + 0.25), (40, 0, 90))):
        body.box("Head", ve, p, (0.02, 0.12, 0.02), rot=r)
    body.box("Head", hi, (0, -0.13, hz + 0.08), (0.22, 0.24, 0.10), top=(0.85, 1.0), front=0.8,
             bevel=0.015)  # upper jaw
    body.box("Head", m["maw"], (0, -0.12, hz + 0.02), (0.18, 0.20, 0.05))
    for s in (1, -1):
        for p in ((0.10, -0.14, hz + 0.12), (0.13, -0.09, hz + 0.16), (0.14, -0.03, hz + 0.11)):
            body.box("Head", m["glow"], (s * p[0], p[1], p[2]), (0.035, 0.035, 0.03),
                     rot=(0, 0, s * 30))
    # Upper teeth, round the front and sides of the snout, pointing down.
    teeth = [(x, -0.245) for x in (-0.06, -0.02, 0.02, 0.06)]
    teeth += [(s * 0.095, y) for s in (1, -1) for y in (-0.20, -0.14, -0.08)]
    for x, y in teeth:
        body.spike("Head", m["teeth"], (x, y, hz + 0.035), 0.016, 0.06, rot=(180, 0, 0))
    # Lower jaw with a bigger underbite of teeth pointing up.
    body.box("Jaw", hi, (0, -0.13, hz - 0.03), (0.24, 0.25, 0.07), front=0.85, bevel=0.012)
    body.box("Jaw", m["maw"], (0, -0.12, hz + 0.005), (0.18, 0.20, 0.02))
    for x, y in teeth:
        body.spike("Jaw", m["teeth"], (x * 1.08, y - 0.01, hz), 0.018, 0.075)
    for s in (1, -1):
        body.spike("Jaw", m["teeth"], (s * 0.07, -0.25, hz - 0.005), 0.025, 0.12, rot=(-10, 0, 0))
    body.xf = Matrix.Identity(4)

    def arm(s, sfx):
        x = s * SHOULDER_X
        up, lo, ha = "UpperArm" + sfx, "LowerArm" + sfx, "Hand" + sfx
        body.ball(up, hi, (x, 0, SHOULDER_Z - 0.01), 0.09)
        body.cyl(up, hi, (x, 0, 1.12), 0.075, 0.09, 0.24, seg=6)
        body.spike_to(up, m["claw"], (x + s * 0.04, 0.03, SHOULDER_Z + 0.02),
                      (x + s * 0.10, 0.10, SHOULDER_Z + 0.14), 0.03)  # shoulder spur
        body.cyl(lo, hi, (x, 0, 0.87), 0.06, 0.085, 0.24, seg=6)
        body.box(ha, hi, (x, -0.01, 0.71), (0.09, 0.10, 0.09), bevel=0.015)
        # Three scythe claws, each in two segments for a curve.
        for dx in (-0.035, 0.0, 0.035):
            a = Vector((x + dx, -0.04, CLAW_ROOT_Z))
            mid = a + CLAW_DIR * (CLAW_LEN * 0.55) + Vector((dx * 0.6, 0, 0))
            tip = a + CLAW_DIR * CLAW_LEN + Vector((dx * 0.9, -0.05, 0.04))
            body.between(ha, m["claw"], a, mid, 0.022, 0.016, seg=4)
            body.spike_to(ha, m["claw"], mid, tip, 0.016)

    def leg(s, sfx):
        x = s * HIP_X
        up, lo, ft = "UpperLeg" + sfx, "LowerLeg" + sfx, "Foot" + sfx
        # Digitigrade: thigh forwards to the knee, shin back to a high hock, long foot forwards.
        body.ball(up, hi, (x, -0.02, 0.66), 0.10, scale=(1.0, 1.1, 1.3))
        body.between(up, hi, (x, 0, HIP_Z), (x, -0.09, KNEE_Z), 0.09, 0.07)
        body.between(lo, hi, (x, -0.09, KNEE_Z), (x, 0.06, ANKLE_Z + 0.04), 0.06, 0.045)
        body.between(ft, hi, (x, 0.06, ANKLE_Z + 0.04), (x, -0.10, 0.04), 0.045, 0.04)
        for dx in (-0.04, 0.0, 0.04):
            body.spike_to(ft, m["claw"], (x + dx, -0.10, 0.04), (x + dx * 1.6, -0.22, 0.01), 0.02)
        body.spike_to(ft, m["claw"], (x, 0.06, ANKLE_Z + 0.03), (x, 0.13, ANKLE_Z - 0.02), 0.02)

    rrkit.both(arm)
    rrkit.both(leg)


# --- clips ---------------------------------------------------------------------------------


def pose(*parts, **extra):
    out = {}
    for p in parts + (extra,):
        for bone, k in p.items():
            out[bone] = {**out.get(bone, {}), **k}
    return out


HUNCH = {
    "Spine": {"r": (24, 0, 0)},
    "Chest": {"r": (14, 0, 0)},
    "Neck": {"r": (-18, 0, 0)},
    "Head": {"r": (-22, 0, 0)},
    "UpperArm.L": {"r": (-25, 0, -10)},
    "UpperArm.R": {"r": (-25, 0, 10)},
    "LowerArm.L": {"r": (-35, 0, 0)},
    "LowerArm.R": {"r": (-35, 0, 0)},
    "UpperLeg.L": {"r": (-20, 0, -6)},
    "UpperLeg.R": {"r": (-20, 0, 6)},
    "LowerLeg.L": {"r": (30, 0, 0)},
    "LowerLeg.R": {"r": (30, 0, 0)},
    "Foot.L": {"r": (-10, 0, 0)},
    "Foot.R": {"r": (-10, 0, 0)},
    "Hips": {"l": (0, -0.05, 0)},
    "Tail1": {"r": (10, 0, 0)},
}


def clips(arm):
    acts = []
    a = pose(HUNCH, Tail1={"r": (10, 0, 14)}, Tail2={"r": (0, 0, 12)}, Head={"r": (-22, 10, 0)})
    b = pose(HUNCH, Tail1={"r": (10, 0, -14)}, Tail2={"r": (0, 0, -12)},
             Head={"r": (-26, -14, 8)}, Jaw={"r": (14, 0, 0)}, Chest={"r": (18, 0, 0)},
             **{"Hand.L": {"r": (-20, 0, 0)}, "Hand.R": {"r": (-20, 0, 0)}})
    c = pose(a, Jaw={"r": (4, 0, 0)}, Head={"r": (-18, 4, -6)})
    acts.append(rrkit.clip(arm, "Idle", [(0, a), (14, b), (20, c), (34, b)]))

    def stalk(lf, rf, lk, rk, sway, bob):
        return pose(HUNCH, **{
            "Hips": {"r": (0, sway, 0), "l": (0, bob - 0.05, 0)},
            "UpperLeg.L": {"r": (lf - 20, 0, -6)},
            "UpperLeg.R": {"r": (rf - 20, 0, 6)},
            "LowerLeg.L": {"r": (lk + 30, 0, 0)},
            "LowerLeg.R": {"r": (rk + 30, 0, 0)},
            "UpperArm.L": {"r": (-25 + rf * 0.6, 0, -10)},
            "UpperArm.R": {"r": (-25 + lf * 0.6, 0, 10)},
            "Tail1": {"r": (10, 0, -sway * 2)},
            "Tail2": {"r": (0, 0, -sway * 2)},
        })

    acts.append(rrkit.clip(arm, "Walk", [
        (0, stalk(-25, 20, 5, 25, 8, 0.0)),
        (6, stalk(0, -5, 0, 55, 0, 0.03)),
        (12, stalk(20, -25, 25, 5, -8, 0.0)),
        (18, stalk(-5, 0, 55, 0, 0, 0.03)),
    ]))

    def bound(lf, rf, lk, rk, bob, tail):
        return pose(HUNCH, **{
            "Hips": {"l": (0, bob - 0.08, 0)},
            "Spine": {"r": (40, 0, 0)},
            "Neck": {"r": (-30, 0, 0)},
            "Head": {"r": (-30, 0, 0)},
            "Jaw": {"r": (22, 0, 0)},
            "UpperLeg.L": {"r": (lf, 0, -6)},
            "UpperLeg.R": {"r": (rf, 0, 6)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "UpperArm.L": {"r": (40, 0, -12)},
            "UpperArm.R": {"r": (40, 0, 12)},
            "LowerArm.L": {"r": (-20, 0, 0)},
            "LowerArm.R": {"r": (-20, 0, 0)},
            "Tail1": {"r": (tail, 0, 0)},
            "Tail2": {"r": (tail, 0, 0)},
        })

    acts.append(rrkit.clip(arm, "Run", [
        (0, bound(-55, 25, 20, 40, -0.02, 15)),
        (4, bound(-15, -25, 30, 90, 0.05, -10)),
        (8, bound(25, -55, 40, 20, -0.02, 15)),
        (12, bound(-25, -15, 90, 30, 0.05, -10)),
    ]))

    # Attack: rear up with claws high and the maw gaping, then rake down and forwards.
    rear = pose(HUNCH, **{
        "Spine": {"r": (0, 0, 0)},
        "Chest": {"r": (-8, 0, 0)},
        "Head": {"r": (-30, 0, 0)},
        "Jaw": {"r": (45, 0, 0)},
        "UpperArm.L": {"r": (-150, 0, -25)},
        "UpperArm.R": {"r": (-150, 0, 25)},
        "LowerArm.L": {"r": (-30, 0, 0)},
        "LowerArm.R": {"r": (-30, 0, 0)},
    })
    slash = pose(HUNCH, **{
        "Spine": {"r": (28, 0, 0)},
        "Chest": {"r": (16, 0, 0)},
        "Neck": {"r": (-30, 0, 0)},
        "Head": {"r": (-28, 0, 0)},
        "Jaw": {"r": (38, 0, 0)},
        "UpperArm.L": {"r": (-95, 0, 8)},
        "UpperArm.R": {"r": (-95, 0, -8)},
        "LowerArm.L": {"r": (-5, 0, 0)},
        "LowerArm.R": {"r": (-5, 0, 0)},
        "Hand.L": {"r": (-35, 0, 0)},
        "Hand.R": {"r": (-35, 0, 0)},
        "UpperLeg.L": {"r": (-50, 0, -6)},
        "LowerLeg.L": {"r": (50, 0, 0)},
    })
    acts.append(rrkit.clip(arm, "Attack", [(0, a), (5, rear), (9, slash), (14, slash), (22, a)],
                           loop=False))

    hurt = pose(HUNCH, Spine={"r": (0, 0, 0)}, Head={"r": (-50, 15, 0)}, Jaw={"r": (40, 0, 0)},
                **{"UpperArm.L": {"r": (-60, 0, -50)}, "UpperArm.R": {"r": (-60, 0, 50)},
                   "Tail1": {"r": (-30, 0, 0)}})
    acts.append(rrkit.clip(arm, "Pain", [(0, a), (3, hurt), (11, a)], loop=False))

    shriek = pose(hurt, Head={"r": (-60, 0, 0)}, Jaw={"r": (55, 0, 0)}, Chest={"r": (-15, 0, 0)},
                  **{"UpperArm.L": {"r": (-120, 0, -40)}, "UpperArm.R": {"r": (-120, 0, 40)}})
    down = pose(**{
        "Hips": {"r": (0, 0, 85), "l": (0, -0.58, 0)},
        "Spine": {"r": (20, 0, 0)},
        "Head": {"r": (-10, 0, -20)},
        "Jaw": {"r": (35, 0, 0)},
        "UpperLeg.L": {"r": (-70, 0, 0)},
        "UpperLeg.R": {"r": (-40, 0, 0)},
        "LowerLeg.L": {"r": (90, 0, 0)},
        "LowerLeg.R": {"r": (60, 0, 0)},
        "UpperArm.L": {"r": (-90, 0, -20)},
        "UpperArm.R": {"r": (-40, 0, 30)},
        "Tail1": {"r": (0, 0, 30)},
        "Tail2": {"r": (0, 0, 30)},
    })
    twitch = pose(down, Head={"r": (-10, 0, -10)}, **{"Hand.L": {"r": (-40, 0, 0)},
                                                       "Tail2": {"r": (0, 0, 50)}})
    acts.append(rrkit.clip(arm, "Death", [(0, a), (5, shriek), (12, shriek), (20, down),
                                          (26, twitch), (32, down)], loop=False))
    return acts


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Gnasher")
    build(body, materials())
    obj = body.object()
    arm = rrkit.armature("GnasherArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        rrkit.preview(prev, "gnasher", arm, HEIGHT, acts)


if __name__ == "__main__":
    main()
