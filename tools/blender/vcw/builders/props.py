"""Category D: street furniture / small city props.

Every prop shares the same authoring rules: Blender-style Z-up, origin at the
centre of the ground footprint with the object resting on z = 0, all geometry
built from the ``core`` kernel primitives (no imported or traced assets).

The runtime shader is ``base * (ambient + light_color * max(dot(n, l), 0))``
plus an emissive term -- NO specular and NO texture (see ``vcw/CONTRACT.md``
section 1).  Three things are therefore the only sources of visible surface
variation, and this module spends its whole budget on exactly those three:

1. **Normal breaks** -- ``C.chamfer_box`` on every chunky body volume.  28
   triangles instead of 12, and every one of the twelve arrises gains a strip
   at an intermediate normal that reads as a highlight.
2. **Albedo zoning** -- ``face_colors`` costs ZERO triangles, so the
   silhouette-carrying volumes are broken into real material zones
   (dark skirt / light body / coloured band / cream bonnet) wherever a
   face-level split is possible.
3. **Emissive** -- lens glass, beacons, signage and the booth interior.  Every
   glowing part carries BOTH a solid ``base_color`` and a non-zero
   ``emissive`` so the shape still reads when the glow is scaled to zero.

Parts are split semantically -- frame / housing / lens / glass / wood -- so each
becomes its own submesh material.

COLLIDER BUDGET.  ``src/game.rs`` derives every prop collider from the asset
bounding box, and narrows bench / trash bin / fire hydrant / newsstand /
phone booth to ``SMALL_PROP_COLLIDER_SCALE`` (0.55) and the traffic cone to 0.30
because their raw bounds are already larger than the real footprint.  None of
those five is allowed to grow much past its current footprint here, and the
street light's base (the only part of it with a full-size collider) is frozen
at its present 0.60 m diameter.
"""

import math

from .. import core as C

# Original palette shared with the building set so the block reads as one city.
CONCRETE = (0.70, 0.68, 0.64)
CONCRETE_DK = (0.50, 0.48, 0.46)
METAL = (0.58, 0.60, 0.62)
METAL_DK = (0.30, 0.32, 0.34)
IRON = (0.24, 0.23, 0.22)
CREAM = (0.96, 0.92, 0.80)
WHITE = (0.95, 0.95, 0.93)
TEAL = (0.16, 0.62, 0.60)
TEAL_DK = (0.11, 0.44, 0.43)
CORAL = (0.96, 0.46, 0.36)
PINK = (0.94, 0.50, 0.61)
MINT = (0.60, 0.88, 0.76)
GRASS_DK = (0.16, 0.40, 0.18)
LEAF_A = (0.26, 0.56, 0.22)
LEAF_B = (0.36, 0.66, 0.27)
BARK = (0.44, 0.35, 0.27)
WOOD = (0.60, 0.42, 0.28)
DARK = (0.09, 0.09, 0.11)
GLASS = (0.26, 0.56, 0.60)
RED = (0.82, 0.10, 0.10)
AMBER = (0.95, 0.55, 0.06)
GREEN = (0.12, 0.72, 0.28)
ORANGE = (0.94, 0.30, 0.10)
PAPER = (0.92, 0.89, 0.80)

LAMP_WARM = (1.00, 0.85, 0.50)
GLOW_COOL = (0.55, 0.88, 0.95)

# Roadworks / parking palette, same register as the rest of the street.
BARRIER_ORANGE = (0.94, 0.40, 0.08)
BARRIER_WHITE = (0.95, 0.94, 0.88)
BEACON_AMBER = (1.00, 0.62, 0.10)
METER_BODY = (0.28, 0.52, 0.56)
METER_TRIM = (0.17, 0.36, 0.39)
METER_FACE = (0.88, 0.90, 0.86)
METER_SLOT = (0.08, 0.09, 0.10)

# Signal lens emissives.  The runtime multiplies emissive by a per-phase gain,
# so a lit red / lit amber / UNLIT green is expressed as a full, a full and a
# zero -- the dark green still reads as a lens because its base colour is set.
SIG_RED_E = (1.00, 0.10, 0.05)
SIG_AMBER_E = (1.00, 0.55, 0.08)
SIG_OFF_E = (0.00, 0.00, 0.00)
GREEN_DARK = (0.07, 0.28, 0.14)


# ---------------------------------------------------------------------------
# small local helpers built on top of the kernel primitives

def _rbox(m, size, center=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
          color=(1.0, 1.0, 1.0)):
    """A box rotated by (rx, ry, rz) DEGREES about its own centre.

    ``C.box`` is axis-aligned only, which is not enough for leaning backrest
    slats, angled roof flaps or the pilasters that have to follow the facets
    of an octagonal pot.  Emitting the same eight corners through the same six
    quads as ``C.box`` keeps the winding (and therefore the volume check)
    identical -- the rotation matrix has determinant +1.
    """
    hx, hy, hz = size[0] * 0.5, size[1] * 0.5, size[2] * 0.5
    corners = [(-hx, -hy, -hz), (hx, -hy, -hz), (hx, hy, -hz), (-hx, hy, -hz),
               (-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz)]
    ax, ay, az = (math.radians(v) for v in rot)
    cx, sx = math.cos(ax), math.sin(ax)
    cy, sy = math.cos(ay), math.sin(ay)
    cz, sz = math.cos(az), math.sin(az)
    # R = Rz * Ry * Rx
    r = ((cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx),
         (sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx),
         (-sy, cy * sx, cy * cx))
    o = tuple(center)
    p = []
    for v in corners:
        p.append(C.add(o, (r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
                           r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
                           r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2])))
    m.quad(p[0], p[3], p[2], p[1], color)
    m.quad(p[4], p[5], p[6], p[7], color)
    m.quad(p[0], p[1], p[5], p[4], color)
    m.quad(p[1], p[2], p[6], p[5], color)
    m.quad(p[2], p[3], p[7], p[6], color)
    m.quad(p[3], p[0], p[4], p[7], color)
    return m


def _bake(dst, temp, offset=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
          pivot=(0.0, 0.0, 0.0)):
    """Rigid-transform ``temp``'s triangles into ``dst``'s mesh.

    ``C.chamfer_box`` is axis-aligned, but chamfered detail is wanted on
    tilted visor plates, leaning backrest slats and half-open dumpster lids.
    Rather than re-deriving a chamfered prism in every orientation, build it
    at the origin in a scratch Part and bake it in rotated.  ``_mat`` is a
    proper rotation (det +1), so the baked triangles keep their winding and no
    inside-out shell can appear.
    """
    m, r = temp.mesh, _mat(rot)
    px, py, pz = pivot

    def xf(p):
        return C.add(offset, (
            r[0][0] * (p[0] - px) + r[0][1] * (p[1] - py) + r[0][2] * (p[2] - pz),
            r[1][0] * (p[0] - px) + r[1][1] * (p[1] - py) + r[1][2] * (p[2] - pz),
            r[2][0] * (p[0] - px) + r[2][1] * (p[1] - py) + r[2][2] * (p[2] - pz)))

    for f, col in zip(m.faces, m.fcol):
        dst.tri(xf(m.pos[f[0]]), xf(m.pos[f[1]]), xf(m.pos[f[2]]), col)
    return dst


def _chamfer_at(dst, size, center=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
                bevel=0.03, color=(1.0, 1.0, 1.0), colors=None, n_corner=1):
    """``C.chamfer_box`` placed at ``center`` and rotated by ``rot`` degrees.

    ``bevel`` is clamped by the kernel to 0.9x the smallest half-extent, so a
    50 mm slat asked for 12 mm gets 12 mm and one asked for 30 mm is silently
    capped rather than collapsing the two rings.
    """
    tmp = C.Part("_chamfer_tmp", base_color=color)
    C.chamfer_box(tmp.mesh, size, center=(0.0, 0.0, 0.0), color=color,
                  bevel=bevel, n_corner=n_corner, colors=colors)
    return _bake(dst, tmp, offset=center, rot=rot)


def _loft_x(m, prof, stations, color, cap=True):
    """Extrude a CCW (y, z) profile along +X.

    ``prof`` points are ``(y, z)`` pairs in CCW order; ``stations`` is a list of
    ``(x, y_scale)`` pairs.  Mapping ``(y, z)`` into the world that way makes
    the ring CCW seen from +X (Y x Z = +X), which is the right-hand direction
    ``loft`` advances in.
    """
    rings = [[(x, py * sy, pz) for (py, pz) in prof] for (x, sy) in stations]
    return C.loft(m, rings, color, cap_start=cap, cap_end=cap,
                  cap_start_flip=True, cap_end_flip=False)


def _sleeve_x(m, prof, stations, color):
    """Sweep a CCW (y, z) profile along +X, scaling it uniformly per station.

    ``stations`` is ``(x, s)``: at that x the profile is dilated to ``s`` times
    its size about its own centroid, so ``(1.0, 1.04, 1.04, 1.0)`` produces a
    band that rises off the surface and sinks back into it -- a closed solid, so
    neither end cap collapses.
    """
    cy = sum(p[0] for p in prof) / float(len(prof))
    cz = sum(p[1] for p in prof) / float(len(prof))
    rings = []
    for (x, s) in stations:
        rings.append([(x, cy + (py - cy) * s, cz + (pz - cz) * s)
                      for (py, pz) in prof])
    return C.loft(m, rings, color, cap_start_flip=True, cap_end_flip=False)


def _mat(deg):
    """Rotation matrix R = Rz * Ry * Rx from Euler angles in DEGREES."""
    ax, ay, az = (math.radians(v) for v in deg)
    cx, sx = math.cos(ax), math.sin(ax)
    cy, sy = math.cos(ay), math.sin(ay)
    cz, sz = math.cos(az), math.sin(az)
    return ((cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx),
            (sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx),
            (-sy, cy * sx, cy * cx))


def _place(parts, offset=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
           pivot=(0.0, 0.0, 0.0)):
    """Rigid-transform whole parts: rotate about ``pivot``, then translate.

    Every angle is a proper rotation (det +1), so face winding survives
    untouched and no inward-facing shell can be introduced.  Used here to tip
    the parking meter's whole head assembly back 14 deg as one rigid group.
    """
    r = _mat(rot)
    px, py, pz = pivot
    for part in parts:
        part.mesh.pos = [C.add(offset, (
            r[0][0] * (p[0] - px) + r[0][1] * (p[1] - py) + r[0][2] * (p[2] - pz),
            r[1][0] * (p[0] - px) + r[1][1] * (p[1] - py) + r[1][2] * (p[2] - pz),
            r[2][0] * (p[0] - px) + r[2][1] * (p[1] - py) + r[2][2] * (p[2] - pz)))
            for p in part.mesh.pos]
    return parts


