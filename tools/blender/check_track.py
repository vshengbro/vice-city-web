"""Does the tank track band actually enclose the road wheels?

Measures the built wep_tank in AUTHORING space (x = lateral, y = length,
z = up) and reports, per side, the track band's centre offset from the hull
axis and the extent of each.

A tank reads as running gear only if the band runs LENGTHWISE (along y) and
sits outboard of the wheels on x.  If the band's long axis is x instead, it
crosses the hull and the wheels float free of it.
"""
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from vcw.builders import military                          # noqa: E402


def ext(part, lo=-1e9, hi=1e9):
    xs = [p[0] for p in part.mesh.pos]
    ys = [p[1] for p in part.mesh.pos]
    zs = [p[2] for p in part.mesh.pos]
    return ((min(xs), max(xs)), (min(ys), max(ys)), (min(zs), max(zs)))


def main():
    asset = [a for a in military.build_all() if a.id == "wep_tank"][0]
    by = {p.name: p for p in asset.parts}
    tris = sum(len(p.mesh.faces) for p in asset.parts)
    print("wep_tank tris=%d parts=%s" % (tris, list(by)))
    gx, gy, gz = ext(by["gear"])
    wx, wy, wz = ext(by["metal"])
    hx, hy, hz = ext(by["hull"])
    print("hull   x[%6.2f,%6.2f] y[%6.2f,%6.2f] z[%6.2f,%6.2f]" %
          (*hx, *hy, *hz))
    print("gear   x[%6.2f,%6.2f] y[%6.2f,%6.2f] z[%6.2f,%6.2f]" %
          (*gx, *gy, *gz))
    print("metal  x[%6.2f,%6.2f] y[%6.2f,%6.2f] z[%6.2f,%6.2f]" %
          (*wx, *wy, *wz))
    gear_len_x = gx[1] - gx[0]
    gear_len_y = gy[1] - gy[0]
    print()
    print("gear long axis: X=%.2f  Y=%.2f" % (gear_len_x, gear_len_y))
    verdict = "LENGTHWISE (correct)" if gear_len_y > gear_len_x else \
        "CROSSWISE (bug)"
    print("track band runs %s" % verdict)
    print("wheel span     : X=%.2f  Y=%.2f" % (wx[1] - wx[0], wy[1] - wy[0]))
    # Track offset: outboard of the hull on x?
    track_off = (abs(gx[0]) + abs(gx[1])) * 0.5
    hull_half = (hx[1] - hx[0]) * 0.5
    print("track band centre offset on X = %.2f m (hull half-width %.2f m)"
          % (track_off, hull_half))
    print("band lowest z = %.3f (must be 0.00: vehicle rests on the ground)"
          % gz[0])
    print("wheel top z   = %.3f" % wz[1])
    print("band height   = %.3f" % (gz[1] - gz[0]))
    print("VERDICT: %s" % ("PASS" if verdict.startswith("LENGTHWISE")
                          else "FAIL - track crosses the hull"))


if __name__ == "__main__":
    main()