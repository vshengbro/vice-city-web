"""Render a preview per asset and prove the frame is not blank.

Three rules from this pipeline's history are enforced here:

1. `pixel stddev > 0` means "not a flat fill". A sign rendered from behind is a
   blank panel WITH healthy stddev, so the camera must be aimed relative to
   each asset's own authored front (-Y), never at a fixed azimuth.
2. `std == 0` means the camera is inside geometry or framed nothing. That is a
   framing bug, and reporting it as "lighting is off" sends the next pass in
   the wrong direction.
3. **Exposure must not depend on the asset's size.** See `_light_rig` — the
   old fixed-wattage rig put a 0.15 m grenade at 123 units of irradiance and a
   48 m ship at 0.023, a 5,271x spread, which made every building read as an
   untextured blockout no matter how well it was modelled.

    Blender --background --factory-startup --python tools/blender/render_previews3a.py
"""

import json
import math
import os
import sys
import traceback

import bpy

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

OUT_DIR = os.path.join(REPO, "www", "preview")
RES = 512

# ---------------------------------------------------------------------------
# exposure constants
# ---------------------------------------------------------------------------
# The KEY is a SUN, not a point/area light. Irradiance from a sun is its
# Strength in W/m^2 and is independent of distance by construction, so a 48 m
# ship and a 0.15 m grenade are lit identically for free — no distance maths,
# nothing to get wrong later. The old rig used a 900 W area light at a distance
# derived from each asset's own bbox, so irradiance went as 1/size^2: a
# measured 5,271x spread, Pearson r(ln extent, mean luma) = -0.649 across the
# 99 shipped previews, with buildings at ~1% of the light the props got.
#
# The fill and rim ARE area lights (a sky dome and a kicker are not
# directional), so their wattage and panel size scale with the framing radius
# by (d / REF_DIST) ** 2 — the same inverse-square compensation — which keeps
# them size-invariant too.
#
# The sun strength, the fill and the rim were chosen from a measured sweep
# over 5 assets spanning building / weapon / prop / vehicle / beach, scoring
# SUBJECT-only mean luma and clipped-pixel fraction (full-frame luma is
# dominated by framing area, so it cannot pick an exposure):
#
#   sun  fill  rim | subject luma | clipped | note
#   3.0    80  140 |     0.7025   | 25.8%   | first guess; blown out
#   1.6    45   80 |     0.5801   |  8.9%   |
#   1.2    35   60 |     0.5084   |  0.7%   | <- chosen
#   0.9    25   45 |     0.4355   |  0.4%   |
#   0.7    20   35 |     0.3722   |  0.4%   |
#
# 1.2/35/60 puts a mid-tone asset at ~50% luma with under 1% of subject
# pixels clipped, which is what a review sheet should look like. The ratio
# sun:fill:rim = 1 : 0.4 : 0.7 is a sunny street, not a studio.
SUN_STRENGTH = 1.2
# Hard Miami sun: a small angular diameter gives a crisp terminator and a
# readable cast shadow instead of a soft wrap.
SUN_ANGLE = 0.035  # radians; the real sun is 0.0093

REF_DIST = 3.0
# Fill/rim energies AT REF_DIST, in watts. The key:fill:rim ratio of the old
# rig is deliberately NOT preserved here, and that matters. With a 900 W area
# key at REF_DIST the old fill (320 W) and rim (500 W) already contribute
# 320/(4*pi*9) + 500/(4*pi*9) = 7.2 W/m^2 between them — more than any sun
# in the sweep below. A measured sweep at sun = 1.2 / 2.0 / 3.0 / 4.0 with
# the old fill/rim moved the tower's mean luma by 0.004: the sun was doing
# nothing. These values put fill at 0.31 W/m^2 and rim at 0.53 W/m^2.
BASE_FILL, BASE_RIM = 35.0, 60.0
FILL_SIZE, RIM_SIZE = 6.0, 3.0
# Sky fill. A constant world background contributes a size-INVARIANT ambient
# term, which is what stops the shadow side of a large asset going to pure
# black.
WORLD_COLOUR = (0.055, 0.068, 0.100, 1.0)
WORLD_STRENGTH = 1.0
# Sun azimuth/elevation relative to the camera, which frames from the asset's
# front-left. 32 deg off the front axis, 42 deg up — high enough to read as
# midday rather than dusk, low enough to throw a shadow across the facade.
SUN_AZIMUTH = 32.0
SUN_ELEVATION = 42.0


def _light_rig(radius):
    """Return the fill/rim rows for a framing radius `radius`.

    Offsets are unit-ish directions scaled by the radius and energies are
    REF_DIST-normalised, so irradiance at the asset is P/d^2 * d^2 = constant.
    """
    d = radius
    return (
        ("fill", (-d, -d * 0.5, d * 0.4), BASE_FILL, FILL_SIZE),
        ("rim", (-d * 0.4, d, d * 0.7), BASE_RIM, RIM_SIZE),
    )


