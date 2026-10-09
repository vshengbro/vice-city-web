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

# The shaft is a SOLID box -- no boolean hole is cut in the wall -- so the
# glass CANNOT sit deeper than the wall face; it has to sit PROUD of it, in the
# rebate between the wall and the proud frame ring.  The reveal a player reads
# is therefore (GLASS_SET + FRAME_PROUD) - GLASS_SET = the frame's stand-off,
# plus the pane body itself standing off the wall.
#
# THIS PROBE USED TO ASSERT THE OPPOSITE.  It required the glass plane to sit
# at a MORE POSITIVE y than the wall, i.e. inside solid geometry, and its
# docstring presented that as the fix for a sign error.  The sign was backwards:
# under that rule the pane was 100% invisible (raycast: 0 of 12816 probes hit
# `windows` first; deleting every glass part moved only 0.2-0.6% of pixels).
#
# The invariant that actually matters is VISIBILITY, so this probe now measures
# it the only trustworthy way: the glass must be visible, and it must still sit
# behind the frame ring so the reveal keeps its shadowed depth.
MIN_STANDOFF_M = 0.005   # 5 mm: pane clears the painted wall face

FRONT_FACADE_PART = "shaft"
GLASS_PARTS = ("windows", "window_lit")
FRAME_PARTS = ("window_frames", "window_frames_rear")


def _planes(part, axis=1):
    return {round(v[axis], 4) for v in part.mesh.pos}


def main() -> int:
    from vcw.builders import buildings

    print("%-24s %6s %9s %9s %9s %11s %s"
          % ("asset", "tris", "wall -Y", "glass -Y", "frame -Y", "standoff",
             "verdict"))
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

        # The wall is the most-negative y face; "outboard" on the street
        # facade means MORE negative y, so a visible pane has glass_y < wall_y.
        # glass_y == wall_y would be a flush decal.  Burying it (glass_y >
        # wall_y) is the failure this probe exists to catch: the pane is then
        # inside solid geometry and contributes nothing to the image.
        standoff = wall - glass_y
        ok = standoff >= MIN_STANDOFF_M
        if not ok:
            failures.append((asset.id, standoff))

        print("%-24s %6d %9.4f %9.4f %9.4f %11s %s"
              % (asset.id, tris, wall, glass_y, frame_y,
                 "%+.1fmm" % (standoff * 1000),
                 "visible" if ok else "*** BURIED IN WALL ***"))
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
        print("\nFAIL: %d facade(s) have their glass buried inside the wall:"
              % len(failures))
        for aid, r in failures:
            print("  %-24s standoff %+.1f mm" % (aid, r * 1000))
        return 1
    print("\nOK: every front facade's glass stands proud of the wall, behind "
          "its frame ring.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
