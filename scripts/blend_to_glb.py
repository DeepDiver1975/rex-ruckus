# Converts one collection of a .blend file to a binary glTF, for CC0 models that are only
# published as .blend. Old-style Diffuse BSDF materials are rebuilt as Principled BSDF in the
# same colour (roughness 0.7), since the glTF exporter ignores Diffuse nodes.
#
# Usage: blender -b SRC.blend --python scripts/blend_to_glb.py -- COLLECTION OUT.glb
# Called by scripts/import-models.sh.
import sys

import bpy

collection, out = sys.argv[sys.argv.index("--") + 1 :]

for mat in bpy.data.materials:
    tree = mat.node_tree
    if tree is None:
        continue
    diffuse = next((n for n in tree.nodes if n.type == "BSDF_DIFFUSE"), None)
    output = next((n for n in tree.nodes if n.type == "OUTPUT_MATERIAL"), None)
    if diffuse is None or output is None:
        continue
    principled = tree.nodes.new("ShaderNodeBsdfPrincipled")
    principled.inputs["Base Color"].default_value = tuple(diffuse.inputs["Color"].default_value)
    principled.inputs["Roughness"].default_value = 0.7
    tree.links.new(principled.outputs["BSDF"], output.inputs["Surface"])
    tree.nodes.remove(diffuse)

bpy.ops.object.select_all(action="DESELECT")
meshes = [o for o in bpy.data.collections[collection].objects if o.type == "MESH"]
for o in meshes:
    o.select_set(True)
bpy.ops.export_scene.gltf(
    filepath=out,
    use_selection=True,
    export_format="GLB",
    export_apply=True,
    export_yup=True,
)
