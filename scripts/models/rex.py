# Rex, the hero, as co-op partners see him: a reactor-plant trouble-shooter. Dark buzz cut and
# stubble, amber work goggles pushed up on the forehead, a padded slate jumpsuit with reflective
# chest bands, an olive chest rig with pouches, heavy gauntlets and steel-capped boots.
#
# The shoulder yoke, back panel, outer thigh panels, the armband and the shin stripes use the
# `RexTeam*` materials, authored near white so the game's per-slot tint colours them; skin,
# hair, goggles, gloves, boots and rig stay untinted. His guns are separate models attached to
# `Hand.R` at runtime (models.ron `player.held`); the chaingun is drawn here only in the preview.
#
# Bones hang straight down (arms and legs) or point straight up (spine, head), so every pose
# rotation means the same thing: X swings a limb (negative = forwards), Z swings it sideways
# (negative = towards +X), Y twists. For spine bones +X leans forwards.
#
# Usage: blender -b --factory-startup --python scripts/models/rex.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mathutils import Matrix  # noqa: E402

import chaingun  # noqa: E402
import rrkit  # noqa: E402

HEIGHT = 1.8
# Athletic: long legs, a tapered torso, a normal-sized head.
HIP_X, HIP_Z, KNEE_Z, ANKLE_Z = 0.105, 0.95, 0.53, 0.10
WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z, HEAD_TOP = 1.05, 1.22, 1.50, 1.58, 1.80
SHOULDER_X, SHOULDER_Z = 0.235, 1.45
ELBOW_Z, WRIST_Z, HAND_Z = 1.15, 0.89, 0.79
THIGH, SHIN = HIP_Z - KNEE_Z, KNEE_Z - ANKLE_Z
# Where a gun's grip sits: in the right fist, at rest (arm hanging, barrel forwards, -Y).
# In Hand.R's frame that is 0.05 m down the hand (+y) and 0.01 m forwards (-z), which is what
# models.ron `player.held` offsets start from.
GRIP = (-SHOULDER_X, -0.01, WRIST_Z - 0.05)


def bones():
    return rrkit.biped(HIP_X, HIP_Z, KNEE_Z, ANKLE_Z, WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z, HEAD_TOP,
                       SHOULDER_X, SHOULDER_Z, ELBOW_Z, WRIST_Z, HAND_Z)


def materials():
    return {
        "skin": rrkit.material("RexSkin", (196, 146, 112)),
        "stubble": rrkit.material("RexStubble", (122, 96, 80)),
        "hair": rrkit.material("RexHair", (36, 30, 27), rough=0.95),
        "eye": rrkit.material("RexEye", (28, 26, 30), rough=0.3),
        "brow": rrkit.material("RexBrow", (40, 32, 28)),
        "strap": rrkit.material("RexGoggleStrap", (44, 42, 40), rough=0.9),
        "frame": rrkit.material("RexGoggleFrame", (70, 72, 76), rough=0.5, metal=0.6),
        "lens": rrkit.material("RexGoggleLens", (255, 160, 30), rough=0.15, emit=1.5),
        "suit": rrkit.material("RexSuit", (62, 68, 78), rough=0.85),
        "seam": rrkit.material("RexSuitSeam", (44, 48, 56), rough=0.9),
        "reflect": rrkit.material("RexReflect", (176, 182, 188), rough=0.3, metal=0.6),
        "rig": rrkit.material("RexRig", (78, 76, 54), rough=0.9),
        "pouch": rrkit.material("RexPouch", (96, 92, 64), rough=0.9),
        "buckle": rrkit.material("RexBuckle", (140, 142, 146), rough=0.4, metal=0.9),
        "glove": rrkit.material("RexGlove", (46, 38, 32), rough=0.75),
        "boot": rrkit.material("RexBoot", (34, 30, 28), rough=0.7),
        "sole": rrkit.material("RexSole", (22, 22, 24), rough=0.95),
        "cap": rrkit.material("RexToecap", (120, 122, 126), rough=0.4, metal=0.8),
        # Slot-tinted (authored near white).
        "team": rrkit.material("RexTeamPanel", (236, 236, 236), rough=0.7),
        "band": rrkit.material("RexTeamArmband", (240, 240, 240), rough=0.6),
        "stripe": rrkit.material("RexTeamStripe", (246, 246, 246), rough=0.35),
    }


