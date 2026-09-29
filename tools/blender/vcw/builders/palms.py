"""Palms: the three roadside silhouettes that line a Miami boulevard.

Every tree is procedural geometry assembled from the ``core`` kernel: a swept,
tapering, slightly curved trunk; a scalloped shell of leaf-base scars banded
light/dark up the bole; a flared crown shaft; and a three-whorl crown of
keeled blades lofted along their own arcs.  No imported or traced assets -- the
arcs, the band spacing and the per-blade jitter come from fixed integer tables
so the set is identical on every build.

Authoring is Blender-style Z-up, origin at the centre of the trunk base, the
tree resting on z = 0.  ``build_all`` returns ``palm_tall`` (~9 m, slender),
``palm_short`` (~6.5 m, stout, broad blades) and ``palm_bushy`` (~6 m, three
whorls plus a skirt of dead fronds under the crown).  All three sit inside the
6-12 m height band the game lays out for them.

WHY THE BLADES ARE NOT ELLIPSES.  The runtime shades with a single directional
light, a constant ambient, no texture and no specular (see CONTRACT.md s1), so
the only thing that separates two adjacent polygons is a different NORMAL or a
different albedo.  A flat elliptical section renders as one dead value: a
paddle.  The section below is therefore KEELED -- a narrow raised midrib spine
with 50-degree walls, a concave fold at the spine root, and two wings that run
out nearly flat before curling down at the edge.  That is twelve points where
the old ellipse had eight, and every one of the extra points buys a normal
change the light can find.
"""

import math

from .. import core as C

# --------------------------------------------------------------------------
# palette
#
# Three blade greens rather than two, so no two whorls read as the same
# material, plus a lighter yellower midrib stripe and a dry yellow-green for
# the oldest blades.  Contrast between the sunlit top and the shaded belly is
# deliberately strong: with no ambient occlusion in the shader, a lit-face /
# dark-face split on the SAME polygon is the only self-shadow cue available.
# --------------------------------------------------------------------------

BARK = (0.53, 0.42, 0.30)
BARK_RING = (0.28, 0.20, 0.13)      # dark leaf-base scar band
BARK_RING_LT = (0.70, 0.58, 0.41)   # light band -- the alternation is the read
BARK_SHAFT = (0.45, 0.35, 0.24)

LEAF_A_TOP = (0.30, 0.60, 0.26)     # mid green
LEAF_A_UND = (0.13, 0.31, 0.14)
LEAF_B_TOP = (0.48, 0.75, 0.30)     # bright, young growth
LEAF_B_UND = (0.22, 0.44, 0.19)
LEAF_C_TOP = (0.20, 0.45, 0.25)     # dark, old growth
LEAF_C_UND = (0.08, 0.22, 0.13)
MIDRIB = (0.64, 0.81, 0.34)         # the spine stripe
OLD_TINT = (0.57, 0.63, 0.21)       # dry yellow-green

DEAD_TOP = (0.56, 0.41, 0.20)
DEAD_UND = (0.30, 0.20, 0.10)
DEAD_RIB = (0.70, 0.57, 0.29)

COCO = (0.31, 0.22, 0.13)
COCO_CUT = (0.53, 0.40, 0.23)


# --------------------------------------------------------------------------
# the keeled blade cross-section
#
# (lateral, height), BOTH in units of the blade's own half width, ordered CCW
# seen from the blade tip so the ring's right-hand normal is the tangent --
# the orientation loft() needs to keep the shell outward.
#
#   0..2  and 10..11   the raised midrib spine: ~11% of the blade width, walls
#                      at ~52 degrees, with a concave root at 2 / 10
#   3..4   and  8..9   the wings, running out at ~8 degrees then curling down
#   5..7               a thin belly so the blade is a solid, not a sheet
# --------------------------------------------------------------------------

