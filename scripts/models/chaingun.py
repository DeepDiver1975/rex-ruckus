# Rex's chaingun: a squat gunmetal receiver with a rear pistol grip, an arched carry handle, a
# hazard-striped ammo box slung on the left with a feed chute, a ribbed drive motor up front and
# a six-barrel cluster with two hex clamp rings and one red index barrel, so the spin reads.
#
# Static meshes, authored like hand_cannon.py: the grip is at the origin, the barrels point
# forwards (-Y, glTF +Z) and the carry handle up (+Z, glTF +Y), real size (about 0.65 m long).
# The barrel cluster is its own object, `Barrels`, a child of the body with its origin on the
# firing axis and its mesh around its local Y axis, so spinning the node about its local glTF
# +Z (Blender -Y) turns the cluster in place. An empty, `Muzzle`, marks the barrel tips.
#
# Usage: blender -b --factory-startup --python scripts/models/chaingun.py -- OUT.glb [--preview DIR]
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy  # noqa: E402
import rrkit  # noqa: E402
from mathutils import Vector  # noqa: E402

# The firing axis: height above the grip and where the barrel cluster starts and ends (Y).
AXIS_Z = 0.10
CLUSTER_Y = -0.20
TIP_Y = -0.53
# Barrels: how many, their radius, and the radius of the ring they sit on.
BARRELS = 6
BARREL_R = 0.0135
RING_R = 0.034


def materials():
    return {
        "gun": rrkit.material("GunMetal", (62, 66, 74), rough=0.55, metal=0.7),
        "dark": rrkit.material("GunDark", (32, 33, 38), rough=0.6, metal=0.5),
        "steel": rrkit.material("GunSteel", (160, 166, 176), rough=0.3, metal=0.85),
        "brass": rrkit.material("GunBrass", (214, 160, 56), rough=0.35, metal=0.9),
        "hazard": rrkit.material("GunHazard", (236, 150, 24)),
        "grip": rrkit.material("GunGrip", (88, 52, 34)),
        "red": rrkit.material("GunRed", (190, 36, 30), rough=0.5),
    }


def build_body(b, m):
    """The receiver and everything that does not spin, on bone `Gun`."""
    g, dk, st, br, hz, gr = m["gun"], m["dark"], m["steel"], m["brass"], m["hazard"], m["grip"]
    z = AXIS_Z
    # Receiver: a slab with a slightly narrower top, and a heavy rear cap.
    b.box("Gun", g, (0, -0.03, z), (0.11, 0.26, 0.13), top=(0.85, 1.0), bevel=0.012)
    b.box("Gun", dk, (0, 0.115, z + 0.005), (0.10, 0.04, 0.11), bevel=0.012)
    # Side plates with bolts, both sides.
    for s in (1, -1):
        b.box("Gun", dk, (s * 0.056, -0.05, z - 0.01), (0.008, 0.16, 0.07))
        for y in (0.0, -0.10):
            b.cyl("Gun", st, (s * 0.061, y, z - 0.01), 0.008, 0.008, 0.006, rot=(0, 90, 0), seg=6)
    # Pistol grip, raked back, and the trigger guard.
    b.box("Gun", gr, (0, 0.04, -0.03), (0.045, 0.06, 0.14), rot=(-18, 0, 0), bevel=0.008)
    b.box("Gun", dk, (0, -0.02, 0.0), (0.02, 0.09, 0.015))
    b.box("Gun", dk, (0, -0.06, 0.02), (0.02, 0.015, 0.05))
    b.box("Gun", st, (0, -0.012, 0.022), (0.01, 0.012, 0.03), rot=(15, 0, 0))  # trigger
    # Carry handle: two posts and an arched bar with a rear sight.
    for y in (0.06, -0.11):
        b.box("Gun", dk, (0, y, z + 0.09), (0.03, 0.025, 0.06))
    b.box("Gun", g, (0, -0.025, z + 0.13), (0.035, 0.21, 0.025), bevel=0.006)
    b.box("Gun", br, (0, 0.05, z + 0.153), (0.02, 0.02, 0.02))
    # Drive motor in front of the receiver: a ribbed drum, with a hazard band.
    b.cyl("Gun", g, (0, -0.175, z), 0.066, 0.066, 0.05, rot=(90, 0, 0), seg=10, bevel=0.006)
    b.cyl("Gun", hz, (0, -0.155, z), 0.069, 0.069, 0.012, rot=(90, 0, 0), seg=10)
    b.cyl("Gun", dk, (0, -0.205, z), 0.05, 0.05, 0.012, rot=(90, 0, 0), seg=10)
    for a in range(0, 360, 45):
        r = math.radians(a)
        b.box("Gun", dk, (0.067 * math.cos(r), -0.18, z + 0.067 * math.sin(r)),
              (0.012, 0.035, 0.012), rot=(0, -a, 0))
    # Ammo box slung on the left (+X), hazard stripes, and a feed chute into the receiver.
    b.box("Gun", g, (0.10, -0.03, z - 0.07), (0.07, 0.15, 0.11), bevel=0.01)
    for y in (-0.08, -0.03, 0.02):
        b.box("Gun", hz, (0.136, y, z - 0.07), (0.004, 0.02, 0.105), rot=(30, 0, 0))
    b.box("Gun", dk, (0.10, -0.03, z - 0.012), (0.075, 0.155, 0.01))  # lid
    b.box("Gun", br, (0.073, -0.06, z + 0.01), (0.03, 0.05, 0.03), rot=(0, 0, 0))  # chute
    for y in (-0.075, -0.06, -0.045):
        b.cyl("Gun", br, (0.077, y, z + 0.032), 0.007, 0.004, 0.02, seg=6)  # rounds


