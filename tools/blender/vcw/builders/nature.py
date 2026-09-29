"""Nature: the living world -- terrain, ground cover and the animals in it.

Everything here is original low-poly geometry assembled from the ``core``
kernel; nothing is imported or traced.  Seven assets, in the order
``build_all`` returns them:

* ``terrain_mountain`` -- a craggy rock mass, ~60 m across the base and ~30 m
  tall, closed-lofted through a stack of shrinking, drifting, radially-varied
  rings so the silhouette is asymmetric and faceted rather than conical.
* ``grass_patch``      -- a clump of 14 leaning blades on a low hummock.
* ``flower_cluster``   -- 4 blooms on leaning stems, with petal fans and leaves.
* ``animal_dog``       -- 0.95 m, four legs, chamfered skull, dropped ears.
* ``animal_bird``      -- 0.8 m wingspan, perched, wings half spread.
* ``animal_cat``       -- 0.45 m, pointed ears, tube-swept curled tail.
* ``animal_fish``      -- 0.5 m, lofted body, four one-sided fin sheets.

AUTHORING.  Blender-style Z-up, ``-Y`` forward, metres, centred on ``X = 0``.
The four animals, the grass and the flowers stand on ``Z = 0``; the fish is
centred on the origin instead, because a fish is a free-swimming prop of the
kind ``weapons.py`` already parents to a spawn point rather than a footprint.

THE ONE RULE THIS MODULE INVENTS ITSELF.  Every genuinely OPEN surface in
here is a sheet whose faces can be wound to a single declared direction, and
each such part passes exactly one ``outward=``:

* grass blades and bird wings are vertical extrusions -- a blade of grass
  stands up, so no matter how far it leans its faces still point at least
  slightly at the sky.  ``_face`` enforces the sign per polygon and the part
  declares ``("dir", (0, 0, 1))``.
* petal fans, leaves and the bird's tail are near-horizontal sheets: one
  ``("dir", (0, 0, 1))``.
* the fish's dorsal and anal fins are vertical sheets in the ``YZ`` plane and
  its caudal fin is a vertical sheet in the ``XZ`` plane, so each is its own
  part with the axis it actually faces: ``("dir", (1, 0, 0))``,
  ``("dir", (-1, 0, 0))`` and ``("dir", (0, 1, 0))``.

That restriction is deliberate.  ``verify_outward``'s ``("point", ref)`` mode
compares a face normal against the direction from ``ref`` to the face
centroid, which is only a meaningful test when the reference is genuinely
inside the solid.  A zero-thickness fin has no interior, so any reference
point gives a near-zero -- and therefore arbitrary -- dot product.  Winding a
sheet to one axis and declaring that axis is the only way to make the check
sound rather than accidentally-passing.

COLOUR.  The renderer has one directional light, no specular term and no
texture (see CONTRACT.md section 1), so per-face albedo and per-face normals
are the only two things that make a surface read.  Every asset here does both:
facet breaks come from flat-shaded lofted geometry, and value variation comes
from ``_facet_noise``, which nudges each face's own colour off its zone
colour so a single flat wall never renders as one dead value.
"""

import math

from .. import core as C

# ---------------------------------------------------------------------------
# palette -- shares palms.py's greens so the boulevard and the ground cover
# read as one world, and weapons.py's neons for the fish and the blooms
# ---------------------------------------------------------------------------

# --- rock ---
ROCK = (0.44, 0.41, 0.38)
ROCK_LIT = (0.57, 0.53, 0.48)
ROCK_BARE = (0.63, 0.60, 0.56)
ROCK_DEEP = (0.30, 0.28, 0.27)
SCRUB_LO = (0.20, 0.37, 0.19)
SCRUB_HI = (0.31, 0.50, 0.24)

# --- vegetation ---
GRASS_A = (0.33, 0.64, 0.28)
GRASS_B = (0.46, 0.73, 0.31)
GRASS_C = (0.26, 0.53, 0.23)
GRASS_DK = (0.17, 0.39, 0.18)
LEAF_A = (0.28, 0.54, 0.24)
LEAF_B = (0.37, 0.63, 0.27)
STEM = (0.20, 0.39, 0.19)
SOIL = (0.31, 0.25, 0.19)

# --- blooms ---
BLOOM_PINK = (0.96, 0.34, 0.53)
BLOOM_MAGENTA = (0.80, 0.22, 0.63)
BLOOM_CORAL = (0.97, 0.47, 0.32)
BLOOM_AMBER = (0.98, 0.76, 0.24)

# --- mammals ---
FAWN = (0.72, 0.55, 0.34)
FAWN_LT = (0.86, 0.71, 0.48)
FAWN_DK = (0.50, 0.35, 0.21)
CREAM = (0.90, 0.86, 0.77)
CAT_GREY = (0.50, 0.50, 0.54)
CAT_DK = (0.33, 0.33, 0.38)
CAT_CREAM = (0.88, 0.83, 0.72)
NOSE_DK = (0.19, 0.14, 0.12)
EYE_DK = (0.09, 0.08, 0.09)

# --- bird ---
BIRD_WHITE = (0.93, 0.94, 0.95)
BIRD_GREY = (0.44, 0.50, 0.60)
BIRD_GREY_DK = (0.29, 0.34, 0.44)
BEAK_ORANGE = (0.98, 0.55, 0.15)

# --- fish ---
FISH_TEAL = (0.15, 0.61, 0.59)
FISH_TEAL_DK = (0.09, 0.40, 0.42)
FISH_SILVER = (0.76, 0.83, 0.86)
FISH_ORANGE = (0.98, 0.47, 0.15)


# ---------------------------------------------------------------------------
# local helpers on top of the kernel primitives
# ---------------------------------------------------------------------------

def _face(m, pts, color, axis):
    """Emit ``pts`` (3 or 4 of them) wound so its normal points along ``axis``.

    This is the single mechanism behind every ``outward=`` declaration in this
    module: build the polygon in whatever order the arithmetic produced, then
    reverse the whole vertex list if its normal leans the wrong way.  For a
    flat sheet reversing the list also reverses the triangulation diagonal, so
    both triangles of a quad stay consistent.
    """
    n = C.normalize(C.cross(C.sub(pts[1], pts[0]), C.sub(pts[2], pts[0])))
    if C.dot(n, axis) < 0.0:
        pts = list(reversed(pts))
    if len(pts) == 3:
        m.tri(pts[0], pts[1], pts[2], color)
    else:
        m.quad(pts[0], pts[1], pts[2], pts[3], color)
    return m