def _arc_points(cx, cy, r, a0, a1, n):
    """``n`` samples along a circular arc, endpoints included, no duplicates."""
    return [(cx + r * math.cos(a0 + (a1 - a0) * i / float(n)),
             cy + r * math.sin(a0 + (a1 - a0) * i / float(n)))
            for i in range(n + 1)]


def _band(part, z_lo, z_hi, color):
    """Free albedo zoning: recolour every face whose centroid z is in range.

    Costs zero triangles, which makes it the highest value-per-triangle change
    available anywhere in this category.  It only produces a crisp horizontal
    band when the surface is actually split into rings at those heights -- a
    single box face spans the whole object, and recolouring it just repaints
    the face.  Call it after the relevant loft stations are in place.
    """
    return C.recolor_faces_where(
        part, lambda n, c: z_lo <= c[2] <= z_hi, color)


# ---------------------------------------------------------------------------
# 1. street light
# ---------------------------------------------------------------------------

def _streetlight():
    """Cobra-head street light on a 6.50 m tapered column.

    Proportions: real cobra-head street lights stand 6-8 m, so the column is
    lifted from the previous 4.79 m to 6.50 m and the arm now rises 1.0 m over
    its 1.0 m reach to meet the underside of the housing.  The BASE is frozen
    at its present 0.60 m diameter -- this asset uses a full-size collider
    derived from its bounds, and a wider base would push an invisible kerb wall
    into the sidewalk.  The post grew, it did not fatten.

    Economical on purpose: 72 placements at ~6.5% of world triangles, so the
    extra budget goes into the head (which is what the player actually looks
    at) and into chamfered arrises, not into post detail.
    """
    a = C.Asset("prop_streetlight", "prop")

    # -- cast base: fluted octagonal plinth, 0.60 m across (unchanged) --------
    # The eight flutes cost 96 triangles and, at 72 placements, read as a
    # slightly ragged silhouette rather than as fluting -- the plinth is
    # seen from 15 m up and mostly edge on.  They are replaced by a single
    # stepped collar that keeps the same footprint and the same octagonal
    # read for 28 triangles; the budget goes to the head instead, which is
    # the part actually in the camera's face.
    base = a.part("light_base", base_color=CONCRETE, roughness=0.85)
    C.cylinder(base.mesh, 0.30, 0.07, 8, center=(0, 0, 0.035), color=CONCRETE_DK)
    C.cylinder(base.mesh, 0.26, 0.19, 8, center=(0, 0, 0.135), color=CONCRETE)
    C.cylinder(base.mesh, 0.225, 0.06, 8, center=(0, 0, 0.26),
               color=C.shade(CONCRETE, 0.88))               # fluting shadow line
    C.cylinder(base.mesh, 0.19, 0.14, 8, center=(0, 0, 0.345), color=CONCRETE)
    C.cylinder(base.mesh, 0.205, 0.05, 8, center=(0, 0, 0.44), color=CONCRETE_DK)

    # -- three-point tapered column: base flare, mid shaft, top collar -------
    post = a.part("light_post", base_color=METAL_DK, metallic=0.35, roughness=0.5)
    C.cylinder(post.mesh, 0.145, 0.73, 10, center=(0, 0, 0.735),
               radius_top=0.095, color=METAL_DK)          # 0.37 -> 1.10
    C.cylinder(post.mesh, 0.115, 0.05, 10, center=(0, 0, 1.125),
               color=METAL)                                # transition collar
    C.cylinder(post.mesh, 0.096, 4.13, 10, center=(0, 0, 3.225),
               color=METAL_DK)                             # 1.15 -> 5.28
    C.cylinder(post.mesh, 0.108, 0.05, 10, center=(0, 0, 5.305),
               color=METAL)                                # top collar
    # Free albedo zoning: a painted pale band round the shaft, the way a real
    # column is marked for maintenance.  Zero triangles.
    _band(post, 1.62, 1.86, METAL)
    _band(post, 3.05, 3.17, C.shade(METAL, 1.25))

    # Access panel hatch on the post base, 600 mm AFF: a chamfered raised plate
    # with a recessed catch.  Faces -Y so it never widens the collider's X or Z
    # footprint, and it stays inside the 0.30 m base radius.
    _chamfer_at(post.mesh, (0.15, 0.045, 0.44), center=(0.0, -0.150, 0.66),
                bevel=0.016, color=METAL)
    _rbox(post.mesh, (0.10, 0.018, 0.36), center=(0.0, -0.176, 0.66),
          color=METAL_DK)
    _rbox(post.mesh, (0.035, 0.022, 0.035), center=(0.0, -0.185, 0.80),
          color=METAL)

    # -- arm sweeping out over the kerb -------------------------------------
    arm = a.part("light_arm", base_color=METAL_DK, metallic=0.35, roughness=0.5)
    path = [(0.0, 0.0, 5.30), (0.16, 0.0, 5.74), (0.40, 0.0, 6.06),
            (0.70, 0.0, 6.24), (0.98, 0.0, 6.28)]
    C.tube(arm.mesh, path, 0.072, 6, color=METAL_DK,
           radii=[0.086, 0.076, 0.070, 0.066, 0.064])
    _chamfer_at(arm.mesh, (0.19, 0.19, 0.19), center=(0, 0, 5.30), bevel=0.028,
                color=METAL_DK)                             # base boss
    _chamfer_at(arm.mesh, (0.15, 0.17, 0.24), center=(0.98, 0, 6.30),
                bevel=0.022, color=METAL_DK)                # fitter clamp

    # -- cobra head: a tapered shell that widens toward the lens and drops
    #    away underneath, built as a six-station loft of rounded-rect rings.
    #    Station values are (x, half_width, half_height, centre_z); the top
    #    of the shell is the top of the asset at 6.50 m.
    hood_stations = ((0.98, 0.085, 0.075, 6.415),
                     (1.14, 0.130, 0.105, 6.390),
                     (1.34, 0.175, 0.140, 6.360),
                     (1.56, 0.205, 0.140, 6.340),
                     (1.74, 0.205, 0.135, 6.330),
                     (1.82, 0.190, 0.115, 6.320))
    rings = []
    for (x, hy, hz, zc) in hood_stations:
        prof = C.rounded_rect(hy, hz, min(hy, hz) * 0.55, 2)
        rings.append([(x, py, pz + zc) for (py, pz) in prof])

    head = a.part("lamp_housing", base_color=(1.0, 0.90, 0.66),
                  emissive=LAMP_WARM, roughness=0.35)
    C.loft(head.mesh, rings, (1.0, 0.90, 0.66), cap_start=True, cap_end=True,
           cap_start_flip=True, cap_end_flip=False)
    # Free zoning on the shell: the crown of the shade is a shade darker than
    # the sides, so the single directional light separates top from flank.
    _band(head, 6.40, 6.55, (0.90, 0.80, 0.60))
    _band(head, 6.14, 6.26, (0.82, 0.72, 0.54))
    # Chamfered reflector tray under the nose -- the shade lip that keeps the
    # lens recessed.  It straddles the shell's bottom face by 15 mm each way.
    _chamfer_at(head.mesh, (0.44, 0.40, 0.05), center=(1.68, 0, 6.19),
                bevel=0.014, color=(0.80, 0.74, 0.60))

    # -- lens: the warm emissive disc, recessed under the tray --------------
    lens = a.part("lamp_lens", base_color=(1.0, 0.94, 0.78), emissive=LAMP_WARM,
                  roughness=0.2)
    C.cylinder(lens.mesh, 0.200, 0.05, 12, center=(1.68, 0, 6.130),
               color=(1.0, 0.94, 0.78))
    C.cylinder(lens.mesh, 0.115, 0.05, 10, center=(1.68, 0, 6.105),
               color=(1.0, 0.97, 0.88))                     # bright inner disc
    for k in range(3):                                     # photocell bar
        _rbox(lens.mesh, (0.30, 0.012, 0.012),
              center=(1.68, -0.10 + k * 0.10, 6.158), color=(0.96, 0.90, 0.72))
    return a


# ---------------------------------------------------------------------------
# 2. traffic light
# ---------------------------------------------------------------------------

