# The Hive Queen (Episode 2's final boss): the matriarch of a burrowing alien hive. A heavy,
# low, armoured crawler on six stubby jointed legs, built like a giant woodlouse or dung beetle
# rather than a lanky insect. Plum carapace in overlapping bands over a pale fleshy underside;
# at the back a swollen, glowing amber egg sac strapped down by dark ridged plates. The forebody
# rears up to a blunt, flat-fronted armoured head with a horseshoe of small amber eye-bulbs, a
# pair of wide horizontal mandibles that open sideways to spit acid (the `Mouth` bone between
# them is the muzzle) and a crown of short, stubby fleshy fronds tipped with amber nodes.
#
# Bones: `Body` (the thorax, root) and `Fore` (the rearing forebody) and `Crown` point up: +X
# leans them forwards, Z rolls them, Y turns them; `Body`'s location Y lifts or sinks the whole
# queen (Burrow ends below the floor). `Head` and `Mandible.L/R` point forwards (tilted down a
# touch, see rrkit.armature): +X pitches down, Z yaws (negative = towards +X, so the left
# mandible opens with -Z). `Abdomen` points backwards: +X lifts the sac. The six legs are
# `UpperLeg1..3` / `LowerLeg1..3` (.L/.R, 1 = front), hanging straight down from hip and knee
# like the biped limbs: Y swings a leg forwards or back, Z lifts it out or folds it in (see
# `leg()` below for the signs).
#
# Usage: blender -b --factory-startup --python scripts/models/hive_queen.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402
from mathutils import Vector  # noqa: E402

HEIGHT = 2.6  # the crown's top; the head itself is about 2.3
PREVIEW_HEIGHT = 4.0  # framing: the queen is longer than she is tall
BODY = (0, 0.05, 0.80)  # the thorax centre
FORE_Z = 0.95
HEAD = (0, -0.62, 1.86)  # the neck joint
FACE_Y = -1.12  # the front of the head
MANDIBLE = (0.32, -1.00, 1.78)  # the left mandible's hinge
MOUTH = (0, -1.24, 1.78)
SAC = (0, 0.98, 0.82)  # the egg sac's centre
SAC_R, SAC_SCALE = 0.78, (1.05, 1.0, 0.9)
# Legs: (number, hip y, knee y, foot y). Hips sit under the thorax rim, knees out and up,
# feet splayed forwards for the front pair and backwards for the rear.
LEGS = [(1, -0.40, -0.65, -1.00), (2, 0.05, 0.05, 0.08), (3, 0.48, 0.72, 1.05)]
HIP_X, HIP_Z = 0.58, 0.62
KNEE_X, KNEE_Z = 1.02, 0.80
FOOT_X = 1.22
LOW_T, HIGH_T, LOW, HIGH = (48, -30), (55, 20), (54, -40), (60, 45)


def bones():
    bx, by, bz = BODY
    hx, hy, hz = HEAD
    b = [
        ("Body", (bx, by, bz), (bx, by, bz + 0.4), None, True),
        ("Fore", (0, -0.40, FORE_Z), (0, -0.40, FORE_Z + 0.6), "Body", True),
        ("Head", HEAD, (hx, hy - 0.3, hz - 0.01), "Fore", True),
        ("Crown", (0, -0.78, 2.20), (0, -0.78, 2.40), "Head", True),
        ("Abdomen", (0, 0.40, 0.85), (0, 1.30, 0.75), "Body", True),
        # Pointing forwards, tilted down a little (see rrkit.armature).
        ("Mouth", MOUTH, (0, MOUTH[1] - 0.1, MOUTH[2] - 0.01), "Head", False),
    ]
    for s, sfx in ((1, ".L"), (-1, ".R")):
        mx, my, mz = MANDIBLE
        b.append(("Mandible" + sfx, (s * mx, my, mz), (s * mx, my - 0.3, mz - 0.01), "Head",
                  True))
        for n, hy_, ky, _ in LEGS:
            up, lo = f"UpperLeg{n}{sfx}", f"LowerLeg{n}{sfx}"
            b.append((up, (s * HIP_X, hy_, HIP_Z), (s * HIP_X, hy_, HIP_Z - 0.4), "Body", True))
            b.append((lo, (s * KNEE_X, ky, KNEE_Z), (s * KNEE_X, ky, KNEE_Z - 0.4), up, True))
    return b