def _ring_z(z, rx, ry, seg, jag=0.0, phase=0.0, cx=0.0, cy=0.0):
    """A CCW-in-XY ring in the plane ``z``, advancing along +Z.

    ``jag`` scales the radius per segment by a deterministic three-harmonic
    ripple, which is what turns a cone into crags: the same phase and segment
    index give the same value on every build, so the mountain is byte-stable.
    """
    out = []
    for i in range(seg):
        a = 2.0 * math.pi * i / seg + phase
        f = 1.0 + jag * (0.55 * math.sin(3.0 * a) + 0.30 * math.sin(5.0 * a)
                         + 0.15 * math.sin(7.0 * a))
        out.append((cx + rx * f * math.cos(a), cy + ry * f * math.sin(a), z))
    return out


def _ring_y(y, rx, rz, seg, cz=0.0, cx=0.0):
    """A ring in the plane ``y``, advancing along +Y.

    NOTE the negated Z.  Walking a circle as ``(r cos a, y, r sin a)`` sweeps
    CCW as seen from ``-Y``, so the loop's right-hand normal is ``-Y`` and
    ``loft`` -- which must advance ALONG the loop's right-hand direction --
    would fill the shell inside-out.  ``loft`` also owns the cap flips, so the
    pair has to be right: ``cap_start_flip=True`` on the front ring, ``False``
    on the back.
    """
    return [(cx + rx * math.cos(a), y, cz - rz * math.sin(a))
            for a in [2.0 * math.pi * i / seg for i in range(seg)]]


def _dome(m, radius, height, seg, colour):
    """A closed ground-flattened dome: sphere cap above z = 0, flat base below.

    ``core.sphere`` cannot express this.  It always emits a FULL closed shell
    with a pole at each end, so flattening the bottom means burying half the
    sphere under the ground plane -- invisible geometry that also drags the
    asset's min-Z below zero.  This builds the upper cap as a fan, then a
    ring, then a base disc at z = 0 sharing both rims, which keeps the shell
    closed, single-component and positive-volume.

    The base disc is why this is a closed shell and needs no ``outward``:
    the hummock is a solid, not a membrane, even though a real soil hummock
    is really only its upper surface that anyone ever sees.
    """
    rings = []
    for v in range(1, 4):                      # 3 cap bands below the apex
        t = math.pi * 0.5 * v / 3.0
        z = height * math.cos(t)
        r = radius * math.sin(t)
        if v == 3:                              # last band: flatten to z = 0
            r = radius
            z = 0.0
        rings.append([(r * math.cos(2.0 * math.pi * i / seg),
                       r * math.sin(2.0 * math.pi * i / seg), z)
                      for i in range(seg)])

    # Every ring is APPENDED to the mesh first, then referenced by index --
    # the same discipline core.loft() uses, so both rims of every band edge
    # are shared by exactly two faces and the shell stays watertight.
    apex = len(m.pos)
    m.pos.append((0.0, 0.0, height))
    spans = []
    for ring in rings:
        base = len(m.pos)
        for p in ring:
            m.pos.append((p[0], p[1], p[2]))
        spans.append(base)
    base_centre = len(m.pos)
    m.pos.append((0.0, 0.0, 0.0))

    # Rows DESCEND here (smallest/highest ring first), which is the same
    # situation core.sphere() documents for its own pole-to-pole sweep: the
    # advancing direction is -Z while the ring's right-hand normal is +Z, so
    # the band quads come out mirrored relative to a +Z-ascending loft.
    # Emitting the ascending order instead leaves a perfectly watertight
    # shell whose signed volume is NEGATIVE -- every face pointing into the
    # soil -- which the exporter rejects.
    for j in range(seg):                       # apex fan, pointing +Z
        m.tri_idx(apex, spans[0] + j, spans[0] + (j + 1) % seg, colour)
    for k in range(len(spans) - 1):            # the cap bands
        a, b = spans[k], spans[k + 1]
        for j in range(seg):
            j2 = (j + 1) % seg
            m.quad_idx(b + j, b + j2, a + j2, a + j, colour)
    rim = spans[-1]                            # base disc, pointing -Z
    for j in range(seg):
        m.tri_idx(rim + j, rim + (j + 1) % seg, base_centre, colour)
    return m


def _facet_noise(part, amp=0.10, seed=0.0):
    """Nudge every face's own colour off its zone colour, deterministically.

    One directional light plus a constant ambient means the ONLY value
    difference available on an untextured surface is albedo or normal
    (CONTRACT.md section 1).  Facet breaks already come from flat shading;
    this supplies the other half, at zero triangles.  Multiplied, not
    replaced, so a zone colour set by ``recolor_faces_where`` survives.
    """
    m = part.mesh
    for i, c in enumerate(m.fcol):
        m.fcol[i] = C.shade(c, 1.0 + amp * math.sin(1.7 * i + seed))
    return part


# ===========================================================================
# 1. terrain_mountain -- ~60 m across the base, ~30 m tall, resting on Z = 0
# ===========================================================================

# (z, radius_x, radius_y, drift_x, drift_y) -- read bottom to top.  The radii
# fall faster than the height rises at the bottom (a broad apron), then the
# drift walks the axis sideways so the summit is NOT over the base centre:
# that off-centre peak is most of what makes a mountain read as a mountain.
MTN_RINGS = (
    (0.00, 30.0, 27.4, 0.0, 0.0),
    (1.10, 28.6, 26.1, 0.7, -0.6),
    (2.80, 25.9, 23.6, 1.5, -1.3),
    (4.90, 22.6, 20.5, 2.0, -2.0),
    (7.40, 18.9, 17.1, 2.2, -2.7),
    (10.20, 15.1, 13.9, 1.8, -3.2),
    (13.10, 11.6, 10.9, 1.1, -3.5),
    (16.00, 8.5, 8.2, 0.3, -3.4),
    (19.00, 5.9, 5.9, -0.6, -3.0),
    (22.00, 3.7, 3.9, -1.5, -2.4),
    (25.00, 2.0, 2.2, -2.3, -1.7),
    (27.60, 0.95, 1.05, -2.9, -1.0),
    (29.80, 0.30, 0.34, -3.2, -0.6),
)

MTN_SEG = 16


