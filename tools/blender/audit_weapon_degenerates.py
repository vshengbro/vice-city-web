"""Count zero-area / micro-area faces per weapon, before and after a fix.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/audit_weapon_degenerates.py
"""
import json
import os
import sys
import traceback

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bmesh  # noqa: E402
import bpy  # noqa: E402

ZERO = 0.0
MICRO = 1e-9


def clear_scene():
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    for blk in (bpy.data.meshes, bpy.data.materials, bpy.data.cameras,
                bpy.data.lights):
        for b in list(blk):
            blk.remove(b, do_unlink=True)


def count_degenerate(objs):
    """Zero-area and sub-micrometre face counts, and the worst offenders.

    A face with zero or near-zero area is not a cosmetic problem: it renders as
    a bright scattered fragment, and it is what makes a magazine look like it
    floats free of the weapon.
    """
    faces = zero = micro = 0
    offenders = []
    for ob in objs:
        if ob is None or ob.type != "MESH":
            continue
        bm = bmesh.new()
        try:
            bm.from_mesh(ob.data)
            faces += len(bm.faces)
            for f in bm.faces:
                a = f.calc_area()
                if a <= ZERO:
                    zero += 1
                    offenders.append((a, ob.name, len(f.verts)))
                elif a < MICRO:
                    micro += 1
                    offenders.append((a, ob.name, len(f.verts)))
        finally:
            bm.free()
    offenders.sort()
    return faces, zero, micro, offenders


def main():
    from vcw_bpy.registry import all_assets

    out = []
    for aid, category, fn, kw in all_assets():
        if category != "weapon":
            continue
        clear_scene()
        try:
            parts = fn(aid, **kw)
        except Exception as exc:  # noqa: BLE001
            traceback.print_exc()
            out.append({"id": aid, "error": repr(exc)})
            continue
        faces, zero, micro, offenders = count_degenerate(
            [p for p in parts if p is not None])
        pct = 100.0 * (zero + micro) / faces if faces else 0.0
        # group offenders by object so the report names the guilty part
        by_obj = {}
        for _a, name, _n in offenders:
            by_obj[name] = by_obj.get(name, 0) + 1
        worst = sorted(by_obj.items(), key=lambda kv: -kv[1])[:6]
        out.append({"id": aid, "faces": faces, "zero_area": zero,
                    "micro_area": micro, "pct": round(pct, 2),
                    "worst_parts": worst})
        print("  %-22s faces=%-6d zero=%-5d micro=%-5d %5.2f%%  %s"
              % (aid, faces, zero, micro, pct, worst[:3]))

    os.makedirs(os.path.join(REPO, ".audit"), exist_ok=True)
    path = os.path.join(REPO, ".audit", "weapon_degenerates.json")
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(out, fh, indent=1)
    print("\nWROTE", path)
    print("WEAPON_AUDIT_OK")


if __name__ == "__main__":
    main()
