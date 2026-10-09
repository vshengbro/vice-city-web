"""Military + heavy-maritime assets: tank, destroyer, sailing yacht, liner.

Four large kit-bashed blockwork vehicles in the same authoring space as
``vehicles.py`` and ``weapons.py``: Z-up, ``-Y`` is forward, metres, centred on
``X = 0``, and the origin rests on ``Z = 0`` at the bottom of the footprint
(the waterline for the three seagoing hulls -- see ORIGINS).

A deliberate difference from the street fleet: these are read at 60-200 m
across a bay or a battlefield, never at 2 m, so the job is a READABLE
SILHOUETTE rather than panel detail.  Every asset here is assembled from
prisms, lofts and faceted cylinders with a deliberately small number of
stations, which is what keeps a 260 m liner inside the 8,000-triangle
per-asset cap.

READABILITY LEVERS, in the order they matter at distance:

* **Slope.** A boat is a sloped hull; a tank is a sloped glacis.  Both read as
  their class from the profile alone.
* **Horizontal banding.** A warship's hull sides, a liner's deck edges and a
  tank's track run are all dark horizontal bands against lighter
  superstructures -- the same trick a city block uses with its storey bands.
* **Albedo zoning** through ``core.recolor_faces_where``.  The runtime shader
  is ``base * (ambient + light * max(dot(n, l), 0))`` with no specular and no
  texture (see ``vcw/CONTRACT.md`` section 1), so a large unbroken facet
  renders as one dead value.  Every big surface below is zoned: undersides
  drop hard, upward-facing decks brighten, and a boot-top stripe splits each
  hull at the waterline.
* **Value contrast on repeated elements.** Deck fittings, port-hole bands and
  road wheels are near-black against a mid grey, so a row of twenty of them
  still reads as twenty things rather than as texture.

ORIGINS.  The tank rests on ``Z = 0`` at the bottom of its tracks, so it sits
on the road.  The three hulls rest on ``Z = 0`` at their **waterline** -- the
keel is at ``Z = 0`` and the superstructure measures up from there -- so a
placement can put ``Z = 0`` on the sea surface and get a boat floating at its
proper draught with nothing drawn below the water plane (it is occluded by
it).  This is also what ``verify_assets.py`` check 7 requires: the
``vehicle`` category may not sink below ``y = 0``.

MATERIALS / PARTS.  Two or three parts per asset, never more.  The exporter
rejects duplicate part names outright, so anything appended after the main
pass goes through :func:`_part` rather than opening a second part of the same
name.

Nothing here is imported or traced: every triangle is emitted by the
``core`` kernel primitives.
"""

import math

from .. import core as C

# ----------------------------------------------------------------- palette --

# armour / tank
ARMOUR = (0.34, 0.36, 0.24)          # olive-drab hull
ARMOUR_LT = (0.44, 0.46, 0.32)       # upper plates catch the light
ARMOUR_DK = (0.21, 0.23, 0.16)       # shadowed sides and underside
STEEL_DK = (0.17, 0.18, 0.20)        # tracks, running gear
STEEL_MID = (0.30, 0.32, 0.35)       # barrel, road-wheel rims

# navy
NAVY = (0.38, 0.41, 0.45)            # hull topside grey
NAVY_DK = (0.25, 0.27, 0.30)         # shadowed superstructure
NAVY_BOOT = (0.14, 0.16, 0.20)       # boot-top / waterline band
DECK_GREY = (0.46, 0.48, 0.47)       # weather deck
DECK_LINE = (0.09, 0.10, 0.12)       # bulwarks, masts, fittings
ANTENNA = (0.52, 0.54, 0.56)
GUN_METAL = (0.24, 0.26, 0.29)
VLS_DK = (0.06, 0.07, 0.08)

# yacht
HULL_WHITE = (0.90, 0.90, 0.88)
HULL_DK = (0.72, 0.73, 0.72)         # topsides in shadow
HULL_BLUE = (0.14, 0.26, 0.42)       # boot-top stripe
TEAK = (0.60, 0.44, 0.24)            # cockpit sole
SAIL = (0.95, 0.95, 0.93)            # the brightest thing on the asset, so
SAIL_DK = (0.84, 0.84, 0.81)         # the silhouette is unambiguous
MAST = (0.78, 0.78, 0.75)

# cruise
LINER_WHITE = (0.90, 0.91, 0.92)
LINER_SHADE = (0.80, 0.82, 0.84)     # the tiers below the top deck
LINER_DK = (0.64, 0.67, 0.70)        # shaded slab sides
FUNNEL_RED = (0.72, 0.16, 0.14)
WINDOW_BAND = (0.10, 0.12, 0.15)     # window rows: near-black against white
BOOT_TOP = (0.12, 0.18, 0.26)

# shared accents
LAMP_AMBER = (1.00, 0.62, 0.10)


# ----------------------------------------------------------- local helpers --

def _part(a, name, **kw):
    """Get-or-create part ``name`` on ``a``.

    Detail passes run after the main body pass, so several of them append to a
    part the asset already made.  ``export.export_asset`` rejects duplicate
    part names outright, so a detail pass must never call ``asset.part`` for a
    name that already exists.
    """
    for p in a.parts:
        if p.name == name:
            if "base_color" in kw:
                p.base_color = tuple(kw["base_color"])
            if "emissive" in kw:
                p.emissive = tuple(kw["emissive"])
            return p
    return a.part(name, **kw)


def _slab(m, pts, push, color):
    """Closed 4-sided slab: the quad ``pts`` extruded by ``push``.

    ``pts`` is the OUTER face and the slab grows along ``push`` into the
    body.  ``core.loft`` advances ring-to-ring along the ring's own right-hand
    normal, so that normal must point along ``push``; this only fixes the
    corner order.  The outer face is then the flipped ``cap_start``.
    """
    pts = [tuple(p) for p in pts]
    p = C.normalize(push)
    n = C.normalize(C.cross(C.sub(pts[1], pts[0]), C.sub(pts[2], pts[0])))
    if C.dot(n, p) < 0.0:
        pts = list(reversed(pts))
    C.loft(m, [pts, [C.add(q, push) for q in pts]], color,
           cap_start_flip=True, cap_end_flip=False)
    return m


def _flat_quad_dir(m, quad, color, direction):
    """A planar quad with its normal forced to ``direction``.

    A decal laid on a flat skin has a normal the caller can state but cannot
    easily achieve: the corner order its arithmetic happens to produce is a
    coin flip, and one Part cannot carry two different outward directions.
    Normalising the winding against the DECLARED direction is what lets a
    part honestly declare ``outward=("dir", direction)`` instead of splitting
    one logical surface into two parts with opposite winding.

    ``direction`` must be normal to the quad's plane; the component of it
    along the plane is ignored, so callers can pass a loose hint.
    """
    d = C.normalize(direction)
    n = C.normalize(C.cross(C.sub(quad[1], quad[0]), C.sub(quad[2], quad[0])))
    if C.dot(n, d) < 0.0:
        quad = list(reversed(quad))
    m.quad(quad[0], quad[1], quad[2], quad[3], color)
    return m


