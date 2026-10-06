# Shared kit for the scripted, original Rex Ruckus models: chunky low-poly parts rigidly skinned
# to a simple armature (each part follows exactly one bone, PS1/Quake style), keyframed clips,
# glTF export and preview contact sheets.
#
# Conventions: Blender units are metres, Z up, characters stand on Z=0 and face -Y (glTF +Z,
# so models.ron keeps its `yaw_deg: 180`). Angles in poses are degrees.
#
# Used by the per-model scripts next to this file; see scripts/build-models.sh.
import math
import os
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Vector

FPS = 30


def args():
    """Returns (out.glb, preview dir or None) from the arguments after `--`."""
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    out, preview = None, None
    i = 0
    while i < len(argv):
        if argv[i] == "--preview":
            preview = argv[i + 1]
            i += 2
        else:
            out = argv[i]
            i += 1
    if out is None:
        sys.exit("usage: blender -b --factory-startup --python MODEL.py -- OUT.glb [--preview DIR]")
    return os.path.abspath(out), preview and os.path.abspath(preview)


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = FPS


# --- materials -------------------------------------------------------------------------------


def srgb(c):
    """sRGB 0-255 or 0-1 triple to linear RGBA, so colours can be picked by eye."""
    c = [x / 255.0 if max(c) > 1.0 else x for x in c]
    return tuple(x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c) + (1.0,)


