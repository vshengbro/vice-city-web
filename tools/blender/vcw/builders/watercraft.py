"""Category "vehicle", second kit: aviation and marine craft.

Four assets, all procedural -- every vertex is generated here from the
``core`` kernel primitives, nothing is imported, traced or measured from a
real aircraft or boat:

* ``veh_airplane``     -- high-wing single-engine prop, ~8.0 m span, ~7.0 m long
* ``veh_jet``          -- delta-wing fighter, ~9.0 m span, ~11.2 m long
* ``veh_boat``         -- open-cockpit speedboat with an outboard, ~6.1 m long
* ``veh_helicopter``   -- utility rotorcraft, ~10 m rotor, ~10.9 m long

Authoring convention (Blender-style): Z-up, -Y is forward, 1 unit = 1 m,
every asset centred on X = 0 with its origin resting on Z = 0 (undercarriage
or skids on the ground; the boat's keel on the waterline).  The exporter
converts to Y-up afterwards -- never pre-convert here.

FIVE KERNEL FACTS THIS MODULE LEANS ON HARD
===========================================

1. **A loft advances along the ring's RIGHT-HAND normal.**  A profile drawn
   CCW in the (x, z) plane has right-hand normal ``x_hat x z_hat = -Y``,
   i.e. the OPPOSITE of the +Y direction these fuselages advance in.  Every
   longitudinal loft here therefore goes through :func:`_loft_y`, which
   reverses the profile.  Miss that and the whole shell is watertight but
   inside-out, which the exporter rejects on signed volume.

2. **A wing is not a slab.**  At gameplay distance a razor-thin, zero
   thickness surface reads better than a box, and it costs a quarter of the
   triangles.  But the renderer enables ``CULL_FACE``/``BACK``
   (``src/render.rs``), so a single-winding open sheet VANISHES when the
   camera drops below it.  :func:`_skin` emits every panel twice, once per
   winding, which makes the sheet genuinely two-sided at no positional cost.
   It still shades correctly: the underside normal faces away from the light,
   so it falls back to ambient -- exactly what a real wing does.

3. **Such a sheet is an OPEN surface, so it must declare ``outward``.**  Two
   faces of the same panel point in opposite directions, so
   ``("dir", n)`` is unusable (it requires *every* face to point along n).
   ``("point", ref)`` works, but only if ``ref`` lies exactly IN the sheet's
   plane: ``verify_outward`` then computes ``dot(+-n_hat, d_hat) == 0`` for
   every face, which clears the ``< -1e-6`` rejection by nine orders of
   magnitude.  Put the reference off-plane and one of the two windings fails.
   The consequence, enforced by :func:`_skin`: **every skinned part must be
   perfectly planar** -- no dihedral, no washout, no taper in thickness.
   Planarity is the price of the declaration, and it is why these aircraft
   have flat flying surfaces and get their volume from taper, sweep and
   section shape instead.

4. **A loft profile's point count must not change between stations.**
   ``core.rounded_rect`` silently drops its closing point when the radius is
   clamped to zero, and ``loft`` asserts equal cardinality -- so every
   station here keeps a corner radius strictly inside ``min(hx, hy)``, and
   every hand-written ring (the boat's hull) has a fixed length by
   construction.

5. **The runtime shader is** ``base * (ambient + light * max(dot(n, l), 0))``
   **with no specular and no texture.**  Two adjacent polygons on one shell
   can only differ through albedo or normal, so every asset here pays for
   :func:`C.recolor_faces_where` zoning and chamfered arris blocks.  That is
   the single biggest quality lever in the file.
"""

import math

from .. import core as C


# --------------------------------------------------------------------------
# palette -- the aviation set reads as one fleet over NEON BAY
# --------------------------------------------------------------------------

WHITE = (0.93, 0.93, 0.90)
CREAM = (0.90, 0.87, 0.78)
CORAL = (0.94, 0.40, 0.24)
MAGENTA = (0.90, 0.26, 0.58)
AMBER = (0.99, 0.74, 0.14)
TEAL = (0.12, 0.58, 0.58)
SLATE = (0.30, 0.33, 0.38)
SLATE_DK = (0.16, 0.18, 0.22)
GRAPHITE = (0.10, 0.11, 0.13)
METAL = (0.62, 0.64, 0.68)
METAL_DK = (0.34, 0.36, 0.40)
CHROME = (0.80, 0.82, 0.85)
GLASS = (0.055, 0.085, 0.120)
GLASS_TINT = (0.075, 0.120, 0.150)
RUBBER = (0.075, 0.075, 0.085)
LAMP_WHITE = (1.00, 0.96, 0.80)
LAMP_RED = (1.00, 0.12, 0.10)
LAMP_GREEN = (0.14, 1.00, 0.28)
BURNT = (1.00, 0.46, 0.10)

# Default chamfer for every solid block bolted onto a hull.  13 mm is the
# smallest bevel that survives the thinnest panel below without being clamped
# away, and it is the only thing that gives a hard edge a highlight.
_BEVEL = 0.013


# --------------------------------------------------------------------------
# local helpers -- all real geometry goes through core
# --------------------------------------------------------------------------

def _mat(deg):
    """Rotation matrix ``R = Rz * Ry * Rx`` from Euler angles in DEGREES."""
    ax, ay, az = (math.radians(v) for v in deg)
    cx, sx = math.cos(ax), math.sin(ax)
    cy, sy = math.cos(ay), math.sin(ay)
    cz, sz = math.cos(az), math.sin(az)
    return ((cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx),
            (sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - sz * sx),
            (-sy, cy * sx, cy * cx))


def _rbox(m, size, center=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
          color=(1.0, 1.0, 1.0)):
    """A box rotated about its own centre -- ``C.box`` is axis-aligned only."""
    hx, hy, hz = size[0] * 0.5, size[1] * 0.5, size[2] * 0.5
    corners = [(-hx, -hy, -hz), (hx, -hy, -hz), (hx, hy, -hz), (-hx, hy, -hz),
               (-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz)]
    r = _mat(rot)
    o = tuple(center)
    p = [C.add(o, (r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
                   r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
                   r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2]))
         for v in corners]
    m.quad(p[0], p[3], p[2], p[1], color)
    m.quad(p[4], p[5], p[6], p[7], color)
    m.quad(p[0], p[1], p[5], p[4], color)
    m.quad(p[1], p[2], p[6], p[5], color)
    m.quad(p[2], p[3], p[7], p[6], color)
    m.quad(p[3], p[0], p[4], p[7], color)
    return m


def _cbx(m, size, center, color, bevel=_BEVEL, rot=(0.0, 0.0, 0.0)):
    """One chamfered box, axis aligned or rotated: 28 tris, every arris lit."""
    if rot == (0.0, 0.0, 0.0):
        C.chamfer_box(m, size, center=center, color=color, bevel=bevel)
        return m
    tmp = C.Mesh()
    C.chamfer_box(tmp, size, center=(0.0, 0.0, 0.0), color=color, bevel=bevel)
    r = _mat(rot)
    base = len(m.pos)
    for q in tmp.pos:
        m.pos.append(C.add(tuple(center), (r[0][0] * q[0] + r[0][1] * q[1] + r[0][2] * q[2],
                                       r[1][0] * q[0] + r[1][1] * q[1] + r[1][2] * q[2],
                                       r[2][0] * q[0] + r[2][1] * q[1] + r[2][2] * q[2])))
    for f in tmp.faces:
        m.faces.append((f[0] + base, f[1] + base, f[2] + base))
    m.fcol.extend(tmp.fcol)
    return m


def _slab(m, pts, push, color):
    """Closed 4-sided slab: the outer quad ``pts`` extruded by ``push``.

    The slab grows along ``push`` into the body, so the corner order is
    normalised here and every caller does not have to think about it.

    ``push`` MUST be perpendicular to the quad.  Extruding a planar quad along
    a direction lying in its own plane gives the slab zero volume: its two
    side walls are the quad itself, every side triangle is zero-area, ``core``
    drops them, and the exporter then rejects the part outright with
    "degenerate triangles were dropped".  Rather than let that surface as a
    confusing downstream failure, refuse it at the call site and say what went
    wrong.
    """
    pts = [tuple(p) for p in pts]
    p = C.normalize(push)
    n = C.normalize(C.cross(C.sub(pts[1], pts[0]), C.sub(pts[2], pts[0])))
    if abs(C.dot(n, p)) < 1e-3:
        raise ValueError(
            "_slab: push %r is in-plane with the quad (normal %r); the slab "
            "would have zero volume and degenerate side walls. Extrude along "
            "the quad's own normal, or build the feature as a chamfered box."
            % (p, n))
    if C.dot(n, p) < 0.0:
        pts = list(reversed(pts))
    C.loft(m, [pts, [C.add(q, push) for q in pts]], color,
           cap_start_flip=True, cap_end_flip=False)
    return m


