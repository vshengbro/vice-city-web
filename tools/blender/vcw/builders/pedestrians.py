"""Category C: low-poly pedestrians.

Every figure is built as a set of SEPARATE closed shells -- one ``Part`` per
joint segment -- so a renderer can pose the result by rotating each part
about its own joint.  Nothing is merged: ``head``, ``torso``,
``upper_arm_L`` ... ``lower_leg_R`` are distinct parts, and the exporter emits
them as separate submeshes with their own materials.

RIG / A-POSE
    Authoring is Blender-style: +X right, -Y forward (the way the figure
    faces), +Z up, metres.  The origin is the centre of the soles at z = 0.
    Arms hang down and angle slightly away from the torso, legs are straight
    and set a little apart, so every joint axis is unambiguous.

    THE 13 LOAD-BEARING PART NAMES.  ``src/game.rs`` holds a hardcoded
    ``PLAYER_PARTS`` list (torso, head, hair, four arm segments, four leg
    segments, two shoes) and matches them against ``ped_suit.json`` by exact
    string, then derives each joint's pivot from that part's OWN local bbox:
    an arm or leg pivots at ``[min.x, max.y, mid.z]``, a shoe at
    ``[mid.x, max.y, mid.z]``.  Two consequences drive the whole file:

    * a limb part's bbox TOP must sit at the joint it rotates about, so
      every tapered segment is built with its widest station FIRST (at the
      shoulder / elbow / hip / knee end) and the profile narrows toward the
      far end;
    * the mitten hand, the wrist cuff and the shoe collar are built INSIDE
      ``lower_arm_*`` / ``upper_arm_*`` / ``shoe_*`` rather than as their own
      parts, because the player rig only ever instantiates those 13 names --
      an extra part would simply never be drawn for the player.

WHY SO MUCH GEOMETRY FOR A PEDESTRIAN
    The runtime shader is ``base * (ambient + light_color * max(dot(n, l),
    0))``: no textures, no specular.  The only things that make two adjacent
    polygons read differently are a different NORMAL and a different ALBEDO.
    So the detail budget goes to chamfers (a normal break per arris) and
    colour zoning (free), not to uniformly dense surfaces.  A straight tube
    has no wrist; a flat slab has no knee; a bare sphere has no face.  Each
    of those is the difference between "person" and "blockout".

Two hand-authored shells per figure (torso, hair) plus the eight limbs
are verified by :func:`_seal`, which flips any closed shell whose signed
volume came out negative.  ``loft`` winding depends on the direction the
rings advance relative to the profile's right-hand normal, so the flip is
the cheap way to guarantee the exporter's outward-facing check passes
without every station table having to be reasoned about by hand.
"""

import math

from .. import core as C

# --------------------------------------------------------------------------
# palettes -- one per figure, all four instantly separable at a glance
# --------------------------------------------------------------------------

SKIN_SUIT = (0.87, 0.69, 0.56)
SKIN_DRESS = (0.94, 0.78, 0.65)
SKIN_WORK = (0.76, 0.58, 0.44)
SKIN_STREET = (0.62, 0.45, 0.34)

# A face is ~8 px tall at gameplay distance, so the features that earn their
# triangles are the eye BAND and the brow: two flat darks on a skin field.
# A nose wedge alone reads as a blank egg at that size.
EYE_DARK = (0.13, 0.11, 0.11)
BROW_DARK = (0.22, 0.16, 0.13)
MOUTH_DARK = (0.55, 0.33, 0.30)

SUIT_CLOTH = (0.15, 0.17, 0.26)      # charcoal-navy jacket
SUIT_LAPEL = (0.21, 0.23, 0.34)     # lighter, so the lapel reads as a fold
SUIT_SHIRT = (0.95, 0.95, 0.92)
SUIT_TIE = (0.70, 0.13, 0.17)
SUIT_TROUSER = (0.12, 0.13, 0.19)
SUIT_KNEE = (0.17, 0.18, 0.25)      # trouser crease at the knee
SUIT_SHOE = (0.08, 0.07, 0.06)
SUIT_SOLE = (0.05, 0.05, 0.05)
SUIT_HAIR = (0.20, 0.14, 0.10)
SUIT_BELT = (0.07, 0.07, 0.09)
SUIT_BUCKLE = (0.62, 0.58, 0.34)

DRESS_CLOTH = (0.94, 0.23, 0.45)     # hot pink
DRESS_TRIM = (0.14, 0.63, 0.66)      # teal
DRESS_SHOE = (0.20, 0.63, 0.62)
DRESS_SOLE = (0.13, 0.45, 0.45)
DRESS_HAIR = (0.93, 0.79, 0.36)      # blonde
DRESS_BAND = (0.14, 0.20, 0.24)      # bodice seam

WORK_DENIM = (0.28, 0.43, 0.66)
WORK_SHIRT = (0.86, 0.67, 0.36)      # dusty tan
WORK_CUFF = (0.72, 0.54, 0.27)       # rolled-up sleeve
WORK_STRAP = (0.24, 0.37, 0.58)
WORK_BOOT = (0.34, 0.23, 0.14)
WORK_SOLE = (0.16, 0.12, 0.09)
WORK_CAP = (0.96, 0.60, 0.13)
WORK_HAIR = (0.18, 0.13, 0.10)

STREET_TOP = (0.32, 0.63, 0.52)      # oversized sea-green hoodie
STREET_RIB = (0.21, 0.44, 0.38)      # ribbed cuffs and hem
STREET_TROUSER = (0.20, 0.21, 0.25)  # baggy charcoal cargos
STREET_SHOE = (0.94, 0.94, 0.92)
STREET_SOLE = (0.20, 0.20, 0.22)
STREET_CAP = (0.85, 0.28, 0.28)
STREET_HAIR = (0.12, 0.10, 0.10)


# --------------------------------------------------------------------------
# shell helpers -- every one of them returns a CLOSED, outward-wound shell
# --------------------------------------------------------------------------

def _shells(m):
    """Split a mesh into index-connected groups of faces.

    A Part may legitimately hold several disjoint closed shells -- a torso
    plus its neck cylinder, a head plus its nose wedge.  ``core.is_closed``
    reports False for those (the shells share no vertices), so a whole-mesh
    signed volume would just be a SUM over shells and a positive total can
    hide one inverted shell.  Splitting on shared vertex indices lets each
    shell be judged on its own.

    Note the shells are returned whether or not they are watertight, so
    callers must gate on :func:`C.is_closed` before using volume as a
    winding test -- see :func:`_seal`.
    """
    parent = {}

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    def union(i, j):
        ri, rj = find(i), find(j)
        if ri != rj:
            parent[ri] = rj

    for i in range(len(m.pos)):
        parent[i] = i
    for (a, b, c) in m.faces:
        union(a, b)
        union(a, c)
    groups = {}
    for fi, (a, b, c) in enumerate(m.faces):
        groups.setdefault(find(a), []).append(fi)
    return list(groups.values())