def _set_view_transform(scene, name: str = "Standard") -> str:
    """Force the display transform and return the one actually in effect.

    Blender 4.5's factory default is AgX, which desaturates and crushes the
    whole image into a narrow mid-grey band; the shipped previews inherited it
    and 24/99 came out with zero pixels above the background's own luma.
    `look` must be cleared too — the AgX 'Punchy' look is not a fix, it is a
    different bug.
    """
    scene.display_settings.display_device = "sRGB"
    try:
        scene.view_settings.view_transform = name
    except TypeError:
        # Fall back to whatever is available, and say so in the summary rather
        # than silently rendering a different transform than the one reported.
        scene.view_settings.view_transform = "Standard"
    try:
        scene.view_settings.look = "None"
    except TypeError:
        pass
    scene.view_settings.exposure = 0.0
    scene.view_settings.gamma = 1.0
    return scene.view_settings.view_transform


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

    # Texture the materials before rendering, or the preview shows flat
    # material colours and the texture layer is never actually looked at.
    # `write_pbr_glb` is the single place that knows how a material's real
    # base colour / roughness / metallic are read back out of Blender 4.
    from vcw_bpy import export3a as X

    for obj in parts:
        if obj is None:
            continue
        for slot in getattr(obj.data, "materials", []):
            if slot is None:
                continue
            rgb, rough, metal = X.material_response(slot)
            try:
                from vcw_bpy import textures as TX

                TX.apply_textures(slot, rgb, rough, metal)
            except Exception as exc:  # pragma: no cover - preview must not die
                print(f"  texture skip {slot.name}: {exc!r}")

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

    # SUN key. Strength is irradiance in W/m^2 and does not fall off with
    # distance, so this one line is what makes exposure size-invariant.
    sun_data = bpy.data.lights.new(f"{asset_id}_sun", type="SUN")
    sun_data.energy = SUN_STRENGTH
    sun_data.angle = SUN_ANGLE
    sun = bpy.data.objects.new(f"{asset_id}_sun", sun_data)
    az = math.radians(SUN_AZIMUTH)
    el = math.radians(SUN_ELEVATION)
    # From the camera's 3/4 front-left vantage, so the lit face and the shaded
    # face are both visible and neither is edge-on.
    sun.location = (
        centre[0] + radius * math.cos(el) * math.sin(az),
        centre[1] - radius * math.cos(el) * math.cos(az),
        centre[2] + radius * math.sin(el),
    )
    scene.collection.objects.link(sun)
    aim = Vector(centre) - Vector(sun.location)
    sun.rotation_euler = aim.to_track_quat("-Z", "Y").to_euler()

    # Fill and rim: area lights, so their wattage and panel size scale with
    # the framing radius by (d / REF_DIST) ** 2. Irradiance at the asset is
    # then P/d^2 * d^2 = constant, matching the sun.
    scale = (radius / REF_DIST) ** 2
    for name, offset, base_energy, base_size in _light_rig(radius):
        loc = tuple(centre[i] + offset[i] for i in range(3))
        data = bpy.data.lights.new(f"{asset_id}_{name}", type="AREA")
        data.energy = base_energy * scale
        data.size = base_size * (radius / REF_DIST)
        light = bpy.data.objects.new(f"{asset_id}_{name}", data)
        light.location = loc
        scene.collection.objects.link(light)
        # aim at the asset
        d = Vector(centre) - Vector(loc)
        light.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()

    world = bpy.data.worlds.new(f"{asset_id}_world")
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = WORLD_COLOUR
    bg.inputs[1].default_value = WORLD_STRENGTH
    scene.world = world

    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x = RES
    scene.render.resolution_y = RES
    scene.render.film_transparent = False
    view_transform = _set_view_transform(scene)
    # Shadow + AO on. Without them every surface gets a single flat value and
    # even a well-lit asset reads as flat-shaded blockout geometry.
    try:
        scene.eevee.taa_render_samples = 64
        scene.eevee.use_shadows = True
    except AttributeError:
        pass
    for attr, value in (("use_raytracing", True), ("use_gtao", True)):
        try:
            setattr(scene.eevee, attr, value)
        except (AttributeError, TypeError):
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
        "view_transform": view_transform,
        "sun_energy": SUN_STRENGTH,
        "fill_energy": round(BASE_FILL * scale, 1),
        "radius": round(radius, 3),
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
    """Render every registry asset (or the subset named after `--`).

    Writes `.audit/preview_render.json` with per-asset framing, key wattage
    and the view transform actually used, so a later pass can check the
    exposure is size-invariant without re-deriving it from the PNGs.
    """
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
            res["category"] = category
        except Exception as exc:  # noqa: BLE001
            res = {"id": aid, "ok": False, "why": repr(exc)}
            traceback.print_exc()
        results.append(res)
        flag = "" if res["ok"] else "  <-- BLANK/FAILED"
        print(f"  {aid:34s} std={res.get('pixel_std', res.get('why'))}"
              f"  sun={res.get('sun_energy', '-')} fill={res.get('fill_energy', '-')}W"
              f"  vt={res.get('view_transform', '-')}{flag}")
        if not res["ok"]:
            failures.append(aid)

    report_dir = os.path.join(REPO, ".audit")
    os.makedirs(report_dir, exist_ok=True)
    with open(os.path.join(report_dir, "preview_render.json"), "w") as fh:
        json.dump({"ref_dist": REF_DIST, "results": results}, fh, indent=1)

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