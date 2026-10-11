#!/usr/bin/env python3.11
"""Render every asset so each model can be judged visually.

Uses the proven helpers in ``render_previews.py`` (camera framing, three-point
rig, world, material building) rather than a hand-rolled camera.  The previous
version of this script built contact sheets itself and every sheet came out
pure black: the scene had no light rig, and the ortho camera was aimed wrong.

Each asset renders to its own PNG, and ``image_std`` reports the pixel standard
deviation.  A blank or single-colour image has a std near zero, so this doubles
as a machine check that the render actually contains a model.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/render_all_assets.py
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "tools", "blender"))

import bpy  # noqa: E402
import render_previews as rp  # noqa: E402

OUT = os.path.join(ROOT, "www", "preview")
os.makedirs(OUT, exist_ok=True)

# A render whose std dev is below this is a flat/blank image, not a model.
BLANK_STD = 0.0008


def load_manifest() -> dict:
    with open(os.path.join(ROOT, "assets", "manifest.json"),
              encoding="utf-8") as handle:
        return json.load(handle)


def main():
    manifest = load_manifest()
    # rp.build_object wants builder objects (`asset.parts[i].mesh.pos`), not
    # the JSON dicts -- passing a dict raises
    # `AttributeError: 'dict' object has no attribute 'parts'`.
    # collect_assets() runs every builder's build_all() and returns them.
    assets = rp.collect_assets()
    print(f"assets in manifest: {manifest['asset_count']}  "
          f"built: {len(assets)}", flush=True)
    if len(assets) != manifest["asset_count"]:
        print("WARNING: manifest and builders disagree on asset count",
              flush=True)

    blank: list = []
    by_cat: dict = {}
    for asset in assets:
        # ⚠️ clear_scene() 会销毁旧的 Scene,之后再拿 scene 就是
        # `ReferenceError: StructRNA of type Scene has been removed`。
        # 每轮都要**重新**取。
        rp.clear_scene()
        scene = bpy.context.scene
        rp.setup_render(scene)
        rp.setup_world(scene)
        objs = rp.build_object(asset, scene.collection)
        centre, _radius, _dist = rp.setup_camera(scene, scene.collection,
                                                 asset)
        # add_rig wants the point to light around, i.e. the model centre --
        # passing None raised `TypeError: cannot unpack non-iterable NoneType`.
        rp.add_rig(scene, scene.collection, centre)
        bpy.context.view_layer.update()
        out = os.path.join(OUT, f"{asset.id}.png")
        scene.render.filepath = out
        bpy.ops.render.render(write_still=True)
        std, _ = rp.image_std(out)
        tris = sum(len(o.data.polygons) for o in objs)
        by_cat.setdefault(asset.category, []).append(
            (asset.id, tris, round(std, 6)))
        flag = ""
        if std < BLANK_STD:
            blank.append(asset.id)
            flag = "  <== BLANK"
        print(f"  {asset.id:24} {tris:7} tris  std={std:.5f}{flag}",
              flush=True)

    summary = os.path.join(OUT, "render_report.json")
    with open(summary, "w", encoding="utf-8") as handle:
        json.dump({cat: rows for cat, rows in by_cat.items()},
                  handle, indent=2, sort_keys=True)
    print(f"\ncategories: {len(by_cat)}", flush=True)
    for cat in sorted(by_cat):
        print(f"  {cat:12} {len(by_cat[cat])}", flush=True)
    if blank:
        print(f"\nBLANK renders ({len(blank)}): {blank}", flush=True)
    else:
        print("\nno blank renders", flush=True)


main()