def materials():
    return {
        "shell": rrkit.material("QueenCarapace", (92, 48, 104), rough=0.45),
        "dark": rrkit.material("QueenCarapaceDark", (50, 26, 62), rough=0.5),
        "rim": rrkit.material("QueenCarapaceRim", (140, 92, 150), rough=0.5),
        "flesh": rrkit.material("QueenUnderside", (214, 178, 166), rough=0.7),
        "pink": rrkit.material("QueenFronds", (196, 120, 138), rough=0.7),
        "sac": rrkit.material("QueenEggSac", (255, 168, 52), rough=0.25, emit=1.8),
        "eggs": rrkit.material("QueenEggs", (255, 214, 120), rough=0.3, emit=2.5),
        "eyes": rrkit.material("QueenEyes", (255, 190, 40), rough=0.2, emit=5.0),
        "acid": rrkit.material("QueenAcid", (130, 236, 60), rough=0.2, emit=3.5),
        "maw": rrkit.material("QueenMaw", (70, 14, 36)),
        "tusk": rrkit.material("QueenMandible", (214, 190, 150), rough=0.4),
        "fang": rrkit.material("QueenClaw", (36, 22, 34), rough=0.35),
    }


def yaw_to(dx, dy):
    """The Z rotation (degrees) that turns a box's +Y axis along (dx, dy)."""
    return math.degrees(math.atan2(-dx, dy))


def surface(centre, radii, dy, a, out=0.0):
    """A point on the ellipsoid (`centre`, `radii`) `dy` along it (Y) from its centre and `a`
    degrees round from the top (positive towards +X), pushed `out` metres further out; and the
    ellipsoid's relative radius at `dy`."""
    cx, cy, cz = centre
    rr = math.sqrt(max(0.0, 1.0 - (dy / radii[1]) ** 2))
    r = math.radians(a)
    return (cx + (radii[0] * rr + out) * math.sin(r), cy + dy,
            cz + (radii[2] * rr + out) * math.cos(r)), rr


def loft(body, bone, mat, rings):
    """A closed tube through `rings` (each a list of points, all the same length), capped at
    both ends: curved plates and blades the box and cylinder parts can't make."""
    vs = [[body.bm.verts.new(Vector(p)) for p in r] for r in rings]
    n = len(vs[0])
    for a, b in zip(vs, vs[1:]):
        for j in range(n):
            body.bm.faces.new((a[j], a[(j + 1) % n], b[(j + 1) % n], b[j]))
    body.bm.faces.new(vs[0])
    body.bm.faces.new(list(reversed(vs[-1])))
    return body._finish([v for r in vs for v in r], bone, mat, 0.0)


def plates(body, bone, mats, centre, radii, bands, span, width, knobs=None, steps=8,
           flat=False):
    """Shingled armour plates over an ellipsoid: at each of the `bands` offsets along Y, a
    curved plate following the surface round +-`span` degrees, widest and thickest on the
    ridge and tapering down towards the legs. Each plate's rear edge lifts off the surface so
    it laps over the next plate behind it. Plates alternate mats[0] and mats[1]."""
    for i, dy in enumerate(bands):
        rings = []
        for k in range(steps + 1):
            a = -span + k * 2 * span / steps
            if flat:
                t = 0.7 + 0.3 * math.sqrt(max(0.0, 1.0 - (a / span) ** 2))
                w = width * t
                lo, hi, lo_r, hi_r = -0.01, 0.035, 0.0, 0.045
            else:
                t = 1.0 - 0.5 * (abs(a) / span) ** 1.5
                w, th = width * t, 0.035 + 0.035 * t
                lo, hi = -0.01, 0.015 + th * 0.5
                lo_r, hi_r = 0.03 + 0.05 * t, 0.03 + th + 0.05 * t
            front_out, _ = surface(centre, radii, dy - w / 2, a, hi)
            rear_out, _ = surface(centre, radii, dy + w / 2, a, hi_r)
            rear_in, _ = surface(centre, radii, dy + w / 2, a, lo_r)
            front_in, _ = surface(centre, radii, dy - w / 2, a, lo)
            rings.append([front_out, rear_out, rear_in, front_in])
        loft(body, bone, mats[i % 2], rings)
        if knobs:
            p, _ = surface(centre, radii, dy + width * 0.1, 0, 0.09)
            body.box(bone, knobs, p, (0.10, width * 0.5, 0.05), top=(0.6, 0.8), bevel=0.015)


