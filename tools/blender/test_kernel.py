"""Unit tests for the vcw geometry kernel -- pure stdlib, no bpy.

    python3 tools/blender/test_kernel.py

The kernel deliberately imports nothing from Blender so every builder stays
testable with a stock ``python3``.  This file is the fastest way to catch a
regression in a primitive before running a full asset build.
"""

import math
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from vcw import core as C                                      # noqa: E402
from vcw import export                                         # noqa: E402


def _all_faces_outward(mesh, interior, tol=-1e-6):
    """Every face normal must point away from ``interior``."""
    ref = tuple(interior)
    for (a, b, c) in mesh.faces:
        va, vb, vc = mesh.pos[a], mesh.pos[b], mesh.pos[c]
        n = C.cross(C.sub(vb, va), C.sub(vc, va))
        if C.length(n) < 1e-12:
            return False
        n = C.normalize(n)
        cen = C.mul(C.add(C.add(va, vb), vc), 1.0 / 3.0)
        d = C.sub(cen, ref)
        if C.length(d) < 1e-9:
            continue
        if C.dot(n, C.normalize(d)) < tol:
            return False
    return True


class TestChamferBox(unittest.TestCase):
    """``chamfer_box`` must be a closed, outward-wound solid of the right size."""

    def _probe(self, size, center, bevel, n_corner):
        m = C.Mesh()
        C.chamfer_box(m, size, center=center, color=(0.8, 0.8, 0.8),
                      bevel=bevel, n_corner=n_corner)
        self.assertEqual(m.skipped, 0, "degenerate triangles were dropped")
        n_closed, n_inv, n_open = C.closed_components(m)
        self.assertEqual(n_inv, 0, "a shell is wound inward")
        self.assertGreaterEqual(n_closed, 1, "no closed shell found")
        self.assertTrue(_all_faces_outward(m, center),
                        "a face points into the solid")
        # A chamfer must not change the silhouette: the extremes of the
        # profile are unchanged by an inward cut.
        lo, hi = m.bounds()
        for k, ax in enumerate(size):
            self.assertAlmostEqual(hi[k] - lo[k], ax, places=6)
            self.assertAlmostEqual((hi[k] + lo[k]) * 0.5, center[k], places=6)
        return m

    def test_volume_matches_the_uncut_box(self):
        # A chamfer only cuts the EDGES, so the enclosed volume is the box
        # volume minus the twelve edge wedges.  It must be strictly less
        # than the box but within a few percent of it -- that catches a
        # cap wound the wrong way (which would make volume negative) and a
        # ring built at the wrong extent (which would make it too small).
        size = (1.0, 0.6, 0.4)
        center = (0.0, 0.0, 0.2)
        m = self._probe(size, center, 0.04, 1)
        exact = size[0] * size[1] * size[2]
        vol = C.signed_volume(m)
        self.assertGreater(vol, 0.0)
        self.assertLess(vol, exact)
        self.assertGreater(vol, exact * 0.90)

    def test_tri_count_is_budgeted(self):
        # 8-point ring (n_corner=1) -> 8 side quads + 2x6 cap triangles.
        m = C.Mesh()
        C.chamfer_box(m, (1.0, 1.0, 1.0), bevel=0.05, n_corner=1)
        self.assertEqual(m.ntris, 8 * 2 + 2 * 6)
        # A hard box is 12; the chamfer is 28.  That ratio is the whole cost
        # argument, so pin it: if rounded_rect gains points, this must change
        # deliberately.
        plain = C.Mesh()
        C.box(plain, (1.0, 1.0, 1.0))
        self.assertEqual(plain.ntris, 12)

    def test_bevel_zero_falls_back_to_a_plain_box(self):
        a = C.Mesh()
        C.chamfer_box(a, (1.0, 0.5, 0.25), bevel=0.0)
        b = C.Mesh()
        C.box(b, (1.0, 0.5, 0.25))
        self.assertEqual(a.ntris, b.ntris)

    def test_bevel_is_clamped_so_the_rings_cannot_cross(self):
        # bevel larger than a quarter of the smallest extent would make the
        # two rings overlap and degenerate the side quads.
        m = C.Mesh()
        C.chamfer_box(m, (0.10, 0.10, 0.10), bevel=5.0, n_corner=1)
        self.assertEqual(m.skipped, 0)
        self.assertTrue(_all_faces_outward(m, (0.0, 0.0, 0.0)))

    def test_n_corner_two_still_closes(self):
        m = C.Mesh()
        C.chamfer_box(m, (1.0, 0.6, 0.4), center=(0, 0, 0.2), bevel=0.08,
                      n_corner=2)
        n_closed, n_inv, _n_open = C.closed_components(m)
        self.assertEqual(n_inv, 0)
        self.assertGreaterEqual(n_closed, 1)
        self.assertEqual(m.skipped, 0)
        lo, hi = m.bounds()
        self.assertAlmostEqual(hi[0] - lo[0], 1.0, places=6)

    def test_export_round_trip(self):
        m = C.Mesh()
        C.chamfer_box(m, (1.0, 0.6, 0.4), center=(0, 0, 0.2), bevel=0.04)
        asset = C.Asset("probe", "probe")
        part = C.Part("body", base_color=(0.8, 0.8, 0.8))
        part.mesh = m
        asset.add(part)
        doc = export.export_asset(asset)
        self.assertEqual(doc["tri_count"], m.ntris)
        self.assertTrue(doc["y_up"])