_KEEL = (
    (0.00, 0.235),      # 0  midrib apex
    (-0.055, 0.232),    # 1  left spine wall  (steep: this is the catch-light)
    (-0.115, 0.155),    # 2  left spine root   (concave -- the fold)
    (-0.70, 0.075),     # 3  left wing
    (-1.00, -0.020),    # 4  left edge, curling down
    (-0.60, -0.105),    # 5  left underside
    (0.00, -0.130),     # 6  belly centre
    (0.60, -0.105),     # 7  right underside
    (1.00, -0.020),     # 8  right edge
    (0.70, 0.075),      # 9  right wing
    (0.115, 0.155),     # 10 right spine root
    (0.055, 0.232),     # 11 right spine wall
)
SEG = len(_KEEL)                  # 12 points per cross-section
RIB_FACES = (0, 1, 10, 11)        # the four faces that form the midrib

# Pinnate silhouette ripple, indexed by station.  A palm blade is a comb of
# leaflets, so its outline is not a smooth lens -- this notches the width
# without adding a single triangle.  Starts at 0 (continuous petiole) and
# deepens toward the tip, where the leaflets are longest and most separated.
NOTCH = (0.0, 0.10, 0.28, 0.10, 0.42, 0.10, 0.30, 0.55)


def _jit(seed):
    """A deterministic 0..1 draw.

    The set has to be byte-identical on every build, so this is a fixed
    integer hash rather than ``random`` -- but it still gives far richer
    per-blade variation than a modular table, and the variation is what stops
    the crown from reading as a mirrored rosette.
    """
    s = (int(seed) * 1103515245 + 12345) & 0x7FFFFFFF
    s = ((s ^ (s >> 13)) * 1274126177) & 0x7FFFFFFF
    return ((s ^ (s >> 16)) & 0xFFFFFF) / float(0xFFFFFF)


# --------------------------------------------------------------------------
# trunk curve
#
# One parameterisation serves the whole tree.  Below ``t0`` the trunk is dead
# vertical, and only above it does the sideways offset kick in:
#
#     d(t) = lean * ((max(0, t - t0) / (1 - t0)) ** 2.4)
#
# That vertical bole is not decoration.  core.tube() builds each ring from a
# tangent, and the first tangent is the plain secant from point 0 to point 1 --
# so on a trunk that starts leaning immediately, that ring is already tilted,
# and its lowest vertex lands r * sin(tilt) BELOW z = 0.  On palm_tall that is
# -0.6 mm of geometry under the road: invisible, and exactly the kind of thing
# that makes a tree sink into the pavement.  A vertical lower bole makes the
# first secant exactly +Z, so the base ring is level and its lowest vertex is
# the base cap centre at z = 0.
#
# ``t0`` is ALSO what lets the banded scar shell start low on the bole: its
# first two stations are held at or below t0 for exactly the same reason, so
# the shell's first ring is level too.  See _banded_bole.
# --------------------------------------------------------------------------

def _curve(cfg, t):
    """A point on the trunk centreline at normalised height ``t``."""
    s = max(0.0, t - cfg["t0"]) / (1.0 - cfg["t0"])
    d = cfg["lean"] * (s ** 2.4)
    a = cfg["heading"]
    return (d * math.cos(a), d * math.sin(a), cfg["height"] * t)


def _radius(cfg, t):
    """Trunk radius at ``t``: wide at the foot, tapering to the crown."""
    return cfg["r_top"] + (cfg["r_base"] - cfg["r_top"]) * ((1.0 - t) ** 0.85)


def _at(cfg, u):
    """(point, unit tangent, radius) at normalised height ``u``."""
    t = min(1.0, max(0.0, u))
    h = 2.0e-3
    p = _curve(cfg, t)
    lo = _curve(cfg, max(0.0, t - h))
    hi = _curve(cfg, min(1.0, t + h))
    return p, C.normalize(C.sub(hi, lo)), _radius(cfg, t)


# --------------------------------------------------------------------------
# trunk, leaf-base banding, crown shaft
# --------------------------------------------------------------------------

