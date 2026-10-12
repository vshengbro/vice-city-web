"""Render a preview per asset and prove the frame is not blank.

Two rules from this pipeline's history are enforced here:

1. `pixel stddev > 0` means "not a flat fill". A sign rendered from behind is a
   blank panel WITH healthy stddev, so the camera must be aimed relative to
   each asset's own authored front (-Y), never at a fixed azimuth.
2. `std == 0` means the camera is inside geometry or framed nothing. That is a
   framing bug, and reporting it as "lighting is off" sends the next pass in
   the wrong direction.

    Blender --background --factory-startup --python tools/blender/render_previews3a.py
"""

import math
import os
import sys
import traceback

import bpy

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

OUT_DIR = os.path.join(REPO, "www", "preview")
RES = 512


def _frame_camera(cam_obj, centre, radius):
    """Orbit the camera to a 3/4 view from the asset's FRONT (-Y) side."""
    direction = math.atan2(0.62, 1.0)  # ~32 deg off the front axis
    cam_obj.location = (
        centre[0] + radius * math.sin(direction),
        centre[1] - radius * math.cos(direction),
        centre[2] + radius * 0.46,
    )
    cam_obj.rotation_euler = (
        math.radians(66.0),
        0.0,
        math.radians(math.degrees(direction)),
    )


def _world_bounds(parts):
    from mathutils import Vector
    lo = [1e18] * 3
    hi = [-1e18] * 3
    for obj in parts:
        if obj is None or obj.type != "MESH":
            continue
        for c in obj.bound_box:
            w = obj.matrix_world @ Vector(c)
            for i in range(3):
                lo[i] = min(lo[i], w[i])
                hi[i] = max(hi[i], w[i])
    if lo[0] > hi[0]:
        return None, None
    return lo, hi


def render_one(asset_id, parts, out_dir):
    from mathutils import Vector

    bounds = _world_bounds(parts)
    if bounds is None or bounds[0] is None or bounds[1] is None:
        return {"id": asset_id, "ok": False, "why": "no geometry"}

    lo, hi = bounds
    centre = [(lo[i] + hi[i]) * 0.5 for i in range(3)]
    extent = max(hi[i] - lo[i] for i in range(3))
    radius = max(extent * 0.9, 0.6) * 2.6

    scene = bpy.context.scene
    cam_data = bpy.data.cameras.new(f"{asset_id}_cam")
    cam = bpy.data.objects.new(f"{asset_id}_cam", cam_data)
    scene.collection.objects.link(cam)
    _frame_camera(cam, centre, radius)
    scene.camera = cam

    # three-point-ish rig: a key light, a fill, and a rim, all outside the asset
    for name, loc, energy, size in (
        ("key", (centre[0] + radius, centre[1] - radius, centre[2] + radius), 900.0, 4.0),
        ("fill", (centre[0] - radius, centre[1] - radius * 0.5, centre[2] + radius * 0.4),
         320.0, 6.0),
        ("rim", (centre[0] - radius * 0.4, centre[1] + radius, centre[2] + radius * 0.7),
         500.0, 3.0),
    ):
        data = bpy.data.lights.new(f"{asset_id}_{name}", type="AREA")
        data.energy = energy
        data.size = size
        light = bpy.data.objects.new(f"{asset_id}_{name}", data)
        light.location = loc
        scene.collection.objects.link(light)
        # aim at the asset
        d = Vector(centre) - Vector(loc)
        light.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()

    world = bpy.data.worlds.new(f"{asset_id}_world")
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.05, 0.06, 0.09, 1.0)
    bg.inputs[1].default_value = 1.0
    scene.world = world

    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x = RES
    scene.render.resolution_y = RES
    scene.render.film_transparent = False
    try:
        scene.eevee.taa_render_samples = 16
    except AttributeError:
        pass

    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, f"{asset_id}.png")
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)

    std = _pixel_stddev(path)
    return {
        "id": asset_id,
        "ok": std > 2.0,
        "pixel_std": round(std, 3),
        "path": os.path.relpath(path, REPO),
    }


def _pixel_stddev(path):
    """Stddev of the rendered pixels — the non-blank assertion."""
    import struct
    import zlib

    with open(path, "rb") as fh:
        data = fh.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        return -1.0
    pos = 8
    idat = b""
    width = height = 0
    while pos < len(data):
        length, ctype = struct.unpack(">II", data[pos:pos + 8])
        chunk = data[pos + 8:pos + 8 + length]
        if ctype == 0x49484452:  # IHDR
            width, height = struct.unpack(">II", chunk[:8])
        elif ctype == 0x49444154:  # IDAT
            idat += chunk
        pos += 12 + length
    raw = zlib.decompress(idat)

    # undo the per-scanline PNG filter, then take the variance of the luma
    stride = width * 4
    prev = bytearray(stride)
    vals = []
    off = 0
    for _y in range(height):
        ftype = raw[off]
        line = bytearray(raw[off + 1:off + 1 + stride])
        off += 1 + stride
        if ftype == 1:
            for i in range(4, stride):
                line[i] = (line[i] + line[i - 4]) & 0xFF
        elif ftype == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif ftype == 3:
            for i in range(stride):
                left = line[i - 4] if i >= 4 else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif ftype == 4:
            for i in range(stride):
                a = line[i - 4] if i >= 4 else 0
                b = prev[i]
                c = prev[i - 4] if i >= 4 else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        prev = line
        for x in range(0, stride, 4):
            vals.append((line[x] * 299 + line[x + 1] * 587 + line[x + 2] * 114) // 1000)
    if not vals:
        return -1.0
    mean = sum(vals) / len(vals)
    var = sum((v - mean) ** 2 for v in vals) / len(vals)
    return var ** 0.5


def main() -> int:
    from vcw_bpy.registry import all_assets

    only = set(sys.argv[sys.argv.index("--") + 1:]) if "--" in sys.argv else None
    entries = all_assets()

    results = []
    failures = []
    for aid, category, fn, kw in entries:
        if only and aid not in only:
            continue
        try:
            parts = fn(aid, **kw)
            res = render_one(aid, parts, OUT_DIR)
        except Exception as exc:  # noqa: BLE001
            res = {"id": aid, "ok": False, "why": repr(exc)}
            traceback.print_exc()
        results.append(res)
        flag = "" if res["ok"] else "  <-- BLANK/FAILED"
        print(f"  {aid:34s} std={res.get('pixel_std', res.get('why'))}{flag}")
        if not res["ok"]:
            failures.append(aid)

    print()
    print(f"PREVIEW: {len(results) - len(failures)}/{len(results)} non-blank")
    if failures:
        print("BLANK: " + ", ".join(failures))
        return 1
    print("PREVIEW_OK")
    return 0


if __name__ == "__main__":
    try:
        rc = main()
    except Exception:
        traceback.print_exc()
        rc = 3
    print(f"RC={rc}")
    sys.exit(rc)