def _surface_z(mesh, x, y):
    """Topmost height of ``mesh`` above the column (x, y), or 0.0 if none.

    A DOWNWARD ray cast against the real triangle list, not an analytic test
    against the ring table.  The analytic version is unstable here and was
    quietly so: the ring-to-ring quads are steeply sloped facets, so a
    parameter-space point-in-quad test returns a wild reading (0.0, i.e. "no
    hit", for columns that plainly sit on the mountain) the moment the query
    column lands in a crevice between two facets.  Cast the actual geometry
    and the answer is exact by construction.

    Plane-then-barycentric, so a near-vertical facet -- whose XY projection is
    a sliver, or degenerates entirely -- is hit as reliably as a flat one.
    """
    o = (x, y, 1.0e5)
    d = (0.0, 0.0, -1.0)
    best = 0.0
    for (ia, ib, ic) in mesh.faces:
        a, b, c = mesh.pos[ia], mesh.pos[ib], mesh.pos[ic]
        n = C.cross(C.sub(b, a), C.sub(c, a))
        den = C.dot(n, d)
        if abs(den) < 1e-12:                       # ray parallel to the face
            continue
        t = C.dot(n, C.sub(a, o)) / den
        if t <= 0.0:                               # behind the ray
            continue
        p = C.add(o, C.mul(d, t))
        # barycentric containment in the triangle's own 3D frame
        v0 = C.sub(b, a)
        v1 = C.sub(c, a)
        v2 = C.sub(p, a)
        dd = C.dot(v0, v0) * C.dot(v1, v1) - C.dot(v0, v1) ** 2
        if abs(dd) < 1e-15:
            continue
        s = (C.dot(v1, v1) * C.dot(v0, v2) - C.dot(v0, v1) * C.dot(v1, v2)) / dd
        t2 = (C.dot(v0, v0) * C.dot(v1, v2) - C.dot(v0, v1) * C.dot(v0, v2)) / dd
        if s >= -1e-6 and t2 >= -1e-6 and s + t2 <= 1.0 + 1e-6:
            if p[2] > best:
                best = p[2]
    return best


def _mountain():
    a = C.Asset("terrain_mountain", "terrain")

    # 13 rings x 16 segments -> 12 gaps x 16 quads + two 14-triangle cap fans
    # = 380 triangles.  The apex is a 0.3 m ring rather than a true point so
    # the top cap stays a real fan instead of seg near-degenerate slivers.
    rings = [_ring_z(z, rx, ry, MTN_SEG,
                     jag=0.16 - 0.075 * (k / float(len(MTN_RINGS) - 1)),
                     phase=0.37 * k, cx=dx, cy=dy)
             for k, (z, rx, ry, dx, dy) in enumerate(MTN_RINGS)]

    # Re-centre on X=0: the drift above makes the peak off-centre, but the
    # HOUSE RULE is that an asset is centred on X=0, so the BASE ring -- not
    # the summit -- is the pivot.  Doing it on the finished ring list means no
    # hand-tuned constant can drift.
    bx = sum(p[0] for p in rings[0]) / MTN_SEG
    by = sum(p[1] for p in rings[0]) / MTN_SEG
    rings = [[(p[0] - bx, p[1] - by, p[2]) for p in r] for r in rings]

    rock = a.part("terrain_mountain_rock", base_color=ROCK, roughness=0.95)
    C.loft(rock.mesh, rings, ROCK, cap_start_flip=True, cap_end_flip=False)

    # Colour zones by centroid height: scrub at the apron, warm rock through
    # the middle, cold bare rock on the summit.  The two scrub passes run
    # first and are mutually exclusive, so neither can eat the other's faces.
    C.recolor_faces_where(rock, lambda n, c: c[2] < 4.2, SCRUB_LO)
    C.recolor_faces_where(rock, lambda n, c: 4.2 <= c[2] < 8.6, SCRUB_HI)
    C.recolor_faces_where(rock, lambda n, c: 15.0 <= c[2] < 23.5, ROCK_LIT)
    C.recolor_faces_where(rock, lambda n, c: c[2] >= 23.5, ROCK_BARE)
    # Downward-facing undersides go deep: those are the overhangs and the
    # crevices, and a single light will never reach them, so they need the
    # albedo to carry the separation.
    C.recolor_faces_where(rock, lambda n, c: n[2] < -0.42, ROCK_DEEP)
    _facet_noise(rock, amp=0.11, seed=0.4)

    # Three talus boulders half-buried in the apron, so the mass meets the
    # ground on something other than a mathematically perfect circle.
    #
    # Each boulder is seated by RAY-CASTING the finished rock shell, then sunk
    # by a fraction of its own radius.  The sink is the part that matters: a
    # ball centred exactly on the surface of a 30-degree slope has its
    # downhill half hanging in mid-air with a visible gap, which is exactly
    # how the first version of this looked.  Burying the lower 30-40% puts
    # the widest part of the sphere below the surface on every side, so the
    # intersection is a closed contact patch rather than a floating cap.
    # Riding the exact surface height also means a change to MTN_RINGS moves
    # the boulders with it instead of entombing or levitating them.
    outcrop = a.part("terrain_mountain_outcrop", base_color=ROCK_LIT,
                     roughness=0.96)
    for (ang_deg, dist, r, sq, sink) in ((150.0, 21.5, 2.45, 0.62, 0.42),
                                         (320.0, 19.5, 1.95, 0.55, 0.46),
                                         (28.0, 24.5, 1.55, 0.50, 0.40)):
        ang = math.radians(ang_deg)
        x, y = dist * math.cos(ang), dist * math.sin(ang)
        # Average the surface over the boulder's own footprint, not just at
        # its centre: on a faceted slope the centre column can land in a
        # crevice a couple of metres below the mean of the ring around it.
        h = _surface_z(rock.mesh, x, y)
        ring = [_surface_z(rock.mesh, x + r * 0.7 * math.cos(2.0 * math.pi * k
                                                             / 6.0),
                           y + r * 0.7 * math.sin(2.0 * math.pi * k / 6.0))
                for k in range(6)]
        h = max(h, sum(ring) / 6.0)          # never sit below the mean
        C.sphere(outcrop.mesh, r, 6, 3,
                 center=(x, y, h + r * sq - r * sink),
                 squash=sq, color=ROCK_LIT)
    C.recolor_faces_where(outcrop, lambda n, c: n[2] < -0.40, ROCK_DEEP)
    _facet_noise(outcrop, amp=0.13, seed=1.9)
    return a


