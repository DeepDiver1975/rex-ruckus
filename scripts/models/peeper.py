# The Peeper (Drone): a bloodshot eyeball in a riveted, rusty lantern cage, held up by two
# rotors, with a gun turret slung underneath. A steel lid squints and blinks; the iris glows.
# The game hovers it (its feet are the hover height), bobs it and rolls it as it dies.
#
# Bones: `Body` points up (+X tilts it forwards); `Eye`, `Lid` and `Gun` point forwards (+X
# pitches them down, Z yaws them); `Rotor.L/R` point up and spin about Y. `Muzzle` is the
# gun's barrel tip.
#
# Usage: blender -b --factory-startup --python scripts/models/peeper.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402

HEIGHT = 0.62
EYE_Z, EYE_R = 0.32, 0.19
ROTOR_X, ROTOR_Z = 0.21, 0.56
GUN_Z = 0.07
MUZZLE = (0, -0.40, GUN_Z)


def bones():
    return [
        ("Body", (0, 0, EYE_Z), (0, 0, EYE_Z + 0.2), None, True),
        # Forward bones tilt down a touch: an exactly level bone gets Blender's other roll, which
        # would flip the sense of X.
        ("Eye", (0, 0, EYE_Z), (0, -0.2, EYE_Z - 0.01), "Body", True),
        ("Lid", (0, 0, EYE_Z), (0, -0.2, EYE_Z - 0.01), "Body", True),
        ("Gun", (0, -0.02, GUN_Z), (0, -0.2, GUN_Z - 0.01), "Body", True),
        ("Rotor.L", (ROTOR_X, 0, ROTOR_Z), (ROTOR_X, 0, ROTOR_Z + 0.06), "Body", True),
        ("Rotor.R", (-ROTOR_X, 0, ROTOR_Z), (-ROTOR_X, 0, ROTOR_Z + 0.06), "Body", True),
        ("Muzzle", MUZZLE, (0, MUZZLE[1] - 0.05, GUN_Z), "Gun", False),
    ]


def materials():
    return {
        "white": rrkit.material("PeeperEyeWhite", (226, 218, 196), rough=0.35),
        "vein": rrkit.material("PeeperVein", (170, 22, 32)),
        "iris": rrkit.material("PeeperIris", (255, 150, 20), rough=0.2, emit=3.0),
        "pupil": rrkit.material("PeeperPupil", (10, 8, 8), rough=0.2),
        "steel": rrkit.material("PeeperSteel", (60, 62, 68), rough=0.55, metal=0.7),
        "worn": rrkit.material("PeeperSteelWorn", (100, 96, 88), rough=0.7, metal=0.6),
        "rust": rrkit.material("PeeperRust", (104, 58, 34), rough=0.9),
        "rivets": rrkit.material("PeeperRivets", (150, 148, 140), rough=0.45, metal=0.9),
        "rotor": rrkit.material("PeeperRotor", (38, 38, 42), rough=0.6),
        "hazard": rrkit.material("PeeperHazard", (170, 128, 30), rough=0.8),
    }