def build(body, m):
    su, se, tm = m["suit"], m["seam"], m["team"]
    # Pelvis and a web belt with a steel buckle and two side pouches.
    body.box("Hips", su, (0, 0, 0.965), (0.31, 0.21, 0.21), top=(1.05, 1.0), bevel=0.02)
    body.box("Hips", m["rig"], (0, 0, 1.035), (0.33, 0.23, 0.055), bevel=0.008)
    body.box("Hips", m["buckle"], (0, -0.118, 1.035), (0.07, 0.012, 0.045), bevel=0.005)
    for s in (1, -1):
        body.box("Hips", m["pouch"], (s * 0.13, -0.10, 0.99), (0.07, 0.05, 0.08), bevel=0.01)
    # Belly and chest: padded quilting, narrowing to the waist.
    body.box("Spine", su, (0, 0, 1.135), (0.31, 0.20, 0.19), top=(1.12, 1.05), bevel=0.02)
    body.box("Chest", su, (0, 0, 1.33), (0.36, 0.22, 0.24), top=(1.14, 1.05), bevel=0.025)
    # Team yoke over the shoulders and upper back, and a back panel.
    body.box("Chest", tm, (0, 0.005, 1.445), (0.43, 0.245, 0.08), top=(0.78, 0.88), bevel=0.015)
    body.box("Chest", tm, (0, 0.115, 1.31), (0.30, 0.02, 0.16), bevel=0.006)
    body.box("Spine", se, (0, 0.104, 1.14), (0.02, 0.01, 0.17))  # back seam
    # Reflective band right round the chest, under the yoke.
    body.box("Chest", m["reflect"], (0, 0, 1.385), (0.385, 0.235, 0.03))
    # Chest rig: a plate, three pouches with flaps, two shoulder straps crossing the yoke.
    body.box("Chest", m["rig"], (0, -0.12, 1.29), (0.28, 0.03, 0.17), bevel=0.008)
    for x in (-0.085, 0.0, 0.085):
        body.box("Chest", m["pouch"], (x, -0.145, 1.255), (0.075, 0.045, 0.09), bevel=0.008)
        body.box("Chest", m["rig"], (x, -0.150, 1.300), (0.08, 0.05, 0.025), bevel=0.005)
    for s in (1, -1):
        body.box("Chest", m["rig"], (s * 0.11, 0.0, 1.43), (0.05, 0.27, 0.06), top=(1.0, 0.9))
        body.box("Chest", m["buckle"], (s * 0.11, -0.137, 1.375), (0.03, 0.01, 0.025))
    # Neck and a padded collar.
    body.cyl("Neck", m["skin"], (0, -0.01, 1.54), 0.055, 0.052, 0.10, seg=8)
    body.cyl("Neck", su, (0, 0.0, 1.505), 0.085, 0.075, 0.05, seg=8, bevel=0.008)

    # Head: skull, a square stubbled jaw, nose, ears, brow; buzz cut; goggles up on the brow.
    sk = m["skin"]
    body.box("Head", sk, (0, -0.005, 1.695), (0.17, 0.20, 0.17), top=(0.92, 0.92), bevel=0.025)
    body.box("Head", m["stubble"], (0, -0.02, 1.62), (0.165, 0.19, 0.07), top=(1.03, 1.0),
             front=0.92, bevel=0.015)
    body.box("Head", m["stubble"], (0, -0.095, 1.597), (0.10, 0.04, 0.04), bevel=0.01)  # chin
    body.box("Head", sk, (0, -0.105, 1.665), (0.034, 0.04, 0.05), top=(0.7, 0.6))  # nose
    body.box("Head", m["brow"], (0, -0.095, 1.712), (0.13, 0.025, 0.018))
    for s in (1, -1):
        body.box("Head", m["eye"], (s * 0.035, -0.099, 1.694), (0.028, 0.01, 0.016))
        body.box("Head", sk, (s * 0.088, 0.005, 1.68), (0.02, 0.045, 0.055), bevel=0.006)  # ear
    body.box("Head", m["hair"], (0, 0.008, 1.765), (0.175, 0.205, 0.055), top=(0.88, 0.88),
             bevel=0.018)
    body.box("Head", m["hair"], (0, 0.075, 1.70), (0.165, 0.07, 0.10), bevel=0.012)  # back
    # Goggle strap round the crown, two lenses tilted up on the forehead.
    body.box("Head", m["strap"], (0, 0.01, 1.752), (0.185, 0.215, 0.025), bevel=0.006)
    for s in (1, -1):
        body.cyl("Head", m["frame"], (s * 0.038, -0.105, 1.755), 0.032, 0.03, 0.03,
                 rot=(60, 0, 0), seg=8)
        body.cyl("Head", m["lens"], (s * 0.038, -0.118, 1.762), 0.025, 0.025, 0.008,
                 rot=(60, 0, 0), seg=8)
    body.box("Head", m["frame"], (0, -0.108, 1.752), (0.02, 0.015, 0.012))  # bridge

    def arm(s, sfx):
        x = s * SHOULDER_X
        up, lo, ha = "UpperArm" + sfx, "LowerArm" + sfx, "Hand" + sfx
        body.ball(up, tm, (x, 0, SHOULDER_Z - 0.005), 0.085, scale=(1.0, 1.05, 0.9))  # shoulder
        body.cyl(up, su, (x, 0, 1.29), 0.058, 0.07, 0.25, seg=8)
        if s > 0:
            body.cyl(up, m["band"], (x, 0, 1.30), 0.073, 0.076, 0.06, seg=8)  # armband
        body.cyl(lo, su, (x, 0, 1.06), 0.05, 0.058, 0.17, seg=8)
        # Gauntlet: a flared cuff over the wrist and a big fist.
        body.cyl(lo, m["glove"], (x, 0, WRIST_Z + 0.05), 0.06, 0.066, 0.10, seg=8)
        body.box(ha, m["glove"], (x, -0.008, WRIST_Z - 0.05), (0.085, 0.10, 0.10), bevel=0.015)
        body.box(ha, m["glove"], (x, -0.056, WRIST_Z - 0.035), (0.055, 0.03, 0.035),
                 bevel=0.008)  # thumb

    def leg(s, sfx):
        x = s * HIP_X
        up, lo, ft = "UpperLeg" + sfx, "LowerLeg" + sfx, "Foot" + sfx
        body.cyl(up, su, (x, 0, 0.74), 0.08, 0.098, 0.40, seg=8)
        body.box(up, tm, (x + s * 0.088, 0, 0.75), (0.02, 0.10, 0.25), bevel=0.005)  # side
        body.box(up, m["pouch"], (x + s * 0.03, -0.078, 0.70), (0.08, 0.04, 0.10), bevel=0.01)
        body.cyl(lo, su, (x, 0, 0.36), 0.06, 0.074, 0.32, seg=8)
        body.box(lo, m["rig"], (x, -0.068, 0.51), (0.10, 0.04, 0.09), bevel=0.012)  # knee pad
        body.cyl(lo, m["stripe"], (x, 0, 0.33), 0.075, 0.077, 0.04, seg=8)  # shin stripe
        body.cyl(lo, m["boot"], (x, 0, 0.17), 0.072, 0.076, 0.17, seg=8)  # boot shaft
        body.box(ft, m["boot"], (x, -0.04, 0.055), (0.11, 0.25, 0.09), top=(0.95, 0.85),
                 bevel=0.02)
        body.box(ft, m["sole"], (x, -0.042, 0.013), (0.118, 0.265, 0.026))
        body.box(ft, m["cap"], (x, -0.16, 0.045), (0.10, 0.03, 0.06), bevel=0.008)

    rrkit.both(arm)
    rrkit.both(leg)