# ===========================================================================
# 2. grass_patch -- ~0.8 m across, ~0.6 m tall, 14 blades, resting on Z = 0
# ===========================================================================

GRASS_HUM_R = 0.300
GRASS_HUM_TOP = 0.024


def _hum_z(r):
    """Height of the clump's soil hummock at radius ``r``.

    The blades are planted ON this surface rather than at a flat z = 0, which
    is what stops a blade at the edge of the clump from hovering a centimetre
    above its own mound.
    """
    k = r / GRASS_HUM_R
    if k >= 1.0:
        return 0.0
    return GRASS_HUM_TOP * (1.0 - math.sqrt(1.0 - k * k))


def _blade(m, base, az, height, lean, droop, w_root, stations=5,
           c_root=GRASS_A, c_tip=GRASS_DK):
    """One grass blade: a tapering strip swept along a leaning arc.

    The centreline is ``height*(u - droop*u^3)`` in Z and ``lean*u`` along the
    blade's own horizontal heading, so the vertical tangent is never zero:
    the surface is spanned by the width axis (horizontal) and the tangent, and
    a tangent with a horizontal component of ``lean`` guarantees every face
    normal keeps a strictly positive Z.  That is what makes
    ``outward=("dir", (0, 0, 1))`` an honest declaration instead of a hope.
    """
    dx, dy = math.cos(az), math.sin(az)
    wx, wy = -dy, dx                      # width axis: horizontal, across
    left, right = [], []
    for i in range(stations):
        u = i / float(stations - 1)
        off = lean * u
        z = base[2] + height * (u - droop * u * u * u)
        cx, cy = base[0] + dx * off, base[1] + dy * off
        hw = 0.5 * w_root * (1.0 - 0.86 * u * u) * (1.0 - 0.12 * u)
        left.append((cx + wx * hw, cy + wy * hw, z))
        right.append((cx - wx * hw, cy - wy * hw, z))
    for i in range(stations - 2):
        _face(m, [left[i], left[i + 1], right[i + 1], right[i]],
              c_root if i else C.shade(c_root, 1.12), (0.0, 0.0, 1.0))
    # Tip: one triangle closing the taper to a point on the same arc.
    tip = (left[-1][0] + (left[-1][0] - left[-2][0]) * 1.6,
           left[-1][1] + (left[-1][1] - left[-2][1]) * 1.6,
           left[-1][2] + (left[-1][2] - left[-2][2]) * 1.6)
    _face(m, [left[-2], tip, right[-2]], c_tip, (0.0, 0.0, 1.0))
    return m


def _grass_patch():
    a = C.Asset("grass_patch", "nature")

    # The clump's foot: a low squashed dome, open at the bottom by the ground
    # plane.  A closed shell, so no ``outward`` needed.  The sphere is CUT at
    # z = 0 rather than sunk below it -- ``sphere`` always emits a closed
    # shell, and a hummock whose lower hemisphere hangs 0.4 m under the
    # ground would be invisible geometry AND would drag this asset's bounds
    # min-Z below zero for no visual gain.  The cap fan that would have
    # covered the cut is the only triangle deliberately removed, and the
    # surrounding rim is sealed instead, keeping the shell closed and
    # positive-volume.
    soil = a.part("grass_patch_soil", base_color=SOIL, roughness=0.98)
    _dome(soil.mesh, GRASS_HUM_R, GRASS_HUM_TOP, seg=9, colour=SOIL)
    C.recolor_faces_where(soil, lambda n, c: n[2] > 0.45, GRASS_C)
    _facet_noise(soil, amp=0.14, seed=3.1)

    # 14 blades, two hue families so the clump never reads as one flat green.
    ga = a.part("grass_patch_blades_a", base_color=GRASS_A, roughness=0.74,
                outward=("dir", (0.0, 0.0, 1.0)))
    gb = a.part("grass_patch_blades_b", base_color=GRASS_B, roughness=0.74,
                outward=("dir", (0.0, 0.0, 1.0)))
    for k in range(14):
        r = 0.030 + 0.100 * ((k * 5) % 7) / 6.0
        base_az = 2.39996 * k                          # golden-angle spread
        bx, by = r * math.cos(base_az), r * math.sin(base_az)
        lean = 0.215 + 0.055 * ((k * 3) % 4) / 3.0
        _blade((ga if k % 2 == 0 else gb).mesh,
               (bx, by, _hum_z(r)),
               az=base_az + 0.31 * math.sin(3.1 * k),
               height=0.455 + 0.070 * ((k * 3) % 5) / 4.0,
               lean=lean,
               droop=0.13 + 0.07 * ((k * 2) % 3) / 2.0,
               w_root=0.030 + 0.011 * (k % 3),
               stations=5,
               c_root=(GRASS_A, GRASS_C)[k % 2],
               c_tip=(GRASS_DK, GRASS_C)[k % 2])
    _facet_noise(ga, amp=0.09, seed=5.0)
    _facet_noise(gb, amp=0.09, seed=6.0)
    return a


# ===========================================================================
# 3. flower_cluster -- 4 blooms, ~0.5 m tall, resting on Z = 0
# ===========================================================================

# (x, y, height, lean_x, lean_y, petal colour) -- the head leans with the stem
# so the clump has its own wind direction rather than four vertical copies.
BLOOMS = (
    (-0.082, 0.040, 0.445, 0.052, 0.018, BLOOM_PINK),
    (0.058, 0.062, 0.500, -0.040, 0.030, BLOOM_MAGENTA),
    (0.014, -0.068, 0.355, 0.018, -0.050, BLOOM_CORAL),
    (-0.048, -0.058, 0.400, -0.030, -0.026, BLOOM_PINK),
)