def _rings_y(stations, n_corner=2):
    """Rounded-rect cross-sections swept along +Y.

    ``stations`` is ``(y, half_width, half_height, z_centre, corner_radius)``.
    A profile drawn CCW in the (x, z) plane has right-hand normal -Y, so it
    is REVERSED here: reversed, the ring normal is +Y, which is the direction
    ``core.loft`` advances in, so the side walls come out outward and only the
    start cap needs flipping.
    """
    rings = []
    for (y, hx, hy, zc, r) in stations:
        hx = max(0.020, hx)
        hy = max(0.020, hy)
        # keep the radius strictly inside both half-extents: a radius clamped
        # to zero drops rounded_rect's closing point and changes the ring's
        # cardinality, which loft rejects with an assertion.
        rr = max(0.004, min(r, hx * 0.9, hy * 0.9))
        prof = C.rounded_rect(hx, hy, rr, n_corner)
        rings.append([(px, y, zc + pz) for (px, pz) in reversed(prof)])
    return rings


def _loft_y(m, stations, color, n_corner=2, cap_start=True, cap_end=True):
    """Loft ``(y, hx, hy, zc, r)`` stations along +Y into a closed shell."""
    return C.loft(m, _rings_y(stations, n_corner), color,
                  cap_start=cap_start, cap_end=cap_end,
                  cap_start_flip=True, cap_end_flip=False)


def _skin(part, panels, ref):
    """Emit each panel twice, once per winding, on a DECLARED-PLANE part.

    ``panels`` is a list of ``(quad, color)``.  Each quad is emitted forward
    and reversed, which makes a zero-thickness sheet visible from both sides
    under back-face culling while still shading like a real surface (the
    downward face gets ambient only).

    The part MUST have been created with ``outward=("point", ref)`` where
    ``ref`` lies exactly in the sheet's plane -- then every face normal is
    exactly perpendicular to ``centroid - ref``, so ``verify_outward`` sees
    ``dot == 0`` for both windings and accepts them with a 1e-6 margin.

    Consequence, enforced by this module: the sheet must be planar.  No
    dihedral, no washout, no taper in thickness.
    """
    m = part.mesh
    for quad, col in panels:
        q = [tuple(p) for p in quad]
        m.quad(q[0], q[1], q[2], q[3], col)
        m.quad(q[0], q[3], q[2], q[1], col)
    return m


def _wing_panels(span, chord_root, chord_tip, z, y_le0, sweep, stations=3,
                 color=WHITE, color_tip=None):
    """``(panels, ref)`` for a flat, straight wing of the given span.

    Stations run outward along +X for ONE side only; the caller mirrors x and
    emits the same panels on -X.  ``panels`` is one ``(quad, color)`` per
    spanwise segment, already stitched edge to edge -- the caller must NOT
    re-join them, or the shared-edge ordering collapses half of them to
    zero-area quads, which ``core`` drops and the exporter then rejects.

    Everything is built at exactly one ``z`` so the sheet stays planar, which
    is what lets ``ref`` sit in its own plane.
    """
    xs = [span * (i / float(stations - 1)) for i in range(stations)]
    le = [y_le0 + sweep * (x / span) for x in xs]
    ch = [chord_root + (chord_tip - chord_root) * (x / span) for x in xs]
    te = [le[i] + ch[i] for i in range(stations)]
    panels = []
    for i in range(stations - 1):
        col = color if (color_tip is None or i < stations - 2) else color_tip
        panels.append(([(xs[i], le[i], z), (xs[i + 1], le[i + 1], z),
                        (xs[i + 1], te[i + 1], z), (xs[i], te[i], z)], col))
    ref = (0.0, sum((le[i] + te[i]) * 0.5 for i in range(stations)) / stations, z)
    return panels, ref


def _mirror_panels(panels, sx):
    """Mirror a +X panel list onto -X, reversing winding to match."""
    return [([(sx * p[0], p[1], p[2]) for p in quad][::sx], col)
            for quad, col in panels]


def _zone_band(part, predicate, color):
    """``recolor_faces_where`` passthrough -- see the kernel docstring."""
    return C.recolor_faces_where(part, predicate, color)


def _struts(m, pairs, radius, color, seg=6):
    """A bundle of 2-point ``C.tube`` struts (closed faceted shells)."""
    for a, b in pairs:
        C.tube(m, [a, b], radius, seg, color=color, caps=True, smooth=False)
    return m


def _shift(parts, offset):
    """Translate whole parts (a pure translation, so winding is untouched)."""
    dx, dy, dz = offset
    for part in parts:
        part.mesh.pos = [(p[0] + dx, p[1] + dy, p[2] + dz)
                         for p in part.mesh.pos]
    return parts


def _seat(a):
    """Drop the asset so its lowest point rests exactly on Z = 0.

    The verifier only rejects a ground-resting category that sinks BELOW
    y = 0 (``GROUND_TOL`` is -0.001), so an asset authored 13 mm in the air
    passes every check and then hovers over the street at runtime.  Seating on
    the measured minimum instead of a hand-guessed strut height is what keeps
    the wheels on the tarmac.

    Declared ``outward=("point", ref)`` references are TRANSLATED BY THE SAME
    OFFSET.  A point reference is only meaningful relative to the geometry that
    was built around it: the wing's ref sits exactly in its sheet's plane, so
    moving the sheet down by 4 cm while leaving the ref behind would put the
    reference off-plane, and one of the sheet's two windings would then fail
    ``verify_outward``.
    """
    lo, _hi = a.bounds()
    dz = -lo[2]
    _shift(a.parts, (0.0, 0.0, dz))
    for p in a.parts:
        if p.outward is not None and p.outward[0] == "point":
            kind, ref = p.outward
            p.outward = (kind, (ref[0], ref[1], ref[2] + dz))
    return a


# ==========================================================================
# 1.  AIRPLANE -- high-wing single-engine propeller aircraft
# ==========================================================================

# Fuselage stations: (y, half_width, half_height, z_centre, corner_radius).
# The centreline rises 0.22 m from nose to tail (tail upsweep) so the deck
# line reads level while the tail clears the stabiliser.
_AIRPLANE_STATIONS = [
    (-3.10, 0.085, 0.095, 1.42, 0.050),
    (-2.86, 0.230, 0.235, 1.41, 0.115),
    (-2.45, 0.360, 0.330, 1.40, 0.150),
    (-1.85, 0.445, 0.375, 1.39, 0.150),
    (-1.00, 0.470, 0.400, 1.40, 0.150),
    (0.00, 0.455, 0.395, 1.43, 0.150),
    (1.05, 0.400, 0.350, 1.47, 0.140),
    (2.05, 0.300, 0.270, 1.52, 0.120),
    (2.95, 0.195, 0.180, 1.58, 0.080),
    (3.62, 0.070, 0.070, 1.64, 0.030),
]

# Cockpit greenhouse: a small bubble loft sitting proud of the spine.
_AIRPLANE_CANOPY = [
    (-2.32, 0.100, 0.050, 1.66, 0.020),
    (-1.98, 0.240, 0.150, 1.70, 0.060),
    (-1.45, 0.320, 0.210, 1.72, 0.080),
    (-0.85, 0.330, 0.200, 1.70, 0.080),
    (-0.50, 0.280, 0.130, 1.64, 0.050),
]

_AIRPLANE_WING_Z = 1.86


