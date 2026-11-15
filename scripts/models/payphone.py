# The payphone gag prop: a wall-mounted steel phone under a little hood with a lit sign, a
# keypad, a coin slot, and a black handset on a coiled cord. Static mesh at real size: the
# origin is the middle of its footprint on the floor, its front faces -Y (glTF +Z), and its back
# plate lies flush with the wall at +Y = 0.15, so the 0.4 m wide, 0.3 m deep core box of
# `PropKind::Payphone` sits against the wall. The phone itself hangs at head height.
#
# Usage: blender -b --factory-startup --python scripts/models/payphone.py -- OUT.glb [--preview DIR]
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402

HEIGHT = 1.9
BONE = "Payphone"
# The wall the back plate rests on.
WALL_Y = 0.15


def materials():
    return {
        "steel": rrkit.material("PhoneSteel", (150, 156, 166), rough=0.4, metal=0.7),
        "chrome": rrkit.material("PhoneChrome", (205, 208, 215), rough=0.25, metal=0.9),
        "dark": rrkit.material("PhoneDark", (28, 28, 32), rough=0.6),
        "key": rrkit.material("PhoneKey", (225, 225, 230), rough=0.5),
        "sign": rrkit.material("PhoneSign", (60, 140, 255), emit=3.5),
    }


def build(b, m):
    # Back plate on the wall, and the hood with its lit sign.
    b.box(BONE, m["steel"], (0, WALL_Y - 0.015, 1.33), (0.40, 0.03, 0.96))
    b.box(BONE, m["steel"], (0, 0.01, 1.83), (0.40, 0.28, 0.05))
    b.box(BONE, m["sign"], (0, -0.125, 1.74), (0.36, 0.03, 0.12))
    for x in (-0.19, 0.19):
        b.box(BONE, m["steel"], (x, 0.01, 1.74), (0.02, 0.28, 0.16))
    # The phone body: a bevelled chrome housing.
    b.box(BONE, m["chrome"], (0.03, 0.05, 1.32), (0.26, 0.17, 0.52), bevel=0.015)
    # Keypad: a dark panel with a 3 x 4 grid of keys.
    b.box(BONE, m["dark"], (0.05, -0.04, 1.30), (0.13, 0.02, 0.17))
    for r in range(4):
        for c in range(3):
            b.box(BONE, m["key"], (0.01 + c * 0.04, -0.052, 1.36 - r * 0.04), (0.026, 0.012, 0.026))
    # Coin slot above, coin return below.
    b.box(BONE, m["dark"], (0.05, -0.04, 1.49), (0.07, 0.02, 0.025))
    b.box(BONE, m["dark"], (0.05, -0.04, 1.12), (0.08, 0.03, 0.06))
    # Handset on its cradle at the left, earpiece up, mouthpiece down.
    b.box(BONE, m["dark"], (-0.15, -0.01, 1.32), (0.05, 0.06, 0.24), bevel=0.01)
    b.cyl(BONE, m["dark"], (-0.15, -0.04, 1.44), 0.038, 0.038, 0.05, rot=(90, 0, 0), seg=8)
    b.cyl(BONE, m["dark"], (-0.15, -0.04, 1.20), 0.038, 0.038, 0.05, rot=(90, 0, 0), seg=8)
    # The coiled cord: a sagging zig-zag from the handset to the housing's foot.
    pts = [(-0.15, -0.01, 1.19), (-0.17, -0.02, 1.10), (-0.13, -0.03, 1.04), (-0.16, -0.03, 0.99),
           (-0.11, -0.02, 0.98), (-0.06, 0.0, 1.06)]
    for a, c in zip(pts, pts[1:]):
        b.between(BONE, m["dark"], a, c, 0.009, 0.009, seg=5)


if __name__ == "__main__":
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Payphone")
    build(body, materials())
    obj = body.object()
    obj.vertex_groups.clear()
    rrkit.export(out, [obj])
    if prev:
        rrkit.preview(prev, "payphone", None, HEIGHT, [])
