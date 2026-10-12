"""Ray-cast facade audit: is the detail actually REACHABLE?

A builder can emit thousands of triangles of facade detail that no player ever
sees, and every count-based check passes. The symptom is a flat, blank-looking
mass; the cause is geometry sealed inside other geometry.

The audit that matters is not "how many triangles" but "what is the FIRST
surface a ray from the street hits". So for every floor, fire rays at the
facade from outside and report which part owns the outermost hit. A floor
whose rays all land on `wall` and none on `glass` has glazing buried inside
solid masonry — modelled, invisible, and invisible to every other check.

Calibration: run against a deliberately-buried control before trusting a
pass. A negative here means nothing.
"""

from __future__ import annotations

import math
import os
import sys
from typing import Sequence

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def _raycast(origin, direction, objects, distance=200.0):
    """Scene ray cast. Returns (hit, location, owner_name)."""
    import bpy
    from mathutils import Vector

    depsgraph = bpy.context.evaluated_depsgraph_get()
    ok, loc, _normal, _idx, obj, _matrix = bpy.context.scene.ray_cast(
        depsgraph, Vector(origin), Vector(direction).normalized(), distance=distance
    )
    if not ok or obj is None:
        return False, None, None
    return True, loc, obj.name


def _face_of(coord: float, centre: float, axis: str) -> str:
    """Name the outward face a surface sits on.

    +Y face is "back", -Y is "front" (authoring convention is -Y forward);
    +X is "right", -X is "left".
    """
    if coord > centre:
        return "back" if axis == "y" else "right"
    return "front" if axis == "y" else "left"


def audit_facade(
    parts: Sequence,
    *,
    floor_height: float,
    glass_prefix: str = "glass",
    min_glazing_share: float = 0.0,
) -> dict:
    """Fire a ray at every window from outside; report glazing visibility.

    Sampling is per-**band**, not on a fixed height grid. A tower's glazing
    sits at each band's centre height, so a fixed grid of 8 sample heights
    finds windows on 3 of 8 samples and reports an empty floor for the other 5
    — a correctly built tower scored 0.50 because five of the eight "floors"
    were sampled at heights where that building has no glazing at all. The
    audit has to follow the geometry it is auditing.

    Rays are aimed at the **actual world-space centre of every glass object**.
    An evenly spread grid cannot resolve 1 m windows across an 18 m wall: rays
    land on mullions, and the instrument cannot tell a good building from a
    buried one.

    `matrix_world.translation` is useless here — every part has its transform
    baked into the mesh, so all origins are (0,0,0). Bounds must be read per
    vertex.
    """
    import bpy
    from mathutils import Vector

    bpy.context.view_layer.update()

    def bounds_of(obj):
        pts = [obj.matrix_world @ Vector(c) for c in obj.bound_box]
        return (
            (min(p.x for p in pts), max(p.x for p in pts)),
            (min(p.y for p in pts), max(p.y for p in pts)),
            (min(p.z for p in pts), max(p.z for p in pts)),
        )

    xs, ys, zs = [], [], []
    for obj in parts:
        if obj.type != "MESH":
            continue
        (ax, bx), (ay, by), (az, bz) = bounds_of(obj)
        xs += [ax, bx]
        ys += [ay, by]
        zs += [az, bz]
    if not xs:
        return {"error": "no geometry", "pass": False}

    min_x, max_x = min(xs), max(xs)
    min_y, max_y = min(ys), max(ys)
    max_z = max(zs)
    cx = (min_x + max_x) * 0.5
    cy = (min_y + max_y) * 0.5
    standoff = 40.0

    # group glazing by height band, then sample each band's own centre
    glass_objs = [o for o in parts if o.type == "MESH" and glass_prefix in o.name]
    band_map: dict[tuple, list] = {}
    for obj in glass_objs:
        (ax, bx), (ay, by), (az, bz) = bounds_of(obj)
        key = (round(az, 2), round(bz, 2))
        band_map.setdefault(key, []).append(obj)

    results = []
    total_glass = 0
    total_rays = 0

    for band_index, ((az, bz), objs) in enumerate(sorted(band_map.items())):
        z = (az + bz) * 0.5
        glass_hits = 0
        wall_hits = 0
        misses = 0

        for obj in objs:
            (ax, bx), (ay, by), (az2, bz2) = bounds_of(obj)
            # Outward face = whichever end of the window's own extent sits
            # furthest from the building centre. Deciding this from the
            # building's global centre is wrong whenever a band is deeper or
            # shallower than the one below it: band 1's front glazing sits at
            # y=+6.79 while the global centre is 0, so a centre-relative test
            # calls it "back" and fires the ray from the opposite side, through
            # the wall.
            # Decide the face axis from which extent is more off-centre, rather
            # than trying to infer it afterwards from a depth value. A window
            # wider than it is deep reads x-major; one deeper than it is wide
            # reads y-major.
            reach_x = max(abs(ax - cx), abs(bx - cx))
            reach_y = max(abs(ay - cy), abs(by - cy))
            if reach_y >= reach_x:
                lateral = (ax + bx) * 0.5
                if (by - cy) > (ay - cy):
                    outward, depth = 1.0, by
                else:
                    outward, depth = -1.0, ay
                origin = (lateral, depth + outward * standoff, z)
                direction = (0.0, -outward, 0.0)
            else:
                lateral = (ay + by) * 0.5
                if (bx - cx) > (ax - cx):
                    outward, depth = 1.0, bx
                else:
                    outward, depth = -1.0, ax
                origin = (depth + outward * standoff, lateral, z)
                direction = (-outward, 0.0, 0.0)

            hit, _loc, owner = _raycast(origin, direction, parts)
            if not hit:
                misses += 1
            elif owner and glass_prefix in owner:
                glass_hits += 1
            else:
                wall_hits += 1

        total_glass += glass_hits
        total_rays += len(objs)
        results.append({
            "band": band_index,
            "z": round(z, 2),
            "glass": glass_hits,
            "wall": wall_hits,
            "miss": misses,
        })

    share = total_glass / total_rays if total_rays else 0.0
    return {
        "floors": results,
        "glass_share": round(share, 4),
        "max_z": round(max_z, 2),
        "pass": share >= min_glazing_share,
    }


def print_report(name: str, report: dict) -> None:
    if "error" in report:
        print(f"  {name}: ERROR {report['error']}")
        return
    line = " ".join(f"b{f['band']}:{f['glass']}/{f['wall']}" for f in report["floors"])
    print(f"  {name}: {line}  share={report['glass_share']:.2f} "
          f"h={report['max_z']}m  {'PASS' if report['pass'] else 'FAIL'}")