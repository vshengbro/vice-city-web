"""Build every asset in the registry, verify each one, export both products.

Run it with the real Blender, not python3:

    /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
        --python tools/blender/build_all3a.py

Two outputs per asset:
  assets/<id>.json      — the game-facing geometry, in the SHIPPED schema, so
                          src/game.rs loads it with no code change
  assets/pbr/<id>.glb   — the 3A master: UV + tangent + layered PBR materials

Why both: src/mesh.rs MeshPart has only positions / normals / faces /
base_color / emissive / face_colors / flat. There is no uv, tangent,
roughness or metallic field, and the loader is serde against that struct. A
GLB-only pipeline would load 0 of 99 assets. The GLB carries the modelling
investment until the renderer grows the vertex format.

`Blender quit` and exit code 0 happen even when a script raises, so BUILD_OK is
the load-bearing marker, not the exit status.
"""

import json
import os
import sys
import traceback

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

OUT_DIR = os.path.join(REPO, "assets")
PBR_DIR = os.path.join(OUT_DIR, "pbr")
# Frozen from the shipped library: the registry must produce exactly this set.
BASELINE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        "asset_baseline.json")


def _baseline_ids() -> set:
    with open(BASELINE, encoding="utf-8") as fh:
        return {entry["id"] for entry in json.load(fh)["assets"]}


def main() -> int:
    from vcw_bpy import export3a as X
    from vcw_bpy.registry import all_assets
    from verify3a import verify_asset

    os.makedirs(OUT_DIR, exist_ok=True)
    os.makedirs(PBR_DIR, exist_ok=True)

    entries = all_assets()

    # A registry that does not match what the game ships must fail loudly, so a
    # typo cannot quietly produce 98 of 99 assets behind a green summary line.
    expected = _baseline_ids()
    registered = {aid for aid, _cat, _fn, _kw in entries}
    missing = expected - registered
    extra = registered - expected
    if missing or extra:
        print(f"REGISTRY MISMATCH: missing={sorted(missing)} extra={sorted(extra)}")
        return 2

    built = []
    per_category: dict[str, dict] = {}
    failures: list = []
    verify_failures: list = []

    for aid, category, fn, kw in entries:
        try:
            parts = fn(aid, **kw)
            asset = X.build_asset_dict(aid, category, parts)
            # Watercraft float: check them against their real waterline, not
            # against the hard y=0 rule that ground assets must obey. The
            # declared value is the DRAFT (a positive depth below the surface),
            # which is what verify_asset compares against — passing the signed
            # world y instead made every ship fail on "waterline 0.1 != 0.1".
            waterline = None
            if category == "watercraft":
                lowest = min(pt[1] for p in asset["parts"] for pt in p["positions"])
                waterline = abs(lowest)
            violations = verify_asset(asset, ground_category=category,
                                      waterline=waterline)
            if violations:
                verify_failures.append((aid, "; ".join(violations[:4])))
                print(f"VERIFY FAIL {aid}: {violations[:4]}")
                continue
            json_path = X.write_asset_json(asset, OUT_DIR)
            glb_path = X.write_pbr_glb(aid, parts, PBR_DIR)
        except Exception as exc:  # noqa: BLE001
            failures.append((aid, repr(exc)))
            traceback.print_exc()
            continue

        tri = asset["tri_count"]
        built.append({
            "id": aid,
            "category": category,
            "file": os.path.relpath(json_path, OUT_DIR),
            "pbr_file": (os.path.relpath(glb_path, OUT_DIR) if glb_path else None),
            "tri_count": tri,
            "part_count": asset["part_count"],
            "bounds": asset["bounds"],
        })
        cat = per_category.setdefault(category, {"count": 0, "triangles": 0})
        cat["count"] += 1
        cat["triangles"] += tri
        flag = "" if glb_path else "  NO_GLB"
        print(f"  {aid:34s} {category:11s} parts={asset['part_count']:3d} "
              f"tris={tri:6d}{flag}")

    manifest = {
        "asset_count": len(built),
        "total_triangles": sum(e["tri_count"] for e in built),
        "generator": "build_all3a.py (bpy)",
        "categories": per_category,
        "assets": built,
    }
    X.write_manifest(built, OUT_DIR, manifest["generator"])

    print()
    print("=== per-category breakdown ===")
    for cat in sorted(per_category):
        print(f"  {cat:12s} {per_category[cat]['count']:3d} assets  "
              f"{per_category[cat]['triangles']:7d} tris")
    print()
    if verify_failures:
        print(f"VERIFY FAILURES: {len(verify_failures)}")
        for aid, why in verify_failures:
            print(f"  {aid}: {why}")
    if failures:
        print(f"BUILD FAILURES: {len(failures)}")
        for aid, why in failures:
            print(f"  {aid}: {why}")

    # A partial library must not report success: 98 of 99 assets is a failure,
    # and the aggregate alone hides which family contributed nothing.
    if failures or verify_failures or len(built) != len(entries):
        print(f"BUILD INCOMPLETE: {len(built)}/{len(entries)} assets")
        return 1

    print(f"BUILD_OK: {len(built)} assets, {manifest['total_triangles']} triangles")
    return 0


if __name__ == "__main__":
    try:
        rc = main()
    except Exception:
        traceback.print_exc()
        rc = 3
    print(f"RC={rc}")
    sys.exit(rc)