class TestPrimitives(unittest.TestCase):
    """Regression cover for the primitives the builders lean on hardest."""

    def test_torus_all_axes_wind_outward(self):
        # The Y axis is the only ODD permutation in torus()'s placement, and
        # a sign error there produces a closed but inside-out tube that no
        # face-count test would catch.
        for axis in ("X", "Y", "Z"):
            m = C.Mesh()
            C.torus(m, 0.5, 0.12, 12, 6, center=(0, 0, 0.5), axis=axis)
            n_closed, n_inv, _o = C.closed_components(m)
            self.assertEqual(n_inv, 0, "torus on %s is inside-out" % axis)
            self.assertGreaterEqual(n_closed, 1, "torus on %s is open" % axis)

    def test_sphere_poles_are_single_vertices(self):
        # Repeating a pole point seg_u times yields seg_u zero-area
        # triangles, which the mesh silently drops.
        m = C.Mesh()
        C.sphere(m, 0.4, seg_u=10, seg_v=6, center=(0, 0, 0.4))
        self.assertEqual(m.skipped, 0)

    def test_cone_apex_is_one_vertex(self):
        m = C.Mesh()
        C.cone(m, 0.3, 0.8, 10, center=(0, 0, 0.4))
        self.assertEqual(m.skipped, 0)
        n_closed, n_inv, _o = C.closed_components(m)
        self.assertEqual(n_inv, 0)

    def test_cylinder_rests_exactly_on_z0_when_asked(self):
        m = C.Mesh()
        C.cylinder(m, 0.2, 0.2, 12, center=(0, 0, 0.1))
        lo, _hi = m.bounds()
        self.assertAlmostEqual(lo[2], 0.0, places=9)


class TestBuilders(unittest.TestCase):
    """Every shipped asset must survive the full export contract."""

    def test_all_builders_export_clean(self):
        from vcw.builders import (buildings, markers, misc, palms, pedestrians,
                                  props, signs, vehicles, weapons)
        total = 0
        ids = set()
        for module in (buildings, vehicles, pedestrians, props, signs, palms,
                       misc, weapons, markers):
            for asset in module.build_all():
                self.assertNotIn(asset.id, ids,
                                 "duplicate asset id %r" % asset.id)
                ids.add(asset.id)
                doc = export.export_asset(asset)   # raises on any violation
                total += doc["tri_count"]
                self.assertGreater(doc["tri_count"], 0, asset.id)
                self.assertLessEqual(doc["tri_count"], 8000,
                                     "%s over the per-asset budget" % asset.id)
        self.assertGreater(total, 0)
        sys.stderr.write("\n%d assets, %d triangles\n" % (len(ids), total))


if __name__ == "__main__":
    unittest.main(verbosity=2)