def build(body, m):
    st, wo, ru = m["steel"], m["worn"], m["rust"]
    # The eyeball: veins creeping in from the back towards a glowing iris.
    body.ball("Eye", m["white"], (0, 0, EYE_Z), EYE_R, sub=2)
    body.cyl("Eye", m["iris"], (0, -EYE_R + 0.025, EYE_Z), 0.09, 0.08, 0.04, rot=(90, 0, 0), seg=10)
    body.cyl("Eye", m["pupil"], (0, -EYE_R - 0.0, EYE_Z), 0.045, 0.04, 0.02, rot=(90, 0, 0), seg=8)
    for i in range(10):
        a = math.radians(i * 36 + 10)
        # Each vein: two short segments on the surface, from the side towards the iris.
        p0 = (math.cos(a) * 0.15, -0.10, EYE_Z + math.sin(a) * 0.15)
        p1 = (math.cos(a) * 0.10, -0.165, EYE_Z + math.sin(a) * 0.10)
        p2 = (math.cos(a + 0.3) * 0.18, 0.02, EYE_Z + math.sin(a + 0.3) * 0.18)
        body.between("Eye", m["vein"], p2, p0, 0.008, 0.006, seg=3)
        body.between("Eye", m["vein"], p0, p1, 0.006, 0.004, seg=3)
    # Lantern cage: caps top and bottom, bars round the back and sides (the front is open).
    body.cyl("Body", st, (0, 0, 0.53), 0.15, 0.12, 0.06, seg=8, bevel=0.01)
    body.cyl("Body", st, (0, 0, 0.11), 0.12, 0.15, 0.06, seg=8, bevel=0.01)
    body.cyl("Body", ru, (0, 0, 0.075), 0.10, 0.10, 0.02, seg=8)
    for deg in (60, 100, 140, 180, 220, 260, 300):
        a = math.radians(deg)
        x, y = math.sin(a) * 0.215, -math.cos(a) * 0.215
        body.box("Body", wo if deg % 80 else st, (x, y, EYE_Z), (0.035, 0.035, 0.40),
                 rot=(0, 0, -deg))
        body.box("Body", m["rivets"], (x * 1.1, y * 1.1, 0.50), (0.025, 0.025, 0.025))
        body.box("Body", m["rivets"], (x * 1.1, y * 1.1, 0.14), (0.025, 0.025, 0.025))
    # Rotor arms and pods.
    for s in (1, -1):
        body.box("Body", st, (s * 0.12, 0, 0.55), (0.20, 0.05, 0.035), bevel=0.008)
        body.cyl("Body", wo, (s * ROTOR_X, 0, ROTOR_Z - 0.01), 0.045, 0.04, 0.06, seg=8)
        body.box("Body", m["hazard"], (s * ROTOR_X, 0, ROTOR_Z - 0.035), (0.095, 0.095, 0.012))
        # Stubby side fins.
        body.box("Body", st, (s * 0.25, 0.06, EYE_Z - 0.04), (0.12, 0.16, 0.025),
                 rot=(0, s * 55, 0), bevel=0.006)
        body.box("Body", m["rivets"], (s * 0.27, 0.06, EYE_Z - 0.01), (0.022, 0.022, 0.022))
    for sfx, s in ((".L", 1), (".R", -1)):
        bone = "Rotor" + sfx
        body.cyl(bone, m["rotor"], (s * ROTOR_X, 0, ROTOR_Z + 0.035), 0.022, 0.022, 0.04, seg=6)
        for r in (0, 90):
            body.box(bone, m["rotor"], (s * ROTOR_X, 0, ROTOR_Z + 0.05), (0.30, 0.035, 0.008),
                     rot=(0, 0, r + 15))
    # The lid: a steel shell over the top front of the eye, closing forwards and down.
    body.box("Lid", wo, (0, -0.06, EYE_Z + 0.17), (0.30, 0.26, 0.05), top=(0.75, 0.75),
             bevel=0.01)
    body.box("Lid", m["rivets"], (0, -0.19, EYE_Z + 0.17), (0.03, 0.025, 0.025))
    # Gun turret slung underneath.
    body.box("Gun", st, (0, -0.02, GUN_Z), (0.14, 0.18, 0.09), top=(0.85, 0.9), bevel=0.012)
    body.between("Gun", wo, (0, -0.10, GUN_Z), (0, MUZZLE[1] + 0.03, GUN_Z), 0.028, 0.024, seg=6)
    body.box("Gun", ru, (0, MUZZLE[1] + 0.02, GUN_Z), (0.06, 0.05, 0.06), bevel=0.008)


# --- clips ---------------------------------------------------------------------------------


def pose(*parts, **extra):
    out = {}
    for p in parts + (extra,):
        for bone, k in p.items():
            out[bone] = {**out.get(bone, {}), **k}
    return out