def _hull_rings(stations, n_corner=2, reverse=True):
    """Rounded-rect box sections for a hull, from a (y, hb, z0, z1, r) table.

    Every hull in this module is ONE closed loft rather than a stack of
    hand-placed boxes, and every one of them shares this helper.

    HANDEDNESS.  A ``rounded_rect`` profile is CCW in its own plane, so its
    right-hand normal is ``X x Z = -Y``.  ``core.loft`` advances ring to ring
    along that right-hand normal -- otherwise the shell comes out inside-out.
    So the ring list is emitted in DESCENDING ``y`` (stern to bow) whatever
    order the control table is written in; the tables read naturally
    bow-first, and this is the one place that has to know about it.

    THE RADIUS CLAMP IS NOT COSMETIC.  ``core.rounded_rect`` closes its loop
    by walking the last corner arc all the way to 2*pi, where it lands on
    exactly the first point of the first arc -- and pops it.  That pop only
    happens when ``r`` has been clamped to the half extent itself (the two
    points are ``(hx, hy - r)`` and ``(hx, -(hy - r))``, identical iff
    ``hy == r``).  So a station whose corner radius reaches its own half
    extent comes back with one point FEWER than every other station, and
    ``core.loft`` rejects the whole run with "rings must have equal point
    counts".  Clamping strictly inside -- 0.98 rather than 0.9 -- and never
    with a lower bound that can push it back out, keeps the cardinality
    constant across every station of every hull.
    """
    rows = list(stations)
    if reverse:
        rows = rows[::-1]
    rings = []
    for (y, hb, z0, z1, r) in rows:
        hz = (z1 - z0) * 0.5
        lim = min(hb, hz)
        if lim <= 1e-6:
            raise ValueError("hull station at y=%r has a zero half extent"
                             % y)
        lift = (z1 + z0) * 0.5
        prof = C.rounded_rect(hb, hz, min(r, lim * 0.98), n_corner)
        rings.append([(px, y, lift + py) for (px, py) in prof])
    return rings


def _loft_hull(part, rings, color):
    """Loft a hull shell from ``_hull_rings`` output, with the caps right.

    The rings advance along ``-Y``, so the first ring sits at the stern and
    its outward face is ``+Y`` (unflipped) while the last ring's is ``-Y``
    (flipped) -- the same cap convention ``core.cylinder`` uses for a ring
    stack advancing along its own right-hand normal.

    The signed volume is then MEASURED rather than assumed: if the station
    order or the profile winding ever changes underneath this, the shell is
    re-wound here at build time instead of arriving at the exporter as a
    negative-volume shell.  This is a self-check, not a workaround -- it does
    not disable any validation, it just puts the one convention that is easy
    to get wrong in exactly the one place that owns it.
    """
    C.loft(part.mesh, rings, color, cap_start_flip=False, cap_end_flip=True)
    if C.signed_volume(part.mesh) <= 0.0:
        part.mesh.faces = [(a, c, b) for (a, b, c) in part.mesh.faces]
    return part


def _merge_rot(part, tmp, center=(0.0, 0.0, 0.0), rot=()):
    """Merge a temporary mesh after a chain of (axis, degrees) rotations.

    Same contract as ``vehicles._merge_xform``: the rotation matrices are
    orthogonal with determinant +1, so a rotated primitive keeps its winding
    and its signed volume.
    """
    m = part.mesh
    base = len(m.pos)
    for p in tmp.pos:
        q = p
        for axis, deg in rot:
            a = math.radians(deg)
            ca, sa = math.cos(a), math.sin(a)
            if axis == "X":
                q = (q[0], q[1] * ca - q[2] * sa, q[1] * sa + q[2] * ca)
            elif axis == "Y":
                q = (q[0] * ca + q[2] * sa, q[1], -q[0] * sa + q[2] * ca)
            else:
                q = (q[0] * ca - q[1] * sa, q[0] * sa + q[1] * ca, q[2])
        m.pos.append((q[0] + center[0], q[1] + center[1], q[2] + center[2]))
    for f in tmp.faces:
        m.faces.append((f[0] + base, f[1] + base, f[2] + base))
    m.fcol.extend(tmp.fcol)
    return part


# ============================================================================
# 1. MAIN BATTLE TANK  -- 7.0 m long, 3.30 m wide, 2.42 m tall
# ============================================================================

# Hull silhouette control rows, nose (-Y) to tail (+Y), as
# (y, half_width, z_bottom, z_top, corner_radius).  The glacis is the slope
# from row 1 to row 2: that single facet is what makes the profile read as a
# tank rather than as a box on tracks.
_TANK_HULL = [
    (-3.50, 0.62, 0.46, 0.52, 0.10),   # nose tip
    (-3.10, 1.05, 0.42, 0.86, 0.14),   # glacis base
    (-2.20, 1.28, 0.40, 1.16, 0.16),   # full-width front deck
    (-0.60, 1.32, 0.40, 1.24, 0.18),   # full-width mid hull
    (0.90, 1.32, 0.40, 1.24, 0.18),    # under the turret ring
    (2.10, 1.28, 0.42, 1.18, 0.16),    # engine deck
    (2.90, 1.14, 0.44, 1.10, 0.14),    # tail
    (3.50, 0.86, 0.48, 0.98, 0.12),    # tail cap
]


def _tank_hull_rings():
    """Rounded-rect box sections along Y for the tank hull.

    A control table plus a loft, exactly as ``vehicles.py`` does it, rather
    than a stack of hand-placed boxes.  The corner radius is clamped per
    station so the arcs can never collapse and change the ring cardinality,
    which ``core.loft`` rejects.
    """
    return _hull_rings(_TANK_HULL)


def _wheel(m, x, y, radius, width):
    """One tank running-gear wheel, resting on the ground at ``z = radius``.

    The centre height IS the radius: a wheel of radius ``r`` tangent to the
    road plane at Z = 0 has its axis at ``z = r``.  Writing the two
    independently is what let a 0.34 sprocket sit on a 0.29 axis and sink
    50 mm through the street, so this helper refuses to express it.
    """
    C.cylinder(m, radius, width, seg=12, center=(x, y, radius),
               color=STEEL_MID, axis="Y")
    return m


# The track band's tube radius, and therefore the height its centreline rides
# at.  One number, because the path height and the sweep radius have to agree
# for the tank to rest on Z = 0 -- see ``_tank_track_path``.
TRACK_R = 0.20


def _tank_track_path(r):
    """Track centreline: a stadium in the YZ plane, one tube-radius up.

    Flat on the ground between the idler and the sprocket, up the front arc,
    over the top run and back down.  Sampling it as a polyline is what keeps
    the swept band's LOWEST point exactly on ``Z = 0`` -- the path rides at
    ``z = r`` precisely because ``core.tube`` sweeps a circle of radius ``r``
    around it, so a centreline authored AT ``z = 0`` would bury half the track
    band under the road.  (The first draft of this file used ``z = 0.44``
    with a separate hard-coded tube radius of ``0.13``; tying the two to one
    variable is what makes the contact patch and the path impossible to
    disagree.)
    """
    y0, y1 = -2.90, 2.70
    path = []
    for i in range(7):                                   # bottom run
        path.append((y0 + (y1 - y0) * (i / 6.0), 0.0, r))
    for i in range(1, 7):                                # rear sprocket
        t = math.pi * 0.5 * (i / 6.0)
        path.append((y1 + r * math.sin(t), 0.0, r + r * math.cos(t)))
    for i in range(1, 7):                                # top run
        path.append((y1 + (y0 - y1) * (i / 6.0), 0.0, 2.0 * r))
    for i in range(1, 7):                                # front idler
        t = math.pi * 0.5 * (i / 6.0)
        path.append((y0 - r * math.sin(t), 0.0, r + r * math.cos(t)))
    return path


