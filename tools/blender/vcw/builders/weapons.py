"""Handheld weapons + carry pickups: four guns, a launcher, a bat, a frag,
six carry pickups.

Everything here is original low-poly blockwork built from the ``core`` kernel
primitives -- no imported or traced geometry, and no reference to any real
manufacturer's design.

AUTHORING CONVENTIONS (inherited from ``CONTRACT.md``, with one deliberate
deviation, see ORIGINS below):

* Z-up, ``-Y`` is the barrel direction, metres, centred on ``X = 0``.
* ORIGINS.  The guns put their origin at the **centre of the grip**, so a
  renderer can parent them to a hand bone and have the palm land in the right
  place; the barrel then runs toward ``-Y`` from that point.  The pickups are
  centred on their own bounds, which is where ``src/game.rs`` expects them:
  it spawns every pickup at a fixed 0.35 m above the street and spins it
  about its own centre.  The guns are hand props and are NOT authored
  resting on ``Z = 0`` -- a weapon's origin is a grip, not a footprint.

MATERIALS / PARTS.  Each gun carries at least THREE distinct material zones,
because with no textures the albedo IS the read: a dark polymer frame, a
lighter machined slide or barrel, and a near-black grip.  A weapon painted one
flat colour is a blockout no matter how many triangles it has.  ``*_body`` is
the painted receiver, ``*_metal`` the bare machined parts -- barrel, guard,
sights -- and ``*_grip`` the matte polymer a hand actually touches, with
``*_trim`` reserved for the one deliberately bright accent.

``core.torus`` and ``core.cylinder`` are called on the X and Y axes here.  Both
were verified with ``C.signed_volume(m) > 0`` on this kernel revision (the
``torus`` ``axis="Y"`` branch compensates its own handedness); the axis-Y
cylinder is the cleanest available primitive for a barrel lying along the
authoring forward axis, and the exporter's closed-shell volume check would
reject it loudly if that ever regressed.
"""

import math

from .. import core as C

# ---- palette (shared with props.py so the block reads as one world) --------
GUNMETAL = (0.21, 0.22, 0.24)       # painted receiver
GUNMETAL_LT = (0.32, 0.33, 0.36)
BARREL = (0.30, 0.31, 0.33)         # bare machined steel
BARREL_DK = (0.18, 0.19, 0.20)
POLYMER = (0.11, 0.11, 0.13)        # grip / furniture
POLYMER_LT = (0.17, 0.17, 0.20)
STEEL_BRIGHT = (0.55, 0.57, 0.60)   # trigger guard, springs, pins
BRASS_HI = (0.72, 0.60, 0.26)       # the single warm accent on a weapon

WOOD = (0.46, 0.28, 0.16)
WOOD_DK = (0.30, 0.18, 0.10)

OLIVE = (0.26, 0.28, 0.20)          # launcher tube
OLIVE_DK = (0.18, 0.20, 0.14)
OLIVE_LT = (0.34, 0.37, 0.27)
ARMO_1 = (0.13, 0.42, 0.44)         # vest ballistic panel
ARMO_2 = (0.09, 0.30, 0.32)
ARMO_TRIM = (0.92, 0.44, 0.18)      # hi-viz trim
ARMO_STRAP = (0.16, 0.17, 0.19)

MED_RED = (0.86, 0.13, 0.14)
MED_RED_DK = (0.62, 0.08, 0.10)
MED_WHITE = (0.94, 0.94, 0.92)
MED_GREY = (0.74, 0.75, 0.74)

# Melee + carry-pickup palette, same neon-boardwalk register as the guns.
BAT_WOOD = (0.72, 0.55, 0.34)
BAT_WOOD_DK = (0.55, 0.40, 0.23)
BAT_TAPE = (0.16, 0.17, 0.19)
BRASS = (0.76, 0.62, 0.28)
GREN_BODY = (0.24, 0.30, 0.22)
GREN_BODY_DK = (0.16, 0.21, 0.15)
NEON_TEAL = (0.16, 0.80, 0.74)
NEON_TEAL_DK = (0.08, 0.44, 0.42)
FLARE_RED = (0.92, 0.28, 0.10)
FLARE_RED_DK = (0.66, 0.16, 0.07)
AMBER_STENCIL = (0.88, 0.72, 0.18)
TANK_WHITE = (0.93, 0.93, 0.89)
BILL_GREEN = (0.30, 0.56, 0.34)
BILL_GREEN_DK = (0.22, 0.44, 0.27)
BILL_TAN = (0.80, 0.72, 0.52)
BILL_TAN_DK = (0.68, 0.60, 0.42)


# ---------------------------------------------------------------------------
# local helpers on top of the kernel primitives
# ---------------------------------------------------------------------------

def _rbox(m, size, center=(0.0, 0.0, 0.0), rot=(0.0, 0.0, 0.0),
          color=(1.0, 1.0, 1.0)):
    """Box rotated by (rx, ry, rz) DEGREES about its own centre.

    ``C.box`` is axis-aligned only, which cannot express a pistol grip raked
    back 18-20 deg, a stock comb sloping down to the butt, or a canted
    magazine.  Emitting the same eight corners through the same six quads as
    ``C.box`` keeps the winding identical -- the rotation matrix is
    orthogonal with determinant +1, so signed volume is unchanged.
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
          color=(1.0, 1.0, 1.0), bevel=0.004, n_corner=1):
    """``core.chamfer_box`` with a rotation about its own centre.

    28 triangles against 12 for a hard box, and the extra 16 are all chamfer
    strips at intermediate normals.  That is the whole quality argument: the
    runtime shades with one directional light and no specular, so a big flat
    face renders as one dead value and only a normal break can make it read
    as a solid.  Rotating the two rings after the rounded-rect profile is
    built keeps the chamfer uniform and the winding intact.
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


def _shift(part, offset):
    """Translate every vertex of ``part``.

    A LOCAL reimplementation of ``core.translate_part``, which is broken: it
    mutates vertices with ``p[0] += ox`` while every entry in ``Mesh.pos`` is
    an immutable tuple, so it raises TypeError on any non-empty mesh.  No
    other builder calls it, so the bug has never been hit.  Rebuilt as a pure
    list comprehension rather than an in-place edit.

    ``C.translate_part`` is listed in CONTRACT.md, so this is worth reporting
    upstream: the fix there is one line -- ``part.mesh.pos = [(p[0] + ox,
    p[1] + oy, p[2] + oz) for p in part.mesh.pos]``.
    """
    ox, oy, oz = offset
    part.mesh.pos = [(p[0] + ox, p[1] + oy, p[2] + oz) for p in part.mesh.pos]
    return part


def _center(asset):
    """Translate every part so the asset's bounds centre lands on the origin.

    Used by the pickups, whose contract is "origin at its centre" -- which is
    what ``src/game.rs`` assumes, spawning each at a fixed height and spinning
    it about its own middle.  Doing it on the finished geometry rather than by
    hand-tuned constants means a late tweak to a trim strip cannot silently
    slide the asset off its own pivot.
    """
    lo, hi = asset.bounds()
    off = (0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2]))
    for p in asset.parts:
        _shift(p, (-off[0], -off[1], -off[2]))
    return asset


def _torso_ring(z, hx, hy, r, n_corner=3):
    """One CCW (x, y) torso cross-section at height ``z``.

    ``rounded_rect`` is already CCW in XY, which is exactly the right-hand
    order ``C.loft`` needs while advancing along +Z, so the rings can be fed
    to ``loft`` unchanged.  The rounded corners are what curve the vest's front
    and back panels: a hard rectangle here reads as a crate.
    """
    return [(px, py, z) for (px, py) in C.rounded_rect(hx, hy, r, n_corner)]


def _trigger_guard(m, y_front, z_top, color, w=0.018, bar=0.011, drop=0.040,
                   span=0.050, thick=0.010):
    """An open trigger guard: front post + bottom rail, 3-sided.

    Deliberately not a closed frame -- a filled loop would be 12 triangles of
    solid where two thin bars read the same and cost half.  ``y_front`` is the
    guard's forward post, ``span`` how far back the bottom rail reaches.
    Chamfered, because a guard is a bright steel loop seen against a dark
    frame and its silhouette edge is most of the read.
    """
    _cbox(m, (w, bar, drop), center=(0.0, y_front, z_top - drop * 0.5),
          color=color, bevel=bar * 0.30)
    _cbox(m, (w, span, thick),
          center=(0.0, y_front + span * 0.5 - bar * 0.5, z_top - drop),
          color=color, bevel=thick * 0.30)
    return m


def _serrations(m, y0, count, pitch, w, h, z, color, x=0.0):
    """Slide serrations: a row of chamfered grooves cut across the side.

    Seven of these are 196 triangles and they are the single detail that
    makes a slide read as machined steel rather than a smooth block, because
    each groove puts a pair of angled facets into a face that would
    otherwise be one dead value.
    """
    for k in range(count):
        _cbox(m, (w, pitch * 0.42, h),
              center=(x, y0 + k * pitch, z), color=color, bevel=pitch * 0.14)
    return m


def _iron_sights(m, y_front, y_rear, z, color, post=0.006, blade_h=0.020,
                 notch_w=0.020):
    """Front post + rear notch, the two ends of a backup sight line.

    Without them a weapon has no readable "up", and at gameplay distance the
    muzzle end is often the only part of a gun on screen.
    """
    _cbox(m, (post, post, blade_h), center=(0.0, y_front, z + blade_h * 0.5),
          color=color, bevel=post * 0.28)
    _cbox(m, (notch_w, post, blade_h * 0.9),
          center=(0.0, y_rear, z + blade_h * 0.45), color=color,
          bevel=post * 0.28)
    _cbox(m, (notch_w, post * 0.6, post * 0.6),
          center=(0.0, y_rear, z + blade_h * 0.9), color=color,
          bevel=post * 0.2)                                       # notch top
    return m


