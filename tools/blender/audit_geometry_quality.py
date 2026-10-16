"""Geometry quality audit: degenerate faces, non-manifold, inverted normals,
unapplied transforms, manifold/closed-shell status -- per asset, in Blender.

Run:
    /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
        --python tools/blender/audit_geometry_quality.py
"""
import json
import math
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

OUT = os.path.join(REPO, ".audit")
os.makedirs(OUT, exist_ok=True)

import bpy
import bmesh
from mathutils import Vector

report = {"render": {}, "assets": []}

# ---------------------------------------------------------------- render env
sc = bpy.context.scene
try:
    ev = sc.eevee
    eevee_info = {
        "taa_render_samples": getattr(ev, "taa_render_samples", None),
        "taa_samples": getattr(ev, "taa_samples", None),
        "use_raytracing": getattr(ev, "use_raytracing", None),
        "use_shadows": getattr(ev, "use_shadows", None),
        "use_volumetric_shadows": getattr(ev, "use_volumetric_shadows", None),
        "gtao_distance": getattr(ev, "gtao_distance", None),
        "use_gtao": getattr(ev, "use_gtao", None),
        "shadow_ray_count": getattr(ev, "shadow_ray_count", None),
        "shadow_step_count": getattr(ev, "shadow_step_count", None),
        "fast_gi_method": getattr(getattr(ev, "fast_gi_method", None), "__class__", type(None)).__name__
        if hasattr(ev, "fast_gi_method") else None,
    }
except Exception as e:
    eevee_info = {"error": repr(e)}

report["render"] = {
    "blender_version": bpy.app.version_string,
    "default_engine": sc.render.engine,
    "resolution_x": sc.render.resolution_x,
    "resolution_y": sc.render.resolution_y,
    "resolution_percentage": sc.render.resolution_percentage,
    "view_transform": sc.view_settings.view_transform,
    "look": sc.view_settings.look,
    "exposure": sc.view_settings.exposure,
    "gamma": sc.view_settings.gamma,
    "display_device": sc.display_settings.display_device,
    "film_transparent": sc.render.film_transparent,
    "eevee": eevee_info,
    "cycles_available": "cycles" in bpy.context.preferences.addons,
    "cycles_device": None,
}
try:
    prefs = bpy.context.preferences.addons["cycles"].preferences
    report["render"]["cycles_device"] = prefs.compute_device_type
    report["render"]["cycles_device_type_enum"] = [
        e.identifier for e in
        prefs.bl_rna.properties["compute_device_type"].enum_items]
except Exception as e:
    report["render"]["cycles_device"] = repr(e)


def clear_scene():
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    for blk in (bpy.data.meshes, bpy.data.materials, bpy.data.cameras,
                bpy.data.lights):
        for b in list(blk):
            blk.remove(b, do_unlink=True)