def _recolor_where(m, predicate, color):
    """Face-colour zoning on a MESH, not a Part.

    ``core.recolor_faces_where`` takes the Part because that is what the
    builder holds at the call site for a whole garment; here the zones are
    drawn straight into a mesh that is still being assembled (a knee crease
    inside the shin, a toe cap inside the shoe), so this is the same
    predicate walk over ``m.faces`` with nothing wrapped around it.
    """
    fn = m.face_normals()
    for i, (a, b, c) in enumerate(m.faces):
        va, vb, vc = m.pos[a], m.pos[b], m.pos[c]
        cen = C.mul(C.add(C.add(va, vb), vc), 1.0 / 3.0)
        if predicate(fn[i], cen):
            m.fcol[i] = tuple(color)
    return m


def _seal(part):
    """Guarantee every closed shell in ``part`` is wound outward.

    ``core.loft`` emits side walls correctly only when the rings advance
    along the profile's right-hand normal; ``core.cylinder`` and
    ``core.tube`` compensate by flipping the start cap.  Rather than hand-
    check every station table, each connected shell whose signed volume came
    out negative is reversed in place.  Faces and face colours are reversed
    together so colours stay bound to their triangle.

    Shells that share a boundary ring (a sleeve cuff overlapping the arm it
    sits on) are deliberately built with a 0.5 mm standoff instead, so each
    stays an independent closed component and keeps being verified.
    """
    m = part.mesh
    if not m.faces:
        return part
    for group in _shells(m):
        sub = C.Mesh()
        # remap only this shell's vertices into a standalone mesh
        remap = {}
        for fi in group:
            tri = []
            for v in m.faces[fi]:
                if v not in remap:
                    remap[v] = len(sub.pos)
                    sub.pos.append(m.pos[v])
                tri.append(remap[v])
            sub.faces.append(tuple(tri))
            sub.fcol.append(m.fcol[fi])
        if not C.is_closed(sub):
            # Not watertight -- an open surface or a stack of flat triangles
            # with no interior.  Signed volume is meaningless here (a lone
            # box face integrates to 0.0), so flipping on its sign would
            # silently invert perfectly good faces.  Leave it alone.
            continue
        if C.signed_volume(sub) >= 0.0:
            continue
        # Reverse each face in place.  The face keeps its index, so its
        # face_colors entry stays bound to it and needs no reordering.
        for fi in group:
            a, b, c = m.faces[fi]
            m.faces[fi] = (a, c, b)
    return part


def _rbox(m, size, center=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
          color=(1.0, 1.0, 1.0)):
    """Box rotated by (rx, ry, rz) DEGREES about its own centre.

    ``C.box`` cannot express a lapel that rakes back, a brow that tilts with
    the face, or a boot toe that lifts.  The rotation matrix is orthogonal
    with determinant +1, so the winding and the signed volume are unchanged.
    """
    hx, hy, hz = size[0] * 0.5, size[1] * 0.5, size[2] * 0.5
    corners = [(-hx, -hy, -hz), (hx, -hy, -hz), (hx, hy, -hz), (-hx, hy, -hz),
               (-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz)]
    ax, ay, az = (math.radians(v) for v in rot)
    cx, sx = math.cos(ax), math.sin(ax)
    cy, sy = math.cos(ay), math.sin(ay)
    cz, sz = math.cos(az), math.sin(az)
    r = ((cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx),
         (sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx),
         (-sy, cy * sx, cy * cx))
    o = tuple(center)
    p = []
    for v in corners:
        p.append((o[0] + r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
                  o[1] + r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
                  o[2] + r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2]))
    m.quad(p[0], p[3], p[2], p[1], color)
    m.quad(p[4], p[5], p[6], p[7], color)
    m.quad(p[0], p[1], p[5], p[4], color)
    m.quad(p[1], p[2], p[6], p[5], color)
    m.quad(p[2], p[3], p[7], p[6], color)
    m.quad(p[3], p[0], p[4], p[7], color)
    return m


def _rot(deg, v):
    """Rotate ``v`` by (rx, ry, rz) in DEGREES, right-handed about X, Y, Z."""
    ax, ay, az = (math.radians(d) for d in deg)
    cx, sx = math.cos(ax), math.sin(ax)
    cy, sy = math.cos(ay), math.sin(ay)
    cz, sz = math.cos(az), math.sin(az)
    r = ((cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx),
         (sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx),
         (-sy, cy * sx, cy * cx))
    return (r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
            r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
            r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2])


def _cbox(m, size, center=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
          color=(1.0, 1.0, 1.0), bevel=0.010, n_corner=1):
    """``core.chamfer_box`` with a rotation about its own centre.

    The kernel primitive is axis-aligned, which cannot express a raked
    lapel, a tilted brow or a lifted boot toe.  Rotating the two rings AFTER
    the rounded-rect profile is built (rather than re-deriving the profile)
    keeps the chamfer uniform and the winding intact: the rotation matrix is
    orthogonal with determinant +1, so signed volume is unchanged.
    """
    sx, sy, sz = size[0] * 0.5, size[1] * 0.5, size[2] * 0.5
    b = max(0.0, min(bevel, sx * 0.9, sy * 0.9, sz * 0.9))
    if b <= 1e-6:
        return _rbox(m, size, center=center, rot=rot, color=color)
    ring = C.rounded_rect(sx, sy, b, n_corner)
    c = tuple(center)
    lo = [C.add(c, _rot(rot, (px, py, -sz))) for (px, py) in ring]
    hi = [C.add(c, _rot(rot, (px, py, sz))) for (px, py) in ring]
    for j in range(len(ring)):
        j2 = (j + 1) % len(ring)
        m.quad(lo[j], lo[j2], hi[j2], hi[j], color)
    for j in range(1, len(ring) - 1):
        m.tri(hi[0], hi[j], hi[j + 1], color)
    for j in range(1, len(ring) - 1):
        m.tri(lo[0], lo[j + 1], lo[j], color)
    return m


def _ring(m, stations, color, seg=10):
    """Loft stacked circles along +Z.

    ``stations`` is ``(z, radius)`` or ``(z, radius, y_offset)``; the offset is
    what turns a symmetric dome into a side-parted crown or a swept quiff.
    """
    rings = []
    for st in stations:
        z, r = st[0], st[1]
        yo = st[2] if len(st) > 2 else 0.0
        rings.append([(r * math.cos(2.0 * math.pi * k / seg),
                       yo + r * math.sin(2.0 * math.pi * k / seg), z)
                      for k in range(seg)])
    C.loft(m, rings, color, cap_start=True, cap_end=True, smooth=False)


def _box_ring(m, stations, color, n=2):
    """Loft stacked rounded rectangles along +Z.

    ``stations`` is ``(z, half_x, half_y, corner)``.  This is the torso /
    pelvis workhorse: a rounded-rect section reads as shoulders and hips at
    low poly counts where a cylinder reads as a barrel.  Passing the
    stations in DESCENDING z lofts the shell downward, and :func:`_seal`
    fixes the winding.
    """
    rings = []
    for (z, hx, hy, r) in stations:
        rings.append([(px, py, z) for (px, py) in C.rounded_rect(hx, hy, r, n)])
    C.loft(m, rings, color, cap_start=True, cap_end=True, smooth=False)


