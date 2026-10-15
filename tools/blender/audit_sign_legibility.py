"""Render selected assets at higher resolution with an explicit Standard
view transform, so signage legibility can be judged properly.

Run:
  /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
      --python tools/blender/audit_sign_legibility.py
"""
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))
OUT = os.path.join(REPO, ".audit", "sign_hires")
os.makedirs(OUT, exist_ok=True)

import bpy
from mathutils import Vector

TARGETS = ["sign_arcade", "sign_bar", "sign_diner", "sign_tropic", "sign_hotel"]
RES = 1024


def render(aid, fn, kw):
    parts = fn(aid, **kw)
    # builders call reset_scene() -> read_factory_settings() destroys the Scene
    sc = bpy.context.scene
    objs = [o for o in parts if o is not None] if isinstance(
        parts, (list, tuple)) else [parts]
    objs = [o for o in objs if o]

    from vcw_bpy import export3a as X
    for obj in objs:
        for slot in getattr(obj.data, "materials", []):
            if slot is None:
                continue
            rgb, rough, metal = X.material_response(slot)
            try:
                from vcw_bpy import textures as TX
                TX.apply_textures(slot, rgb, rough, metal)
            except Exception:
                pass

    lo = [1e18] * 3
    hi = [-1e18] * 3
    for ob in objs:
        for c in ob.bound_box:
            w = ob.matrix_world @ Vector(c)
            for i in range(3):
                lo[i] = min(lo[i], w[i])
                hi[i] = max(hi[i], w[i])
    centre = [(lo[i] + hi[i]) * 0.5 for i in range(3)]
    extent = max(hi[i] - lo[i] for i in range(3))
    radius = max(extent * 0.9, 0.6) * 2.6

    import math
    cd = bpy.data.cameras.new("cam")
    cam = bpy.data.objects.new("cam", cd)
    sc.collection.objects.link(cam)
    d = math.atan2(0.62, 1.0)
    cam.location = (centre[0] + radius * math.sin(d),
                    centre[1] - radius * math.cos(d),
                    centre[2] + radius * 0.46)
    cam.rotation_euler = (math.radians(66.0), 0.0,
                          math.radians(math.degrees(d)))
    sc.camera = cam

    for name, loc, energy, size in (
            ("key", (centre[0] + radius, centre[1] - radius, centre[2] + radius), 900.0, 4.0),
            ("fill", (centre[0] - radius, centre[1] - radius * 0.5, centre[2] + radius * 0.4), 320.0, 6.0),
            ("rim", (centre[0] - radius * 0.4, centre[1] + radius, centre[2] + radius * 0.7), 500.0, 3.0)):
        ld = bpy.data.lights.new(name, type="AREA")
        ld.energy = energy
        ld.size = size
        lo_ = bpy.data.objects.new(name, ld)
        lo_.location = loc
        sc.collection.objects.link(lo_)
        lo_.rotation_euler = (Vector(centre) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()

    w = bpy.data.worlds.new("w")
    w.use_nodes = True
    bg = w.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.05, 0.06, 0.09, 1.0)
    bg.inputs[1].default_value = 1.0
    sc.world = w

    sc.render.engine = "BLENDER_EEVEE_NEXT"
    sc.render.resolution_x = RES
    sc.render.resolution_y = RES
    try:
        sc.eevee.taa_render_samples = 64
    except AttributeError:
        pass
    sc.view_settings.view_transform = "Standard"
    sc.view_settings.look = "None"
    p = os.path.join(OUT, f"{aid}_hires.png")
    sc.render.filepath = p
    bpy.ops.render.render(write_still=True)
    print("WROTE", p)


def main():
    from vcw_bpy.registry import all_assets
    reg = {aid: (fn, kw) for aid, _cat, fn, kw in all_assets()}
    for aid in TARGETS:
        fn, kw = reg[aid]
        render(aid, fn, kw)
    print("SIGN_HIRES_OK")


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        import traceback
        traceback.print_exc()
        print("SIGN_HIRES_FAILED")
