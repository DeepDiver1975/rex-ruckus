# The Goon's oversized hand-cannon: a chrome revolver with a brass drum and a ported barrel.
# Static mesh, authored like any prop: the grip is centred on the origin, the barrel points
# forwards (-Y, glTF +Z) and the sights up. models.ron turns it into the frame of `Hand.R`.
#
# Usage: blender -b --factory-startup --python scripts/models/hand_cannon.py -- OUT.glb [--preview DIR]
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rrkit  # noqa: E402

# The muzzle, relative to the grip (origin).
MUZZLE = (0.0, -0.52, 0.11)


def materials():
    return {
        "steel": rrkit.material("CannonSteel", (150, 156, 168), rough=0.35, metal=0.8),
        "dark": rrkit.material("CannonDark", (45, 46, 52), rough=0.5, metal=0.6),
        "brass": rrkit.material("CannonBrass", (220, 168, 60), rough=0.35, metal=0.9),
        "grip": rrkit.material("CannonGrip", (110, 50, 30)),
    }


def build(body, bone, m):
    """Adds the cannon's parts to `body`, all on `bone`, in the frame set by `body.xf`."""
    st, dk, br, gr = m["steel"], m["dark"], m["brass"], m["grip"]
    body.box(bone, gr, (0, 0.01, 0), (0.06, 0.08, 0.15), rot=(-15, 0, 0), bevel=0.01)
    body.box(bone, st, (0, -0.06, 0.10), (0.07, 0.20, 0.08), bevel=0.012)  # frame
    body.cyl(bone, br, (0, -0.07, 0.11), 0.075, 0.075, 0.12, rot=(90, 0, 0), bevel=0.01)
    body.cyl(bone, st, (0, -0.30, 0.11), 0.045, 0.04, 0.36, rot=(90, 0, 0))  # barrel
    body.box(bone, dk, (0, -0.30, 0.158), (0.025, 0.36, 0.02))  # top rib
    body.box(bone, dk, (0, -0.47, 0.11), (0.11, 0.08, 0.10), bevel=0.012)  # ported muzzle
    body.box(bone, br, (0, -0.47, 0.172), (0.015, 0.03, 0.03))  # front sight
    body.box(bone, dk, (0, 0.05, 0.16), (0.03, 0.05, 0.05), rot=(30, 0, 0))  # hammer
    body.box(bone, dk, (0, -0.035, 0.03), (0.02, 0.08, 0.02))  # trigger guard


if __name__ == "__main__":
    out, prev = rrkit.args()
    rrkit.reset()
    body = rrkit.Body("HandCannon")
    build(body, "Cannon", materials())
    obj = body.object()
    obj.vertex_groups.clear()
    rrkit.export(out, [obj])
    if prev:
        obj.location.z = 0.4
        rrkit.preview(prev, "hand_cannon", None, 1.0, [])