def _bar(m, p0, p1, hx, hy, color, n=1):
    """A closed rectangular-section bar between two world points."""
    t = C.normalize(C.sub(p1, p0))
    ref = (0.0, 0.0, 1.0) if abs(C.dot(t, (0.0, 0.0, 1.0))) < 0.9 \
        else (1.0, 0.0, 0.0)
    side = C.normalize(C.cross(ref, t))
    up = C.cross(t, side)
    rings = []
    for p in (tuple(p0), tuple(p1)):
        rings.append([C.add(p, C.add(C.mul(side, px), C.mul(up, py)))
                      for (px, py) in C.rounded_rect(hx, hy, min(hx, hy) * 0.35, n)])
    C.loft(m, rings, color, cap_start=True, cap_end=True, smooth=False)


def _frame(t):
    """Orthonormal (side, up) pair for a unit direction, with side x up = t."""
    t = C.normalize(t)
    ref = (0.0, 0.0, 1.0) if abs(C.dot(t, (0.0, 0.0, 1.0))) < 0.9 \
        else (1.0, 0.0, 0.0)
    side = C.normalize(C.cross(ref, t))
    up = C.cross(t, side)
    return t, side, up


def _limb(m, p0, p1, stations, color, seg=8):
    """A limb segment with an ARBITRARY radius profile along p0 -> p1.

    ``stations`` is ``(t, radius)`` with ``t`` running 0..1 from ``p0`` (the
    joint end, always the widest) to ``p1``.  This replaces the old
    two-station cone, which is the whole reason a forearm had no wrist: a
    straight taper between two rings has no place to change slope, so the
    silhouette reads as a dowel.  Four stations give shoulder -> bicep -> a
    narrow elbow, and elbow -> forearm swell -> wrist, which is a limb.

    ``p0`` must be the joint the engine pivots about, so the part's bbox top
    lands on the joint and nothing on the far end is wider than the joint.
    """
    a, b = tuple(p0), tuple(p1)
    t, side, up = _frame(C.sub(b, a))
    rings = []
    for (f, r) in stations:
        c = C.add(a, C.mul(C.sub(b, a), f))
        rings.append([C.add(c, C.add(C.mul(side, r * math.cos(w)),
                                     C.mul(up, r * math.sin(w))))
                      for w in [2.0 * math.pi * k / seg for k in range(seg)]])
    C.loft(m, rings, color, cap_start=True, cap_end=True, smooth=False)
    return t


def _limb_rect(m, origin, along, stations, color, n=1):
    """A rounded-rect-section slab swept along ``along``.

    ``stations`` is ``(distance, half_a, half_b, corner)``, measured in
    METRES from ``origin`` in the direction ``along``.  Used for the mitten
    hand and its thumb: a rounded-rect profile is what makes a fist look
    flat-sided and knuckled rather than cylindrical.
    """
    t, side, up = _frame(along)
    rings = []
    for (d, ha, hb, r) in stations:
        c = C.add(tuple(origin), C.mul(t, d))
        rings.append([C.add(c, C.add(C.mul(side, px), C.mul(up, py)))
                      for (px, py) in C.rounded_rect(ha, hb, r, n)])
    C.loft(m, rings, color, cap_start=True, cap_end=True, smooth=False)


# --------------------------------------------------------------------------
# hand, head, hair, shoe
# --------------------------------------------------------------------------

def _hand(m, wrist, along, r_wrist, color, skin_cuff=None, long_sleeve=True):
    """A mitten hand at the end of a forearm, built INSIDE the lower_arm part.

    Four tapered rounded-rect stations give the knuckle swell and the slight
    inward roll at the fingertips; a separate thumb slab on the -Y face is
    what stops the mitten reading as a paddle.  All of it lives in the
    ``lower_arm_L`` / ``lower_arm_R`` mesh: the player rig instantiates only
    the 13 hardcoded part names, so a separate ``hand_L`` part would never be
    drawn for the player character.

    ``skin_cuff`` is a slightly flared ring at the sleeve end, one half-mill
    -metre proud of the forearm so the two shells stay independent.
    """
    wrist = tuple(wrist)
    t, side, up = _frame(along)
    if long_sleeve and skin_cuff is not None:
        _limb(m, wrist, C.add(wrist, C.mul(t, -0.030)),
              [(0.0, r_wrist * 0.98), (0.55, r_wrist * 1.22),
               (1.0, r_wrist * 1.18)], skin_cuff, 8)

    # Mitten: the wrist is hidden inside the sleeve, so the first station is
    # the narrowest visible part and the mass sits a third of the way down.
    _limb_rect(m, wrist, t, [
        (0.004, r_wrist * 0.96, r_wrist * 0.80, r_wrist * 0.30),
        (0.030, r_wrist * 1.12, r_wrist * 0.94, r_wrist * 0.36),
        (0.052, r_wrist * 1.10, r_wrist * 0.86, r_wrist * 0.34),
        (0.068, r_wrist * 0.80, r_wrist * 0.62, r_wrist * 0.26),
    ], color, 1)
    # Thumb: a short slab on the -Y side, angled away from the palm.
    _cbox(m, (r_wrist * 1.00, r_wrist * 1.15, r_wrist * 0.60),
                  center=C.add(C.add(wrist, C.mul(t, 0.030)),
                               C.mul(up, -r_wrist * 0.80)),
                  rot=(0.0, 0.0, 0.0), color=color,
                  bevel=r_wrist * 0.22)