def _trafficlight():
    """Signal head on a 4.60 m mast arm, plus a pedestrian head at 2.50 m.

    Proportions: a signal pole with a mast arm puts the head 4-5 m up.  The
    post runs to 4.60 m and the housing hangs from the arm with its top at
    4.12 m, comfortably inside the 4.2-5.0 m band.  The base is untouched at
    0.48 m across -- this asset uses a full-size collider.

    The three lenses are separate disc Parts with a hood over and cheeks either
    side of each, and the colour story is the traffic convention rather than
    an arbitrary palette: red and amber carry a full emissive, green carries a
    dark base colour and a ZERO emissive so it reads as "not lit" under the
    runtime's per-phase gain.
    """
    a = C.Asset("prop_trafficlight", "prop")

    base = a.part("signal_base", base_color=CONCRETE, roughness=0.85)
    C.cylinder(base.mesh, 0.24, 0.12, 8, center=(0, 0, 0.06), color=CONCRETE)
    C.cylinder(base.mesh, 0.17, 0.14, 8, center=(0, 0, 0.19), color=CONCRETE_DK)
    C.cylinder(base.mesh, 0.185, 0.04, 8, center=(0, 0, 0.28), color=CONCRETE)

    post = a.part("signal_post", base_color=METAL_DK, metallic=0.4, roughness=0.5)
    C.cylinder(post.mesh, 0.095, 4.26, 10, center=(0, 0, 2.39),
               color=METAL_DK)                             # 0.26 -> 4.52
    C.cylinder(post.mesh, 0.112, 0.06, 10, center=(0, 0, 0.99),
               color=METAL)                                # base band
    C.cylinder(post.mesh, 0.105, 0.08, 10, center=(0, 0, 4.56),
               color=METAL)                                # domed top cap
    _band(post, 3.10, 3.30, C.shade(METAL, 1.3))           # painted band

    # horizontal mast arm + diagonal brace
    C.cylinder(post.mesh, 0.065, 1.29, 8, center=(0.695, 0, 4.30), axis="X",
               color=METAL_DK)
    C.tube(post.mesh, [(0.02, 0, 3.52), (0.34, 0, 3.90), (0.68, 0, 4.16)],
           0.040, 6, color=METAL_DK)

    # -- the three-aspect housing -------------------------------------------
    housing = a.part("signal_housing", base_color=DARK, roughness=0.45)
    _chamfer_at(housing.mesh, (0.40, 0.34, 1.20), center=(1.30, 0, 3.44),
                bevel=0.026, color=DARK)
    _chamfer_at(housing.mesh, (0.46, 0.40, 0.10), center=(1.30, 0, 4.07),
                bevel=0.020, color=METAL_DK)               # rain hood
    _chamfer_at(housing.mesh, (0.44, 0.38, 0.09), center=(1.30, 0, 2.84),
                bevel=0.020, color=METAL_DK)               # bottom skirt
    _chamfer_at(housing.mesh, (0.26, 0.19, 0.13), center=(1.30, 0, 4.22),
                bevel=0.018, color=METAL_DK)               # hanger yoke
    _rbox(housing.mesh, (0.26, 0.05, 0.66), center=(1.30, 0.19, 3.44),
          color=METAL_DK)                                  # rear access panel
    _rbox(housing.mesh, (0.16, 0.035, 0.16), center=(1.30, 0.205, 2.96),
          color=(0.22, 0.23, 0.25))                        # latch

    # -- lenses: 360 mm centres, 250 mm clear discs -------------------------
    # Segment counts are cut to 10/8/10 here, and the three pieces collapse to
    # one cap thickness each.  This asset is placed 64 times, so a 12-gon
    # every lens is 96 extra world triangles for a shape the player reads as a
    # disc anyway -- the retention ring and the lit inner disc are what carry
    # the read, not their facet count.
    lens_z = (3.80, 3.44, 3.08)
    lens_c = (RED, AMBER, GREEN_DARK)
    lens_e = (SIG_RED_E, SIG_AMBER_E, SIG_OFF_E)
    for i in range(3):
        p = C.Part("signal_lens_%d" % i, base_color=lens_c[i],
                   emissive=lens_e[i], roughness=0.18)
        C.cylinder(p.mesh, 0.125, 0.07, 10, center=(1.30, -0.190, lens_z[i]),
                   axis="Y", color=lens_c[i])
        C.cylinder(p.mesh, 0.092, 0.075, 8, center=(1.30, -0.202, lens_z[i]),
                   axis="Y", color=C.shade(lens_c[i], 1.18))   # inner disc
        C.cylinder(p.mesh, 0.130, 0.03, 10, center=(1.30, -0.170, lens_z[i]),
                   axis="Y", color=C.shade(DARK, 1.25))         # retaining ring
        a.add(p)

    # -- hood over and cheeks beside every lens ----------------------------
    # The hood is CHAMFERED because it is a 300 mm plate seen from below --
    # the one surface on this asset where a lit arris actually reads.  The
    # cheeks and the lower shroud are 30-35 mm slivers viewed almost edge on,
    # so they stay hard boxes: a chamfer on a 30 mm sliver is 16 wasted
    # triangles per piece and 64 placements would pay for every one.
    #
    # y = -0.236 for the hood is not arbitrary: 0.1053 of half-depth puts the
    # leading edge at exactly -0.341, which is where it sat before this pass.
    # This asset uses a FULL-size collider, so overhanging the lens (which
    # does not change the silhouette) is free, but pushing the hood 3 cm
    # further out is a wider AABB on 64 placements for no visual gain.
    visor = a.part("signal_visors", base_color=DARK, roughness=0.5)
    for i in range(3):
        _chamfer_at(visor.mesh, (0.30, 0.21, 0.035),
                    center=(1.30, -0.236, lens_z[i] + 0.168), rot=(-18, 0, 0),
                    bevel=0.010, color=DARK)                 # hood over lens
        for sx in (-1, 1):                                  # side cheeks
            _rbox(visor.mesh, (0.035, 0.21, 0.15),
                  center=(1.30 + sx * 0.15, -0.236, lens_z[i] + 0.085),
                  color=DARK)
        _rbox(visor.mesh, (0.30, 0.14, 0.030),
              center=(1.30, -0.175, lens_z[i] - 0.158),
              color=(0.16, 0.17, 0.19))                     # lower shroud

    # -- pedestrian head on the far side of the post at 2.50 m ------------
    # Only the 260 mm case and the 300 mm rain hood are chamfered; the collar
    # and the recessed face are 30-40 mm slivers read from 10 m away, where a
    # chamfer is invisible and 64 placements would pay for it regardless.
    ped = a.part("ped_signal", base_color=DARK, roughness=0.45)
    _chamfer_at(ped.mesh, (0.26, 0.22, 0.52), center=(-0.17, 0, 2.50),
                bevel=0.022, color=DARK)
    _chamfer_at(ped.mesh, (0.30, 0.26, 0.06), center=(-0.17, 0, 2.79),
                bevel=0.014, color=METAL_DK)                # rain hood
    _rbox(ped.mesh, (0.09, 0.09, 0.16), center=(-0.10, 0, 2.50),
          color=METAL_DK)                                   # mounting collar
    _rbox(ped.mesh, (0.19, 0.03, 0.42), center=(-0.17, -0.112, 2.50),
          color=(0.10, 0.10, 0.12))                         # recessed face

    # Two aspects: an unlit orange "wait" above, a lit white walker below.
    # The walker is four flat plates (body, head, legs, arms) of 12 triangles
    # each -- at 64 placements that is 3,072 world triangles, and at 10 m the
    # silhouette is indistinguishable from a lower-segment extrusion.
    walk = a.part("ped_lens", base_color=(0.95, 0.93, 0.82),
                  emissive=(0.85, 0.85, 0.72), roughness=0.18)
    _rbox(walk.mesh, (0.16, 0.025, 0.16), center=(-0.17, -0.128, 2.66),
          color=(0.26, 0.10, 0.04))                         # unlit wait aspect
    _rbox(walk.mesh, (0.16, 0.025, 0.19), center=(-0.17, -0.128, 2.38),
          color=(0.95, 0.93, 0.82))                         # body + legs panel
    _rbox(walk.mesh, (0.055, 0.022, 0.055), center=(-0.17, -0.140, 2.44),
          color=(1.0, 0.98, 0.88))                          # head
    _rbox(walk.mesh, (0.115, 0.022, 0.032), center=(-0.17, -0.140, 2.49),
          color=(1.0, 0.98, 0.88))                          # arms
    _rbox(walk.mesh, (0.30, 0.18, 0.030), center=(-0.17, -0.135, 2.585),
          color=DARK)                                       # aspect divider
    return a


# ---------------------------------------------------------------------------
# 3. palm in a decorative pot
# ---------------------------------------------------------------------------

def _frond(m, base, azimuth, length, rise, droop, wmax, thick, color,
           stations=8, seg=6):
    """One drooping palm blade: a flattened, tapering loft along an arc."""
    dx, dy = math.cos(azimuth), math.sin(azimuth)
    pts, rad = [], []
    for i in range(stations):
        u = i / float(stations - 1)
        r = length * u
        z = base[2] + rise * u - droop * (u * u)
        pts.append((base[0] + dx * r, base[1] + dy * r, z))
        w = wmax * (0.34 + 0.66 * math.sin(math.pi * (u ** 0.7))) \
            * ((1.0 - u) ** 0.45)
        rad.append((max(w, 0.024), max(thick * math.sqrt(1.0 - u), 0.013)))

    rings = []
    for i, p in enumerate(pts):
        if i == 0:
            t = C.sub(pts[1], pts[0])
        elif i == len(pts) - 1:
            t = C.sub(pts[-1], pts[-2])
        else:
            t = C.sub(pts[i + 1], pts[i - 1])
        t = C.normalize(t)
        # ``s`` is horizontal and perpendicular to the tangent; s x up == t, which
        # is the right-hand rule loft needs.  The arc never turns vertical, so
        # the cross product below can never collapse.
        s = C.normalize(C.cross((0.0, 0.0, 1.0), t))
        up = C.cross(t, s)
        w, th = rad[i]
        rings.append([C.add(C.add(p, C.mul(s, w * math.cos(a))),
                            C.mul(up, th * math.sin(a)))
                      for a in [2.0 * math.pi * k / seg for k in range(seg)]])
    return C.loft(m, rings, color, cap_start_flip=True, cap_end_flip=False)