def _tank():
    """Main battle tank: sloped glacis, twin tracks, turret, long barrel.

    Origin is on the ground at the bottom of the tracks (``Z = 0``), so the
    asset drops straight onto the road plane.  The barrel runs along ``-Y``,
    the authoring forward axis.
    """
    a = C.Asset("wep_tank", "vehicle")

    # ---- hull: one lofted closed shell, zoned by normal afterwards --------
    hull = a.part("hull", base_color=ARMOUR, roughness=0.70)
    _loft_hull(hull, _tank_hull_rings(), ARMOUR)
    C.recolor_faces_where(hull, lambda n, c: n[2] < -0.5, ARMOUR_DK)
    C.recolor_faces_where(hull, lambda n, c: n[2] > 0.5, ARMOUR_LT)
    # the glacis is the silhouette carrier, so it must not sink into the flanks
    C.recolor_faces_where(
        hull, lambda n, c: n[1] < -0.45 and c[2] < 1.05, ARMOUR_LT)
    # a dark skirt band along both flanks, just above the tracks
    C.recolor_faces_where(
        hull, lambda n, c: abs(n[2]) < 0.5 and c[2] < 0.72, ARMOUR_DK)

    gear = a.part("gear", base_color=STEEL_DK, roughness=0.55)
    metal = a.part("metal", base_color=STEEL_MID, roughness=0.40, metallic=0.50)

    # ---- tracks: a faceted swept band per side ----------------------------
    # ``TRACK_R`` is the tube radius, and it is the SAME number the centreline
    # is built from (see ``_tank_track_path``), so the swept band's lowest
    # point lands exactly on Z = 0 -- verify_assets check 7 requires a vehicle
    # to rest on y = 0, and a hard-coded 0.44 centreline against a hard-coded
    # 0.13 radius is how that check gets failed by 50 mm.
    #
    # AXIS ORDER MATTERS HERE.  ``_tank_track_path`` returns (along, 0, up) --
    # component 0 runs down the tank's LENGTH (y), because that is the axis the
    # stadium shape is described on.  It therefore has to be emitted as the
    # path's Y, with the per-side lateral offset on X:
    #
    #     (sy * 1.36, p[0], p[2])     <- correct
    #     (p[0], sy * 1.36, p[2])     <- crossed the hull
    #
    # The crossed spelling looked fine because both terms are numbers and the
    # tube still swept a closed band on the ground.  Measured, though, the band
    # came out 6.36 m long in X against a hull only 2.64 m wide, so each band
    # speared clean through the hull and stuck out 3 m to port and starboard,
    # while the five road wheels sat on a completely separate 5.92 m span with
    # nothing enclosing them.  That is the "wheels read as floating blobs with
    # no continuous track" symptom: it was never about the band's thickness.
    # `check_track.py` in tools/blender guards the orientation.
    for sy in (-1, 1):
        C.tube(gear.mesh,
               [(sy * 1.36, p[0], p[2]) for p in _tank_track_path(TRACK_R)],
               TRACK_R, seg=6, color=STEEL_DK, caps=True, smooth=False)

    # ---- road wheels, sprocket, idler, return rollers ---------------------
    # Five road wheels per side plus sprocket and idler, in a lighter metal
    # against the near-black track: that value break is what makes a track run
    # read as running gear at 60 m instead of as a plain black rectangle.
    #
    # Five, not six, because at ``TRACK_R = 0.20`` the swept band's vertical
    # extent is 3 * 0.20 = 0.60, which now stands PROUD of the road wheels'
    # top at 2 * 0.29 = 0.58.  At the old 0.13 the band only reached 0.39 and
    # every wheel poked 0.19 m above it, so the wheels owned the silhouette
    # and the band read as a thin dark smear behind them.  A sixth wheel only
    # added more of the thing that was dominating the read.
    #
    # EVERY WHEEL IS CENTRED ON ITS OWN RADIUS.  A wheel tangent to the ground
    # at Z = 0 has its axis at z = r, so the centre height and the radius
    # cannot be independent numbers.  The first draft hard-coded the centre at
    # the road-wheel radius and then drew a LARGER sprocket and idler on the
    # same axis, which put their bottom at 0.29 - 0.34 = -0.05 and failed
    # verify_assets check 7 (a vehicle may not sink below y = 0) with the
    # track band itself resting correctly on 0.  One helper now ties the two.
    wheel_r = 0.29
    # Five wheels on the SAME -2.15 .. +2.15 span the six-wheel run of 0.86
    # already covered, just at an even 1.075 stride with one gap less to fill.
    # Keeping the span is the point: dropping to range(5) without respacing
    # would have pulled the last wheel in to y = 0.43 and left a 1.7 m bare
    # stretch of track under the engine deck.
    for sy in (-1, 1):
        for i in range(5):
            _wheel(metal.mesh, sy * 1.36, -2.15 + i * 1.075, wheel_r, 0.30)
        _wheel(metal.mesh, sy * 1.36, 2.70, 0.34, 0.32)     # drive sprocket
        _wheel(metal.mesh, sy * 1.36, -2.90, 0.34, 0.32)    # front idler
        for i in range(3):                                   # return rollers
            C.cylinder(metal.mesh, 0.16, 0.26, seg=8,
                       center=(sy * 1.36, -1.30 + i * 1.30, 0.74),
                       color=STEEL_MID, axis="Y")

    # ---- turret: a chamfered wedge at y = +1.40, barrel along -Y ----------
    # y=+1.40 is amidships, so the turret sits centred on the hull ring and
    # the barrel overhangs the glacis by about 1.2 m -- the readable ratio
    # that says "main battle tank" from the side.
    tur = a.part("turret", base_color=ARMOUR_LT, roughness=0.66)
    C.loft(tur.mesh,
           [[(-0.78, 0.70, 1.22), (0.78, 0.70, 1.22),
             (0.78, 2.02, 1.22), (-0.78, 2.02, 1.22)],
            [(-0.94, 1.10, 1.62), (0.94, 1.10, 1.62),
             (0.94, 2.14, 1.62), (-0.94, 2.14, 1.62)],
            [(-0.86, 1.16, 1.96), (0.86, 1.16, 1.96),
             (0.86, 2.10, 1.10), (-0.86, 2.10, 1.10)]],
           ARMOUR_LT, cap_start_flip=True, cap_end_flip=False)
    C.recolor_faces_where(tur, lambda n, c: n[2] > 0.5, C.shade(ARMOUR_LT, 1.08))

    # mantlet: the armoured collar the barrel emerges from
    C.chamfer_box(tur.mesh, (1.30, 0.34, 1.06), center=(0.0, 0.78, 1.58),
                  color=ARMOUR_LT, bevel=0.07)
    C.chamfer_box(tur.mesh, (1.10, 0.30, 0.90), center=(0.0, 0.64, 1.58),
                  color=ARMOUR, bevel=0.07)
    # commander's cupola, offset to the loader's right (+X) so the roof line
    # is asymmetric and the tank is recognisable from either flank
    C.cylinder(tur.mesh, 0.40, 0.34, seg=8, center=(0.34, 1.72, 2.09),
               color=ARMOUR, axis="Z")
    C.cylinder(tur.mesh, 0.34, 0.16, seg=8, center=(0.34, 1.72, 2.32),
               color=ARMOUR_DK, axis="Z")
    C.chamfer_box(tur.mesh, (0.44, 0.16, 0.30), center=(0.34, 1.36, 2.26),
                  color=ARMOUR_DK, bevel=0.04)          # vision block
    C.cylinder(tur.mesh, 0.26, 0.10, seg=8, center=(-0.34, 1.72, 2.02),
               color=ARMOUR_DK, axis="Z")              # loader's hatch
    C.chamfer_box(tur.mesh, (0.52, 0.90, 0.34), center=(-0.80, 1.92, 1.42),
                  color=ARMOUR_DK, bevel=0.04)         # stowage bin
    # antenna: the vertical accent that stops the turret reading as a lump
    C.cylinder(metal.mesh, 0.026, 1.30, seg=4, center=(-0.72, 2.00, 2.10),
               color=STEEL_MID, axis="Z")

    # ---- barrel: axis-Y cylinders, muzzle forward -------------------------
    # Laying the tube on the axis-Y branch of ``core.cylinder`` is the one
    # place the Y-axis handedness matters; it is verified positive-volume on
    # this kernel revision, and the exporter's closed-shell volume check would
    # reject it loudly if that ever regressed.
    C.cylinder(metal.mesh, 0.115, 3.40, seg=10, center=(0.0, -0.95, 1.58),
               color=STEEL_MID, axis="Y", radius_top=0.098)
    C.cylinder(metal.mesh, 0.150, 0.30, seg=10, center=(0.0, -2.52, 1.58),
               color=GUN_METAL, axis="Y")              # muzzle collar
    C.cylinder(metal.mesh, 0.105, 0.24, seg=8, center=(0.0, -2.72, 1.58),
               color=STEEL_MID, axis="Y")              # bore

    # ---- running detail: glacis blocks, exhaust, headlight ---------------
    detail = a.part("detail", base_color=ARMOUR_DK, roughness=0.50)
    for sy in (-1, 1):
        for i in range(3):                              # spare-track blocks
            C.box(detail.mesh, (0.34, 0.09, 0.22),
                  center=(sy * 0.72, -3.18 + i * 0.10, 0.62 + i * 0.09),
                  color=ARMOUR_DK)
        C.cylinder(detail.mesh, 0.075, 0.07, seg=8,
                   center=(sy * 0.86, -3.30, 1.00), color=STEEL_MID, axis="Y")
    lamp = a.part("lamps", base_color=LAMP_AMBER, roughness=0.22,
                  emissive=(0.85, 0.42, 0.05))
    for sy in (-1, 1):
        C.cylinder(lamp.mesh, 0.058, 0.05, seg=8,
                   center=(sy * 0.86, -3.36, 1.00), color=LAMP_AMBER, axis="Y")
    C.box(detail.mesh, (1.10, 0.08, 0.34), center=(0.0, 3.44, 0.80),
          color=ARMOUR_DK)                              # exhaust grille
    C.box(detail.mesh, (0.20, 0.16, 0.14), center=(0.0, 3.48, 0.56),
          color=STEEL_MID)                              # towing eye
    return a