def _hair(asset, spec):
    """The ``hair`` part: a crown loft plus whatever the style adds.

    Every figure gets a crown that hugs the skull (so it never pokes through
    the face) and then a style marker that reads in silhouette: a fringe
    across the brow, side falls beside the jaw, a bun above the nape, or a
    crown-left-visible under a cap.  The part name is load-bearing for the
    player rig, so it stays ``hair`` for all four figures.
    """
    r = spec["head_r"]
    z = spec["head_z"]
    col = spec["hair_color"]
    style = spec.get("hair_style", "crop")
    p = asset.part("hair", base_color=col)

    top = z + r * spec["head_squash"] * 0.985
    lean = 0.004 if style in ("swoop", "bob") else 0.0
    _ring(p.mesh, [
        (z + r * spec["head_squash"] * 0.28, r * 1.010),
        (top - r * 0.062, r * 0.845, -lean),
        (top - r * 0.024, r * 0.520, -2.0 * lean),
        (top, r * 0.210, -3.0 * lean),
    ], col, 10)

    if style in ("swoop", "bob"):
        # Fringe: a swept bar across the brow, the single strongest
        # "this is hair" cue on a low-poly head.
        _cbox(p.mesh, (r * 1.62, r * 0.62, r * 0.30),
                      center=(0.0, -r * 0.70, z + r * 0.60),
                      rot=(0.0, 0.0, 0.0), color=col, bevel=r * 0.10)
    if style == "swoop":
        # Side part: a quiff lifted off the temple and raked back.
        _cbox(p.mesh, (r * 0.92, r * 0.86, r * 0.34),
                      center=(r * 0.34, -r * 0.30, z + r * 1.02),
                      rot=(-16.0, 9.0, 0.0), color=col, bevel=r * 0.11)
    if style == "bob":
        # Jaw-length side falls plus a back panel: the bob silhouette.
        for sx in (-1, 1):
            _limb_rect(p.mesh, (sx * r * 0.90, r * 0.10, z + r * 0.42),
                       (sx * 0.10, 0.0, -1.0), [
                           (0.0, r * 0.40, r * 0.62, r * 0.22),
                           (r * 0.90, r * 0.42, r * 0.52, r * 0.20),
                           (r * 1.10, r * 0.30, r * 0.34, r * 0.14),
                       ], col, 1)
        _cbox(p.mesh, (r * 1.50, r * 0.40, r * 1.40),
                      center=(0.0, r * 0.80, z - r * 0.72),
                      color=col, bevel=r * 0.16)
        C.sphere(p.mesh, r * 0.44, 8, 5, center=(0.0, r * 0.86, z + r * 0.92),
                 color=col, squash=0.82, smooth=False)      # top-knot bun
    if style == "crop":
        # A close crop under a cap: a shallow crown plus a short fringe
        # fringe-line at the hairline, nothing more.
        _cbox(p.mesh, (r * 1.44, r * 0.40, r * 0.22),
                      center=(0.0, -r * 0.78, z + r * 0.34),
                      color=col, bevel=r * 0.08)
    if style == "tail":
        _cbox(p.mesh, (r * 1.30, r * 0.44, r * 0.26),
                      center=(0.0, -r * 0.76, z + r * 0.30),
                      color=col, bevel=r * 0.08)
        _limb_rect(p.mesh, (0.0, r * 0.92, z - r * 0.10), (0.0, 0.35, -1.0), [
            (0.0, r * 0.46, r * 0.30, r * 0.14),
            (r * 1.30, r * 0.30, r * 0.20, r * 0.09),
        ], col, 1)
    # HARD HEIGHT CAP.  ``ped_suit``'s bbox is 1.75 m by contract (the
    # acceptance probe in src/const.rs assumes exactly that), and the skull
    # crown already reaches it.  A style marker that pokes above -- a swept
    # quiff, a top-knot -- would silently grow the player.  So any overshoot
    # is pushed down to 2 mm BELOW the crown: far enough that the hair
    # visibly intersects the skull (never floating above it), and short of
    # the crown itself so the silhouette top stays the head's, at 1.7500.
    crown = z + r * spec["head_squash"]
    m = p.mesh
    top = max((q[2] for q in m.pos), default=crown)
    limit = crown - 0.002
    if top > limit:
        drop = top - limit
        m.pos = [(q[0], q[1], q[2] - drop) for q in m.pos]
    return _seal(p)


def _head(asset, spec):
    """The ``head`` part: skull + a face that survives 8 pixels.

    A nose wedge alone reads as a blank egg.  What actually sells a face at
    distance is a dark horizontal EYE BAND at eye height plus BROW boxes
    above it: two flat darks on a skin field, 36 triangles, and the head
    stops being a blank ovoid.  The nose is chamfered so it catches light on
    its own edges.
    """
    r = spec["head_r"]
    z = spec["head_z"]
    skin = spec["skin"]
    head = asset.part("head", base_color=skin)
    C.sphere(head.mesh, r, seg_u=spec.get("head_seg_u", 8),
             seg_v=spec.get("head_seg_v", 5), center=(0.0, 0.0, z),
             color=skin, squash=spec["head_squash"], smooth=False)
    _cbox(head.mesh, (r * 0.30, r * 0.42, r * 0.34),
                  center=(0.0, -r * 0.92, z - r * 0.14), color=skin,
                  bevel=r * 0.08)                                   # nose
    C.box(head.mesh, (r * 1.20, r * 0.22, r * 0.21),
          center=(0.0, -r * 0.86, z + r * 0.12), color=EYE_DARK)  # eye band
    for sx in (-1, 1):                                              # brows
        _cbox(head.mesh, (r * 0.36, r * 0.16, r * 0.11),
                      center=(sx * r * 0.30, -r * 0.80, z + r * 0.30),
                      rot=(0.0, sx * 7.0, 0.0), color=BROW_DARK,
                      bevel=r * 0.04)
    C.box(head.mesh, (r * 0.36, r * 0.12, r * 0.08),
          center=(0.0, -r * 0.84, z - r * 0.44), color=MOUTH_DARK)  # mouth
    for sx in (-1, 1):                                              # ears
        _cbox(head.mesh, (r * 0.12, r * 0.26, r * 0.30),
                      center=(sx * r * 0.98, r * 0.02, z - r * 0.10),
                      color=skin, bevel=r * 0.04)
    return _seal(head)


def _shoe(asset, name, x, y, w, d, h, collar, upper, sole, lace):
    """A real shoe planted on z = 0, toe toward -Y: sole, upper, eyestay.

    One box cannot read as a foot.  Three pieces can: a dark chamfered sole
    that meets the ground and catches a highlight along its rim, an upper
    lofted over four stations from a tall heel collar down to a low toe box,
    and a light eyestay band across the instep.  The toe and heel get their
    own albedo zones by face colour, which is free.

    ``y`` is the HEEL edge, and the shoe spans ``y - d`` forward of it.  The
    sole and the upper therefore share ONE extent -- the old code centred
    the sole on ``y - d/2`` and lofted the upper from ``y``, which put the
    sole a half-length ahead of the upper and made the foot 50% longer than
    its own spec.

    ``h`` is the total height and ``collar`` is the part of it that overlaps
    the shin.  The toe stations are pinned to the VISIBLE height ``h -
    collar`` so raising the collar to meet the ankle lifts only the heel
    counter, leaving the toe box planted on the ground -- otherwise the whole
    foot would grow upward off the street.
    """
    p = asset.part(name, base_color=upper)
    sole_h = (h - collar) * 0.28
    up = h - collar - sole_h
    front_y = -d

    _cbox(p.mesh, (w, d, sole_h), center=(x, y - d * 0.5, sole_h * 0.5),
          color=sole, bevel=sole_h * 0.34)

    def station(fy, fx, fz):
        """``fy`` is the fraction along the foot from heel (0) to toe (1)."""
        yy = y + front_y * fy
        hx = w * 0.5 * fx
        hz = up * 0.5 * fz
        return (yy, hx, hz, sole_h + hz, min(hx, hz) * 0.34)

    # Heel collar -> instep -> ball -> toe box.  The heel station alone is
    # lifted by the collar overlap.
    rings = []
    for (fy, fx, fz) in ((0.00, 0.92, 1.00), (0.26, 0.99, 0.66),
                         (0.68, 0.95, 0.42), (1.00, 0.70, 0.28)):
        yy, hx, hz, zc, rr = station(fy, fx, fz)
        lift = collar * (1.0 - fy) ** 1.6
        rings.append([(x + px, yy, zc + py + lift) for (px, py) in
                      C.rounded_rect(hx, hz, rr, 1)])
    C.loft(p.mesh, rings, upper, cap_start=True, cap_end=True, smooth=False)

    # Eyestay / lace band, 28 triangles, and the one genuinely light value
    # on the shoe -- a white band is what makes a dark boot read as a boot.
    _cbox(p.mesh, (w * 0.78, d * 0.24, up * 0.15),
          center=(x, y - d * 0.30, sole_h + up * 0.40),
          color=lace, bevel=up * 0.05)
    # Toe cap and heel counter as face colours only.
    _recolor_where(p.mesh, lambda n, c: c[1] < y - d * 0.80,
                   C.shade(upper, 1.20))
    _recolor_where(p.mesh, lambda n, c: c[1] > y - d * 0.06,
                   C.shade(upper, 0.78))
    return _seal(p)