def _palm_planter():
    """Octagonal planter: raised rim, recessed mounded soil, nine fronds.

    The pot is a full-size collider, so the crown of fronds is the thing to
    hold steady: nine blades at 40 deg keep the 3.2 x 3.4 m footprint this
    asset already had instead of the seven-blade 51 deg spread drifting wider.
    """
    a = C.Asset("prop_palm_planter", "prop")

    pot = a.part("planter_pot", base_color=CREAM, roughness=0.8)
    C.cylinder(pot.mesh, 0.44, 0.10, 8, center=(0, 0, 0.05), color=CONCRETE)
    C.cylinder(pot.mesh, 0.42, 0.53, 8, center=(0, 0, 0.365), radius_top=0.62,
               color=CREAM)
    for k in range(8):                                  # corner pilasters
        ang = math.pi / 8.0 + k * math.pi / 4.0
        _rbox(pot.mesh, (0.085, 0.085, 0.46),
              center=(0.535 * math.cos(ang), 0.535 * math.sin(ang), 0.33),
              rot=(0, 0, math.degrees(ang)), color=CREAM)
    C.cylinder(pot.mesh, 0.585, 0.07, 8, center=(0, 0, 0.545), color=TEAL)
    C.cylinder(pot.mesh, 0.605, 0.035, 8, center=(0, 0, 0.598),
               color=C.shade(TEAL, 0.72))
    # Raised rim: an inward-sloping collar instead of the old solid plug, so
    # the soil sits in a visible recess and the rim edge catches light.
    C.cone(pot.mesh, 0.665, 0.10, 8, center=(0, 0, 0.66), radius_top=0.575,
           color=CREAM)
    C.cylinder(pot.mesh, 0.652, 0.05, 8, center=(0, 0, 0.622),
               color=C.shade(CREAM, 0.86))              # rim under-shadow
    C.cylinder(pot.mesh, 0.672, 0.025, 8, center=(0, 0, 0.6125),
               color=C.shade(CREAM, 1.06))              # rim nose highlight

    soil = a.part("planter_soil", base_color=(0.20, 0.14, 0.10), roughness=1.0)
    C.cylinder(soil.mesh, 0.555, 0.10, 8, center=(0, 0, 0.685),
               color=(0.20, 0.14, 0.10))
    _band(soil, 0.70, 0.76, (0.26, 0.19, 0.13))          # mounded, damp top
    for (dx, dy, r) in ((0.20, 0.14, 0.13), (-0.18, -0.12, 0.11),
                        (0.06, -0.22, 0.09)):
        C.sphere(soil.mesh, r, 7, 3, center=(dx, dy, 0.735), squash=0.42,
                 color=(0.30, 0.22, 0.15))              # mulch mounds
    for k in range(6):                                  # pebbles
        ang = k * math.pi / 3.0
        C.cylinder(soil.mesh, 0.035, 0.03, 6,
                   center=(0.40 * math.cos(ang), 0.40 * math.sin(ang), 0.745),
                   color=(0.44, 0.40, 0.34))

    trunk = C.Part("palm_trunk", base_color=BARK, roughness=0.9, flat=False)
    path = [(0.0, 0.0, 0.70), (0.04, 0.02, 1.38), (0.08, 0.05, 2.08),
            (0.05, 0.03, 2.64)]
    C.tube(trunk.mesh, path, 0.14, 8, smooth=True,
           radii=[0.150, 0.120, 0.101, 0.089], color=BARK)
    a.add(trunk)

    scar_c = C.shade(BARK, 0.82)
    scars = a.part("palm_scars", base_color=scar_c, roughness=0.9)
    for k in range(5):
        C.cylinder(scars.mesh, 0.128 - k * 0.011, 0.05, 8,
                   center=(0.04, 0.02, 0.97 + k * 0.33), color=scar_c)
    for k in range(5):                                  # frond-boot collars
        ang = math.radians(20.0 + k * 40.0)
        C.cylinder(scars.mesh, 0.045, 0.10, 6,
                   center=(0.05 + 0.13 * math.cos(ang),
                           0.03 + 0.13 * math.sin(ang), 2.52),
                   axis="Z", color=C.shade(BARK, 0.72))

    crown_c = C.shade(BARK, 0.92)
    crown = a.part("palm_crown", base_color=crown_c, roughness=0.9)
    C.sphere(crown.mesh, 0.15, 10, 5, center=(0.05, 0.03, 2.64), squash=0.85,
             color=crown_c)

    blades = [C.Part("palm_fronds_a", base_color=LEAF_A, roughness=0.75),
              C.Part("palm_fronds_b", base_color=LEAF_B, roughness=0.75)]
    for k in range(9):
        part = blades[k % 2]
        _frond(part.mesh, (0.05, 0.03, 2.62),
               azimuth=math.radians(20.0 + k * 40.0),
               length=1.50 + 0.11 * ((k * 5) % 3),
               rise=0.64, droop=1.30 + 0.09 * (k % 2),
               wmax=0.32, thick=0.022,
               color=part.base_color)
    for p in blades:
        a.add(p)
    return a


# ---------------------------------------------------------------------------
# 4. bench
# ---------------------------------------------------------------------------

def _bench():
    """Timber-slat bench on cast ends, 1.86 m long.

    The footprint is collider-critical (0.55 scale), so nothing here grows: the
    same 1.86 x 0.65 m envelope as before, with the slats now CHAMFERED rather
    than hard-edged.  A 50 mm slat with a 12 mm chamfer gains a lit top arris
    and a shaded under arris, which is the whole difference between "a grey
    plank" and "a plank" under a single directional light.
    """
    a = C.Asset("prop_bench", "prop")

    frame = a.part("bench_frame", base_color=METAL_DK, metallic=0.3, roughness=0.6)
    for sx in (-1, 1):
        x = sx * 0.72
        _chamfer_at(frame.mesh, (0.13, 0.64, 0.07), center=(x, 0.01, 0.035),
                    bevel=0.014, color=METAL_DK)          # ground plate
        C.box(frame.mesh, (0.09, 0.09, 0.42), center=(x, -0.20, 0.27),
              color=METAL_DK)                             # front leg
        C.box(frame.mesh, (0.09, 0.11, 0.52), center=(x, 0.22, 0.70),
              color=METAL_DK)                             # back leg
        C.box(frame.mesh, (0.07, 0.44, 0.07), center=(x, 0.01, 0.17),
              color=METAL_DK)                             # seat rail
        C.box(frame.mesh, (0.07, 0.07, 0.30), center=(x, -0.02, 0.62),
              color=METAL_DK)                             # back stile
        _chamfer_at(frame.mesh, (0.13, 0.50, 0.06), center=(x, 0.02, 0.795),
                    bevel=0.014, color=METAL_DK)          # armrest
        _chamfer_at(frame.mesh, (0.10, 0.15, 0.10), center=(x, -0.20, 0.47),
                    bevel=0.014, color=METAL_DK)          # front leg cap
        _chamfer_at(frame.mesh, (0.15, 0.15, 0.055), center=(x, -0.26, 0.030),
                    bevel=0.012, color=METAL_DK)          # toe plate

    # -- slats: chamfered, and zonaltinted so the seat and back read apart ---
    slats = a.part("bench_slats", base_color=TEAL, roughness=0.7)
    for i, y in enumerate((-0.24, -0.12, 0.0, 0.12, 0.24)):
        col = TEAL if i % 2 == 0 else C.shade(TEAL, 1.13)   # free zoning
        _chamfer_at(slats.mesh, (1.86, 0.115, 0.05), center=(0, y, 0.475),
                    bevel=0.012, color=col)
    for i, z in enumerate((0.60, 0.74, 0.88)):
        col = C.shade(TEAL, 0.86) if i == 0 else C.shade(TEAL, 1.08)
        _chamfer_at(slats.mesh, (1.86, 0.10, 0.05),
                    center=(0, 0.20 + 0.12 * (z - 0.45), z), rot=(-6.9, 0, 0),
                    bevel=0.012, color=col)

    trim = a.part("bench_trim", base_color=CREAM, roughness=0.7)
    for sx in (-1, 1):
        _chamfer_at(trim.mesh, (0.055, 0.64, 0.040), center=(sx * 0.72, 0.01,
                                                             0.078),
                    bevel=0.009, color=CREAM)             # contrast end band
    return a


# ---------------------------------------------------------------------------
# 5. trash bin
# ---------------------------------------------------------------------------

def _trash_bin():
    """Litter bin, 0.66 m across, 1.15 m tall.

    Collider-critical (0.55 scale), so the envelope is unchanged.  The quality
    work here is almost entirely FREE: a dark skirt, a mid body and a pale
    shoulder, applied with ``face_colors`` to the barrel's own loft rings, plus
    chamfers on the service hatch and the lid grip.
    """
    a = C.Asset("prop_trash_bin", "prop")

    steel = a.part("bin_body", base_color=TEAL_DK, metallic=0.25, roughness=0.6)
    C.cylinder(steel.mesh, 0.26, 0.07, 12, center=(0, 0, 0.035), color=METAL_DK)
    C.cylinder(steel.mesh, 0.24, 0.74, 14, center=(0, 0, 0.44),
               radius_top=0.30, color=TEAL_DK)
    # Free albedo zoning on the barrel: dark skirt, base teal, pale shoulder.
    _band(steel, 0.07, 0.30, C.shade(TEAL_DK, 0.62))
    _band(steel, 0.66, 0.82, C.shade(TEAL_DK, 1.30))
    for k in range(10):                                  # vertical stiffeners
        ang = k * math.pi / 5.0
        _rbox(steel.mesh, (0.045, 0.045, 0.70),
              center=(0.268 * math.cos(ang), 0.268 * math.sin(ang), 0.44),
              rot=(0, 0, math.degrees(ang)), color=TEAL_DK)
    C.cylinder(steel.mesh, 0.315, 0.06, 14, center=(0, 0, 0.845), color=METAL)

    lid = a.part("bin_lid", base_color=METAL, metallic=0.35, roughness=0.45)
    C.cone(lid.mesh, 0.33, 0.22, 12, center=(0, 0, 0.985), radius_top=0.07,
           color=C.shade(METAL, 1.12))                    # lighter than body
    C.cylinder(lid.mesh, 0.075, 0.06, 8, center=(0, 0, 1.115), color=METAL_DK)
    _chamfer_at(lid.mesh, (0.18, 0.06, 0.05), center=(0, -0.10, 1.09),
                rot=(-20, 0, 0), bevel=0.012, color=METAL_DK)   # lid grip
    _chamfer_at(lid.mesh, (0.30, 0.30, 0.05), center=(0, 0, 1.098), bevel=0.012,
                color=C.shade(METAL, 0.80))              # hood over the hatch

    trim = a.part("bin_trim", base_color=CORAL, roughness=0.6)
    C.cylinder(trim.mesh, 0.31, 0.055, 14, center=(0, 0, 0.16), color=CORAL)
    C.box(trim.mesh, (0.30, 0.10, 0.05), center=(0, -0.33, 0.055), color=METAL_DK)
    C.box(trim.mesh, (0.07, 0.07, 0.24), center=(0, -0.24, 0.20), color=METAL_DK)
    # Service hatch: the plate's half-height is 0.10, so its bottom sits at
    # z = 0.015 -- a chamfered plate that stops short of the ground rather
    # than the 0.34 m one that sank the whole bin 55 mm below the street.
    _chamfer_at(trim.mesh, (0.24, 0.05, 0.20), center=(0, -0.315, 0.115),
                bevel=0.012, color=METAL_DK)
    _chamfer_at(trim.mesh, (0.05, 0.04, 0.11), center=(0.06, -0.335, 0.135),
                bevel=0.008, color=CORAL)                # hatch catch
    return a


# ---------------------------------------------------------------------------
# 6. dumpster
# ---------------------------------------------------------------------------

