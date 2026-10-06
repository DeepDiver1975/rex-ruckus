# The Goon (Grunt): a squat mutant thug. Lantern jaw with tusks, red mohawk, glowing wraparound
# shades, dirty tank top, Popeye forearms and a big swagger. His hand-cannon is a separate model
# (hand_cannon.py) attached to `Hand.R` at runtime; it is drawn here only in the preview.
#
# Bones hang straight down (arms and legs) or point straight up (spine, head), so every pose
# rotation means the same thing: X swings a limb (negative = forwards), Z swings it sideways
# (negative = towards +X), Y twists. For spine bones +X leans forwards.
#
# Usage: blender -b --factory-startup --python scripts/models/goon.py -- OUT.glb [--preview DIR]
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mathutils import Matrix  # noqa: E402

import hand_cannon  # noqa: E402
import rrkit  # noqa: E402

HEIGHT = 1.8
# Short, thick legs under a huge torso: the body is the character.
HIP_X, HIP_Z, KNEE_Z, ANKLE_Z = 0.17, 0.68, 0.38, 0.11
WAIST_Z, CHEST_Z, NECK_Z, HEAD_Z = 0.84, 1.06, 1.40, 1.45
SHOULDER_X, SHOULDER_Z = 0.47, 1.32
ELBOW_Z, WRIST_Z, HAND_Z = 1.04, 0.80, 0.68
# Where the cannon's grip sits: in the fist, at rest (arm hanging, barrel forwards).
GRIP = (-SHOULDER_X, -0.01, WRIST_Z - 0.055)


def bones():
    b = [
        ("Hips", (0, 0, HIP_Z), (0, 0, WAIST_Z), None, True),
        ("Spine", (0, 0, WAIST_Z), (0, 0, CHEST_Z), "Hips", True),
        ("Chest", (0, 0, CHEST_Z), (0, 0, NECK_Z), "Spine", True),
        ("Neck", (0, 0, NECK_Z), (0, 0, HEAD_Z), "Chest", True),
        ("Head", (0, 0, HEAD_Z), (0, 0, HEAD_Z + 0.25), "Neck", True),
    ]
    for s, sfx in ((1, ".L"), (-1, ".R")):
        x = s * SHOULDER_X
        b += [
            ("UpperArm" + sfx, (x, 0, SHOULDER_Z), (x, 0, ELBOW_Z), "Chest", True),
            ("LowerArm" + sfx, (x, 0, ELBOW_Z), (x, 0, WRIST_Z), "UpperArm" + sfx, True),
            ("Hand" + sfx, (x, 0, WRIST_Z), (x, 0, HAND_Z), "LowerArm" + sfx, True),
            ("UpperLeg" + sfx, (s * HIP_X, 0, HIP_Z), (s * HIP_X, 0, KNEE_Z), "Hips", True),
            ("LowerLeg" + sfx, (s * HIP_X, 0, KNEE_Z), (s * HIP_X, 0, ANKLE_Z), "UpperLeg" + sfx, True),
            ("Foot" + sfx, (s * HIP_X, 0, ANKLE_Z), (s * HIP_X, -0.16, 0.04), "LowerLeg" + sfx, True),
        ]
    # The cannon's muzzle, for the shot glow (models.ron `tip: Node("Muzzle")`).
    mz = tuple(g + o for g, o in zip(GRIP, hand_cannon.MUZZLE))
    b.append(("Muzzle", mz, (mz[0], mz[1] - 0.05, mz[2]), "Hand.R", False))
    return b


def materials():
    return {
        "skin": rrkit.material("GoonSkin", (118, 150, 70)),
        "tank": rrkit.material("GoonTank", (222, 214, 182)),
        "pants": rrkit.material("GoonPants", (72, 62, 110)),
        "boots": rrkit.material("GoonBoots", (38, 32, 30), rough=0.6),
        "belt": rrkit.material("GoonBelt", (92, 56, 30)),
        "gold": rrkit.material("GoonGold", (232, 182, 40), rough=0.3, metal=0.9),
        "hair": rrkit.material("GoonMohawk", (226, 36, 48)),
        "shades": rrkit.material("GoonShades", (255, 70, 20), rough=0.2, emit=4.0),
        "frame": rrkit.material("GoonFrame", (20, 20, 24), rough=0.3),
        "ivory": rrkit.material("GoonTusk", (236, 226, 190)),
        "studs": rrkit.material("GoonStuds", (200, 204, 210), rough=0.3, metal=0.9),
    }