def _airplane():
    """~8.0 m span / ~7.0 m long / 2.44 m tall high-wing propeller plane.

    Nose at y = -3.40, tail at y = +3.62, wheels on Z = 0.  The flying
    surfaces are flat open sheets declared with ``outward=("point", ref)``
    pointing into their own plane -- see :func:`_skin`.
    """
    a = C.Asset("veh_airplane", "vehicle")

    # ---- fuselage ---------------------------------------------------------
    body = a.part("airplane_body", base_color=WHITE, roughness=0.42)
    _loft_y(body.mesh, _AIRPLANE_STATIONS, WHITE, n_corner=2)
    # Albedo zoning: the runtime has no specular, so the belly, the shoulder
    # band and the accent stripe are the ONLY things that can separate two
    # adjacent polygons of this shell.
    _zone_band(body, lambda n, c: n[2] < -0.55, C.shade(WHITE, 0.52))
    _zone_band(body, lambda n, c: abs(n[2]) < 0.55 and c[2] < 1.24,
               C.shade(WHITE, 0.80))
    _zone_band(body, lambda n, c: abs(n[2]) < 0.60 and 1.24 <= c[2] < 1.40,
               CORAL)
    _zone_band(body, lambda n, c: n[2] > 0.55, C.shade(WHITE, 1.05))

    # rear bulkhead fairing so the tail does not end in a bare 10-gon
    _cbx(body.mesh, (0.16, 0.26, 0.20), (0.0, 3.60, 1.64), C.shade(WHITE, 0.9),
         bevel=0.03)

    # ---- cockpit glazing --------------------------------------------------
    glass = a.part("airplane_glass", base_color=GLASS, roughness=0.10,
                   metallic=0.30)
    _loft_y(glass.mesh, _AIRPLANE_CANOPY, GLASS, n_corner=2)
    for sy in (-1, 1):
        _cbx(glass.mesh, (0.62, 0.03, 0.16), (sy * 0.335, -1.20, 1.70), GLASS,
             bevel=0.006)

    # ---- main wing (OPEN, declared in its own plane) ----------------------
    # 8.0 m span, 1.62 m root chord tapering to 0.74 m, 0.53 m of sweep and a
    # little dihedral handled by the root fairings rather than by warping the
    # sheet -- see _skin(): a declared-plane part MUST stay planar.
    panels, ref = _wing_panels(4.00, 1.62, 0.74, _AIRPLANE_WING_Z,
                               -0.35, sweep=0.53, stations=4, color=CREAM)
    wing = a.part("airplane_wing", base_color=CREAM, roughness=0.46,
                  outward=("point", ref))
    for sx in (-1, 1):
        _skin(wing, _mirror_panels(panels, sx), ref)
    # Wing root fairings hide the razor edge where the sheet meets the spine.
    # They are CLOSED solids, so they cannot live in the wing part: a declared
    # ("point", ref) rule is applied to EVERY face of the part, and a fairing's
    # own inward-facing flanks point straight at the reference point sitting in
    # the sheet plane.  Closed geometry goes in its own part, where the
    # exporter's signed-volume test decides outwardness with no reference.
    fair = a.part("airplane_fairings", base_color=CREAM, roughness=0.46)
    for sx in (-1, 1):
        _cbx(fair.mesh, (0.52, 1.72, 0.20), (sx * 0.30, 0.46, 1.84), CREAM,
             bevel=0.045)

    # ---- tailplane (OPEN, its own plane) ----------------------------------
    tp, tref = _wing_panels(1.58, 0.96, 0.24, 1.50, 2.60, sweep=0.64,
                            stations=3, color=CREAM)
    tail = a.part("airplane_tailplane", base_color=CREAM, roughness=0.46,
                  outward=("point", tref))
    for sx in (-1, 1):
        _skin(tail, _mirror_panels(tp, sx), tref)

    # ---- vertical fin (OPEN, the x = 0 plane) ----------------------------
    fin_ref = (0.0, 2.60, 2.10)
    fin = a.part("airplane_fin", base_color=CORAL, roughness=0.46,
                 outward=("point", fin_ref))
    root = [(0.0, 2.05, 1.62), (0.0, 3.62, 1.66)]
    kink = [(0.0, 2.46, 2.10), (0.0, 3.40, 2.12)]
    tip = [(0.0, 2.82, 2.44), (0.0, 3.26, 2.45)]
    for a_ring, b_ring in ((root, kink), (kink, tip)):
        _skin(fin, [([a_ring[0], a_ring[1], b_ring[1], b_ring[0]], CORAL)],
              fin_ref)
    # the fin shoe is a closed solid, so it gets its own part (see the wing
    # root fairings above for why it cannot share the declared-plane part)
    _cbx(a.part("airplane_fin_shoe", base_color=CORAL, roughness=0.46).mesh,
         (0.14, 1.30, 0.14), (0.0, 2.90, 1.62), C.shade(CORAL, 0.8), bevel=0.03)

    # ---- control-surface shut lines --------------------------------------
    # One up-facing ("dir", +Z) part, so every strip in it must be horizontal.
    # A rudder line would have to live in the fin's vertical plane, which no
    # single direction can describe; the elevator covers the tail instead.
    lines = a.part("airplane_lines", base_color=C.shade(SLATE_DK, 0.9),
                   outward=("dir", (0.0, 0.0, 1.0)))
    for sx in (-1, 1):                      # ailerons, in the wing plane
        _lines_panel(lines, [(sx * 1.30, 0.86, _AIRPLANE_WING_Z),
                             (sx * 3.90, 0.96, _AIRPLANE_WING_Z),
                             (sx * 3.90, 1.00, _AIRPLANE_WING_Z),
                             (sx * 1.30, 0.92, _AIRPLANE_WING_Z)],
                    C.shade(SLATE_DK, 0.9))
    for sx in (-1, 1):                      # elevators, in the tailplane plane
        _lines_panel(lines, [(sx * 0.55, 3.34, 1.50), (sx * 1.50, 3.62, 1.50),
                             (sx * 1.50, 3.66, 1.50), (sx * 0.55, 3.38, 1.50)],
                    C.shade(SLATE_DK, 0.9))

    # ---- engine cowl, prop disc and blades --------------------------------
    cowl = a.part("airplane_engine", base_color=METAL_DK, roughness=0.34,
                  metallic=0.55)
    C.cylinder(cowl.mesh, 0.20, 0.18, 10, center=(0.0, -3.22, 1.42),
               color=METAL_DK, axis="Y")
    C.cone(cowl.mesh, 0.19, 0.30, 10, center=(0.0, -3.46, 1.42), color=CHROME,
           axis="Y")
    for sx in (-1, 1):                      # exhaust stubs
        C.cylinder(cowl.mesh, 0.045, 0.26, 6, center=(sx * 0.26, -2.92, 1.18),
                   color=SLATE_DK, axis="Y")

    prop = a.part("airplane_prop", base_color=SLATE_DK, roughness=0.22,
                  metallic=0.40)
    # a short closed cylinder reads as a motion-blurred disc edge-on and as
    # a solid disc face-on, and it is watertight, so no outward declaration.
    C.cylinder(prop.mesh, 0.80, 0.055, 12, center=(0.0, -3.30, 1.42),
               color=C.shade(SLATE_DK, 0.8), axis="Y")
    # two real blades, in the plane x = -3.34 (OPEN -> declared in-plane)
    pb_ref = (-3.34, 0.0, 1.42)
    pblades = a.part("airplane_prop_blades", base_color=GRAPHITE,
                     roughness=0.30, outward=("point", pb_ref))
    for sz in (-1, 1):
        _skin(pblades, [([(-3.34, -3.34 - 0.07, 1.42 + sz * 0.18),
                          (-3.34, -3.34 + 0.07, 1.42 + sz * 0.18),
                          (-3.34, -3.34 + 0.12, 1.42 + sz * 0.78),
                          (-3.34, -3.34 - 0.12, 1.42 + sz * 0.78)], GRAPHITE)],
               pb_ref)

    # ---- fixed tricycle undercarriage -------------------------------------
    gear = a.part("airplane_gear", base_color=CHROME, roughness=0.30,
                  metallic=0.65)
    _struts(gear.mesh, [
        ((0.0, -2.30, 0.22), (0.0, -2.24, 1.18)),
        ((-0.62, 0.78, 0.24), (-0.46, 0.72, 1.30)),
        ((0.62, 0.78, 0.24), (0.46, 0.72, 1.30)),
    ], 0.048, CHROME, seg=6)
    for sx in (-1, 1):                      # drag brace
        _struts(gear.mesh, [
            ((sx * 0.50, 0.76, 1.14), (sx * 0.30, 1.34, 1.66)),
        ], 0.028, CHROME, seg=5)
    C.cylinder(gear.mesh, 0.055, 0.80, 6, center=(0.0, 0.76, 1.06),
               color=CHROME, axis="X")

    tyres = a.part("airplane_tyres", base_color=RUBBER, roughness=0.90)
    hubs = a.part("airplane_hubs", base_color=METAL, roughness=0.34,
                  metallic=0.60)
    # The tyre is a cylinder centred at z = r, so it spans 0..2r: the wheels
    # already touch the ground exactly.  VERIFY, do not assume -- the verifier
    # only fails a vehicle that sinks BELOW y = 0, so a tyre authored a
    # millimetre high ships as a hovering aeroplane and nothing complains.
    for (x, y, r, hw) in ((0.0, -2.24, 0.22, 0.11),
                          (-0.70, 0.76, 0.26, 0.13),
                          (0.70, 0.76, 0.26, 0.13)):
        C.cylinder(tyres.mesh, r, hw * 2.0, 10, center=(x, y, r),
                   color=RUBBER, axis="X")
        C.cylinder(hubs.mesh, r * 0.46, hw * 2.3, 8, center=(x, y, r),
                   color=METAL, axis="X")

    # ---- lights and trim ---------------------------------------------------
    nav_p = a.part("airplane_nav_port", base_color=LAMP_RED, roughness=0.18,
                   emissive=(0.90, 0.08, 0.06))
    nav_s = a.part("airplane_nav_stbd", base_color=LAMP_GREEN, roughness=0.18,
                   emissive=(0.10, 0.86, 0.20))
    _cbx(nav_p.mesh, (0.20, 0.16, 0.09), (-3.92, 0.54, _AIRPLANE_WING_Z + 0.03),
         LAMP_RED, bevel=0.02)
    _cbx(nav_s.mesh, (0.20, 0.16, 0.09), (3.92, 0.54, _AIRPLANE_WING_Z + 0.03),
         LAMP_GREEN, bevel=0.02)
    beacon = a.part("airplane_beacon", base_color=LAMP_RED, roughness=0.18,
                    emissive=(0.90, 0.10, 0.08))
    C.sphere(beacon.mesh, 0.065, 8, 4, center=(0.0, 1.90, 1.86), color=LAMP_RED)

    antenna = a.part("airplane_antenna", base_color=SLATE_DK, roughness=0.5)
    _cbx(antenna.mesh, (0.03, 0.42, 0.10), (0.0, 0.10, 1.78), SLATE_DK,
         bevel=0.006)
    _cbx(antenna.mesh, (0.03, 0.16, 0.34), (0.0, -1.02, 2.02), SLATE_DK,
         bevel=0.006)
    # wing bracing wires
    _struts(antenna.mesh, [
        ((-0.30, -0.30, 1.72), (-3.60, 0.40, _AIRPLANE_WING_Z)),
        ((0.30, -0.30, 1.72), (3.60, 0.40, _AIRPLANE_WING_Z)),
    ], 0.016, SLATE_DK, seg=4)
    return _seat(a)


