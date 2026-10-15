"""True vertex-proximity test: which parts of an asset float free?

AABB overlap is the wrong test for a radially-symmetric assembly — a bolt ring
around a tube overlaps nothing in its AABB yet touches the tube. This walks
actual vertices against actual vertices via a kd-tree, so "detached" means
"no vertex is within `tol` of any vertex of any other part".

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/find_floating_parts.py -- <asset_id>...
"""
import os
import sys

REPO = "/Users/sqs/code/vice-city-web"
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402
from mathutils import kdtree  # noqa: E402
from mathutils import Vector  # noqa: E402

TOL = 5e-4  # 0.5 mm — parts are authored to overlap by 5-10 mm by contract


def part_points(obj):
    return [obj.matrix_world @ v.co for v in obj.data.vertices]


def build_tree(points):
    tree = kdtree.KDTree(len(points))
    for i, p in enumerate(points):
        tree.insert(p, i)
    tree.balance()
    return tree


def floating_parts(parts, tol=TOL):
    """Return [(gap, name, nverts, nearest_name)] for genuinely detached parts."""
    meshes = [o for o in parts if o is not None and o.type == "MESH" and o.data.vertices]
    pts = {o.name: part_points(o) for o in meshes}
    trees = {o.name: build_tree(pts[o.name]) for o in meshes}
    lo = {o.name: [min(p[k] for p in pts[o.name]) for k in range(3)] for o in meshes}
    hi = {o.name: [max(p[k] for p in pts[o.name]) for k in range(3)] for o in meshes}

    out = []
    for o in meshes:
        best = float("inf")
        best_name = None
        for m in meshes:
            if m.name == o.name:
                continue
            # bbox reject: a lower bound on the true vertex distance
            lb = max(max(lo[m.name][k] - hi[o.name][k],
                         lo[o.name][k] - hi[m.name][k]) for k in range(3))
            if lb >= best:
                continue
            # Take the NEAREST pair over ALL of o's vertices. Testing only the
            # first vertex reports a bolt ring as "52 mm detached" purely
            # because vertex 0 happens to point away from the tube.
            for p in pts[o.name]:
                _co, _idx, dist = trees[m.name].find(p)
                if dist < best:
                    best = dist
                    best_name = m.name
                if best <= tol:
                    break
            if best <= tol:
                break
        if best > tol:
            out.append((best, o.name, len(o.data.vertices), best_name))
    out.sort(reverse=True)
    return out


def main():
    from vcw_bpy.registry import all_assets

    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    entries = {a: (f, k) for a, _c, f, k in all_assets()}
    if not args:
        args = sorted(entries)
    total_bad = 0
    for aid in args:
        fn, kw = entries[aid]
        fn(aid, **kw)
        bpy.context.view_layer.update()
        parts = [o for o in bpy.data.objects if o.type == "MESH"]
        bad = floating_parts(parts)
        total_bad += len(bad)
        if bad:
            print("%-22s %d/%d parts detached" % (aid, len(bad), len(parts)))
            for gap, name, nv, near in bad[:8]:
                print("    gap=%.5f m  %-34s verts=%-5d nearest=%s"
                      % (gap, name, nv, near))
        else:
            print("%-22s all %d parts connected" % (aid, len(parts)))
        # each asset builds in its own scene
        for ob in list(bpy.data.objects):
            bpy.data.objects.remove(ob, do_unlink=True)
    print("TOTAL_DETACHED_PARTS=%d" % total_bad)


if __name__ == "__main__":
    main()