def shell(body, bone, mats, centre, radii, bands, span, width, knobs=None, sub=2):
    """An ellipsoid dome in mats[0] under shingled plates in mats[1], mats[2] (see plates)."""
    body.ball(bone, mats[0], centre, 1.0, scale=radii, sub=sub)
    plates(body, bone, mats[1:], centre, radii, bands, span, width, knobs)


def sickle(s, t):
    """A point and plan-view direction along the left (s = 1) or right (s = -1) mandible's
    sickle curve, t from 0 (hinge) to 1 (tip): out, forwards, then hooking back in."""
    p0, p1, p2 = Vector((MANDIBLE[0], MANDIBLE[1] + 0.04)), Vector((0.70, -1.34)), \
        Vector((0.16, -1.74))
    p = p0 * (1 - t) ** 2 + p1 * 2 * t * (1 - t) + p2 * t * t
    d = ((p1 - p0) * 2 * (1 - t) + (p2 - p1) * 2 * t).normalized()
    return Vector((s * p.x, p.y)), Vector((s * d.x, d.y))


def build(body, m):
    sh, dk, rim, fl = m["shell"], m["dark"], m["rim"], m["flesh"]
    bx, by, bz = BODY
    # Thorax: a wide, low domed shell banded with plates over a pale belly.
    body.box("Body", fl, (bx, by, 0.60), (1.16, 1.06, 0.26), top=(1.05, 1.0), bevel=0.05)
    shell(body, "Body", (dk, sh, sh), (bx, by + 0.02, 0.74), (0.78, 0.64, 0.42),
          (-0.42, -0.15, 0.12, 0.38), 80, 0.31, knobs=rim)
    for s in (1, -1):  # coxa sockets under the rim
        for _, hy_, _, _ in LEGS:
            body.ball("Body", fl, (s * HIP_X, hy_, HIP_Z), 0.19, scale=(1.1, 1.0, 0.9))

    # The forebody: a high, humped dome of banded carapace that the head juts from, pale
    # underneath.
    shell(body, "Fore", (dk, sh, sh), (0, -0.34, 1.36), (0.74, 0.52, 0.66),
          (-0.38, -0.14, 0.10, 0.34), 80, 0.28, knobs=rim)
    body.ball("Fore", fl, (0, -0.60, 1.02), 1.0, scale=(0.56, 0.30, 0.30))  # pale throat

    # The head: blunt and flat-fronted, wider than tall, under a stack of short shield plates.
    body.box("Head", sh, (0, -0.86, 1.90), (0.86, 0.52, 0.50), top=(0.85, 0.9), front=0.9,
             bevel=0.05)
    body.box("Head", fl, (0, -0.88, 1.68), (0.62, 0.40, 0.14), bevel=0.03)  # throat
    body.box("Head", dk, (0, -0.80, 2.16), (0.94, 0.48, 0.12), top=(0.8, 0.85), bevel=0.03)
    body.box("Head", rim, (0, -1.04, 2.12), (0.88, 0.08, 0.10), bevel=0.02)  # brow lip
    for s in (1, -1):  # cheek plates
        body.box("Head", dk, (s * 0.45, -0.92, 1.86), (0.10, 0.42, 0.36), top=(1.0, 0.8),
                 bevel=0.02)
        # Acid glands bulging under the cheeks.
        body.ball("Head", m["acid"], (s * 0.40, -1.05, 1.68), 0.08, scale=(1.0, 1.2, 0.8))
    # The face: a dark maw low in the middle, a horseshoe of eye-bulbs arching over it.
    body.box("Head", m["maw"], (0, FACE_Y + 0.02, 1.78), (0.34, 0.06, 0.18), bevel=0.02)
    body.box("Head", m["acid"], (0, FACE_Y, 1.74), (0.22, 0.04, 0.04))
    for i in range(9):
        a = math.radians(-20 + i * 27.5)
        r = 0.065 if i in (0, 4, 8) else 0.05
        body.ball("Head", m["eyes"], (0.30 * math.cos(a), FACE_Y - 0.01,
                                      1.84 + 0.20 * math.sin(a)), r)
    for x, ln in ((-0.08, 0.16), (0.05, 0.22), (0.12, 0.10)):  # acid drool
        body.spike("Head", m["acid"], (x, FACE_Y - 0.01, 1.72), 0.025, ln, rot=(180, 0, 0))

    # Mandibles: wide, flat, pale sickles from the cheeks, bowing out and forwards and hooking
    # back in, lens-shaped in section, tapering to dark points, serrated along the inner edge.
    mx, my, mz = MANDIBLE
    for s, sfx in ((1, ".L"), (-1, ".R")):
        bone = "Mandible" + sfx
        rings = []
        for i in range(9):
            t = 0.86 * i / 8
            c, d = sickle(s, t)
            out = Vector((-d.y, d.x)) * s  # in plan view, away from the middle
            if out.x * s < 0:
                out = -out
            w, h = 0.20 - 0.13 * t, 0.15 - 0.10 * t
            ring = []
            for u, v in ((0.5, 0.0), (0.22, 0.5), (-0.22, 0.5), (-0.5, 0.0), (-0.22, -0.5),
                         (0.22, -0.5)):
                q = c + out * (u * w)
                ring.append((q.x, q.y, mz + v * h))
            rings.append(ring)
        loft(body, bone, m["tusk"], rings)
        c, _ = sickle(s, 0.86)
        tip, _ = sickle(s, 1.0)
        body.spike_to(bone, m["fang"], (c.x, c.y, mz), (tip.x, tip.y - 0.02, mz - 0.03), 0.05,
                      seg=5)
        body.ball(bone, dk, (s * mx, my + 0.02, mz), 0.11)
        # Serrated inner edge: teeth pointing in, towards the middle.
        for t in (0.34, 0.50, 0.64, 0.76):
            c, d = sickle(s, t)
            inward = Vector((d.y, -d.x)) * s
            if inward.x * s > 0:
                inward = -inward
            w = 0.20 - 0.13 * t
            a_ = c + inward * (w * 0.35)
            b_ = c + inward * (w * 0.5 + 0.09)
            body.spike_to(bone, m["fang"], (a_.x, a_.y, mz), (b_.x, b_.y, mz), 0.035)

    # The crown: short, stubby fleshy fronds on top of the head, each with an amber node.
    for x, y, ox, oy, h in ((0.0, -0.96, 0.0, -0.10, 0.22), (0.24, -0.90, 0.16, -0.04, 0.20),
                            (-0.24, -0.90, -0.16, -0.04, 0.20), (0.36, -0.74, 0.18, 0.06, 0.16),
                            (-0.36, -0.74, -0.18, 0.06, 0.16), (0.13, -0.70, 0.06, 0.12, 0.26),
                            (-0.13, -0.70, -0.06, 0.12, 0.26)):
        a = (x, y, 2.20)
        mid = (x + ox * 0.5, y + oy * 0.5, 2.20 + h * 0.6)
        tip = (x + ox, y + oy, 2.20 + h)
        body.between("Crown", m["pink"], a, mid, 0.075, 0.06, seg=6)
        body.between("Crown", m["pink"], mid, tip, 0.06, 0.045, seg=6)
        body.ball("Crown", m["eggs"], tip, 0.06)

    # The egg sac: a swollen amber bulb with eggs pressing at its skin, under shingled plates.
    sac = tuple(SAC_R * k for k in SAC_SCALE)
    body.ball("Abdomen", m["sac"], SAC, 1.0, scale=sac, sub=3)
    for dy, a in ((-0.25, 100), (-0.2, -105), (0.15, 95), (0.05, -95), (0.45, 110),
                  (0.42, -115), (0.62, 160), (0.66, -150), (-0.05, 130), (0.25, -135)):
        p, _ = surface(SAC, sac, dy, a, -0.03)
        body.ball("Abdomen", m["eggs"], p, 0.12, scale=(1.0, 1.0, 1.2))
    plates(body, "Abdomen", (dk, sh), SAC, sac, (-0.50, -0.25, 0.0, 0.25, 0.48), 46, 0.22,
           flat=True)
    body.spike_to("Abdomen", fl, (0, 1.70, 0.72), (0, 1.94, 0.58), 0.13, seg=6)

    # Six stubby, thick legs: an armoured thigh out to a high knee, a shin down to a blunt claw.
    for s, sfx in ((1, ".L"), (-1, ".R")):
        for n, hy_, ky, fy in LEGS:
            up, lo = f"UpperLeg{n}{sfx}", f"LowerLeg{n}{sfx}"
            hip = Vector((s * HIP_X, hy_, HIP_Z))
            knee = Vector((s * KNEE_X, ky, KNEE_Z))
            foot = Vector((s * FOOT_X, fy, 0.10))
            body.between(up, fl, hip, knee, 0.19, 0.16, seg=6)
            mid = (hip + knee) / 2
            d = knee - hip
            body.box(up, sh, (mid.x, mid.y, mid.z + 0.12), (0.30, d.length + 0.08, 0.14),
                     rot=(0, s * 20, yaw_to(d.x, d.y)), top=(0.7, 0.9), bevel=0.03)
            body.ball(lo, dk, knee, 0.18)
            body.between(lo, sh, knee, foot, 0.16, 0.11, seg=6)
            k2 = knee + (foot - knee) * 0.4
            body.spike_to(lo, dk, k2 + Vector((s * 0.08, 0, 0)),
                          k2 + Vector((s * 0.24, 0, 0.10)), 0.06)  # shin spur
            body.cyl(lo, dk, foot, 0.14, 0.11, 0.12, seg=6)
            for dy in (-0.08, 0.08):
                body.spike_to(lo, m["fang"], foot + Vector((0, dy, -0.02)),
                              foot + Vector((s * 0.16, dy * 1.6, -0.10)), 0.045)