def _petal_head(m, centre, colour, phase, n_petals=6):
    """A flat petal fan: ``n_petals`` tapered quads radiating from a hub.

    The hub sits slightly BELOW the tips so the fan cups upward, which keeps
    every face normal's Z positive and gives the bloom a dish rather than a
    decal.  All of it is one open sheet, hence ``outward=("dir", (0, 0, 1))``
    on the owning part.
    """
    r_in, r_out, hub_drop = 0.014, 0.056, 0.007
    for k in range(n_petals):
        a = 2.0 * math.pi * k / n_petals + phase
        hw = math.pi / n_petals * 0.86
        ix0, iy0 = r_in * math.cos(a - hw * 0.55), r_in * math.sin(a - hw * 0.55)
        ix1, iy1 = r_in * math.cos(a + hw * 0.55), r_in * math.sin(a + hw * 0.55)
        ox0, oy0 = r_out * math.cos(a - hw), r_out * math.sin(a - hw)
        ox1, oy1 = r_out * math.cos(a + hw), r_out * math.sin(a + hw)
        zl = centre[2] - hub_drop
        zt = centre[2] + 0.013
        _face(m, [(centre[0] + ix0, centre[1] + iy0, zl),
                  (centre[0] + ox0, centre[1] + oy0, zt),
                  (centre[0] + ox1, centre[1] + oy1, zt),
                  (centre[0] + ix1, centre[1] + iy1, zl)],
              colour, (0.0, 0.0, 1.0))
        _face(m, [(centre[0] + ox0, centre[1] + oy0, zt),
                  (centre[0] + ix0, centre[1] + iy0, zl),
                  (centre[0] + ix1, centre[1] + iy1, zl),
                  (centre[0] + ox1, centre[1] + oy1, zt)],
              C.shade(colour, 0.84), (0.0, 0.0, 1.0))
    return m


def _leaf(m, base, az, length, drop, colour):
    """A narrow leaf blade splaying out of the stem and tipping downward.

    Near-horizontal, so ``_face`` keeps every normal's Z positive.  Three
    stations then a tip triangle: 5 triangles for a leaf that still reads at
    20 m, which is the whole point of the tri budget.
    """
    dx, dy = math.cos(az), math.sin(az)
    wx, wy = -dy, dx
    left, right = [], []
    for i, (u, w) in enumerate(((0.0, 0.5), (0.45, 0.5), (0.80, 0.32))):
        off = length * u
        z = base[2] - drop * u * u
        hw = 0.030 * w
        left.append((base[0] + dx * off + wx * hw,
                     base[1] + dy * off + wy * hw, z))
        right.append((base[0] + dx * off - wx * hw,
                      base[1] + dy * off - wy * hw, z))
    for i in range(2):
        _face(m, [left[i], left[i + 1], right[i + 1], right[i]],
              colour if i == 0 else C.shade(colour, 0.88), (0.0, 0.0, 1.0))
    tip = (left[-1][0] + (left[-1][0] - left[-2][0]) * 0.7,
           left[-1][1] + (left[-1][1] - left[-2][1]) * 0.7,
           left[-1][2] - 0.012)
    _face(m, [left[1], tip, right[1]], C.shade(colour, 0.78), (0.0, 0.0, 1.0))
    return m


def _flower_cluster():
    a = C.Asset("flower_cluster", "nature")

    stems = a.part("flower_cluster_stems", base_color=STEM, roughness=0.8)
    leaves = a.part("flower_cluster_leaves", base_color=LEAF_A, roughness=0.76,
                    outward=("dir", (0.0, 0.0, 1.0)))
    petals = a.part("flower_cluster_petals", base_color=BLOOM_PINK,
                    roughness=0.62, outward=("dir", (0.0, 0.0, 1.0)))
    cores = a.part("flower_cluster_cores", base_color=BLOOM_AMBER,
                   roughness=0.55)

    for k, (x, y, h, lx, ly, colour) in enumerate(BLOOMS):
        # Three path points, the first TWO of them exactly vertical: tube()
        # derives its first ring from the 0->1 secant, so a stem that leaned
        # immediately would hang its base ring below z = 0 by r*sin(tilt).
        top = (x + lx, y + ly, h)
        C.tube(stems.mesh,
               [(x, y, 0.0), (x, y, h * 0.42), (x + lx * 0.55,
                                                 y + ly * 0.55, h * 0.76),
                top],
               0.0105, 5, color=STEM,
               radii=[0.0105, 0.0092, 0.0078, 0.0068])
        _petal_head(petals.mesh, top, colour, phase=0.41 * k, n_petals=6)
        C.cylinder(cores.mesh, 0.0155, 0.013, 6,
                   center=(top[0], top[1], top[2] + 0.001), color=BLOOM_AMBER)
        az = 0.9 * k + 0.6
        for s in (-1, 1):
            lz = h * (0.34 if s < 0 else 0.56)
            _leaf(leaves.mesh, (x, y, lz), az + (0.0 if s < 0 else 2.1),
                  0.115 + 0.030 * (k % 2), 0.030,
                  LEAF_A if k % 2 == 0 else LEAF_B)

    C.recolor_faces_where(cores, lambda n, c: n[2] < -0.45,
                          C.shade(BLOOM_AMBER, 0.78))
    _facet_noise(petals, amp=0.07, seed=7.3)
    _facet_noise(leaves, amp=0.08, seed=8.1)
    return a


# ===========================================================================
# 4. animal_dog -- 0.95 m long, 0.45 m at the shoulder, 0.25 m wide
# ===========================================================================

# (y, half_width, half_height, centre_z) -- a deep chest tapering to the
# rump, with the tallest point just behind the forelegs.  Nose at y = -0.62,
# tail tip at y = +0.42: 0.90 m nose-to-tail-root, and ~0.45 m at the shoulder.
DOG_BODY = (
    (-0.240, 0.072, 0.073, 0.330),
    (-0.164, 0.097, 0.102, 0.326),
    (-0.044, 0.105, 0.113, 0.317),
    (0.080, 0.102, 0.108, 0.315),
    (0.192, 0.090, 0.097, 0.318),
    (0.276, 0.065, 0.072, 0.327),
)


def _quad_leg(m, x, y, z_top, r_bot, r_top, paw_r, paw_h, colour, dark):
    """One leg: a tapered shin plus a slightly wider paw pad."""
    C.cylinder(m, r_bot, z_top - paw_h, 5, center=(x, y, paw_h +
                                                  (z_top - paw_h) * 0.5),
               radius_top=r_top, color=colour)
    C.cylinder(m, paw_r, paw_h, 5, center=(x, y - 0.012, paw_h * 0.5),
               color=dark)
    return m