def build_barrels(b, m):
    """The spinning cluster, in its own frame: origin on the firing axis, barrels along -Y."""
    g, dk, st, red = m["gun"], m["dark"], m["steel"], m["red"]
    length = CLUSTER_Y - TIP_Y
    b.between("Barrels", st, (0, 0.0, 0), (0, -length + 0.01, 0), 0.014, 0.014, seg=6)  # spindle
    for i in range(BARRELS):
        a = 2 * math.pi * i / BARRELS
        x, zz = RING_R * math.cos(a), RING_R * math.sin(a)
        mat = red if i == 0 else st
        b.between("Barrels", mat, (x, 0.0, zz), (x, -length, zz), BARREL_R, BARREL_R, seg=8)
        b.between("Barrels", dk, (x, -length - 0.003, zz), (x, -length + 0.012, zz),
                  BARREL_R * 0.6, BARREL_R * 0.6, seg=6)  # bore
    # Hex clamp rings (the flats show the spin) and a rear hub.
    for y, w in ((-0.02, 0.025), (-0.15, 0.022), (-length + 0.035, 0.03)):
        b.cyl("Barrels", g, (0, y, 0), 0.056, 0.056, w, rot=(90, 0, 0), seg=6)
    b.box("Barrels", red, (0, -0.15, 0.052), (0.02, 0.026, 0.012))  # index lug on a clamp


def objects(m):
    body = rrkit.Body("Chaingun")
    build_body(body, m)
    gun = body.object()
    gun.vertex_groups.clear()
    cl = rrkit.Body("Barrels")
    build_barrels(cl, m)
    barrels = cl.object()
    barrels.vertex_groups.clear()
    barrels.parent = gun
    barrels.location = (0.0, CLUSTER_Y, AXIS_Z)
    muzzle = bpy.data.objects.new("Muzzle", None)
    bpy.context.scene.collection.objects.link(muzzle)
    muzzle.parent = gun
    muzzle.location = (0.0, TIP_Y, AXIS_Z)
    muzzle.empty_display_size = 0.03
    return gun, barrels, muzzle


def extra_previews(dir, gun, barrels):
    """A first-person close-up and the cluster at 0 and 30 degrees, seen down the axis."""
    scene = bpy.context.scene
    cam_data = bpy.data.cameras.new("FpCam")
    cam_data.lens = 35
    scene.render.resolution_x, scene.render.resolution_y = 640, 400
    cam = bpy.data.objects.new("FpCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    base = gun.matrix_world.translation
    cam.location = base + Vector((0.22, 0.62, 0.52))
    target = base + Vector((0.0, -0.45, AXIS_Z))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = os.path.join(dir, "chaingun_fp.png")
    bpy.ops.render.render(write_still=True)
    print(f"rrkit: wrote {scene.render.filepath}")

    cam_data.type = "ORTHO"
    cam_data.ortho_scale = 0.22
    axis = base + Vector((0.0, 0.0, AXIS_Z))
    cam.location = axis + Vector((0.0, -1.5, 0.0))
    cam.rotation_euler = (axis - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.resolution_x = scene.render.resolution_y = 360
    tmp = []
    for i, deg in enumerate((0, 30)):
        barrels.rotation_euler = (0.0, math.radians(deg), 0.0)
        bpy.context.view_layer.update()
        print(f"rrkit: Barrels at {deg} deg: origin {tuple(round(c, 4) for c in barrels.matrix_world.translation)}, "
              f"bbox centre {tuple(round(c, 4) for c in sum((barrels.matrix_world @ Vector(v) for v in barrels.bound_box), Vector()) / 8)}")
        p = os.path.join(dir, f"_s{i}.png")
        scene.render.filepath = p
        bpy.ops.render.render(write_still=True)
        tmp.append(p)
    rrkit._sheet(tmp, 2, os.path.join(dir, "chaingun_spin.png"))
    barrels.rotation_euler = (0.0, 0.0, 0.0)


if __name__ == "__main__":
    out, prev = rrkit.args()
    rrkit.reset()
    gun, barrels, muzzle = objects(materials())
    rrkit.export(out, [gun, barrels, muzzle])
    if prev:
        gun.location.z = 0.4
        bpy.context.view_layer.update()
        rrkit.preview(prev, "chaingun", None, 0.8, [], views=(0, 90, 35, 145, 180, 270))
        extra_previews(prev, gun, barrels)