def _lines_panel(part, quad, color):
    """One up-facing shut-line strip (``outward=("dir", +Z)`` part)."""
    q = [tuple(p) for p in quad]
    n = C.normalize(C.cross(C.sub(q[1], q[0]), C.sub(q[2], q[0])))
    if n[2] < 0.0:
        q = list(reversed(q))
    part.mesh.quad(q[0], q[1], q[2], q[3], color)
    return part


# ==========================================================================
# 2.  JET -- delta-wing military fighter
# ==========================================================================

_JET_STATIONS = [
    (-6.00, 0.035, 0.045, 1.95, 0.015),
    (-5.50, 0.115, 0.130, 1.92, 0.050),
    (-4.90, 0.230, 0.250, 1.90, 0.090),
    (-4.10, 0.360, 0.360, 1.88, 0.120),
    (-3.10, 0.480, 0.440, 1.86, 0.130),
    (-1.80, 0.580, 0.490, 1.86, 0.130),
    (-0.40, 0.620, 0.510, 1.88, 0.130),
    (1.00, 0.590, 0.500, 1.92, 0.130),
    (2.30, 0.530, 0.470, 1.98, 0.120),
    (3.40, 0.460, 0.430, 2.04, 0.110),
    (4.30, 0.390, 0.390, 2.10, 0.100),
    (4.90, 0.330, 0.340, 2.14, 0.090),
]

_JET_CANOPY = [
    (-3.20, 0.100, 0.060, 2.18, 0.020),
    (-2.80, 0.260, 0.170, 2.24, 0.070),
    (-2.20, 0.360, 0.240, 2.30, 0.090),
    (-1.40, 0.400, 0.260, 2.32, 0.090),
    (-0.80, 0.360, 0.200, 2.28, 0.070),
]

_JET_WING_Z = 1.62
_JET_FIN_REF = (0.0, 3.10, 2.95)
_JET_STAB_Z = 2.40