def _banded_bole(a, cfg, name):
    """One continuous shell over the bole, scalloped into light/dark bands.

    The old code swept a separate 3-station collar per scar: 13 collars cost
    13 x 6 x seg triangles and every one of them was a *single* colour, so the
    banding had to be carried by geometry alone.  Building one shell with two
    stations per band instead costs (2 x bands) x 2 x seg and buys BOTH cues:
    the radius alternates between the trunk and trunk+bulge (a real normal
    break at every band edge) and the faces alternate dark / light (an albedo
    break).  Same look, roughly a third of the triangles.
    """
    rings = C.Part("%s_rings" % name, base_color=BARK_RING, roughness=0.95)
    bands = cfg["bands"]
    seg = cfg["ring_seg"]
    bulge = cfg["bulge"]
    off_min = 0.008

    u_top = 0.855
    # The shell must clear the ground by its own radius plus the standing-off
    # gap, and its first TWO stations must stay at or below t0 so the opening
    # ring is level (see the t0 note above).  Both are hard, not advisory.
    u0 = (cfg["r_base"] + off_min + 0.012) / cfg["height"]
    u0 = min(u0, cfg["t0"] * 0.55)
    u0 = max(u0, 0.012)
    step = (u_top - u0) / (2.0 * bands)
    u0 = min(u0, cfg["t0"] - step * 0.98)

    # Station heights: alternate band widths (0.68 / 1.32) so the alternation
    # is not a machine-perfect stripe.
    us = [u0]
    for b in range(2 * bands):
        w = 0.68 if b % 2 == 0 else 1.32
        us.append(us[-1] + step * w)
    scale = (u_top - u0) / (us[-1] - u0)
    us = [u0 + (u - u0) * scale for u in us]

    # Offsets alternate trunk / trunk+bulge, and the first and last stations
    # return to trunk so the shell blends into the bare bole and the shaft.
    offs = []
    for i in range(2 * bands + 1):
        b = min(bands - 1, i // 2)
        peak = bulge * (1.18 if b % 2 == 0 else 0.78)
        offs.append(off_min + peak * (0.5 if i % 2 else 0.0))
    offs[0] = off_min
    offs[-1] = off_min

    path = [_curve(cfg, u) for u in us]
    radii = [_radius(cfg, u) + offs[i] for i, u in enumerate(us)]
    C.tube(rings.mesh, path, radii[0], seg, color=BARK_RING, radii=radii)

    # tube() emits the side walls first: 2 triangles per (station gap, seg).
    for s in range(2 * bands):
        b = s // 2
        col = BARK_RING if b % 2 == 0 else BARK_RING_LT
        # the leading face of a band is a little darker than its body -- it is
        # the strip the band below shadows
        col_lead = C.shade(col, 0.84) if s % 2 == 0 else col
        for j in range(seg):
            i0 = (s * seg + j) * 2
            rings.mesh.fcol[i0] = col_lead
            rings.mesh.fcol[i0 + 1] = col
    a.add(rings)


def _crown(a, cfg, name, top, rtop):
    """The flared boot the fronds emerge from: a two-band frustum."""
    shaft = C.Part("%s_crown" % name, base_color=BARK_SHAFT,
                   roughness=0.88, flat=False)
    h = cfg["shaft"]
    seg = 10
    z0 = top[2]
    ring = C.circle(1.0, seg)
    levels = ((z0, rtop * 0.90), (z0 + h * 0.50, rtop * 1.42),
              (z0 + h * 0.86, rtop * 1.60))
    rings = [[(top[0] + px * r, top[1] + py * r, z) for (px, py) in ring]
             for (z, r) in levels]
    c_bot = C.shade(BARK_SHAFT, 0.70)
    c_mid = BARK_SHAFT
    c_top = C.shade(BARK_SHAFT, 1.26)
    C.loft(shaft.mesh, rings, c_mid, cap_start_flip=True, cap_end_flip=False,
           smooth=True, cap_start_color=c_bot, cap_end_color=c_top)
    for s, col in ((0, c_mid), (1, c_top)):
        for j in range(seg):
            i0 = (s * seg + j) * 2
            shaft.mesh.fcol[i0] = col
            shaft.mesh.fcol[i0 + 1] = col
    a.add(shaft)
    return (top[0], top[1], z0 + h * 0.80)


def _trunk(a, cfg, name):
    """Swept trunk + banded bole + crown shaft.

    Returns the point the MAIN whorl should radiate from.
    """
    trunk = C.Part("%s_trunk" % name, base_color=BARK, roughness=0.92,
                   flat=False)
    n = cfg["stations"]
    # Stations run t = 0 .. 1 inclusive.  Every gap is metres apart, far above
    # tube()'s 1e-6 dedup gate, so path and radii stay the same length and
    # radii[] indexes the rings directly.
    path = [_curve(cfg, i / float(n - 1)) for i in range(n)]
    radii = [_radius(cfg, i / float(n - 1)) for i in range(n)]
    C.tube(trunk.mesh, path, radii[0], cfg["seg"], smooth=True, radii=radii,
           color=BARK)
    a.add(trunk)

    _banded_bole(a, cfg, name)

    top, _, rtop = _at(cfg, 1.0)
    return _crown(a, cfg, name, top, rtop)


# --------------------------------------------------------------------------
# fronds
# --------------------------------------------------------------------------

def _face_normal(m, i):
    """The geometric normal of face ``i``, computed in isolation."""
    a, b, c = m.faces[i]
    return C.normalize(C.cross(C.sub(m.pos[b], m.pos[a]), C.sub(m.pos[c], m.pos[a])))


def _tint(col, u, age):
    """Longitudinal grading along a blade, then the dry old-blade wash.

    The bases sit down inside the crown and are genuinely darker there; the
    tips are the oldest tissue and go yellow.  Both are pure albedo and cost
    nothing.
    """
    c = C.shade(col, 0.84 + 0.24 * u)
    if age > 0.0:
        c = C.lerp(c, C.shade(OLD_TINT, 0.90 + 0.20 * u),
                   age * (0.28 + 0.50 * u))
    return c


def _frond(m, base, azimuth, length, rise, droop, plunge, wmax, twist, sway,
           c_top, c_under, c_rib, stations=8, age=0.0):
    """One palm blade: a keeled, tapering loft along a drooping arc.

    The centreline climbs at ``rise`` per unit length, gives it back at
    ``droop`` u^2 and then whips down harder over the last third at
    ``plunge`` u^4.  The peak therefore lands at roughly u = rise / (2 droop)
    -- near 0.35 for the numbers below -- and the tip finishes well below the
    crown, which is the pendulous read; ``rise``, ``droop`` and ``plunge`` are
    all per-blade, so no two blades share a silhouette.

    ``twist`` rotates the cross-section frame about the tangent along the
    blade.  A rotation about the tangent preserves s x up == t, so the shell
    stays outward, and it costs zero triangles while making the normals sweep
    continuously -- the single cheapest animation-of-light in the module.

    Cross-sections are the keeled profile above, scaled by a lanceolate width
    that swells to its widest at ~40% and tapers to a point.
    """
    dx, dy = math.cos(azimuth), math.sin(azimuth)
    px, py = -dy, dx
    n = SEG

    # ---- centreline -----------------------------------------------------
    pts = []
    for i in range(stations):
        u = i / float(stations - 1)
        r = length * u
        s = sway * (u * u) * (1.0 - 0.3 * u)
        pts.append((base[0] + dx * r + px * s,
                    base[1] + dy * r + py * s,
                    base[2] + rise * u - droop * (u * u) - plunge * (u ** 4)))

    # ---- per-station frame, width and section ----------------------------
    rings = []
    ups = []
    for i, p in enumerate(pts):
        if i == 0:
            t = C.sub(pts[1], pts[0])
        elif i == len(pts) - 1:
            t = C.sub(pts[-1], pts[-2])
        else:
            t = C.sub(pts[i + 1], pts[i - 1])
        t = C.normalize(t)
        ref = C.cross((0.0, 0.0, 1.0), t)
        if C.length(ref) < 0.25:
            # A near-vertical tangent collapses cross(z, t); fall back to the
            # blade's own horizontal perpendicular instead.
            ref = (px, py, 0.0)
        s0 = C.normalize(ref)
        up0 = C.cross(t, s0)
        # A twist is a rotation about the tangent, so it preserves s x up == t
        # and the shell can never invert.
        ang = twist * (i / float(stations - 1))
        ca, sa = math.cos(ang), math.sin(ang)
        s_ax = C.add(C.mul(s0, ca), C.mul(up0, sa))
        up = C.add(C.mul(up0, ca), C.mul(s0, -sa))

        u = i / float(stations - 1)
        # lanceolate: narrow petiole, quickest swell, long taper to a point.
        # The 0.024 floor keeps the last ring from collapsing to a zero-area
        # sliver -- core drops those silently, and a dropped triangle would
        # break the face-colour index arithmetic above it.
        w = wmax * max((0.21 + 0.79 * math.sin(math.pi * (u ** 0.58)))
                       * ((1.0 - (u ** 1.75)) ** 0.62), 0.024)
        # A real frond is not a smooth lens: the blade is pinnate, so its
        # silhouette ripples.  Multiplying the width by a fixed table indexed
        # by the STATION (never by a continuous phase) gives each blade a
        # coarser, notched outline that survives at any length for free -- it
        # re-uses the stations already there.  Table chosen so the notches
        # deepen toward the tip, where leaflets are longest.
        w *= (1.0 - 0.20 * NOTCH[i % len(NOTCH)])
        rings.append([C.add(C.add(p, C.mul(s_ax, lat * w)),
                            C.mul(up, hi * w))
                      for (lat, hi) in _KEEL])
        ups.append(up)

    first = len(m.faces)
    C.loft(m, rings, c_top, cap_start_flip=True, cap_end_flip=False)

    # loft() emits the side walls first -- (stations-1) gaps x SEG quads, two
    # triangles each -- then the two cap fans.  No ring here can collapse (the
    # width is clamped well above zero), so the split is exact.
    nwall = 2 * (stations - 1) * n
    for k in range(nwall):
        q = k >> 1
        s = q // n
        j = q % n
        u = (s + 0.5) / float(stations - 1)
        mid_up = C.normalize(C.add(ups[s], ups[s + 1]))
        nrm = _face_normal(m, first + k)
        if j in RIB_FACES:
            col = c_rib
        elif C.dot(nrm, mid_up) < -0.08:
            col = c_under
        else:
            col = c_top
        m.fcol[first + k] = _tint(col, u, age)
    for k in range(n):
        m.fcol[first + nwall + k] = _tint(c_under, 0.0, age)
        m.fcol[first + nwall + n + k] = _tint(c_rib, 1.0, age)
    return m


def _blade(m, base, az_deg, length, rise, droop, plunge, wmax, twist, sway,
           top=None, under=None, rib=None, stations=8, age=0.0):
    _frond(m, base, math.radians(az_deg), length, rise, droop, plunge, wmax,
           twist, sway, top or LEAF_A_TOP, under or LEAF_A_UND,
           rib or MIDRIB, stations, age)


def _blade_base(top, dz, rad, ang):
    """A blade attachment point on the crown axis.

    ``dz`` metres up the axis from the shaft's mid-flare and ``rad`` metres
    out at azimuth ``ang``.  Staggering the bases is what gives the crown
    depth -- a single whorl from a single point reads as a rosette decal.
    """
    return (top[0] + rad * math.cos(ang),
            top[1] + rad * math.sin(ang),
            top[2] + dz)


def _whorl(parts, top, count, phase, seed, length, rise, droop, plunge, wmax,
           dz, rad, length_var, rise_var, droop_var, plunge_var, sway_amp,
           twist_amp, az_jit, stations, age_base, age_var, top_a, und_a,
           rib_a, top_b, und_b, rib_b):
    """One crown whorl: ``count`` blades, evenly phased then heavily jittered.

    ``parts`` is a list of Parts; blade ``k`` goes into ``parts[k % len]`` so
    the whorl alternates materials.  Every length / rise / droop / plunge /
    sway / twist / azimuth gets its own deterministic draw, so the whorl holds
    short stiff blades next to long pendulous ones and no two blades mirror.
    """
    if not isinstance(parts, (list, tuple)):
        parts = [parts]
    for k in range(count):
        j = lambda o: _jit(seed * 97 + k * 13 + o)     # noqa: E731
        az = phase + 360.0 * k / float(count) \
            + az_jit * (j(1) - 0.5) * 2.0
        even = (k % 2 == 0)
        _blade(parts[k % len(parts)].mesh,
               _blade_base(top,
                           dz + (j(2) - 0.5) * 0.26,
                           rad + 0.035 * (j(3) - 0.5),
                           math.radians(az)),
               az,
               length * (1.0 + length_var * (j(4) - 0.5) * 2.0),
               rise * (1.0 + rise_var * (j(5) - 0.5) * 2.0),
               droop * (1.0 + droop_var * (j(6) - 0.5) * 2.0),
               plunge * (1.0 + plunge_var * (j(7) - 0.5) * 2.0),
               wmax * (0.86 + 0.30 * j(8)),
               twist_amp * (j(9) - 0.5) * 2.0,
               sway_amp * (j(10) - 0.5) * 2.0,
               top=top_a if even else top_b,
               under=und_a if even else und_b,
               rib=rib_a if even else rib_b,
               stations=stations,
               age=max(0.0, min(1.0, age_base + age_var * (j(11) - 0.5) * 2.0)))


# --------------------------------------------------------------------------
# 1. palm_tall -- ~9 m, slender, coconut cluster
# --------------------------------------------------------------------------

TALL = dict(height=7.44, lean=0.95, heading=18.0, t0=0.30,
            r_base=0.255, r_top=0.135, seg=9, stations=9, ring_seg=8,
            bands=9, bulge=0.030, shaft=0.46)


def _palm_tall():
    a = C.Asset("palm_tall", "palm")
    top = _trunk(a, TALL, "palm_tall")

    ga = C.Part("palm_tall_fronds_a", base_color=LEAF_A_TOP, roughness=0.72)
    gb = C.Part("palm_tall_fronds_b", base_color=LEAF_B_TOP, roughness=0.72)
    gc = C.Part("palm_tall_fronds_core", base_color=LEAF_B_TOP, roughness=0.70)

    # MAIN whorl: the six full-length blades the silhouette is made of.
    _whorl([ga, gb], top, 6, 14.0, 3,
           length=3.42, rise=2.28, droop=2.74, plunge=0.46, wmax=0.275,
           dz=-0.02, rad=0.16,
           length_var=0.17, rise_var=0.13, droop_var=0.20, plunge_var=0.42,
           sway_amp=0.34, twist_amp=0.62, az_jit=26.0, stations=8,
           age_base=0.30, age_var=0.28,
           top_a=LEAF_A_TOP, und_a=LEAF_A_UND, rib_a=MIDRIB,
           top_b=LEAF_B_TOP, und_b=LEAF_B_UND, rib_b=MIDRIB)

    # INNER whorl: four short, steep, bright young blades filling the core.
    _whorl(gc, top, 4, 52.0, 11,
           length=1.62, rise=2.05, droop=1.02, plunge=0.04, wmax=0.165,
           dz=0.20, rad=0.09,
           length_var=0.24, rise_var=0.10, droop_var=0.26, plunge_var=0.90,
           sway_amp=0.16, twist_amp=0.45, az_jit=34.0, stations=5,
           age_base=0.0, age_var=0.16,
           top_a=LEAF_B_TOP, und_a=LEAF_B_UND, rib_a=MIDRIB,
           top_b=LEAF_A_TOP, und_b=LEAF_A_UND, rib_b=MIDRIB)
    a.add(ga)
    a.add(gb)
    a.add(gc)

    # Coconut cluster tucked under the shaft, bunched on one side.
    nuts = a.part("palm_tall_coconuts", base_color=COCO, roughness=0.8)
    cz = TALL["height"] - 0.14
    axis, _, _ = _at(TALL, cz / TALL["height"])
    first_nut = len(nuts.mesh.faces)
    for k in range(6):
        ang = math.radians(95.0 + 17.0 * k)
        d = 0.19 + 0.05 * (k % 3)
        C.sphere(nuts.mesh, 0.105, 8, 4,
                 center=(axis[0] + d * math.cos(ang),
                         axis[1] + d * math.sin(ang),
                         cz + 0.10 * (k % 2)),
                 color=COCO)
    # A lit crown on each nut: the sphere is one material, so the whole top
    # cap takes the highlight.  The runtime has no specular, so without this
    # the cluster is six identical brown dots.
    for i in range(first_nut, len(nuts.mesh.faces)):
        if _face_normal(nuts.mesh, i)[2] > 0.35:
            nuts.mesh.fcol[i] = COCO_CUT
    return a


# --------------------------------------------------------------------------
# 2. palm_short -- ~6.5 m, stout, broad blades
# --------------------------------------------------------------------------

SHORT = dict(height=4.95, lean=0.42, heading=-35.0, t0=0.24,
             r_base=0.345, r_top=0.215, seg=10, stations=8, ring_seg=8,
             bands=9, bulge=0.034, shaft=0.40)


def _palm_short():
    a = C.Asset("palm_short", "palm")
    top = _trunk(a, SHORT, "palm_short")

    ga = C.Part("palm_short_fronds_a", base_color=LEAF_A_TOP, roughness=0.72)
    gb = C.Part("palm_short_fronds_b", base_color=LEAF_B_TOP, roughness=0.72)
    gc = C.Part("palm_short_fronds_core", base_color=LEAF_B_TOP, roughness=0.70)

    # MAIN whorl: six broad, paddle-like blades.
    _whorl([ga, gb], top, 6, 22.0, 5,
           length=2.52, rise=1.86, droop=2.34, plunge=0.34, wmax=0.405,
           dz=-0.02, rad=0.22,
           length_var=0.16, rise_var=0.14, droop_var=0.19, plunge_var=0.44,
           sway_amp=0.30, twist_amp=0.55, az_jit=28.0, stations=7,
           age_base=0.26, age_var=0.26,
           top_a=LEAF_A_TOP, und_a=LEAF_A_UND, rib_a=MIDRIB,
           top_b=LEAF_C_TOP, und_b=LEAF_C_UND, rib_b=MIDRIB)

    # INNER whorl: three short, steep blades.
    _whorl(gc, top, 3, 62.0, 23,
           length=1.26, rise=1.66, droop=0.80, plunge=0.03, wmax=0.215,
           dz=0.17, rad=0.12,
           length_var=0.24, rise_var=0.10, droop_var=0.26, plunge_var=0.90,
           sway_amp=0.14, twist_amp=0.42, az_jit=40.0, stations=5,
           age_base=0.0, age_var=0.16,
           top_a=LEAF_B_TOP, und_a=LEAF_B_UND, rib_a=MIDRIB,
           top_b=LEAF_A_TOP, und_b=LEAF_A_UND, rib_b=MIDRIB)
    a.add(ga)
    a.add(gb)
    a.add(gc)
    return a


# --------------------------------------------------------------------------
# 3. palm_bushy -- ~6 m, three whorls plus a dead-frond skirt
# --------------------------------------------------------------------------

BUSHY = dict(height=4.92, lean=0.70, heading=52.0, t0=0.28,
             r_base=0.295, r_top=0.185, seg=9, stations=8, ring_seg=8,
             bands=8, bulge=0.032, shaft=0.44)


def _palm_bushy():
    a = C.Asset("palm_bushy", "palm")
    top = _trunk(a, BUSHY, "palm_bushy")

    ga = C.Part("palm_bushy_fronds_a", base_color=LEAF_A_TOP, roughness=0.72)
    gb = C.Part("palm_bushy_fronds_b", base_color=LEAF_B_TOP, roughness=0.72)
    gc = C.Part("palm_bushy_fronds_core", base_color=LEAF_B_TOP, roughness=0.70)

    # MAIN whorl: six full blades.
    _whorl([ga, gb], top, 6, 9.0, 7,
           length=2.94, rise=2.10, droop=2.58, plunge=0.40, wmax=0.268,
           dz=-0.02, rad=0.19,
           length_var=0.18, rise_var=0.13, droop_var=0.20, plunge_var=0.44,
           sway_amp=0.32, twist_amp=0.60, az_jit=26.0, stations=7,
           age_base=0.28, age_var=0.26,
           top_a=LEAF_A_TOP, und_a=LEAF_A_UND, rib_a=MIDRIB,
           top_b=LEAF_B_TOP, und_b=LEAF_B_UND, rib_b=MIDRIB)

    # OUTER whorl: three old, low, heavily drooping blades hanging off the
    # shaft below the main crown -- the layering that stops this reading as a
    # single flat rosette.  The base sits only just below the main whorl: far
    # enough to read as a separate tier, not so far that the crown looks like
    # three stacked rosettes.
    _whorl(gb, top, 3, 38.0, 31,
           length=2.42, rise=1.12, droop=3.15, plunge=0.86, wmax=0.246,
           dz=-0.20, rad=0.26,
           length_var=0.20, rise_var=0.18, droop_var=0.16, plunge_var=0.40,
           sway_amp=0.38, twist_amp=0.70, az_jit=32.0, stations=6,
           age_base=0.72, age_var=0.22,
           top_a=LEAF_C_TOP, und_a=LEAF_C_UND, rib_a=MIDRIB,
           top_b=LEAF_A_TOP, und_b=LEAF_A_UND, rib_b=MIDRIB)

    # INNER whorl: three short, steep, bright blades at the core.
    _whorl(gc, top, 3, 71.0, 47,
           length=1.40, rise=1.88, droop=0.86, plunge=0.03, wmax=0.176,
           dz=0.19, rad=0.10,
           length_var=0.24, rise_var=0.10, droop_var=0.26, plunge_var=0.90,
           sway_amp=0.15, twist_amp=0.44, az_jit=40.0, stations=5,
           age_base=0.0, age_var=0.16,
           top_a=LEAF_B_TOP, und_a=LEAF_B_UND, rib_a=MIDRIB,
           top_b=LEAF_A_TOP, und_b=LEAF_A_UND, rib_b=MIDRIB)
    a.add(ga)
    a.add(gb)
    a.add(gc)

    # Skirt of dead fronds hanging off the trunk just below the crown.
    sz = BUSHY["height"] - 1.05
    axis, _, rad = _at(BUSHY, sz / BUSHY["height"])
    dead = C.Part("palm_bushy_skirt", base_color=DEAD_TOP, roughness=0.9)
    for k in range(3):
        ang = math.radians(30.0 + k * 120.0 + 9.0 * (k % 2))
        push = (axis[0] + (rad + 0.05) * math.cos(ang),
                axis[1] + (rad + 0.05) * math.sin(ang), sz)
        _blade(dead.mesh, push, math.degrees(ang) + 24.0,
               length=1.02 + 0.16 * (k % 3),
               rise=-0.30, droop=1.20 + 0.14 * (k % 3), plunge=0.10,
               wmax=0.205, twist=0.18 * (k % 3 - 1),
               sway=0.16 if k % 2 else -0.16,
               stations=6,
               top=DEAD_TOP, under=DEAD_UND, rib=DEAD_RIB, age=0.55)
    a.add(dead)
    return a


def build_all():
    """Return the ordered list of palm assets."""
    return [
        _palm_tall(),
        _palm_short(),
        _palm_bushy(),
    ]