def build(body, m):
    sk, tk, pa, bo = m["skin"], m["tank"], m["pants"], m["boots"]
    # Pelvis, belt and a big gold buckle.
    body.box("Hips", pa, (0, 0, 0.74), (0.46, 0.30, 0.20), top=(1.05, 1.0), bevel=0.025)
    body.box("Hips", m["belt"], (0, 0, WAIST_Z), (0.50, 0.33, 0.07))
    body.box("Hips", m["gold"], (0, -0.17, WAIST_Z), (0.15, 0.025, 0.10), bevel=0.01)
    # Narrow waist flaring into a barrel chest: a V under the tank top.
    body.box("Spine", tk, (0, 0, 0.95), (0.48, 0.32, 0.22), top=(1.3, 1.15), bevel=0.03)
    body.box("Spine", tk, (0, -0.10, 0.95), (0.30, 0.12, 0.18), top=(1.1, 1.0), bevel=0.03)  # abs
    body.box("Chest", tk, (0, 0, 1.18), (0.66, 0.40, 0.26), top=(1.22, 1.0), bevel=0.04)
    for s in (1, -1):
        # Pecs bulging the tank top, lats flaring out under the arms.
        body.box("Chest", tk, (s * 0.15, -0.17, 1.22), (0.28, 0.14, 0.20), top=(1.0, 0.7),
                 bevel=0.04)
        body.box("Chest", sk, (s * 0.33, 0.04, 1.12), (0.12, 0.30, 0.22), top=(1.5, 1.0),
                 bevel=0.03)
    # Bare, mountainous traps sloping from the neck to the shoulders.
    body.box("Chest", sk, (0, 0.03, 1.36), (0.84, 0.36, 0.14), top=(0.42, 0.7), bevel=0.03)
    for s in (1, -1):
        body.box("Chest", tk, (s * 0.15, -0.03, 1.34), (0.09, 0.40, 0.05))  # straps
    # Thick neck, head sunk low and pushed forwards: little cranium, huge jaw.
    hz = HEAD_Z - 1.57  # the head parts were laid out with the neck top at 1.57
    body.cyl("Neck", sk, (0, -0.02, NECK_Z + 0.03), 0.15, 0.12, 0.12, seg=8)
    body.box("Head", sk, (0, -0.02, 1.71 + hz), (0.22, 0.22, 0.17), top=(0.8, 0.85), bevel=0.02)
    body.box("Head", sk, (0, -0.06, 1.61 + hz), (0.38, 0.28, 0.15), top=(0.85, 0.95), front=1.1,
             bevel=0.025)
    body.box("Head", sk, (0, -0.19, 1.585 + hz), (0.25, 0.08, 0.09), bevel=0.015)  # chin
    body.box("Head", sk, (0, -0.125, 1.735 + hz), (0.23, 0.06, 0.05), bevel=0.01)  # brow
    body.box("Head", sk, (0, -0.155, 1.665 + hz), (0.05, 0.06, 0.06), top=(1.3, 1.0))  # nose
    for s in (1, -1):
        body.spike("Head", m["ivory"], (s * 0.10, -0.20, 1.61 + hz), 0.028, 0.11,
                   rot=(-12, 0, s * 4))
        body.box("Head", sk, (s * 0.115, -0.01, 1.69 + hz), (0.03, 0.06, 0.07))  # ears
    # Wraparound shades: a glowing band with dark side arms.
    body.box("Head", m["shades"], (0, -0.145, 1.70 + hz), (0.25, 0.03, 0.05), front=1.05)
    for s in (1, -1):
        body.box("Head", m["frame"], (s * 0.125, -0.08, 1.70 + hz), (0.02, 0.12, 0.035))
    # Mohawk: five swept blades.
    for i, y in enumerate((-0.11, -0.06, -0.01, 0.04, 0.09)):
        h = 0.10 + 0.03 * (2 - abs(2 - i))
        body.box("Head", m["hair"], (0, y, 1.79 + hz + h / 2 - 0.01), (0.04, 0.06, h),
                 top=(0.4, 0.5), shift=(0, 0.25), rot=(-15, 0, 0))

    def arm(s, sfx):
        x = s * SHOULDER_X
        up, lo, ha = "UpperArm" + sfx, "LowerArm" + sfx, "Hand" + sfx
        body.ball(up, sk, (x, 0, SHOULDER_Z - 0.01), 0.17, scale=(1.0, 1.05, 0.9))  # deltoid
        body.cyl(up, sk, (x, 0, 1.17), 0.10, 0.13, 0.26, seg=8)
        body.ball(up, sk, (x, -0.06, 1.16), 0.10, scale=(0.9, 0.8, 1.2))  # biceps
        # Popeye forearm, a studded wristband and a big fist.
        body.cyl(lo, sk, (x, 0, 0.93), 0.10, 0.15, 0.24, seg=8)
        body.cyl(lo, m["frame"], (x, 0, WRIST_Z + 0.015), 0.115, 0.115, 0.06, seg=8)
        for a in (0, 90, 180, 270):
            body.spike(lo, m["studs"], (x, 0, WRIST_Z + 0.015), 0.017, 0.15, rot=(90, 0, a))
        body.box(ha, sk, (x, -0.01, WRIST_Z - 0.06), (0.15, 0.16, 0.14), bevel=0.025)

    def leg(s, sfx):
        x = s * HIP_X
        up, lo, ft = "UpperLeg" + sfx, "LowerLeg" + sfx, "Foot" + sfx
        body.cyl(up, pa, (x, 0, 0.53), 0.13, 0.17, 0.32, seg=8)
        body.cyl(lo, pa, (x, 0, 0.28), 0.115, 0.13, 0.22, seg=8)
        body.cyl(lo, bo, (x, 0, 0.17), 0.125, 0.13, 0.14, seg=8)  # boot shaft
        body.box(ft, bo, (x, -0.06, 0.06), (0.20, 0.34, 0.12), top=(0.95, 0.8), bevel=0.025)
        body.box(ft, m["studs"], (x, -0.225, 0.055), (0.18, 0.02, 0.07))  # toecap

    rrkit.both(arm)
    rrkit.both(leg)