def _dumpster():
    """2.38 x 1.46 x 2.24 m commercial skip on castors.

    The shell is re-stations from two rings to four purely so a horizontal
    safety band has REAL faces to sit on -- ``face_colors`` cannot band a quad
    that spans the whole body, it can only repaint that quad.  Cost is 32
    triangles for a band that reads from across the street.
    """
    a = C.Asset("prop_dumpster", "prop")

    # Open-topped tapered shell: an open surface, so it declares its outward
    # reference explicitly instead of relying on a signed volume.
    def _ring(t):
        hx, hy, r = 0.96 + 0.16 * t, 0.56 + 0.10 * t, 0.10 + 0.02 * t
        return [(x, y) for (x, y) in C.rounded_rect(hx, hy, r, 3)]

    body = C.Part("dumpster_shell", base_color=GRASS_DK, roughness=0.65,
                  outward=("point", (0.0, 0.0, 0.60)))
    C.loft(body.mesh, [[(x, y, z) for (x, y) in _ring(t)]
                       for (z, t) in ((0.26, 0.0), (0.86, 0.625),
                                      (1.14, 0.917), (1.22, 1.0))],
           GRASS_DK, cap_start=True, cap_end=False, cap_start_flip=True)
    a.add(body)
    # Colour band: dark plinth, body green, a bright reflective belt at the
    # rail line, a lighter top rail.  All free.
    _band(body, 0.26, 0.60, C.shade(GRASS_DK, 0.66))
    _band(body, 0.87, 1.13, BARRIER_ORANGE)
    _band(body, 1.15, 1.22, C.shade(GRASS_DK, 1.22))

    ribs = a.part("dumpster_ribs", base_color=C.shade(GRASS_DK, 1.18), roughness=0.6)
    for sx in (-1, 1):
        for i in range(5):
            t = -0.60 + i * 0.30
            _rbox(ribs.mesh, (0.07, 0.07, 0.94),
                  center=(sx * 1.055, t * 1.02, 0.74), color=C.shade(GRASS_DK,
                                                                      1.18))
        _rbox(ribs.mesh, (0.07, 1.34, 0.94), center=(sx * 1.055, 0.0, 0.74),
              color=C.shade(GRASS_DK, 1.18))

    rails = a.part("dumpster_rails", base_color=METAL_DK, metallic=0.3,
                   roughness=0.6)
    for sy in (-1, 1):
        _chamfer_at(rails.mesh, (2.36, 0.10, 0.09), center=(0, sy * 0.68, 1.20),
                    bevel=0.016, color=METAL_DK)
    for sx in (-1, 1):
        _chamfer_at(rails.mesh, (0.10, 1.42, 0.09), center=(sx * 1.14, 0, 1.20),
                    bevel=0.016, color=METAL_DK)
    # Two-part lid, both halves chamfered so the open angle catches light.
    _chamfer_at(rails.mesh, (2.30, 0.70, 0.07), center=(0, 0.32, 1.255),
                bevel=0.014, color=C.shade(GRASS_DK, 1.10))
    _chamfer_at(rails.mesh, (2.30, 1.30, 0.07), center=(0, -0.242, 1.718),
                rot=(50.0, 0, 0), bevel=0.014, color=C.shade(GRASS_DK, 0.94))
    for sx in (-1, 1):
        C.box(rails.mesh, (0.10, 0.10, 0.12), center=(sx * 0.90, -0.66, 1.22),
              color=METAL_DK)

    wheels = a.part("dumpster_wheels", base_color=DARK, roughness=0.85)
    for sx in (-1, 1):
        for sy in (-1, 1):
            # seg=12 puts a profile vertex exactly at 270 deg, so the wheel's
            # lowest vertex is at centre.z - r and the bin truly touches z=0.
            # A 10-gon's is at 0.995r and leaves the whole prop hovering 5 mm.
            C.cylinder(wheels.mesh, 0.105, 0.085, 12,
                       center=(sx * 0.80, sy * 0.50, 0.105), axis="X",
                       color=DARK)
            C.box(wheels.mesh, (0.07, 0.14, 0.14), center=(sx * 0.80, sy * 0.50,
                                                           0.20), color=METAL_DK)
    # Free zoning: the hub caps read as machined steel against the tyre.
    _band(wheels, 0.16, 0.25, C.shade(DARK, 2.1))

    plaque = a.part("dumpster_plaque", base_color=CREAM, roughness=0.7)
    _chamfer_at(plaque.mesh, (0.54, 0.04, 0.32), center=(0, -0.60, 0.86),
                bevel=0.012, color=CREAM)
    _rbox(plaque.mesh, (0.30, 0.02, 0.09), center=(0, -0.625, 0.90),
          color=METAL_DK)
    _rbox(plaque.mesh, (0.62, 0.05, 0.07), center=(0, -0.60, 1.05),
          color=METAL_DK)
    return a


# ---------------------------------------------------------------------------
# 7. mailbox
# ---------------------------------------------------------------------------

def _mailbox():
    """Street collection box, 0.74 x 0.68 x 1.79 m on a twin-leg stand.

    The barrel is a swept CCW (y, z) profile, so the chamfer work goes where
    the silhouette actually turns: the door panel, the leg cap plates and the
    flag, and the barrel itself is zonaltinted dark-at-the-shoulder.
    """
    a = C.Asset("prop_mailbox", "prop")

    legs = a.part("mailbox_legs", base_color=METAL_DK, metallic=0.3, roughness=0.6)
    for sx in (-1, 1):
        C.cylinder(legs.mesh, 0.072, 0.94, 10, center=(sx * 0.22, 0, 0.47),
                   color=METAL_DK)
        C.cylinder(legs.mesh, 0.115, 0.05, 10, center=(sx * 0.22, 0, 0.025),
                   color=METAL)
    _chamfer_at(legs.mesh, (0.64, 0.10, 0.10), center=(0, 0, 0.86), bevel=0.016,
                color=METAL_DK)                           # crossbar
    for sx in (-1, 1):                                    # leg cap
        _chamfer_at(legs.mesh, (0.11, 0.11, 0.05), center=(sx * 0.22, 0, 0.925),
                    bevel=0.012, color=METAL)

    # Rounded-top collection box: a CCW (y, z) profile swept along X.
    prof = [(0.28, 0.92), (0.28, 1.32)]
    prof += _arc_points(0.0, 1.32, 0.28, 0.0, math.pi, 10)[1:]
    prof.append((-0.28, 0.92))
    body = a.part("mailbox_body", base_color=TEAL, metallic=0.15, roughness=0.55)
    _loft_x(body.mesh, prof,
            [(-0.36, 0.92), (-0.31, 1.0), (0.31, 1.0), (0.36, 0.92)],
            TEAL)
    # seam band: the barrel profile dilated, then returned to flush
    _sleeve_x(body.mesh, prof,
              [(-0.06, 1.0), (-0.05, 1.035), (0.05, 1.035), (0.06, 1.0)],
              C.shade(TEAL, 0.72))
    for sx in (-1, 1):                          # corrugation ribs on the sides
        for i in range(4):
            _rbox(body.mesh, (0.055, 0.50, 0.045),
                  center=(sx * 0.345, 0.0, 1.02 + i * 0.14),
                  rot=(0, 0, 0), color=C.shade(TEAL, 1.14))

    door = a.part("mailbox_door", base_color=C.shade(TEAL, 1.12), metallic=0.15,
                  roughness=0.5)
    _chamfer_at(door.mesh, (0.64, 0.05, 0.62), center=(0, -0.288, 1.14),
                bevel=0.012, color=C.shade(TEAL, 1.12))
    _chamfer_at(door.mesh, (0.68, 0.05, 0.06), center=(0, -0.292, 1.46),
                bevel=0.010, color=C.shade(TEAL, 0.78))    # top hinge rail
    _chamfer_at(door.mesh, (0.68, 0.05, 0.06), center=(0, -0.292, 0.90),
                bevel=0.010, color=C.shade(TEAL, 0.78))    # bottom hinge rail
    C.cylinder(door.mesh, 0.035, 0.12, 8, center=(0.22, -0.33, 1.12), axis="Y",
               color=METAL)
    C.cylinder(door.mesh, 0.048, 0.10, 8, center=(0.22, -0.325, 1.12),
               axis="Y", color=C.shade(METAL, 0.7))         # handle boss

    slot = a.part("mailbox_slot", base_color=DARK, roughness=0.9)
    _chamfer_at(slot.mesh, (0.46, 0.06, 0.07), center=(0, -0.315, 1.34),
                bevel=0.010, color=DARK)
    _rbox(slot.mesh, (0.52, 0.04, 0.03), center=(0, -0.318, 1.385),
          color=C.shade(TEAL, 0.58))                        # hood over the slot

    label = a.part("mailbox_label", base_color=PAPER, roughness=0.85)
    _chamfer_at(label.mesh, (0.36, 0.02, 0.12), center=(-0.05, -0.315, 1.06),
                bevel=0.006, color=PAPER)
    _rbox(label.mesh, (0.22, 0.015, 0.03), center=(-0.05, -0.327, 1.05),
          color=(0.30, 0.32, 0.40))                        # collection plate

    flag = a.part("mailbox_flag", base_color=RED, roughness=0.6)
    _chamfer_at(flag.mesh, (0.04, 0.04, 0.28), center=(0.30, 0.0, 1.63),
                bevel=0.008, color=METAL_DK)
    _chamfer_at(flag.mesh, (0.04, 0.17, 0.10), center=(0.30, 0.07, 1.76),
                bevel=0.010, color=RED)                     # raised flag
    return a


# ---------------------------------------------------------------------------
# 8. newsstand
# ---------------------------------------------------------------------------