def material(name, colour, rough=0.8, metal=0.0, emit=0.0):
    """A flat Principled material; `emit` > 0 makes it glow in its own colour."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    p = m.node_tree.nodes["Principled BSDF"]
    rgba = srgb(colour)
    p.inputs["Base Color"].default_value = rgba
    p.inputs["Roughness"].default_value = rough
    p.inputs["Metallic"].default_value = metal
    if emit > 0.0:
        p.inputs["Emission Color"].default_value = rgba
        p.inputs["Emission Strength"].default_value = emit
    m.diffuse_color = rgba  # what the Workbench preview shows
    return m


# --- mesh building ---------------------------------------------------------------------------


class Body:
    """Accumulates parts into one mesh; every vertex is weighted 1.0 to a single bone."""

    def __init__(self, name):
        self.name = name
        self.bm = bmesh.new()
        self.deform = self.bm.verts.layers.deform.verify()
        self.bones = []
        self.mats = []
        # Applied after each part's own placement, e.g. to build a prop in its holder's hand.
        self.xf = Matrix.Identity(4)

    def _index(self, items, x):
        if x not in items:
            items.append(x)
        return items.index(x)

    def _finish(self, geom, bone, mat, bevel):
        verts = [g for g in geom if isinstance(g, bmesh.types.BMVert)]
        if bevel > 0.0:
            edges = list({e for v in verts for e in v.link_edges})
            res = bmesh.ops.bevel(
                self.bm, geom=edges, offset=bevel, segments=1, affect="EDGES", profile=0.5
            )
            faces = {f for v in verts if v.is_valid for f in v.link_faces} | set(res["faces"])
            verts = list({v for f in faces for v in f.verts})
        b = self._index(self.bones, bone)
        mi = self._index(self.mats, mat)
        faces = {f for v in verts for f in v.link_faces}
        for f in faces:
            f.material_index = mi
            f.smooth = False
        for v in verts:
            v[self.deform].clear()
            v[self.deform][b] = 1.0
        return verts

    def box(self, bone, mat, loc, size, rot=(0, 0, 0), top=(1.0, 1.0), front=1.0, bevel=0.0,
            shift=(0.0, 0.0)):
        """A box of `size` (x, y, z) centred at `loc`. `top` scales the +Z face (x, y) for
        tapers, `front` scales the -Y (forward) face's width, `shift` slides the top face."""
        res = bmesh.ops.create_cube(self.bm, size=1.0)
        verts = res["verts"]
        for v in verts:
            x, y, z = v.co
            if z > 0:
                x, y = x * top[0] + shift[0], y * top[1] + shift[1]
            if y < 0:
                x *= front
            v.co = Vector((x * size[0], y * size[1], z * size[2]))
        self._place(verts, loc, rot)
        return self._finish(verts, bone, mat, bevel)

    def cyl(self, bone, mat, loc, r1, r2, depth, rot=(0, 0, 0), seg=8, bevel=0.0):
        """A (tapered) cylinder along local Z, r1 at the bottom, r2 at the top."""
        res = bmesh.ops.create_cone(
            self.bm, cap_ends=True, cap_tris=False, segments=seg, radius1=r1, radius2=r2,
            depth=depth,
        )
        self._place(res["verts"], loc, rot)
        return self._finish(res["verts"], bone, mat, bevel)

    def between(self, bone, mat, a, b, r1, r2, seg=6, bevel=0.0):
        """A (tapered) cylinder from point a (radius r1) to point b (radius r2)."""
        a, b = Vector(a), Vector(b)
        rot = [math.degrees(x) for x in (b - a).to_track_quat("Z", "Y").to_euler()]
        return self.cyl(bone, mat, (a + b) / 2, r1, r2, (b - a).length, rot=rot, seg=seg,
                        bevel=bevel)

    def spike_to(self, bone, mat, a, b, r, seg=4):
        """A cone from point a (radius r) to a point at b."""
        a, b = Vector(a), Vector(b)
        rot = [math.degrees(x) for x in (b - a).to_track_quat("Z", "Y").to_euler()]
        return self.spike(bone, mat, a, r, (b - a).length, rot=rot, seg=seg)

    def ball(self, bone, mat, loc, r, scale=(1, 1, 1), rot=(0, 0, 0), sub=1):
        res = bmesh.ops.create_icosphere(self.bm, subdivisions=sub, radius=r)
        for v in res["verts"]:
            v.co = Vector((v.co.x * scale[0], v.co.y * scale[1], v.co.z * scale[2]))
        self._place(res["verts"], loc, rot)
        return self._finish(res["verts"], bone, mat, 0.0)

    def spike(self, bone, mat, loc, r, length, rot=(0, 0, 0), seg=4):
        """A cone pointing along local +Z from `loc`: horns, teeth, mohawk blades, claws."""
        res = bmesh.ops.create_cone(
            self.bm, cap_ends=True, cap_tris=True, segments=seg, radius1=r, radius2=0.0,
            depth=length,
        )
        for v in res["verts"]:
            v.co.z += length / 2
        self._place(res["verts"], loc, rot)
        return self._finish(res["verts"], bone, mat, 0.0)

    def _place(self, verts, loc, rot):
        m = Matrix.Translation(loc) @ Euler([math.radians(a) for a in rot]).to_matrix().to_4x4()
        m = self.xf @ m
        for v in verts:
            v.co = m @ v.co

    def _canonical(self):
        """Rebuilds the mesh in a canonical vertex and face order. Bevel orders its output by
        memory address, so without this the same script would export different files."""
        dl = self.deform
        key = {v: (tuple(round(c, 5) for c in v.co), tuple(sorted(v[dl].items()))) for v in
               self.bm.verts}
        verts = sorted(self.bm.verts, key=key.get)
        index = {v: i for i, v in enumerate(verts)}
        faces = []
        for f in self.bm.faces:
            idx = [index[v] for v in f.verts]
            r = idx.index(min(idx))
            faces.append((tuple(idx[r:] + idx[:r]), f.material_index))
        faces.sort()
        bm = bmesh.new()
        ndl = bm.verts.layers.deform.verify()
        new = []
        for v in verts:
            nv = bm.verts.new(v.co)
            for g, w in v[dl].items():
                nv[ndl][g] = w
            new.append(nv)
        for idx, mi in faces:
            f = bm.faces.new([new[i] for i in idx])
            f.material_index = mi
            f.smooth = False
        self.bm.free()
        self.bm, self.deform = bm, ndl

    def object(self):
        bmesh.ops.remove_doubles(self.bm, verts=self.bm.verts, dist=1e-5)
        self._canonical()
        bmesh.ops.recalc_face_normals(self.bm, faces=self.bm.faces)
        mesh = bpy.data.meshes.new(self.name)
        self.bm.to_mesh(mesh)
        self.bm.free()
        for m in self.mats:
            mesh.materials.append(m)
        obj = bpy.data.objects.new(self.name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        for b in self.bones:
            obj.vertex_groups.new(name=b)
        return obj


def mirror(name):
    """`Arm.L` <-> `Arm.R`; other names unchanged."""
    if name.endswith(".L"):
        return name[:-2] + ".R"
    if name.endswith(".R"):
        return name[:-2] + ".L"
    return name


def both(fn):
    """Calls fn(s, suffix) for both sides. Facing -Y, the character's left is world +X:
    s = +1 with ".L", then s = -1 with ".R"."""
    fn(1.0, ".L")
    fn(-1.0, ".R")


# --- armature --------------------------------------------------------------------------------


def armature(name, bones):
    """bones: [(name, head, tail, parent or None, deform)]; returns the armature object.

    All rolls are 0. Bones pointing straight up or down get X = world X; a bone pointing
    forwards should tilt down a little (see peeper.py), since an exactly level one gets
    X = -world X and every pitch in its poses flips."""
    data = bpy.data.armatures.new(name)
    arm = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    for bname, head, tail, parent, deform in bones:
        eb = data.edit_bones.new(bname)
        eb.head, eb.tail, eb.roll = Vector(head), Vector(tail), 0.0
        eb.use_deform = deform
        if parent:
            eb.parent = data.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    for pb in arm.pose.bones:
        pb.rotation_mode = "XYZ"
    return arm


def biped(hip_x, hip_z, knee_z, ankle_z, waist_z, chest_z, neck_z, head_z, head_top,
          shoulder_x, shoulder_z, elbow_z, wrist_z, hand_z, toe=(-0.16, 0.04)):
    """The standard humanoid bones: limbs hang straight down and the spine points straight up,
    so a pose means the same on every character (see the per-model notes on rotation axes)."""
    b = [
        ("Hips", (0, 0, hip_z), (0, 0, waist_z), None, True),
        ("Spine", (0, 0, waist_z), (0, 0, chest_z), "Hips", True),
        ("Chest", (0, 0, chest_z), (0, 0, neck_z), "Spine", True),
        ("Neck", (0, 0, neck_z), (0, 0, head_z), "Chest", True),
        ("Head", (0, 0, head_z), (0, 0, head_top), "Neck", True),
    ]
    for s, sfx in ((1, ".L"), (-1, ".R")):
        x, lx = s * shoulder_x, s * hip_x
        b += [
            ("UpperArm" + sfx, (x, 0, shoulder_z), (x, 0, elbow_z), "Chest", True),
            ("LowerArm" + sfx, (x, 0, elbow_z), (x, 0, wrist_z), "UpperArm" + sfx, True),
            ("Hand" + sfx, (x, 0, wrist_z), (x, 0, hand_z), "LowerArm" + sfx, True),
            ("UpperLeg" + sfx, (lx, 0, hip_z), (lx, 0, knee_z), "Hips", True),
            ("LowerLeg" + sfx, (lx, 0, knee_z), (lx, 0, ankle_z), "UpperLeg" + sfx, True),
            ("Foot" + sfx, (lx, 0, ankle_z), (lx, toe[0], toe[1]), "LowerLeg" + sfx, True),
        ]
    return b


def skin(body_obj, arm):
    body_obj.parent = arm
    mod = body_obj.modifiers.new("Armature", "ARMATURE")
    mod.object = arm


# --- animation -------------------------------------------------------------------------------


def clip(arm, name, keys, loop=True, linear=False, closed=False):
    """keys: [(frame, {bone: {"r": (x, y, z) degrees, "l": (x, y, z) metres}})]. Every bone
    is keyed at every key frame (unlisted bones at rest), so clips never leak into each other.
    A looping clip gets its first key repeated at the end automatically if it isn't already.
    `linear` keys without easing, for steady spins (rotors); `closed` says the keys already end
    on a pose equivalent to the first (e.g. a rotor a whole turn on), so none is added."""
    act = bpy.data.actions.new(name)
    act.use_fake_user = True
    arm.animation_data_create()
    arm.animation_data.action = act
    if loop and not closed and keys[-1][1] is not keys[0][1]:
        period = keys[-1][0] + (keys[1][0] - keys[0][0] if len(keys) > 1 else 1)
        keys = keys + [(period, keys[0][1])]
    for frame, pose in keys:
        for pb in arm.pose.bones:
            k = pose.get(pb.name, {})
            pb.rotation_euler = [math.radians(a) for a in k.get("r", (0, 0, 0))]
            pb.location = k.get("l", (0, 0, 0))
            pb.keyframe_insert("rotation_euler", frame=frame)
            pb.keyframe_insert("location", frame=frame)
    if linear:
        for layer in act.layers:
            for strip in layer.strips:
                for bag in strip.channelbags:
                    for fc in bag.fcurves:
                        for kp in fc.keyframe_points:
                            kp.interpolation = "LINEAR"
    act.frame_range = (keys[0][0], keys[-1][0])
    act.use_frame_range = True
    act.use_cyclic = loop
    return act


def lerp_pose(a, b, t):
    """Blends two pose dicts (for in-betweens built in code)."""
    out = {}
    for bone in set(a) | set(b):
        ka, kb = a.get(bone, {}), b.get(bone, {})
        out[bone] = {
            ch: tuple(x + (y - x) * t for x, y in zip(ka.get(ch, (0, 0, 0)), kb.get(ch, (0, 0, 0))))
            for ch in ("r", "l")
        }
    return out


# --- export and preview ----------------------------------------------------------------------


def export(out, objects, rest_action=None):
    os.makedirs(os.path.dirname(out), exist_ok=True)
    for o in bpy.context.scene.objects:
        o.select_set(o in objects)
    for o in objects:
        if o.type == "ARMATURE" and o.animation_data:
            o.animation_data.action = rest_action
    bpy.ops.export_scene.gltf(
        filepath=out,
        use_selection=True,
        export_format="GLB",
        export_apply=True,
        export_yup=True,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_optimize_animation_size=True,
        export_anim_slide_to_zero=True,
        export_materials="EXPORT",
        export_texcoords=False,
        export_normals=True,
    )
    print(f"rrkit: wrote {out}")


def _setup_preview(height):
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    shading = scene.display.shading
    shading.light = "STUDIO"
    shading.color_type = "MATERIAL"
    shading.show_object_outline = True
    shading.show_cavity = True
    scene.render.resolution_x = 360
    scene.render.resolution_y = 480
    scene.render.film_transparent = False
    world = bpy.data.worlds.new("Preview")
    world.color = (0.16, 0.18, 0.24)
    scene.world = world
    cam_data = bpy.data.cameras.new("PreviewCam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = height * 1.25
    cam = bpy.data.objects.new("PreviewCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    # A floor so the feet read against something.
    bpy.ops.mesh.primitive_plane_add(size=4.0)
    floor = bpy.context.active_object
    floor.name = "PreviewFloor"
    floor.data.materials.append(material("PreviewFloor", (70, 74, 82)))
    return cam


def _aim(cam, yaw_deg, height):
    """Camera on a ring around the origin, `yaw_deg` 0 = in front of the character (-Y)."""
    a = math.radians(yaw_deg)
    d = 6.0
    target = Vector((0, 0, height * 0.5))
    cam.location = target + Vector((math.sin(a) * d, -math.cos(a) * d, d * 0.18))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()


def _sheet(paths, cols, out):
    import numpy as np

    imgs = []
    for p in paths:
        img = bpy.data.images.load(p)
        w, h = img.size
        px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
        imgs.append(px)
        bpy.data.images.remove(img)
    h, w = imgs[0].shape[:2]
    rows = (len(imgs) + cols - 1) // cols
    sheet = np.zeros((rows * h, cols * w, 4), dtype=np.float32)
    sheet[..., 3] = 1.0
    for i, px in enumerate(imgs):
        r, c = divmod(i, cols)
        # Blender pixel rows run bottom-up.
        sheet[(rows - 1 - r) * h : (rows - r) * h, c * w : (c + 1) * w] = px
    img = bpy.data.images.new("sheet", cols * w, rows * h, alpha=True)
    img.pixels = sheet.ravel()
    img.filepath_raw = out
    img.file_format = "PNG"
    img.save()
    for p in paths:
        os.remove(p)
    print(f"rrkit: wrote {out}")


def preview(dir, name, arm, height, actions, frames_per_clip=6, views=(0, 90, 35, 180)):
    """Writes `<name>_turn.png` (rest pose from several angles) and `<name>_<clip>.png`
    (frames of each action from the 3/4 view) into `dir`."""
    os.makedirs(dir, exist_ok=True)
    cam = _setup_preview(height)
    scene = bpy.context.scene
    tmp = []

    def shot(path):
        scene.render.filepath = path
        bpy.ops.render.render(write_still=True)
        tmp.append(path)

    if arm is not None:
        arm.animation_data.action = None
        for pb in arm.pose.bones:
            pb.rotation_euler = (0, 0, 0)
            pb.location = (0, 0, 0)
    for i, yaw in enumerate(views):
        _aim(cam, yaw, height)
        shot(os.path.join(dir, f"_t{i}.png"))
    _sheet(tmp, len(views), os.path.join(dir, f"{name}_turn.png"))
    for act in actions:
        tmp = []
        arm.animation_data.action = act
        start, end = int(act.frame_range[0]), int(act.frame_range[1])
        _aim(cam, 35, height)
        n = frames_per_clip
        for i in range(n):
            f = start + round((end - start) * i / (n - 1 if not act.use_cyclic else n))
            scene.frame_set(f)
            shot(os.path.join(dir, f"_c{i}.png"))
        _sheet(tmp, n, os.path.join(dir, f"{name}_{act.name}.png"))