# ============================================================================
# 2. NAVY DESTROYER  -- 140 m long, 16 m wide, ~25 m tall overall
# ============================================================================

# Hull stations, stern (+Y) to bow (-Y), as (y, half_beam, z_bottom, z_deck,
# corner_radius).  ``Z = 0`` IS the waterline: ``z_bottom`` is the keel/bilge
# height above it, so the hull's lower body is a shallow skirt and nothing is
# ever drawn below the sea plane.
_DESTROYER_STATIONS = [
    # y,     half_beam, z_bottom, z_deck, radius
    (70.0, 2.4, 2.6, 7.4, 0.8),       # transom
    (56.0, 5.6, 1.6, 7.6, 1.2),
    (40.0, 7.0, 1.0, 7.8, 1.6),
    (20.0, 7.8, 0.8, 8.0, 1.8),       # full beam
    (0.0, 8.0, 0.8, 8.1, 1.8),        # amidships
    (-20.0, 7.9, 0.9, 8.2, 1.8),
    (-38.0, 7.2, 1.3, 8.3, 1.6),
    (-52.0, 5.6, 2.0, 8.4, 1.2),
    (-62.0, 3.6, 3.0, 8.4, 0.9),
    (-68.0, 1.6, 4.2, 8.4, 0.5),      # sharp entry
    (-70.0, 0.35, 5.0, 8.4, 0.2),     # stem
]

# Superstructure blocks, measured up from the weather deck (z = 8.1).  A
# destroyer's silhouette is a stack of narrowing blocks with a tall thin
# lattice mast, and this is the only stack in the asset, so it carries the
# ship's whole read.
_DESTROYER_TIERS = [
    # y0,  y1,  half_width, z_base, z_top, color
    (-34.0, 4.0, 6.6, 8.10, 12.60, NAVY),     # forward deckhouse
    (-14.0, 16.0, 6.2, 8.10, 12.90, NAVY),    # main bridge block
    (16.0, 34.0, 6.0, 8.10, 12.20, NAVY),     # aft deckhouse
    (36.0, 50.0, 5.4, 8.10, 11.60, NAVY),     # aft house
]


def _destroyer_hull_rings():
    """Closed hull shell: a lofted run of box sections, stern to bow.

    The station table is walked from stern to bow, so the rings advance along
    ``-Y`` -- the OPPOSITE of ``core.section_rings``, which always advances
    along ``+X`` and whose profiles are CCW in their own (Y, Z) plane.  Here
    the profiles are CCW in (X, Z), whose right-hand normal is ``-Y``; that
    matches the run direction, so ``cap_start_flip``/``cap_end_flip`` are set
    for the standard case and :func:`_destroyer` measures the signed volume
    once as a self-check rather than trusting the convention from memory.
    """
    return _hull_rings(_DESTROYER_STATIONS)


def _beam_at(y):
    """Half-beam of the destroyer hull at station ``y`` (linear in the table)."""
    tab = _DESTROYER_STATIONS
    if y >= tab[0][0]:
        return tab[0][1]
    for i in range(len(tab) - 1):
        ya, yb = tab[i][0], tab[i + 1][0]
        if ya >= y >= yb:
            t = (ya - y) / (ya - yb)
            return tab[i][1] + (tab[i + 1][1] - tab[i][1]) * t
    return tab[-1][1]