def _jet():
    """~9.0 m delta span / ~11.2 m long / 3.45 m tall single-seat fighter.

    Nose at y = -6.00, nozzle at y = +5.20, wheels on Z = 0.
    """
    a = C.Asset("veh_jet", "vehicle")

    body = a.part("jet_body", base_color=SLATE, roughness=0.44)
    _loft_y(body.mesh, _JET_STATIONS, SLATE, n_corner=2)
    _zone_band(body, lambda n, c: n[2] < -0.55, C.shade(SLATE, 0.55))
    _zone_band(body, lambda n, c: abs(n[2]) < 0.55 and 1.72 < c[2] < 1.88,
               C.shade(SLATE, 0.74))
    _zone_band(body, lambda n, c: abs(n[2]) < 0.60 and 1.88 <= c[2] < 2.00,
               SLATE_DK)
    _zone_band(body, lambda n, c: n[2] > 0.55, C.shade(SLATE, 1.10))
    # spine panel + panel joints
    _zone_band(body, lambda n, c: n[2] > 0.55 and 0.0 < c[1] < 0.9,
               C.shade(SLATE, 0.86))

    glass = a.part("jet_glass", base_color=GLASS_TINT, roughness=0.08,
                   metallic=0.35)
    _loft_y(glass.mesh, _JET_CANOPY, GLASS_TINT, n_corner=2)
    _cbx(glass.mesh, (0.05, 1.20, 0.05), (0.0, -1.32, 2.56), SLATE_DK,
         bevel=0.010)                        # canopy sill

    # ---- delta wings (OPEN, declared in the z = 1.62 plane) ---------------
    # 9.0 m span, ~57 degrees of leading-edge sweep.  The root extension and
    # the fences are closed solids, so they go in their own part rather than
    # in the declared-plane sheet (see the airplane's wing fairings).
    wref = (0.0, 0.35, _JET_WING_Z)
    wing = a.part("jet_wings", base_color=SLATE, roughness=0.46,
                  outward=("point", wref))
    xs = (0.45, 2.20, 4.50)
    le = (-1.70, -0.90, 1.00)
    te = (2.40, 2.20, 1.85)
    for sx in (-1, 1):
        for i in range(len(xs) - 1):
            quad = [(sx * xs[i], le[i], _JET_WING_Z),
                    (sx * xs[i + 1], le[i + 1], _JET_WING_Z),
                    (sx * xs[i + 1], te[i + 1], _JET_WING_Z),
                    (sx * xs[i], te[i], _JET_WING_Z)]
            col = SLATE if i == 0 else C.shade(SLATE, 0.88)
            _skin(wing, [(quad, col)], wref)
    wr = a.part("jet_wing_roots", base_color=SLATE, roughness=0.46)
    for sx in (-1, 1):
        _cbx(wr.mesh, (0.90, 1.50, 0.16), (sx * 0.44, -1.30, _JET_WING_Z),
             C.shade(SLATE, 0.72), bevel=0.035)
        for yy in (0.10, 1.35):              # wing fences
            _cbx(wr.mesh, (0.06, 0.34, 0.20),
                 (sx * 2.30, yy, _JET_WING_Z + 0.08),
                 C.shade(SLATE, 1.05), bevel=0.012)

    # ---- single vertical fin (OPEN, the x = 0 plane) ---------------------
    fin = a.part("jet_fin", base_color=SLATE_DK, roughness=0.46,
                 outward=("point", _JET_FIN_REF))
    rings = [((1.80, 2.36), (4.35, 2.42)),
             ((2.60, 2.95), (4.20, 2.98)),
             ((3.15, 3.45), (4.05, 3.45))]
    for i in range(len(rings) - 1):
        (a0, a1), (b0, b1) = rings[i], rings[i + 1]
        _skin(fin, [([(0.0, a0[0], a0[1]), (0.0, a1[0], a1[1]),
                      (0.0, b1[0], b1[1]), (0.0, b0[0], b0[1])],
                     SLATE_DK if i == 0 else C.shade(SLATE_DK, 1.15))],
              _JET_FIN_REF)
    _cbx(a.part("jet_fin_shoe", base_color=SLATE_DK, roughness=0.46).mesh,
         (0.16, 2.10, 0.22), (0.0, 3.00, 2.34), SLATE_DK, bevel=0.04)
    # bright fin cap so the tail reads against the night sky
    _cbx(a.part("jet_fin_cap", base_color=CORAL, roughness=0.40).mesh,
         (0.17, 0.90, 0.10), (0.0, 3.58, 3.40), CORAL, bevel=0.02)

    # ---- all-moving horizontal stabilisers (OPEN, z = 2.40) --------------
    sref = (0.0, 3.30, _JET_STAB_Z)
    stab = a.part("jet_stabilisers", base_color=SLATE, roughness=0.46,
                  outward=("point", sref))
    for sx in (-1, 1):
        _skin(stab, [([(sx * 0.10, 2.60, _JET_STAB_Z),
                       (sx * 1.90, 3.30, _JET_STAB_Z),
                       (sx * 1.90, 4.00, _JET_STAB_Z),
                       (sx * 0.10, 4.15, _JET_STAB_Z)], SLATE)], sref)

    # ---- chin intake ------------------------------------------------------
    # The lip is a real wedge, not a slab: a quad extruded along a direction
    # lying IN its own plane has zero volume, so its side walls collapse to
    # zero-area triangles that core drops and the exporter then rejects.  Any
    # push must be perpendicular to the quad it extrudes.
    intake = a.part("jet_intake", base_color=GRAPHITE, roughness=0.50)
    _cbx(intake.mesh, (0.92, 1.70, 0.46), (0.0, -2.95, 1.63), GRAPHITE,
         bevel=0.05)
    _cbx(intake.mesh, (0.94, 0.26, 0.10), (0.0, -3.74, 1.63), C.shade(GRAPHITE, 1.5),
         bevel=0.025)                        # splitter lip
    for sx in (-1, 1):                       # inlet cavity walls
        _cbx(intake.mesh, (0.06, 0.30, 0.22), (sx * 0.30, -3.80, 1.66),
             C.shade(GRAPHITE, 1.4), bevel=0.010)
    _cbx(intake.mesh, (0.30, 0.24, 0.24), (0.0, -3.76, 1.66),
         C.shade(GRAPHITE, 0.55), bevel=0.010)   # dark duct mouth

    # ---- exhaust ----------------------------------------------------------
    pipe = a.part("jet_exhaust", base_color=METAL_DK, roughness=0.28,
                  metallic=0.75)
    C.cylinder(pipe.mesh, 0.36, 0.34, 10, center=(0.0, 5.02, 2.14),
               radius_top=0.30, color=METAL_DK, axis="Y")
    C.cylinder(pipe.mesh, 0.22, 0.40, 8, center=(0.0, 4.96, 2.14),
               color=SLATE_DK, axis="Y")
    burn = a.part("jet_burner", base_color=BURNT, roughness=0.20,
                  emissive=(0.98, 0.40, 0.06))
    C.cylinder(burn.mesh, 0.24, 0.05, 10, center=(0.0, 5.16, 2.14),
               color=BURNT, axis="Y")

    # ---- undercarriage ----------------------------------------------------
    gear = a.part("jet_gear", base_color=CHROME, roughness=0.30,
                  metallic=0.70)
    _struts(gear.mesh, [
        ((0.0, -3.20, 0.30), (0.0, -3.14, 1.48)),
        ((-0.92, 1.40, 0.32), (-0.74, 1.34, 1.56)),
        ((0.92, 1.40, 0.32), (0.74, 1.34, 1.56)),
    ], 0.050, CHROME, seg=6)
    C.cylinder(gear.mesh, 0.058, 1.90, 6, center=(0.0, 1.38, 1.42),
               color=CHROME, axis="X")
    _cbx(gear.mesh, (1.10, 0.90, 0.07), (0.0, 1.38, 1.30), SLATE_DK, bevel=0.02)

    tyres = a.part("jet_tyres", base_color=RUBBER, roughness=0.90)
    hubs = a.part("jet_hubs", base_color=METAL, roughness=0.34, metallic=0.60)
    for (x, y, r, hw) in ((0.0, -3.14, 0.26, 0.10),
                          (-0.96, 1.40, 0.30, 0.14),
                          (0.96, 1.40, 0.30, 0.14)):
        C.cylinder(tyres.mesh, r, hw * 2.0, 10, center=(x, y, r), color=RUBBER,
                   axis="X")
        C.cylinder(hubs.mesh, r * 0.48, hw * 2.3, 8, center=(x, y, r),
                   color=METAL, axis="X")
        _cbx(gear.mesh, (0.04, 0.40, 0.34), (x, y, r), SLATE_DK, bevel=0.012)

    # ---- lights -----------------------------------------------------------
    for nm, col, emis, pos in (
            ("jet_nav_port", LAMP_RED, (0.92, 0.08, 0.06), (-4.44, 1.42, _JET_WING_Z + 0.04)),
            ("jet_nav_stbd", LAMP_GREEN, (0.10, 0.88, 0.20), (4.44, 1.42, _JET_WING_Z + 0.04)),
            ("jet_tail_light", LAMP_WHITE, (0.90, 0.86, 0.70), (0.0, 4.12, 3.44))):
        p = a.part(nm, base_color=col, roughness=0.18, emissive=emis)
        _cbx(p.mesh, (0.16, 0.16, 0.08), pos, col, bevel=0.02)

    pitot = a.part("jet_pitot", base_color=METAL_DK, roughness=0.35)
    C.cylinder(pitot.mesh, 0.030, 0.44, 6, center=(0.0, -6.16, 1.95),
               color=METAL_DK, axis="Y")
    return _seat(a)


# ==========================================================================
# 3.  BOAT -- small open-cockpit motorboat
# ==========================================================================

# (y, half_beam_at_bilge, half_beam_at_gunwale, z_keel, z_gunwale,
#  deadrise, z_deck).  Peak beam is 2.00 m, which leaves the 2.2 m overall
#  width to the rub rail and the fenders hung outboard of it.
_BOAT_STATIONS = [
    (-3.00, 0.060, 0.080, 0.92, 1.05, 0.10, 1.04),
    (-2.55, 0.200, 0.320, 0.66, 1.05, 0.30, 1.00),
    (-1.95, 0.400, 0.660, 0.34, 1.05, 0.50, 0.86),
    (-1.20, 0.570, 0.870, 0.06, 1.06, 0.62, 0.72),
    (-0.20, 0.720, 0.980, 0.00, 1.08, 0.70, 0.68),
    (0.80, 0.780, 1.000, 0.00, 1.08, 0.70, 0.68),
    (1.70, 0.800, 1.000, 0.02, 1.06, 0.70, 0.74),
    (2.40, 0.780, 0.980, 0.08, 1.00, 0.62, 0.86),
]


def _field(col, y):
    """Linear interpolation of column ``col`` of the hull station table."""
    if y <= _BOAT_STATIONS[0][0]:
        return _BOAT_STATIONS[0][col]
    for a, b in zip(_BOAT_STATIONS, _BOAT_STATIONS[1:]):
        if a[0] <= y <= b[0]:
            t = (y - a[0]) / (b[0] - a[0])
            return a[col] + (b[col] - a[col]) * t
    return _BOAT_STATIONS[-1][col]