# --------------------------------------------------------------------------
# the figure assembly
# --------------------------------------------------------------------------

def _figure(spec):
    """Assemble one pedestrian from its skeleton + palette table."""
    a = C.Asset(spec["id"], "pedestrian")
    j = spec["joints"]

    sh_z, sh_x = j["shoulder_z"], j["shoulder_x"]
    el_z, el_x = j["elbow_z"], j["elbow_x"]
    wr_z, wr_x = j["wrist_z"], j["wrist_x"]
    hip_z, hip_x = j["hip_z"], j["hip_x"]
    kn_z, kn_x = j["knee_z"], j["knee_x"]
    an_z, an_x = j["ankle_z"], j["ankle_x"]
    arm_y = j.get("arm_y", 0.0)
    leg_y = j.get("leg_y", 0.0)

    ra0, ra1, ra2 = spec["arm_r"]
    rl0, rl1, rl2 = spec["leg_r"]
    arm_c = spec["arm_color"]
    leg_c = spec["leg_color"]
    knee_c = spec.get("knee_color", C.shade(leg_c, 0.78))
    cuff_c = spec.get("cuff_color")
    hand_c = spec.get("hand_color", spec["skin"])
    long_sleeve = spec.get("long_sleeve", True)
    cuff_at = spec.get("cuff_at", 1.0)      # 0..1 along the forearm

    # ---- torso: pelvis, waist, ribcage, shoulders, neck -------------------
    torso = a.part("torso", base_color=spec["torso_color"])
    _box_ring(torso.mesh, spec["torso_stations"], spec["torso_color"], 2)
    C.cylinder(torso.mesh, j["neck_r"], j["neck_top"] - j["shoulder_z"] * 0.96,
               seg=8, center=(0.0, 0.0, (j["neck_top"]
                                          + j["shoulder_z"] * 0.96) * 0.5),
               color=spec["skin"])
    for extra in spec.get("torso_detail", ()):
        extra(torso.mesh, spec)
    for zone in spec.get("torso_zones", ()):
        torso = zone(torso)
    _seal(torso)

    # ---- head + hair -----------------------------------------------------
    _head(a, spec)
    _hair(a, spec)

    # ---- optional silhouette-defining extras ------------------------------
    for extra in spec.get("extras", ()):
        extra(a)

    # ---- arms: upper_arm -> lower_arm (+ cuff + mitten), both off the body
    for side, sx in (("L", 1), ("R", -1)):
        p = a.part("upper_arm_%s" % side, base_color=arm_c)
        _limb(p.mesh, (sx * sh_x, arm_y, sh_z), (sx * el_x, arm_y, el_z),
              [(0.00, ra0), (0.30, ra0 * 0.94), (0.72, ra1 * 1.10),
               (1.00, ra1)], arm_c, 8)
        _seal(p)

        elbow = (sx * el_x, arm_y, el_z)
        wrist = (sx * wr_x, arm_y, wr_z)
        p = a.part("lower_arm_%s" % side, base_color=arm_c)
        # Elbow -> forearm swell -> narrow wrist: the extra station at 0.34
        # is the whole wrist transition, and it is four triangles.
        _limb(p.mesh, elbow, wrist,
              [(0.00, ra1), (0.34, ra1 * 0.92), (0.74, ra2 * 1.18),
               (1.00, ra2 * 0.86)], arm_c, 8)
        # Bare skin keeps a slim bare wrist and no cuff at all; a sleeved
        # forearm either runs to the wrist with a shirt cuff, or stops at
        # ``cuff_at`` with a rolled sleeve and hangs the hand on below it.
        arm_dir = C.normalize(C.sub(wrist, elbow))
        if cuff_c is None or cuff_at >= 1.0:
            _hand(p.mesh, wrist, arm_dir, ra2 * 0.86, hand_c,
                  skin_cuff=cuff_c, long_sleeve=long_sleeve)
        else:
            cut = C.add(elbow, C.mul(C.sub(wrist, elbow), cuff_at))
            _limb(p.mesh, elbow, cut,
                  [(0.00, ra1), (0.34, ra1 * 0.92), (0.80, ra2 * 1.06),
                   (1.00, ra2 * 1.00)], arm_c, 8)
            _limb(p.mesh, cut, C.add(cut, C.mul(arm_dir, 0.026)),
                  [(0.0, ra2 * 1.04), (0.5, ra2 * 1.30), (1.0, ra2 * 1.24)],
                  cuff_c, 8)
            _hand(p.mesh, wrist, arm_dir, ra2 * 0.86, hand_c,
                  skin_cuff=None, long_sleeve=False)
        _seal(p)

    # ---- legs: upper_leg -> lower_leg, straight and a little apart -------
    for side, sx in (("L", 1), ("R", -1)):
        p = a.part("upper_leg_%s" % side, base_color=leg_c)
        _limb(p.mesh, (sx * hip_x, leg_y, hip_z), (sx * kn_x, leg_y, kn_z),
              [(0.00, rl0), (0.32, rl0 * 0.98), (0.78, rl1 * 1.02),
               (1.00, rl1 * 0.94)], leg_c, 8)
        _seal(p)

        knee = (sx * kn_x, leg_y, kn_z)
        ankle = (sx * an_x, leg_y, an_z)
        p = a.part("lower_leg_%s" % side, base_color=leg_c)
        # Knee (a touch wide, so the joint reads) -> calf -> ankle, the last
        # station less than half the calf so the shoe collar has something
        # to sit on.
        _limb(p.mesh, knee, ankle,
              [(0.00, rl1 * 1.06), (0.30, rl1 * 1.02), (0.70, rl2 * 1.34),
               (1.00, rl2 * 0.92)], leg_c, 8)
        # Trouser crease at the knee: a face-colour band, zero triangles.
        _recolor_where(
            p.mesh,
            lambda n, c, z=kn_z, r=rl1: abs(c[2] - z) < r * 0.95
            and abs(c[1] - leg_y) < r * 1.30,
            knee_c)
        _seal(p)

    # ---- feet ------------------------------------------------------------
    # The shoe collar is raised so its top always lands ABOVE the ankle joint
    # the shin ends at: the leg parts and the shoe parts are posed as separate
    # batches about that ankle, so a shoe collar that stops short of it leaves
    # a visible 6 mm slit of daylight at the join in every walk frame.
    # ``h`` in the spec stays the VISIBLE shoe height; the extra is collar.
    sw, sd, sh_, shoe_up, shoe_sole, shoe_lace = spec["shoe"]
    total_h = sh_ + max(0.0, an_z + 0.014 - sh_)
    for side, sx in (("L", 1), ("R", -1)):
        _shoe(a, "shoe_%s" % side, sx * an_x, -sd * 0.5 + leg_y * 0.0,
              sw, sd, total_h, sh_, shoe_up, shoe_sole, shoe_lace)

    return a