def _animal_dog():
    a = C.Asset("animal_dog", "animal")

    body = a.part("animal_dog_body", base_color=FAWN, roughness=0.8)
    C.loft(body.mesh, [_ring_y(y, rx, rz, 8, cz=cz) for (y, rx, rz, cz) in DOG_BODY],
           FAWN, cap_start_flip=True, cap_end_flip=False)
    # Neck: two rings raked forward and up off the chest.  Listed low-y first
    # so the loft advances +Y, which is what ``_ring_y``'s winding demands.
    # The upper ring tops out at z = 0.450, which IS the shoulder height the
    # brief asks for -- everything above it (skull, ears) is head, not body.
    C.loft(body.mesh, [_ring_y(-0.330, 0.054, 0.050, 8, cz=0.400),
                       _ring_y(-0.240, 0.071, 0.070, 8, cz=0.352)],
           FAWN, cap_start_flip=True, cap_end_flip=False)
    # Counter-shading: the belly is the only surface the ground bounce reaches.
    C.recolor_faces_where(body, lambda n, c: n[2] < -0.42, FAWN_LT)
    C.recolor_faces_where(body, lambda n, c: c[2] < 0.16 and n[2] > 0.30,
                          C.shade(FAWN, 1.10))
    _facet_noise(body, amp=0.06, seed=11.0)

    legs = a.part("animal_dog_legs", base_color=FAWN, roughness=0.8)
    for (x, y, top) in ((-0.069, -0.164, 0.285), (0.069, -0.164, 0.285),
                        (-0.074, 0.190, 0.295), (0.074, 0.190, 0.295)):
        _quad_leg(legs.mesh, x, y, top, 0.026, 0.036, 0.031, 0.028,
                  FAWN, FAWN_DK)
    _facet_noise(legs, amp=0.07, seed=12.0)

    head = a.part("animal_dog_head", base_color=FAWN, roughness=0.78)
    C.chamfer_box(head.mesh, (0.098, 0.123, 0.098),
                  center=(0.0, -0.376, 0.412), color=FAWN,
                  bevel=0.020, n_corner=1)
    C.recolor_faces_where(head, lambda n, c: n[2] < -0.50, FAWN_LT)
    for sx in (-1, 1):                                   # eyes
        C.box(head.mesh, (0.011, 0.022, 0.018),
              center=(sx * 0.048, -0.404, 0.434), color=EYE_DK)

    muzzle = a.part("animal_dog_muzzle", base_color=FAWN_LT, roughness=0.72)
    C.chamfer_box(muzzle.mesh, (0.060, 0.090, 0.052),
                  center=(0.0, -0.466, 0.384), color=FAWN_LT,
                  bevel=0.015, n_corner=1)
    C.box(muzzle.mesh, (0.038, 0.018, 0.027), center=(0.0, -0.512, 0.394),
          color=NOSE_DK)                                  # nose leather

    ears = a.part("animal_dog_ears", base_color=FAWN_DK, roughness=0.82)
    for sx in (-1, 1):
        C.cone(ears.mesh, 0.038, 0.078, 5,
               center=(sx * 0.044, -0.354, 0.490), color=FAWN_DK)
    C.recolor_faces_where(ears, lambda n, c: n[2] > 0.55, FAWN)

    # The tail is a carriage dog carried low, not a spitz's plume: it stays
    # below the rump so the silhouette's top line is the spine, not the tail.
    tail = a.part("animal_dog_tail", base_color=FAWN, roughness=0.78)
    C.tube(tail.mesh,
           [(0.0, 0.270, 0.318), (0.0, 0.340, 0.348),
            (0.0, 0.376, 0.394), (0.0, 0.368, 0.442),
            (0.0, 0.338, 0.468)],
           0.025, 5, color=FAWN,
           radii=[0.025, 0.022, 0.019, 0.015, 0.011])
    return a


# ===========================================================================
# 5. animal_bird -- 0.8 m wingspan, perched on Z = 0
# ===========================================================================

# (span_x, leading_y, trailing_y, z) -- the wing sweeps BACK as it goes out
# and rises slightly, so the span reads as one gesture instead of two planks.
BIRD_WING = (
    (0.040, -0.062, 0.074, 0.132),
    (0.118, -0.052, 0.084, 0.140),
    (0.216, -0.026, 0.088, 0.152),
    (0.316, 0.006, 0.084, 0.161),
    (0.394, 0.030, 0.074, 0.167),
)

# (y, half_width, half_height, centre_z)
BIRD_BODY = (
    (-0.126, 0.017, 0.019, 0.116),
    (-0.090, 0.038, 0.044, 0.118),
    (-0.030, 0.050, 0.056, 0.120),
    (0.030, 0.048, 0.052, 0.120),
    (0.080, 0.034, 0.036, 0.120),
    (0.116, 0.014, 0.016, 0.122),
)