def _destroyer():
    """Naval destroyer: raked bow, grey hull, bridge tower, funnel, mast.

    The hull is ONE closed loft: ``core.loft`` fans both end caps, so the
    shell is watertight and its signed volume is checked by the exporter with
    no declared ``outward`` at all.
    """
    a = C.Asset("ship_destroyer", "vehicle")

    hull = a.part("hull", base_color=NAVY, roughness=0.55)
    _loft_hull(hull, _destroyer_hull_rings(), NAVY)
    C.recolor_faces_where(hull, lambda n, c: n[2] < -0.4, NAVY_DK)
    C.recolor_faces_where(hull, lambda n, c: n[2] > 0.4, DECK_GREY)
    # waterline band: a dark boot-top stripe that splits the hull at Z = 0 and
    # stops the topside grey from running as one dead value down 140 m
    C.recolor_faces_where(
        hull, lambda n, c: c[2] < 2.6 and abs(n[2]) < 0.86, NAVY_BOOT)
    C.recolor_faces_where(hull, lambda n, c: c[2] < 1.1, C.shade(NAVY_BOOT, 0.7))

    # ---- weather deck + bulwarks -----------------------------------------
    deck = a.part("deck", base_color=DECK_GREY, roughness=0.72)
    for (y0, y1, hw, _z0, _z1, _c) in _DESTROYER_TIERS:
        C.box(deck.mesh, (hw * 2.0, y1 - y0, 0.40),
              center=(0.0, (y0 + y1) * 0.5, 8.10), color=DECK_GREY)
    bul = a.part("bulwark", base_color=DECK_LINE, roughness=0.60)
    for i in range(len(_DESTROYER_STATIONS) - 1):
        ya, yb = _DESTROYER_STATIONS[i][0], _DESTROYER_STATIONS[i + 1][0]
        za, zb = _DESTROYER_STATIONS[i][3], _DESTROYER_STATIONS[i + 1][3]
        ha, hbb = _DESTROYER_STATIONS[i][1], _DESTROYER_STATIONS[i + 1][1]
        for sy in (-1, 1):
            # a low wall set just inboard of the hull side, following the
            # sheer: sampled off the station table so it can never drift off
            # the hull's own beam
            ym = (ya + yb) * 0.5
            C.box(bul.mesh, (0.34, abs(yb - ya) + 0.10, 1.15),
                  center=(sy * (_beam_at(ym) - 0.20), ym, (za + zb) * 0.5 + 0.50),
                  color=DECK_LINE)
    # transom, so the stern does not read as an open box
    C.box(bul.mesh, (9.0, 0.40, 6.6), center=(0.0, 69.7, 5.10), color=NAVY)

    # ---- superstructure tiers --------------------------------------------
    house = a.part("house", base_color=NAVY, roughness=0.52)
    for (y0, y1, hw, z0, z1, col) in _DESTROYER_TIERS:
        C.box(house.mesh, (hw * 2.0, y1 - y0, z1 - z0),
              center=(0.0, (y0 + y1) * 0.5, (z0 + z1) * 0.5), color=col)
    # bridge wings: the overhanging platform that gives a warship its width
    C.box(house.mesh, (14.4, 3.4, 0.50), center=(0.0, 6.0, 13.20), color=NAVY)
    C.box(house.mesh, (13.4, 2.8, 0.32), center=(0.0, 6.0, 13.70), color=NAVY)
    # funnels: two raked boxes, the tallest things aft
    C.box(house.mesh, (5.2, 5.0, 7.4), center=(0.0, 30.0, 16.30), color=NAVY_DK)
    C.box(house.mesh, (5.8, 2.4, 0.9), center=(0.0, 30.0, 20.30), color=GUN_METAL)
    C.box(house.mesh, (4.2, 4.2, 6.0), center=(0.0, 43.0, 15.20), color=NAVY_DK)
    C.box(house.mesh, (4.8, 2.0, 0.8), center=(0.0, 43.0, 18.50), color=GUN_METAL)
    # 76 mm gun: a faceted turret on the foredeck
    C.chamfer_box(house.mesh, (3.6, 4.2, 2.6), center=(0.0, -46.0, 9.70),
                  color=NAVY, bevel=0.35)
    metal = a.part("metal", base_color=GUN_METAL, roughness=0.42, metallic=0.45)
    C.cylinder(metal.mesh, 0.26, 4.6, seg=8, center=(0.0, -51.4, 10.20),
               color=GUN_METAL, axis="Y")
    C.cylinder(metal.mesh, 0.34, 0.40, seg=8, center=(0.0, -53.4, 10.20),
               color=GUN_METAL, axis="Y")
    # VLS cells: a grid of near-black rectangles -- texture that costs nothing
    for i in range(4):
        for j in range(4):
            C.box(house.mesh, (0.92, 0.90, 0.20),
                  center=(-1.35 + i * 0.90, -32.0 + j * 0.90, 12.72), color=VLS_DK)
    # ship's boat in its davit, aft: a light mass against the dark deck
    C.cylinder(a.part("boat", base_color=(0.72, 0.72, 0.70),
                      roughness=0.55).mesh,
               1.05, 6.0, seg=6, center=(0.0, 52.0, 9.40), color=DECK_GREY,
               axis="Y")

    # ---- radar mast: the ship's vertical signature -----------------------
    mast = a.part("mast", base_color=DECK_LINE, roughness=0.50)
    C.box(mast.mesh, (0.50, 0.50, 12.0), center=(0.0, 6.0, 19.60), color=DECK_LINE)
    C.box(mast.mesh, (0.32, 0.32, 5.2), center=(0.0, 6.0, 24.40), color=DECK_LINE)
    C.box(mast.mesh, (5.0, 0.34, 0.34), center=(0.0, 6.0, 22.60), color=ANTENNA)
    C.box(mast.mesh, (3.4, 0.34, 0.34), center=(0.0, 6.0, 25.60), color=ANTENNA)
    # navigation lights: red to port, green to starboard -- pure silhouette value
    nav = a.part("navlights", base_color=LAMP_AMBER, roughness=0.20,
                 emissive=(0.80, 0.40, 0.05))
    for sy, col in ((-1, (0.90, 0.12, 0.12)), (1, (0.15, 0.75, 0.25))):
        C.box(nav.mesh, (0.50, 0.50, 0.60), center=(sy * 2.50, 6.0, 22.60),
              color=col)
    return a


# ============================================================================
# 3. SAILING YACHT  -- 11.0 m long, 3.40 m wide, 13.2 m mast head
# ============================================================================

# Hull stations, stern (+Y) to bow (-Y), as (y, half_beam, z_bottom, z_deck,
# radius).  The keel is at Z = 0 and the deck edge at 1.25 m, so the visible
# freeboard above the water is about 1.25 m -- right for an 11 m yacht.
_YACHT_STATIONS = [
    # y,     half_beam, z_bottom, z_deck, radius
    (5.50, 1.55, 0.55, 1.10, 0.30),     # transom
    (4.20, 1.70, 0.40, 1.14, 0.32),
    (2.40, 1.70, 0.30, 1.18, 0.34),
    (0.00, 1.68, 0.26, 1.22, 0.36),     # max beam amidships
    (-2.40, 1.52, 0.32, 1.24, 0.34),
    (-4.00, 1.15, 0.52, 1.25, 0.28),
    (-5.00, 0.62, 0.78, 1.25, 0.16),    # sharp entry
    (-5.50, 0.14, 0.95, 1.25, 0.05),    # stem
]


def _yacht_hull_rings():
    """Closed hull shell, stern to bow.  Same winding convention as the
    destroyer's: profiles CCW in (X, Z), run advancing along -Y."""
    return _hull_rings(_YACHT_STATIONS)