def _sheer(y):
    """(half_beam_at_gunwale, z_gunwale, z_deck) at station ``y``."""
    return _field(2, y), _field(4, y), _field(6, y)


def _hull_ring(hwb, hwg, zk, zg, dead, zdeck):
    """One closed hull cross-section, CLOCKWISE in (x, z).

    Clockwise in (x, z) means the ring's right-hand normal is +Y, which is
    the direction :func:`core.loft` advances in -- so the hull comes out
    outside-out with only the start cap flipped.  Nine points, always: the
    loft asserts equal cardinality between rings.
    """
    zb = zk + dead
    inset = min(0.10, hwg * 0.35)
    return [
        (0.0, zk),                          # keel
        (-hwb * 0.55, zk + dead * 0.42),     # port turn of the bilge
        (-hwb, zb),                         # port bilge
        (-hwg, zg),                         # port gunwale
        (-(hwg - inset), zdeck),            # port deck edge
        (hwg - inset, zdeck),               # starboard deck edge
        (hwg, zg),                          # starboard gunwale
        (hwb, zb),                          # starboard bilge
        (hwb * 0.55, zk + dead * 0.42),      # starboard turn
    ]


def _boat():
    """~6.0 m long / 2.20 m beam / 1.42 m tall open-cockpit speedboat.

    Bow at y = -3.00, outboard tip at y = +3.00, keel resting on Z = 0.
    Every deck-mounted fitting asks ``_sheer`` for the hull's own half-beam
    and gunwale height at its station, so nothing can drift off the flare.
    """
    a = C.Asset("veh_boat", "vehicle")

    hull = a.part("boat_hull", base_color=WHITE, roughness=0.34)
    rings = [[(px, y, pz) for (px, pz) in _hull_ring(hwb, hwg, zk, zg, dead, zd)]
             for (y, hwb, hwg, zk, zg, dead, zd) in _BOAT_STATIONS]
    C.loft(hull.mesh, rings, WHITE, cap_start_flip=True, cap_end_flip=False)
    # Paint zoning, in order, each one overwriting the last: boot-top, accent
    # stripe, then the topsides.  A hull is 90% one large smooth surface, so
    # without these bands the whole boat renders as a single dead value.
    _zone_band(hull, lambda n, c: n[2] < -0.55, C.shade(WHITE, 0.44))
    _zone_band(hull, lambda n, c: abs(n[2]) < 0.70 and c[2] < 0.13,
               C.shade(GRAPHITE, 1.0))
    _zone_band(hull, lambda n, c: abs(n[2]) < 0.70 and 0.13 <= c[2] < 0.27,
               CORAL)
    _zone_band(hull, lambda n, c: abs(n[2]) < 0.70 and 0.27 <= c[2] < 0.33,
               C.shade(MAGENTA, 1.0))
    _zone_band(hull, lambda n, c: n[2] > 0.55 and c[2] > 0.60,
               C.shade(WHITE, 0.80))

    # rub rail: one faceted tube per sheer line, the biggest silhouette win
    rail = a.part("boat_rail", base_color=CHROME, roughness=0.26, metallic=0.5)
    for sy in (-1, 1):
        path = [(sy * (hwg + 0.022), y, zg + 0.010)
                for (y, _b, hwg, _k, zg, _d, _c) in _BOAT_STATIONS]
        C.tube(rail.mesh, path, 0.038, 4, color=CHROME, caps=True, smooth=False)

    # ---- cockpit well -----------------------------------------------------
    hw, zg, zd = _sheer(0.20)
    well_hw = hw - 0.24
    sole = a.part("boat_sole", base_color=C.shade(SLATE, 0.8), roughness=0.72)
    _cbx(sole.mesh, (well_hw * 2.0 - 0.10, 2.20, 0.06), (0.0, 0.30, 0.68),
         C.shade(SLATE, 0.8), bevel=0.02)

    coam = a.part("boat_coaming", base_color=WHITE, roughness=0.30)
    for sy in (-1, 1):
        _cbx(coam.mesh, (0.10, 2.50, 0.36), (sy * well_hw, 0.22, 0.86), WHITE,
             bevel=0.035)
    _cbx(coam.mesh, (well_hw * 2.0, 0.11, 0.36), (0.0, 1.46, 0.86), WHITE,
         bevel=0.035)
    _cbx(coam.mesh, (well_hw * 2.0, 0.11, 0.30), (0.0, -1.06, 0.83), WHITE,
         bevel=0.035)
    # bow deck riser, closing the forward end of the well
    _cbx(coam.mesh, (hw - 0.30, 0.52, 0.22), (0.0, -1.44, 0.78),
         C.shade(WHITE, 0.92), bevel=0.04)
    # locker hatch on the foredeck
    _cbx(coam.mesh, (0.62, 0.52, 0.05), (0.0, -2.10, 0.90), C.shade(WHITE, 0.80),
         bevel=0.018)

    # ---- windscreen -------------------------------------------------------
    # a real raked pane, extruded along the screen's OWN normal
    screen = a.part("boat_screen", base_color=GLASS_TINT, roughness=0.08,
                    metallic=0.30)
    _slab(screen.mesh, [(-0.66, -0.92, 0.92), (0.66, -0.92, 0.92),
                        (0.50, -1.42, 1.42), (-0.50, -1.42, 1.42)],
          (0.0, -0.169, -0.106), GLASS_TINT)
    screen_frame = a.part("boat_screen_frame", base_color=CHROME, roughness=0.26,
                          metallic=0.6)
    for sx in (-1, 1):
        _struts(screen_frame.mesh, [
            ((sx * 0.68, -0.92, 0.92), (sx * 0.52, -1.42, 1.42))], 0.030, CHROME,
            seg=5)
    _cbx(screen_frame.mesh, (1.04, 0.06, 0.06), (0.0, -1.42, 1.42), CHROME,
         bevel=0.014)

    # ---- seats, console, wheel -------------------------------------------
    # One chamfered box per seat pan-and-back, no separate legs: at gameplay
    # distance the legs were never legible and cost 11 closed shells between
    # them, which is 5% of the whole asset's budget.
    trim = a.part("boat_trim", base_color=TEAL, roughness=0.44)
    for sy in (-1, 1):
        _cbx(trim.mesh, (0.42, 0.56, 0.54), (sy * 0.34, 1.04, 0.80), TEAL,
             bevel=0.05)
    _cbx(trim.mesh, (0.86, 0.30, 0.30), (0.0, 0.16, 0.84), C.shade(TEAL, 0.85),
         bevel=0.04)
    C.cylinder(trim.mesh, 0.17, 0.04, 6, center=(0.30, 0.22, 1.05),
               color=GRAPHITE, axis="Z")
    C.cylinder(trim.mesh, 0.035, 0.24, 6, center=(0.30, 0.22, 0.94),
               color=GRAPHITE)
    # bow grab rail, hung on the sheer rather than at a guessed offset
    ghw, gzg, _g = _sheer(-2.45)
    for sy in (-1, 1):
        _struts(trim.mesh, [
            ((sy * ghw * 0.80, -2.45, gzg + 0.02), (sy * ghw * 0.55, -2.20,
                                                     _field(4, -2.20) + 0.02))],
            0.026, CHROME, seg=4)

    # ---- outboard motor ---------------------------------------------------
    motor = a.part("boat_motor", base_color=SLATE_DK, roughness=0.30,
                   metallic=0.35)
    _cbx(motor.mesh, (0.52, 0.58, 0.56), (0.0, 2.68, 0.90), SLATE_DK, bevel=0.06)
    _cbx(motor.mesh, (0.44, 0.44, 0.18), (0.0, 2.86, 1.14), C.shade(SLATE_DK, 1.2),
         bevel=0.04)
    # transom bracket + tilt/steer housing
    _cbx(motor.mesh, (0.30, 0.22, 0.20), (0.0, 2.44, 0.86), C.shade(SLATE_DK, 0.8),
         bevel=0.03)
    # lower unit
    _cbx(motor.mesh, (0.22, 0.34, 0.52), (0.0, 2.76, 0.44), SLATE_DK, bevel=0.035)
    _cbx(motor.mesh, (0.14, 0.30, 0.24), (0.0, 2.86, 0.22), C.shade(SLATE_DK, 0.8),
         bevel=0.025)                        # skeg
    _cbx(motor.mesh, (0.18, 0.26, 0.05), (0.0, 2.74, 0.18), METAL_DK, bevel=0.012)
    # exhaust outlet
    _cbx(motor.mesh, (0.16, 0.06, 0.14), (0.0, 2.98, 0.92), GRAPHITE, bevel=0.012)

    prop = a.part("boat_prop", base_color=METAL, roughness=0.30, metallic=0.6)
    C.cylinder(prop.mesh, 0.13, 0.14, 6, center=(0.0, 2.96, 0.30), color=GRAPHITE,
               axis="Y")
    for k in range(3):                        # three cupped blades
        _cbx(prop.mesh, (0.04, 0.20, 0.11), (0.0, 2.96, 0.30), METAL, bevel=0.010,
             rot=(120.0 * k, 22.0, 0.0))

    # ---- lights, cleats, fenders ------------------------------------------
    lamp = a.part("boat_lamp", base_color=LAMP_RED, roughness=0.18,
                  emissive=(0.92, 0.10, 0.08))
    _cbx(lamp.mesh, (0.14, 0.14, 0.10), (0.0, 1.52, 1.30), LAMP_RED, bevel=0.02)
    for sx, col, nm in ((-1, LAMP_GREEN, "boat_nav_port"),
                        (1, LAMP_RED, "boat_nav_stbd")):
        p = a.part(nm, base_color=col, roughness=0.18,
                   emissive=(0.10, 0.86, 0.20) if col is LAMP_GREEN
                   else (0.92, 0.10, 0.08))
        _cbx(p.mesh, (0.12, 0.16, 0.08), (sx * _field(2, -1.90) * 0.9, -1.90,
                                          1.10), col, bevel=0.018)

    cleats = a.part("boat_cleats", base_color=CHROME, roughness=0.28,
                    metallic=0.6)
    for (y, z) in ((-2.40, 1.05), (2.30, 1.04)):
        for sy in (-1, 1):
            _struts(cleats.mesh, [
                ((sy * 0.26, y, z), (sy * 0.26, y, z + 0.09)),
                ((sy * 0.26, y, z + 0.09), (sy * 0.10, y, z + 0.09))],
                0.024, CHROME, seg=4)

    # two stub fenders, hung just outboard of the rub rail at their own
    # station: three cost 264 triangles and the third was never visible.
    fend = a.part("boat_fenders", base_color=CREAM, roughness=0.72)
    for (y, sy) in ((-1.20, -1), (1.30, -1)):
        hw_f = _field(2, y)
        C.cylinder(fend.mesh, 0.11, 0.44, 6,
                   center=(sy * (hw_f + 0.055), y, 0.86), color=CREAM, axis="Y")
        for dy in (-0.14, 0.14):
            _cbx(fend.mesh, (0.05, 0.05, 0.14),
                 (sy * (hw_f - 0.05), y + dy, 1.00), CREAM, bevel=0.01)
    return _seat(a)