def _animal_bird():
    a = C.Asset("animal_bird", "animal")

    body = a.part("animal_bird_body", base_color=BIRD_WHITE, roughness=0.72)
    C.loft(body.mesh,
           [_ring_y(y, rx, rz, 8, cz=cz) for (y, rx, rz, cz) in BIRD_BODY],
           BIRD_WHITE, cap_start_flip=True, cap_end_flip=False)
    # White belly, grey mantle: the counter-shading is what makes it a bird.
    C.recolor_faces_where(body, lambda n, c: n[2] < -0.30, BIRD_WHITE)
    C.recolor_faces_where(body, lambda n, c: n[2] > 0.42, BIRD_GREY)
    C.recolor_faces_where(body, lambda n, c: c[2] > 0.086 and n[2] > 0.0,
                          BIRD_GREY)
    _facet_noise(body, amp=0.05, seed=21.0)

    legs = a.part("animal_bird_legs", base_color=BEAK_ORANGE, roughness=0.6)
    for sx in (-1, 1):
        C.cylinder(legs.mesh, 0.0075, 0.058, 4, center=(sx * 0.021, 0.004,
                                                        0.029),
                   color=BEAK_ORANGE)
        C.box(legs.mesh, (0.040, 0.034, 0.009),
              center=(sx * 0.021, -0.008, 0.0045), color=BEAK_ORANGE)

    head = a.part("animal_bird_head", base_color=BIRD_WHITE, roughness=0.66)
    C.sphere(head.mesh, 0.033, 7, 4, center=(0.0, -0.142, 0.150),
             color=BIRD_WHITE)
    C.recolor_faces_where(head, lambda n, c: c[2] > 0.166 and n[2] > 0.25,
                          BIRD_GREY)
    for sx in (-1, 1):
        C.sphere(head.mesh, 0.0095, 5, 2, center=(sx * 0.024, -0.158, 0.160),
                 color=EYE_DK)

    beak = a.part("animal_bird_beak", base_color=BEAK_ORANGE, roughness=0.5)
    C.cone(beak.mesh, 0.016, 0.058, 5, center=(0.0, -0.188, 0.146),
           radius_top=0.004, color=BEAK_ORANGE)

    # OPEN SHEET 1 of 3.  A wing is a half-thickness membrane: a vertical
    # extrusion whose Z never falls, so every face still faces the sky.
    wings = a.part("animal_bird_wings", base_color=BIRD_GREY, roughness=0.7,
                   outward=("dir", (0.0, 0.0, 1.0)))
    for sx in (-1, 1):
        le, te = [], []
        for (x, y_le, y_te, z) in BIRD_WING:
            le.append((sx * x, y_le, z))
            te.append((sx * x, y_te, z))
        for i in range(len(BIRD_WING) - 1):
            _face(wings.mesh, [le[i], le[i + 1], te[i + 1], te[i]],
                  BIRD_GREY, (0.0, 0.0, 1.0))
            _face(wings.mesh, [le[i], te[i], te[i + 1], le[i + 1]],
                  BIRD_GREY_DK, (0.0, 0.0, 1.0))      # the underside tone
        tip = (sx * 0.414, 0.054, 0.169)
        _face(wings.mesh, [le[-1], tip, te[-1]], BIRD_GREY_DK, (0.0, 0.0, 1.0))
    C.recolor_faces_where(wings, lambda n, c: abs(c[0]) > 0.28,
                          C.shade(BIRD_GREY_DK, 0.86))  # darker outer feathers

    # OPEN SHEET 2.  A fanned tail, flat in the XY plane.
    tail = a.part("animal_bird_tail", base_color=BIRD_WHITE, roughness=0.68,
                  outward=("dir", (0.0, 0.0, 1.0)))
    root = (0.0, 0.098, 0.126)
    for k in range(5):
        x0 = -0.050 + 0.100 * k / 5.0
        x1 = -0.050 + 0.100 * (k + 1) / 5.0
        _face(tail.mesh, [root, (x0, 0.204, 0.128), (x1, 0.204, 0.128)],
              BIRD_WHITE, (0.0, 0.0, 1.0))
    _facet_noise(tail, amp=0.06, seed=23.0)
    return a


# ===========================================================================
# 6. animal_cat -- 0.45 m long, 0.25 m tall, standing on Z = 0
# ===========================================================================

# (y, half_width, half_height, centre_z) -- nose at y = -0.24, rump at
# y = +0.21: 0.45 m nose-to-rump.  With the tail curling past y = +0.30 the
# total silhouette reads ~0.55 m, and the ear tips put the top at 0.25 m.
CAT_BODY = (
    (-0.160, 0.050, 0.053, 0.128),
    (-0.082, 0.065, 0.071, 0.124),
    (0.000, 0.069, 0.077, 0.121),
    (0.090, 0.064, 0.072, 0.123),
    (0.170, 0.048, 0.055, 0.131),
    (0.205, 0.033, 0.038, 0.136),
)


def _animal_cat():
    a = C.Asset("animal_cat", "animal")

    body = a.part("animal_cat_body", base_color=CAT_GREY, roughness=0.78)
    C.loft(body.mesh,
           [_ring_y(y, rx, rz, 8, cz=cz) for (y, rx, rz, cz) in CAT_BODY],
           CAT_GREY, cap_start_flip=True, cap_end_flip=False)
    C.loft(body.mesh, [_ring_y(-0.198, 0.038, 0.040, 8, cz=0.158),
                       _ring_y(-0.160, 0.049, 0.052, 8, cz=0.140)],
           CAT_GREY, cap_start_flip=True, cap_end_flip=False)
    C.recolor_faces_where(body, lambda n, c: n[2] < -0.38, CAT_CREAM)
    C.recolor_faces_where(body, lambda n, c: n[2] > 0.58, CAT_DK)  # dorsal
    _facet_noise(body, amp=0.07, seed=31.0)

    legs = a.part("animal_cat_legs", base_color=CAT_GREY, roughness=0.78)
    for (x, y, top) in ((-0.050, -0.108, 0.122), (0.050, -0.108, 0.122),
                        (-0.053, 0.118, 0.128), (0.053, 0.118, 0.128)):
        C.cylinder(legs.mesh, 0.017, top - 0.018, 5,
                   center=(x, y, 0.018 + (top - 0.018) * 0.5),
                   radius_top=0.023, color=CAT_GREY)
        C.cylinder(legs.mesh, 0.024, 0.018, 5, center=(x, y - 0.008, 0.009),
                   color=CAT_CREAM)
    _facet_noise(legs, amp=0.07, seed=32.0)

    head = a.part("animal_cat_head", base_color=CAT_GREY, roughness=0.74)
    C.chamfer_box(head.mesh, (0.084, 0.088, 0.072),
                  center=(0.0, -0.204, 0.166), color=CAT_GREY,
                  bevel=0.016, n_corner=1)
    C.recolor_faces_where(head, lambda n, c: n[2] < -0.48, CAT_CREAM)
    for sx in (-1, 1):                                   # eyes
        C.box(head.mesh, (0.010, 0.021, 0.017),
              center=(sx * 0.042, -0.224, 0.176), color=EYE_DK)

    muzzle = a.part("animal_cat_muzzle", base_color=CAT_CREAM, roughness=0.7)
    C.chamfer_box(muzzle.mesh, (0.042, 0.050, 0.034),
                  center=(0.0, -0.256, 0.150), color=CAT_CREAM,
                  bevel=0.010, n_corner=1)
    C.box(muzzle.mesh, (0.020, 0.014, 0.014), center=(0.0, -0.282, 0.156),
          color=NOSE_DK)                                  # nose leather

    ears = a.part("animal_cat_ears", base_color=CAT_GREY, roughness=0.76)
    for sx in (-1, 1):
        C.cone(ears.mesh, 0.029, 0.050, 4,
               center=(sx * 0.031, -0.192, 0.216), color=CAT_GREY)
    C.recolor_faces_where(ears, lambda n, c: n[2] > 0.45,
                          C.shade(CAT_GREY, 1.16))
    C.recolor_faces_where(ears, lambda n, c: n[2] < -0.45,
                          C.shade(NOSE_DK, 2.2))            # inner ear

    # The long curled tail: a 5-point swept tube.  ``tube`` builds each ring
    # from a parallel-transport frame, so a vertical first pair of points is
    # not needed here -- the tail hangs off a hip 0.14 m up, nowhere near the
    # ground -- but the radii taper so it ends in a tip, not a pipe.  The curl
    # is kept in the YZ plane: the tail reaches 0.24 m, and the arc it
    # describes is the cat's silhouette, not a separate coil off the hip.
    tail = a.part("animal_cat_tail", base_color=CAT_GREY, roughness=0.78)
    C.tube(tail.mesh,
           [(0.0, 0.190, 0.142), (0.0, 0.252, 0.172),
            (0.0, 0.288, 0.230), (0.0, 0.276, 0.284),
            (0.0, 0.228, 0.312)],
           0.024, 5, color=CAT_GREY,
           radii=[0.024, 0.021, 0.018, 0.015, 0.011])
    C.recolor_faces_where(tail, lambda n, c: n[2] > 0.60, CAT_DK)
    return a