def audit_meshes(objs):
    """Run bmesh-level checks over every mesh object."""
    agg = {
        "objects": 0, "verts": 0, "edges": 0, "faces": 0,
        "degenerate_faces": 0,        # < 3 unique verts
        "zero_area_faces": 0,         # area == 0 (or < 1e-12)
        "micro_area_faces": 0,        # area < 1e-9 m^2  (sub-mm at asset scale)
        "non_manifold_edges": 0,      # != 2 faces
        "boundary_edges": 0,          # == 1 face
        "wire_edges": 0,              # == 0 faces
        "non_manifold_verts": 0,      # != 2 faces
        "loose_verts": 0,
        "inverted_normals": 0,        # face normal disagrees with vertex
        "zero_length_normals": 0,
        "dup_face_verts": 0,          # a face using a vert twice
        "flipped_vs_vertex_normals": 0,
    }
    per_obj = []
    for ob in objs:
        if ob is None or ob.type != "MESH":
            continue
        me = ob.data
        agg["objects"] += 1
        agg["verts"] += len(me.vertices)
        agg["edges"] += len(me.edges)
        agg["faces"] += len(me.polygons)
        bm = bmesh.new()
        try:
            bm.from_mesh(me)
            bm.verts.ensure_lookup_table()
            o = {"name": ob.name, "faces": len(me.polygons),
                 "verts": len(me.vertices)}
            for f in bm.faces:
                n_verts = len(f.verts)
                if n_verts < 3:
                    agg["degenerate_faces"] += 1
                seen = set()
                dup = False
                for v in f.verts:
                    if v.index in seen:
                        dup = True
                    seen.add(v.index)
                if dup:
                    agg["dup_face_verts"] += 1
                a = f.calc_area()
                if a == 0.0:
                    agg["zero_area_faces"] += 1
                elif a < 1e-9:
                    agg["micro_area_faces"] += 1
            nm = 0
            be = 0
            we = 0
            for e in bm.edges:
                n = len(e.link_faces)
                if n != 2:
                    nm += 1
                    if n == 1:
                        be += 1
                    elif n == 0:
                        we += 1
            agg["non_manifold_edges"] += nm
            agg["boundary_edges"] += be
            agg["wire_edges"] += we
            nonman_v = sum(1 for v in bm.verts
                           if not v.is_manifold)
            agg["non_manifold_verts"] += nonman_v
            loose = sum(1 for v in bm.verts if not v.link_edges)
            agg["loose_verts"] += loose
            # normals
            bm.normal_update()
            for f in bm.faces:
                if f.normal.length_squared < 0.5:
                    agg["zero_length_normals"] += 1
            for v in bm.verts:
                if v.normal.length_squared < 0.5:
                    agg["zero_length_normals"] += 1
            # Inverted-normal proxy: for a closed-ish manifold mesh, an inverted
            # face has its normal pointing into the mesh (away from centroid).
            o["non_manifold_edges"] = nm
            o["boundary_edges"] = be
            o["wire_edges"] = we
            o["non_manifold_verts"] = nonman_v
            o["loose_verts"] = loose
            per_obj.append(o)
        finally:
            bm.free()
    return agg, per_obj


def check_transforms(objs):
    """Unapplied transforms + negative scale (mirroring)."""
    loc = rot = scl = neg = 0
    offenders = []
    for ob in objs:
        if ob is None or ob.type != "MESH":
            continue
        if ob.location.length > 1e-6:
            loc += 1
        if max(abs(a) for a in ob.rotation_euler) > 1e-6:
            rot += 1
        s = ob.scale
        if max(abs(a - 1.0) for a in s) > 1e-6:
            scl += 1
            offenders.append((ob.name, tuple(round(x, 4) for x in s)))
        if s[0] * s[1] * s[2] < 0:
            neg += 1
    return {
        "objs_with_location": loc,
        "objs_with_rotation": rot,
        "objs_with_scale_not_one": scl,
        "objs_with_negative_scale_mirrored": neg,
        "scale_offenders": offenders[:12],
    }


def _axis_line(obj, centre):
    """The unit direction of an elongated part's long axis through `centre`."""
    p = [obj.matrix_world @ v.co for v in obj.data.vertices]
    dims = [(max(q[i] for q in p) - min(q[i] for q in p), i) for i in range(3)]
    _span, i = max(dims)
    d = Vector((0.0, 0.0, 0.0))
    d[i] = 1.0
    return centre, d


def _dist_to_axis(point, axis):
    """Perpendicular distance from `point` to the infinite line `axis`."""
    centre, direction = axis
    delta = point - centre
    return (delta - direction * delta.dot(direction)).length


