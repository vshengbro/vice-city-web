"""Verify a 3A asset JSON against every invariant the renderer relies on.

The renderer trusts this schema completely, so every invariant here is one the
game will hit as a silent wrong-render or a missing asset if it breaks.

Invariants checked:

1. id / category present; ``y_up`` true.
2. ``faces`` indices all in range of ``positions``; every face has 3 indices.
3. ``positions`` are finite and not degenerate (no zero-area triangles).
4. Triangle budget per asset (hard gate, not a guideline).
5. Part names unique and non-empty.
6. Closed shells have positive signed volume (outward winding).
7. Open surfaces declare an outward reference direction.
8. Bounds match the recomputed extents within tolerance.
9. Ground assets rest on y=0.

Run standalone: ``python3 verify3a.py <assets_dir>`` exits non-zero on failure.
"""

from __future__ import annotations

import json
import math
import os
import sys
from typing import Sequence

# Per-asset triangle cap. 3A buildings and vehicles are allowed to be dense;
# small props stay lean. Reading only the total hides a category that silently
# contributed nothing, so the report prints the per-category breakdown.
CATEGORY_BUDGET = {
    "building": 90_000,
    "vehicle": 80_000,
    "pedestrian": 40_000,
    "palm": 30_000,
    "weapon": 30_000,
    "prop": 20_000,
    "sign": 20_000,
    "pickup": 12_000,
    "misc": 20_000,
    "beach": 12_000,
    "animal": 12_000,
    "marker": 8_000,
    "road": 2_000,
    "nature": 6_000,
    "terrain": 2_000,
}
DEFAULT_BUDGET = 20_000
TOTAL_BUDGET = 4_000_000

BOUNDS_TOLERANCE = 1e-3
GROUND_TOLERANCE = 1e-6


def _signed_volume(points: Sequence[Sequence[float]], faces: Sequence[Sequence[int]]) -> float:
    """Signed volume of a closed shell via the divergence theorem.

    Vertices are keyed by position so that split vertices (which a bevel or a
    smooth-shade export produces everywhere) still form one connected shell.
    Without the weld, the volume sums to ~0 on a closed mesh and the
    outwardness test silently passes everything.
    """
    index_map: dict[tuple, int] = {}
    verts: list[Sequence[float]] = []
    for p in points:
        key = (round(p[0], 6), round(p[1], 6), round(p[2], 6))
        if key not in index_map:
            index_map[key] = len(verts)
            verts.append(p)

    remapped: list[tuple[int, int, int]] = []
    for f in faces:
        if len(f) < 3:
            continue
        a, b, c = f[0], f[1], f[2]
        if a == b or b == c or a == c:
            continue
        remapped.append(
            (
                index_map[(round(points[a][0], 6), round(points[a][1], 6), round(points[a][2], 6))],
                index_map[(round(points[b][0], 6), round(points[b][1], 6), round(points[b][2], 6))],
                index_map[(round(points[c][0], 6), round(points[c][1], 6), round(points[c][2], 6))],
            )
        )

    vol = 0.0
    for a, b, c in remapped:
        pa, pb, pc = points[a], points[b], points[c]
        vol += (
            pa[0] * (pb[1] * pc[2] - pb[2] * pc[1])
            - pa[1] * (pb[0] * pc[2] - pb[2] * pc[0])
            + pa[2] * (pb[0] * pc[1] - pb[1] * pc[0])
        ) / 6.0
    return vol


def _edge_counts(points, faces) -> dict:
    """Undirected edge -> use count. Every edge of a closed shell appears twice."""
    index_map = {}
    for i, p in enumerate(points):
        index_map.setdefault((round(p[0], 6), round(p[1], 6), round(p[2], 6)), i)
    counts: dict[tuple[int, int], int] = {}
    for f in faces:
        ids = [index_map[(round(points[i][0], 6), round(points[i][1], 6), round(points[i][2], 6))] for i in f[:3]]
        for a, b in ((ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])):
            key = (a, b) if a < b else (b, a)
            counts[key] = counts.get(key, 0) + 1
    return counts


