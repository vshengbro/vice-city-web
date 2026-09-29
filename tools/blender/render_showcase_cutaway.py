"""Render the two enterable showcase buildings with the front shell removed.

The exported shell is an opaque box, so an exterior preview can never show
the doorway interior, the partition or the stairs. This script drops the
front facade and the roof, then shoots three views per building: a cutaway
three-quarter, a straight-on look at the stair, and a top-down plan.
"""

import json
import math
import os
import sys

import bpy  # type: ignore
import mathutils  # type: ignore

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "assets", "preview", "showcase")
ASSETS = ["bldg_loft_showcase", "bldg_shop_showcase"]


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for block in (bpy.data.meshes, bpy.data.materials, bpy.data.cameras,
                  bpy.data.lights):
        for item in list(block):
            if item.users == 0:
                block.remove(item)


def material(bsdf_color, emissive, rough, metal):
    m = bpy.data.materials.new("m")
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*bsdf_color, 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if "Emission Color" in bsdf.inputs:
        bsdf.inputs["Emission Color"].default_value = (*emissive, 1.0)
        bsdf.inputs["Emission Strength"].default_value = (
            1.0 if any(emissive) else 0.0)
    return m


def build_part(part):
    mesh = bpy.data.meshes.new(part["name"])
    mesh.from_pydata(part["positions"], [], part["faces"])
    mesh.validate()
    mesh.update()
    ob = bpy.data.objects.new(part["name"], mesh)
    bpy.context.collection.objects.link(ob)
    ob.data.materials.append(material(
        part["base_color"], part["emissive"],
        part.get("roughness", 0.7), part.get("metallic", 0.0)))
    return ob


def setup_world_and_light():
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (
        0.05, 0.06, 0.09, 1.0)
    world.node_tree.nodes["Background"].inputs[1].default_value = 1.0
    bpy.context.scene.world = world

    sun_data = bpy.data.lights.new("sun", type="SUN")
    sun_data.energy = 4.0
    # `angle` is the sun's angular diameter in radians (a float), NOT its
    # direction — the direction is the rotation below.
    sun_data.angle = math.radians(3.0)
    sun = bpy.data.objects.new("sun", sun_data)
    sun.rotation_euler = (math.radians(50), 0.0, math.radians(35))
    bpy.context.collection.objects.link(sun)

    fill_data = bpy.data.lights.new("fill", type="AREA")
    fill_data.energy = 900.0
    fill_data.size = 12.0
    fill = bpy.data.objects.new("fill", fill_data)
    fill.location = (-10, -10, 12)
    bpy.context.collection.objects.link(fill)


def setup_render(width=1280, height=900):
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_EEVEE_NEXT"
    sc.render.resolution_x = width
    sc.render.resolution_y = height
    sc.render.resolution_percentage = 100
    sc.render.film_transparent = False
    sc.view_settings.view_transform = "Standard"


def shoot(objs, cam_loc, look_at, ortho, path):
    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = ortho
    cam = bpy.data.objects.new("cam", cam_data)
    bpy.context.collection.objects.link(cam)
    cam.location = cam_loc
    direction = mathutils.Vector(look_at) - mathutils.Vector(cam_loc)
    cam.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = cam
    bpy.context.view_layer.update()
    bpy.context.scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    bpy.data.objects.remove(cam, do_unlink=True)


def main() -> int:
    os.makedirs(OUT, exist_ok=True)
    for aid in ASSETS:
        with open(os.path.join(ROOT, "assets", f"{aid}.json")) as fh:
            data = json.load(fh)
        for view in ("cutaway", "stair", "plan"):
            clear_scene()
            setup_render()
            setup_world_and_light()
            # Rebuild the world collection each view.
            for part in data["parts"]:
                if view != "cutaway" and part["name"] in ("shell", "roof",
                                                          "windows", "cornice"):
                    continue
                if view == "cutaway" and part["name"] in ("roof", "windows",
                                                          "cornice"):
                    continue
                build_part(part)
            lo, hi = data["bounds"]["min"], data["bounds"]["max"]
            cx, cy = (lo[0] + hi[0]) / 2, (lo[2] + hi[2]) / 2
            h = hi[1]
            span = max(hi[0] - lo[0], hi[2] - lo[2]) * 1.35
            if view == "cutaway":
                # three-quarter from the doorway side (+Z), elevated
                shoot(None, (cx + span * 0.9, cy + span * 1.1, h * 1.5),
                      (cx, cy, h * 0.4), span * 1.6,
                      os.path.join(OUT, f"{aid}_cutaway.png"))
            elif view == "stair":
                # look at the stair core (local +X) from inside
                sx = hi[0] - (hi[0] - lo[0]) * 0.18
                shoot(None, (sx - span * 0.7, cy + span * 0.15, h * 0.75),
                      (sx, cy, h * 0.3), span * 0.95,
                      os.path.join(OUT, f"{aid}_stair.png"))
            else:
                # top-down plan
                shoot(None, (cx, cy, h * 3.0), (cx, cy, 0), span * 1.5,
                      os.path.join(OUT, f"{aid}_plan.png"))
            print("wrote", aid, view, flush=True)
    return 0


sys.exit(main())