# --- clips ---------------------------------------------------------------------------------

STANCE = {
    "UpperLeg.L": {"r": (0, 0, -6)},
    "UpperLeg.R": {"r": (0, 0, 6)},
    "Foot.L": {"r": (0, 0, 6)},
    "Foot.R": {"r": (0, 0, -6)},
}
# Left fist planted on the hip, elbow out.
HIP_HAND = {
    "UpperArm.L": {"r": (8, 0, -38)},
    "LowerArm.L": {"r": (-40, 0, 80)},
    "Hand.L": {"r": (0, 0, 20)},
}
# Cannon hanging at the right side, a little out from the leg.
GUN_DOWN = {
    "UpperArm.R": {"r": (0, 0, 14)},
    "LowerArm.R": {"r": (-12, 0, -4)},
}
# Cannon straight out at the target; the wrist cocks back to keep the barrel level.
GUN_AIM = {
    "UpperArm.R": {"r": (-86, -16, -6)},
    "LowerArm.R": {"r": (-4, 0, 0)},
    "Hand.R": {"r": (88, 0, 0)},
    "Chest": {"r": (0, -8, 0)},
}


def pose(*parts, **extra):
    out = {}
    for p in parts + (extra,):
        for bone, k in p.items():
            out[bone] = {**out.get(bone, {}), **k}
    return out