def _newsstand():
    """Kiosk, 2.34 x 2.25 x 3.12 m.

    Collider-critical (0.55 scale), so the 2.34 x 2.25 m awning-inclusive
    footprint is held exactly.  The zoning does the heavy lifting: a dark
    plinth, a cream carcass, a teal counter and a much darker roof, so the
    silhouette is described by material as well as by normal.
    """
    a = C.Asset("prop_newsstand", "prop")

    body = a.part("newsstand_body", base_color=CREAM, roughness=0.75)
    _chamfer_at(body.mesh, (2.10, 1.50, 0.12), center=(0, 0, 0.06), bevel=0.020,
                color=CONCRETE)                            # ground plinth
    _chamfer_at(body.mesh, (2.02, 1.44, 0.10), center=(0, 0, 0.16), bevel=0.016,
                color=C.shade(TEAL, 0.82))                 # dark base band
    for sx in (-1, 1):
        for sy in (-1, 1):
            _chamfer_at(body.mesh, (0.13, 0.13, 2.20),
                        center=(sx * 0.94, sy * 0.66, 1.26), bevel=0.018,
                        color=CREAM)                       # corner posts
    _chamfer_at(body.mesh, (2.02, 0.10, 2.06), center=(0, 0.66, 1.17),
                bevel=0.016, color=CREAM)                 # rear wall
    for sx in (-1, 1):
        _chamfer_at(body.mesh, (0.10, 1.32, 2.06), center=(sx * 0.94, 0, 1.17),
                    bevel=0.016, color=C.shade(CREAM, 0.93))   # side walls
    _chamfer_at(body.mesh, (1.86, 0.50, 0.94), center=(0, -0.38, 0.63),
                bevel=0.018, color=TEAL)                   # counter front
    _chamfer_at(body.mesh, (2.00, 0.62, 0.08), center=(0, -0.40, 1.13),
                bevel=0.014, color=CORAL)                  # counter top
    _rbox(body.mesh, (1.92, 0.05, 0.10), center=(0, -0.66, 1.02), color=CREAM)
    for z in (1.15, 1.62):                                 # magazine shelves
        _chamfer_at(body.mesh, (1.80, 0.34, 0.05), center=(0, 0.46, z),
                    bevel=0.010, color=CONCRETE_DK)
        _rbox(body.mesh, (1.80, 0.03, 0.07), center=(0, 0.31, z + 0.06),
              color=C.shade(CONCRETE_DK, 0.8))             # shelf lip

    roof = a.part("newsstand_roof", base_color=C.shade(METAL, 0.62),
                  metallic=0.3, roughness=0.5)
    _chamfer_at(roof.mesh, (2.34, 1.78, 0.09), center=(0, 0, 2.38), bevel=0.018,
                color=C.shade(METAL, 0.72))                # dark roof deck
    _chamfer_at(roof.mesh, (2.26, 1.70, 0.15), center=(0, 0, 2.47), bevel=0.024,
                color=C.shade(METAL, 0.60))                # dark parapet
    for i in range(5):                                     # standing seams
        _chamfer_at(roof.mesh, (0.075, 1.66, 0.06),
                    center=(-0.90 + i * 0.45, 0, 2.565), bevel=0.010,
                    color=C.shade(METAL, 0.44))
    for sx in (-1, 1):                                     # sign stanchions
        _chamfer_at(roof.mesh, (0.09, 0.09, 0.38), center=(sx * 0.62, 0, 2.72),
                    bevel=0.012, color=METAL_DK)

    awning = a.part("newsstand_awning", base_color=PINK, roughness=0.6)
    _chamfer_at(awning.mesh, (2.06, 0.88, 0.06), center=(0, -0.92, 1.72),
                rot=(-16.0, 0, 0), bevel=0.014, color=PINK)
    _rbox(awning.mesh, (2.06, 0.05, 0.18), center=(0, -1.31, 1.53), color=CORAL)
    for sx in (-1, 1):
        _rbox(awning.mesh, (0.05, 0.88, 0.06),
              center=(sx * 1.01, -0.92, 1.72), color=C.shade(PINK, 0.85))
    for i in range(6):                                     # awning scallops
        _rbox(awning.mesh, (0.05, 0.05, 0.16),
              center=(-0.85 + i * 0.34, -1.295, 1.585), color=C.shade(PINK, 1.1))

    sign = a.part("newsstand_sign", base_color=MINT, emissive=(0.25, 0.85, 0.70),
                  roughness=0.35)
    _chamfer_at(sign.mesh, (1.64, 0.11, 0.46), center=(0, -0.08, 2.90),
                bevel=0.018, color=MINT)
    _rbox(sign.mesh, (1.70, 0.05, 0.06), center=(0, -0.14, 2.66),
          color=METAL_DK)
    _chamfer_at(sign.mesh, (1.50, 0.04, 0.10), center=(0, -0.145, 2.96),
                bevel=0.008, color=C.shade(MINT, 1.18))     # lightbox diffuser
    _rbox(sign.mesh, (0.30, 0.04, 0.22), center=(0, -0.15, 2.90), color=CREAM)

    mags = a.part("newsstand_mags", base_color=PAPER, roughness=0.8)
    for i in range(7):
        x = -0.66 + i * 0.22
        _chamfer_at(mags.mesh, (0.15, 0.035, 0.30),
                    center=(x, -0.50 + 0.05 * ((i % 3) - 1), 1.30), bevel=0.006,
                    color=(PAPER if i % 2 else CORAL))
        _chamfer_at(mags.mesh, (0.17, 0.035, 0.26),
                    center=(x, -0.52 + 0.04 * ((i % 3) - 1), 1.10), bevel=0.006,
                    color=(MINT if i % 2 else PINK))
    return a


# ---------------------------------------------------------------------------
# 9. phone booth
# ---------------------------------------------------------------------------

def _phone_booth():
    """Call box, 1.24 x 1.27 x 2.94 m, glazed on three sides.

    Collider-critical (0.55 scale): the 1.24 m canopy is the widest thing on
    the asset and stays exactly where it was.  The new work is all INSIDE the
    glass, where it costs no collider at all -- a payphone handset on the back
    wall with a coiled cord, and a brighter ceiling lamp plus a diffuser panel
    and a faint floor bounce so the interior reads as lit at night.
    """
    a = C.Asset("prop_phone_booth", "prop")

    shell = a.part("booth_frame", base_color=TEAL, metallic=0.2, roughness=0.5)
    _chamfer_at(shell.mesh, (1.18, 1.18, 0.15), center=(0, 0, 0.075),
                bevel=0.020, color=CONCRETE)              # ground plinth
    _chamfer_at(shell.mesh, (1.08, 1.08, 0.12), center=(0, 0, 0.19),
                bevel=0.016, color=METAL_DK)              # kick plate
    for sx in (-1, 1):
        for sy in (-1, 1):
            _chamfer_at(shell.mesh, (0.11, 0.11, 2.04),
                        center=(sx * 0.46, sy * 0.46, 1.26), bevel=0.016,
                        color=TEAL)                        # corner mullions
    _chamfer_at(shell.mesh, (1.08, 1.08, 0.16), center=(0, 0, 2.30),
                bevel=0.022, color=TEAL)                  # fascia beam
    _chamfer_at(shell.mesh, (1.24, 1.24, 0.13), center=(0, 0, 2.44),
                bevel=0.024, color=C.shade(METAL, 0.80))  # canopy
    _chamfer_at(shell.mesh, (0.94, 0.94, 0.10), center=(0, 0, 2.56),
                bevel=0.018, color=CREAM)                 # cap

    finial = a.part("booth_finial", base_color=CORAL, roughness=0.6)
    C.cone(finial.mesh, 0.15, 0.22, 8, center=(0, 0, 2.71), color=CORAL)
    C.cylinder(finial.mesh, 0.045, 0.12, 8, center=(0, 0, 2.88), color=METAL)
    C.cylinder(finial.mesh, 0.070, 0.05, 8, center=(0, 0, 2.945), color=CORAL)

    glass = a.part("booth_glass", base_color=GLASS, emissive=(0.10, 0.22, 0.24),
                   roughness=0.12)
    C.box(glass.mesh, (0.84, 0.05, 1.70), center=(0, 0.46, 1.19), color=GLASS)
    for sx in (-1, 1):
        C.box(glass.mesh, (0.05, 0.84, 1.70), center=(sx * 0.46, 0, 1.19),
              color=GLASS)
    C.box(glass.mesh, (0.78, 0.05, 1.62), center=(0, -0.46, 1.15), color=GLASS)
    # Free zoning: a paler band across each pane where light rakes it, so the
    # glazing is not one dead sheet of colour.
    _band(glass, 1.86, 2.02, C.shade(GLASS, 1.35))

    mullions = a.part("booth_mullions", base_color=CREAM, roughness=0.6)
    for sx in (-1, 1):
        C.box(mullions.mesh, (0.055, 0.07, 1.74), center=(sx * 0.40, -0.47, 1.15),
              color=CREAM)
    _chamfer_at(mullions.mesh, (0.86, 0.07, 0.055), center=(0, -0.47, 1.15),
                bevel=0.010, color=CREAM)                 # door rail
    for z in (1.66, 0.62):
        _chamfer_at(mullions.mesh, (0.80, 0.06, 0.055), center=(0, -0.48, z),
                    bevel=0.010, color=C.shade(CREAM, 0.9))
    C.box(mullions.mesh, (0.05, 0.05, 0.16), center=(0.33, -0.50, 1.12),
          color=METAL)                                     # door pull

    interior = a.part("booth_interior", base_color=CREAM, roughness=0.8)
    _chamfer_at(interior.mesh, (0.80, 0.30, 0.06), center=(0, 0.28, 0.98),
                bevel=0.010, color=CONCRETE_DK)            # shelf
    _chamfer_at(interior.mesh, (0.26, 0.18, 0.56), center=(0, 0.35, 1.30),
                bevel=0.016, color=C.shade(TEAL, 0.72))    # phone body
    _chamfer_at(interior.mesh, (0.20, 0.10, 0.10), center=(0, 0.28, 1.62),
                bevel=0.012, color=C.shade(TEAL, 0.62))    # ear/mouth piece
    _chamfer_at(interior.mesh, (0.10, 0.04, 0.16), center=(0, 0.245, 1.12),
                bevel=0.006, color=CREAM)                  # coin slot strip
    _chamfer_at(interior.mesh, (0.34, 0.06, 0.26), center=(0, 0.26, 1.80),
                bevel=0.012, color=CREAM)                  # instruction card

    # -- payphone handset and its coiled cord, on the back wall -------------
    phone = a.part("booth_payphone", base_color=(0.20, 0.22, 0.24),
                   roughness=0.45)
    _chamfer_at(phone.mesh, (0.07, 0.09, 0.30), center=(-0.155, 0.235, 1.30),
                bevel=0.012, color=(0.20, 0.22, 0.24))    # handset cradle
    for z in (1.42, 1.18):                                  # earpiece / mouthpiece
        _chamfer_at(phone.mesh, (0.085, 0.10, 0.075), center=(-0.155, 0.215, z),
                    bevel=0.014, color=(0.26, 0.28, 0.31))
    _chamfer_at(phone.mesh, (0.05, 0.10, 0.10), center=(-0.16, 0.16, 1.20),
                rot=(24, 0, 0), bevel=0.010, color=(0.26, 0.28, 0.31))
    C.cylinder(phone.mesh, 0.020, 0.16, 6, center=(0.10, 0.245, 1.30),
               axis="Y", color=(0.30, 0.32, 0.35))         # keypad block
    # Coiled cord: a helix, so it hangs like a real one instead of a straight
    # wire, and it stays clear of the glass.
    coil = []
    for i in range(17):
        u = i / 16.0
        ang = 2.0 * math.pi * 3.0 * u
        coil.append((-0.115 + 0.045 * math.cos(ang),
                     0.185 + 0.030 * math.sin(ang),
                     1.17 - 0.34 * u))
    C.tube(phone.mesh, coil, 0.011, 5, color=(0.18, 0.19, 0.21))

    # -- internal lighting: lamp, diffuser, floor bounce --------------------
    lamp = a.part("booth_lamp", base_color=(1.0, 0.95, 0.82),
                  emissive=(0.95, 0.92, 0.78), roughness=0.2)
    C.cylinder(lamp.mesh, 0.12, 0.07, 10, center=(0, 0, 2.19),
               color=(1.0, 0.95, 0.82))
    _chamfer_at(lamp.mesh, (0.46, 0.30, 0.05), center=(0, 0.02, 2.205),
                bevel=0.010, color=(1.0, 0.96, 0.86))     # ceiling diffuser
    _rbox(lamp.mesh, (0.34, 0.22, 0.02), center=(0, 0.04, 2.175),
          color=(0.92, 0.88, 0.76))
    C.cylinder(lamp.mesh, 0.30, 0.02, 12, center=(0, 0.02, 0.255),
               color=(0.86, 0.84, 0.74))                  # floor bounce pool

    sign = a.part("booth_sign", base_color=CORAL, emissive=(0.85, 0.30, 0.20),
                  roughness=0.35)
    _chamfer_at(sign.mesh, (0.92, 0.06, 0.24), center=(0, -0.60, 2.44),
                bevel=0.010, color=CORAL)
    _rbox(sign.mesh, (0.10, 0.06, 0.46), center=(0, -0.62, 2.62), color=CREAM)
    _rbox(sign.mesh, (0.80, 0.03, 0.06), center=(0, -0.635, 2.44),
          color=C.shade(CORAL, 1.2))                        # legend band
    return a


