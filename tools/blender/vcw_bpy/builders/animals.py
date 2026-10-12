"""3A animal builder — organic quadrupeds, a perched bird and a swimming fish.

An animal is the one asset family where the old pipeline's approach fails
outright. Vehicles and buildings are manufactured: they *are* boxes with
chamfers, and a bevel plus a correct massing is genuinely most of what makes
them read. A creature is not manufactured — it is a soft volume that tapers,
slopes and overlaps. A chamfered box with ears glued on reads as a toy robot,
not a cat, at any triangle count.

So nothing here is a box. Every organic volume is one of three primitives:

* ``_ellipsoid`` — a UV sphere scaled to three radii. A smooth-shaded
  ellipsoid is a rounded, tapered, believable lump of anatomy: torso, chest,
  skull, shoulder mass.
* ``_segment`` — a tapered cone aimed from one joint to the next. A limb, a
  muzzle, an ear, a fin ray, a tail.
* ``_segment(..., thin=)`` — the same cone squashed in world X after the aim,
  which is what turns a round tube into a flat sheet: wings, tail fans,
  fish fins. Thin parts stay CLOSED solids, never open sheets, so the
  export outwardness check (signed volume > 0) applies to every single part
  in this module and nothing here needs an ``open_surface`` declaration.

Three rules this module exists to enforce:

1. **Aim at the origin, translate afterwards.** ``K.transform_apply`` bakes
   location as well as rotation, so rotating an object *after* it has been
   placed spins it around the world origin rather than around its own axis —
   a leg rotated after placement sweeps a metre away from the animal. Every
   rotation here happens while the part still sits at (0, 0, 0).
2. **Every part is separately addressable and independently pivotable.** The
   renderer animates legs, so the four legs are four objects named
   ``<asset>_leg_FL / FR / BL / BR`` and each gets its origin moved to its
   own geometry, so a limb swing pivots at the limb rather than at the world
   origin. Same for wings, ears and tail segments.
3. **Every tail/limb segment overlaps its neighbour.** Two cones that merely
   touch at one point show a seam and a hole at every grazing angle; overlap
   them by a few percent of their length and the union silhouette is
   continuous.

Coordinate contract: Z-up, **-Y is FRONT**, +X is right, metres, and the
whole asset rests with its lowest point on z = 0.

The renderer looks parts up by name (``animal_bird_head``, ``animal_bird_beak``
in the beak-direction regression), so the ``<asset_id>_`` prefix is load
bearing and every part carries it.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy

from .. import kernel as K
from .. import palette as P


# ---------------------------------------------------------------------------
# low-level shape helpers
# ---------------------------------------------------------------------------

# Triangle budgets, derived from the primitive formulas, so the numbers in the
# per-asset comments at the foot of each builder are checkable rather than
# hopeful.  uv_sphere(seg, ring) -> seg * (4 * ring - 6);
#  cone(verts, flat_tip=False) -> 4 * verts - 4;  flat_tip -> 2 * verts - 2.
#   (a cone frustum is `verts` side quads + two `verts`-gon caps; a cone with
#   radius2 = 0 has `verts` side triangles and no apex cap, and Blender's
#   cylinder/cone caps are n-gons until P.finish triangulates them).


def _direction_euler(dx: float, dy: float, dz: float) -> tuple[float, float, float]:
    """Euler XYZ that turns a +Z-aligned primitive onto the vector (dx, dy, dz).

    Blender composes XYZ eulers as Rz @ Ry @ Rx, so Rx tilts +Z into -Y and
    Rz then swings that into place.  Inverting that gives a = atan2(h, dz) for
    the tilt and c = atan2(dx, -dy) for the swing.  `a` is derived from the
    horizontal magnitude rather than acos(dz) because acos loses all
    precision near dz = +-1, which is exactly the head-on and straight-down
    cases (a beak, a pectoral fin).
    """
    h = math.hypot(dx, dy)
    if h < 1e-9:
        # Perfectly vertical: +Z stays up, +Z flips to -Z below the horizon.
        return (0.0 if dz >= 0.0 else math.pi, 0.0, 0.0)
    return (math.atan2(h, dz), 0.0, math.atan2(dx, -dy))


def _ellipsoid(
    name: str,
    centre: Sequence[float],
    radii: Sequence[float],
    material: bpy.types.Material | None = None,
    *,
    segments: int = 16,
    rings: int = 8,
    rotation: Sequence[float] | None = None,
) -> bpy.types.Object:
    """A smooth-shaded ellipsoid: the organic workhorse of this module.

    `radii` are half-extents, so `radii=(a, b, c)` spans exactly -a..+a,
    -b..+b, -c..+c around `centre` — which is what makes the z=0 contact math
    below exact rather than approximate.

    Rotation is about the ellipsoid's OWN centre: K.sphere bakes the scale
    into the mesh and leaves the object at the world origin, so the object
    origin *is* the ellipsoid centre at the moment rotation_euler is set.
    An optional rotation is therefore a tilt of the anatomy (a neck, a wing
    at a dihedral), not a spin about the world origin.
    """
    obj = K.sphere(
        name, radius=1.0, segments=segments, ring_count=rings,
        location=(0.0, 0.0, 0.0), material=material, scale=radii,
    )
    if rotation is not None:
        obj.rotation_euler = rotation
        K.transform_apply(obj)
    obj.location = (float(centre[0]), float(centre[1]), float(centre[2]))
    K.transform_apply(obj)
    K.shade_smooth(obj)
    return obj


def _segment(
    name: str,
    start: Sequence[float],
    end: Sequence[float],
    r_start: float,
    r_end: float,
    material: bpy.types.Material | None = None,
    *,
    vertices: int = 12,
    thin: float = 1.0,
) -> bpy.types.Object:
    """A tapered tube from `start` to `end`, smooth-shaded.

    `r_start` is the radius at `start`, `r_end` the radius at `end`; equal
    radii give a cylinder, r_end = 0 gives a cone point (an ear tip, a beak,
    a fin ray).

    `thin` < 1 squashes the result in world X *after* it has been aimed and
    placed, which is the only order that produces a fin: flattening in local
    space would squash it along the segment's own axis instead.
    """
    sx, sy, sz = float(start[0]), float(start[1]), float(start[2])
    ex, ey, ez = float(end[0]), float(end[1]), float(end[2])
    length = math.dist((sx, sy, sz), (ex, ey, ez))
    if length < 1e-6:
        raise ValueError(f"{name}: start and end are the same point")

    obj = K.cone(
        name, radius1=r_start, radius2=r_end, depth=length, vertices=vertices,
        location=(0.0, 0.0, 0.0), material=material, smooth=False,
    )
    # Aim at the origin first, THEN translate: see the module docstring.
    obj.rotation_euler = _direction_euler(ex - sx, ey - sy, ez - sz)
    K.transform_apply(obj)
    obj.location = ((sx + ex) * 0.5, (sy + ey) * 0.5, (sz + ez) * 0.5)
    K.transform_apply(obj)
    if thin != 1.0:
        obj.scale = (thin, 1.0, 1.0)
        K.transform_apply(obj)
    K.shade_smooth(obj)
    return obj


def _pivot_at_geometry(obj: bpy.types.Object) -> bpy.types.Object:
    """Move a part's origin to its own centre.

    Everything else in this module bakes its transform into the mesh, so
    every part would otherwise sit at the world origin — and a renderer that
    animates a leg by rotating the part would swing it around the animal's
    feet instead of around its hip. Applied to the parts that actually get
    animated (legs, wings, ears, tail segments).
    """
    K.set_origin_to_geometry(obj)
    return obj


def _coat(name: str, rgb, roughness: float = 0.78):
    """Fur / feather / scale as a shared palette layer.

    Deliberately routed through ``P.paint`` rather than a private material
    factory: fur is a rough dielectric, which is exactly what P.paint is,
    while P.foliage's two-sided leaf treatment is wrong for an animal and
    P.skin's 0.55 sheen reads as bare hide on something that is meant to be
    covered in hair.
    """
    return P.paint(name, rgb, metallic=0.0, roughness=roughness)


def _eye(name: str, rgb, strength: float = 5.0):
    """Eyes are the one genuinely emissive organ on any of these animals."""
    return P.emissive(name, rgb, strength=strength)


# ---------------------------------------------------------------------------
# 1. animal_bird — perched tropical bird, ~0.29 m tall, 0.23 m folded span
# ---------------------------------------------------------------------------


def build_bird(
    asset_id: str,
    *,
    body_rgb: Sequence[float] = (0.16, 0.70, 0.76),
    breast_rgb: Sequence[float] = (0.98, 0.56, 0.40),
    wing_rgb: Sequence[float] = (0.08, 0.48, 0.60),
    tail_rgb: Sequence[float] = (0.98, 0.82, 0.32),
    beak_rgb: Sequence[float] = (0.98, 0.74, 0.22),
    leg_rgb: Sequence[float] = (0.92, 0.68, 0.34),
    eye_rgb: Sequence[float] = (1.0, 0.86, 0.28),
    segments: int = 16,
) -> list[bpy.types.Object]:
    """A perched bird. Body, head, beak, two wings, tail, two legs.

    The beak is aimed along -Y at a constant height, and that is load
    bearing rather than cosmetic: `cone` is Z-aligned by default, so a beak
    built without an aim stands up on the head like a spike. The renderer
    asserts the beak's forward reach and that its horizontal span is under
    half its length — write the section backwards and it is a vertical spike
    that still pokes past the face by enough to pass a naive "did it move"
    check.

    The legs are two thin shanks standing on two flat feet whose z-radius
    equals their own height, so the lowest vertex of each foot lands on
    z = 0 exactly.
    """
    K.reset_scene()

    body_mat = _coat(f"{asset_id}_body", body_rgb, roughness=0.66)
    breast_mat = _coat(f"{asset_id}_breast", breast_rgb, roughness=0.68)
    wing_mat = _coat(f"{asset_id}_wing", wing_rgb, roughness=0.62)
    tail_mat = _coat(f"{asset_id}_tail", tail_rgb, roughness=0.64)
    beak_mat = _coat(f"{asset_id}_beak", beak_rgb, roughness=0.34)
    leg_mat = _coat(f"{asset_id}_leg", leg_rgb, roughness=0.48)
    eye_mat = _eye(f"{asset_id}_eye", eye_rgb)

    parts: list[bpy.types.Object] = []

    # --- torso, breast, head ------------------------------------------------
    body = _ellipsoid(
        f"{asset_id}_body", (0.0, 0.005, 0.155), (0.072, 0.115, 0.082),
        body_mat, segments=segments, rings=9,
    )
    parts.append(body)

    # The breast is a deliberately proud ellipsoid, not a recoloured patch on
    # the body: a colour split needs a real surface to sit on or the bird is
    # one flat colour from every angle. Its rear reaches y = -0.004, well
    # inside the torso, and it is tall enough (z 0.070..0.210) to rise past
    # the head's underside at z = 0.183 — a bird with a gap between its chest
    # and its chin reads as two stacked balls.
    breast = _ellipsoid(
        f"{asset_id}_breast", (0.0, -0.062, 0.140), (0.058, 0.060, 0.070),
        breast_mat, segments=12, rings=7,
    )
    parts.append(breast)

    head = _ellipsoid(
        f"{asset_id}_head", (0.0, -0.092, 0.235), (0.055, 0.055, 0.052),
        body_mat, segments=14, rings=8,
    )
    parts.append(head)

    # --- beak: horizontal, slender, rooted INSIDE the head -------------------
    # Base at y = -0.142 sits inside the head (which spans y -0.150..-0.040),
    # so the root never floats in front of the face. The base radius is small
    # relative to the length on purpose: the renderer asserts the beak's
    # horizontal span is under half its length, and a fat stub by 0.028 m of
    # that margin is a beak that fails the check one tweak away from passing.
    beak = _segment(
        f"{asset_id}_beak", (0.0, -0.142, 0.226), (0.0, -0.247, 0.226),
        0.014, 0.0, beak_mat, vertices=12,
    )
    parts.append(beak)

    # --- eyes ----------------------------------------------------------------
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        eye = _ellipsoid(
            f"{asset_id}_eye_{tag}", (sx * 0.036, -0.128, 0.248),
            (0.011, 0.011, 0.011), eye_mat, segments=8, rings=4,
        )
        parts.append(eye)

    # --- wings: flattened, folded along the flanks --------------------------
    # Flattened ellipsoids rather than flat sheets, so they stay closed
    # solids and pass the signed-volume outwardness test.
    #
    # Wing x-offset is 0.078 against a 0.072 torso half-width and a 0.024 wing
    # half-thickness: the wing's inner face lands at x = 0.054, which is 18 mm
    # INSIDE the torso. At the 0.086 used before, the wing only grazed the
    # body and a real overlap probe measured 0.4 % of the wing inside the
    # torso — a seam that opens visibly in a wing flap.
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        wing = _ellipsoid(
            f"{asset_id}_wing_{tag}", (sx * 0.078, 0.022, 0.170),
            (0.024, 0.128, 0.072), wing_mat,
            segments=12, rings=7,
            rotation=(math.radians(-14.0), 0.0, math.radians(sx * 8.0)),
        )
        _pivot_at_geometry(wing)
        parts.append(wing)

    # --- tail ----------------------------------------------------------------
    tail = _ellipsoid(
        f"{asset_id}_tail", (0.0, 0.150, 0.150), (0.050, 0.090, 0.020),
        tail_mat, segments=12, rings=6, rotation=(math.radians(8.0), 0.0, 0.0),
    )
    parts.append(tail)

    # --- legs and feet -------------------------------------------------------
    # foot_z_radius == foot centre height, so the lowest vertex is exactly
    # z = 0; the shank then rises into the belly (body underside z = 0.073).
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        foot = _ellipsoid(
            f"{asset_id}_foot_{tag}", (sx * 0.030, 0.012, 0.012),
            (0.022, 0.034, 0.012), leg_mat, segments=8, rings=5,
        )
        shank = _segment(
            f"{asset_id}_leg_{tag}", (sx * 0.030, 0.020, 0.020),
            (sx * 0.030, 0.006, 0.090), 0.010, 0.0075, leg_mat, vertices=8,
        )
        _pivot_at_geometry(shank)
        parts += [foot, shank]

    # ~2.3k tris: torso 480, breast 264, head 364, beak 22, eyes 160,
    # wings 528, tail 216, shanks 56, feet 224.
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 2. animal_cat — ~0.45 m at the shoulder, ~0.78 m nose to rump
# ---------------------------------------------------------------------------


def build_cat(
    asset_id: str,
    *,
    fur_rgb: Sequence[float] = (0.78, 0.52, 0.26),
    belly_rgb: Sequence[float] = (0.96, 0.90, 0.80),
    point_rgb: Sequence[float] = (0.42, 0.26, 0.14),
    nose_rgb: Sequence[float] = (0.94, 0.52, 0.56),
    eye_rgb: Sequence[float] = (0.55, 0.95, 0.35),
    shoulder: float = 0.45,
    segments: int = 16,
) -> list[bpy.types.Object]:
    """A cat: head, muzzle, two ears, body, four legs, tail.

    `shoulder` is the withers height and sets the torso's vertical placement
    directly — the torso centre is at `shoulder - half_depth`, so the back
    lands on the declared height whatever the girth.

    Recognisable-ness comes from three separate decisions rather than from
    the triangle count: the legs are thin and roughly half the shoulder
    height (a cat on stilt-leg proportions reads as a ferret), the head is a
    near-sphere with a short muzzle set low and forward, and the ears are
    flat upright cones rather than side-mounted lobes. The tail is a three
    segment curl that overlaps its own base, so it reads as continuous.

    Legs are named `<asset>_leg_FL / FR / BL / BR`: FL is -X (left, since +X
    is right) and -Y (front). Left/right pairs are mirrored about X and the
    front/rear pairs about Y, so a left-to-right or front-to-back copy of the
    limb phase actually moves the correct legs.
    """
    K.reset_scene()

    fur = _coat(f"{asset_id}_fur", fur_rgb, roughness=0.80)
    belly = _coat(f"{asset_id}_belly", belly_rgb, roughness=0.82)
    point = _coat(f"{asset_id}_point", point_rgb, roughness=0.76)
    nose_mat = _coat(f"{asset_id}_nose", nose_rgb, roughness=0.42)
    eye_mat = _eye(f"{asset_id}_eye", eye_rgb)

    half_depth = 0.22 * shoulder          # 0.099 at the default 0.45 m
    torso_z = shoulder - half_depth       # back tops out exactly on `shoulder`
    # The shank root sits ABOVE the belly line (torso_z - half_depth), by
    # half the torso depth, so the upper third of every leg is genuinely
    # inside the torso mass rather than terminating on its skin.
    leg_top = shoulder - half_depth * 0.5

    parts: list[bpy.types.Object] = []

    # --- torso mass ----------------------------------------------------------
    torso = _ellipsoid(
        f"{asset_id}_body", (0.0, 0.020, torso_z), (0.115, 0.300, half_depth),
        fur, segments=segments, rings=9,
    )
    parts.append(torso)

    chest = _ellipsoid(
        f"{asset_id}_chest", (0.0, -0.140, torso_z - 0.005),
        (0.105, 0.155, 0.105), fur, segments=14, rings=8,
    )
    parts.append(chest)

    # A chest ruff in the pale colour: the one place a cat's underside colour
    # is visible head-on, and without it a ginger cat is one flat orange blob.
    # Its y is pushed back to -0.240 so it buries its rear inside the chest
    # rather than resting tangent to the chest's front pole.
    ruff = _ellipsoid(
        f"{asset_id}_ruff", (0.0, -0.240, torso_z + 0.005),
        (0.085, 0.070, 0.090), belly, segments=12, rings=7,
    )
    parts.append(ruff)

    # --- neck and head -------------------------------------------------------
    # The neck is moved back to y = -0.285 so its rear third is inside the
    # chest mass, and the head to y = -0.372 so the skull's rear is inside
    # the neck. A neck that starts at the chest's exact surface is a seam
    # across the most-looked-at part of the animal.
    neck = _ellipsoid(
        f"{asset_id}_neck", (0.0, -0.285, torso_z + 0.045),
        (0.078, 0.095, 0.075), fur, segments=12, rings=7,
        rotation=(math.radians(-18.0), 0.0, 0.0),
    )
    parts.append(neck)

    head = _ellipsoid(
        f"{asset_id}_head", (0.0, -0.372, torso_z + 0.085),
        (0.085, 0.085, 0.075), fur, segments=14, rings=8,
    )
    parts.append(head)

    # Muzzle: low, short and forward of the skull centre. A muzzle centred on
    # the head reads as a snout poking out sideways; a cat's is set down and
    # forward, under the eye line.
    muzzle = _ellipsoid(
        f"{asset_id}_muzzle", (0.0, -0.438, torso_z + 0.055),
        (0.040, 0.046, 0.032), belly, segments=10, rings=6,
    )
    parts.append(muzzle)

    # The nose pads are set back so they sit ON the muzzle's front rather than
    # floating a millimetre in front of it: at y -0.474 the pad's rear is at
    # -0.463, inside a muzzle whose front is at -0.484.
    nose = _ellipsoid(
        f"{asset_id}_nose", (0.0, -0.470, torso_z + 0.060),
        (0.013, 0.016, 0.011), nose_mat, segments=8, rings=4,
    )
    parts.append(nose)

    # --- ears: upright cones, splayed out and raked back -------------------
    # Root is low and deep inside the skull (z +0.115 against a 0.075 head
    # half-height, x 0.048 against a 0.085 half-width), so only the upper
    # half of the cone clears the fur. A cone rooted at the skull's crown
    # sits on top of the head instead of growing out of it.
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        ear = _segment(
            f"{asset_id}_ear_{tag}",
            (sx * 0.044, -0.360, torso_z + 0.115),
            (sx * 0.070, -0.340, torso_z + 0.212),
            0.034, 0.0, fur, vertices=10,
        )
        _pivot_at_geometry(ear)
        parts.append(ear)

    # --- eyes ----------------------------------------------------------------
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        eye = _ellipsoid(
            f"{asset_id}_eye_{tag}", (sx * 0.056, -0.432, torso_z + 0.108),
            (0.015, 0.013, 0.015), eye_mat, segments=8, rings=4,
        )
        parts.append(eye)

    # --- tail: three overlapping segments curling up and back ---------------
    # Each segment's start is pulled BACK toward the previous joint by 0.035,
    # so consecutive cones interpenetrate instead of meeting on a disc. A
    # shared disc is a hairline crack at grazing angles and it opens as the
    # tail curls; overlapping them keeps the union silhouette continuous.
    tail_path = (
        ((0.0, 0.255, torso_z + 0.040), (0.0, 0.400, torso_z + 0.155), 0.032, 0.024),
        ((0.0, 0.365, torso_z + 0.130), (0.0, 0.520, torso_z + 0.255), 0.026, 0.016),
        ((0.0, 0.488, torso_z + 0.232), (0.0, 0.578, torso_z + 0.318), 0.018, 0.004),
    )
    for i, (a, b, ra, rb) in enumerate(tail_path):
        seg = _segment(f"{asset_id}_tail{i}", a, b, ra, rb, fur, vertices=12)
        _pivot_at_geometry(seg)
        parts.append(seg)

    # --- four legs ------------------------------------------------------------
    # Paw centre height == paw z-radius, so the lowest vertex of every paw is
    # exactly z = 0 and the cat stands on all four feet.
    #
    # leg_top is the torso's mid-depth, not its underside: the shank root sits
    # well inside the belly mass. Ending the shank at the belly's skin means
    # the leg and body share a surface, and the probe measured zero shared
    # volume — the hip seam is exactly where a walking cat's leg pivots.
    paw_rz = 0.030
    for tag, sx, sy, foot_r in (
        ("FL", -1.0, -0.155, (0.036, 0.048, paw_rz)),
        ("FR", 1.0, -0.155, (0.036, 0.048, paw_rz)),
        ("BL", -1.0, 0.195, (0.038, 0.050, paw_rz)),
        ("BR", 1.0, 0.195, (0.038, 0.050, paw_rz)),
    ):
        limb = _segment(
            f"{asset_id}_leg_{tag}", (sx * 0.072, sy, leg_top),
            (sx * 0.072, sy, paw_rz + 0.018), 0.038, 0.028, fur, vertices=12,
        )
        _pivot_at_geometry(limb)
        parts.append(limb)
        parts.append(
            _ellipsoid(
                f"{asset_id}_paw_{tag}", (sx * 0.072, sy - 0.012, paw_rz),
                foot_r, point, segments=8, rings=5,
            )
        )

    # ~2.9k tris: torso 480, chest 364, ruff 264, neck 264, head 364,
    # muzzle 180, nose 80, ears 36, eyes 160, tail 110, legs 176, paws 448.
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 3. animal_dog — ~0.60 m at the shoulder, ~1.05 m nose to rump
# ---------------------------------------------------------------------------


def build_dog(
    asset_id: str,
    *,
    fur_rgb: Sequence[float] = (0.76, 0.55, 0.34),
    chest_rgb: Sequence[float] = (0.95, 0.89, 0.78),
    muzzle_rgb: Sequence[float] = (0.30, 0.22, 0.17),
    nose_rgb: Sequence[float] = (0.14, 0.12, 0.12),
    eye_rgb: Sequence[float] = (0.98, 0.72, 0.25),
    shoulder: float = 0.60,
    segments: int = 16,
) -> list[bpy.types.Object]:
    """A dog: head, muzzle, two drop ears, body, four legs, tail.

    Differentiated from the cat in the places the silhouette actually
    distinguishes them, not by scale alone: a deeper chest and a separate
    hindquarter mass, a head that is longer than it is tall, a long heavy
    muzzle with a defined jaw underneath it, and drop ears that hang down and
    forward instead of standing up.

    Ear and muzzle dimensions are what make a "dog" read as a dog; a scaled-up
    cat with the same torso is a cat. Grounding is by paw, with the paw's
    z-radius equal to its centre height, exactly as in build_cat.
    """
    K.reset_scene()

    fur = _coat(f"{asset_id}_fur", fur_rgb, roughness=0.80)
    chest_mat = _coat(f"{asset_id}_chest", chest_rgb, roughness=0.82)
    muzzle_mat = _coat(f"{asset_id}_muzzle", muzzle_rgb, roughness=0.62)
    nose_mat = _coat(f"{asset_id}_nose", nose_rgb, roughness=0.30)
    eye_mat = _eye(f"{asset_id}_eye", eye_rgb)

    half_depth = 0.175 * shoulder         # 0.105 at the default 0.60 m
    torso_z = shoulder - half_depth
    # Same rule as the cat: root the shank well inside the torso, not on its
    # skin, or the hip is a seam right where the leg pivots.
    leg_top = shoulder - half_depth * 0.5

    parts: list[bpy.types.Object] = []

    # --- torso mass: deep chest, separate hindquarters ----------------------
    torso = _ellipsoid(
        f"{asset_id}_body", (0.0, 0.030, torso_z), (0.135, 0.355, half_depth),
        fur, segments=segments, rings=9,
    )
    parts.append(torso)

    chest = _ellipsoid(
        f"{asset_id}_chest", (0.0, -0.180, torso_z - 0.010),
        (0.128, 0.185, 0.105), fur, segments=14, rings=8,
    )
    parts.append(chest)

    # The hindquarter mass is what stops a dog reading as a stretched cat —
    # without it the rump tapers to a point.
    haunch = _ellipsoid(
        f"{asset_id}_haunch", (0.0, 0.255, torso_z - 0.017),
        (0.145, 0.175, 0.118), fur, segments=14, rings=8,
    )
    parts.append(haunch)

    ruff = _ellipsoid(
        f"{asset_id}_chest_bib", (0.0, -0.300, torso_z - 0.015),
        (0.098, 0.060, 0.088), chest_mat, segments=12, rings=7,
    )
    parts.append(ruff)

    # --- neck, head, muzzle --------------------------------------------------
    neck = _ellipsoid(
        f"{asset_id}_neck", (0.0, -0.355, torso_z + 0.050),
        (0.105, 0.110, 0.098), fur, segments=12, rings=7,
        rotation=(math.radians(-12.0), 0.0, 0.0),
    )
    parts.append(neck)

    # Longer than tall: that ratio is the single strongest cat-vs-dog cue.
    head = _ellipsoid(
        f"{asset_id}_head", (0.0, -0.455, torso_z + 0.065),
        (0.100, 0.098, 0.088), fur, segments=14, rings=8,
        rotation=(math.radians(-8.0), 0.0, 0.0),
    )
    parts.append(head)

    muzzle = _ellipsoid(
        f"{asset_id}_muzzle", (0.0, -0.555, torso_z + 0.023),
        (0.052, 0.075, 0.046), muzzle_mat, segments=12, rings=7,
        rotation=(math.radians(-10.0), 0.0, 0.0),
    )
    parts.append(muzzle)

    # A jaw under the muzzle. Without it the muzzle floats on the head as a
    # tube with daylight under it.
    jaw = _ellipsoid(
        f"{asset_id}_jaw", (0.0, -0.540, torso_z - 0.015),
        (0.046, 0.062, 0.028), muzzle_mat, segments=10, rings=6,
    )
    parts.append(jaw)

    # Set back so the pad's rear is inside the muzzle, whose front is at -0.630.
    nose = _ellipsoid(
        f"{asset_id}_nose", (0.0, -0.616, torso_z + 0.050),
        (0.021, 0.024, 0.017), nose_mat, segments=8, rings=4,
    )
    parts.append(nose)

    # --- drop ears: flattened cones hanging down and forward ---------------
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        ear = _segment(
            f"{asset_id}_ear_{tag}",
            (sx * 0.092, -0.392, torso_z + 0.110),
            (sx * 0.108, -0.448, torso_z - 0.015),
            0.046, 0.010, fur, vertices=10, thin=0.42,
        )
        _pivot_at_geometry(ear)
        parts.append(ear)

    # --- eyes ----------------------------------------------------------------
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        eye = _ellipsoid(
            f"{asset_id}_eye_{tag}", (sx * 0.060, -0.522, torso_z + 0.102),
            (0.017, 0.014, 0.017), eye_mat, segments=8, rings=4,
        )
        parts.append(eye)

    # --- tail: carried up, curling over the back ---------------------------
    # Rooted at y = 0.330, which is inside the haunch (front face y = 0.080,
    # rear face y = 0.430), so the tail emerges from the rump mass instead of
    # starting on its surface. Consecutive segments overlap by ~40 mm.
    tail_path = (
        ((0.0, 0.330, torso_z + 0.060), (0.0, 0.520, torso_z + 0.025), 0.038, 0.026),
        ((0.0, 0.488, torso_z + 0.038), (0.0, 0.645, torso_z + 0.080), 0.028, 0.016),
        ((0.0, 0.615, torso_z + 0.066), (0.0, 0.700, torso_z + 0.160), 0.018, 0.005),
    )
    for i, (a, b, ra, rb) in enumerate(tail_path):
        seg = _segment(f"{asset_id}_tail{i}", a, b, ra, rb, fur, vertices=12)
        _pivot_at_geometry(seg)
        parts.append(seg)

    # --- four legs ------------------------------------------------------------
    paw_rz = 0.034
    for tag, sx, sy in (
        ("FL", -1.0, -0.200),
        ("FR", 1.0, -0.200),
        ("BL", -1.0, 0.270),
        ("BR", 1.0, 0.270),
    ):
        limb = _segment(
            f"{asset_id}_leg_{tag}", (sx * 0.086, sy, leg_top),
            (sx * 0.086, sy, paw_rz + 0.018), 0.042, 0.032, fur, vertices=12,
        )
        _pivot_at_geometry(limb)
        parts.append(limb)
        parts.append(
            _ellipsoid(
                f"{asset_id}_paw_{tag}", (sx * 0.086, sy - 0.014, paw_rz),
                (0.044, 0.058, paw_rz), muzzle_mat, segments=8, rings=5,
            )
        )

    # ~3.3k tris: torso 480, chest 364, haunch 364, ruff 264, neck 264,
    # head 364, muzzle 264, jaw 180, nose 80, ears 72, eyes 160, tail 110,
    # legs 176, paws 448.
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 4. animal_fish — ~0.55 m, the only horizontal asset in the family
# ---------------------------------------------------------------------------


def build_fish(
    asset_id: str,
    *,
    body_rgb: Sequence[float] = (0.18, 0.62, 0.80),
    belly_rgb: Sequence[float] = (0.90, 0.95, 0.92),
    fin_rgb: Sequence[float] = (0.98, 0.55, 0.30),
    eye_rgb: Sequence[float] = (1.0, 0.94, 0.35),
    segments: int = 22,
) -> list[bpy.types.Object]:
    """A fish: a closed fusiform body, a forked tail fin, a dorsal fin.

    The fish is the one asset in this family that lies along -Y instead of
    standing on it. That makes it the natural place for an open horizontal
    form, but it is NOT used here: the fins are flattened cones, which are
    closed solids. A fish built from flat single-sided sheets needs an
    `open_surface` outward-reference declaration, and a sheet wound the wrong
    way is invisible in a render from the one side you always screenshot
    from — only the export check catches it. Closed fins need no declaration
    and cannot be inside-out.

    Grounding: the body's belly sits at z = 0.054 and the lower lobe of the
    caudal fin is aimed at exactly z = 0, so the fish rests on the point of
    its tail. Every other part — pectorals, anal, dorsal, head — is kept
    clear of z = 0 so that single contact is the asset's lowest point.
    """
    K.reset_scene()

    body_mat = _coat(f"{asset_id}_body", body_rgb, roughness=0.36)
    belly_mat = _coat(f"{asset_id}_belly", belly_rgb, roughness=0.34)
    fin_mat = _coat(f"{asset_id}_fin", fin_rgb, roughness=0.40)
    eye_mat = _eye(f"{asset_id}_eye", eye_rgb)

    axis_z = 0.130
    parts: list[bpy.types.Object] = []

    # --- body: fusiform, blunt at the head, tapering to the tail stock -----
    body = _ellipsoid(
        f"{asset_id}_body", (0.0, 0.0, axis_z), (0.052, 0.205, 0.076),
        body_mat, segments=segments, rings=10,
    )
    parts.append(body)

    head = _ellipsoid(
        f"{asset_id}_head", (0.0, -0.125, axis_z - 0.010), (0.048, 0.095, 0.062),
        body_mat, segments=14, rings=8,
    )
    parts.append(head)

    # Pale underside, proud of the belly by a few millimetres so it is a
    # surface rather than a coplanar colour change.
    belly = _ellipsoid(
        f"{asset_id}_belly", (0.0, -0.020, 0.075), (0.040, 0.165, 0.030),
        belly_mat, segments=12, rings=6,
    )
    parts.append(belly)

    # Tail stock: the narrow waist between body and caudal fin. Without it the
    # body ellipsoid's own taper is too gradual and the fish reads as a lozenge.
    peduncle = _segment(
        f"{asset_id}_peduncle", (0.0, 0.155, axis_z), (0.0, 0.255, axis_z),
        0.032, 0.017, body_mat, vertices=12,
    )
    parts.append(peduncle)

    # --- caudal fin: two lobes splayed about the body axis ------------------
    # The lower lobe apex is the asset's lowest point, at z = 0 exactly.
    for tag, tip_z in (("lower", 0.0), ("upper", 0.262)):
        lobe = _segment(
            f"{asset_id}_fin_caudal_{tag}",
            (0.0, 0.250, axis_z), (0.0, 0.395, tip_z),
            0.020, 0.0, fin_mat, vertices=12, thin=0.35,
        )
        _pivot_at_geometry(lobe)
        parts.append(lobe)

    # --- dorsal fin ----------------------------------------------------------
    dorsal = _segment(
        f"{asset_id}_fin_dorsal",
        (0.0, 0.030, axis_z + 0.070), (0.0, 0.105, axis_z + 0.160),
        0.030, 0.0, fin_mat, vertices=12, thin=0.32,
    )
    parts.append(dorsal)

    # --- anal fin ------------------------------------------------------------
    anal = _segment(
        f"{asset_id}_fin_anal",
        (0.0, 0.120, axis_z - 0.055), (0.0, 0.185, 0.008),
        0.022, 0.0, fin_mat, vertices=10, thin=0.30,
    )
    parts.append(anal)

    # --- pectoral fins --------------------------------------------------------
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        pectoral = _segment(
            f"{asset_id}_fin_pectoral_{tag}",
            (sx * 0.048, -0.075, axis_z - 0.020),
            (sx * 0.098, -0.005, 0.055),
            0.024, 0.0, fin_mat, vertices=10, thin=0.30,
        )
        _pivot_at_geometry(pectoral)
        parts.append(pectoral)

    # --- eyes ------------------------------------------------------------------
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        eye = _ellipsoid(
            f"{asset_id}_eye_{tag}", (sx * 0.036, -0.170, axis_z + 0.030),
            (0.013, 0.013, 0.013), eye_mat, segments=8, rings=4,
        )
        parts.append(eye)

    # --- gill plates -----------------------------------------------------------
    # Shallow discs standing proud on the flank, just behind the head. They are
    # what stops a smooth fish body reading as a chrome lozenge: the gill line
    # is the single feature that says "fish" rather than "tube".
    for sx, tag in ((-1.0, "L"), (1.0, "R")):
        gill = _ellipsoid(
            f"{asset_id}_gill_{tag}", (sx * 0.044, -0.070, axis_z - 0.008),
            (0.012, 0.040, 0.044), fin_mat, segments=10, rings=6,
        )
        parts.append(gill)

    # --- fin rays on the caudal fin --------------------------------------------
    # A forked tail built from two smooth cones reads as a solid wedge. Thin
    # rays fanning out past the lobes give it the ragged edge a real caudal fin
    # has, and they are what makes the fork read at gameplay distance.
    for i, (dx, dz) in enumerate(((-0.030, -0.012), (0.0, 0.006), (0.030, -0.012))):
        ray = _segment(
            f"{asset_id}_fin_caudal_ray{i}",
            (0.0, 0.262, axis_z + dz * 0.4),
            (dx, 0.408, axis_z + dz * 3.4),
            0.009, 0.0, fin_mat, vertices=8, thin=0.30,
        )
        parts.append(ray)

    # ~2.1k tris: body 748, head 364, belly 216, peduncle 44, caudal 44,
    # dorsal 22, anal 18, pectorals 36, eyes 160, gills 216, rays 66.
    return P.finish(parts)


# ---------------------------------------------------------------------------
# registry entries — (asset_id, builder_callable, kwargs)
# ---------------------------------------------------------------------------

ANIMALS: list[tuple[str, Callable, dict]] = [
    ("animal_bird", build_bird, dict()),
    ("animal_cat", build_cat, dict()),
    ("animal_dog", build_dog, dict()),
    ("animal_fish", build_fish, dict()),
]