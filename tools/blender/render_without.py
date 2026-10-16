"""Render an asset with named parts suppressed, to prove what they looked like.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/render_without.py -- <asset_id> <substr> [...]
"""
import os
import sys

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402

import render_previews3a as R  # noqa: E402


def main():
    from vcw_bpy.registry import all_assets

    args = sys.argv[sys.argv.index("--") + 1:]
    aid, drop = args[0], args[1:]
    out = os.path.join("/Users/sqs/.hermes/cache/scratch/art-fix/ab",
                       f"{aid}_without_{'-'.join(drop)}")
    os.makedirs(out, exist_ok=True)

    fn, kw = {a: (f, k) for a, _c, f, k in all_assets()}[aid]
    parts = fn(aid, **kw)
    for o in parts:
        if o is None or o.type != "MESH":
            continue
        if any(d in o.name for d in drop):
            bpy.data.objects.remove(o, do_unlink=True)
    res = R.render_one(aid, [o for o in parts if o and o.name in bpy.data.objects], out)
    print("RENDERED", res["path"], "std=", res["pixel_std"])


if __name__ == "__main__":
    main()