def spun(p, frame, speed):
    """Pose p with the rotors turned `speed` degrees per frame at `frame` (opposite ways)."""
    a = frame * speed
    return pose(p, **{"Rotor.L": {"r": (0, a, 0)}, "Rotor.R": {"r": (0, -a, 0)}})


def track(frames, speed, poses):
    """Keys every frame in `frames`, each from the latest of `poses` (frame -> pose) at or
    before it, with the rotors spinning."""
    keys = []
    for f in frames:
        base = max((k for k in poses if k <= f), default=min(poses))
        keys.append((f, spun(poses[base], f, speed)))
    return keys


def clips(arm):
    acts = []
    # Idle: eye darting about, a blink; the rotors turn 3 times in the loop (36 deg/frame).
    look = {"Eye": {"r": (0, 0, 0)}}
    idle = {
        0: look,
        8: {"Eye": {"r": (-6, 0, 28)}},
        14: {"Eye": {"r": (8, 0, -24)}},
        22: {"Eye": {"r": (0, 0, 0)}},
        24: {"Eye": {"r": (0, 0, 0)}, "Lid": {"r": (70, 0, 0)}},
        26: look,
    }
    acts.append(rrkit.clip(arm, "Idle", track(range(0, 31, 2), 36, idle), linear=True,
                           closed=True))
    # Moving: leaning into it, eye locked on.
    move = {0: {"Body": {"r": (12, 0, 0)}, "Eye": {"r": (-12, 0, 0)}, "Lid": {"r": (15, 0, 0)}}}
    acts.append(rrkit.clip(arm, "Walk", track(range(0, 21, 2), 36, move), linear=True,
                           closed=True))
    dash = {0: {"Body": {"r": (24, 0, 0)}, "Eye": {"r": (-20, 0, 0)}, "Lid": {"r": (25, 0, 0)},
                "Gun": {"r": (-10, 0, 0)}}}
    acts.append(rrkit.clip(arm, "Run", track(range(0, 21, 2), 54, dash), linear=True,
                           closed=True))
    # Shoot: squint, recoil, iris flaring is the tip glow.
    squint = {"Lid": {"r": (32, 0, 0)}, "Eye": {"r": (-4, 0, 0)}}
    shoot = {
        0: squint,
        2: pose(squint, Gun={"l": (0, -0.06, 0), "r": (-12, 0, 0)}, Body={"r": (-10, 0, 0)}),
        6: squint,
    }
    acts.append(rrkit.clip(arm, "Attack", track(range(0, 13, 1), 36, shoot), loop=False,
                           linear=True))
    # Pain: the eye rolls and the whole thing wobbles.
    pain = {
        0: look,
        2: {"Eye": {"r": (-40, 0, 20)}, "Body": {"r": (-10, 0, 18)}, "Lid": {"r": (50, 0, 0)}},
        5: {"Eye": {"r": (-30, 0, -25)}, "Body": {"r": (6, 0, -14)}},
        9: look,
    }
    acts.append(rrkit.clip(arm, "Pain", track(range(0, 11, 1), 36, pain), loop=False,
                           linear=True))
    # Death: the eye rolls up, the lid slams, the rotors wind down, it droops.
    keys = []
    angle = 0.0
    for f in range(0, 19):
        angle += max(0.0, 36 - f * 2.2)
        t = min(1.0, f / 10)
        keys.append((f, pose(**{
            "Eye": {"r": (-75 * t, 0, 15 * t)},
            "Lid": {"r": (48 * min(1.0, f / 6), 0, 0)},
            "Body": {"r": (35 * t, 0, 0)},
            "Gun": {"r": (40 * t, 0, 0)},
            "Rotor.L": {"r": (0, angle, 0)},
            "Rotor.R": {"r": (0, -angle, 0)},
        })))
    acts.append(rrkit.clip(arm, "Death", keys, loop=False, linear=True))
    return acts


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Peeper")
    build(body, materials())
    obj = body.object()
    arm = rrkit.armature("PeeperArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        rrkit.preview(prev, "peeper", arm, HEIGHT, acts)


if __name__ == "__main__":
    main()
