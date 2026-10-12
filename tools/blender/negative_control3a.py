"""Negative control: prove verify3a.py rejects geometry it is supposed to reject.

A verifier that passes proves only that it did not crash — it cannot tell
"every mesh is correct" from "the verifier checks nothing". Both print
`0 violations`. Each mutation below is a real defect class this pipeline has
actually produced, and the control asserts the verifier catches every one.

Run: ``python3 negative_control3a.py``
Exits non-zero if ANY mutation slips through.
"""

from __future__ import annotations

import copy
import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from verify3a import verify_asset  # noqa: E402


def _box_part(name: str, size=(1.0, 1.0, 1.0), base=(0.8, 0.2, 0.3)):
    """A valid outward-wound unit-ish box, used as the mutation baseline."""
    sx, sy, sz = (s / 2.0 for s in size)
    pts = [
        [-sx, -sy, -sz], [sx, -sy, -sz], [sx, sy, -sz], [-sx, sy, -sz],
        [-sx, -sy, sz], [sx, -sy, sz], [sx, sy, sz], [-sx, sy, sz],
    ]
    faces = [
        [0, 2, 1], [0, 3, 2],   # bottom, outward = -Z
        [4, 5, 6], [4, 6, 7],   # top, outward = +Z
        [0, 1, 5], [0, 5, 4],   # -Y
        [2, 3, 7], [2, 7, 6],   # +Y
        [1, 2, 6], [1, 6, 5],   # +X
        [3, 0, 4], [3, 4, 7],   # -X
    ]
    return {"name": name, "base_color": list(base), "positions": pts, "faces": faces, "flat": True}


def valid_asset() -> dict:
    return {
        "id": "probe",
        "category": "prop",
        "y_up": True,
        "bounds": {"min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5]},
        "part_count": 1,
        "tri_count": 12,
        "parts": [_box_part("body")],
    }


# --- mutations -------------------------------------------------------------


def mut_valid(a: dict) -> dict:
    return a


def mut_invert_faces(a: dict) -> dict:
    """Flip every face's winding: the shell is inside-out."""
    for p in a["parts"]:
        p["faces"] = [list(reversed(f)) for f in p["faces"]]
    return a


def mut_index_out_of_range(a: dict) -> dict:
    a["parts"][0]["faces"][0] = [0, 1, 999]
    return a


def mut_tri_count_mismatch(a: dict) -> dict:
    a["tri_count"] = 999
    return a


def mut_bounds_mismatch(a: dict) -> dict:
    a["bounds"] = {"min": [-9.0, -9.0, -9.0], "max": [9.0, 9.0, 9.0]}
    return a


def mut_duplicate_part_names(a: dict) -> dict:
    a["parts"].append(copy.deepcopy(a["parts"][0]))
    return a


def mut_degenerate_face(a: dict) -> dict:
    """Collapse a triangle to zero area: it renders as nothing."""
    p = a["parts"][0]
    p["faces"][0] = [0, 0, 0]
    p["positions"][1] = list(p["positions"][0])
    return a


def mut_non_finite(a: dict) -> dict:
    a["parts"][0]["positions"][2][0] = float("nan")
    return a


def mut_missing_y_up(a: dict) -> dict:
    a["y_up"] = False
    return a


def mut_open_surface_undeclared(a: dict) -> dict:
    """An open surface without an outward reference cannot be checked."""
    p = a["parts"][0]
    p["positions"] = p["positions"][:4]
    p["faces"] = [[0, 1, 2]]
    a["tri_count"] = 1
    a["bounds"] = {"min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5]}
    p["open_surface"] = True
    return a


def mut_over_budget(a: dict) -> dict:
    """A prop far past its triangle cap must be rejected."""
    p = a["parts"][0]
    big = p["positions"][0]
    for i in range(1, len(p["positions"])):
        p["positions"][i] = [big[0] + i * 1e-4, big[1], big[2]]
    a["tri_count"] = 12
    return a


MUTATIONS = [
    ("valid", mut_valid),
    ("invert_faces", mut_invert_faces),
    ("index_out_of_range", mut_index_out_of_range),
    ("tri_count_mismatch", mut_tri_count_mismatch),
    ("bounds_mismatch", mut_bounds_mismatch),
    ("duplicate_part_names", mut_duplicate_part_names),
    ("degenerate_face", mut_degenerate_face),
    ("non_finite_coordinate", mut_non_finite),
    ("y_up_false", mut_missing_y_up),
    ("open_surface_undeclared", mut_open_surface_undeclared),
]


def main() -> int:
    print("NEGATIVE CONTROL — verify3a.py")

    base = valid_asset()
    base_errors = verify_asset(base)
    if base_errors:
        print(f"FATAL: the valid baseline is itself rejected: {base_errors}")
        return 1
    print("  baseline valid asset: accepted (as required)")

    rejected = 0
    failed: list[str] = []

    for name, fn in MUTATIONS:
        if name == "valid":
            continue
        mutant = fn(copy.deepcopy(base))
        errors = verify_asset(mutant)
        if errors:
            rejected += 1
            first = errors[0]
            print(f"  REJECTED {name:28s} -> {first[:72]}")
        else:
            failed.append(name)
            print(f"  ACCEPTED {name:28s} -> LEAK (verifier missed it)")

    # the budget case needs a real triangle explosion, so build it directly
    bomb = valid_asset()
    bomb["category"] = "road"          # 2k budget
    part = bomb["parts"][0]
    grid = []
    faces = []
    n = 40
    for x in range(n):
        for y in range(n):
            grid.append([x * 0.01, 0.0, y * 0.01])
    for x in range(n - 1):
        for y in range(n - 1):
            a = x * n + y
            b = a + 1
            c = a + n
            d = c + 1
            faces.extend([[a, c, b], [b, c, d]])
    part["positions"] = grid
    part["faces"] = faces
    bomb["tri_count"] = len(faces)
    bomb["bounds"] = {"min": [0.0, 0.0, 0.0], "max": [(n - 1) * 0.01, 0.0, (n - 1) * 0.01]}
    errs = verify_asset(bomb)
    if errs:
        rejected += 1
        print(f"  REJECTED {'over_budget':28s} -> {errs[0][:72]}")
    else:
        failed.append("over_budget")
        print(f"  ACCEPTED {'over_budget':28s} -> LEAK")

    print(f"\nNEGATIVE CONTROL: {rejected} rejected, {len(failed)} leaked")
    if failed:
        print("NEGATIVE_CONTROL_FAIL: verifier is too weak: " + ", ".join(failed))
        return 1
    print("NEGATIVE_CONTROL_OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())