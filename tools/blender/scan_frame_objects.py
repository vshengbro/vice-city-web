"""List every distinct object that occupies pixels in a preview frame.

Sweeps a grid of camera rays across the render and reports each hit object
once, with its pixel count. This answers "what is actually in this picture"
without reading a single pixel by hand, and it is how the scattered pale
fragments in the wep_tank preview were traced back to named parts.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/scan_frame_objects.py -- <asset_id> [step]
"""
import os
import sys
from collections import Counter

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402

import render_previews3a as R  # noqa: E402


def main():
    from vcw_bpy.registry import all_assets

    args = sys.argv[sys.argv.index("--") + 1:]
    aid = args[0]
    step = int(args[1]) if len(args) > 1 else 4

    fn, kw = {a: (f, k) for a, _c, f, k in all_assets()}[aid]
    parts = fn(aid, **kw)
    bpy.context.view_layer.update()

    lo, hi = R._world_bounds([p for p in parts if p is not None])
    centre = [(lo[i] + hi[i]) * 0.5 for i in range(3)]
    extent = max(hi[i] - lo[i] for i in range(3))
    radius = max(extent * 0.9, 0.6) * 2.6

    scene = bpy.context.scene
    cam_data = bpy.data.cameras.new(f"{aid}_cam")
    cam = bpy.data.objects.new(f"{aid}_cam", cam_data)
    scene.collection.objects.link(cam)
    R._frame_camera(cam, centre, radius)
    scene.camera = cam
    res = R.RES
    scene.render.resolution_x = scene.render.resolution_y = res
    bpy.context.view_layer.update()

    dg = bpy.context.evaluated_depsgraph_get()
    tr, br, bl, tl = cam.data.view_frame(scene=scene)
    # Rotate by the camera's ROTATION only: view_frame returns corners in
    # camera space, and applying the full inverse matrix scales each axis by
    # the camera scale, which throws every ray away from the asset.
    rot3 = cam.matrix_world.to_3x3()
    origin = cam.matrix_world.translation

    counts = Counter()
    first_hit = {}
    bg = 0
    for py in range(0, res, step):
        v = 1.0 - py / res
        for px in range(0, res, step):
            u = px / res
            # NDC v is 0 at the BOTTOM; bl/br are the bottom corners. Lerp
            # bottom-to-top or the ray sweeps the frame upside down — measured
            # as a 0.80 m perpendicular miss on a 2.6 m frame.
            p_cam = bl.lerp(br, u).lerp(tl.lerp(tr, u), v)
            direction = (rot3 @ p_cam).normalized()
            hit, _loc, _nor, _idx, obj, _mat = scene.ray_cast(dg, origin, direction)
            if not hit:
                bg += 1
                continue
            counts[obj.name] += 1
            first_hit.setdefault(obj.name, (px, py))

    total = sum(counts.values()) + bg
    print(f"{aid}: frame={res}x{res} step={step} rays={total} "
          f"hits={sum(counts.values())} background={bg} "
          f"({100.0 * bg / total:.1f}% empty)")
    print(f"  {len(counts)} distinct objects visible:")
    for name, n in counts.most_common():
        px, py = first_hit[name]
        print("    %-36s %6d px (%5.2f%%)  first at (%d,%d)"
              % (name, n * step * step, 100.0 * n / total, px, py))


if __name__ == "__main__":
    main()
