# The Warlord (Boss): a three-metre alien tyrant in riveted battle armour, in the spirit of the
# hulking bosses of 90s shooters. Sickly hide, a barrel chest with a glowing furnace grille, a
# small tusked head sunk between spiked pauldrons under a horned helm with burning eye slits,
# clawed three-toed feet. A rocket pod rides above his left shoulder (the `Rockets` bone at the
# tube mouths, phase one's volleys); his right forearm is a six-barrel rotary gun (the `Muzzle`
# bone at the barrel tips, phase two's minigun); his left hand is a clawed gauntlet.
#
# Rotation axes as in goon.py: limbs hang down, the spine points up; X swings (negative =
# forwards), Z swings sideways (negative = towards +X), Y twists. For spine bones +X leans in.
#
# Usage: blender -b --factory-startup --python scripts/models/warlord.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402
from mathutils import Matrix, Vector  # noqa: E402

HEIGHT = 3.0
HIP_X, HIP_Z, KNEE_Z, ANKLE_Z = 0.34, 1.30, 0.78, 0.20
WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z = 1.55, 1.80, 2.45, 2.50
SHOULDER_X, SHOULDER_Z = 0.92, 2.30
ELBOW_Z, WRIST_Z, HAND_Z = 1.80, 1.36, 1.10
# Where the rotary gun's barrels end, at rest (the arm hanging, barrels down).
MUZZLE_Z = 0.94
# The rocket pod above the left shoulder: its centre and where its tubes open (facing -Y).
POD = (0.76, 0.16, 2.90)
POD_FRONT = POD[1] - 0.34


def bones():
    b = rrkit.biped(HIP_X, HIP_Z, KNEE_Z, ANKLE_Z, WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z, HEAD_Z + 0.36,
                    SHOULDER_X, SHOULDER_Z, ELBOW_Z, WRIST_Z, HAND_Z, toe=(-0.30, 0.06))
    x = -SHOULDER_X
    b.append(("Muzzle", (x, 0, MUZZLE_Z), (x, 0, MUZZLE_Z - 0.08), "LowerArm.R", False))
    # Pointing forwards, tilted down a little (see rrkit.armature).
    px, _, pz = POD
    b.append(("Rockets", (px, POD_FRONT, pz), (px, POD_FRONT - 0.1, pz - 0.01), "Chest", False))
    return b


def materials():
    return {
        "hide": rrkit.material("WarlordHide", (98, 112, 72)),
        "flesh": rrkit.material("WarlordFlesh", (128, 58, 52), rough=0.6),
        "steel": rrkit.material("WarlordSteel", (56, 58, 66), rough=0.5, metal=0.75),
        "worn": rrkit.material("WarlordSteelWorn", (112, 106, 96), rough=0.7, metal=0.6),
        "rust": rrkit.material("WarlordRust", (112, 58, 32), rough=0.9),
        "rivets": rrkit.material("WarlordRivets", (156, 152, 142), rough=0.45, metal=0.9),
        "hazard": rrkit.material("WarlordHazard", (184, 136, 28), rough=0.8),
        "eyes": rrkit.material("WarlordEyes", (255, 40, 16), rough=0.2, emit=6.0),
        "furnace": rrkit.material("WarlordFurnace", (255, 120, 20), rough=0.3, emit=4.0),
        "rubber": rrkit.material("WarlordRubber", (26, 25, 28), rough=0.9),
        "chrome": rrkit.material("WarlordChrome", (172, 174, 180), rough=0.3, metal=0.9),
        "bone": rrkit.material("WarlordBone", (218, 204, 164)),
        "warhead": rrkit.material("WarlordWarhead", (190, 30, 24), rough=0.5),
    }


def rivets(body, bone, m, points, size=0.045):
    for p in points:
        body.box(bone, m["rivets"], p, (size, size, size), bevel=size * 0.25)