# ==========================================================================
# 4.  HELICOPTER -- small utility rotorcraft
# ==========================================================================

# The cabin and the boom are ONE loft, so the fuselage has no waist seam: the
# stations run from the nose to the tail cone through a long, steady taper.
# The boom is kept short enough that the overall length stays near 11 m --
# a longer one is easier to author but reads as an oversized airliner rotor
# rather than a utility bird.
_HELI_STATIONS = [
    (-3.00, 0.090, 0.100, 1.75, 0.040),
    (-2.70, 0.320, 0.340, 1.74, 0.140),
    (-2.30, 0.550, 0.560, 1.72, 0.220),
    (-1.70, 0.780, 0.720, 1.72, 0.260),
    (-1.00, 0.920, 0.800, 1.72, 0.280),
    (-0.20, 0.960, 0.820, 1.74, 0.280),
    (0.60, 0.930, 0.800, 1.76, 0.280),
    (1.30, 0.840, 0.720, 1.78, 0.260),
    (1.90, 0.700, 0.620, 1.80, 0.220),
    (2.50, 0.560, 0.500, 1.82, 0.180),
    (3.20, 0.420, 0.400, 1.84, 0.140),
    (4.20, 0.330, 0.330, 1.86, 0.110),
    (5.40, 0.250, 0.260, 1.88, 0.080),
    (6.60, 0.175, 0.185, 1.90, 0.060),
    (7.10, 0.130, 0.140, 1.92, 0.050),
]

_HELI_CANOPY = [
    (-2.62, 0.140, 0.090, 2.06, 0.030),
    (-2.22, 0.340, 0.240, 2.14, 0.090),
    (-1.70, 0.500, 0.350, 2.19, 0.120),
    (-1.10, 0.560, 0.390, 2.21, 0.130),
    (-0.60, 0.520, 0.340, 2.18, 0.110),
]

_HELI_MAST_Y = -0.55
_HELI_ROTOR_Z = 3.10
_HELI_ROTOR_REF = (0.0, _HELI_MAST_Y, _HELI_ROTOR_Z)
_HELI_TAIL_X = -0.36
_HELI_TAIL_Y = 5.92
_HELI_TAIL_Z = 2.24
_HELI_TAIL_REF = (_HELI_TAIL_X, _HELI_TAIL_Y, _HELI_TAIL_Z)
_HELI_STAB_Z = 1.92