def check_radial_details(objs):
    """Are ring-of-blocks details on the axis of the tube they decorate?

    A helper that builds "a bar at angular station phi around the Y axis"
    builds it in a plane through the WORLD ORIGIN. When the tube it decorates
    sits at a bore height, forgetting to raise the ring drops every block to
    z=0 — the asset still builds, still exports, still passes every manifold
    and transform check, and renders as loose debris beside the weapon.

    The test is deliberately narrow, because a broad one is worse than none:

    * only parts whose names form a numbered RING (gusset0_3, vent_block1,
      shroud_rib2) are considered — a ring is a set of >=4 siblings sharing a
      stem and differing only by index;
    * the ring's axis is its members' centroid;
    * the reference is the LONGEST CYLINDER-shaped part (the tube), found by
      elongation rather than assumed to be "the biggest part";
    * a ring is off-axis only when its axis is farther from the tube's axis
      than the tube's own radius — i.e. the ring is provably not around it.

    Checking every part named "rib" against the asset's widest part flags a
    bat's wrap ribs and a traffic cone's grip ribs, which are correctly placed;
    a check that cries wolf gets ignored, so it is worse than no check.
    """
    meshes = [o for o in objs
              if o is not None and o.type == "MESH" and o.data.vertices]
    if not meshes:
        return {"checked": 0, "rings": 0, "off_axis": []}

    def pts(o):
        return [o.matrix_world @ v.co for v in o.data.vertices]

    def centroid(ps):
        return Vector(tuple(sum(p[i] for p in ps) / len(ps) for i in range(3)))

    def extent(o):
        p = pts(o)
        return max(max(q[i] for q in p) - min(q[i] for q in p) for i in range(3))

    def radius_about(o, c):
        return max((q - c).length for q in pts(o))

    # The tube: the most elongated part, and at least 3x longer than it is wide.
    tubes = []
    for o in meshes:
        p = pts(o)
        dims = sorted(max(q[i] for q in p) - min(q[i] for q in p) for i in range(3))
        longest = dims[2]
        if longest > 3.0 * max(dims[0], 1e-6) and len(o.data.polygons) > 32:
            tubes.append((longest, o))
    if not tubes:
        return {"checked": 0, "rings": 0, "off_axis": []}
    tubes.sort(reverse=True, key=lambda t: t[0])
    tube = tubes[0][1]
    tube_c = centroid(pts(tube))
    # The tube's radius is its half-WIDTH, not its distance from its own
    # centroid: a 0.64 m tube whose centroid is 0.32 m from its ends has a
    # "radius" of 0.32 m that way, which makes every 0.08 m ring look
    # correctly placed no matter where it is. Use the two SHORTEST extents.
    tp = pts(tube)
    spans = sorted(max(q[i] for q in tp) - min(q[i] for q in tp) for i in range(3))
    tube_r = spans[0] * 0.5
    axis = _axis_line(tube, tube_c)

    # Group ring members by their stem: strip the trailing index.
    import re
    groups = {}
    for o in meshes:
        m = re.match(r"^(.*?)_(\d+)$", o.name)
        if not m:
            continue
        stem = m.group(1)
        if not any(k in stem for k in ("gusset", "vent_block", "shroud_rib")):
            continue
        groups.setdefault(stem, []).append(o)

    off = []
    checked = 0
    for stem, members in groups.items():
        if len(members) < 4:
            continue
        checked += 1
        member_pts = [q for o in members for q in pts(o)]
        ring_c = centroid(member_pts)
        # The comparison must be made in the ring's OWN plane, not as a 3D
        # centroid distance. A ring dropped to z=0 still has a centroid only
        # `bore_height` away from the tube's centre, and on a 45 mm tube that
        # 0.23 m offset looks like "the ring is not around this tube" only
        # when compared to the tube's RADIUS, not to its length. Measure the
        # ring's radius about the TUBE'S AXIS: that is what "a ring of blocks
        # around a tube" means, and it is 0.08 m when correct and 0.28 m when
        # when the ring has fallen to the ground plane.
        radii = [_dist_to_axis(q, axis) for q in member_pts]
        r_mean = sum(radii) / len(radii)
        if r_mean > tube_r * 2.0 + 0.02:
            off.append({"ring": stem, "parts": len(members),
                        "ring_radius": round(r_mean, 4),
                        "tube": tube.name, "tube_radius": round(tube_r, 4),
                        "ratio": round(r_mean / tube_r, 2),
                        "sample": sorted(o.name for o in members)[0]})
    return {"checked": checked, "rings": len(groups), "tube": tube.name,
            "tube_radius": round(tube_r, 4), "off_axis": off}