def _ribbed_rail(m, y0, y1, z, w, h, color, count):
    """A top rail: base bar plus ``count`` raised slots.

    A rail is one of the few places a weapon genuinely benefits from a
    normal break on every edge, because the whole point is the alternating
    light and dark along its length when the light rakes across it.
    """
    _cbox(m, (w, y1 - y0, h), center=(0.0, (y0 + y1) * 0.5, z), color=color,
          bevel=h * 0.22)
    pitch = (y1 - y0) / max(1, count)
    for k in range(count):
        _cbox(m, (w * 0.62, pitch * 0.40, h * 0.55),
              center=(0.0, y0 + pitch * (k + 0.5), z + h * 0.5),
              color=C.shade(color, 0.72), bevel=h * 0.10)
    return m


# ---------------------------------------------------------------------------
# 1. pistol -- ~0.21 m, origin at the centre of the grip
# ---------------------------------------------------------------------------

def _pistol():
    a = C.Asset("wep_pistol", "weapon")

    # Grip centre IS the origin, raked back 18 deg: local (0,0,+h) lands at
    # -Y (forward) so the top of the grip tucks under the frame and the
    # bottom trails to +Y the way a raked grip does.
    grip = a.part("pistol_grip", base_color=POLYMER, roughness=0.62)
    _cbox(grip.mesh, (0.032, 0.036, 0.102), rot=(18.0, 0.0, 0.0),
          color=POLYMER, bevel=0.006)
    for k in range(4):                               # finger-groove relief
        _cbox(grip.mesh, (0.036, 0.008, 0.008),
              center=(0.0, 0.008 + k * 0.001, -0.028 + k * 0.018),
              rot=(18.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.002)
    # Backstrap, the strip a palm actually wraps.
    _cbox(grip.mesh, (0.034, 0.010, 0.094), center=(0.0, 0.021, -0.004),
          rot=(18.0, 0.0, 0.0), color=C.shade(POLYMER, 1.28), bevel=0.003)
    # Magazine floor plate, proud of the grip so the bottom is not a flat cut.
    _cbox(grip.mesh, (0.036, 0.038, 0.010), center=(0.0, 0.030, -0.062),
          rot=(18.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.003)

    # slide + frame, one block, the mass of the silhouette
    body = a.part("pistol_body", base_color=GUNMETAL, metallic=0.25,
                  roughness=0.48)
    _cbox(body.mesh, (0.032, 0.160, 0.042), center=(0.0, -0.040, 0.066),
          color=GUNMETAL, bevel=0.005)               # slide
    # Slide top is a separate lighter value: the two-tone slide is what makes
    # the upper half of a pistol read as a separate part at a glance.
    _cbox(body.mesh, (0.028, 0.150, 0.012), center=(0.0, -0.042, 0.090),
          color=GUNMETAL_LT, bevel=0.003)
    _cbox(body.mesh, (0.026, 0.036, 0.030), center=(0.0, 0.016, 0.050),
          color=GUNMETAL, bevel=0.004)               # frame / dust cover
    _cbox(body.mesh, (0.026, 0.032, 0.028), center=(0.0, 0.042, 0.062),
          color=GUNMETAL_LT, bevel=0.005)            # rear sight / hammer
    _cbox(body.mesh, (0.020, 0.014, 0.026), center=(0.0, 0.056, 0.050),
          color=C.shade(GUNMETAL, 0.80), bevel=0.003)  # hammer spur
    # Accessory rail under the dust cover.
    _ribbed_rail(body.mesh, -0.004, 0.024, 0.038, 0.018, 0.008, POLYMER, 3)
    # Ejection port: a recessed dark plate on the +X flank, plus the cut edge
    # that catches light around it.
    _cbox(body.mesh, (0.006, 0.044, 0.026), center=(0.015, -0.052, 0.070),
          color=BARREL_DK, bevel=0.002)
    _cbox(body.mesh, (0.008, 0.050, 0.032), center=(0.013, -0.052, 0.070),
          color=GUNMETAL_LT, bevel=0.002)
    # Slide stop lever + magazine release, the two small controls on a frame.
    _cbox(body.mesh, (0.010, 0.024, 0.006), center=(-0.017, 0.020, 0.052),
          color=STEEL_BRIGHT, bevel=0.0015)
    _cbox(body.mesh, (0.010, 0.014, 0.016), center=(-0.016, 0.012, 0.036),
          color=STEEL_BRIGHT, bevel=0.002)

    metal = a.part("pistol_metal", base_color=STEEL_BRIGHT, metallic=0.6,
                   roughness=0.35)
    _serrations(body.mesh, 0.008, 5, 0.0085, 0.034, 0.036, 0.068,
                GUNMETAL_LT)
    _serrations(body.mesh, 0.008, 5, 0.0085, 0.034, 0.036, 0.068,
                C.shade(GUNMETAL_LT, 0.94), x=0.0)
    C.cylinder(metal.mesh, 0.0095, 0.024, 10, center=(0.0, -0.132, 0.066),
               axis="Y", color=BARREL)                # muzzle / barrel tip
    C.cylinder(metal.mesh, 0.0055, 0.016, 8, center=(0.0, -0.146, 0.066),
               axis="Y", color=BARREL_DK)             # bore
    C.torus(metal.mesh, 0.0090, 0.0018, 10, 4, center=(0.0, -0.142, 0.066),
            color=STEEL_BRIGHT, axis="Y", smooth=False)  # muzzle crown
    _cbox(metal.mesh, (0.008, 0.026, 0.010), center=(0.0, -0.124, 0.088),
          color=STEEL_BRIGHT, bevel=0.002)            # front sight blade
    _cbox(metal.mesh, (0.020, 0.008, 0.008), center=(0.0, 0.040, 0.088),
          color=STEEL_BRIGHT, bevel=0.0015)           # rear notch
    _trigger_guard(metal.mesh, -0.046, 0.048, STEEL_BRIGHT,
                   w=0.018, bar=0.011, drop=0.040, span=0.056)
    _cbox(metal.mesh, (0.013, 0.012, 0.026), center=(0.0, -0.024, 0.032),
          rot=(10.0, 0.0, 0.0), color=STEEL_BRIGHT, bevel=0.002)   # trigger

    trim = a.part("pistol_trim", base_color=BRASS_HI, metallic=0.7,
                  roughness=0.3)
    # A single warm accent.  One bright part on an otherwise cold weapon is
    # what the eye lands on first when the muzzle is 30 m away.
    _cbox(trim.mesh, (0.006, 0.014, 0.006), center=(0.0, -0.104, 0.086),
          color=BRASS_HI, bevel=0.0012)
    _cbox(trim.mesh, (0.026, 0.006, 0.006), center=(0.0, 0.028, 0.048),
          color=BRASS_HI, bevel=0.0012)
    return a


# ---------------------------------------------------------------------------
# 2. SMG -- ~0.47 m, origin at the centre of the grip
# ---------------------------------------------------------------------------

def _smg():
    a = C.Asset("wep_smg", "weapon")

    grip = a.part("smg_grip", base_color=POLYMER, roughness=0.62)
    _cbox(grip.mesh, (0.028, 0.036, 0.108), rot=(20.0, 0.0, 0.0),
          color=POLYMER, bevel=0.006)
    _cbox(grip.mesh, (0.032, 0.010, 0.100), center=(0.0, 0.024, -0.004),
          rot=(20.0, 0.0, 0.0), color=C.shade(POLYMER, 1.30), bevel=0.003)
    # 8 deg forward cant: the magazine bottom kicks toward -Y.  Tapered in
    # three steps so it is a magazine shape, not a slab.
    _cbox(grip.mesh, (0.024, 0.058, 0.170), center=(0.0, -0.062, -0.050),
          rot=(-8.0, 0.0, 0.0), color=POLYMER, bevel=0.005)     # magazine
    _cbox(grip.mesh, (0.026, 0.050, 0.040), center=(0.0, -0.048, -0.028),
          rot=(-8.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.004)   # mag well lip
    _cbox(grip.mesh, (0.030, 0.060, 0.014), center=(0.0, -0.072, -0.130),
          rot=(-8.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.003)   # floor plate
    _cbox(grip.mesh, (0.028, 0.042, 0.080), center=(0.0, -0.180, 0.008),
          color=POLYMER, bevel=0.006)                             # foregrip
    for k in range(3):                                            # grip ribs
        _cbox(grip.mesh, (0.030, 0.006, 0.062),
              center=(0.0, -0.180 + (k - 1) * 0.012, 0.008),
              color=POLYMER_LT, bevel=0.0015)

    body = a.part("smg_body", base_color=GUNMETAL, metallic=0.25, roughness=0.48)
    _cbox(body.mesh, (0.042, 0.172, 0.074), center=(0.0, -0.045, 0.068),
          color=GUNMETAL, bevel=0.006)                            # receiver
    _cbox(body.mesh, (0.036, 0.108, 0.052), center=(0.0, -0.182, 0.068),
          color=GUNMETAL_LT, bevel=0.005)                         # handguard
    # Handguard cooling slots: six dark recesses that break up the single
    # biggest flat face on the weapon.
    for k in range(6):
        _cbox(body.mesh, (0.040, 0.010, 0.026),
              center=(0.0, -0.226 + k * 0.016, 0.062),
              color=BARREL_DK, bevel=0.002)
    _cbox(body.mesh, (0.028, 0.028, 0.028), center=(0.0, 0.052, 0.068),
          color=GUNMETAL, bevel=0.004)                            # buffer tube
    # Stock: a tube, a sliding brace and a shoulder pad, so the extension
    # reads as three parts rather than one box.
    C.cylinder(body.mesh, 0.014, 0.070, 8, center=(0.0, 0.096, 0.064),
               axis="Y", color=POLYMER)
    _cbox(body.mesh, (0.030, 0.076, 0.046), center=(0.0, 0.104, 0.062),
          color=GUNMETAL, bevel=0.005)                            # stock body
    _cbox(body.mesh, (0.040, 0.014, 0.062), center=(0.0, 0.152, 0.060),
          color=GUNMETAL_LT, bevel=0.004)                         # butt pad
    _cbox(body.mesh, (0.010, 0.030, 0.010), center=(0.0, 0.118, 0.034),
          color=POLYMER_LT, bevel=0.002)                          # cheek riser
    _ribbed_rail(body.mesh, -0.130, 0.030, 0.110, 0.030, 0.010, GUNMETAL_LT, 7)
    _cbox(body.mesh, (0.014, 0.038, 0.020), center=(0.0, 0.014, 0.116),
          color=POLYMER, bevel=0.003)                             # charging handle
    _cbox(body.mesh, (0.006, 0.040, 0.024), center=(0.020, -0.060, 0.074),
          color=BARREL_DK, bevel=0.002)                           # ejection port
    _cbox(body.mesh, (0.008, 0.046, 0.030), center=(0.018, -0.060, 0.074),
          color=GUNMETAL_LT, bevel=0.002)
    _cbox(body.mesh, (0.012, 0.026, 0.014), center=(-0.024, 0.020, 0.060),
          color=STEEL_BRIGHT, bevel=0.002)                        # selector

    metal = a.part("smg_metal", base_color=STEEL_BRIGHT, metallic=0.6,
                   roughness=0.35)
    C.cylinder(metal.mesh, 0.013, 0.030, 10, center=(0.0, -0.249, 0.068),
               axis="Y", color=BARREL)                # barrel shroud tip
    C.cylinder(metal.mesh, 0.012, 0.022, 10, center=(0.0, -0.275, 0.068),
               axis="Y", color=BARREL)                # muzzle
    C.cylinder(metal.mesh, 0.006, 0.028, 8, center=(0.0, -0.287, 0.068),
               axis="Y", color=BARREL_DK)             # bore
    C.torus(metal.mesh, 0.0125, 0.0020, 10, 4, center=(0.0, -0.283, 0.068),
            color=STEEL_BRIGHT, axis="Y", smooth=False)  # muzzle crown
    _iron_sights(metal.mesh, -0.228, -0.100, 0.112, STEEL_BRIGHT,
                 post=0.007, blade_h=0.022, notch_w=0.024)
    _trigger_guard(metal.mesh, -0.048, 0.040, STEEL_BRIGHT,
                   w=0.018, bar=0.011, drop=0.038, span=0.052)
    _cbox(metal.mesh, (0.013, 0.012, 0.026), center=(0.0, -0.026, 0.026),
          rot=(10.0, 0.0, 0.0), color=STEEL_BRIGHT, bevel=0.002)   # trigger
    return a


# ---------------------------------------------------------------------------
# 3. shotgun -- ~0.78 m, origin at the centre of the grip
# ---------------------------------------------------------------------------

def _shotgun():
    a = C.Asset("wep_shotgun", "weapon")

    grip = a.part("shotgun_grip", base_color=WOOD, roughness=0.7)
    _cbox(grip.mesh, (0.028, 0.038, 0.110), rot=(20.0, 0.0, 0.0),
          color=WOOD, bevel=0.006)
    for k in range(4):
        _cbox(grip.mesh, (0.032, 0.007, 0.008),
              center=(0.0, 0.006 + k * 0.001, -0.030 + k * 0.019),
              rot=(20.0, 0.0, 0.0), color=WOOD_DK, bevel=0.0015)
    _cbox(grip.mesh, (0.030, 0.010, 0.100), center=(0.0, 0.026, -0.004),
          rot=(20.0, 0.0, 0.0), color=C.shade(WOOD, 1.24), bevel=0.003)
    # pump / forend, ribbed, the tell that reads "shotgun" not "rifle"
    _cbox(grip.mesh, (0.048, 0.138, 0.054), center=(0.0, -0.192, 0.044),
          color=WOOD, bevel=0.008)
    for k in range(7):                                  # 14 ribs, both sides
        _cbox(grip.mesh, (0.052, 0.007, 0.050),
              center=(0.0, -0.245 + k * 0.019, 0.044), color=WOOD_DK,
              bevel=0.0018)
    _cbox(grip.mesh, (0.050, 0.012, 0.056), center=(0.0, -0.192, 0.072),
          color=C.shade(WOOD, 1.18), bevel=0.003)        # pump cap

    body = a.part("shotgun_body", base_color=GUNMETAL, metallic=0.25,
                  roughness=0.5)
    _cbox(body.mesh, (0.040, 0.172, 0.062), center=(0.0, -0.010, 0.048),
          color=GUNMETAL, bevel=0.006)                              # receiver
    _cbox(body.mesh, (0.042, 0.030, 0.064), center=(0.0, -0.086, 0.048),
          color=GUNMETAL_LT, bevel=0.005)                           # barrel band
    # Stock: -6 deg about X drops the butt end, so the comb slopes down to the
    # heel the way a gunstock does instead of reading as a second box.
    _cbox(body.mesh, (0.036, 0.216, 0.060), center=(0.0, 0.182, 0.055),
          rot=(-6.0, 0.0, 0.0), color=GUNMETAL, bevel=0.008)
    _cbox(body.mesh, (0.042, 0.026, 0.074), center=(0.0, 0.290, 0.042),
          rot=(-6.0, 0.0, 0.0), color=POLYMER, bevel=0.006)         # recoil pad
    _cbox(body.mesh, (0.032, 0.012, 0.034), center=(0.0, 0.070, 0.044),
          color=GUNMETAL_LT, bevel=0.003)                           # joint
    # Side saddle: five shell loops on the +X of the receiver.  Real
    # geometry rather than a texture, so it casts its own shading.
    for k in range(5):
        C.torus(body.mesh, 0.0090, 0.0026, 8, 4,
                center=(-0.024, -0.058 + k * 0.026, 0.060),
                color=POLYMER_LT, axis="X", smooth=False)
    _cbox(body.mesh, (0.008, 0.140, 0.028), center=(-0.022, -0.006, 0.048),
          color=POLYMER, bevel=0.002)                              # saddle base
    _cbox(body.mesh, (0.006, 0.040, 0.022), center=(0.021, -0.030, 0.056),
          color=BARREL_DK, bevel=0.002)                             # action port

    metal = a.part("shotgun_metal", base_color=STEEL_BRIGHT, metallic=0.6,
                   roughness=0.35)
    C.cylinder(metal.mesh, 0.016, 0.400, 12, center=(0.0, -0.235, 0.055),
               axis="Y", color=BARREL)                # barrel
    C.cylinder(metal.mesh, 0.019, 0.022, 12, center=(0.0, -0.424, 0.055),
               axis="Y", color=BARREL_DK)             # muzzle ring
    C.torus(metal.mesh, 0.0175, 0.0022, 12, 4, center=(0.0, -0.430, 0.055),
            color=STEEL_BRIGHT, axis="Y", smooth=False)  # muzzle crown
    C.cylinder(metal.mesh, 0.013, 0.380, 10, center=(0.0, -0.205, 0.035),
               axis="Y", color=BARREL_DK)             # magazine tube
    C.cylinder(metal.mesh, 0.010, 0.018, 8, center=(0.0, -0.404, 0.035),
               axis="Y", color=STEEL_BRIGHT)          # tube end cap
    # Heat shield: four bridges over the barrel, a small detail that gives
    # the barrel a top edge instead of one unbroken cylinder.
    for k in range(4):
        _cbox(metal.mesh, (0.026, 0.006, 0.008),
              center=(0.0, -0.330 + k * 0.040, 0.070), color=BARREL_DK,
              bevel=0.0015)
    C.sphere(metal.mesh, 0.006, 8, 4, center=(0.0, -0.432, 0.076),
             color=STEEL_BRIGHT)                      # bead sight
    _cbox(metal.mesh, (0.010, 0.010, 0.014), center=(0.0, -0.424, 0.070),
          color=STEEL_BRIGHT, bevel=0.002)            # bead post
    _cbox(metal.mesh, (0.024, 0.010, 0.016), center=(0.0, -0.092, 0.076),
          color=STEEL_BRIGHT, bevel=0.002)            # rear sight
    _trigger_guard(metal.mesh, -0.044, 0.042, STEEL_BRIGHT,
                   w=0.018, bar=0.012, drop=0.038, span=0.052)
    _cbox(metal.mesh, (0.013, 0.012, 0.026), center=(0.0, -0.022, 0.026),
          rot=(10.0, 0.0, 0.0), color=STEEL_BRIGHT, bevel=0.002)   # trigger
    return a


# ---------------------------------------------------------------------------
# 4. rocket launcher -- ~1.15 m, origin at the centre of the grip
#
# The launcher's id and its forward shape are load-bearing for the engine, so
# this one gets the most triangles of anything in the file: it is a
# deliberately readable silhouette with a real sight line, a shoulder pad, a
# front sight bracket, a forward grip and an exhaust cone at the rear.
# ---------------------------------------------------------------------------

def _rocket_launcher():
    a = C.Asset("wep_rocket_launcher", "weapon")

    grip = a.part("rpg_grip", base_color=POLYMER, roughness=0.6)
    _cbox(grip.mesh, (0.032, 0.042, 0.118), rot=(18.0, 0.0, 0.0),
          color=POLYMER, bevel=0.006)
    for k in range(4):                                       # grip ribs
        _cbox(grip.mesh, (0.036, 0.008, 0.008),
              center=(0.0, 0.008 + k * 0.001, -0.030 + k * 0.019),
              rot=(18.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.002)
    _cbox(grip.mesh, (0.034, 0.012, 0.108), center=(0.0, 0.026, -0.004),
          rot=(18.0, 0.0, 0.0), color=C.shade(POLYMER, 1.30), bevel=0.003)
    # Forward grip, hand-hold tube, and the shoulder strut the whole weapon
    # hangs off -- three separate masses instead of one stub.
    _cbox(grip.mesh, (0.030, 0.040, 0.090), center=(0.0, -0.330, -0.004),
          color=POLYMER, bevel=0.005)                               # hand hold
    for k in range(3):
        _cbox(grip.mesh, (0.034, 0.006, 0.072),
              center=(0.0, -0.330 + (k - 1) * 0.013, -0.004),
              color=POLYMER_LT, bevel=0.0015)
    _cbox(grip.mesh, (0.030, 0.180, 0.032), center=(0.0, 0.170, 0.012),
          color=POLYMER, bevel=0.005)                               # shoulder strut
    _cbox(grip.mesh, (0.062, 0.032, 0.096), center=(0.0, 0.250, 0.028),
          rot=(-10.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.007)     # shoulder pad
    _cbox(grip.mesh, (0.064, 0.012, 0.060), center=(0.0, 0.264, 0.040),
          rot=(-10.0, 0.0, 0.0), color=C.shade(POLYMER, 0.72), bevel=0.004)
    _trigger_guard(grip.mesh, -0.052, 0.062, POLYMER,
                   w=0.020, bar=0.013, drop=0.044, span=0.058)
    _cbox(grip.mesh, (0.014, 0.013, 0.028), center=(0.0, -0.028, 0.040),
          rot=(10.0, 0.0, 0.0), color=POLYMER_LT, bevel=0.002)      # trigger
    _cbox(grip.mesh, (0.012, 0.022, 0.024), center=(0.022, 0.010, 0.056),
          color=STEEL_BRIGHT, bevel=0.002)                          # safety

    body = a.part("rpg_body", base_color=OLIVE, metallic=0.2, roughness=0.62)
    C.cylinder(body.mesh, 0.052, 0.740, 14, center=(0.0, -0.265, 0.075),
               axis="Y", color=OLIVE)                  # launch tube
    C.cone(body.mesh, 0.055, 0.062, 14, center=(0.0, 0.136, 0.075),
           radius_top=0.072, axis="Y", color=OLIVE_DK)  # rear exhaust flare
    C.cylinder(body.mesh, 0.074, 0.014, 14, center=(0.0, 0.170, 0.075),
               axis="Y", color=C.shade(OLIVE_DK, 0.80))  # exhaust lip
    C.cylinder(body.mesh, 0.070, 0.020, 14, center=(0.0, 0.100, 0.075),
               axis="Y", color=BARREL_DK)             # venturi throat
    _ribbed_rail(body.mesh, -0.520, -0.060, 0.134, 0.022, 0.012, OLIVE_DK, 9)
    for k in range(4):                                  # tube bands
        C.cylinder(body.mesh, 0.057, 0.018, 14, center=(0.0, -0.470 + k * 0.180,
                                                        0.075),
                   axis="Y", color=OLIVE_DK)
        _cbox(body.mesh, (0.014, 0.010, 0.024),
              center=(0.0, -0.470 + k * 0.180, 0.130), color=OLIVE_LT,
              bevel=0.002)                             # band lug
    # Grenade-sight style cut-outs along the tube: four raised bosses, each
    # with a dark face, so the tube has a rhythm instead of being a pipe.
    for k in range(4):
        _cbox(body.mesh, (0.018, 0.026, 0.014),
              center=(0.058, -0.420 + k * 0.130, 0.075), color=OLIVE_LT,
              bevel=0.003)
        _cbox(body.mesh, (0.006, 0.016, 0.010),
              center=(0.068, -0.420 + k * 0.130, 0.075), color=BARREL_DK,
              bevel=0.002)
    # Blow-off panel seam and a stencilled hazard band, both face colour.
    C.cylinder(body.mesh, 0.0535, 0.006, 14, center=(0.0, 0.060, 0.075),
               axis="Y", color=OLIVE_LT)
    C.cylinder(body.mesh, 0.0545, 0.030, 14, center=(0.0, -0.170, 0.075),
               axis="Y", color=AMBER_STENCIL)

    metal = a.part("rpg_metal", base_color=STEEL_BRIGHT, metallic=0.65,
                   roughness=0.32)
    # Flared muzzle: cone radius is the -Y end, radius_top the +Y end, so the
    # flare opens toward the muzzle the way it should.
    C.cone(metal.mesh, 0.100, 0.075, 14, center=(0.0, -0.6675, 0.075),
           radius_top=0.055, axis="Y", color=BARREL)    # flare
    C.cylinder(metal.mesh, 0.104, 0.016, 14, center=(0.0, -0.713, 0.075),
               axis="Y", color=BARREL_DK)             # muzzle lip
    C.torus(metal.mesh, 0.102, 0.004, 14, 4, center=(0.0, -0.708, 0.075),
            color=STEEL_BRIGHT, axis="Y", smooth=False)  # lip crown
    C.cylinder(metal.mesh, 0.052, 0.014, 14, center=(0.0, -0.635, 0.075),
               axis="Y", color=BARREL_DK)             # blast-shield collar
    C.cylinder(metal.mesh, 0.040, 0.190, 10, center=(0.0, 0.040, 0.075),
               axis="Y", color=BARREL_DK)             # breech
    C.cylinder(metal.mesh, 0.046, 0.016, 10, center=(0.0, -0.058, 0.075),
               axis="Y", color=STEEL_BRIGHT)          # breech ring
    # Rear sight: a bracket on a stalk with a fold-up aperture, so the eye
    # line runs muzzle -> tube -> bracket.
    _cbox(metal.mesh, (0.032, 0.094, 0.046), center=(0.0, 0.120, 0.098),
          color=BARREL, bevel=0.005)
    _cbox(metal.mesh, (0.010, 0.014, 0.052), center=(0.0, 0.152, 0.146),
          color=STEEL_BRIGHT, bevel=0.002)            # aperture stalk
    C.torus(metal.mesh, 0.014, 0.003, 12, 4, center=(0.0, 0.152, 0.174),
            color=STEEL_BRIGHT, axis="Y", smooth=False)  # aperture ring
    C.cylinder(metal.mesh, 0.006, 0.008, 8, center=(0.0, 0.152, 0.174),
               axis="Y", color=STEEL_BRIGHT)
    # Front sight bracket, 130 mm forward of the rear sight: two stanchions
    # and a cross-bar with a post in the middle.
    for sx in (-1, 1):
        _cbox(metal.mesh, (0.010, 0.012, 0.070),
              center=(sx * 0.020, -0.230, 0.170), color=STEEL_BRIGHT,
              bevel=0.002)
    _cbox(metal.mesh, (0.050, 0.012, 0.012), center=(0.0, -0.230, 0.208),
          color=STEEL_BRIGHT, bevel=0.003)            # cross-bar
    _cbox(metal.mesh, (0.008, 0.010, 0.024), center=(0.0, -0.230, 0.196),
          color=STEEL_BRIGHT, bevel=0.002)            # front post
    # Optic body + lens band on the tube's left shoulder.
    _cbox(metal.mesh, (0.034, 0.096, 0.046), center=(0.0, -0.210, 0.150),
          color=BARREL, bevel=0.006)                  # optic body
    C.cylinder(metal.mesh, 0.020, 0.016, 10, center=(0.0, -0.260, 0.150),
               axis="Y", color=STEEL_BRIGHT)          # objective ring
    C.cylinder(metal.mesh, 0.016, 0.010, 10, center=(0.0, -0.258, 0.150),
               axis="Y", color=NEON_TEAL_DK)          # lens
    _cbox(metal.mesh, (0.012, 0.016, 0.026), center=(0.0, -0.164, 0.150),
          color=STEEL_BRIGHT, bevel=0.002)            # eyepiece
    # Firing cable: a shallow arc clipped along the tube's right shoulder.
    C.tube(metal.mesh, [(0.048, -0.060, 0.108), (0.052, -0.100, 0.100),
                        (0.052, -0.140, 0.104), (0.048, -0.166, 0.112)],
           0.004, 5, color=POLYMER, smooth=False)
    return a


# ---------------------------------------------------------------------------
# 5. bat -- ~1.05 m, origin at the centre of the grip
# ---------------------------------------------------------------------------

def _bat():
    """A taped-handle club, ~0.72 m end to end.

    Same origin contract as the four guns: the origin is the centre of the
    GRIP, not of the bat, so a runtime can parent it to a hand bone exactly as
    it parents a pistol.  The club points along +Z so the silhouette reads
    upright in the weapon preview instead of edge-on.
    """
    a = C.Asset("wep_bat", "weapon")

    grip = a.part("bat_grip", base_color=BAT_WOOD, roughness=0.72)
    # Tapered handle: three stations so it is not a plain dowel.
    C.loft(grip.mesh,
           [[(r * math.cos(t), r * math.sin(t), z)
             for t in [2.0 * math.pi * i / 8 for i in range(8)]]
            for (r, z) in ((0.028, -0.150), (0.031, -0.040), (0.030, 0.075))],
           BAT_WOOD_DK, cap_start_flip=True, cap_end_flip=False)
    # Grip tape: five bands, alternating shade, so the hand position reads.
    for k, z in enumerate((-0.128, -0.086, -0.044, -0.002, 0.040)):
        C.cylinder(grip.mesh, 0.0345, 0.030, 8, center=(0.0, 0.0, z),
                   color=BAT_TAPE if k % 2 == 0 else C.shade(BAT_TAPE, 1.35))
    # Knurled diamonds: a ring of small blocks, the texture of a grip.
    for k in range(6):
        a2 = 2.0 * math.pi * k / 6
        _cbox(grip.mesh, (0.008, 0.008, 0.060),
              center=(0.033 * math.cos(a2), 0.033 * math.sin(a2), -0.020),
              color=C.shade(BAT_TAPE, 1.6), bevel=0.0015)

    body = a.part("bat_body", base_color=BAT_WOOD, roughness=0.62)
    # Barrel -> barrel swell -> blunt cap, as one closed flat loft.
    C.loft(body.mesh,
           [[(r * math.cos(t), r * math.sin(t), z)
             for t in [2.0 * math.pi * i / 10 for i in range(10)]]
            for (r, z) in ((0.031, 0.078), (0.033, 0.185), (0.040, 0.330),
                           (0.048, 0.455), (0.052, 0.530), (0.050, 0.556))],
           BAT_WOOD, cap_start_flip=True, cap_end_flip=False)
    # Two lacquered bands near the throat, face colour only.
    C.recolor_faces_where(
        body, lambda n, c: 0.178 < c[2] < 0.226, FLARE_RED)
    C.recolor_faces_where(
        body, lambda n, c: 0.402 < c[2] < 0.442, C.shade(BAT_WOOD_DK, 0.8))

    knob = a.part("bat_knob", base_color=BAT_TAPE, roughness=0.55)
    C.cylinder(knob.mesh, 0.036, 0.022, 8, center=(0.0, 0.0, -0.160),
               color=BAT_TAPE)                                # butt end cap
    C.cylinder(knob.mesh, 0.052, 0.020, 10, center=(0.0, 0.0, 0.562),
               color=C.shade(BAT_TAPE, 1.2))                 # strike face ring
    C.cylinder(knob.mesh, 0.044, 0.012, 10, center=(0.0, 0.0, 0.556),
               color=C.shade(BAT_TAPE, 1.6))                 # strike face
    for k in range(6):                                        # face screws
        a2 = 2.0 * math.pi * k / 6
        C.cylinder(knob.mesh, 0.005, 0.014, 6,
                   center=(0.028 * math.cos(a2), 0.028 * math.sin(a2), 0.562),
                   color=STEEL_BRIGHT)
    return a


# ---------------------------------------------------------------------------
# 6. grenade -- ~0.14 m, origin at the centre of the body
# ---------------------------------------------------------------------------

def _grenade():
    """A frag with a pull ring and a safety pin.

    Origin at the body's centre, which is where the runtime's hand/throw
    transform expects a point-throw primitive -- unlike the guns, this is not
    meant to be sighted down its own length.
    """
    a = C.Asset("wep_grenade", "weapon")

    body = a.part("gren_body", base_color=GREN_BODY, roughness=0.55)
    # Classic two-dome silhouette: sphere, waist, sphere.  A single sphere
    # reads as a ball bearing.
    C.sphere(body.mesh, 0.0425, 12, 6, center=(0.0, 0.0, 0.0075), squash=0.86,
             color=GREN_BODY)
    C.sphere(body.mesh, 0.0425, 12, 6, center=(0.0, 0.0, -0.0075), squash=0.86,
             color=GREN_BODY)
    C.cylinder(body.mesh, 0.0335, 0.030, 10, center=(0.0, 0.0, 0.0),
               color=GREN_BODY_DK)                            # moulded waist
    # Cast ribs around the waist: a frag has a part line, and the ribs give
    # the equator a normal break in every frame.  Two bars crossing on the X
    # and Y axes in the same darker value as the mould line, plus four
    # lighter bosses on the diagonals.  Each is placed at its own coordinate
    # rather than by rotating one block, which would stack four copies on
    # the same 26 mm of surface.
    for size in ((0.010, 0.068, 0.026), (0.068, 0.010, 0.026)):
        _cbox(body.mesh, size, center=(0.0, 0.0, 0.0), color=GREN_BODY_DK,
              bevel=0.002)
    for k in range(4):
        a2 = math.pi * 0.5 * k
        _cbox(body.mesh, (0.010, 0.010, 0.026),
              center=(0.035 * math.cos(a2), 0.035 * math.sin(a2), 0.0),
              color=C.shade(GREN_BODY_DK, 1.45), bevel=0.002)
    # Fuse assembly: neck, collar, striker cap, and the safety lever.
    C.cylinder(body.mesh, 0.0175, 0.020, 8, center=(0.0, 0.0, 0.047),
               color=GREN_BODY_DK)
    C.cylinder(body.mesh, 0.0205, 0.008, 8, center=(0.0, 0.0, 0.041),
               color=C.shade(GREN_BODY_DK, 1.3))
    _cbox(body.mesh, (0.010, 0.044, 0.008), center=(0.0, 0.020, 0.056),
          color=GREN_BODY_DK, bevel=0.002)                    # safety lever
    # Muzzle-authored sight band, a face-colour detail that gives the olive
    # dome a top and a bottom at gameplay distance.
    C.recolor_faces_where(
        body, lambda n, c: n[2] < -0.72, GREN_BODY_DK)

    cap = a.part("gren_cap", base_color=STEEL_BRIGHT, metallic=0.6,
                 roughness=0.35)
    C.cylinder(cap.mesh, 0.0165, 0.014, 8, center=(0.0, 0.0, 0.062),
               color=STEEL_BRIGHT)                            # striker cap
    C.cylinder(cap.mesh, 0.0055, 0.030, 6, center=(0.010, 0.0, 0.080),
               axis="X", color=STEEL_BRIGHT)                  # pin, pulled out
    C.torus(cap.mesh, 0.0045, 0.0014, 8, 4, center=(0.026, 0.0, 0.080),
            color=STEEL_BRIGHT, axis="X", smooth=False)       # pin eye

    ring = a.part("gren_ring", base_color=BRASS, metallic=0.7, roughness=0.32)
    # Pull ring hanging off the pin's eye, plane of the ring facing the user.
    C.torus(ring.mesh, 0.0135, 0.0030, 10, 5, center=(0.024, 0.0, 0.076),
            color=BRASS, axis="Y", smooth=False)
    return a


# ---------------------------------------------------------------------------
# 7. armour vest -- 0.45 wide x 0.55 tall x 0.22 deep, centred
# ---------------------------------------------------------------------------

def _armor_vest():
    a = C.Asset("pickup_armor_vest", "pickup")

    # Envelope target: 0.45 wide (X) x 0.55 tall (Z) x 0.22 deep (Y).  The
    # shell is authored at full size, then _center() lands the bounds centre
    # on the origin -- neck opening above, hem below.
    #
    # Torso shell: rounded cross-sections lofted up the Z axis.  The rounded
    # corners ARE the "slightly curved front and back panels" -- a hard
    # rectangle here reads as a crate, not a body armour carrier.  The half
    # depth is held at 0.106 so the shell alone spans 0.212 of the 0.22
    # budget and the pouches / trim fit inside it.
    shell = a.part("vest_shell", base_color=ARMO_1, roughness=0.66)
    C.loft(shell.mesh,
           [_torso_ring(-0.272, 0.155, 0.068, 0.048),
            _torso_ring(-0.200, 0.192, 0.086, 0.060),
            _torso_ring(-0.060, 0.210, 0.098, 0.068),
            _torso_ring(0.070, 0.216, 0.100, 0.068),
            _torso_ring(0.170, 0.206, 0.092, 0.062),
            _torso_ring(0.228, 0.188, 0.076, 0.050)],
           ARMO_1, cap_start_flip=True, cap_end_flip=False)
    # Quilted-panel seams as face colour: the horizontal breaks are what stop
    # the shell reading as one continuous teal slab.
    C.recolor_faces_where(
        shell, lambda n, c: abs(c[2] - 0.020) < 0.012, C.shade(ARMO_1, 0.82))
    C.recolor_faces_where(
        shell, lambda n, c: abs(c[2] + 0.150) < 0.012, C.shade(ARMO_1, 0.82))

    plate = a.part("vest_plate", base_color=ARMO_2, roughness=0.5)
    _cbox(plate.mesh, (0.290, 0.026, 0.370), center=(0.0, -0.088, -0.020),
          color=ARMO_2, bevel=0.008)                          # front plate
    _cbox(plate.mesh, (0.310, 0.020, 0.350), center=(0.0, 0.088, -0.020),
          color=ARMO_2, bevel=0.008)                          # back plate
    # Front plate gets its own raised border, a different value, so the panel
    # has an edge rather than dissolving into the shell behind it.
    _cbox(plate.mesh, (0.250, 0.018, 0.330), center=(0.0, -0.096, -0.020),
          color=C.shade(ARMO_2, 0.80), bevel=0.006)
    for sx in (-1, 1):                                  # magazine pouches
        _cbox(plate.mesh, (0.092, 0.044, 0.150),
              center=(sx * 0.096, -0.090, -0.180), color=ARMO_2, bevel=0.008)
        _cbox(plate.mesh, (0.100, 0.010, 0.022),
              center=(sx * 0.096, -0.106, -0.110), color=ARMO_TRIM,
              bevel=0.003)
        _cbox(plate.mesh, (0.084, 0.014, 0.014),
              center=(sx * 0.096, -0.108, -0.196), color=C.shade(ARMO_2, 0.7),
              bevel=0.003)                                   # pouch flap line

    straps = a.part("vest_straps", base_color=ARMO_STRAP, roughness=0.72)
    # Two shoulder straps arch over the top leaving a neck gap between them.
    for sx in (-1, 1):
        _cbox(straps.mesh, (0.078, 0.186, 0.028),
              center=(sx * 0.126, 0.0, 0.252), color=ARMO_STRAP, bevel=0.006)
    _cbox(straps.mesh, (0.180, 0.030, 0.070), center=(0.0, 0.072, 0.244),
          color=ARMO_STRAP, bevel=0.008)                         # back neck yoke
    for sx in (-1, 1):                                  # side cummerbunds
        _cbox(straps.mesh, (0.018, 0.160, 0.200), center=(sx * 0.207, 0.0, 0.020),
              color=ARMO_STRAP, bevel=0.005)
        _cbox(straps.mesh, (0.026, 0.056, 0.046), center=(sx * 0.205, 0.0, 0.020),
              color=ARMO_TRIM, bevel=0.004)
        _cbox(straps.mesh, (0.024, 0.020, 0.180), center=(sx * 0.212, -0.060, 0.060),
              color=C.shade(ARMO_STRAP, 0.72), bevel=0.003)      # adjuster web

    trim = a.part("vest_trim", base_color=ARMO_TRIM, roughness=0.55)
    _cbox(trim.mesh, (0.146, 0.010, 0.024), center=(0.0, -0.101, 0.075),
          color=ARMO_TRIM, bevel=0.003)                      # hi-viz chest flash
    _cbox(trim.mesh, (0.146, 0.010, 0.024), center=(0.0, 0.098, 0.075),
          color=ARMO_TRIM, bevel=0.003)
    for sx in (-1, 1):                                  # shoulder yoke tabs
        _cbox(trim.mesh, (0.048, 0.124, 0.011), center=(sx * 0.126, 0.0, 0.270),
              color=ARMO_TRIM, bevel=0.003)
    for sx in (-1, 1):                                  # side hi-viz strip
        _cbox(trim.mesh, (0.010, 0.150, 0.018), center=(sx * 0.216, 0.0, -0.060),
              color=ARMO_TRIM, bevel=0.003)
    return _center(a)


# ---------------------------------------------------------------------------
# 8. first-aid pack -- 0.30 wide x 0.20 tall x 0.12 deep, centred
# ---------------------------------------------------------------------------

def _health_pack():
    a = C.Asset("pickup_health_pack", "pickup")

    # Envelope target: 0.30 wide (X) x 0.20 tall (Z) x 0.12 deep (Y).
    #
    # Every fitting is kept INSIDE that envelope rather than allowed to grow
    # it, which is why the numbers here are hand-checked instead of derived:
    # hinges proud of the case add 20-30 mm to a 300 mm box, and a handle
    # that arcs above the lid adds it in Z.  The handle is a low-profile loop
    # whose bar caps exactly flush with the case top (0.100), and the cross
    # straddles the front wall rather than standing off it.
    case = a.part("med_case", base_color=MED_RED, roughness=0.55)
    _cbox(case.mesh, (0.300, 0.116, 0.200), color=MED_RED, bevel=0.012)
    _cbox(case.mesh, (0.300, 0.116, 0.030), center=(0.0, 0.0, 0.085),
          color=MED_RED_DK, bevel=0.008)                             # lid
    _cbox(case.mesh, (0.300, 0.118, 0.012), center=(0.0, 0.0, 0.064),
          color=MED_RED_DK, bevel=0.004)                             # clasp band
    _cbox(case.mesh, (0.280, 0.106, 0.014), center=(0.0, 0.0, -0.093),
          color=MED_RED_DK, bevel=0.004)                             # base plinth
    for sx in (-1, 1):                                  # handle posts
        _cbox(case.mesh, (0.020, 0.034, 0.024), center=(sx * 0.036, 0.0, 0.082),
              color=MED_RED_DK, bevel=0.004)
    _cbox(case.mesh, (0.092, 0.034, 0.006), center=(0.0, 0.0, 0.097),
          color=MED_RED_DK, bevel=0.002)                             # handle bar
    # Moulded ribs on the lid, so the top face is not one dead value.
    for k in range(3):
        _cbox(case.mesh, (0.240, 0.012, 0.008), center=(0.0, -0.030 + k * 0.030, 0.100),
              color=C.shade(MED_RED, 1.20), bevel=0.002)

    trim = a.part("med_trim", base_color=MED_WHITE, roughness=0.5)
    # The cross: two slim boxes straddling the front (-Y) wall, 5 mm proud of
    # it and 5 mm sunk in, so it is a real protruding inlay rather than a
    # coplanar decal that would z-fight with the case face.
    _cbox(trim.mesh, (0.042, 0.010, 0.130), center=(0.0, -0.057, -0.006),
          color=MED_WHITE, bevel=0.003)
    _cbox(trim.mesh, (0.130, 0.010, 0.042), center=(0.0, -0.057, -0.006),
          color=MED_WHITE, bevel=0.003)
    _cbox(trim.mesh, (0.056, 0.010, 0.144), center=(0.0, -0.058, -0.006),
          color=C.shade(MED_WHITE, 0.86), bevel=0.003)              # cross backing
    for sx in (-1, 1):                                  # lid clasps
        _cbox(trim.mesh, (0.030, 0.010, 0.048), center=(sx * 0.098, -0.058, 0.046),
              color=MED_WHITE, bevel=0.003)
    _cbox(trim.mesh, (0.130, 0.008, 0.016), center=(0.0, -0.057, -0.080),
          color=MED_WHITE, bevel=0.002)                             # lower label bar
    _cbox(trim.mesh, (0.086, 0.008, 0.012), center=(0.0, 0.059, 0.086),
          color=MED_WHITE, bevel=0.002)                             # rear label

    metal = a.part("med_metal", base_color=MED_GREY, metallic=0.5,
                   roughness=0.4)
    for sx in (-1, 1):                                  # side hinges
        C.cylinder(metal.mesh, 0.008, 0.024, 8,
                   center=(sx * 0.136, 0.0, 0.048), axis="X", color=MED_GREY)
    # Corner bumpers: four bright chips that make a red box read as gear.
    for sx in (-1, 1):
        for sy in (-1, 1):
            _cbox(metal.mesh, (0.026, 0.026, 0.026),
                  center=(sx * 0.130, sy * 0.044, -0.086),
                  color=MED_GREY, bevel=0.005)
    return _center(a)


# ---------------------------------------------------------------------------
# 9. ammo box -- 0.35 wide x 0.17 tall x 0.23 deep, centred
# ---------------------------------------------------------------------------

def _ammo_box():
    """Olive steel ammo can with a latching lid.

    Origin centred like the other pickups: a pickup is spawned in the world,
    not parented to a hand.
    """
    a = C.Asset("pickup_ammo_box", "pickup")

    body = a.part("ammo_body", base_color=OLIVE, metallic=0.30, roughness=0.60)
    _cbox(body.mesh, (0.340, 0.200, 0.128), color=OLIVE, bevel=0.012)  # tin
    _cbox(body.mesh, (0.348, 0.208, 0.020), center=(0.0, 0.0, 0.064),
          color=OLIVE_DK, bevel=0.006)                                # lid flange
    _cbox(body.mesh, (0.348, 0.208, 0.014), center=(0.0, 0.0, -0.070),
          color=OLIVE_DK, bevel=0.005)                                # base flange
    # Pressed side ribs: real geometry now, chamfered, three per side.  A can
    # without ribs is a shoebox.
    for k in range(3):
        for sx in (-1, 1):
            _cbox(body.mesh, (0.008, 0.150, 0.016),
                  center=(sx * 0.171, 0.0, -0.044 + k * 0.044),
                  color=OLIVE_DK, bevel=0.003)
    # A recessed top panel + a bright stencil bar: two values on the lid.
    _cbox(body.mesh, (0.250, 0.130, 0.008), center=(0.0, 0.010, 0.075),
          color=OLIVE_LT, bevel=0.003)
    C.recolor_faces_where(
        body, lambda n, c: abs(n[1]) > 0.70 and abs(c[2]) < 0.030,
        OLIVE_DK)

    latch = a.part("ammo_latch", base_color=MED_GREY, metallic=0.55,
                   roughness=0.42)
    for sx in (-1, 1):
        _cbox(latch.mesh, (0.030, 0.014, 0.052),
              center=(sx * 0.112, -0.104, 0.028), color=MED_GREY, bevel=0.004)
        C.cylinder(latch.mesh, 0.008, 0.026, 6,
                   center=(sx * 0.112, -0.112, 0.006), axis="Y",
                   color=STEEL_BRIGHT)                             # catch pin
    _cbox(latch.mesh, (0.150, 0.014, 0.012), center=(0.0, -0.104, 0.060),
          color=MED_GREY, bevel=0.003)                              # hasp
    _cbox(latch.mesh, (0.040, 0.010, 0.030), center=(0.0, -0.110, 0.050),
          color=STEEL_BRIGHT, bevel=0.003)                         # hasp keeper

    stencil = a.part("ammo_stencil", base_color=AMBER_STENCIL, roughness=0.7)
    # Struck through 5 mm proud of the front wall so it reads as paint on tin
    # rather than a coplanar decal that z-fights with the body face.
    _cbox(stencil.mesh, (0.180, 0.010, 0.026), center=(0.0, -0.101, -0.014),
          color=AMBER_STENCIL, bevel=0.003)
    _cbox(stencil.mesh, (0.120, 0.010, 0.016), center=(0.0, -0.101, -0.042),
          color=AMBER_STENCIL, bevel=0.003)
    for sx in (-1, 1):                                             # stencil dot
        C.cylinder(stencil.mesh, 0.014, 0.010, 8,
                   center=(sx * 0.128, -0.101, -0.014), axis="Y",
                   color=AMBER_STENCIL)
    handle = a.part("ammo_handle", base_color=MED_GREY, metallic=0.5,
                    roughness=0.45)
    _cbox(handle.mesh, (0.130, 0.030, 0.014), center=(0.0, 0.0, 0.084),
          color=MED_GREY, bevel=0.004)                            # carry bar
    for sx in (-1, 1):
        _cbox(handle.mesh, (0.016, 0.030, 0.024),
              center=(sx * 0.058, 0.0, 0.076), color=MED_GREY, bevel=0.003)
    return _center(a)


# ---------------------------------------------------------------------------
# 10. cash stack -- 0.33 wide x 0.11 tall x 0.28 deep, centred
# ---------------------------------------------------------------------------

def _cash_stack():
    """Four banded bundles of notes plus a loose wad on top.

    Banded bundles are how the shape stays legible at 20 m: a single block
    reads as a brick, four stepped bands read as money.
    """
    a = C.Asset("pickup_cash_stack", "pickup")

    notes = a.part("cash_notes", base_color=BILL_GREEN, roughness=0.85)
    bands = a.part("cash_bands", base_color=BILL_TAN, roughness=0.9)
    # (size, centre, colour) per bundle, deliberately stepped in X and Y so the
    # pile is not three identical bricks in a row.
    bundles = (
        ((0.300, 0.196, 0.030), (-0.006, -0.052, -0.044), BILL_GREEN),
        ((0.272, 0.180, 0.026), (0.028, 0.040, -0.016), BILL_GREEN_DK),
        ((0.240, 0.162, 0.022), (-0.030, 0.026, 0.012), BILL_GREEN),
        ((0.170, 0.130, 0.020), (0.034, -0.026, 0.033), BILL_TAN_DK),
    )
    for size, centre, col in bundles:
        _cbox(notes.mesh, size, center=centre, color=col, bevel=0.004)
    # Edge striping: the top bundle gets a printed band across its face, and
    # the lower ones get a lighter chamfer, so the pile has three values.
    _cbox(notes.mesh, (0.140, 0.100, 0.004), center=(0.034, -0.026, 0.044),
          color=BILL_TAN, bevel=0.0015)
    C.recolor_faces_where(
        notes, lambda n, c: n[2] > 0.6, C.shade(BILL_GREEN, 1.14))
    # Rubber bands: one hoop around each of the three lower bundles.  A band is
    # two thin straps, not a closed ring, so the bundle shows through.
    for size, centre, _col in bundles[:3]:
        hx, hy, hz = size[0] * 0.5, size[1] * 0.5, size[2]
        for sx in (-1, 1):
            _cbox(bands.mesh, (0.014, hy * 2.04, hz * 1.06),
                  center=(centre[0] + sx * hx * 0.52, centre[1], centre[2]),
                  color=BILL_TAN, bevel=0.002)
        _cbox(bands.mesh, (hx * 2.04, 0.014, hz * 1.06),
              center=(centre[0], centre[1] + hy * 0.52, centre[2]),
              color=BILL_TAN, bevel=0.002)
    return _center(a)


# ---------------------------------------------------------------------------
# 11. oxygen tank -- 0.16 wide x 0.64 tall x 0.16 deep, centred
# ---------------------------------------------------------------------------

def _o2_tank():
    """Scuba cylinder with a valve wheel and a carry handle."""
    a = C.Asset("pickup_o2_tank", "pickup")

    body = a.part("tank_body", base_color=TANK_WHITE, metallic=0.35,
                  roughness=0.42)
    # Rounded-shoulder cylinder: three lofted rings plus a spherical dome cap
    # reads far better than a flat-ended tube.
    C.loft(body.mesh,
           [[(r * math.cos(t), r * math.sin(t), z)
             for t in [2.0 * math.pi * i / 12 for i in range(12)]]
            for (r, z) in ((0.062, -0.250), (0.072, -0.190), (0.072, 0.140),
                           (0.062, 0.196))],
           TANK_WHITE, cap_start_flip=True, cap_end_flip=False)
    C.sphere(body.mesh, 0.062, 12, 5, center=(0.0, 0.0, 0.196), squash=0.92,
             color=TANK_WHITE)
    C.sphere(body.mesh, 0.062, 12, 5, center=(0.0, 0.0, -0.250), squash=0.92,
             color=TANK_WHITE)
    # Two teal wrap bands -- the only place the pickup category gets neon --
    # plus a stencil plate, so the cylinder has a front as well as stripes.
    for z in (-0.090, 0.050):
        C.cylinder(body.mesh, 0.0735, 0.034, 12, center=(0.0, 0.0, z),
                   color=NEON_TEAL)
        C.cylinder(body.mesh, 0.0740, 0.006, 12, center=(0.0, 0.0, z + 0.020),
                   color=NEON_TEAL_DK)
    _cbox(body.mesh, (0.090, 0.016, 0.120), center=(0.0, -0.070, -0.010),
          color=NEON_TEAL_DK, bevel=0.005)                      # stencil plate
    C.recolor_faces_where(
        body, lambda n, c: n[2] < -0.80, C.shade(TANK_WHITE, 0.86))

    valve = a.part("tank_valve", base_color=BRASS, metallic=0.7, roughness=0.34)
    C.cylinder(valve.mesh, 0.026, 0.048, 8, center=(0.0, 0.0, 0.268),
               color=BRASS)                                       # valve body
    C.cylinder(valve.mesh, 0.0125, 0.036, 8, center=(0.0, 0.0, 0.308),
               color=BRASS)                                       # stem
    C.cylinder(valve.mesh, 0.0145, 0.050, 6,
               center=(0.034, 0.0, 0.308), axis="X", color=BRASS)  # outlet
    C.torus(valve.mesh, 0.030, 0.0068, 12, 5, center=(0.0, 0.0, 0.330),
            color=BRASS, smooth=False)                            # hand wheel
    # Four spokes, each on its own bearing, so the wheel reads as a wheel
    # rather than as a bare torus.  A spoke is a bar from the hub out to the
    # rim: its long axis follows the spoke direction, and 7 mm thick on the
    # other two.
    for k in range(4):
        a2 = math.pi * 0.5 * k
        dx, dy = 0.030 * math.cos(a2), 0.030 * math.sin(a2)
        _cbox(valve.mesh, (abs(dx) + 0.007, abs(dy) + 0.007, 0.007),
              center=(0.0, 0.0, 0.330), color=BRASS, bevel=0.0015)
    _cbox(valve.mesh, (0.014, 0.014, 0.030), center=(0.0, -0.028, 0.272),
          color=C.shade(BRASS, 0.80), bevel=0.002)                # outlet boss

    handle = a.part("tank_handle", base_color=STEEL_BRIGHT, metallic=0.6,
                    roughness=0.38)
    # The carry handle arcs over the crown and clears the valve wheel by
    # 30 mm, which is why it is a tube rather than a box.
    C.tube(handle.mesh, [(-0.058, 0.0, 0.150), (-0.046, 0.0, 0.238),
                         (0.0, 0.0, 0.268), (0.046, 0.0, 0.238),
                         (0.058, 0.0, 0.150)], 0.0105, 6, color=STEEL_BRIGHT,
           smooth=False)
    for sx in (-1, 1):                                             # handle feet
        C.cylinder(handle.mesh, 0.013, 0.020, 6, center=(sx * 0.058, 0.0, 0.144),
                   color=STEEL_BRIGHT)
    return _center(a)


# ---------------------------------------------------------------------------
# 12. flare pack -- 0.25 wide x 0.16 tall x 0.28 deep, centred
# ---------------------------------------------------------------------------

def _flare_pack():
    """A bundle of six road flares, rubber-banded."""
    a = C.Asset("pickup_flare_pack", "pickup")

    tubes = a.part("flare_tubes", base_color=FLARE_RED, roughness=0.62)
    caps = a.part("flare_caps", base_color=FLARE_RED_DK, roughness=0.55)
    # Six tubes, 4 in the bottom row and 2 riding the crease of the two
    # middle ones -- the pile-up that makes a flat box read as cylinders.
    layout = ((-0.082, -0.046), (-0.006, -0.046), (0.070, -0.046),
              (-0.044, 0.004), (0.032, 0.004), (-0.004, 0.050))
    for i, (x, y) in enumerate(layout):
        r = 0.0235 if i < 4 else 0.0215
        z = -0.006 if i < 4 else 0.026
        C.cylinder(tubes.mesh, r, 0.176, 8, center=(x, y, z),
                   axis="Y", color=FLARE_RED)
        # Cap at the -Y end and a paper band near the middle.
        C.cylinder(caps.mesh, r * 1.03, 0.020, 8, center=(x, y - 0.082, z),
                   axis="Y", color=FLARE_RED_DK)
    for i, (x, y) in enumerate(layout):
        r = 0.0235 if i < 4 else 0.0215
        z = -0.006 if i < 4 else 0.026
        C.cylinder(caps.mesh, r * 1.04, 0.016, 8, center=(x, y + 0.030, z),
                   axis="Y", color=AMBER_STENCIL)
    # A printed label wrapped round the two front tubes, a brighter value
    # that separates them from the four behind.
    for (x, y) in layout[:2]:
        C.cylinder(tubes.mesh, 0.0240, 0.048, 8, center=(x, y + 0.058, -0.006),
                   axis="Y", color=AMBER_STENCIL)

    strap = a.part("flare_strap", base_color=(0.14, 0.15, 0.16), roughness=0.85)
    _cbox(strap.mesh, (0.246, 0.020, 0.128), center=(0.0, -0.014, 0.0),
          color=(0.14, 0.15, 0.16), bevel=0.005)                 # lower band
    _cbox(strap.mesh, (0.246, 0.020, 0.070), center=(0.0, -0.030, 0.050),
          color=(0.20, 0.21, 0.22), bevel=0.005)                 # upper band
    _cbox(strap.mesh, (0.030, 0.048, 0.012), center=(0.0, -0.030, 0.086),
          color=BAT_TAPE, bevel=0.003)                            # buckle
    _cbox(strap.mesh, (0.020, 0.056, 0.034), center=(0.0, -0.030, 0.086),
          color=STEEL_BRIGHT, bevel=0.003)                        # buckle frame
    return _center(a)


# ---------------------------------------------------------------------------
# 13. rocket round -- launcher ammunition, crate
# ---------------------------------------------------------------------------

def _rocket_round():
    """A finned projectile standing in a stencilled olive crate.

    A NEW asset id.  The engine will not place it until ``src/game.rs``
    grows a placement for it, which is exactly the point: it is ammunition
    for the launcher above, modelled to the same scale so the two read as a
    matched pair on the ground.
    """
    a = C.Asset("pickup_rocket_round", "pickup")

    crate = a.part("round_crate", base_color=OLIVE, roughness=0.68)
    # Open-topped crate: four walls and a floor, each a real slab, so the
    # projectile inside is visible from above as the pickup spins.
    _cbox(crate.mesh, (0.300, 0.300, 0.020), center=(0.0, 0.0, -0.080),
          color=C.shade(OLIVE, 0.82), bevel=0.005)                # floor
    for sx in (-1, 1):
        _cbox(crate.mesh, (0.018, 0.300, 0.170), center=(sx * 0.141, 0.0, -0.015),
              color=OLIVE, bevel=0.005)                           # side walls
        _cbox(crate.mesh, (0.300, 0.018, 0.170), center=(0.0, sx * 0.141, -0.015),
              color=OLIVE, bevel=0.005)                           # end walls
        # Corner posts and a top rail.  The posts sit on the DIAGONAL, not on
        # the axes: a post at (sx*0.132, sx*0.132) is 0.187 from the centre
        # diagonal, which is outside the 0.150 half-width and makes the crate
        # 0.61 m wide instead of the intended 0.32.  Clamped to the inside
        # corner, it is a 26 mm post flush with two walls.
        for sy in (-1, 1):
            _cbox(crate.mesh, (0.026, 0.026, 0.180),
                  center=(sx * 0.126, sy * 0.126, -0.010),
                  color=OLIVE_LT, bevel=0.004)
        _cbox(crate.mesh, (0.300, 0.026, 0.016), center=(0.0, sx * 0.134, 0.070),
              color=OLIVE_LT, bevel=0.004)                        # top rail
        _cbox(crate.mesh, (0.026, 0.300, 0.016), center=(sx * 0.134, 0.0, 0.070),
              color=OLIVE_LT, bevel=0.004)
    C.recolor_faces_where(
        crate, lambda n, c: c[2] < -0.100, C.shade(OLIVE, 0.70))

    # The round itself: nose cone, body tube, four fins, standing on the
    # crate floor and leaning slightly back so the nose is visible.
    body = a.part("round_body", base_color=OLIVE_DK, metallic=0.3,
                  roughness=0.55)
    C.cylinder(body.mesh, 0.046, 0.300, 12, center=(0.0, 0.0, 0.070),
               color=OLIVE_DK)                                    # body tube
    C.cone(body.mesh, 0.046, 0.110, 12, center=(0.0, 0.0, 0.275),
           radius_top=0.016, color=OLIVE_DK)                     # nose cone
    C.cylinder(body.mesh, 0.048, 0.016, 12, center=(0.0, 0.0, 0.228),
               color=C.shade(OLIVE_DK, 1.35))                   # joint ring
    C.cylinder(body.mesh, 0.048, 0.020, 12, center=(0.0, 0.0, -0.062),
               color=OLIVE_LT)                                    # tail ring
    for k in range(4):                                            # fins
        a2 = math.pi * 0.5 * k
        fx, fy = math.cos(a2), math.sin(a2)
        _cbox(body.mesh, (0.014, 0.110, 0.110),
              center=(fx * 0.080, fy * 0.080, -0.024),
              rot=(0.0, 0.0, math.degrees(a2)), color=OLIVE_LT, bevel=0.004)
    C.cylinder(body.mesh, 0.030, 0.024, 10, center=(0.0, 0.0, -0.082),
               color=BARREL_DK)                                   # nozzle

    trim = a.part("round_trim", base_color=AMBER_STENCIL, roughness=0.7)
    # Hazard bands straddle the crate's END walls.  They are struck ON the
    # wall at x = +/-0.144 (the wall's own inner face), not outside it: at
    # +/-0.301 they floated 150 mm clear of the crate and doubled its width
    # to 0.61 m, which is the width of a car door.
    for sx in (-1, 1):
        _cbox(trim.mesh, (0.006, 0.076, 0.076), center=(sx * 0.144, 0.0, 0.010),
              color=AMBER_STENCIL, bevel=0.003)
        _cbox(trim.mesh, (0.010, 0.140, 0.014), center=(sx * 0.146, 0.0, 0.010),
              color=C.shade(AMBER_STENCIL, 0.76), bevel=0.002)
    _cbox(trim.mesh, (0.180, 0.006, 0.050), center=(0.0, -0.142, -0.030),
          color=AMBER_STENCIL, bevel=0.003)                       # crate stencil
    _cbox(trim.mesh, (0.110, 0.006, 0.024), center=(0.0, -0.142, -0.064),
          color=AMBER_STENCIL, bevel=0.002)
    return _center(a)


# ---------------------------------------------------------------------------
# 14. grenade case -- a boxed set of frags
# ---------------------------------------------------------------------------

def _grenade_case():
    """A lidded field case of frags, a NEW id that pairs with ``wep_grenade``.

    Modelled at the same scale as the other carry pickups so a row of them
    along a kerb reads as one consistent set of loot.
    """
    a = C.Asset("pickup_grenade_case", "pickup")

    shell = a.part("case_shell", base_color=GREN_BODY, roughness=0.62)
    _cbox(shell.mesh, (0.260, 0.180, 0.140), color=GREN_BODY, bevel=0.012)
    _cbox(shell.mesh, (0.268, 0.188, 0.024), center=(0.0, 0.0, 0.058),
          color=GREN_BODY_DK, bevel=0.006)                        # lid
    _cbox(shell.mesh, (0.268, 0.190, 0.012), center=(0.0, 0.0, 0.040),
          color=GREN_BODY_DK, bevel=0.004)                        # clasp band
    _cbox(shell.mesh, (0.240, 0.166, 0.014), center=(0.0, 0.0, -0.074),
          color=GREN_BODY_DK, bevel=0.004)                        # base plinth
    # Moulded lid ribs, three across, so the top is not one dead value.
    for k in range(3):
        _cbox(shell.mesh, (0.200, 0.014, 0.008), center=(0.0, -0.048 + k * 0.048, 0.070),
              color=C.shade(GREN_BODY, 1.22), bevel=0.002)
    # Two grab handles on the ends.
    for sx in (-1, 1):
        _cbox(shell.mesh, (0.012, 0.060, 0.026), center=(sx * 0.132, 0.0, 0.010),
              color=C.shade(GREN_BODY, 0.78), bevel=0.003)

    trim = a.part("case_trim", base_color=AMBER_STENCIL, roughness=0.7)
    # Crossed stencil bars on the front: the one bright value, and the thing
    # that says "this is ordnance, do not sit on it" at 25 m.
    _cbox(trim.mesh, (0.130, 0.008, 0.014), center=(0.0, -0.092, 0.010),
          color=AMBER_STENCIL, bevel=0.002)
    _cbox(trim.mesh, (0.130, 0.008, 0.014), center=(0.0, -0.092, -0.014),
          color=AMBER_STENCIL, bevel=0.002)
    for sx in (-1, 1):
        _cbox(trim.mesh, (0.024, 0.010, 0.048), center=(sx * 0.098, -0.093, 0.026),
              color=AMBER_STENCIL, bevel=0.003)                   # lid clasps
    for sx in (-1, 1):
        _cbox(trim.mesh, (0.008, 0.070, 0.010), center=(sx * 0.076, -0.092, 0.010),
              color=C.shade(AMBER_STENCIL, 0.78), bevel=0.002)

    metal = a.part("case_metal", base_color=STEEL_BRIGHT, metallic=0.55,
                   roughness=0.38)
    for sx in (-1, 1):
        C.cylinder(metal.mesh, 0.007, 0.022, 8, center=(sx * 0.128, 0.060, 0.028),
                   axis="X", color=STEEL_BRIGHT)                 # hinges
    for sx in (-1, 1):
        _cbox(metal.mesh, (0.024, 0.024, 0.024), center=(sx * 0.112, -0.078, -0.068),
              color=STEEL_BRIGHT, bevel=0.004)                    # corner bumpers
    return _center(a)


# ---------------------------------------------------------------------------

def build_all():
    """Return the ordered list of weapon + pickup assets."""
    return [
        _pistol(),
        _smg(),
        _shotgun(),
        _rocket_launcher(),
        _bat(),
        _grenade(),
        _armor_vest(),
        _health_pack(),
        _ammo_box(),
        _cash_stack(),
        _o2_tank(),
        _flare_pack(),
        _rocket_round(),
        _grenade_case(),
    ]