# --------------------------------------------------------------------------
# 1. business suit -- narrowest silhouette, jacket + tie, long sleeves
# --------------------------------------------------------------------------

def _suit_tie(m, spec):
    """Tie, collar wings, lapels, belt and a seam line.

    All of this is a face-colour story told with thin slabs: 12 triangles
    each, and a jacket that was one flat charcoal value becomes a garment
    with a front opening, a fold and a waist.
    """
    C.box(m, (0.052, 0.030, 0.250), center=(0.0, -0.102, 1.320),
          color=SUIT_TIE)                                    # tie
    C.box(m, (0.086, 0.026, 0.060), center=(0.0, -0.104, 1.452),
          color=SUIT_TIE)                                    # knot
    for sx in (-1, 1):                                       # shirt + collar
        C.box(m, (0.060, 0.026, 0.130), center=(sx * 0.056, -0.100, 1.400),
              color=SUIT_SHIRT)
        _rbox(m, (0.070, 0.030, 0.150), center=(sx * 0.070, -0.106, 1.318),
               rot=(0.0, sx * -13.0, 0.0), color=SUIT_LAPEL)   # lapel
    C.box(m, (0.290, 0.230, 0.034), center=(0.0, 0.004, 1.082),
          color=SUIT_BELT)                                    # belt
    C.box(m, (0.052, 0.026, 0.044), center=(0.0, -0.108, 1.082),
          color=SUIT_BUCKLE)                                  # buckle
    C.box(m, (0.320, 0.240, 0.022), center=(0.0, 0.004, 0.850),
          color=C.shade(SUIT_CLOTH, 0.78))                    # hem band
    C.box(m, (0.012, 0.014, 0.180), center=(0.0, -0.118, 1.180),
          color=C.shade(SUIT_CLOTH, 0.70))                    # front seam


def _ped_suit():
    # Shoulder breadth is 0.41 m across the torso station and 0.45 m across
    # the deltoid caps, which is the adult figure the brief asks for; the
    # original was 0.364 m, i.e. visibly adolescent.  shoulder_x clears the
    # station by 13 mm so the arm never sweeps through the ribcage.
    j = dict(shoulder_z=1.425, shoulder_x=0.218,
             elbow_z=1.100, elbow_x=0.252,
             wrist_z=0.838, wrist_x=0.268,
             hip_z=0.945, hip_x=0.108,
             knee_z=0.485, knee_x=0.111,
             ankle_z=0.098, ankle_x=0.114,
             neck_r=0.052, neck_top=1.560)
    spec = dict(
        id="ped_suit",
        skin=SKIN_SUIT, torso_color=SUIT_CLOTH, arm_color=SUIT_CLOTH,
        leg_color=SUIT_TROUSER, shoe_color=SUIT_SHOE,
        hair_color=SUIT_HAIR, hair_style="swoop",
        joints=j,
        # 1.6337 + 0.102 * 1.14 == 1.7500 exactly: the bbox the acceptance
        # probe (PED_SCREEN_PROBE_HEIGHT) assumes.
        head_z=1.6337, head_r=0.102, head_squash=1.14,
        head_seg_u=10, head_seg_v=6,
        torso_stations=[
            (0.845, 0.150, 0.108, 0.050),   # jacket hem
            (0.980, 0.162, 0.114, 0.052),
            (1.100, 0.156, 0.105, 0.046),   # waist, the suit's pinch
            (1.215, 0.176, 0.112, 0.052),   # ribs
            (1.325, 0.194, 0.114, 0.058),   # chest
            (1.412, 0.205, 0.106, 0.068),   # shoulders, 0.41 m
            (1.470, 0.064, 0.060, 0.050),   # neck
        ],
        arm_r=(0.062, 0.050, 0.041),
        leg_r=(0.092, 0.076, 0.058),
        knee_color=SUIT_KNEE,
        long_sleeve=True, cuff_color=SUIT_SHIRT, cuff_at=1.0,
        hand_color=SKIN_SUIT,
        shoe=(0.102, 0.252, 0.090, SUIT_SHOE, SUIT_SOLE, SUIT_SHIRT),
        torso_detail=(_suit_tie,),
    )
    return _figure(spec)


# --------------------------------------------------------------------------
# 2. dress -- narrow bodice over an A-line cone skirt, bare arms
# --------------------------------------------------------------------------

def _ped_dress():
    def skirt(a):
        """Truncated cone flaring out from the waist -- the dress read."""
        p = a.part("skirt", base_color=DRESS_CLOTH)
        top_z, bot_z = 1.055, 0.585
        # Three stations rather than a single cone: a straight-sided cone
        # reads as a traffic cone, a curved one as a skirt.
        rings = []
        for (z, r) in ((bot_z, 0.305), (bot_z + 0.150, 0.268),
                       (top_z - 0.060, 0.190), (top_z, 0.152)):
            rings.append([(r * math.cos(2.0 * math.pi * k / 14),
                           r * math.sin(2.0 * math.pi * k / 14), z)
                          for k in range(14)])
        C.loft(p.mesh, rings, DRESS_CLOTH, cap_start=True, cap_end=True,
               smooth=False)
        # Panel seams as face colours: four darker wedges up the flare.
        _recolor_where(
            p.mesh, lambda n, c: abs(math.cos(math.atan2(c[1], c[0])
                                              * 2.0)) > 0.93
            and c[2] < 0.95, C.shade(DRESS_CLOTH, 0.84))
        _seal(p)

        hem = a.part("skirt_hem", base_color=DRESS_TRIM)
        C.cylinder(hem.mesh, 0.313, 0.058, seg=14,
                   center=(0.0, 0.0, bot_z + 0.031), color=DRESS_TRIM)
        C.cylinder(hem.mesh, 0.316, 0.014, seg=14,
                   center=(0.0, 0.0, bot_z + 0.062),
                   color=C.shade(DRESS_TRIM, 0.80))
        _seal(hem)

    def bodice(m, spec):
        """Bust seam, waistband and shoulder straps -- the dress's own zones."""
        C.box(m, (0.250, 0.240, 0.030), center=(0.0, 0.0, 1.046),
              color=DRESS_TRIM)                              # waistband
        C.box(m, (0.300, 0.260, 0.020), center=(0.0, 0.0, 1.160),
              color=DRESS_BAND)                              # underbust seam
        for sx in (-1, 1):
            _rbox(m, (0.028, 0.180, 0.100), center=(sx * 0.086, -0.048, 1.352),
                   color=DRESS_TRIM)                          # shoulder straps
        C.box(m, (0.290, 0.240, 0.020), center=(0.0, 0.0, 0.948),
              color=C.shade(DRESS_CLOTH, 0.82))              # skirt/waist join

    j = dict(shoulder_z=1.400, shoulder_x=0.172,
             elbow_z=1.086, elbow_x=0.208,
             wrist_z=0.822, wrist_x=0.232,
             hip_z=0.930, hip_x=0.096,
             knee_z=0.472, knee_x=0.099,
             ankle_z=0.092, ankle_x=0.102,
             neck_r=0.046, neck_top=1.530)
    spec = dict(
        id="ped_dress",
        skin=SKIN_DRESS, torso_color=DRESS_CLOTH, arm_color=SKIN_DRESS,
        leg_color=SKIN_DRESS, shoe_color=DRESS_SHOE,
        hair_color=DRESS_HAIR, hair_style="bob",
        joints=j,
        head_z=1.6077, head_r=0.099, head_squash=1.13,
        torso_stations=[
            (0.940, 0.120, 0.090, 0.044),   # skirt/waist join
            (1.040, 0.126, 0.090, 0.042),   # natural waist
            (1.150, 0.140, 0.094, 0.046),
            (1.270, 0.158, 0.102, 0.054),   # bust
            (1.390, 0.168, 0.096, 0.060),   # shoulders, 0.34 m
            (1.452, 0.058, 0.054, 0.046),
        ],
        arm_r=(0.046, 0.038, 0.032),
        leg_r=(0.080, 0.064, 0.048),
        knee_color=C.shade(SKIN_DRESS, 0.90),
        long_sleeve=False, cuff_color=None, cuff_at=0.0,
        hand_color=SKIN_DRESS,
        shoe=(0.086, 0.214, 0.086, DRESS_SHOE, DRESS_SOLE, DRESS_TRIM),
        torso_detail=(bodice,),
        extras=(skirt,),
    )
    return _figure(spec)


