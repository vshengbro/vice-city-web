"""Render one asset under several engine settings, to attribute an artefact.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/render_ab_engine.py -- <asset_id>
"""
import os
import sys

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402

import render_previews3a as R  # noqa: E402

OUT = "/Users/sqs/.hermes/cache/scratch/art-fix/ab"


def variants():
    """(tag, engine, raytracing, samples, film_transparent)."""
    return (
        ("rt_on_eevee", "BLENDER_EEVEE_NEXT", True, 64, False),
        ("rt_off_eevee", "BLENDER_EEVEE_NEXT", False, 64, False),
        ("cycles_32", "CYCLES", None, 32, False),
    )


def main():
    from vcw_bpy.registry import all_assets

    args = sys.argv[sys.argv.index("--") + 1:]
    aid = args[0] if args else "wep_tank"
    res = int(args[1]) if len(args) > 1 else 1024
    os.makedirs(OUT, exist_ok=True)

    original_engine = None
    real_render = bpy.ops.render.render

    for tag, engine, raytracing, samples, film in variants():
        fn, kw = {a: (f, k) for a, _c, f, k in all_assets()}[aid]

        # Patch the two things render_one cannot be told directly.
        def patched_render(aid_, parts, out_dir, _engine=engine,
                           _rt=raytracing, _samples=samples, _film=film,
                           _tag=tag):
            return _render_with(aid_, parts, out_dir, _tag, _engine, _rt,
                                _samples, _film)

        globals()["_render_with"] = _render_with_factory(res)
        R.render_one = patched_render
        parts = fn(aid, **kw)
        r = R.render_one(aid, parts, OUT)
        print("  %-14s std=%-9s %s" % (tag, r.get("pixel_std"), r.get("path")))


def _render_with_factory(res):
    """Render under an explicit engine/raytracing setting, bypassing render_one."""
    import math
    from mathutils import Vector

    from vcw_bpy import export3a as X

    def _render_with(aid, parts, out_dir, tag, engine, raytracing, samples, film):
        for obj in parts:
            if obj is None:
                continue
            for slot in getattr(obj.data, "materials", []):
                if slot is None:
                    continue
                rgb, rough, metal = X.material_response(slot)
        lo, hi = R._world_bounds(parts)
        centre = [(lo[i] + hi[i]) * 0.5 for i in range(3)]
        extent = max(hi[i] - lo[i] for i in range(3))
        radius = max(extent * 0.9, 0.6) * 2.6
        scene = bpy.context.scene
        cam_data = bpy.data.cameras.new(f"{aid}_cam")
        cam = bpy.data.objects.new(f"{aid}_cam", cam_data)
        scene.collection.objects.link(cam)
        R._frame_camera(cam, centre, radius)
        scene.camera = cam
        scale = (radius / R.REF_DIST) ** 2
        az, el = math.radians(R.SUN_AZIMUTH), math.radians(R.SUN_ELEVATION)
        sd = bpy.data.lights.new("s", type="SUN")
        sd.energy = R.SUN_STRENGTH
        sd.angle = R.SUN_ANGLE
        sun = bpy.data.objects.new("s", sd)
        sun.location = (centre[0] + radius * math.cos(el) * math.sin(az),
                        centre[1] - radius * math.cos(el) * math.cos(az),
                        centre[2] + radius * math.sin(el))
        scene.collection.objects.link(sun)
        sun.rotation_euler = (Vector(centre) - Vector(sun.location)).to_track_quat("-Z", "Y").to_euler()
        for name, off, be, bs in R._light_rig(radius):
            d = bpy.data.lights.new(name, type="AREA")
            d.energy = be * scale
            d.size = bs * (radius / R.REF_DIST)
            lo2 = bpy.data.objects.new(name, d)
            lo2.location = tuple(centre[i] + off[i] for i in range(3))
            scene.collection.objects.link(lo2)
            lo2.rotation_euler = (Vector(centre) - Vector(lo2.location)).to_track_quat("-Z", "Y").to_euler()
        world = bpy.data.worlds.new("w")
        world.use_nodes = True
        bg = world.node_tree.nodes["Background"]
        bg.inputs[0].default_value = R.WORLD_COLOUR
        bg.inputs[1].default_value = R.WORLD_STRENGTH
        scene.world = world
        scene.render.engine = engine
        scene.render.resolution_x = scene.render.resolution_y = res
        scene.render.film_transparent = film
        R._set_view_transform(scene)
        if engine == "BLENDER_EEVEE_NEXT":
            scene.eevee.taa_render_samples = samples
            scene.eevee.use_shadows = True
            for attr, value in (("use_raytracing", raytracing), ("use_gtao", raytracing)):
                try:
                    setattr(scene.eevee, attr, value)
                except (AttributeError, TypeError):
                    pass
        else:
            scene.cycles.samples = samples
        path = os.path.join(out_dir, f"{tag}.png")
        scene.render.filepath = path
        bpy.ops.render.render(write_still=True)
        return {"id": aid, "ok": True, "pixel_std": round(R._pixel_stddev(path), 3),
                "path": path, "view_transform": scene.view_settings.view_transform}

    return _render_with


if __name__ == "__main__":
    main()
