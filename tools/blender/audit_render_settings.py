"""A/B the colour-management and lighting settings the live preview pipeline uses,
measuring the result on real pixels.

render_previews3a.py sets engine/resolution/samples but NEVER sets
scene.view_settings.view_transform, so under --factory-startup it inherits
Blender 4.5's default. This measures what that costs.

Run:
  /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
      --python tools/blender/audit_render_settings.py
"""
import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))
OUT = os.path.join(REPO, ".audit", "render_ab")
os.makedirs(OUT, exist_ok=True)

import bpy
from mathutils import Vector

SAMPLE = ["bldg_lilac_tower", "car_sedan", "ped_suit", "sign_pizza",
          "beach_umbrella", "palm_tall"]
RES = 256


def clear_scene():
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    for blk in (bpy.data.meshes, bpy.data.materials, bpy.data.cameras,
                bpy.data.lights):
        for b in list(blk):
            blk.remove(b, do_unlink=True)


def pixels(path):
    img = bpy.data.images.load(path)
    try:
        px = list(img.pixels[:])
    finally:
        bpy.data.images.remove(img)
    r = g = b = a = 0.0
    lum = []
    sat = []
    n = len(px) // 4
    for i in range(0, len(px), 4):
        rr, gg, bb, aa = px[i], px[i + 1], px[i + 2], px[i + 3]
        if aa < 0.5:            # ignore transparent background
            continue
        r += rr; g += gg; b += bb; a += 1.0
        y = 0.2126 * rr + 0.7152 * gg + 0.0722 * bb
        lum.append(y)
        mx, mn = max(rr, gg, bb), min(rr, gg, bb)
        sat.append(0.0 if mx <= 0 else (mx - mn) / mx)
    if a == 0:
        return None
    return {
        "mean_rgb": [round(r / a, 4), round(g / a, 4), round(b / a, 4)],
        "mean_luma": round(sum(lum) / len(lum), 4),
        "mean_saturation": round(sum(sat) / len(sat), 4),
        "pct_pixels_over_0.5": round(
            100.0 * sum(1 for v in lum if v > 0.5) / len(lum), 3),
        "pct_pixels_over_0.75": round(
            100.0 * sum(1 for v in lum if v > 0.75) / len(lum), 3),
        "coverage_pct": round(100.0 * a / (len(px) // 4), 2),
    }


def render(aid, fn, kw, *, view_transform, look, samples, ground, label):
    clear_scene()
    parts = fn(aid, **kw)
    # The builders call reset_scene(), which does read_factory_settings() -- that
    # DESTROYS the Scene datablock, so any reference taken before the build is a
    # dangling StructRNA. Re-fetch the scene after building.
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

    cd = bpy.data.cameras.new("cam")
    cam = bpy.data.objects.new("cam", cd)
    sc.collection.objects.link(cam)
    import math
    d = math.atan2(0.62, 1.0)
    cam.location = (centre[0] + radius * math.sin(d),
                    centre[1] - radius * math.cos(d),
                    centre[2] + radius * 0.46)
    cam.rotation_euler = (math.radians(66.0), 0.0, math.radians(math.degrees(d)))
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

    if ground:
        # A shadow-catching ground plane at the asset's base, built with bmesh
        # so nothing goes through bpy.ops (ops needs a live view layer).
        import bmesh
        z = lo[2]
        half = max(extent * 3.0, 2.0)
        me = bpy.data.meshes.new("ground")
        bm = bmesh.new()
        bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=half)
        bm.to_mesh(me)
        bm.free()
        gp = bpy.data.objects.new("ground", me)
        gp.location = (centre[0], centre[1], z)
        sc.collection.objects.link(gp)
        m = bpy.data.materials.new("ground")
        m.use_nodes = True
        me.materials.append(m)

    w = bpy.data.worlds.new("w")
    w.use_nodes = True
    bg = w.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.05, 0.06, 0.09, 1.0)
    bg.inputs[1].default_value = 1.0
    sc.world = w

    sc.render.engine = "BLENDER_EEVEE_NEXT"
    sc.render.resolution_x = RES
    sc.render.resolution_y = RES
    sc.render.film_transparent = False
    try:
        sc.eevee.taa_render_samples = samples
    except AttributeError:
        pass
    sc.view_settings.view_transform = view_transform
    sc.view_settings.look = look

    p = os.path.join(OUT, f"{aid}__{label}.png")
    sc.render.filepath = p
    bpy.ops.render.render(write_still=True)
    res = pixels(p)
    res["path"] = os.path.relpath(p, REPO)
    res["view_transform"] = view_transform
    res["look"] = look
    res["samples"] = samples
    res["ground"] = ground
    return res


def main():
    from vcw_bpy.registry import all_assets
    reg = {aid: (cat, fn, kw) for aid, cat, fn, kw in all_assets()}
    variants = [
        ("A_live_asis", "AgX", "None", 16, False),     # what render_previews3a.py produces
        ("B_standard", "Standard", "None", 16, False),  # colour only changed
        ("C_standard_ground", "Standard", "None", 16, True),
        ("D_standard_ground_128", "Standard", "None", 128, True),
        ("E_filmic_high", "Filmic", "High Contrast", 32, True),
        ("F_standard_bogus_look", "Standard", "AgX - Punchy", 16, True),
    ]
    out = []
    for aid in SAMPLE:
        cat, fn, kw = reg[aid]
        print("###", aid)
        for label, vt, lk, sm, gr in variants:
            try:
                r = render(aid, fn, kw, view_transform=vt, look=lk,
                           samples=sm, ground=gr, label=label)
                print("  %-22s vt=%-9s luma=%.4f sat=%.4f >0.5=%.2f%% cov=%.1f%%"
                      % (label, vt, r["mean_luma"], r["mean_saturation"],
                         r["pct_pixels_over_0.5"], r["coverage_pct"]))
                out.append({"id": aid, **r})
            except Exception as exc:  # noqa: BLE001
                import traceback
                print("  %-22s FAILED %r" % (label, exc))
                traceback.print_exc()
                out.append({"id": aid, "label": label, "error": repr(exc)})
    with open(os.path.join(REPO, ".audit", "render_ab.json"), "w") as fh:
        json.dump(out, fh, indent=1)
    print("RENDER_AB_OK")


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        import traceback
        traceback.print_exc()
        print("RENDER_AB_FAILED")