# --- clips ---------------------------------------------------------------------------------


def pose(*parts, **extra):
    out = {}
    for p in parts + (extra,):
        for bone, k in p.items():
            out[bone] = {**out.get(bone, {}), **k}
    return out


SIDES = ((1, ".L"), (-1, ".R"))


def leg(n, s, sfx, swing=0.0, lift=0.0, fold=0.0):
    """One leg: `swing` forwards (degrees, negative = back), `lift` the thigh up and out,
    `fold` the shin in under the body (negative splays it out)."""
    return {
        f"UpperLeg{n}{sfx}": {"r": (0, s * swing, -s * lift)},
        f"LowerLeg{n}{sfx}": {"r": (0, 0, s * fold)},
    }


def legs(fn):
    """Merges fn(n, s, sfx) -> leg() for all six legs."""
    out = {}
    for s, sfx in SIDES:
        for n, _, _, _ in LEGS:
            out.update(fn(n, s, sfx))
    return out


def jaws(open_deg):
    return {"Mandible.L": {"r": (0, 0, -open_deg)}, "Mandible.R": {"r": (0, 0, open_deg)}}


# The two tripods of the gait: front and rear on one side with the middle of the other.
TRIPOD_A = {(1, ".L"), (2, ".R"), (3, ".L")}


