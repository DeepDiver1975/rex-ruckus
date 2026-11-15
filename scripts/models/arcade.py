# The arcade cabinet gag prop: an upright cabinet with stepped side panels, a lit marquee, a
# tilted glowing screen, a control panel with a stick and buttons, and a coin door. Static mesh
# at real size: it stands on the origin, its front faces -Y (glTF +Z), and it fills the core
# footprint of `PropKind::Arcade` (0.7 m wide, 0.8 m deep, 1.8 m tall).
#
# Usage: blender -b --factory-startup --python scripts/models/arcade.py -- OUT.glb [--preview DIR]
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402

HEIGHT = 1.8
BONE = "Cabinet"


def materials():
    return {
        "body": rrkit.material("ArcadeBody", (40, 30, 90)),
        "side": rrkit.material("ArcadeSide", (230, 60, 120)),
        "trim": rrkit.material("ArcadeTrim", (25, 25, 30), rough=0.5, metal=0.4),
        "screen": rrkit.material("ArcadeScreen", (70, 240, 170), emit=3.0),
        "marquee": rrkit.material("ArcadeMarquee", (255, 200, 60), emit=4.0),
        "red": rrkit.material("ArcadeRed", (230, 40, 40), rough=0.4),
        "yellow": rrkit.material("ArcadeYellow", (250, 220, 40), rough=0.4),
        "coin": rrkit.material("ArcadeCoin", (255, 120, 30), emit=2.5),
        "chrome": rrkit.material("ArcadeChrome", (190, 195, 205), rough=0.3, metal=0.9),
    }


def build(b, m):
    # Lower body (the front at y = -0.30) and the shallower upper body.
    b.box(BONE, m["body"], (0, 0.05, 0.45), (0.62, 0.70, 0.90))
    b.box(BONE, m["body"], (0, 0.15, 1.35), (0.62, 0.50, 0.90))
    # Stepped side panels, proud of the body: they set the full 0.7 x 0.8 m footprint.
    for x in (-0.335, 0.335):
        b.box(BONE, m["side"], (x, 0.0, 0.50), (0.03, 0.80, 1.00))
        b.box(BONE, m["side"], (x, 0.08, 1.40), (0.03, 0.64, 0.80), shift=(0.0, 0.03))
    # Top cap and the lit marquee under its front edge.
    b.box(BONE, m["trim"], (0, 0.08, 1.78), (0.70, 0.64, 0.04))
    b.box(BONE, m["marquee"], (0, -0.12, 1.65), (0.60, 0.06, 0.20))
    # Screen bezel tilted back, the glowing screen just in front of it.
    b.box(BONE, m["trim"], (0, -0.11, 1.28), (0.58, 0.06, 0.48), rot=(-12, 0, 0))
    b.box(BONE, m["screen"], (0, -0.145, 1.29), (0.48, 0.02, 0.38), rot=(-12, 0, 0))
    # Control panel jutting out at waist height, with a stick and four buttons.
    b.box(BONE, m["trim"], (0, -0.30, 0.98), (0.64, 0.24, 0.08), rot=(14, 0, 0), bevel=0.01)
    b.cyl(BONE, m["chrome"], (-0.14, -0.31, 1.06), 0.012, 0.012, 0.10)
    b.ball(BONE, m["red"], (-0.14, -0.31, 1.12), 0.035)
    for i, x in enumerate((0.02, 0.09, 0.16, 0.23)):
        col = m["yellow"] if i % 2 else m["red"]
        b.cyl(BONE, col, (x, -0.31 + 0.02 * (i % 2), 1.03), 0.022, 0.022, 0.03, seg=8)
    # Coin door with two lit slots, and a dark kick plate.
    b.box(BONE, m["trim"], (0, -0.31, 0.50), (0.24, 0.02, 0.28))
    for x in (-0.05, 0.05):
        b.box(BONE, m["coin"], (x, -0.325, 0.56), (0.03, 0.01, 0.06))
    b.box(BONE, m["trim"], (0, -0.31, 0.06), (0.62, 0.02, 0.12))


if __name__ == "__main__":
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Arcade")
    build(body, materials())
    obj = body.object()
    obj.vertex_groups.clear()
    rrkit.export(out, [obj])
    if prev:
        rrkit.preview(prev, "arcade", None, HEIGHT, [])