def _heli():
    """~10.0 m rotor / ~10.9 m long / 3.30 m tall utility helicopter.

    Nose at y = -3.00, tail cone at y = +7.90, skids on Z = 0.  The cabin
    and the boom are ONE loft so the fuselage has no waist seam; the rotor
    blades and the tail fin are open declared-plane sheets.
    """
    a = C.Asset("veh_helicopter", "vehicle")

    body = a.part("heli_body", base_color=SLATE_DK, roughness=0.46)
    _loft_y(body.mesh, _HELI_STATIONS, SLATE_DK, n_corner=2)
    # A dark airframe needs its accent band to do the silhouette work, so the
    # stripe is wide and bright and the rest is zoned a shade apart.
    _zone_band(body, lambda n, c: n[2] < -0.55, C.shade(SLATE_DK, 0.55))
    _zone_band(body, lambda n, c: abs(n[2]) < 0.60 and 1.44 < c[2] < 1.62,
               C.shade(SLATE_DK, 1.25))
    _zone_band(body, lambda n, c: abs(n[2]) < 0.70 and 1.62 <= c[2] < 2.02,
               AMBER)
    _zone_band(body, lambda n, c: n[2] > 0.55 and c[2] < 2.10,
               C.shade(SLATE_DK, 1.20))
    _zone_band(body, lambda n, c: n[2] > 0.55 and c[2] >= 2.10,
               C.shade(SLATE_DK, 0.85))

    glass = a.part("heli_glass", base_color=GLASS, roughness=0.08, metallic=0.35)
    _loft_y(glass.mesh, _HELI_CANOPY, GLASS, n_corner=2)
    for sy in (-1, 1):                      # sliding cabin door glass
        for y0 in (-0.95, 0.25):
            _slab(glass.mesh, [(sy * 0.90, y0, 1.90), (sy * 0.90, y0 + 0.55, 1.90),
                               (sy * 0.80, y0 + 0.55, 2.14),
                               (sy * 0.80, y0, 2.14)], (sy * -0.05, 0.0, 0.0),
                  GLASS)

    # ---- main rotor head ---------------------------------------------------
    head = a.part("heli_rotor_head", base_color=METAL_DK, roughness=0.30,
                  metallic=0.70)
    C.cylinder(head.mesh, 0.20, 0.16, 8, center=(0.0, _HELI_MAST_Y, 3.02),
               color=METAL_DK, axis="Z")
    C.cylinder(head.mesh, 0.26, 0.05, 8, center=(0.0, _HELI_MAST_Y, 2.86),
               color=CHROME, axis="Z")
    C.cylinder(head.mesh, 0.30, 0.09, 8, center=(0.0, _HELI_MAST_Y, 2.72),
               color=CHROME, axis="Z")
    C.cylinder(head.mesh, 0.12, 0.46, 8, center=(0.0, _HELI_MAST_Y, 2.52),
               color=METAL_DK, axis="Z")
    # engine deck fairing behind the mast
    _cbx(head.mesh, (0.90, 1.60, 0.30), (0.0, 0.62, 2.44), C.shade(SLATE_DK, 1.15),
         bevel=0.07)

    grips = a.part("heli_blade_grips", base_color=METAL, roughness=0.28,
                   metallic=0.7)
    blades = a.part("heli_rotor_blades", base_color=CHROME, roughness=0.34,
                    outward=("point", _HELI_ROTOR_REF))
    for k in range(4):
        ang = 90.0 * k + 18.0
        ca, sa = math.cos(math.radians(ang)), math.sin(math.radians(ang))
        r = [(0.34, 0.13), (1.30, 0.20), (5.00, 0.10)]   # (radius, half chord)
        pts = []
        for (rr, ch) in r:
            for sgn in (-1, 1):
                off = sgn * ch
                pts.append((rr * ca - off * sa, _HELI_MAST_Y + rr * sa
                            + off * ca, _HELI_ROTOR_Z))
        # ring order per station: LE, TE going outward, reversed at the tip
        for i in range(len(r) - 1):
            a0, a1, b0, b1 = pts[i * 2], pts[i * 2 + 1], pts[i * 2 + 2], pts[i * 2 + 3]
            col = C.shade(SLATE_DK, 0.9) if i == len(r) - 2 else CHROME
            _skin(blades, [([a0, b0, b1, a1], col)], _HELI_ROTOR_REF)
        # blade root cuff + pitch link
        _cbx(grips.mesh, (0.20, 0.20, 0.14),
             (0.34 * ca, _HELI_MAST_Y + 0.34 * sa, _HELI_ROTOR_Z), METAL,
             bevel=0.02, rot=(0.0, 0.0, ang))
        _cbx(grips.mesh, (0.09, 0.22, 0.05),
             (0.46 * ca, _HELI_MAST_Y + 0.46 * sa, _HELI_ROTOR_Z + 0.10), METAL_DK,
             bevel=0.012, rot=(0.0, 0.0, ang))

    # ---- tail: fin, stabiliser, rotor --------------------------------------
    fin_ref = (0.0, 6.20, 2.30)
    fin = a.part("heli_fin", base_color=SLATE_DK, roughness=0.46,
                 outward=("point", fin_ref))
    _skin(fin, [([(0.0, 5.30, 1.86), (0.0, 7.08, 1.94),
                  (0.0, 6.92, 2.78), (0.0, 5.98, 2.60)], SLATE_DK)], fin_ref)
    # closed fin shoe -> its own part (a declared-plane part admits no solids)
    _cbx(a.part("heli_fin_shoe", base_color=SLATE_DK, roughness=0.46).mesh,
         (0.13, 1.40, 0.18), (0.0, 6.25, 1.90), C.shade(SLATE_DK, 0.9), bevel=0.03)

    stab = a.part("heli_stabiliser", base_color=SLATE, roughness=0.46,
                  outward=("point", (0.0, 5.25, _HELI_STAB_Z)))
    for sx in (-1, 1):
        _skin(stab, [([(sx * 0.10, 4.70, _HELI_STAB_Z), (sx * 0.88, 5.18, _HELI_STAB_Z),
                       (sx * 0.88, 5.74, _HELI_STAB_Z), (sx * 0.10, 5.82, _HELI_STAB_Z)],
                      SLATE)], (0.0, 5.25, _HELI_STAB_Z))

    gearbox = a.part("heli_tail_gearbox", base_color=METAL_DK, roughness=0.30,
                     metallic=0.65)
    _cbx(gearbox.mesh, (0.46, 0.40, 0.40), (-0.18, _HELI_TAIL_Y, _HELI_TAIL_Z),
         METAL_DK, bevel=0.05)
    C.cylinder(gearbox.mesh, 0.10, 0.10, 6, center=(_HELI_TAIL_X, _HELI_TAIL_Y,
                                                   _HELI_TAIL_Z),
               color=GRAPHITE, axis="X")
    trotor = a.part("heli_tail_rotor", base_color=GRAPHITE, roughness=0.34,
                    outward=("point", _HELI_TAIL_REF))
    for k in range(2):
        ang = math.radians(90.0 * k + 20.0)
        ca, sa = math.cos(ang), math.sin(ang)
        tip = (_HELI_TAIL_X, _HELI_TAIL_Y + 0.46 * ca, _HELI_TAIL_Z + 0.46 * sa)
        root = (_HELI_TAIL_X, _HELI_TAIL_Y + 0.08 * ca, _HELI_TAIL_Z + 0.08 * sa)
        # a blade in the x = const plane: its faces point +-X, so the
        # reference point lying in that plane makes every dot exactly zero.
        ny, nz = -sa * 0.045, ca * 0.045
        quad = [(root[0], root[1] - ny, root[2] - nz), tip,
                (tip[0], tip[1] + ny, tip[2] + nz),
                (root[0], root[1] + ny, root[2] + nz)]
        _skin(trotor, [(quad, GRAPHITE)], _HELI_TAIL_REF)

    # ---- skids and struts ---------------------------------------------------
    # ONE strut pair per side, not two, and no separate cross tubes: the pair
    # alone was 352 triangles, and a landing skid is a single clean line in
    # silhouette -- the second pair only ever added clutter inside it.
    # The tube is walked through z = 0 exactly, so the skids rest on the
    # ground and the whole asset is seated by them.
    skids = a.part("heli_skids", base_color=METAL, roughness=0.30, metallic=0.55)
    for sx in (-1, 1):
        x = sx * 1.16
        C.tube(skids.mesh, [(x, -1.72, 0.17), (x, -1.48, 0.052),
                            (x, 1.20, 0.052), (x, 1.44, 0.18)],
               0.052, 5, color=METAL, caps=True, smooth=False)
        _struts(skids.mesh, [
            ((sx * 0.60, 0.00, 1.02), (x, -0.10, 0.09)),
            ((sx * 0.44, 0.34, 1.12), (x, 0.26, 0.09))],
            0.046, METAL, seg=5)
    # step
    for sx in (-1, 1):
        _cbx(skids.mesh, (0.26, 0.34, 0.04), (sx * 0.80, -0.85, 0.72), METAL_DK,
             bevel=0.012)

    # ---- exhaust and lights -------------------------------------------------
    exhaust = a.part("heli_exhaust", base_color=METAL_DK, roughness=0.36,
                     metallic=0.6)
    C.cylinder(exhaust.mesh, 0.10, 0.42, 6, center=(0.34, 0.90, 2.46),
               color=METAL_DK, axis="X")
    C.cylinder(exhaust.mesh, 0.12, 0.08, 6, center=(0.56, 0.90, 2.46),
               color=GRAPHITE, axis="X")

    for nm, col, emis, pos in (
            ("heli_nav_port", LAMP_RED, (0.92, 0.08, 0.06), (-0.94, -1.80, 1.62)),
            ("heli_nav_stbd", LAMP_GREEN, (0.10, 0.88, 0.20), (0.94, -1.80, 1.62)),
            ("heli_beacon", LAMP_RED, (0.95, 0.10, 0.08), (0.0, 6.90, 2.82)),
            ("heli_landing", LAMP_WHITE, (0.92, 0.88, 0.72), (0.0, -2.40, 1.46))):
        p = a.part(nm, base_color=col, roughness=0.18, emissive=emis)
        _cbx(p.mesh, (0.12, 0.14, 0.10), pos, col, bevel=0.02)

    # pitot boom and a nose sensor turret
    probe = a.part("heli_probe", base_color=GRAPHITE, roughness=0.40)
    C.cylinder(probe.mesh, 0.030, 0.46, 6, center=(0.0, -3.16, 1.80),
               color=GRAPHITE, axis="Y")
    C.sphere(probe.mesh, 0.16, 8, 5, center=(0.0, -2.92, 1.06), color=GRAPHITE,
             squash=0.7)
    return _seat(a)


# ==========================================================================
# assembly
# ==========================================================================

def build_all():
    """Return the ordered list of aviation and marine assets."""
    return [_airplane(), _jet(), _boat(), _heli()]