def _yacht():
    """Sailing yacht: white hull, coachroof, mast, triangular mainsail.

    THE SAIL IS THE ONLY OPEN SURFACE IN THIS MODULE.  A sail is a single
    triangle with no enclosed volume, so signed volume says nothing about
    which way its faces point; the part therefore declares
    ``outward=("dir", (0, 0, 1))`` and every face is wound to face ``+Z``.
    A sail in the XY... the mast is in the XZ plane, so the sail's normal is
    ``+Z`` (the athwartships direction), and both triangles of the sail are
    emitted with that winding.  Getting it wrong fails the export loudly
    rather than silently shading the sail inside-out.
    """
    a = C.Asset("yacht_sailboat", "vehicle")

    # ---- hull ------------------------------------------------------------
    hull = a.part("hull", base_color=HULL_WHITE, roughness=0.40)
    _loft_hull(hull, _yacht_hull_rings(), HULL_WHITE)
    C.recolor_faces_where(hull, lambda n, c: n[2] < -0.35, HULL_DK)
    C.recolor_faces_where(hull, lambda n, c: c[2] < 0.62, HULL_BLUE)
    C.recolor_faces_where(hull, lambda n, c: n[2] > 0.5, HULL_WHITE)

    # ---- boot-top stripe: a proud BAND, not a painted decal ---------------
    # A stripe on a hull side is a vertical surface, so the open-surface
    # treatment the sail needs is exactly wrong for it: its faces point
    # sideways, not up, and one part cannot declare two outward directions.
    # So it is built as what a real builder would actually make -- a thin
    # CLOSED slab laid onto the hull flank and pushed inboard, which the
    # exporter can volume-check exactly like every other solid.  Building the
    # band as a solid also means the stripe catches the light on its top
    # arris, which a one-sided decal could never do.
    stripe = a.part("stripe", base_color=HULL_BLUE, roughness=0.35)
    for sy in (-1, 1):
        for i in range(len(_YACHT_STATIONS) - 1):
            ya, yb = _YACHT_STATIONS[i][0], _YACHT_STATIONS[i + 1][0]
            ba, bb = _YACHT_STATIONS[i][1], _YACHT_STATIONS[i + 1][1]
            outer = [(sy * (ba + 0.04), ya, 0.60), (sy * (bb + 0.04), yb, 0.60),
                     (sy * (bb + 0.04), yb, 0.86), (sy * (ba + 0.04), ya, 0.86)]
            # extrude the outer quad INBOARD, against the hull side
            _slab(stripe.mesh, outer, (-sy * 0.10, 0.0, 0.0), HULL_BLUE)
    # a matching band around the transom
    for sy in (-1, 1):
        quad = [(sy * 0.06, 5.54, 0.60), (sy * 1.55, 5.54, 0.60),
                (sy * 1.55, 5.54, 0.86), (sy * 0.06, 5.54, 0.86)]
        _slab(stripe.mesh, quad, (0.0, -0.10, 0.0), HULL_BLUE)

    # ---- coachroof + cockpit ---------------------------------------------
    roof = a.part("cabin", base_color=HULL_WHITE, roughness=0.42)
    C.chamfer_box(roof.mesh, (2.40, 5.20, 0.86), center=(0.0, 1.10, 1.60),
                  color=HULL_WHITE, bevel=0.14)
    C.chamfer_box(roof.mesh, (1.90, 3.20, 0.62), center=(0.0, 0.30, 2.22),
                  color=C.shade(HULL_WHITE, 0.94), bevel=0.12)
    glass = a.part("glass", base_color=(0.07, 0.12, 0.17), roughness=0.12,
                   metallic=0.30)
    # coachroof side windows, both flanks
    for sy in (-1, 1):
        for i in range(3):
            C.box(glass.mesh, (0.06, 0.92, 0.34),
                  center=(sy * 1.21, -0.50 + i * 1.05, 1.66), color=glass.base_color)
    C.box(glass.mesh, (1.40, 0.06, 0.30), center=(0.0, -1.51, 1.66),
          color=glass.base_color)
    # companionway and a cockpit coaming, so the aft deck is not a bare plane
    C.chamfer_box(roof.mesh, (0.90, 0.90, 0.50), center=(0.0, 3.10, 1.50),
                  color=TEAK, bevel=0.08)
    C.box(glass.mesh, (0.70, 0.06, 0.30), center=(0.0, 3.56, 1.52),
          color=glass.base_color)
    # toe rail running the length of the deck edge: the horizontal line that
    # separates white topsides from the deck
    for sy in (-1, 1):
        C.box(roof.mesh, (0.14, 10.0, 0.10), center=(sy * 1.62, -0.30, 1.26),
              color=C.shade(HULL_WHITE, 0.80))
    # pulpit and a couple of stanchions forward
    for sy in (-1, 1):
        C.cylinder(roof.mesh, 0.045, 0.72, seg=6, center=(sy * 1.30, -4.30, 1.55),
                   color=MAST, axis="Z")
    C.cylinder(roof.mesh, 0.045, 2.30, seg=6, center=(0.0, -4.60, 1.90),
               color=MAST, axis="X")

    # ---- mast + boom -----------------------------------------------------
    rig = a.part("rig", base_color=MAST, roughness=0.40, metallic=0.35)
    C.cylinder(rig.mesh, 0.095, 11.6, seg=8, center=(0.0, 0.60, 7.10),
               color=MAST, axis="Z")                   # mast
    C.cylinder(rig.mesh, 0.075, 3.40, seg=6, center=(0.0, -1.10, 2.30),
               color=MAST, axis="Y")                   # boom
    C.cylinder(rig.mesh, 0.035, 1.30, seg=4, center=(0.0, 0.20, 12.60),
               color=MAST, axis="Z")                   # masthead

    # ---- THE SAIL: the only open surfaces in this module -----------------
    #
    # WHY OUTWARD MUST BE DECLARED AT ALL.  A sail is a bare triangle: it
    # encloses no volume, so the signed-volume check the exporter runs on
    # every solid has nothing to measure and the part would ship as an
    # unverified "open-shell".  ``outward=("dir", ...)`` states which way
    # "out of the asset" is instead, and the exporter then asserts that EVERY
    # face agrees with it.
    #
    # WHY +X, NOT -Y.  A rig runs fore-and-aft: the luff is the mast, the
    # foot is the boom, both lie along Y, and the sail's triangular plane
    # spans Y and Z.  Its normal is therefore ATHWARTSHIPS, along X.  The
    # forward axis -Y lies IN the sail's own plane, so declaring it would
    # give dot(n, d) == 0 on every face and the exporter's strict ``> 0``
    # test would reject the whole part.
    #
    # WHY TWO PARTS.  A single one-sided triangle is only ever visible from
    # one side, and the runtime back-face culls, so a yacht would lose its
    # sail -- the single most identifying feature it has -- the moment the
    # camera crossed the centreline.  Two parts, the SAME geometry with
    # opposite winding and opposite declared directions, is the standard
    # two-sided decal: each part is independently outward-checked, and the
    # sail is solid from every angle.
    # The two skins are created ONCE here, not inside the helper: the
    # exporter rejects duplicate part names within an asset, and a.part()
    # always creates a new one rather than looking one up.  ``_part`` is the
    # get-or-create accessor for exactly this reason.
    port = _part(a, "sail_port", base_color=SAIL, roughness=0.85)
    stbd = _part(a, "sail_stbd", base_color=SAIL, roughness=0.85)
    port.outward = ("dir", (1.0, 0.0, 0.0))
    stbd.outward = ("dir", (-1.0, 0.0, 0.0))

    def _sail_face(verts, color):
        """Emit one sail face into both the +X and the -X skin."""
        # cross((0,dy,0), (0,dy,dz)) is +X for any dy, dz > 0, so this order
        # is the +X-facing one and the reversal is the -X-facing one
        port.mesh.tri(verts[0], verts[1], verts[2], color)
        stbd.mesh.tri(verts[2], verts[1], verts[0], color)

    # mainsail: luff up the mast (y=+0.45), foot along the boom (y=-2.70),
    # head at the masthead -- a right triangle, the whole readable silhouette
    _sail_face([(0.0, -2.70, 2.40), (0.0, 0.45, 2.40), (0.0, 0.45, 12.40)],
               SAIL)
    # a jib forward of the mast, the same open surface wound to the same
    # side: it is what makes the rig read as a RIG and not as one lone triangle
    _sail_face([(0.0, -4.20, 1.90), (0.0, -0.30, 1.90), (0.0, -0.30, 9.40)],
               SAIL_DK)
    # batten lines: the horizontal seams that stop a flat triangle from
    # reading as a single dead value
    for i in range(3):
        zb = 4.4 + i * 2.4
        ya = -2.70 + 0.42 * i
        _sail_face([(0.0, ya, zb), (0.0, ya + 0.10, zb),
                    (0.0, ya + 0.10, zb + 0.10), (0.0, ya, zb + 0.10)],
                   C.shade(SAIL, 0.90))
    return a