def clips(arm):
    acts = []
    # Idle: cocky lean, chest heaving, head bobbing, gun hand twitching.
    a = pose(STANCE, HIP_HAND, GUN_DOWN, **{
        "Spine": {"r": (-5, 0, 0)},
        "Head": {"r": (-6, 0, 0)},
    })
    b = pose(STANCE, HIP_HAND, GUN_DOWN, **{
        "Hips": {"l": (0, -0.015, 0)},
        "Spine": {"r": (-3, 6, 0)},
        "Chest": {"r": (-6, 0, 0)},
        "Head": {"r": (-10, -14, 4)},
        "LowerArm.R": {"r": (-22, 0, -4)},
    })
    acts.append(rrkit.clip(arm, "Idle", [(0, a), (30, b)]))

    # Walk: a rolling swagger, 1 s per cycle.
    def step(lf, rf, lk, rk, twist, bob):
        return pose(HIP_HAND, GUN_DOWN, **{
            "Hips": {"r": (0, twist, -twist * 0.5), "l": (0, bob, 0)},
            "Spine": {"r": (4, -twist * 0.6, 0)},
            "Head": {"r": (0, twist * 0.4, 0)},
            "UpperLeg.L": {"r": (lf, 0, -4)},
            "UpperLeg.R": {"r": (rf, 0, 4)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "Foot.L": {"r": (-lf * 0.4, 0, 0)},
            "Foot.R": {"r": (-rf * 0.4, 0, 0)},
            "UpperArm.R": {"r": (-rf * 0.5, 0, 14)},
        })

    acts.append(rrkit.clip(arm, "Walk", [
        (0, step(-28, 24, 8, 20, 10, 0.0)),
        (8, step(0, -6, 4, 55, 0, 0.03)),
        (15, step(24, -28, 20, 8, -10, 0.0)),
        (23, step(-6, 0, 55, 4, 0, 0.03)),
    ]))

    # Run: leaning in with the cannon levelled, 2/3 s per cycle.
    def stride(lf, rf, lk, rk, bob):
        return pose(GUN_AIM, **{
            "Hips": {"l": (0, bob, 0)},
            "Spine": {"r": (14, 0, 0)},
            "Head": {"r": (-12, 0, 0)},
            "UpperLeg.L": {"r": (lf, 0, -3)},
            "UpperLeg.R": {"r": (rf, 0, 3)},
            "LowerLeg.L": {"r": (lk, 0, 0)},
            "LowerLeg.R": {"r": (rk, 0, 0)},
            "Foot.L": {"r": (-lf * 0.3, 0, 0)},
            "Foot.R": {"r": (-rf * 0.3, 0, 0)},
            "UpperArm.L": {"r": (rf * 0.9, 0, -12)},
            "LowerArm.L": {"r": (-50, 0, 0)},
        })

    acts.append(rrkit.clip(arm, "Run", [
        (0, stride(-45, 30, 10, 40, -0.02)),
        (5, stride(-5, -10, 15, 90, 0.04)),
        (10, stride(30, -45, 40, 10, -0.02)),
        (15, stride(-10, -5, 90, 15, 0.04)),
    ]))

    # Attack: aimed, then a big kick that throws the arm up and the head back.
    aim = pose(STANCE, HIP_HAND, GUN_AIM, **{"Spine": {"r": (-2, 0, 0)}})
    kick = pose(STANCE, HIP_HAND, GUN_AIM, **{
        "Spine": {"r": (-9, 0, 0)},
        "Head": {"r": (-12, 0, 0)},
        "UpperArm.R": {"r": (-118, -16, -6)},
        "LowerArm.R": {"r": (-14, 0, 0)},
        "Hand.R": {"r": (70, 0, 0)},
    })
    acts.append(rrkit.clip(arm, "Attack", [(0, aim), (2, kick), (9, aim), (18, aim)], loop=False))

    # Pain: snapped back, arms flung wide.
    hurt = pose(STANCE, **{
        "Hips": {"l": (0, -0.04, 0)},
        "Spine": {"r": (-18, 0, 0)},
        "Chest": {"r": (-10, 0, 0)},
        "Head": {"r": (-28, 12, 0)},
        "UpperArm.L": {"r": (-20, 0, -55)},
        "UpperArm.R": {"r": (-20, 0, 55)},
        "LowerArm.L": {"r": (-30, 0, 0)},
        "LowerArm.R": {"r": (-30, 0, 0)},
    })
    acts.append(rrkit.clip(arm, "Pain", [(0, a), (3, hurt), (12, a)], loop=False))

    # Death: spun round by the hit, legs buckle, flops flat on his back with a bounce.
    spin = pose(**{
        "Hips": {"r": (0, 120, 0), "l": (0, -0.05, 0)},
        "Spine": {"r": (-15, 20, 0)},
        "Head": {"r": (-30, 0, 0)},
        "UpperArm.L": {"r": (-30, 0, -80)},
        "UpperArm.R": {"r": (-30, 0, 80)},
    })
    buckle = pose(**{
        "Hips": {"r": (-20, 200, 0), "l": (0, -0.35, 0)},
        "Spine": {"r": (-20, 0, 0)},
        "Head": {"r": (-20, 0, 0)},
        "UpperLeg.L": {"r": (-50, 0, -10)},
        "UpperLeg.R": {"r": (-40, 0, 10)},
        "LowerLeg.L": {"r": (80, 0, 0)},
        "LowerLeg.R": {"r": (70, 0, 0)},
        "UpperArm.L": {"r": (-150, 0, -40)},
        "UpperArm.R": {"r": (-150, 0, 40)},
    })
    flat = pose(**{
        "Hips": {"r": (-90, 200, 0), "l": (0, -0.80, 0)},
        "Head": {"r": (10, 25, 0)},
        "UpperLeg.L": {"r": (0, 0, -18)},
        "UpperLeg.R": {"r": (-10, 0, 14)},
        "LowerLeg.R": {"r": (25, 0, 0)},
        "UpperArm.L": {"r": (-160, 0, -50)},
        "UpperArm.R": {"r": (-170, 0, 60)},
        "LowerArm.L": {"r": (-30, 0, 0)},
    })
    bounce = pose(flat, Hips={"r": (-82, 200, 0), "l": (0, -0.72, 0)})
    acts.append(rrkit.clip(arm, "Death", [(0, a), (8, spin), (18, buckle), (28, flat),
                                          (33, bounce), (40, flat)], loop=False))
    return acts


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    m = materials()
    body = rrkit.Body("Goon")
    build(body, m)
    obj = body.object()
    arm = rrkit.armature("GoonArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        # The cannon in the fist, as models.ron attaches it.
        gun = rrkit.Body("PreviewCannon")
        gun.xf = Matrix.Translation(GRIP)
        hand_cannon.build(gun, "Hand.R", hand_cannon.materials())
        rrkit.skin(gun.object(), arm)
        rrkit.preview(prev, "goon", arm, HEIGHT, acts)


if __name__ == "__main__":
    main()