def build(body, m):
    hi, st, wo, ru, bo = m["hide"], m["steel"], m["worn"], m["rust"], m["bone"]
    # Pelvis, a heavy belt with a skull buckle, armoured kilt plates front and back.
    body.box("Hips", m["rubber"], (0, 0, 1.40), (0.84, 0.58, 0.34), top=(1.08, 1.0), bevel=0.03)
    body.box("Hips", st, (0, 0, WAIST_Z), (0.98, 0.70, 0.15), bevel=0.025)
    rivets(body, "Hips", m, [(x, -0.355, WAIST_Z) for x in (-0.40, -0.27, 0.27, 0.40)])
    body.ball("Hips", bo, (0, -0.37, 1.53), 0.11, scale=(1.0, 0.55, 1.05))
    for s in (1, -1):
        body.box("Hips", m["rubber"], (s * 0.04, -0.425, 1.555), (0.04, 0.02, 0.035))
    body.box("Hips", bo, (0, -0.39, 1.46), (0.10, 0.05, 0.05), bevel=0.01)  # its jaw
    for x in (-0.28, 0.0, 0.28):
        body.box("Hips", wo if x else st, (x, -0.33, 1.27), (0.26, 0.05, 0.36), top=(1.15, 1.0),
                 rot=(-8, 0, 0), bevel=0.012)
        body.box("Hips", st, (x, 0.33, 1.29), (0.28, 0.05, 0.34), top=(1.1, 1.0), rot=(8, 0, 0),
                 bevel=0.012)
    # Ribbed hide belly bound by steel bands, with hoses running to the chest.
    body.cyl("Spine", hi, (0, 0, 1.69), 0.40, 0.46, 0.30, seg=10)
    for z in (1.64, 1.76):
        body.cyl("Spine", st, (0, 0, z), 0.43, 0.46, 0.05, seg=10)
    for s in (1, -1):
        body.between("Spine", m["rubber"], (s * 0.22, -0.40, 1.60), (s * 0.30, -0.44, 1.86),
                     0.04, 0.04)
    # Barrel chest of hide under a riveted cuirass with a furnace grille, a hump of trapezius
    # around the neck.
    body.ball("Chest", hi, (0, 0.02, 2.14), 0.5, scale=(1.32, 0.92, 0.84), sub=2)
    body.ball("Chest", hi, (0, 0.06, 2.44), 0.36, scale=(1.55, 1.0, 0.62))
    body.box("Chest", st, (0, -0.17, 2.10), (0.96, 0.56, 0.62), top=(1.12, 0.95), bevel=0.05)
    body.box("Chest", wo, (0, -0.44, 2.12), (0.86, 0.05, 0.52), top=(1.15, 1.0), bevel=0.015)
    body.box("Chest", m["rubber"], (0, -0.47, 2.08), (0.36, 0.03, 0.24), bevel=0.01)
    for z in (2.00, 2.07, 2.14):
        body.box("Chest", m["furnace"], (0, -0.485, z), (0.30, 0.012, 0.03))
    rivets(body, "Chest", m, [(x, -0.475, z) for x in (-0.36, 0.36) for z in (1.94, 2.08, 2.22)]
           + [(x, -0.475, 2.32) for x in (-0.22, 0.0, 0.22)])
    for i in range(6):
        body.box("Chest", m["hazard"], (-0.30 + 0.12 * i, -0.455, 2.37), (0.05, 0.012, 0.09),
                 rot=(0, 35, 0))
    for s in (1, -1):  # harness straps over the shoulders
        body.box("Chest", m["rubber"], (s * 0.36, -0.02, 2.47), (0.12, 0.84, 0.06),
                 rot=(0, s * 18, 0))
    # Power pack on the back: two smoke stacks with glowing throats and a row of spines.
    body.box("Chest", st, (0, 0.52, 2.12), (0.78, 0.28, 0.60), bevel=0.03)
    body.box("Chest", ru, (0, 0.665, 2.12), (0.56, 0.02, 0.40))
    for s in (1, -1):
        body.cyl("Chest", ru, (s * 0.24, 0.56, 2.52), 0.07, 0.06, 0.42, seg=6)
        body.cyl("Chest", m["rubber"], (s * 0.24, 0.56, 2.74), 0.075, 0.075, 0.04, seg=6)
        body.cyl("Chest", m["furnace"], (s * 0.24, 0.56, 2.755), 0.05, 0.05, 0.02, seg=6)
    for z in (1.92, 2.12, 2.32):
        body.spike_to("Chest", bo, (0, 0.66, z), (0, 0.84, z + 0.06), 0.05)
    # The rocket pod on a mount behind the left shoulder: six tubes with warheads showing.
    px, py, pz = POD
    body.box("Chest", wo, (px - 0.12, py + 0.30, pz - 0.30), (0.16, 0.16, 0.36), bevel=0.02)
    body.box("Chest", st, POD, (0.52, 0.68, 0.40), bevel=0.04)
    body.box("Chest", m["hazard"], (px + 0.265, py, pz), (0.012, 0.56, 0.08))
    body.box("Chest", m["hazard"], (px - 0.265, py, pz), (0.012, 0.56, 0.08))
    body.box("Chest", wo, (px, py + 0.36, pz), (0.44, 0.06, 0.32), bevel=0.015)  # exhaust plate
    for c in (-1, 0, 1):
        for r in (-1, 1):
            tx, tz = px + c * 0.15, pz + r * 0.095
            body.cyl("Chest", m["rubber"], (tx, POD_FRONT + 0.03, tz), 0.065, 0.065, 0.06,
                     rot=(90, 0, 0), seg=8)
            body.spike("Chest", m["warhead"], (tx, POD_FRONT + 0.03, tz), 0.05, 0.08,
                       rot=(90, 0, 0), seg=6)
    rivets(body, "Chest", m, [(px + c * 0.2, POD_FRONT + 0.01, pz + 0.18) for c in (-1, 1)], 0.04)
    # Thick neck, the head low between the shoulders: tusked jaw, horned helm, burning eye
    # slits. The head is drawn at 1.25x around its base.
    body.cyl("Neck", hi, (0, -0.04, NECK_Z + 0.04), 0.24, 0.20, 0.18, seg=8)
    c = Vector((0, -0.08, HEAD_Z))
    body.xf = Matrix.Translation(c) @ Matrix.Scale(1.25, 4) @ Matrix.Translation(-c)
    body.box("Head", hi, (0, -0.08, 2.64), (0.32, 0.32, 0.24), bevel=0.03)
    body.box("Head", hi, (0, -0.16, 2.56), (0.36, 0.20, 0.12), front=1.1, bevel=0.02)  # jaw
    body.box("Head", m["flesh"], (0, -0.265, 2.575), (0.24, 0.02, 0.035))  # snarl
    for s in (1, -1):
        body.spike("Head", bo, (s * 0.12, -0.25, 2.55), 0.035, 0.15, rot=(-14, s * -12, 0))
    body.box("Head", st, (0, -0.06, 2.80), (0.42, 0.44, 0.18), top=(0.8, 0.8), bevel=0.035)
    body.box("Head", st, (0, -0.255, 2.71), (0.38, 0.06, 0.09), bevel=0.012)  # brow guard
    for s in (1, -1):
        body.box("Head", m["eyes"], (s * 0.075, -0.29, 2.69), (0.09, 0.012, 0.03),
                 rot=(0, s * -12, 0))
    body.spike("Head", ru, (0, -0.10, 2.88), 0.05, 0.14, rot=(-25, 0, 0))  # nose spike
    rivets(body, "Head", m, [(s * 0.17, -0.27, 2.71) for s in (1, -1)], 0.035)
    for s in (1, -1):
        # Ram-ish horns: out, up and forward, in three tapering segments.
        p0, p1, p2 = (s * 0.18, 0.0, 2.82), (s * 0.40, 0.06, 2.92), (s * 0.50, 0.0, 3.08)
        body.between("Head", bo, p0, p1, 0.07, 0.06)
        body.between("Head", bo, p1, p2, 0.06, 0.045)
        body.spike_to("Head", bo, p2, (s * 0.46, -0.14, 3.20), 0.045, seg=6)
    body.xf = Matrix.Identity(4)

    def arm(s, sfx):
        x = s * SHOULDER_X
        up, lo, ha = "UpperArm" + sfx, "LowerArm" + sfx, "Hand" + sfx
        # Massive spiked pauldron.
        body.ball(up, hi, (x, 0, SHOULDER_Z - 0.02), 0.24)
        body.box(up, st, (x + s * 0.06, 0, 2.48), (0.62, 0.72, 0.30), top=(0.62, 0.72),
                 rot=(0, s * 16, 0), bevel=0.04)
        body.box(up, m["hazard"], (x + s * 0.31, 0, 2.36), (0.05, 0.66, 0.08), rot=(0, s * 16, 0))
        rivets(body, up, m, [(x + s * 0.04, y, 2.635) for y in (-0.22, 0.22)], 0.05)
        for y in (-0.18, 0.0, 0.18):
            body.spike_to(up, bo, (x + s * 0.10, y, 2.60), (x + s * 0.24, y, 2.88), 0.06)
        body.cyl(up, hi, (x, 0, 2.06), 0.19, 0.22, 0.42, seg=8)
        body.box(up, wo, (x, 0, ELBOW_Z), (0.36, 0.38, 0.14), bevel=0.03)  # elbow plate
        body.spike_to(up, bo, (x, 0.17, ELBOW_Z), (x, 0.36, ELBOW_Z - 0.04), 0.05)
        if s > 0:
            # Armoured forearm with spikes and a clawed gauntlet.
            body.cyl(lo, st, (x, 0, 1.57), 0.19, 0.23, 0.42, seg=8, bevel=0.015)
            body.cyl(lo, ru, (x, 0, 1.40), 0.20, 0.20, 0.06, seg=8)
            for z in (1.48, 1.64):
                body.spike_to(lo, bo, (x + 0.20, 0, z), (x + 0.36, 0, z + 0.06), 0.04)
            body.box(ha, st, (x, -0.02, 1.24), (0.34, 0.36, 0.26), bevel=0.04)
            for fx in (-0.10, 0.0, 0.10):
                body.box(ha, wo, (x + fx, -0.14, 1.13), (0.08, 0.12, 0.12), bevel=0.015)
                body.spike_to(ha, bo, (x + fx, -0.15, 1.07), (x + fx * 1.3, -0.24, 0.90), 0.035)
            body.box(ha, wo, (x - 0.17, -0.07, 1.18), (0.08, 0.12, 0.12), bevel=0.015)  # thumb
            body.spike_to(ha, bo, (x - 0.18, -0.10, 1.12), (x - 0.22, -0.20, 0.98), 0.03)
        else:
            # Rotary gun replacing the forearm: housing, ammo drum, six barrels along the bone.
            body.cyl(lo, st, (x, 0, 1.58), 0.22, 0.25, 0.42, seg=10, bevel=0.015)
            body.cyl(lo, ru, (x, 0, 1.36), 0.23, 0.23, 0.06, seg=10)
            body.cyl(lo, wo, (x - 0.30, 0.04, 1.60), 0.17, 0.17, 0.20, rot=(0, 90, 0), seg=10)
            body.cyl(lo, m["hazard"], (x - 0.41, 0.04, 1.60), 0.10, 0.10, 0.02, rot=(0, 90, 0),
                     seg=10)
            body.box(lo, m["rubber"], (x - 0.20, -0.10, 1.48), (0.06, 0.06, 0.20))  # feed
            body.cyl(lo, st, (x, 0, 1.30), 0.10, 0.10, 0.10, seg=8)  # spindle
            for a in range(0, 360, 60):
                ox = 0.10 * math.cos(math.radians(a))
                oy = 0.10 * math.sin(math.radians(a))
                body.cyl(lo, m["chrome"], (x + ox, oy, 1.10), 0.038, 0.034, 0.36, seg=6)
                body.cyl(lo, m["rubber"], (x + ox, oy, MUZZLE_Z + 0.01), 0.04, 0.04, 0.03, seg=6)
            for z in (1.20, 0.99):
                body.cyl(lo, wo, (x, 0, z), 0.16, 0.16, 0.04, seg=10)  # barrel clamps

    def leg(s, sfx):
        x = s * HIP_X
        up, lo, ft = "UpperLeg" + sfx, "LowerLeg" + sfx, "Foot" + sfx
        body.cyl(up, hi, (x, 0, 1.04), 0.23, 0.28, 0.54, seg=8)
        body.box(up, st, (x + s * 0.10, 0, 1.06), (0.14, 0.44, 0.40), bevel=0.02)  # thigh plate
        body.box(lo, wo, (x, -0.21, KNEE_Z + 0.02), (0.30, 0.10, 0.22), bevel=0.03)  # knee
        body.spike_to(lo, bo, (x, -0.26, KNEE_Z + 0.04), (x, -0.42, KNEE_Z + 0.10), 0.06)
        body.box(lo, st, (x, 0, 0.48), (0.38, 0.40, 0.48), top=(1.15, 1.1), bevel=0.035)  # greave
        body.box(lo, m["hazard"], (x, -0.215, 0.62), (0.32, 0.012, 0.06))
        rivets(body, lo, m, [(x + dx, -0.215, 0.40) for dx in (-0.1, 0.1)])
        body.spike_to(lo, bo, (x, 0.20, 0.32), (x, 0.42, 0.18), 0.05)  # heel spur
        body.box(ft, st, (x, -0.10, 0.10), (0.42, 0.56, 0.20), top=(0.85, 0.8), bevel=0.04)
        for fx in (-0.14, 0.0, 0.14):
            body.box(ft, wo, (x + fx, -0.38, 0.06), (0.11, 0.14, 0.11), bevel=0.015)
            body.spike_to(ft, bo, (x + fx, -0.44, 0.06), (x + fx * 1.25, -0.62, 0.01), 0.045)

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
    "UpperLeg.L": {"r": (0, 0, -9)},
    "UpperLeg.R": {"r": (0, 0, 9)},
    "Foot.L": {"r": (0, 0, 9)},
    "Foot.R": {"r": (0, 0, -9)},
}
READY = {
    "UpperArm.R": {"r": (-20, 0, 12)},
    "LowerArm.R": {"r": (-25, 0, 0)},
    "UpperArm.L": {"r": (-10, 0, -12)},
    "LowerArm.L": {"r": (-25, 0, 0)},
}
AIM = {
    "UpperArm.R": {"r": (-62, -20, 4)},
    "LowerArm.R": {"r": (-28, 0, 0)},
    "UpperArm.L": {"r": (-24, 0, -26)},
    "LowerArm.L": {"r": (-38, 0, 0)},
    "Chest": {"r": (4, -8, 0)},
    "Head": {"r": (-4, 0, 0)},
}


