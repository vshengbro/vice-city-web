"""Identify what geometry occupies given pixels of a preview render.

Re-frames the asset exactly as `render_previews3a.render_one` does, then
ray-casts the camera through a grid of image pixels and reports which object
each ray hits. That turns "there are pale chips down there" into "those are
wep_tank_bell_gusset0_3" — a name you can go and fix.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/identify_pixels.py -- <asset_id> <x,y> <x,y> ...
"""
import os
import sys

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402
from bpy_extras.object_utils import world_to_camera_view  # noqa: E402
from mathutils import Vector  # noqa: E402

import render_previews3a as R  # noqa: E402


def main():
    from vcw_bpy.registry import all_assets

    args = sys.argv[sys.argv.index("--") + 1:]
    aid = args[0]
    # Pixel coordinates are in the RENDER's resolution, which is R.RES unless
    # this script was called from a driver that changed it. Probing with
    # coordinates from a 1024 render against a 512 camera silently reports
    # every probe as background, because the ray goes to the wrong place.
    res = int(args[1]) if len(args) > 1 and args[1].isdigit() else R.RES
    pixels = [tuple(float(v) for v in a.split(",")) for a in args[2:]]
    if not pixels:
        pixels = [tuple(float(v) for v in a.split(",")) for a in args[1:]
                  if "," in a]

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
    scene.render.resolution_x = scene.render.resolution_y = res
    bpy.context.view_layer.update()

    dg = bpy.context.evaluated_depsgraph_get()
    print(f"{aid}: centre={[round(c,3) for c in centre]} radius={radius:.3f} res={res}")
    for (px, py) in pixels:
        u = px / res
        v = 1.0 - py / res
        tr, br, bl, tl = cam.data.view_frame(scene=scene)
        # NDC v is 0 at the BOTTOM of the frame; bl/br are the bottom
        # corners, so interpolate bottom-to-top. Lerping top-to-bottom sends
        # every ray to the vertically mirrored pixel — measured as a 0.80 m
        # perpendicular miss on a 2.6 m frame, which silently reports the
        # whole lower half as background.
        p_cam = bl.lerp(br, u).lerp(tl.lerp(tr, u), v)
        origin = cam.matrix_world.translation
        # The frustum corners are in CAMERA SPACE at unit-ish depth, so the
        # ray direction must be rotated by the camera's rotation ONLY.
        # Applying the full inverse matrix scales each axis by the camera's
        # scale — non-uniform, and it sends every ray off into space.
        direction = (cam.matrix_world.to_3x3() @ p_cam).normalized()
        hit, loc, nor, idx, obj, _mat = scene.ray_cast(dg, origin, direction)
        dist = (loc - origin).length if hit else float("inf")
        print("  pixel(%6.1f,%6.1f) -> %-34s hit=%s dist=%.3f"
              % (px, py, obj.name if hit else "(background)", hit, dist))


if __name__ == "__main__":
    main()
