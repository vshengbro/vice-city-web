"""Report every asset's real dimensions in human units, and flag ones that
disagree with the real-world size they are meant to represent.

    python3 tools/blender/report_sizes.py

Reads the exported JSON, so it measures exactly what the game will load.

The bounds are reported as ``L x W x H`` where H is the JSON +Y (up) extent
and L/W are the two horizontal extents, largest first.  That is deliberately
NOT the builder's local frame: the exporter rotates Blender's Y-up mesh into
the JSON frame, and getting that wrong makes a 4.6 m sedan look like a 9.1 m
bus.
"""
from __future__ import annotations

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.normpath(os.path.join(HERE, "..", "..", "assets"))

# (id, expected L, W, H in metres, tolerance) -- real-world reference sizes.
# Anything within tol is considered on-model; outside is listed for review.
# (id, expected X, expected Y(up), expected Z, tolerance) -- all in the JSON
# frame's OWN axis order, because several assets are long in Z rather than X
# (a shotgun's barrel points along Z; a palm is tall in Y but its crown spans
# X and Z).  A zero means "not constrained on this axis".
#
#   Y is the only up axis.  X and Z are both horizontal, and which one
#   carries the length differs by category: road vehicles run along X, while
#   weapons and beach props are authored long in Z (see the per-block notes).
#
# Specs are copied from the real-world reference object, with a tolerance that
# allows for deliberate stylisation.  The game draws small props slightly
# oversized so they read from a car at 30 m.
SPEC = {
    # ---- road vehicles: X = length, Y = height, Z = width --------------
    "car_sedan":      (4.57, 1.45, 1.85, 0.25),
    "car_coupe":      (4.45, 1.30, 1.84, 0.25),
    "car_taxi":       (4.57, 1.60, 1.85, 0.30),   # roof sign adds height
    "car_police":     (4.57, 1.59, 1.86, 0.30),   # light bar adds height
    "truck_pickup":   (5.36, 1.78, 1.99, 0.40),
    "bus_city":       (11.82, 3.26, 2.62, 1.20),
    # ---- pedestrians: height is the only thing that matters -------------
    "ped_suit":       (0.0, 1.78, 0.0, 0.10),
    "ped_dress":      (0.0, 1.72, 0.0, 0.10),
    "ped_overalls":   (0.0, 1.78, 0.0, 0.10),
    "ped_streetwear": (0.0, 1.80, 0.0, 0.10),
    # ---- palms: height only; the horizontal box is the crown spread -----
    "palm_tall":      (0.0, 9.50, 0.0, 2.50),
    "palm_short":     (0.0, 6.60, 0.0, 1.60),
    "palm_bushy":     (0.0, 6.10, 0.0, 1.40),
    # ---- street furniture: height only (mast arms dominate the X/Z box) --
    "prop_streetlight": (0.0, 6.50, 0.0, 1.00),
    "prop_trafficlight": (0.0, 4.60, 0.0, 0.60),
    "prop_traffic_cone": (0.44, 0.68, 0.44, 0.10),
    "prop_parking_meter": (0.24, 1.39, 0.23, 0.12),
    "prop_fire_hydrant": (0.0, 1.05, 0.0, 0.10),
    "prop_mailbox":   (0.0, 1.81, 0.0, 0.20),
    "prop_bench":     (1.86, 0.96, 0.67, 0.15),
    # ---- weapons: held in the hand, so Y = height of the held pose -------
    "wep_pistol":     (0.04, 0.17, 0.22, 0.05),
    "wep_smg":        (0.05, 0.28, 0.46, 0.08),
    "wep_shotgun":    (0.05, 0.16, 0.74, 0.10),
    "wep_rocket_launcher": (0.21, 0.28, 1.00, 0.12),
    "wep_bat":        (0.10, 0.74, 0.10, 0.10),
    # ---- beach ------------------------------------------------------------
    "beach_umbrella": (2.60, 2.27, 2.60, 0.60),
    "beach_chair":    (0.66, 1.01, 1.85, 0.30),   # reclines along Z
    "beach_surfboard": (1.99, 0.44, 0.50, 0.20),
    "beach_boardwalk_plank": (2.00, 0.06, 0.25, 0.05),
    # ---- craft: height only (rotor span / hull LOA dominate) ------------
    "veh_helicopter": (0.0, 3.20, 0.0, 0.60),
    "veh_boat":       (0.0, 1.45, 0.0, 0.40),
}


def dims(doc):
    """Return (X, Y-up, Z) extents in the JSON frame, unsorted.

    Do NOT sort these.  A spec is written against specific axes (a shotgun is
    0.74 along Z because that is where the barrel points), so silently
    reordering the axes makes an exact match report as a mismatch and hides
    real size errors behind a lucky permutation.
    """
    lo, hi = doc["bounds"]["min"], doc["bounds"]["max"]
    return hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]


def human(x, y, z):
    """Render as the L x W x H a person would quote (height = Y)."""
    l, w = sorted((x, z), reverse=True)
    return "%.2f x %.2f x %.2f" % (l, w, y)


def raw(x, y, z):
    """Render in the JSON frame's own axis order, for axis-sensitive specs."""
    return "%.2f x %.2f x %.2f" % (x, y, z)


def main() -> int:
    man = json.load(open(os.path.join(ASSETS, "manifest.json")))
    entries = sorted(man["assets"], key=lambda e: (e["category"], e["id"]))

    print("ALL ASSETS  (%d assets, %s triangles)"
          % (man["asset_count"], format(man["tri_count"], ",")))
    print()
    print("%-24s %-10s %5s  %s" % ("id", "category", "tris", "L x W x H (m)"))
    print("-" * 78)
    for e in entries:
        d = dims(json.load(open(os.path.join(ASSETS, e["file"]))))
        print("%-24s %-10s %5d  %s"
              % (e["id"], e["category"], e["tri_count"], human(*d)))

    print()
    print("PROPORTION CHECK vs real-world reference sizes")
    print("-" * 78)
    print("%-22s %-22s %-22s %s" % ("id", "expected LxWxH", "actual LxWxH", "verdict"))
    rows = []
    for e in entries:
        spec = SPEC.get(e["id"])
        if not spec:
            continue
        el, ew, eh, tol = spec
        d = dims(json.load(open(os.path.join(ASSETS, e["file"]))))
        # A zero in the expected axis means "not constrained" (e.g. a palm's
        # crown spread is whatever the fronds happen to span).
        # Compare the RAW, axis-ordered extents: a spec says "0.74 along Z"
        # because that is the axis the barrel points down, and permuting axes
        # here would both invent a mismatch and mask a real one.
        err = max(abs(d[0] - el) if el else 0.0,
                  abs(d[1] - ew) if ew else 0.0,
                  abs(d[2] - eh) if eh else 0.0)
        ok = err <= tol
        rows.append((e["id"], (el, ew, eh), d, err, ok))

    bad = 0
    for aid, exp, act, err, ok in rows:
        print("%-22s %-22s %-22s %s"
              % (aid,
                 raw(*exp),
                 raw(*act),
                 "ok" if ok else "OFF by %.2fm" % err))
        bad += (not ok)
    print()
    print("proportion check: %d/%d within tolerance, %d flagged"
          % (len(rows) - bad, len(rows), bad))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