# --- clips ---------------------------------------------------------------------------------


def pose(*parts, **extra):
    out = {}
    for p in parts + (extra,):
        for bone, k in p.items():
            out[bone] = {**out.get(bone, {}), **k}
    return out


def leg_ik(sfx, hip_z, ahead, lift=0.0, out=0.0):
    """Bends one leg so its ankle sits `ahead` metres forwards of the hip and `lift` off its
    rest height, with the hip joint at `hip_z`; the foot stays level. `out` splays it (Z)."""
    f, d = ahead, hip_z - ANKLE_Z - lift
    dist = min(math.hypot(f, d), THIGH + SHIN - 1e-4)
    phi = math.atan2(f, d)
    alpha = math.acos((THIGH ** 2 + dist ** 2 - SHIN ** 2) / (2 * THIGH * dist))
    beta = math.acos((SHIN ** 2 + dist ** 2 - THIGH ** 2) / (2 * SHIN * dist))
    fa, fb = math.degrees(phi + alpha), math.degrees(phi - beta)
    side = 1 if sfx == ".L" else -1
    return {
        "UpperLeg" + sfx: {"r": (-fa, 0, -side * out)},
        "LowerLeg" + sfx: {"r": (fa - fb, 0, 0)},
        "Foot" + sfx: {"r": (fb, 0, side * out)},
    }