# ============================================================================
# 4. PASSENGER CRUISE SHIP  -- 260 m long, 32 m wide, ~55 m tall
# ============================================================================

# Superstructure tiers, stern (+Y) to bow (-Y), as
# (y0, y1, half_width, z_base, z_top).  Four slabs of steadily decreasing
# width is the whole read of a liner: at 200 m nothing else survives.
_LINER_TIERS = [
    # y0,   y1,   half_width, z_base, z_top
    (-96.0, -66.0, 14.5, 22.0, 30.0),    # forward tiers: the bridge block
    (-66.0, -22.0, 15.2, 22.0, 33.0),
    (-22.0, 30.0, 15.0, 22.0, 36.0),
    (30.0, 82.0, 14.2, 22.0, 33.0),
]


def _cruise_hull_rings():
    """Closed hull shell, stern to bow, with a raked bow at the forward end.

    Same winding convention as the other two hulls.  A 260 m hull is the
    reason this asset needs low segment counts everywhere: a 6-station loft
    with rounded corners is ~120 triangles, and the budget is spent on the
    superstructure, not on the hull's own facets.
    """
    return _hull_rings(_CRUISE_STATIONS)


def _cruise_deck_rings():
    """The deck slab that closes the step from hull deck to deckhouse base.

    The hull's deck is at z = 20 and every deckhouse tier starts at z = 22,
    so without a slab between them the two are joined by nothing: a 2.0 m
    slot, open along the whole length, that you can see the far side of the
    ship through from any beam-on angle.

    THE SLAB FOLLOWS THE HULL, IT DOES NOT OVERHANG IT.  One four-point cross
    section per station:

    * the bottom ring sits at the hull's own deck plane, half width
      ``hb - r`` -- exactly the width of the hull's flat top at that station,
      so the two surfaces meet with no gap and no cantilever;
    * the top ring is at the deckhouse base, half width ``hb - 0.4``, which
      is inside the hull's maximum beam AND wider than the widest deckhouse
      tier, so the slot is closed for every horizontal sight line.

    Rings advance stern to bow like the hull's (see ``_hull_rings``) and each
    profile is CCW in its own plane, which is the winding ``core.loft`` wants.
    """
    zt = min(t[3] for t in _LINER_TIERS)      # the deckhouse base, 22.0
    rings = []
    for (y, hb, _z0, z_deck, r) in reversed(_CRUISE_STATIONS):
        wb = hb - r                             # the hull's flat top width
        wt = hb - 0.4                           # inside the max beam
        rings.append([(wb, y, z_deck), (wt, y, zt), (-wt, y, zt), (-wb, y, z_deck)])
    return rings


_CRUISE_STATIONS = [
    # y,     half_beam, z_bottom, z_deck, radius
    (130.0, 8.0, 6.0, 20.0, 1.2),     # transom
    (116.0, 12.5, 4.0, 20.0, 2.0),
    (100.0, 15.0, 2.4, 20.0, 2.6),
    (60.0, 16.0, 1.6, 20.0, 3.0),     # full beam
    (0.0, 16.0, 1.6, 20.0, 3.0),
    (-60.0, 16.0, 1.6, 20.0, 3.0),
    (-96.0, 15.0, 2.4, 20.0, 2.6),
    (-116.0, 12.0, 4.4, 20.0, 2.0),
    (-126.0, 7.0, 6.4, 20.0, 1.4),
    (-130.0, 2.2, 7.6, 20.0, 0.6),    # raked stem
]


def _cruise_port_bands(z0, z1):
    """Window bands for one deckhouse tier, as (z_lo, z_hi, w, h, margin).

    Returns the tier's ``z0 .. z1`` span cut into horizontal bands, each band
    carrying the ports it should emit and the plain-hull margin that should
    sit at either end of its run.

    A band is described by an absolute height, and both the height AND the
    port width shrink on each repeat, so the tier reads bottom-up as
    STRIP / band of ports / STRIP / band / STRIP with a falling rhythm
    rather than as one tall field of identical boxes.  Varying the pitch
    band to band is what breaks the single-frequency moire a uniform grid
    produces -- the eye latches onto a single horizontal frequency and
    aliases against it, and that noise is worse than the missing detail.

    The remainder above the last band is left as plain hull, which is both
    what a real liner does with its upper decks and what keeps the top of
    the deckhouse from reading as one more band.
    """
    bands = []
    z = z0
    # plain hull between two bands.  0.6 m, not less: this strip IS the
    # banding, so it has to survive being 200 m away, and at 0.6 m of plain
    # hull between 1.05 m ports the run reads as a dashed dark band against
    # white rather than as a grey wall.  It also has to clear the trim strip
    # the promenade band puts at z1 - 1.2, or the top band's ports would sit
    # inside that trim instead of above it.
    strip = 0.6
    # (band height, port width, port height) per repeat, tallest first
    for (bh, pw, ph) in ((3.2, 2.60, 1.05), (2.4, 3.60, 0.95),
                         (1.8, 4.60, 0.85)):
        if z + bh + strip > z1 - 0.9:
            break
        # end margin shrinks with the band, so each run stops well short of
        # its tier end and leaves plain superstructure on both sides
        bands.append((z, z + bh, pw, ph, 1.4 + 1.6 * (bh / 3.2)))
        z += bh + strip
    return bands