# ---------------------------------------------------------------------------
# 10. fire hydrant
# ---------------------------------------------------------------------------

def _fire_hydrant():
    """Hydrant, 0.62 m across, 1.05 m tall -- a correct real-world size.

    Collider-critical (0.55 scale), so the envelope is unchanged.  Two real
    wins here: the 120-triangle sphere bonnet becomes a 28-triangle CHAMFERED
    bonnet block, which is both cheaper and -- with a faceted crown under a
    single directional light -- more legible than a smooth dome; and the
    barrel is a darker red under a pale bonnet, which is the zoning the
    classic dry-barrel hydrant already has painted on it.
    """
    a = C.Asset("prop_fire_hydrant", "prop")

    BARREL = C.shade(RED, 0.88)
    body = a.part("hydrant_body", base_color=RED, roughness=0.55)
    C.cylinder(body.mesh, 0.215, 0.07, 12, center=(0, 0, 0.035), color=METAL_DK)
    C.cylinder(body.mesh, 0.135, 0.58, 12, center=(0, 0, 0.36), color=BARREL)
    C.cylinder(body.mesh, 0.30, 0.05, 12, center=(0, 0, 0.60), color=BARREL)
    C.cone(body.mesh, 0.205, 0.16, 12, center=(0, 0, 0.73), radius_top=0.105,
           color=CREAM)                                    # pale shoulder
    # Chamfered bonnet block replaces a 12x6 UV sphere (120 tris -> 28) and
    # gives the crown four lit facets and four shaded ones.
    _chamfer_at(body.mesh, (0.30, 0.30, 0.14), center=(0, 0, 0.80), bevel=0.048,
                color=CREAM)
    _chamfer_at(body.mesh, (0.20, 0.20, 0.05), center=(0, 0, 0.885), bevel=0.016,
                color=C.shade(CREAM, 0.86))

    caps = a.part("hydrant_caps", base_color=CREAM, roughness=0.6)
    for sx in (-1, 1):                     # side hose outlets, axis X
        C.cylinder(caps.mesh, 0.062, 0.13, 10, center=(sx * 0.175, 0, 0.47),
                   axis="X", color=CREAM)
        C.cone(caps.mesh, 0.078, 0.07, 10, center=(sx * 0.275, 0, 0.47),
               axis="X", color=CREAM)
    C.cylinder(caps.mesh, 0.058, 0.12, 10, center=(0, -0.175, 0.47), axis="Y",
               color=CREAM)
    C.cone(caps.mesh, 0.074, 0.07, 10, center=(0, -0.27, 0.47), axis="Y",
           color=CREAM)
    C.cylinder(caps.mesh, 0.038, 0.09, 6, center=(0, 0, 0.95), color=METAL)
    C.cone(caps.mesh, 0.055, 0.07, 8, center=(0, 0, 1.02), color=METAL)
    _rbox(caps.mesh, (0.15, 0.035, 0.02), center=(0, 0, 0.965),
          color=C.shade(METAL, 0.82))                      # operating nut bar

    bolts = a.part("hydrant_bolts", base_color=METAL_DK, metallic=0.4,
                   roughness=0.5)
    for k in range(4):
        ang = math.pi / 4.0 + k * math.pi / 2.0
        C.cylinder(bolts.mesh, 0.020, 0.05, 6,
                   center=(0.175 * math.cos(ang), 0.175 * math.sin(ang), 0.085),
                   color=METAL_DK)

    # Chain links as a catenary tube rather than toruses: core.torus()'s
    # axis="Y" branch maps (x, y, z) -> (x, z, y), determinant -1, which mirrors
    # the shell inside out (every face inverted, negative signed volume).  tube()
    # builds its frame from the tangent and is correct on every path.
    chain = a.part("hydrant_chain", base_color=METAL_DK, metallic=0.5,
                   roughness=0.45)
    links = [(0.02, -0.10, 0.66), (0.10, -0.14, 0.56), (0.17, -0.15, 0.49),
             (0.19, -0.16, 0.46)]
    for i in range(3):
        C.tube(chain.mesh, [links[i], links[i + 1]], 0.016, 6, color=METAL_DK)
    C.cylinder(chain.mesh, 0.028, 0.05, 6, center=links[0], color=METAL_DK)
    C.cylinder(chain.mesh, 0.028, 0.05, 6, center=links[-1], color=METAL_DK)

    band = a.part("hydrant_band", base_color=CREAM, roughness=0.6)
    C.cylinder(band.mesh, 0.142, 0.09, 12, center=(0, 0, 0.25), color=CREAM)
    C.cylinder(band.mesh, 0.146, 0.03, 12, center=(0, 0, 0.285),
               color=C.shade(CREAM, 0.82))                 # band shadow line
    return a


# ---------------------------------------------------------------------------
# 11. manhole cover -- a ground decal, deliberately wafer thin
# ---------------------------------------------------------------------------

def _manhole_cover():

    a = C.Asset("prop_manhole_cover", "prop")

    iron = a.part("cover_iron", base_color=IRON, metallic=0.45, roughness=0.7)
    C.cylinder(iron.mesh, 0.38, 0.024, 24, center=(0, 0, 0.012), color=IRON)
    # Free zoning: the outer flange sits a shade under the field, which is
    # how a real cover's seating ring reads in raking light.
    _band(iron, 0.0, 0.012, C.shade(IRON, 0.80))

    field = a.part("cover_field", base_color=C.shade(IRON, 1.18), metallic=0.45,
                   roughness=0.62)
    C.cylinder(field.mesh, 0.33, 0.028, 20, center=(0, 0, 0.014),
               color=C.shade(IRON, 1.18))

    # The 8 ribs used to be built in the SAME colour as the field they sit on,
    # which made this the only fully achromatic prop in the set -- every base
    # colour satisfied r == g == b, so the cover had no hue at all and read as
    # a grey disc.  They are their own part now, shaded a stop darker, so the
    # rib shadows separate from the plate instead of being invisible on it.
    ribs = a.part("cover_ribs", base_color=C.shade(IRON, 0.72), metallic=0.45,
                  roughness=0.55)
    for k in range(8):
        ang = k * math.pi / 4.0
        _rbox(ribs.mesh, (0.22, 0.035, 0.030), rot=(0, 0, math.degrees(ang)),
              center=(0.20 * math.cos(ang), 0.20 * math.sin(ang), 0.015),
              color=C.shade(IRON, 0.72))
    C.cylinder(ribs.mesh, 0.255, 0.016, 20, center=(0, 0, 0.016),
               color=C.shade(IRON, 0.86))                   # inner ring

    # Hub with a REAL recessed pull slot.  It was a solid chamfered key block,
    # so the one feature a player would reach for was a raised blank square --
    # the opposite of a lifting hole.  The slot is a dark inset bar sunk into
    # the key, flanked by the two remaining cheeks, which is what makes the
    # cover look like it can be lifted rather than being a welded disc.
    hub = a.part("cover_hub", base_color=IRON, metallic=0.45, roughness=0.7)
    # A chamfered key block instead of a 10-gon hub: fewer triangles (28 vs 40)
    # and a lit arris all round, on the one piece the player looks straight at.
    _chamfer_at(hub.mesh, (0.15, 0.15, 0.030), center=(0, 0, 0.015), bevel=0.011,
                color=IRON)
    _chamfer_at(hub.mesh, (0.07, 0.07, 0.020), center=(0, 0, 0.028), bevel=0.007,
                color=C.shade(IRON, 1.45))                  # lifting key
    # The slot itself: a shallow sunk bar in near-black, reading as a void
    # because it is both darker than every neighbouring face AND set below the
    # key's top plane.
    slot = a.part("cover_pull_slot", base_color=C.shade(IRON, 0.34),
                  metallic=0.30, roughness=0.85)
    _chamfer_at(slot.mesh, (0.115, 0.030, 0.012), center=(0, 0, 0.0355),
                bevel=0.004, color=C.shade(IRON, 0.34))
    return a


# ---------------------------------------------------------------------------
# 12. traffic cone
# ---------------------------------------------------------------------------