# --------------------------------------------------------------------------
# 3. work overalls -- widest torso, denim bib + shoulder straps + cap
# --------------------------------------------------------------------------

def _ped_overalls():
    def bib(a):
        """Denim chest bib, four straps, a cap and a tool belt."""
        p = a.part("bib", base_color=WORK_DENIM)
        _cbox(p.mesh, (0.300, 0.040, 0.310),
                      center=(0.0, -0.120, 1.310), color=WORK_DENIM,
                      bevel=0.010)
        _cbox(p.mesh, (0.060, 0.040, 0.120),
                      center=(0.0, -0.120, 1.476), color=WORK_DENIM,
                      bevel=0.010)
        C.box(p.mesh, (0.230, 0.024, 0.026), center=(0.0, -0.126, 1.400),
              color=C.shade(WORK_DENIM, 0.74))               # bib pocket line
        for sx in (-1, 1):                                   # rivets
            C.box(p.mesh, (0.018, 0.024, 0.018),
                  center=(sx * 0.120, -0.126, 1.380),
                  color=(0.76, 0.70, 0.42))
        _seal(p)

        s = a.part("straps", base_color=WORK_STRAP)
        for sx in (-1, 1):
            _bar(s.mesh, (sx * 0.104, -0.122, 1.175), (sx * 0.116, -0.064, 1.452),
                 0.031, 0.018, WORK_STRAP)
            _bar(s.mesh, (sx * 0.104, 0.104, 1.175), (sx * 0.116, 0.066, 1.452),
                 0.031, 0.018, WORK_STRAP)
            C.box(s.mesh, (0.062, 0.040, 0.050),      # adjuster buckle
                  center=(sx * 0.108, -0.110, 1.262), color=(0.72, 0.66, 0.38))
        _cbox(s.mesh, (0.076, 0.044, 0.062),
                      center=(0.0, -0.112, 1.040), color=(0.72, 0.66, 0.38),
                      bevel=0.008)                                  # buckle
        C.box(s.mesh, (0.330, 0.250, 0.044), center=(0.0, 0.0, 1.006),
              color=C.shade(WORK_STRAP, 0.80))                     # waistband
        _seal(s)

        cap = a.part("cap", base_color=WORK_CAP)
        C.sphere(cap.mesh, 0.110, seg_u=12, seg_v=3, center=(0.0, 0.004, 1.716),
                 color=WORK_CAP, squash=0.52, smooth=False)
        _box_ring(cap.mesh, [(1.636, 0.114, 0.106, 0.056)], WORK_CAP, 2)
        _cbox(cap.mesh, (0.196, 0.156, 0.022),
                      center=(0.0, -0.142, 1.658), color=WORK_CAP,
                      bevel=0.008)                                  # peak
        C.box(cap.mesh, (0.196, 0.150, 0.014), center=(0.0, -0.140, 1.646),
              color=C.shade(WORK_CAP, 0.80))                        # band
        _seal(cap)

        tool = a.part("tool_belt", base_color=WORK_BOOT)
        _cbox(tool.mesh, (0.312, 0.226, 0.062),
                      center=(0.0, 0.020, 1.010), color=WORK_BOOT,
                      bevel=0.012)
        for sx in (-1, 1):                                           # pouches
            _cbox(tool.mesh, (0.070, 0.052, 0.110),
                          center=(sx * 0.108, -0.090, 0.960), color=WORK_BOOT,
                          bevel=0.010)
        _cbox(tool.mesh, (0.030, 0.026, 0.150),
                      center=(0.140, 0.048, 1.000),
                      rot=(0.0, 0.0, 0.0), color=WORK_CAP,
                      bevel=0.006)                                   # tool handle
        _seal(tool)

    def shirt(m, spec):
        """Chest pocket, yoke and hem -- zones the flat tan shirt."""
        C.box(m, (0.120, 0.026, 0.120), center=(-0.086, -0.126, 1.242),
              color=C.shade(WORK_SHIRT, 0.80))                  # chest pocket
        C.box(m, (0.130, 0.026, 0.026), center=(-0.086, -0.130, 1.288),
              color=C.shade(WORK_SHIRT, 0.64))                  # pocket flap
        C.box(m, (0.300, 0.240, 0.022), center=(0.0, 0.0, 1.396),
              color=C.shade(WORK_SHIRT, 0.86))                  # yoke
        C.box(m, (0.320, 0.250, 0.024), center=(0.0, 0.0, 0.866),
              color=C.shade(WORK_SHIRT, 0.72))                  # hem
        C.box(m, (0.250, 0.230, 0.026), center=(0.0, 0.0, 0.978),
              color=C.shade(WORK_DENIM, 0.90))                  # waistband

    # shoulder_x must sit at or OUTSIDE the torso's half-width at shoulder
    # height (the 1.408 station below is hx = 0.220): a pivot buried in the
    # ribcage makes the upper arm sweep through the chest when it rotates.
    j = dict(shoulder_z=1.400, shoulder_x=0.234,
             elbow_z=1.072, elbow_x=0.276,
             wrist_z=0.798, wrist_x=0.298,
             hip_z=0.935, hip_x=0.126,
             knee_z=0.478, knee_x=0.128,
             ankle_z=0.100, ankle_x=0.130,
             neck_r=0.060, neck_top=1.540)
    spec = dict(
        id="ped_overalls",
        skin=SKIN_WORK, torso_color=WORK_SHIRT, arm_color=WORK_SHIRT,
        leg_color=WORK_DENIM, shoe_color=WORK_BOOT,
        hair_color=WORK_HAIR, hair_style="crop",
        joints=j,
        head_z=1.6087, head_r=0.103, head_squash=1.13,
        torso_stations=[
            (0.860, 0.176, 0.126, 0.060),   # work shirt hem over the hips
            (0.990, 0.196, 0.132, 0.062),
            (1.110, 0.190, 0.126, 0.056),   # waist under the bib
            (1.240, 0.208, 0.134, 0.062),   # bulkier ribcage
            (1.340, 0.218, 0.136, 0.066),   # chest
            (1.408, 0.220, 0.126, 0.072),   # squared-off work shoulders
            (1.470, 0.070, 0.064, 0.052),
        ],
        arm_r=(0.074, 0.062, 0.052),
        leg_r=(0.110, 0.092, 0.072),
        knee_color=C.shade(WORK_DENIM, 0.76),
        long_sleeve=False, cuff_color=WORK_CUFF, cuff_at=0.62,
        hand_color=SKIN_WORK,
        shoe=(0.122, 0.280, 0.094, WORK_BOOT, WORK_SOLE, WORK_CAP),
        torso_detail=(shirt,),
        extras=(bib,),
    )
    return _figure(spec)