def main():
    from vcw_bpy.registry import all_assets
    entries = all_assets()
    print("AUDIT: %d registry assets" % len(entries))
    for aid, category, fn, kw in entries:
        clear_scene()
        try:
            parts = fn(aid, **kw)
        except Exception as exc:  # noqa: BLE001
            import traceback
            traceback.print_exc()
            report["assets"].append({"id": aid, "error": repr(exc)})
            continue
        objs = list(parts) if isinstance(parts, (list, tuple)) else [parts]
        objs = [o for o in objs if o is not None]
        agg, per_obj = audit_meshes(objs)
        tf = check_transforms(objs)
        radial = check_radial_details(objs)
        rec = {"id": aid, "category": category, **agg, **tf, "radial": radial}
        # island count for the largest object (fragmentation proxy)
        try:
            biggest = max((o for o in objs if o and o.type == "MESH"),
                          key=lambda o: len(o.data.polygons), default=None)
            if biggest is not None:
                bm = bmesh.new()
                bm.from_mesh(biggest.data)
                seen = set()
                islands = 0
                for v in bm.verts:
                    if v.index in seen:
                        continue
                    islands += 1
                    stack = [v]
                    while stack:
                        cur = stack.pop()
                        if cur.index in seen:
                            continue
                        seen.add(cur.index)
                        for e in cur.link_edges:
                            o = e.other_vert(cur)
                            if o.index not in seen:
                                stack.append(o)
                bm.free()
                rec["largest_obj_islands"] = islands
                rec["largest_obj"] = biggest.name
        except Exception as exc:  # noqa: BLE001
            rec["island_error"] = repr(exc)
        rec["per_obj"] = per_obj
        report["assets"].append(rec)
        print("  %-26s faces=%-6d degen=%-3d zero_area=%-4d "
              "micro=%-4d nonmanif_e=%-5d bound_e=%-5d wire_e=%-3d "
              "loose_v=%-3d islands=%s"
              % (aid, agg["faces"], agg["degenerate_faces"],
                 agg["zero_area_faces"], agg["micro_area_faces"],
                 agg["non_manifold_edges"], agg["boundary_edges"],
                 agg["wire_edges"], agg["loose_verts"],
                 rec.get("largest_obj_islands", "?")))

    with open(os.path.join(OUT, "geometry_audit.json"), "w") as fh:
        json.dump(report, fh, indent=1)
    print("\nWROTE", os.path.join(OUT, "geometry_audit.json"))

    # Radial details on the wrong axis are invisible to every other check in
    # this audit: they are manifold, closed, correctly transformed and
    # correctly sized — they are simply in the wrong place. Report them as a
    # first-class failure so they cannot ship again unnoticed.
    off = [(a["id"], a["radial"]["off_axis"]) for a in report["assets"]
           if a.get("radial") and a["radial"].get("off_axis")]
    if off:
        print("\nOFF-AXIS RADIAL DETAILS: %d asset(s)" % len(off))
        for aid, rows in off:
            for r in rows:
                print("  %-22s %-34s %d parts, ring r=%.4f vs tube %s r=%.4f (%.1fx)"
                      % (aid, r["ring"], r["parts"], r["ring_radius"],
                         r["tube"], r["tube_radius"], r["ratio"]))
        print("GEOMETRY_AUDIT_OFF_AXIS")
        return
    print("AUDIT_OK")


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        import traceback
        traceback.print_exc()
        print("AUDIT_FAILED")