def _cruise():
    """Passenger cruise ship: hull, main deck, four deck tiers, window bands.

    The biggest asset in the set and the one that most needs discipline: the
    window bands alone are a few dozen ports per tier, so every one of those
    is a plain 12-triangle box rather than a 28-triangle chamfered one.
    That single decision is what keeps the asset inside the budget.
    """
    a = C.Asset("ship_cruise", "vehicle")

    # ---- hull ------------------------------------------------------------
    hull = a.part("hull", base_color=LINER_WHITE, roughness=0.45)
    _loft_hull(hull, _cruise_hull_rings(), LINER_WHITE)
    C.recolor_faces_where(hull, lambda n, c: n[2] < -0.35, BOOT_TOP)
    C.recolor_faces_where(hull, lambda n, c: c[2] < 8.0, BOOT_TOP)
    # a dark waterline band and a bright topside stripe
    C.recolor_faces_where(
        hull, lambda n, c: 8.0 < c[2] < 12.0 and abs(n[2]) < 0.9, BOOT_TOP)
    C.recolor_faces_where(hull, lambda n, c: n[2] > 0.4, LINER_DK)

    # ---- main deck slab: closes the 2.0 m step up to the deckhouse ----------
    # The hull deck is at z = 20, the deckhouse tiers start at z = 22, and
    # nothing joined the two.  Its own part, so the slab's outwardness is
    # measured on its own shell instead of riding on the hull's.
    deck = a.part("deck", base_color=LINER_DK, roughness=0.50)
    C.loft(deck.mesh, _cruise_deck_rings(), LINER_DK,
           cap_start_flip=False, cap_end_flip=True)
    if C.signed_volume(deck.mesh) <= 0.0:
        deck.mesh.faces = [(x, z, y) for (x, y, z) in deck.mesh.faces]

    # ---- superstructure: four slabs of decreasing width -------------------
    house = a.part("house", base_color=LINER_WHITE, roughness=0.40)
    for (y0, y1, hw, z0, z1) in _LINER_TIERS:
        C.box(house.mesh, (hw * 2.0, y1 - y0, z1 - z0),
              center=(0.0, (y0 + y1) * 0.5, (z0 + z1) * 0.5),
              color=LINER_WHITE)
        # tier cap: a slightly proud band so the stack reads as separate
        # decks even where the albedo is identical
        C.box(house.mesh, (hw * 2.0 + 0.8, y1 - y0 + 0.6, 0.7),
              center=(0.0, (y0 + y1) * 0.5, z1 + 0.35), color=LINER_SHADE)
    # bridge at the bow top, set back from the stem and cantilevered forward
    C.box(house.mesh, (26.0, 16.0, 6.0), center=(0.0, -84.0, 33.0),
          color=LINER_WHITE)
    C.box(house.mesh, (27.6, 17.0, 0.8), center=(0.0, -84.0, 36.4),
          color=LINER_SHADE)
    # forward observation deck
    C.box(house.mesh, (24.0, 12.0, 1.2), center=(0.0, -92.0, 30.6),
          color=LINER_SHADE)

    # ---- window bands -----------------------------------------------------
    # A liner's read at distance is a set of near-black horizontal bands
    # against white, NOT a field of individual dots.  A port is a 12-triangle
    # box, and 338 of them laid out on one uniform pitch and one uniform size
    # is a moire risk the eye resolves as texture noise -- exactly what the
    # asset is trying not to be.
    #
    # So each tier's height is divided into BANDS: a run of ports with a
    # plain hull strip above and below it.  Two things make the result read
    # as banding rather than as 338 boxes:
    #
    #   * the vertical PITCH VARIES between bands (a tall band gets wide
    #     ports, a short one gets narrow ones), so there is no single
    #     horizontal frequency the eye can lock onto and alias against;
    #   * each band's ports are cut off short of both ends by its own
    #     margin, so the run has real plain hull at each end rather than
    #     running tier-edge to tier-edge.
    #
    # Port count and triangle cost: ~200 boxes instead of ~338, i.e. ~2 400
    # triangles instead of ~4 050 -- the saving is spent on the deckhouse.
    win = a.part("windows", base_color=WINDOW_BAND, roughness=0.30)
    for (y0, y1, hw, z0, z1) in _LINER_TIERS:
        for (_z_lo, _z_hi, port_w, port_h, margin) in _cruise_port_bands(z0, z1):
            run = (y1 - y0) - 2.0 * margin      # usable length of the band
            if run <= port_w:
                continue
            # The port count follows from the GAP, not from the port width:
            # n ports spread over run - port_w leave a pitch of
            # (run - port_w) / (n - 1), and solving that for a pitch of at
            # least port_w + gap keeps every port visibly separate.  Deriving
            # n from the width alone would let the ports overlap into one
            # solid dark stripe, which is the thing banding is meant to avoid.
            gap = port_w * 0.55
            n = int((run - port_w) / (port_w + gap)) + 1
            z = (_z_lo + _z_hi) * 0.5
            first = y0 + margin + port_w * 0.5
            pitch = 0.0 if n < 2 else (run - port_w) / float(n - 1)
            for i in range(n):
                y = first + pitch * i
                for sy in (-1, 1):
                    C.box(win.mesh, (0.40, port_w, port_h),
                          center=(sy * (hw + 0.18), y, z), color=WINDOW_BAND)
    # bridge windows: one wide band, forward
    C.box(win.mesh, (24.0, 0.40, 1.30), center=(0.0, -92.1, 33.6),
          color=WINDOW_BAND)

    # ---- funnel + mast ----------------------------------------------------
    stack = a.part("funnel", base_color=FUNNEL_RED, roughness=0.50)
    C.box(stack.mesh, (14.0, 22.0, 13.0), center=(0.0, 46.0, 40.5),
          color=FUNNEL_RED)
    C.box(stack.mesh, (15.0, 23.0, 0.9), center=(0.0, 46.0, 47.4),
          color=C.shade(FUNNEL_RED, 0.75))
    C.box(stack.mesh, (11.0, 6.0, 4.0), center=(0.0, 46.0, 49.6),
          color=C.shade(FUNNEL_RED, 0.55))      # exhaust uptake
    top = a.part("mast", base_color=LINER_DK, roughness=0.45)
    C.box(top.mesh, (0.9, 0.9, 16.0), center=(0.0, -40.0, 44.0), color=LINER_DK)
    C.box(top.mesh, (0.7, 0.7, 9.0), center=(0.0, 46.0, 55.0), color=LINER_DK)
    C.box(top.mesh, (7.0, 0.6, 0.6), center=(0.0, -40.0, 49.0), color=ANTENNA)
    # funnel casing forward of the stack, the second vertical accent
    C.box(top.mesh, (6.0, 12.0, 4.0), center=(0.0, 26.0, 38.5), color=LINER_SHADE)

    # ---- promenade deck bands + lifeboats --------------------------------
    trim = a.part("trim", base_color=LINER_SHADE, roughness=0.55)
    for (y0, y1, hw, _z0, z1) in _LINER_TIERS:
        for sy in (-1, 1):
            C.box(trim.mesh, (0.9, y1 - y0 - 2.0, 0.5),
                  center=(sy * (hw + 0.5), (y0 + y1) * 0.5, z1 - 1.2),
                  color=LINER_SHADE)
    # lifeboats: a row of bright ovals along one promenade deck
    boats = a.part("lifeboats", base_color=(0.94, 0.52, 0.10), roughness=0.55)
    for i in range(11):
        y = -20.0 + i * 9.0
        for sy in (-1, 1):
            tmp = C.Mesh()
            C.sphere(tmp, 1.0, seg_u=8, seg_v=4, center=(0, 0, 0), color=NAVY_DK,
                     squash=0.55)
            _merge_rot(boats, tmp, center=(sy * 16.2, y, 25.6),
                       rot=(("Y", 90.0),))
    # superstructure lighting at night
    lamps = a.part("lamps", base_color=LAMP_AMBER, roughness=0.20,
                   emissive=(0.85, 0.45, 0.08))
    for i in range(9):
        y = -70.0 + i * 16.0
        C.box(lamps.mesh, (29.0, 0.5, 0.4), center=(0.0, y, 30.6),
              color=LAMP_AMBER)
    return a


# ------------------------------------------------------------------ assembly

def build_all():
    """Return the ordered list of military / maritime vehicle assets."""
    return [_tank(), _destroyer(), _yacht(), _cruise()]