# --------------------------------------------------------------------------
# 4. streetwear -- drop shoulders, hoodie mass, baggy cargo legs
# --------------------------------------------------------------------------

def _ped_streetwear():
    def hoodie(a):
        """Hood bunched at the neck, kangaroo pocket, ribbed hem, back cap."""
        h = a.part("hood", base_color=STREET_TOP)
        _box_ring(h.mesh, [
            (1.360, 0.070, 0.062, 0.055),
            (1.430, 0.120, 0.100, 0.070),
            (1.520, 0.132, 0.112, 0.078),
            (1.580, 0.108, 0.092, 0.070),
            (1.612, 0.060, 0.052, 0.048),
        ], STREET_TOP, 2)
        # Hood opening: a darker recessed plate + two drawstrings, so the
        # mass at the neck has a front instead of being a lump.
        _cbox(h.mesh, (0.140, 0.040, 0.110),
                      center=(0.0, -0.104, 1.512), color=C.shade(STREET_TOP, 0.60),
                      bevel=0.012)
        for sx in (-1, 1):
            _cbox(h.mesh, (0.012, 0.012, 0.170),
                          center=(sx * 0.040, -0.118, 1.446),
                          color=(0.90, 0.90, 0.88), bevel=0.004)
            _cbox(h.mesh, (0.018, 0.018, 0.026),
                          center=(sx * 0.040, -0.118, 1.368),
                          color=(0.80, 0.80, 0.78), bevel=0.006)
        _seal(h)

        pk = a.part("pocket", base_color=C.shade(STREET_TOP, 0.86))
        _cbox(pk.mesh, (0.300, 0.044, 0.180),
                      center=(0.0, -0.138, 1.050),
                      color=C.shade(STREET_TOP, 0.86), bevel=0.010)
        for sx in (-1, 1):                                   # pocket openings
            _cbox(pk.mesh, (0.012, 0.020, 0.150),
                          center=(sx * 0.088, -0.156, 1.062),
                          color=C.shade(STREET_TOP, 0.58), bevel=0.004)
        _seal(pk)

        rib = a.part("rib", base_color=STREET_RIB)
        _cbox(rib.mesh, (0.300, 0.250, 0.046),
                      center=(0.0, 0.0, 0.782), color=STREET_RIB,
                      bevel=0.012)                            # waistband rib
        C.box(rib.mesh, (0.302, 0.252, 0.012), center=(0.0, 0.0, 0.804),
              color=C.shade(STREET_RIB, 1.25))                # top stitch
        _seal(rib)

        cap = a.part("cap", base_color=STREET_CAP)
        _ring(cap.mesh, [
            (1.648, 0.114, 0.004), (1.700, 0.118, 0.004),
            (1.736, 0.106, 0.004), (1.756, 0.072, 0.004),
            (1.764, 0.028, 0.004),
        ], STREET_CAP, 10)
        # backwards brim, so it sits behind the head (+Y) and still reads
        _cbox(cap.mesh, (0.180, 0.136, 0.020),
                      center=(0.0, 0.150, 1.666), color=C.shade(STREET_CAP, 0.85),
                      bevel=0.007)
        C.box(cap.mesh, (0.196, 0.030, 0.020), center=(0.0, 0.084, 1.660),
              color=C.shade(STREET_CAP, 0.66))                # adjuster strap
        _seal(cap)

    def top(m, spec):
        """Chest print, shoulder seams and a zip line -- the hoodie's zones."""
        _cbox(m, (0.180, 0.026, 0.180), center=(0.0, -0.150, 1.268),
                      color=(0.93, 0.93, 0.90), bevel=0.008)  # chest graphic
        _cbox(m, (0.120, 0.026, 0.040), center=(0.0, -0.154, 1.268),
                      color=STREET_CAP, bevel=0.006)
        for sx in (-1, 1):                                    # shoulder seams
            C.box(m, (0.014, 0.200, 0.014), center=(sx * 0.180, 0.0, 1.372),
                  color=C.shade(STREET_TOP, 0.70))
        C.box(m, (0.014, 0.014, 0.520), center=(0.0, -0.152, 1.220),
              color=C.shade(STREET_TOP, 0.62))                # zip line
        for z in (1.060, 1.150, 1.240, 1.330):               # zip teeth
            C.box(m, (0.020, 0.018, 0.010), center=(0.0, -0.156, z),
                  color=(0.80, 0.80, 0.78))

    # heavy drop shoulders; the pivot clears the 1.390 station (hx = 0.238)
    j = dict(shoulder_z=1.372, shoulder_x=0.262,
             elbow_z=1.038, elbow_x=0.308,
             wrist_z=0.760, wrist_x=0.340,
             hip_z=0.915, hip_x=0.134,
             knee_z=0.462, knee_x=0.138,
             ankle_z=0.102, ankle_x=0.142,
             neck_r=0.062, neck_top=1.520)
    spec = dict(
        id="ped_streetwear",
        skin=SKIN_STREET, torso_color=STREET_TOP, arm_color=STREET_TOP,
        leg_color=STREET_TROUSER, shoe_color=STREET_SHOE,
        hair_color=STREET_HAIR, hair_style="tail",
        joints=j,
        head_z=1.5857, head_r=0.102, head_squash=1.14,
        torso_stations=[
            (0.760, 0.196, 0.132, 0.066),   # boxy hoodie hem
            (0.920, 0.216, 0.140, 0.070),
            (1.080, 0.206, 0.132, 0.064),
            (1.230, 0.224, 0.142, 0.070),
            (1.330, 0.236, 0.146, 0.076),   # widest point of the silhouette
            (1.390, 0.238, 0.136, 0.084),
            (1.452, 0.074, 0.068, 0.054),
        ],
        arm_r=(0.084, 0.071, 0.058),       # sleeves swallow the arms
        leg_r=(0.126, 0.114, 0.090),       # baggy, barely-tapering cargo leg
        knee_color=C.shade(STREET_TROUSER, 1.28),
        long_sleeve=True, cuff_color=STREET_RIB, cuff_at=1.0,
        hand_color=SKIN_STREET,
        shoe=(0.134, 0.292, 0.098, STREET_SHOE, STREET_SOLE, STREET_CAP),
        torso_detail=(top,),
        extras=(hoodie,),
    )
    return _figure(spec)


def build_all():
    """Return the ordered list of pedestrian assets."""
    return [_ped_suit(), _ped_dress(), _ped_overalls(), _ped_streetwear()]