def clips(arm):
    acts = []
    a = pose(STANCE, READY, Chest={"r": (-2, 0, 0)}, Head={"r": (4, 6, 0)})
    b = pose(STANCE, READY, Hips={"l": (0, -0.03, 0)}, Chest={"r": (3, 0, 0)},
             Head={"r": (10, -8, 0)}, **{"UpperArm.L": {"r": (-14, 0, -16)},
                                          "Hand.L": {"r": (-25, 0, 0)},
                                          "UpperArm.R": {"r": (-24, 0, 14)}})
    acts.append(rrkit.clip(arm, "Idle", [(0, a), (45, b)]))

    def step(lf, rf, lk, rk, roll, bob):
        return pose(READY, **{
            "Hips": {"r": (0, roll * 0.6, roll), "l": (0, bob, 0)},
            "Spine": {"r": (6, 0, -roll * 0.5)},
            "Chest": {"r": (0, roll * 0.8, 0)},
            "Head": {"r": (0, 0, -roll * 0.6)},
            "UpperLeg.L": {"r": (lf, 0, -8)},
            "UpperLeg.R": {"r": (rf, 0, 8)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "Foot.L": {"r": (-lf * 0.4, 0, 0)},
            "Foot.R": {"r": (-rf * 0.4, 0, 0)},
            "UpperArm.L": {"r": (rf * 0.5 - 10, 0, -12)},
            "UpperArm.R": {"r": (lf * 0.3 - 20, 0, 12)},
        })

    acts.append(rrkit.clip(arm, "Walk", [
        (0, step(-24, 18, 6, 15, 6, -0.06)),
        (12, step(0, -4, 4, 50, 0, 0.03)),
        (24, step(18, -24, 15, 6, -6, -0.06)),
        (36, step(-4, 0, 50, 4, 0, 0.03)),
    ]))

    def charge(lf, rf, lk, rk, bob):
        return pose(**{
            "Hips": {"l": (0, bob, 0)},
            "Spine": {"r": (14, 0, 0)},
            "Head": {"r": (-12, 0, 0)},
            "UpperLeg.L": {"r": (lf, 0, -8)},
            "UpperLeg.R": {"r": (rf, 0, 8)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "UpperArm.R": {"r": (-50, -10, 8)},
            "LowerArm.R": {"r": (-30, 0, 0)},
            "UpperArm.L": {"r": (rf * 0.7 - 12, 0, -14)},
            "LowerArm.L": {"r": (-60, 0, 0)},
        })

    acts.append(rrkit.clip(arm, "Run", [
        (0, charge(-32, 24, 10, 32, -0.07)),
        (8, charge(-4, -8, 15, 75, 0.04)),
        (16, charge(24, -32, 32, 10, -0.07)),
        (24, charge(-8, -4, 75, 15, 0.04)),
    ]))

    # Brace and level the gun, then the volley's kick throws chest and gun arm back.
    brace = pose(STANCE, AIM, Hips={"l": (0, -0.05, 0)}, Spine={"r": (5, 0, 0)},
                 **{"LowerLeg.L": {"r": (12, 0, 0)}, "LowerLeg.R": {"r": (12, 0, 0)},
                    "UpperLeg.L": {"r": (-8, 0, -9)}, "UpperLeg.R": {"r": (-8, 0, 9)}})
    kick = pose(brace, Spine={"r": (-6, 0, 0)}, Chest={"r": (-6, -12, 0)},
                Head={"r": (-12, 0, 0)}, **{"UpperArm.R": {"r": (-76, -24, 4)},
                                             "LowerArm.R": {"r": (-12, 0, 0)}})
    acts.append(rrkit.clip(arm, "Attack", [(0, brace), (3, kick), (14, brace), (26, brace)],
                           loop=False))

    hurt = pose(STANCE, READY, Spine={"r": (-12, 8, 0)}, Chest={"r": (-6, 0, 0)},
                Head={"r": (-22, -15, 0)},
                **{"UpperArm.L": {"r": (-30, 0, -40)}, "UpperArm.R": {"r": (-30, 0, 32)}})
    acts.append(rrkit.clip(arm, "Pain", [(0, a), (5, hurt), (16, a)], loop=False))

    stagger = pose(STANCE, Spine={"r": (-14, 0, 10)}, Head={"r": (-28, 0, 0)},
                   **{"UpperArm.R": {"r": (-50, 0, 40)}, "UpperArm.L": {"r": (-30, 0, -55)}})
    kneel = pose(**{
        "Hips": {"r": (12, 0, 0), "l": (0, -0.55, 0)},
        "Spine": {"r": (18, 0, 0)},
        "Head": {"r": (25, 0, 0)},
        "UpperLeg.L": {"r": (-85, 0, -10)},
        "UpperLeg.R": {"r": (-30, 0, 10)},
        "LowerLeg.L": {"r": (95, 0, 0)},
        "LowerLeg.R": {"r": (110, 0, 0)},
        "UpperArm.R": {"r": (-20, 0, 18)},
        "UpperArm.L": {"r": (-10, 0, -18)},
    })
    face = pose(**{
        "Hips": {"r": (84, 0, 0), "l": (0, -0.85, -0.35)},
        "Spine": {"r": (4, 0, 0)},
        "Head": {"r": (-20, 25, 0)},
        "UpperLeg.L": {"r": (-6, 0, -12)},
        "UpperLeg.R": {"r": (0, 0, 14)},
        "LowerLeg.L": {"r": (20, 0, 0)},
        "UpperArm.R": {"r": (-160, 0, 30)},
        "UpperArm.L": {"r": (-20, 0, -65)},
    })
    bounce = pose(face, Hips={"r": (78, 0, 0), "l": (0, -0.77, -0.35)})
    acts.append(rrkit.clip(arm, "Death", [(0, a), (8, stagger), (20, kneel), (28, kneel),
                                          (40, face), (44, bounce), (50, face)], loop=False))
    return acts


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Warlord")
    build(body, materials())
    obj = body.object()
    arm = rrkit.armature("WarlordArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        rrkit.preview(prev, "warlord", arm, HEIGHT, acts)


if __name__ == "__main__":
    main()