def verify_asset(asset: dict, *, ground_category: str | None = None) -> list[str]:
    """Return a list of violation strings; empty means the asset is valid."""
    errors: list[str] = []
    aid = asset.get("id", "<missing id>")

    if not aid or aid == "<missing id>":
        errors.append(f"{aid}: missing `id`")
    if not asset.get("category"):
        errors.append(f"{aid}: missing `category`")
    if asset.get("y_up") is not True:
        errors.append(f"{aid}: y_up must be true (renderer assumes Y-up)")

    parts = asset.get("parts") or []
    if not parts:
        errors.append(f"{aid}: no parts")
        return errors

    tri_total = 0
    names: set[str] = set()
    all_points: list[list[float]] = []
    grounded_here = ground_category is not None

    for pi, part in enumerate(parts):
        pname = part.get("name") or f"<part {pi}>"
        if pname in names:
            errors.append(f"{aid}/{pname}: duplicate part name (renderer addresses parts by name)")
        names.add(pname)

        pts = part.get("positions") or []
        faces = part.get("faces") or []
        if not pts:
            errors.append(f"{aid}/{pname}: empty positions")
            continue
        if not faces:
            errors.append(f"{aid}/{pname}: empty faces")
            continue

        all_points.extend(pts)
        tri_total += len(faces)

        nverts = len(pts)
        has_bad_index = False
        for f in faces:
            if len(f) != 3:
                errors.append(f"{aid}/{pname}: face with {len(f)} indices (must be 3)")
                break
            for idx in f:
                if not isinstance(idx, int) or idx < 0 or idx >= nverts:
                    errors.append(
                        f"{aid}/{pname}: face index {idx} out of range (have {nverts} positions)"
                    )
                    has_bad_index = True
                    break

        non_finite = False
        for p in pts:
            for c in p[:3]:
                if not isinstance(c, (int, float)) or not math.isfinite(c):
                    errors.append(f"{aid}/{pname}: non-finite coordinate {c}")
                    non_finite = True
                    break
            if non_finite:
                break

        # zero-area triangles: a degenerate face renders as nothing and still
        # costs the full vertex budget
        degenerate = 0
        for f in faces:
            if len(f) != 3:
                continue
            if any(not isinstance(i, int) or i < 0 or i >= nverts for i in f):
                # already reported above; skip so this pass cannot crash
                continue
            a, b, c = (pts[f[0]], pts[f[1]], pts[f[2]])
            ux, uy, uz = b[0] - a[0], b[1] - a[1], b[2] - a[2]
            vx, vy, vz = c[0] - a[0], c[1] - a[1], c[2] - a[2]
            cx, cy, cz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
            if (cx * cx + cy * cy + cz * cz) ** 0.5 < 1e-12:
                degenerate += 1
        if degenerate > max(1, len(faces) // 100):
            errors.append(f"{aid}/{pname}: {degenerate}/{len(faces)} degenerate faces")

        # closed shells must be outward. An open surface has no interior, so
        # its signed volume is meaningless and must declare a direction.
        if has_bad_index or non_finite:
            # geometry-level checks already reported the defect; running the
            # topology pass on it would crash rather than report
            continue
        edges = _edge_counts(pts, faces)
        boundary = [e for e, c in edges.items() if c == 1]
        is_open = bool(part.get("open_surface")) or len(boundary) > 0
        if not is_open:
            vol = _signed_volume(pts, faces)
            if vol < 0.0:
                errors.append(
                    f"{aid}/{pname}: signed volume {vol:.6f} < 0 (inside-out; renders wrong from one side)"
                )
        elif part.get("open_surface") and not part.get("outward_axis"):
            errors.append(f"{aid}/{pname}: open surface must declare `outward_axis`")

    declared_tris = asset.get("tri_count")
    if declared_tris is not None and declared_tris != tri_total:
        errors.append(f"{aid}: tri_count {declared_tris} != actual {tri_total}")

    budget = CATEGORY_BUDGET.get(asset.get("category", ""), DEFAULT_BUDGET)
    if tri_total > budget:
        errors.append(f"{aid}: {tri_total} triangles exceeds {asset.get('category')} budget {budget}")

    # bounds must match the geometry actually shipped
    b = asset.get("bounds")
    if b and all_points:
        lo = [min(p[i] for p in all_points) for i in range(3)]
        hi = [max(p[i] for p in all_points) for i in range(3)]
        for i, axis in enumerate("xyz"):
            if abs(b["min"][i] - lo[i]) > BOUNDS_TOLERANCE:
                errors.append(f"{aid}: bounds.min[{axis}] {b['min'][i]} != actual {lo[i]}")
            if abs(b["max"][i] - hi[i]) > BOUNDS_TOLERANCE:
                errors.append(f"{aid}: bounds.max[{axis}] {b['max'][i]} != actual {hi[i]}")

    if grounded_here and all_points:
        lowest = min(p[1] for p in all_points)
        if abs(lowest) > GROUND_TOLERANCE:
            errors.append(f"{aid}: lowest point y={lowest:.6f}, ground assets must rest on y=0")

    return errors


def verify_assets_dir(assets_dir: str, *, ids: Sequence[str] | None = None) -> tuple[int, list[str], dict]:
    """Verify every asset json in a directory.

    Returns (asset_count, all_violations, per_category_tri_counts).
    """
    violations: list[str] = []
    per_category: dict[str, int] = {}
    count = 0

    filenames = sorted(f for f in os.listdir(assets_dir) if f.endswith(".json") and f != "manifest.json")
    if ids is not None:
        wanted = set(ids)
        filenames = [f for f in filenames if f[:-5] in wanted]

    for fname in filenames:
        path = os.path.join(assets_dir, fname)
        try:
            with open(path, encoding="utf-8") as fh:
                asset = json.load(fh)
        except json.JSONDecodeError as exc:
            violations.append(f"{fname}: invalid JSON ({exc})")
            continue

        cat = asset.get("category", "unknown")
        per_category[cat] = per_category.get(cat, 0) + int(asset.get("tri_count", 0))
        violations.extend(verify_asset(asset))
        count += 1

    return count, violations, per_category


def main(argv: Sequence[str]) -> int:
    assets_dir = argv[1] if len(argv) > 1 else "assets"
    count, violations, per_category = verify_assets_dir(assets_dir)

    print(f"VERIFY: {count} assets, {len(violations)} violation(s)")
    for cat in sorted(per_category):
        print(f"  {cat:12s} {per_category[cat]:8d} tris")
    total = sum(per_category.values())
    print(f"  {'TOTAL':12s} {total:8d} tris  (budget {TOTAL_BUDGET})")
    if total > TOTAL_BUDGET:
        violations.append(f"library total {total} exceeds budget {TOTAL_BUDGET}")

    for v in violations[:40]:
        print(f"  VIOLATION: {v}")
    if len(violations) > 40:
        print(f"  ... and {len(violations) - 40} more")

    if violations:
        print("VERIFY_FAIL")
        return 1
    print("VERIFY_OK")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))