def legs(hip_z, l_ahead, r_ahead, l_lift=0.0, r_lift=0.0, out=4.0):
    return pose(leg_ik(".L", hip_z, l_ahead, l_lift, out),
                leg_ik(".R", hip_z, r_ahead, r_lift, out),
                Hips={"l": (0, hip_z - HIP_Z, 0)})


# Gun held at the ready: right forearm level, wrist cocked so the barrel points ahead; the left
# hand comes across under the gun to steady it.
READY = {
    "UpperArm.R": {"r": (-22, 0, 4)},
    "LowerArm.R": {"r": (-62, 0, 0)},
    "Hand.R": {"r": (84, 0, 0)},
    "UpperArm.L": {"r": (-28, 0, 22)},
    "LowerArm.L": {"r": (-58, -20, 30)},
    "Hand.L": {"r": (40, 0, 0)},
}
# Gun shouldered and levelled at the target.
AIM = {
    "UpperArm.R": {"r": (-80, -12, -4)},
    "LowerArm.R": {"r": (-8, 0, 0)},
    "Hand.R": {"r": (88, 0, 0)},
    "UpperArm.L": {"r": (-62, 0, 30)},
    "LowerArm.L": {"r": (-30, -20, 40)},
    "Hand.L": {"r": (30, 0, 0)},
    "Chest": {"r": (0, -6, 0)},
}