def clips(arm):
    acts = []
    a = pose(jaws(4), Head={"r": (4, 6, 0)}, Crown={"r": (0, 0, 6)})
    b = pose(jaws(14), Body={"l": (0, -0.03, 0)}, Fore={"r": (-4, 0, 0)},
             Head={"r": (-6, -8, 4)}, Crown={"r": (6, 0, -8)}, Abdomen={"r": (4, 0, 0)})
    c = pose(a, jaws(8), Head={"r": (0, 10, -3)}, Abdomen={"r": (-2, 0, 0)})
    acts.append(rrkit.clip(arm, "Idle", [(0, a), (20, b), (32, c), (48, b)]))

    def gait(t, amp, lift, bob, sway):
        """Tripod gait at phase t (0..1): tripod A swings forwards lifted while B pushes back."""
        def one(n, s, sfx):
            ph = t if (n, sfx) in TRIPOD_A else (t + 0.5) % 1.0
            sw = amp * math.cos(2 * math.pi * ph)  # +amp: forward, at phase 0
            # cos falls from +amp to -amp over the first half: that half is the push (feet
            # down); the second half swings forwards with the foot lifted.
            up = lift * max(0.0, -math.sin(2 * math.pi * ph))
            return leg(n, s, sfx, swing=sw, lift=up, fold=-up * 0.4)
        return pose(legs(one), jaws(6),
                    Body={"l": (0, bob * abs(math.sin(2 * math.pi * t)), 0),
                          "r": (0, 0, sway * math.sin(2 * math.pi * t))},
                    Fore={"r": (6, 0, -sway * 0.6 * math.sin(2 * math.pi * t))},
                    Head={"r": (2, 0, 0)},
                    Abdomen={"r": (0, 0, -sway * 1.5 * math.sin(2 * math.pi * t))},
                    Crown={"r": (-8, 0, 0)})

    acts.append(rrkit.clip(arm, "Walk", [(f, gait(f / 32, 18, 22, 0.04, 3)) for f in
                                          range(0, 32, 4)]))
    acts.append(rrkit.clip(arm, "Run", [(f, pose(gait(f / 18, 26, 30, 0.07, 4),
                                                 Fore={"r": (14, 0, 0)}, Head={"r": (-6, 0, 0)},
                                                 Crown={"r": (-18, 0, 0)}))
                                         for f in range(0, 18, 3)]))

    # Attack: rear back with the mandibles spread wide, then thrust the head and spit.
    rear = pose(jaws(30), Body={"l": (0, 0.05, 0)}, Fore={"r": (-20, 0, 0)},
                Head={"r": (-22, 0, 0)}, Crown={"r": (-20, 0, 0)}, Abdomen={"r": (-6, 0, 0)},
                **legs(lambda n, s, sfx: leg(n, s, sfx, swing=4 if n == 1 else 0, fold=-6)))
    spit = pose(jaws(42), Body={"l": (0, -0.08, 0)}, Fore={"r": (16, 0, 0)},
                Head={"r": (-10, 0, 0), "l": (0, 0.22, 0)}, Crown={"r": (20, 0, 0)},
                Abdomen={"r": (6, 0, 0)},
                **legs(lambda n, s, sfx: leg(n, s, sfx, swing=-6 if n == 1 else 0, fold=-10)))
    acts.append(rrkit.clip(arm, "Attack", [(0, a), (7, rear), (11, spit), (16, spit),
                                           (28, a)], loop=False))

    hurt = pose(jaws(26), Body={"r": (0, 0, -8), "l": (0, -0.06, 0)}, Fore={"r": (-16, 10, 0)},
                Head={"r": (-26, -18, 8)}, Crown={"r": (-25, 0, 10)}, Abdomen={"r": (10, 0, 6)},
                **legs(lambda n, s, sfx: leg(n, s, sfx, lift=10, fold=-12)))
    acts.append(rrkit.clip(arm, "Pain", [(0, a), (4, hurt), (16, a)], loop=False))

    # Burrow: crouch and splay, dig nose-first with the legs scrabbling, sink out of sight with
    # a side-to-side shudder, legs folding in. Ends with the crown well below the floor.
    crouch = pose(jaws(10), Body={"l": (0, -0.25, 0)}, Fore={"r": (18, 0, 0)},
                  Head={"r": (20, 0, 0)}, Crown={"r": (-20, 0, 0)},
                  **legs(lambda n, s, sfx: leg(n, s, sfx, lift=14, fold=-18)))

    def dig(depth, roll, fold, scrab):
        def one(n, s, sfx):
            sw = scrab if (n, sfx) in TRIPOD_A else -scrab
            return leg(n, s, sfx, swing=sw, lift=20 + fold * 0.4, fold=fold)
        return pose(legs(one), jaws(6), Body={"l": (0, -depth, 0), "r": (8, 0, roll)},
                    Fore={"r": (30, 0, -roll)}, Head={"r": (30, 0, 0)},
                    Crown={"r": (-35, 0, roll)}, Abdomen={"r": (10, 0, -roll)})

    acts.append(rrkit.clip(arm, "Burrow", [
        (0, a), (6, crouch),
        (11, dig(0.55, 6, 0, 20)), (16, dig(1.0, -7, 20, -20)), (21, dig(1.6, 7, 45, 20)),
        (26, dig(2.2, -6, 70, -15)), (31, dig(2.8, 4, 90, 10)), (36, dig(3.3, 0, 100, 0)),
    ], loop=False))

    # Emerge: from below the floor (Burrow's end), the head bursts up rearing with mandibles
    # wide, the body pops above its stance, the legs fling out and slam down.
    under = dig(3.3, 0, 100, 0)
    burst = pose(jaws(36), Body={"l": (0, -1.3, 0), "r": (-10, 0, 0)}, Fore={"r": (-24, 0, 0)},
                 Head={"r": (-30, 0, 0)}, Crown={"r": (25, 0, 0)}, Abdomen={"r": (-12, 0, 0)},
                 **legs(lambda n, s, sfx: leg(n, s, sfx, swing=10 if n == 1 else 0, lift=40,
                                              fold=50)))
    pop = pose(jaws(40), Body={"l": (0, 0.12, 0), "r": (-8, 0, 0)}, Fore={"r": (-22, 0, 0)},
               Head={"r": (-26, 0, 0)}, Crown={"r": (25, 0, 0)},
               **legs(lambda n, s, sfx: leg(n, s, sfx, swing=14 if n == 1 else
                                            (-14 if n == 3 else 0), lift=22, fold=-12)))
    slam = pose(jaws(20), Body={"l": (0, -0.18, 0), "r": (4, 0, 0)}, Fore={"r": (8, 0, 0)},
                Head={"r": (6, 0, 0)}, Crown={"r": (-10, 0, 0)},
                **legs(lambda n, s, sfx: leg(n, s, sfx, lift=6, fold=-14)))
    acts.append(rrkit.clip(arm, "Emerge", [(0, under), (8, burst), (13, pop), (18, slam),
                                           (30, a)], loop=False))

    # Death: rear up screeching, then collapse onto the belly and roll to one side, the legs
    # curling in under her and the sac sagging.
    shriek = pose(jaws(45), Body={"l": (0, 0.05, 0)}, Fore={"r": (-26, 0, 0)},
                  Head={"r": (-38, 10, 0)}, Crown={"r": (-30, 0, 0)}, Abdomen={"r": (-10, 0, 0)},
                  **legs(lambda n, s, sfx: leg(n, s, sfx, lift=18, fold=-10)))
    # The sac props her rear up as she settles; the legs fold up high before they curl in, so
    # none of them dips through the floor.
    slump = pose(jaws(30), Body={"l": (0, -0.30, 0), "r": (6, 0, 6)}, Fore={"r": (22, 0, 0)},
                 Head={"r": (28, -10, 0)}, Crown={"r": (30, 0, 0)},
                 Abdomen={"r": (-6, 0, 0), "l": (0, 0, 0.22)},
                 **legs(lambda n, s, sfx: leg(n, s, sfx, lift=42, fold=-40)))

    def curled(roll, drop, low, high):
        """`low` and `high`: (lift, fold) for the legs on the side she rolls onto and the other."""
        return pose(jaws(22), Body={"l": (0, -drop, 0), "r": (8, 0, roll)},
                    Fore={"r": (26, 0, 0)}, Head={"r": (34, -16, 0)}, Crown={"r": (40, 0, 20)},
                    Abdomen={"r": (-8, 0, 0), "l": (0, 0, drop - 0.06)},
                    **legs(lambda n, s, sfx: leg(n, s, sfx, (n - 2) * -12,
                                                 *(high if s * roll > 0 else low))))

    acts.append(rrkit.clip(arm, "Death", [(0, a), (7, shriek), (14, shriek), (22, slump),
                                          (27, curled(8, 0.24, LOW_T, HIGH_T)),
                                          (32, curled(16, 0.24, LOW, HIGH)),
                                          (38, curled(13, 0.23, LOW, HIGH)),
                                          (46, curled(16, 0.24, LOW, HIGH))],
                           loop=False))
    return acts


def main():
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("HiveQueen")
    build(body, materials())
    obj = body.object()
    arm = rrkit.armature("HiveQueenArmature", bones())
    rrkit.skin(obj, arm)
    acts = clips(arm)
    rrkit.export(out, [obj, arm])
    if prev:
        rrkit.preview(prev, "hive_queen", arm, PREVIEW_HEIGHT, acts)


if __name__ == "__main__":
    main()