def _traffic_cone():

    a = C.Asset("prop_traffic_cone", "prop")

    base = a.part("cone_base", base_color=ORANGE, roughness=0.7)
    # Chamfered skirt: a 44 cm square plate is a big flat area facing the
    # camera, and a chamfer is the only thing that will break it up.
    _chamfer_at(base.mesh, (0.44, 0.44, 0.05), center=(0, 0, 0.025), bevel=0.012,
                color=ORANGE)
    _chamfer_at(base.mesh, (0.36, 0.36, 0.035), center=(0, 0, 0.055),
                bevel=0.010, color=C.shade(ORANGE, 0.88))

    body = a.part("cone_body", base_color=ORANGE, roughness=0.7)
    rings = [[(r * math.cos(a_), r * math.sin(a_), z)
              for a_ in [2.0 * math.pi * i / 12 for i in range(12)]]
             for (r, z) in ((0.165, 0.06), (0.140, 0.17), (0.088, 0.42),
                            (0.038, 0.68))]
    C.loft(body.mesh, rings, ORANGE, cap_start_flip=True, cap_end_flip=False)
    # Free zoning: the apex above the top band is scuffed paler, the skirt
    # below the bottom band is dirtied.  Zero triangles.
    _band(body, 0.53, 0.68, C.shade(ORANGE, 1.22))
    _band(body, 0.06, 0.25, C.shade(ORANGE, 0.78))

    bands = a.part("cone_bands", base_color=WHITE, roughness=0.45)
    for (z0, z1, r0, r1) in ((0.255, 0.345, 0.115, 0.092),
                             (0.455, 0.520, 0.076, 0.058)):
        C.cylinder(bands.mesh, r0, z1 - z0, 12, center=(0, 0, (z0 + z1) * 0.5),
                   radius_top=r1, color=WHITE)
    return a


# ---------------------------------------------------------------------------
# 13. construction barrier -- A-frame with a flashing beacon
# ---------------------------------------------------------------------------

def _construction_barrier():
    """Orange/white striped A-frame road barrier with an amber lamp.

    Origin at the centre of the ground footprint, resting on z = 0 like every
    other prop, so it drops straight onto the kerb without a runtime tweak.

    The stripes are REAL alternating slabs, not one panel painted with
    face colours.  A single 2 m box is 12 triangles and only two distinct
    centroid X positions, so banding it by X produced one broad orange half
    and one broad cream half.  Each slab is now CHAMFERED, so every stripe has
    a lit top arris against the one behind it and the panel reads as eight
    boards rather than a painted wall.
    """
    a = C.Asset("prop_construction_barrier", "prop")

    HW, HH, HD = 0.62, 0.28, 0.055          # half width / height / plank depth
    span = 2.00                             # foot span along X
    leg_rise = 0.86                         # how high the A-frame legs stand

    # -- feet ---------------------------------------------------------------
    feet = a.part("barrier_feet", base_color=METAL_DK, metallic=0.35,
                  roughness=0.6)
    for sx in (-1, 1):
        _chamfer_at(feet.mesh, (0.30, 0.42, 0.048),
                    center=(sx * span * 0.5, 0.0, 0.024), bevel=0.011,
                    color=METAL_DK)

    # -- the striped plank --------------------------------------------------
    plank = a.part("barrier_plank", base_color=BARRIER_ORANGE, roughness=0.62)
    n_band = 8
    bw = span / n_band
    zc = leg_rise + 0.22
    for i in range(n_band):
        col = BARRIER_ORANGE if i % 2 == 0 else BARRIER_WHITE
        _chamfer_at(plank.mesh, (bw, HD * 2.0, HH * 2.0),
                    center=(-span * 0.5 + bw * (i + 0.5), 0.0, zc),
                    bevel=0.014, color=col)
    # Reflective tape stripes top and bottom, free zoning on the slab faces.
    _band(plank, zc + HH - 0.075, zc + HH, C.shade(BARRIER_ORANGE, 1.25))

    # -- A-frame legs -------------------------------------------------------
    legs = a.part("barrier_legs", base_color=BARRIER_WHITE, roughness=0.66)
    # The legs tilt 7 deg outboard, so the chamfered block's lowest corner is
    # NOT at centre.z - hz: it is at
    #     hz*cos(7) - hy*sin(7)
    # which for this 80 mm section is 0.00086 m LOWER than a flat-bottomed
    # box of the same length.  Lifting the centre by exactly that amount puts
    # the corner back on z = 0; leaving the old 0.96 length sank the barrier
    # 1.3 mm into the road.
    tilt = 7.0
    leg_len = leg_rise + 0.10
    sink = 0.5 * leg_len * math.cos(math.radians(tilt)) \
        - 0.5 * 0.080 * math.sin(math.radians(tilt)) - 0.048
    for sx in (-1, 1):
        _chamfer_at(legs.mesh, (0.080, 0.080, leg_len),
                    center=(sx * span * 0.5, 0.0, 0.5 * leg_len + sink),
                    rot=(0.0, sx * tilt, 0.0), bevel=0.014,
                    color=BARRIER_WHITE)
    _chamfer_at(legs.mesh, (span * 0.92, 0.065, 0.065), center=(0.0, 0.0, 0.075),
                bevel=0.012, color=BARRIER_WHITE)           # lower spreader
    for sx in (-1, 1):                                      # hinge blocks
        _chamfer_at(legs.mesh, (0.10, 0.13, 0.09), center=(sx * span * 0.5, 0.0,
                                                           leg_rise + 0.24),
                    bevel=0.012, color=C.shade(BARRIER_WHITE, 0.78))

    # -- amber beacon on top ------------------------------------------------
    beacon_body = a.part("barrier_beacon", base_color=METAL_DK, metallic=0.4,
                         roughness=0.45)
    C.cylinder(beacon_body.mesh, 0.070, 0.035, 10,
               center=(0.0, 0.0, leg_rise + 0.22 + HH + 0.018),
               color=METAL_DK)
    lamp = a.part("barrier_lamp", base_color=BEACON_AMBER, emissive=BEACON_AMBER,
                  roughness=0.18)
    # Both a solid colour AND an emissive tint, so the lamp still reads as an
    # object when the runtime scales the emissive channel to zero.
    C.cylinder(lamp.mesh, 0.058, 0.070, 10,
               center=(0.0, 0.0, leg_rise + 0.22 + HH + 0.070),
               color=BEACON_AMBER)
    C.cylinder(lamp.mesh, 0.042, 0.055, 10,
               center=(0.0, 0.0, leg_rise + 0.22 + HH + 0.085),
               color=C.shade(BEACON_AMBER, 1.30))           # bright core
    C.cylinder(beacon_body.mesh, 0.062, 0.018, 10,
               center=(0.0, 0.0, leg_rise + 0.22 + HH + 0.114),
               color=METAL_DK)                                   # rain cap
    return a


# ---------------------------------------------------------------------------
# 14. parking meter -- slim post, head box, coin slot
# ---------------------------------------------------------------------------

def _parking_meter():
    """Kerbside parking meter: slim post, angled head box, coin slot."""
    a = C.Asset("prop_parking_meter", "prop")

    post_h = 1.08
    post = a.part("meter_post", base_color=METER_BODY, metallic=0.45,
                  roughness=0.48)
    C.cylinder(post.mesh, 0.115, 0.055, 10, center=(0.0, 0.0, 0.0275),
               color=METER_TRIM)                                # cast base
    C.cylinder(post.mesh, 0.052, post_h, 10, center=(0.0, 0.0, post_h * 0.5),
               color=METER_BODY)                                # column
    C.cylinder(post.mesh, 0.060, 0.040, 10, center=(0.0, 0.0, 0.075),
               color=METER_TRIM)                                # base collar
    C.cylinder(post.mesh, 0.058, 0.035, 10, center=(0.0, 0.0, 0.92),
               color=METER_TRIM)                                # banded collar

    # Head box, tipped back 14 deg so the face looks up at a driver.
    head = C.Part("meter_head", base_color=METER_BODY, metallic=0.45,
                  roughness=0.44)
    _chamfer_at(head.mesh, (0.190, 0.150, 0.290), center=(0.0, 0.0, 0.0),
                bevel=0.020, color=METER_BODY)
    _chamfer_at(head.mesh, (0.206, 0.166, 0.032), center=(0.0, 0.0, 0.130),
                bevel=0.010, color=METER_TRIM)                  # top cap
    _chamfer_at(head.mesh, (0.210, 0.170, 0.024), center=(0.0, 0.0, -0.138),
                bevel=0.009, color=C.shade(METER_BODY, 0.72))  # bottom cap
    C.cylinder(head.mesh, 0.030, 0.110, 8, center=(0.0, 0.0, -0.150),
               color=METER_BODY)                                # neck into post
    _place([head], offset=(0.0, 0.0, post_h + 0.150), rot=(-14.0, 0.0, 0.0))
    a.add(head)

    face = C.Part("meter_face", base_color=METER_FACE, roughness=0.35)
    _chamfer_at(face.mesh, (0.150, 0.016, 0.190), center=(0.0, -0.082, 0.010),
                bevel=0.005, color=METER_FACE)                  # display panel
    # Free zoning: a lit LCD band across the top third of the display.
    _band(face, 0.055, 0.098, (0.42, 0.74, 0.70))
    _place([face], offset=(0.0, 0.0, post_h + 0.150), rot=(-14.0, 0.0, 0.0))
    a.add(face)

    trim = C.Part("meter_trim", base_color=METER_TRIM, metallic=0.5,
                  roughness=0.4)
    # Coin slot, proud of the face by 5 mm: a real recess shadow rather than a
    # coplanar decal that z-fights with the panel behind it.
    _chamfer_at(trim.mesh, (0.022, 0.018, 0.058), center=(0.0, -0.086, -0.088),
                bevel=0.005, color=METER_SLOT)
    _chamfer_at(trim.mesh, (0.090, 0.016, 0.026), center=(0.0, -0.086, -0.062),
                bevel=0.005, color=METER_TRIM)                 # coin shelf lip
    for sx in (-1, 1):                                          # display bezel
        _chamfer_at(trim.mesh, (0.018, 0.018, 0.190),
                    center=(sx * 0.083, -0.082, 0.010), bevel=0.005,
                    color=METER_TRIM)
    for z in (0.105, -0.085):                                   # bezel caps
        _chamfer_at(trim.mesh, (0.184, 0.018, 0.018), center=(0.0, -0.082, z),
                    bevel=0.005, color=METER_TRIM)
    _place([trim], offset=(0.0, 0.0, post_h + 0.150), rot=(-14.0, 0.0, 0.0))
    a.add(trim)
    return a


# ---------------------------------------------------------------------------

def build_all():
    """Return the ordered list of street-prop assets."""
    return [
        _streetlight(),
        _trafficlight(),
        _palm_planter(),
        _bench(),
        _trash_bin(),
        _dumpster(),
        _mailbox(),
        _newsstand(),
        _phone_booth(),
        _fire_hydrant(),
        _manhole_cover(),
        _traffic_cone(),
        _construction_barrier(),
        _parking_meter(),
    ]