def clips(arm):
    acts = []
    stand = legs(HIP_Z, 0.0, 0.0)
    # Idle: alert, weight shifting, scanning left and right.
    a = pose(stand, READY, Spine={"r": (3, 0, 0)}, Head={"r": (-3, -12, 0)})
    b = pose(legs(HIP_Z - 0.015, 0.0, 0.0), READY, **{
        "Spine": {"r": (4, 4, 0)},
        "Chest": {"r": (-2, 0, 0)},
        "Head": {"r": (-2, 14, 0)},
        "LowerArm.R": {"r": (-66, 0, 0)},
    })
    acts.append(rrkit.clip(arm, "Idle", [(0, a), (36, b)]))

    # Walk: a purposeful, gun-up stride, 1 s per cycle.
    def step(l_ahead, r_ahead, l_lift, r_lift, twist, hip_z):
        return pose(legs(hip_z, l_ahead, r_ahead, l_lift, r_lift, 3), READY, **{
            "Hips": {"r": (0, twist, 0), "l": (0, hip_z - HIP_Z, 0)},
            "Spine": {"r": (4, -twist * 0.8, 0)},
            "Head": {"r": (0, twist * 0.3, 0)},
        })

    acts.append(rrkit.clip(arm, "Walk", [
        (0, step(0.30, -0.26, 0.0, 0.0, 6, HIP_Z - 0.04)),
        (8, step(0.0, 0.02, 0.0, 0.10, 0, HIP_Z - 0.005)),
        (15, step(-0.26, 0.30, 0.0, 0.0, -6, HIP_Z - 0.04)),
        (23, step(0.02, 0.0, 0.10, 0.0, 0, HIP_Z - 0.005)),
    ]))

    # Run: leaning in, gun levelled, long strides; 2/3 s per cycle.
    def stride(l_ahead, r_ahead, l_lift, r_lift, hip_z):
        return pose(legs(hip_z, l_ahead, r_ahead, l_lift, r_lift, 2), AIM, **{
            "Spine": {"r": (12, 0, 0)},
            "Chest": {"r": (2, -6, 0)},
            "Head": {"r": (-12, 0, 0)},
        })

    acts.append(rrkit.clip(arm, "Run", [
        (0, stride(0.42, -0.34, 0.02, 0.10, HIP_Z - 0.06)),
        (5, stride(0.05, -0.05, 0.0, 0.26, HIP_Z - 0.01)),
        (10, stride(-0.34, 0.42, 0.10, 0.02, HIP_Z - 0.06)),
        (15, stride(-0.05, 0.05, 0.26, 0.0, HIP_Z - 0.01)),
    ]))

    # Crouch: a low duck-walk, bent over the gun; the head stays under 1.1 m.
    low = 0.37

    def duck(l_ahead, r_ahead, l_lift, r_lift, bob):
        return pose(legs(low + bob, l_ahead, r_ahead, l_lift, r_lift, 14), READY, **{
            "Spine": {"r": (30, 0, 0)},
            "Chest": {"r": (26, 0, 0)},
            "Neck": {"r": (-14, 0, 0)},
            "Head": {"r": (-18, 0, 0)},
            "UpperArm.R": {"r": (-72, 0, 4)},
            "LowerArm.R": {"r": (-36, 0, 0)},
            "Hand.R": {"r": (52, 0, 0)},
            "UpperArm.L": {"r": (-66, 0, 24)},
            "LowerArm.L": {"r": (-40, -20, 30)},
        })

    acts.append(rrkit.clip(arm, "Crouch", [
        (0, duck(0.36, 0.08, 0.0, 0.0, 0.0)),
        (12, duck(0.22, 0.22, 0.0, 0.06, 0.015)),
        (24, duck(0.08, 0.36, 0.0, 0.0, 0.0)),
        (36, duck(0.22, 0.22, 0.06, 0.0, 0.015)),
    ]))

    # Attack: aimed, a sharp kick up and back, settling on aim.
    aim = pose(stand, AIM, Spine={"r": (2, 0, 0)})
    kick = pose(stand, AIM, **{
        "Spine": {"r": (-6, 0, 0)},
        "Head": {"r": (-8, 0, 0)},
        "UpperArm.R": {"r": (-100, -12, -4)},
        "LowerArm.R": {"r": (-14, 0, 0)},
        "Hand.R": {"r": (74, 0, 0)},
        "UpperArm.L": {"r": (-78, 0, 30)},
    })
    acts.append(rrkit.clip(arm, "Attack", [(0, aim), (2, kick), (8, aim), (14, aim)], loop=False))

    # Pain: doubled over and knocked back a step, gun hand flung out.
    hurt = pose(legs(HIP_Z - 0.06, 0.12, -0.10), **{
        "Spine": {"r": (-16, 8, 0)},
        "Chest": {"r": (-10, 0, 0)},
        "Head": {"r": (-26, -12, 0)},
        "UpperArm.L": {"r": (-20, 0, -40)},
        "UpperArm.R": {"r": (-30, 0, 46)},
        "LowerArm.L": {"r": (-30, 0, 0)},
        "LowerArm.R": {"r": (-40, 0, 0)},
    })
    acts.append(rrkit.clip(arm, "Pain", [(0, a), (3, hurt), (12, a)], loop=False))

    # Death: knees go, he twists and falls flat on his back, arms out; a small bounce.
    buckle = pose(legs(0.62, 0.20, 0.05, out=10), **{
        "Hips": {"r": (0, 25, 0), "l": (0, 0.62 - HIP_Z, 0)},
        "Spine": {"r": (-22, 10, 0)},
        "Head": {"r": (-25, 0, 0)},
        "UpperArm.L": {"r": (-60, 0, -40)},
        "UpperArm.R": {"r": (-70, 0, 40)},
        "LowerArm.L": {"r": (-30, 0, 0)},
        "LowerArm.R": {"r": (-30, 0, 0)},
    })
    sit = pose(**{
        "Hips": {"r": (-45, 28, 0), "l": (0, 0.30 - HIP_Z, 0)},
        "Spine": {"r": (-10, 0, 0)},
        "Head": {"r": (20, 0, 0)},
        "UpperLeg.L": {"r": (-72, 0, -12)},
        "UpperLeg.R": {"r": (-66, 0, 10)},
        "LowerLeg.L": {"r": (93, 0, 0)},
        "LowerLeg.R": {"r": (86, 0, 0)},
        "Foot.L": {"r": (24, 0, 0)},
        "Foot.R": {"r": (24, 0, 0)},
        "UpperArm.L": {"r": (-40, 0, -60)},
        "UpperArm.R": {"r": (-40, 0, 60)},
    })
    flat = pose(**{
        "Hips": {"r": (-90, 30, 0), "l": (0, 0.14 - HIP_Z, 0)},
        "Head": {"r": (8, 0, 20)},
        "UpperLeg.L": {"r": (0, 0, -14)},
        "UpperLeg.R": {"r": (-8, 0, 10)},
        "LowerLeg.R": {"r": (16, 0, 0)},
        "Foot.L": {"r": (46, 0, -10)},
        "Foot.R": {"r": (46, 0, 10)},
        "UpperArm.L": {"r": (6, 0, -70)},
        "UpperArm.R": {"r": (6, 0, 80)},
        "LowerArm.L": {"r": (-20, 0, 0)},
    })
    bounce = pose(flat, Hips={"r": (-88, 30, 0), "l": (0, 0.17 - HIP_Z, 0)})
    # Half-way down, the hips stay up until the legs have folded, so the feet keep the floor.
    fold = pose(rrkit.lerp_pose(buckle, sit, 0.5),
                Hips={"r": (-22, 27, 0), "l": (0, 0.55 - HIP_Z, 0)})
    acts.append(rrkit.clip(arm, "Death", [(0, a), (8, buckle), (11, fold), (14, sit), (20, flat),
                                          (25, bounce), (32, flat)], loop=False))
    return acts


