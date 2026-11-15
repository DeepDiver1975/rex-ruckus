# The jukebox gag prop: a wooden cabinet with a round-topped hood, a ring of glowing tubes
# around the hood, lit side pillars, a record window and a chrome speaker grille. Static mesh at
# real size: it stands on the origin, its front faces -Y (glTF +Z), and it fills the core
# footprint of `PropKind::Jukebox` (0.9 m wide, 0.6 m deep, 1.5 m tall).
#
# Usage: blender -b --factory-startup --python scripts/models/jukebox.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402

HEIGHT = 1.5
BONE = "Jukebox"
# The hood: a half cylinder (front to back) on top of the cabinet.
ARCH_Z = 1.07
ARCH_R = 0.43


def materials():
    return {
        "wood": rrkit.material("JukeWood", (120, 55, 25)),
        "dark": rrkit.material("JukeDark", (35, 22, 15)),
        "chrome": rrkit.material("JukeChrome", (200, 200, 210), rough=0.25, metal=0.9),
        "tube": rrkit.material("JukeTube", (255, 140, 30), emit=4.0),
        "pillar": rrkit.material("JukePillar", (255, 60, 90), emit=3.0),
        "window": rrkit.material("JukeWindow", (255, 230, 150), emit=2.5),
        "grille": rrkit.material("JukeGrille", (20, 20, 24), rough=0.9),
    }


def build(b, m):
    b.box(BONE, m["dark"], (0, 0, 0.12), (0.90, 0.60, 0.24))  # plinth
    b.box(BONE, m["wood"], (0, 0.01, 0.65), (0.86, 0.56, 0.84))  # cabinet
    b.cyl(BONE, m["wood"], (0, 0.01, ARCH_Z), ARCH_R, ARCH_R, 0.56, rot=(90, 0, 0), seg=16)
    # A ring of glowing tube segments around the front of the hood.
    n = 9
    pts = [
        (math.cos(math.pi * i / (n - 1)) * 0.40, -0.285, ARCH_Z + math.sin(math.pi * i / (n - 1)) * 0.40)
        for i in range(n)
    ]
    for a, c in zip(pts, pts[1:]):
        b.between(BONE, m["tube"], a, c, 0.025, 0.025, seg=6)
    # The window onto the records under the hood, framed in chrome.
    b.box(BONE, m["chrome"], (0, -0.27, 1.02), (0.58, 0.02, 0.34))
    b.box(BONE, m["window"], (0, -0.285, 1.02), (0.50, 0.02, 0.27))
    # Lit pillars down both front corners.
    for x in (-0.39, 0.39):
        b.box(BONE, m["pillar"], (x, -0.27, 0.66), (0.07, 0.04, 0.80))
    # Speaker grille with chrome bars, and a chrome kick rail.
    b.box(BONE, m["grille"], (0, -0.27, 0.55), (0.60, 0.02, 0.40))
    for i in range(7):
        x = -0.27 + i * 0.09
        b.box(BONE, m["chrome"], (x, -0.285, 0.55), (0.02, 0.02, 0.40))
    b.box(BONE, m["chrome"], (0, -0.295, 0.28), (0.80, 0.02, 0.04))


if __name__ == "__main__":
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("Jukebox")
    build(body, materials())
    obj = body.object()
    obj.vertex_groups.clear()
    rrkit.export(out, [obj])
    if prev:
        rrkit.preview(prev, "jukebox", None, HEIGHT, [])
