"""Measure the exact 3-D clearance between adjacent sign letters.

`audit_sign_glyph_separation.py` counts rendered connected components, which
is a noisy proxy: a glyph like "O" can split into two components that share
the same x-range, so "adjacent boxes overlap" does not imply the letters
touch. The exact question is whether two NEIGHBOURING CHARACTERS' tubes
intersect in 3-D, and that is answerable directly from the geometry with no
render at all.

For every sign, group its stroke tubes by character index, take each group's
world AABB, and report the smallest horizontal gap between consecutive
groups. A gap > 0 means the letters are physically separate; a gap <= 0
means the tubes of adjacent letters interpenetrate and the word reads as one
fused stroke.

    /Applications/Blender.app/Contents/MacOS/Blender --background \
        --factory-startup --python tools/blender/audit_sign_letter_clearance.py
"""
from __future__ import annotations

import json
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO, "tools", "blender"))

import bpy  # noqa: E402

from vcw_bpy.registry import all_assets  # noqa: E402

CHAR_RE = re.compile(r"_(main|sub)_c(\d+)s\d+$")


def group_boxes(parts, prefix: str):
    """World AABB per character index for one text line."""
    groups: dict[int, list] = {}
    for obj in parts:
        if obj is None or obj.type != "MESH":
            continue
        m = CHAR_RE.search(obj.name)
        if not m or f"{prefix}_" not in obj.name:
            continue
        idx = int(m.group(2))
        lo = [1e18] * 3
        hi = [-1e18] * 3
        for v in obj.data.vertices:
            w = obj.matrix_world @ v.co
            for i in range(3):
                lo[i] = min(lo[i], w[i])
                hi[i] = max(hi[i], w[i])
        groups.setdefault(idx, []).append((lo, hi))
    boxes = []
    for idx in sorted(groups):
        lo = [min(b[0][i] for b in groups[idx]) for i in range(3)]
        hi = [max(b[1][i] for b in groups[idx]) for i in range(3)]
        boxes.append((idx, lo, hi))
    return boxes


def clearances(boxes):
    """Horizontal gap between consecutive character boxes, left to right."""
    out = []
    ordered = sorted(boxes, key=lambda b: b[1][0])
    for i in range(len(ordered) - 1):
        # gap in X from this char's right edge to the next char's left edge
        gap = ordered[i + 1][1][0] - ordered[i][2][0]
        out.append((ordered[i][0], ordered[i + 1][0], round(gap, 5)))
    return out


def main() -> int:
    entries = all_assets()
    report = []
    worst_overall = None
    for aid, category, fn, kw in entries:
        if category != "sign":
            continue
        parts = fn(aid, **kw)
        row = {"id": aid}
        for line in ("main", "sub"):
            boxes = group_boxes(parts, line)
            gaps = clearances(boxes)
            row[f"{line}_glyphs"] = len(boxes)
            row[f"{line}_min_gap_m"] = round(min((g[2] for g in gaps), default=0.0), 5)
            row[f"{line}_touching"] = sum(1 for g in gaps if g[2] <= 0.0)
            if line == "main" and gaps:
                worst = min(gaps, key=lambda g: g[2])
                row["main_worst_pair"] = worst[:2]
        report.append(row)
        print("%-14s main glyphs=%-3d touching=%-3d min_gap=%+.4fm | "
              "sub glyphs=%-3d touching=%-3d min_gap=%+.4fm"
              % (aid, row["main_glyphs"], row["main_touching"],
                 row["main_min_gap_m"], row["sub_glyphs"], row["sub_touching"],
                 row["sub_min_gap_m"]))
        if worst_overall is None or row["main_min_gap_m"] < worst_overall:
            worst_overall = row["main_min_gap_m"]

    os.makedirs(os.path.join(REPO, ".audit"), exist_ok=True)
    with open(os.path.join(REPO, ".audit", "sign_letter_clearance.json"), "w",
              encoding="utf-8") as fh:
        json.dump(report, fh, indent=1)
    total_touch = sum(r["main_touching"] + r["sub_touching"] for r in report)
    print()
    print("smallest main-line gap across all signs: %+.5f m" % worst_overall)
    print("letter pairs whose tubes interpenetrate: %d" % total_touch)
    print("SIGN_CLEARANCE_OK" if total_touch == 0 else "SIGN_CLEARANCE_FUSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