def preview_gun(arm):
    """The chaingun in the right fist at 0.9 scale, as models.ron attaches it."""
    m = chaingun.materials()
    xf = Matrix.Translation(GRIP) @ Matrix.Scale(0.9, 4)
    body = rrkit.Body("PreviewChaingun")
    body.xf = xf
    chaingun.build_body(body, m)
    body.xf = xf @ Matrix.Translation((0.0, chaingun.CLUSTER_Y, chaingun.AXIS_Z))
    chaingun.build_barrels(body, m)
    obj = body.object()
    for g in list(obj.vertex_groups):
        obj.vertex_groups.remove(g)
    obj.vertex_groups.new(name="Hand.R").add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
    rrkit.skin(obj, arm)


def report_heights(obj, acts):
    """Prints each clip's lowest and highest vertex over its frames (posed, world space), so the
    crouch's 1.1 m bound and the feet on the floor can be checked by number."""
    import bpy

    arm = obj.parent
    scene = bpy.context.scene
    for act in acts:
        arm.animation_data.action = act
        lo, hi, last, low_at = 9.0, -9.0, None, 0
        for f in range(int(act.frame_range[0]), int(act.frame_range[1]) + 1):
            scene.frame_set(f)
            ev = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
            zs = [(obj.matrix_world @ v.co).z for v in ev.to_mesh().vertices]
            ev.to_mesh_clear()
            if min(zs) < lo:
                lo, low_at = min(zs), f
            hi, last = max(hi, max(zs)), (min(zs), max(zs))
        print(f"rrkit: {act.name}: z {lo:.3f} (frame {low_at})..{hi:.3f}, "
              f"last frame {last[0]:.3f}..{last[1]:.3f}")


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    m = materials()
    body = rrkit.Body("Rex")
    build(body, m)
    obj = body.object()
    arm = rrkit.armature("RexArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        report_heights(obj, acts)
        preview_gun(arm)
        rrkit.preview(prev, "rex", arm, HEIGHT, acts)


if __name__ == "__main__":
    main()
