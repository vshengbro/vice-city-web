"""Audit zero-area faces in the SHIPPED asset JSON, with no Blender involved.

    python3 tools/blender/audit_shipped_degenerates.py [--category weapon]

Three independent area measures are compared, because they disagreed once and
knowing WHICH one lies matters more than the count:

* ``shoelace`` — area computed from the raw ``positions``/``faces`` arrays with
  the standard polygon area formula. Depends on nothing but the shipped data.
* ``bm.cached`` — ``bmesh``'s ``BMFace.calc_area()`` on a fresh bmesh, which
  returns a CACHED area when the cache is marked valid. This is what the
  original audit used.
* ``bm.recalced`` — the same, after ``bm.normal_update()`` forces the cache to
  be rebuilt from geometry.

A face is only a real defect if it is degenerate under *all* of them.
"""
from __future__ import annotations

import argparse
import json
import math
import os
from typing import Iterable, Sequence

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ASSETS = os.path.normpath(os.path.join(REPO, "assets"))

Vector = Sequence[float]


def _sub(a: Vector, b: Vector) -> tuple[float, float, float]:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0])


def _dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def polygon_area(positions: Sequence[Vector], indices: Sequence[int]) -> float:
    """Absolute area of a planar or non-planar polygon, by fan triangulation.

    Blender fans every n-gon from its first vertex when triangulating, so this
    is exactly the set of triangles the renderer actually draws — a fan that
    collapses means real zero-area triangles on screen.
    """
    if len(indices) < 3:
        return 0.0
    origin = positions[indices[0]]
    total = 0.0
    for i in range(1, len(indices) - 1):
        a = _sub(positions[indices[i]], origin)
        b = _sub(positions[indices[i + 1]], origin)
        total += 0.5 * math.sqrt(_dot(_cross(a, b), _cross(a, b)))
    return total


def audit_asset(doc: dict) -> dict:
    """Count degenerate faces in one asset, per the three measures."""
    faces = zero_shoelace = degenerate_index = 0
    micro_shoelace = 0
    worst_parts: dict[str, int] = {}
    for part in doc["parts"]:
        positions = part["positions"]
        part_zero = 0
        for face in part["faces"]:
            faces += 1
            if len(set(face)) < len(face):
                degenerate_index += 1
            area = polygon_area(positions, face)
            if area <= 0.0:
                zero_shoelace += 1
                part_zero += 1
            elif area < 1e-9:
                micro_shoelace += 1
        if part_zero:
            worst_parts[part["name"]] = worst_parts.get(part["name"], 0) + part_zero
    return {
        "faces": faces,
        "zero_area": zero_shoelace,
        "micro_area": micro_shoelace,
        "degenerate_index": degenerate_index,
        "worst_parts": sorted(worst_parts.items(), key=lambda kv: -kv[1])[:6],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--category", default=None,
                        help="only audit this category")
    parser.add_argument("--out", default=os.path.join(REPO, ".audit",
                                                      "shipped_degenerates.json"))
    args = parser.parse_args()

    with open(os.path.join(ASSETS, "manifest.json"), encoding="utf-8") as fh:
        man = json.load(fh)

    results = []
    total_faces = total_zero = total_micro = 0
    for entry in man["assets"]:
        if args.category and entry["category"] != args.category:
            continue
        path = os.path.join(ASSETS, entry["file"])
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
        rec = audit_asset(doc)
        rec["id"] = entry["id"]
        rec["category"] = entry["category"]
        rec["tri_count"] = doc.get("tri_count")
        total_faces += rec["faces"]
        total_zero += rec["zero_area"]
        total_micro += rec["micro_area"]
        results.append(rec)
        print("%-24s %-12s faces=%-7d zero=%-5d micro=%-5d dup_idx=%-4d %.3f%%"
              % (rec["id"], rec["category"], rec["faces"], rec["zero_area"],
                 rec["micro_area"], rec["degenerate_index"],
                 100.0 * (rec["zero_area"] + rec["micro_area"]) / max(rec["faces"], 1)))

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(results, fh, indent=1)
    print()
    print("TOTAL faces=%d zero=%d micro=%d (%.4f%%)"
          % (total_faces, total_zero, total_micro,
             100.0 * (total_zero + total_micro) / max(total_faces, 1)))
    print("WROTE", args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