# ===========================================================================
# 7. animal_fish -- 0.5 m, centred on the origin, swimming toward -Y
# ===========================================================================

# (y, half_width, half_height) -- a deep-bodied reef fish: the maximum girth
# sits a third of the way back from the nose, not at the middle.
FISH_BODY = (
    (-0.242, 0.011, 0.017),
    (-0.196, 0.033, 0.053),
    (-0.140, 0.050, 0.072),
    (-0.080, 0.060, 0.087),
    (-0.010, 0.064, 0.092),
    (0.058, 0.056, 0.080),
    (0.120, 0.038, 0.053),
    (0.168, 0.024, 0.032),
    (0.205, 0.013, 0.017),
)


def _animal_fish():
    a = C.Asset("animal_fish", "animal")

    body = a.part("animal_fish_body", base_color=FISH_TEAL, roughness=0.42,
                  metallic=0.15)
    C.loft(body.mesh, [_ring_y(y, rx, rz, 8) for (y, rx, rz) in FISH_BODY],
           FISH_TEAL, cap_start_flip=True, cap_end_flip=False)
    # Three colour zones in three passes: a silver belly, a dark teal back,
    # and a narrow orange flank stripe that survives both because it runs
    # last and its predicate excludes faces the other two already claimed.
    C.recolor_faces_where(body, lambda n, c: n[2] < -0.34, FISH_SILVER)
    C.recolor_faces_where(body, lambda n, c: n[2] > 0.30, FISH_TEAL_DK)
    C.recolor_faces_where(body,
                          lambda n, c: abs(n[2]) <= 0.34
                          and -0.105 < c[1] < 0.150 and c[2] < 0.100,
                          FISH_ORANGE)
    _facet_noise(body, amp=0.07, seed=41.0)

    # OPEN SHEET 3 of 5.  A vertical membrane in the YZ plane: one axis, one
    # declaration, and a sheet with genuinely no interior to measure.
    dorsal = a.part("animal_fish_dorsal", base_color=FISH_TEAL_DK,
                    roughness=0.5, outward=("dir", (1.0, 0.0, 0.0)))
    b0 = (0.0, -0.078, 0.068)
    bm = (0.0, 0.008, 0.058)
    b1 = (0.0, 0.098, 0.044)
    apex = (0.0, -0.014, 0.172)
    _face(dorsal.mesh, [bm, b1, apex], FISH_TEAL_DK, (1.0, 0.0, 0.0))
    _face(dorsal.mesh, [bm, apex, b0], FISH_TEAL_DK, (1.0, 0.0, 0.0))

    # OPEN SHEET 4.  The anal fin is the same shape mirrored below the body
    # and therefore faces -X, so it needs its own part and its own axis --
    # sharing the dorsal part would force one axis on two opposite faces.
    ventral = a.part("animal_fish_ventral", base_color=FISH_ORANGE,
                     roughness=0.5, outward=("dir", (-1.0, 0.0, 0.0)))
    v0 = (0.0, -0.010, -0.084)
    vm = (0.0, 0.050, -0.074)
    v1 = (0.0, 0.118, -0.052)
    vtip = (0.0, 0.042, -0.150)
    _face(ventral.mesh, [vm, v1, vtip], FISH_ORANGE, (-1.0, 0.0, 0.0))
    _face(ventral.mesh, [vm, vtip, v0], FISH_ORANGE, (-1.0, 0.0, 0.0))

    # OPEN SHEET 5.  The caudal fin is a vertical membrane: it spans
    # fore-and-aft (Y) and up-and-down (Z), so it lies in the YZ plane and its
    # one-sided direction is X -- the same axis the dorsal fin needs.  What
    # makes it its own part rather than more dorsal-fin geometry is the
    # colour, not the winding.
    caudal = a.part("animal_fish_caudal", base_color=FISH_TEAL,
                    roughness=0.5, outward=("dir", (1.0, 0.0, 0.0)))
    root_c = (0.0, 0.206, 0.0)
    notch = (0.0, 0.262, 0.0)
    _face(caudal.mesh, [root_c, (0.0, 0.300, 0.086), notch],
          FISH_TEAL, (1.0, 0.0, 0.0))
    _face(caudal.mesh, [root_c, notch, (0.0, 0.300, -0.086)],
          C.shade(FISH_TEAL, 0.84), (1.0, 0.0, 0.0))

    # Pectorals: near-horizontal, so they belong on the Z-declared sheet and
    # are the reason the ``("dir", (0, 0, 1))`` part above them exists.
    pecs = a.part("animal_fish_pectorals", base_color=FISH_SILVER,
                  roughness=0.48, outward=("dir", (0.0, 0.0, 1.0)))
    for sx in (-1, 1):
        _face(pecs.mesh, [(sx * 0.046, -0.088, 0.004),
                          (sx * 0.126, -0.026, -0.004),
                          (sx * 0.080, 0.030, -0.010)],
              FISH_SILVER, (0.0, 0.0, 1.0))

    eyes = a.part("animal_fish_eyes", base_color=EYE_DK, roughness=0.4)
    for sx in (-1, 1):
        C.sphere(eyes.mesh, 0.016, 6, 3, center=(sx * 0.038, -0.152, 0.028),
                 color=EYE_DK)
    C.recolor_faces_where(eyes, lambda n, c: n[2] > 0.30, FISH_SILVER)
    return a


# ---------------------------------------------------------------------------

def build_all():
    """Return the ordered list of nature assets."""
    return [
        _mountain(),
        _grass_patch(),
        _flower_cluster(),
        _animal_dog(),
        _animal_bird(),
        _animal_cat(),
        _animal_fish(),
    ]
