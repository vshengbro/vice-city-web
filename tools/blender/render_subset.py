"""Render only a subset of an asset's parts, to attribute render artefacts.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/render_subset.py -- <asset_id> <keep_substr> [...]
"""
import os
import sys

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402

import render_previews3a as R  # noqa: E402

OUT = "/Users/sqs/.hermes/cache/scratch/art-fix/ab"


def main():
    from vcw_bpy.registry import all_assets

    args = sys.argv[sys.argv.index("--") + 1:]
    aid = args[0]
    keep = args[1:]
    os.makedirs(OUT, exist_ok=True)

    fn, kw = {a: (f, k) for a, _c, f, k in all_assets()}[aid]
    parts = [p for p in fn(aid, **kw) if p is not None]
    # Capture every NAME first: once an object is deleted its StructRNA raises
    # on any attribute access, including `.name`, so a lazy generator over
    # `parts` after the deletions dies with ReferenceError.
    all_names = [o.name for o in parts if o.type == "MESH"]
    keep_names = [n for n in all_names if any(k in n for k in keep)]
    for name in all_names:
        if name in keep_names:
            continue
        obj = bpy.data.objects.get(name)
        if obj is not None:
            bpy.data.objects.remove(obj, do_unlink=True)
    live = [bpy.data.objects[n] for n in keep_names]
    tag = aid + "_" + ("-".join(keep) if keep else "all")
    res = R.render_one(tag, live, OUT)
    print("RENDERED", tag, "parts=", len(live), "std=", res.get("pixel_std"),
          res.get("path"))


if __name__ == "__main__":
    main()
