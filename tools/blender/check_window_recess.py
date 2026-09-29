"""Measure whether building windows are actually RECESSED behind the wall.

Run from tools/blender/ (or anywhere -- paths are absolute):

    python3 tools/blender/check_window_recess.py

A recess is the difference between the wall plane and the glass plane.  It is
the single detail that stops an Art Deco facade reading as a flat sticker, and
it is easy to get backwards: `sgn = -1` names the street facade, so an inset of
`GLASS_SET` metres has to move the pane TOWARD the building centre, not away
from it.  This probe exists because that sign error survived a code review and
a render.

Reads the built mesh in Blender's own frame (Y up, front facade at min Y) and
reports the recess in millimetres per asset.  Exits non-zero if any facade is
flush or proud.
"""
from __future__ import annotations

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

# GLASS_SET in builders.py.  A 15 mm reveal is shallow in absolute terms but
# reads clearly because the frame ring stands 90 mm PROUD of the wall, giving
# a 75 mm frame-to-glass step that the single directional light can catch.
# The floor here only guards against a total collapse back to a flush decal.
MIN_RECESS_M = 0.010  # 10 mm

FRONT_FACADE_PART = "shaft"
GLASS_PARTS = ("windows", "window_lit")
FRAME_PARTS = ("window_frames", "window_frames_rear")


def _planes(part, axis=1):
    return {round(v[axis], 4) for v in part.mesh.pos}


def main() -> int:
    from vcw.builders import buildings

    print("%-24s %6s %9s %9s %9s %9s %s"
          % ("asset", "tris", "wall -Y", "glass -Y", "frame -Y", "recess", "verdict"))
    print("-" * 84)

    failures = []
    steps = []
    total = 0
    for asset in buildings.build_all():
        tris = sum(len(p.mesh.faces) for p in asset.parts)
        total += tris

        by_name = {p.name: p for p in asset.parts}
        wall_part = by_name.get(FRONT_FACADE_PART)
        if wall_part is None:
            continue

        # The front facade is the most-negative-Y plane of the wall volume.
        wall = min(_planes(wall_part))

        glass = [by_name[n] for n in GLASS_PARTS if n in by_name]
        if not glass:
            continue
        glass_y = min(min(_planes(p)) for p in glass)

        frames = [by_name[n] for n in FRAME_PARTS if n in by_name]
        frame_y = min(min(_planes(p)) for p in frames) if frames else float("nan")

        # Recessed means the glass plane sits at a LARGER y (further inside,
        # away from the street) than the wall plane.  In the front-wall
        # direction the wall is the most-negative y, so "inside" is UP in y:
        # glass_y > wall_y.  Proud glass -- a decal -- gives glass_y < wall_y.
        recess = glass_y - wall
        ok = recess >= MIN_RECESS_M
        if not ok:
            failures.append((asset.id, recess))

        print("%-24s %6d %9.4f %9.4f %9.4f %8.1fmm %s"
              % (asset.id, tris, wall, glass_y, frame_y, recess * 1000,
                 "recessed" if ok else "*** FLUSH/PROUD ***"))
        if ok and frames:
            # The step the eye actually reads: frame face -> glass face.
            step = glass_y - frame_y
            steps.append((asset.id, step))

    print("-" * 84)
    print("buildings total triangles: %d" % total)
    if steps:
        lo = min(s for _, s in steps)
        hi = max(s for _, s in steps)
        print("frame->glass step: %.1f..%.1f mm (this is the edge the light catches)"
              % (lo * 1000, hi * 1000))
    if failures:
        print("\nFAIL: %d/%d facades are not recessed:"
              % (len(failures), len(failures)))
        for aid, r in failures:
            print("  %-24s recess %+.1f mm" % (aid, r * 1000))
        return 1
    print("\nOK: every front facade has its glass set back behind the wall.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
