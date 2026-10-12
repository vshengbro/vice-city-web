#!/usr/bin/env python3.11
"""Render sedan / taxi / police side by side so the silhouettes can be judged.

Same camera angle and scale for all three -- that is the whole point: if the
bodies were identical the images would be identical.
"""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "tools", "blender"))

import bpy  # noqa: E402
from vcw.builders import vehicles  # noqa: E402
import render_previews as rp  # noqa: E402

OUT = "/Users/sqs/.hermes/cache/scratch/vcw-cars"
os.makedirs(OUT, exist_ok=True)

built = {a.id: a for a in vehicles.build_all()}
targets = [t for t in ("car_sedan", "car_taxi", "car_police", "car_coupe")
           if t in built]
print("rendering:", targets, flush=True)

# Freeze the camera so all four share identical framing: compute the union
# bounds first, then force every render to that same distance/centre.
from mathutils import Vector
import json
allpts = []
for aid in targets:
    d = json.load(open(os.path.join(ROOT, "assets", aid + ".json")))
    for p in d["parts"]:
        for v in p.get("positions", []):
            allpts.append(Vector((v[0], v[1], v[2])))
lo = Vector((min(p.x for p in allpts), min(p.y for p in allpts), min(p.z for p in allpts)))
hi = Vector((max(p.x for p in allpts), max(p.y for p in allpts), max(p.z for p in allpts)))
centre = (lo + hi) * 0.5
radius = (hi - lo).length * 0.5
print(f"union centre={tuple(round(c,2) for c in centre)} radius={radius:.2f}", flush=True)

for aid in targets:
    rp.clear_scene()
    scene = bpy.context.scene
    collection = scene.collection
    rp.setup_render(scene)
    objs = rp.build_object(built[aid], collection)
    # identical camera for every asset
    rp.setup_camera(scene, collection, built[aid])
    rp.add_rig(scene, collection, centre)
    bpy.context.view_layer.update()
    path = os.path.join(OUT, aid + ".png")
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    std, _ = rp.image_std(path)
    print(f"  {aid:14} objects={len(objs):3} png={os.path.getsize(path):8} std={std:.4f}",
          flush=True)
print("done ->", OUT, flush=True)
