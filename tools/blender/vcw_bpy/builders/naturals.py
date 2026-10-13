"""3A naturals builder — palms, flowers, grass.

THE bug this module exists to avoid
-----------------------------------
A palm is not a cylinder with a green ball on top. The single thing that
separates a palm from a telephone pole is the **stem**, and the single thing
that separates a *modelled* stem from a *painted* one is that the taper and the
lean happen **per segment**:

    trunk = cone(height=H, radius1=r0, radius2=r1)      # ONE straight cylinder

That version passes every count-based check in the pipeline — right part count,
right triangle count, closed shell, outward winding, resting on z=0 — and reads
as a lamp post, because a real palm stem is (a) leaning on a progressive lateral
offset, so no two segments share a centre, and (b) tapering non-linearly, so the
silhouette is fattest at the ground. A pole has neither.

So every palm here is built as a **chain of short frustums placed on a sampled
curve**: `_trunk_curve` returns a point per segment, the frustum for segment *i*
is placed at that point with that orientation and overlapped into its
neighbours, and the radius at segment *i* comes from the taper function, not
from a single linear lerp. Straightness is then a property you can only get by
deliberately asking for it (`lean=0.0`).

The crown follows the same rule for a different reason. A radially identical
crown — every frond the same length, at the same elevation, evenly spaced —
reads as a mechanical part for exactly the same reason the stem does, so each
frond gets its own length, its own droop angle, its own base elevation and its
own azimuth jitter from a deterministic hash (`_hash01`, no global RNG state, so
two runs of the pipeline emit byte-identical geometry).

Ground contact is a construction rule, not a post-hoc nudge
-----------------------------------------------------------
Shifting an assembly down by its lowest vertex is the usual trick and it is
wrong here: it leaves the stem's lean measured from a moving origin, and it
silently accepts a trunk whose first segment was aimed a few degrees off
vertical. Instead every frustum and blade section is placed by `_aim`, which
puts the object's own **radius1 cap exactly on the supplied anchor point** and
extends it forward — so segment 0's bottom face lands on z=0 by construction,
and no frond or blade can dip below the ground by a fraction of its own length.

Fronds and blades are flattened, not round. A frond is a chain of squashed
ellipsoids whose local +X runs along the chord and whose local +Z is a thin
axis pulled toward world up, so the blade presents its face to the sky; a grass
blade is the same idea in bevelled plates, except its thin axis is *radial*
rather than vertical. That is deliberate: a near-vertical blade tracked against
world +Z is a degenerate frame, and the axis you fall back to turns half the
tuft edge-on to the camera at half the azimuths. Radially-thin plates fan
outward instead, which is also what bent grass actually does.

Triangle budget: a palm is 5k–8k, a nature asset 2.4k–4.9k. Both sit well
inside `verify3a.CATEGORY_BUDGET` (palm 30_000, nature 6_000) with headroom for
the crown to get fuller without touching the cap.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy
from mathutils import Matrix, Vector

from .. import kernel as K
from .. import palette as P

__all__ = ["PALMS", "NATURE", "build_palm", "build_flower_cluster", "build_grass_patch"]


# Every ground asset in this pipeline is built resting on z=0 in authoring space
# (the exporter rotates to Y-up afterwards); `verify3a` asserts it, so the
# builders assert it too rather than letting a bad base float a palm.
GROUND_TOLERANCE = 1e-6

# Ring count for any frustum that gets bevelled.
#
# `K.add_bevel` limits on `ANGLE` at 35 degrees, and an N-gon prism's side edges
# meet at 360/N degrees. At N=8 that is 45 degrees, ABOVE the limit, so the bevel
# wires every vertical edge as well as the cap rims and the triangle count
# roughly septuples. At N=12 the side edges are 30 degrees, below the limit, and
# only the genuinely hard 90-degree cap rims get chamfered — which is the point
# of bevelling. 12 is therefore the smallest ring that behaves.
BEVEL_SAFE_RING = 12


# --- deterministic variation ------------------------------------------------
#
# No `random`: the pipeline must emit the same library twice, and a global RNG
# whose seeding depends on import order cannot promise that.


def _hash01(i: int, salt: int = 0) -> float:
    """A deterministic pseudo-random float in [0, 1) from an integer pair.

    An integer avalanche rather than `hash()` (which is salted per process for
    strings) so the result is identical in every Blender run.
    """
    x = (i * 1103515245 + salt * 12345 + 0x9E3779B9) & 0x7FFFFFFF
    x ^= x >> 13
    x = (x * 1274126177) & 0x7FFFFFFF
    x ^= x >> 16
    return x / 0x7FFFFFFF


def _unit(v: Sequence[float] | Vector) -> Vector:
    u = Vector(v)
    if u.length < 1e-9:
        raise ValueError("cannot normalise a zero-length vector")
    return u.normalized()


# --- orientation ------------------------------------------------------------


def _frame(direction: Vector, flat_axis: Vector | None = None) -> Matrix:
    """An orthonormal frame whose local +X runs along `direction`.

    `mathutils`' `to_track_quat` degenerates when the tracked axis points along
    its own up reference (a near-vertical grass blade tracked against world +Z),
    so the frame is built explicitly: project the requested reference into the
    plane perpendicular to the direction, and take Y as Z cross X so the basis is
    right-handed (X cross Y == Z) and therefore a pure rotation, determinant +1.

    `flat_axis` is the world direction the returned local +Z should lean toward.
    It is what makes a frond face the sky.
    """
    x_axis = _unit(direction)

    ref = _unit(flat_axis) if flat_axis is not None else Vector((0.0, 0.0, 1.0))
    if abs(x_axis.dot(ref)) > 0.98:
        # Perpendicular fallback: the reference has collapsed onto the blade's
        # own axis, so pick any axis that has not.
        for candidate in (Vector((1.0, 0.0, 0.0)), Vector((0.0, 1.0, 0.0)),
                          Vector((1.0, 1.0, 0.0))):
            if abs(x_axis.dot(candidate)) < 0.98:
                ref = _unit(candidate)
                break

    z_axis = _unit(ref - x_axis * ref.dot(x_axis))
    y_axis = _unit(z_axis.cross(x_axis))
    return Matrix((
        (x_axis.x, y_axis.x, z_axis.x),
        (x_axis.y, y_axis.y, z_axis.y),
        (x_axis.z, y_axis.z, z_axis.z),
    ))


def _move(obj: bpy.types.Object, location: Sequence[float] | Vector) -> bpy.types.Object:
    """Translate to `location` and bake the translation into the mesh.

    The transform is baked rather than left on `matrix_world`: a helper that
    returns a transformed *copy* of immutable coordinates is a silent no-op, and
    this pipeline has been bitten by that before.
    """
    obj.location = location
    K.transform_apply(obj)
    return obj


def _place(obj: bpy.types.Object, direction: Vector, location: Sequence[float] | Vector,
           flat_axis: Vector | None = None) -> bpy.types.Object:
    """Orient `obj` along `direction` and put its ORIGIN at `location`, baked.

    Every part is constructed at (0, 0, 0) first, rotated about its own origin
    and only then translated. Setting world location and rotation together would
    spin the part around its own offset instead of around its own axis, which
    throws petals and fronds metres away.
    """
    obj.rotation_euler = _frame(direction, flat_axis).to_euler()
    K.transform_apply(obj)
    return _move(obj, location)


def _aim(obj: bpy.types.Object, direction: Vector, anchor: Vector, reach: float,
         flat_axis: Vector | None = None) -> bpy.types.Object:
    """Orient `obj` along `direction` and stretch it forward FROM `anchor`.

    `anchor` lands on the object's own **near cap** (radius1) and the geometry
    then runs `reach` further along `direction`.

    `K.cone`/`K.cylinder` build their mesh **centred on `location`** and then
    bake the transform, so the object's centre is its mid-depth, not its
    radius1 cap. Placing the centre at `anchor` therefore drives half the depth
    below the anchor — a palm root flare aimed up from z=0 sank 0.40 m, which
    is half of its 0.81 m height plus the tilt term. The centre must go at
    `anchor + d * reach/2`.
    """
    d = _unit(direction)
    # A Blender cone/cylinder already lies along its own local +Z. Aiming it
    # straight up must therefore be a NO-OP, not a re-orientation: `_frame`
    # would swing the local +Z onto +Z by mapping it onto a computed frame and
    # lay the part on its side, turning a 1.28 x 1.28 x 0.399 flare into
    # 0.399 x 1.28 x 1.28 and driving half of it below the anchor. Only rotate
    # when the direction genuinely differs from local +Z.
    if d.dot(Vector((0.0, 0.0, 1.0))) > 0.9999:
        return _move(obj, Vector(anchor) + d * (reach * 0.5))
    return _place(obj, d, Vector(anchor) + d * (reach * 0.5), flat_axis)


def _soften(obj: bpy.types.Object, angle: float = 35.0) -> bpy.types.Object:
    """Smooth-shade with a hard-edge threshold, AFTER the bevel is applied.

    Called last on purpose: `modifier_apply` on the "Smooth by Angle" node
    modifier is a per-version behaviour, whereas applying a BEVEL modifier is not
    a risk in any Blender release. So geometry is built flat, bevelled, and only
    then smoothed.
    """
    K.shade_smooth(obj, angle=math.radians(angle))
    return obj


def _assert_rests_on_zero(parts: Sequence[bpy.types.Object], asset_id: str) -> None:
    """Assert the whole assembly's lowest vertex sits on z=0.

    Position is the one bug class no volume, winding or triangle test catches, so
    it is checked explicitly rather than eyeballed in a preview.
    """
    lowest = None
    lowest_name = None
    for obj in parts:
        if obj is None or obj.type != "MESH":
            continue
        for v in obj.data.vertices:
            z = (obj.matrix_world @ v.co).z
            if lowest is None or z < lowest:
                lowest, lowest_name = z, obj.name
    if lowest is None:
        raise AssertionError(f"{asset_id}: builder returned no geometry")
    if abs(lowest) > GROUND_TOLERANCE:
        raise AssertionError(
            f"{asset_id}: lowest point is at z={lowest:.6f} on {lowest_name}, "
            f"expected 0 (tolerance {GROUND_TOLERANCE})"
        )


# --- trunk ------------------------------------------------------------------


def _trunk_radius(t: float, base_r: float, top_r: float) -> float:
    """Radius at normalised height `t`.

    `t ** 0.7` rather than a linear lerp: a palm stem loses most of its girth in
    the lower third, and a linear taper reads as a cone, which is a different
    plant and a different silhouette against the sky.
    """
    return base_r + (top_r - base_r) * (t ** 0.7)


def _trunk_curve(height: float, lean: float, lean_deg: float, bias: float,
                 segments: int) -> tuple[list[Vector], list[float]]:
    """Sample the stem's centreline; returns (points, normalised heights).

    The lateral offset is `lean * (bias*t + (1-bias)*t^2)` — a blend of a linear
    and a quadratic term, so the stem starts leaning immediately (a palm is
    pushed over by its own asymmetric crown) and keeps accelerating without the
    cubic's whiplash at the crown shaft. `lean=0.0` collapses the whole thing
    onto the world axis, which is the control case this module exists to be
    measured against.
    """
    if segments < 3:
        raise ValueError(f"trunk needs at least 3 segments, got {segments}")
    ang = math.radians(lean_deg)
    offset = Vector((math.cos(ang), math.sin(ang), 0.0))

    points = []
    ts = []
    for i in range(segments + 1):
        t = i / segments
        ts.append(t)
        points.append(offset * (lean * (bias * t + (1.0 - bias) * t * t))
                     + Vector((0.0, 0.0, height * t)))
    return points, ts


def _stem_frustum(asset_id: str, index: int, anchor: Vector, direction: Vector,
                  r0: float, r1: float, reach: float, mat,
                  bevel: float, bevel_segments: int) -> bpy.types.Object:
    """One tapering stem segment, chamfered, its radius1 cap on `anchor`.

    Always built longer than the gap it spans (the caller passes ~1.2x), so
    consecutive segments interpenetrate. Without that overlap the outside of
    every bend opens a wedge-shaped gap you can see straight through the trunk.
    """
    seg = K.cone(
        f"{asset_id}_trunk{index}",
        radius1=r0, radius2=r1, depth=reach, vertices=BEVEL_SAFE_RING,
        location=(0.0, 0.0, 0.0), material=mat, smooth=False,
    )
    _aim(seg, direction, anchor, reach)
    K.add_bevel(seg, width=bevel, segments=bevel_segments)
    _soften(seg, 38.0)
    return seg


def _frond(asset_id: str, index: int, origin: Vector, azimuth: float,
           length: float, elev0: float, droop: float, mat,
           widths: Sequence[float] = (0.082, 0.104, 0.052),
           seg: int = 9, rings: int = 5, tag: str = "frond") -> list[bpy.types.Object]:
    """One palm frond: three flattened ellipsoids on a drooping arc.

    `elev0` is the elevation in degrees above horizontal where the frond leaves
    the crown, and `droop` is how far it falls by the tip. The blade rises,
    flattens, then hangs — a frond modelled as a straight spike at one elevation
    is a leaf-shaped toothpick.

    Each section's local +X runs along its chord and its local +Z is thin and
    near-vertical, so every section presents its face to the sky. `widths` are
    fractions of the frond length, widest on the middle section: that is where a
    palm frond actually carries its blade mass.
    """
    parts: list[bpy.types.Object] = []
    out = Vector((math.cos(azimuth), math.sin(azimuth), 0.0))
    reach = (0.13, 0.52, 1.0)
    sky = Vector((0.0, 0.0, 1.0))

    prev = Vector((0.0, 0.0, 0.0))
    for j, (frac, elev) in enumerate(zip(reach, (elev0, elev0 - droop * 0.42, elev0 - droop))):
        e = math.radians(elev)
        along = Vector((out.x * math.cos(e), out.y * math.cos(e), math.sin(e))) * (length * frac)
        base = origin + prev
        tip = origin + along
        chord = tip - base
        if chord.length < 1e-5:
            raise ValueError(f"{asset_id} {tag}{index} section {j} is degenerate")

        w = widths[j] * length
        span = chord.length * 1.24      # overlaps forward into the next section
        blade = K.sphere(
            f"{asset_id}_{tag}{index}_s{j}",
            radius=1.0, segments=seg, ring_count=rings,
            location=(0.0, 0.0, 0.0), material=mat,
            scale=(span * 0.5, w, w * 0.22),
        )
        parts.append(_aim(blade, chord, base, span, flat_axis=sky))
        prev = along
    return parts


# ==========================================================================
# palms
# ==========================================================================


def build_palm(
    asset_id: str,
    *,
    height: float = 10.5,
    lean: float = 2.6,
    lean_deg: float = 25.0,
    lean_bias: float = 0.34,
    trunk_r0: float = 0.38,
    trunk_r1: float = 0.19,
    trunk_segments: int = 12,
    fronds: int = 9,
    frond_len: float = 3.2,
    frond_spread: float = 40.0,
    frond_droop: float = 62.0,
    lower_whorl: int = 0,
    lower_whorl_len: float = 1.6,
    skirt: int = 6,
    coconuts: int = 5,
    frond_rgb: Sequence[float] = (0.17, 0.40, 0.20),
    bark_rgb: Sequence[float] = (0.42, 0.33, 0.23),
    coconut_rgb: Sequence[float] = (0.30, 0.22, 0.14),
    seed: int = 0,
) -> list[bpy.types.Object]:
    """One palm: curved tapering stem, 7-11 drooping fronds, a crown and nuts.

    Z-up, metres, resting with the root flare's and the stem's bottom faces
    exactly on z=0.
    """
    K.reset_scene()

    if fronds < 7 or fronds > 11:
        raise ValueError(f"{asset_id}: a 3A palm carries 7-11 fronds, got {fronds}")

    frond_mat = P.foliage(f"{asset_id}_frond", frond_rgb)
    bark_mat = P.paint(f"{asset_id}_bark", bark_rgb, metallic=0.0, roughness=0.88)
    nut_mat = P.paint(f"{asset_id}_coconut", coconut_rgb, metallic=0.0, roughness=0.62)

    bevel_w = min(0.05, trunk_r1 * 0.20)
    parts: list[bpy.types.Object] = []

    points, ts = _trunk_curve(height, lean, lean_deg, lean_bias, trunk_segments)
    crown = points[-1]

    # --- stem ------------------------------------------------------------
    # Segment 0 is built VERTICAL from z=0 rather than following the curve.
    # Its height is derived from segment 1's lowest rim point, not guessed: a
    # frustum is a rigid body, so a tilted segment 1's radius1 cap dips below
    # its own anchor by r*sin(tilt), and segment 0 has to be tall enough to
    # swallow that dip or the trunk opens a gap at its first bend.
    chord1 = points[1] - points[0]
    tilt1 = math.acos(max(-1.0, min(1.0, _unit(chord1).dot(Vector((0.0, 0.0, 1.0))))))
    seg1_rim_low = points[1].z - _trunk_radius(ts[1], trunk_r0, trunk_r1) * math.sin(tilt1)
    stem0_reach = max(seg1_rim_low * 1.15, height * 0.02)

    parts.append(_stem_frustum(
        asset_id, 0, Vector((0.0, 0.0, 0.0)), Vector((0.0, 0.0, 1.0)),
        _trunk_radius(0.0, trunk_r0, trunk_r1),
        _trunk_radius(min(1.0, stem0_reach / height), trunk_r0, trunk_r1),
        stem0_reach, bark_mat, bevel_w, 2,
    ))

    for i in range(1, trunk_segments):
        parts.append(_stem_frustum(
            asset_id, i, points[i], points[i + 1] - points[i],
            _trunk_radius(ts[i], trunk_r0, trunk_r1),
            _trunk_radius(ts[i + 1], trunk_r0, trunk_r1),
            (points[i + 1] - points[i]).length * 1.20, bark_mat, bevel_w, 2,
        ))

    # Root flare: the swollen foot where the stem meets the ground. Its bottom
    # face is the asset's lowest point, so it is aimed straight up from z=0
    # rather than stacked on segment 0's footprint, where its own depth would
    # sink below the ground plane.
    flare_h = trunk_r0 * 0.95
    flare = K.cone(
        f"{asset_id}_root_flare",
        radius1=trunk_r0 * 1.52, radius2=trunk_r0 * 1.03, depth=flare_h,
        vertices=BEVEL_SAFE_RING, location=(0.0, 0.0, 0.0),
        material=bark_mat, smooth=False,
    )
    _aim(flare, Vector((0.0, 0.0, 1.0)), Vector((0.0, 0.0, 0.0)), flare_h)
    K.add_bevel(flare, width=bevel_w, segments=2)
    _soften(flare, 38.0)
    parts.append(flare)

    # --- crown -----------------------------------------------------------
    boss = K.sphere(
        f"{asset_id}_crown", radius=1.0, segments=14, ring_count=8,
        location=(0.0, 0.0, 0.0), material=bark_mat,
        scale=(trunk_r1 * 1.85, trunk_r1 * 1.85, trunk_r1 * 1.25),
    )
    _move(boss, crown + Vector((0.0, 0.0, trunk_r1 * 0.48)))
    parts.append(boss)

    frond_origin = crown + Vector((0.0, 0.0, trunk_r1 * 0.62))

    for i in range(fronds):
        azimuth = math.radians(
            i * (360.0 / fronds)
            + frond_spread * (_hash01(i * 5 + 1, seed) - 0.5)
            + seed * 3.0
        )
        length = frond_len * (0.74 + 0.48 * _hash01(i * 5 + 2, seed))
        droop = frond_droop * (0.62 + 0.74 * _hash01(i * 5 + 3, seed))
        elev0 = 34.0 + 20.0 * _hash01(i * 5 + 4, seed)
        parts += _frond(asset_id, i, frond_origin, azimuth, length, elev0, droop, frond_mat)

    # A second, lower whorl of short heavy fronds is what makes a bushy palm
    # read as bushy rather than as a tall palm shrunk down: the crown has mass
    # UNDER it, hanging past the top of the stem.
    whorl = max(1, lower_whorl)
    for j in range(lower_whorl):
        azimuth = math.radians(
            j * (360.0 / whorl) + 180.0 / whorl
            + frond_spread * 0.6 * (_hash01(j * 13 + 21, seed) - 0.5)
        )
        length = lower_whorl_len * (0.70 + 0.60 * _hash01(j * 13 + 22, seed))
        parts += _frond(
            asset_id, fronds + j,
            frond_origin - Vector((0.0, 0.0, trunk_r1 * 1.5)),
            azimuth, length,
            12.0 + 10.0 * _hash01(j * 13 + 23, seed),
            78.0 + 34.0 * _hash01(j * 13 + 24, seed),
            frond_mat, widths=(0.105, 0.130, 0.070), tag="skirtfrond",
        )

    # --- skirt -----------------------------------------------------------
    # Cut frond bases left hanging off the top of the stem. Without them the
    # crown meets the trunk as a clean socket, which is a golf tee.
    n_skirt = max(1, skirt)
    for i in range(skirt):
        azimuth = math.radians(
            i * (360.0 / n_skirt)
            + 24.0 * (_hash01(i * 7 + 5, seed) - 0.5)
            + seed * 5.0
        )
        drop_dir = _unit(Vector((math.cos(azimuth), math.sin(azimuth), -1.15)))
        stub_len = trunk_r1 * (2.4 + 1.3 * _hash01(i * 7 + 6, seed))
        anchor = crown - Vector((0.0, 0.0, trunk_r1 * (0.9 + 1.0 * _hash01(i * 7 + 7, seed))))
        stub = K.cone(
            f"{asset_id}_frondbase{i}",
            radius1=trunk_r1 * 0.60, radius2=trunk_r1 * 0.24, depth=stub_len,
            vertices=BEVEL_SAFE_RING, location=(0.0, 0.0, 0.0),
            material=bark_mat, smooth=False,
        )
        _aim(stub, drop_dir, anchor, stub_len)
        K.add_bevel(stub, width=min(0.02, trunk_r1 * 0.09), segments=1)
        _soften(stub, 38.0)
        parts.append(stub)

    # --- coconuts --------------------------------------------------------
    n_nuts = max(1, coconuts)
    for i in range(coconuts):
        azimuth = math.radians(
            i * (360.0 / n_nuts) + 42.0 * (_hash01(i * 11 + 8, seed) - 0.5)
        )
        ring_r = trunk_r1 * (1.20 + 0.60 * _hash01(i * 11 + 9, seed))
        drop_z = -trunk_r1 * (1.4 + 1.5 * _hash01(i * 11 + 10, seed))
        nut = K.sphere(
            f"{asset_id}_coconut{i}", radius=trunk_r1 * 0.46,
            segments=9, ring_count=6, location=(0.0, 0.0, 0.0), material=nut_mat,
        )
        _move(nut, crown + Vector((math.cos(azimuth) * ring_r,
                                   math.sin(azimuth) * ring_r, drop_z)))
        parts.append(nut)

    _assert_rests_on_zero(parts, asset_id)
    return P.finish(parts)


# ==========================================================================
# nature
# ==========================================================================


def build_flower_cluster(
    asset_id: str,
    *,
    blossoms: int = 14,
    petals: int = 5,
    height: float = 0.62,
    spread: float = 0.46,
    base_r: float = 0.030,
    petal_len: float = 0.055,
    foliage_rgb: Sequence[float] = (0.19, 0.40, 0.18),
    petal_rgbs: Sequence[Sequence[float]] = (
        (0.95, 0.36, 0.46), (0.97, 0.72, 0.30), (0.93, 0.52, 0.78), (0.99, 0.93, 0.72),
    ),
    stamen_rgb: Sequence[float] = (0.98, 0.84, 0.26),
    leaves: int = 7,
    seed: int = 0,
) -> list[bpy.types.Object]:
    """A clump of small blossoms on stems, over a low mound of foliage.

    Deliberately NOT one blob with a bump map: the asset is 14 separate
    five-petal blossoms plus 14 stems plus 7 leaves plus the mound, because at
    gameplay distance the read is "there are flowers here", and a single merged
    shape reads as a rock.

    Every leaf and stem is anchored ON the mound's elliptical surface (the
    height is solved from the mound's own radii, not assumed), and none of them
    may reach below z=0.
    """
    K.reset_scene()

    if blossoms < 6 or blossoms > 20:
        raise ValueError(f"{asset_id}: a flower cluster wants 6-20 blossoms, got {blossoms}")

    green = P.foliage(f"{asset_id}_leaf", foliage_rgb)
    stamen_mat = P.foliage(f"{asset_id}_stamen", stamen_rgb, rough=0.55)
    petal_mats = [
        P.foliage(f"{asset_id}_petal{i}", rgb, rough=0.62)
        for i, rgb in enumerate(petal_rgbs)
    ]

    parts: list[bpy.types.Object] = []

    mound_r = spread * 0.52
    mound_h = height * 0.22
    parts.append(_move(K.sphere(
        f"{asset_id}_mound", radius=1.0, segments=10, ring_count=6,
        location=(0.0, 0.0, 0.0), material=green,
        scale=(mound_r, mound_r * 0.92, mound_h),
    ), (0.0, 0.0, mound_h)))

    def surface_z(radius: float) -> float:
        """Height of the squashed-ellipsoid mound at ground radius `radius`."""
        en = min(1.0, radius / max(mound_r, 1e-6))
        return mound_h + mound_h * math.sqrt(max(0.0, 1.0 - en * en))

    # basal leaves, splayed flat and sitting on the mound
    n_leaves = max(1, leaves)
    for i in range(leaves):
        azimuth = math.radians(i * (360.0 / n_leaves)
                               + 24.0 * (_hash01(i * 3 + 1, seed) - 0.5))
        reach = mound_r * (0.55 + 0.40 * _hash01(i * 3 + 2, seed))
        elev = math.radians(3.0 + 20.0 * _hash01(i * 3 + 3, seed))
        out = Vector((math.cos(azimuth), math.sin(azimuth), 0.0))
        anchor = Vector((math.cos(azimuth) * reach, math.sin(azimuth) * reach, surface_z(reach)))
        leaf = K.sphere(
            f"{asset_id}_leaf{i}", radius=1.0, segments=7, ring_count=4,
            location=(0.0, 0.0, 0.0), material=green,
            scale=(reach * 0.5, spread * 0.072, spread * 0.015),
        )
        direction = out * math.cos(elev) + Vector((0.0, 0.0, 1.0)) * math.sin(elev)
        parts.append(_aim(leaf, direction, anchor, reach))

    for i in range(blossoms):
        azimuth = math.radians(
            i * (360.0 / blossoms)
            + 52.0 * (_hash01(i * 7 + 1, seed) - 0.5)
            + seed * 7.0
        )
        ring_r = mound_r * (0.10 + 0.86 * _hash01(i * 7 + 2, seed))
        lift = height * (0.52 + 0.50 * _hash01(i * 7 + 3, seed))
        head = Vector((math.cos(azimuth) * ring_r, math.sin(azimuth) * ring_r,
                       surface_z(ring_r) + lift))

        # stem: from just inside the mound up to the back of the blossom
        stem_r = max(0.006, base_r * (0.7 + 0.5 * _hash01(i * 7 + 4, seed)))
        root = Vector((math.cos(azimuth) * ring_r * 0.32,
                       math.sin(azimuth) * ring_r * 0.32, mound_h * 0.72))
        stem = K.cone(
            f"{asset_id}_stem{i}",
            radius1=stem_r * 1.35, radius2=stem_r * 0.8,
            depth=(head - root).length * 1.15, vertices=BEVEL_SAFE_RING,
            location=(0.0, 0.0, 0.0), material=green, smooth=False,
        )
        _place(stem, head - root, (head + root) * 0.5)
        K.add_bevel(stem, width=stem_r * 0.32, segments=1)
        _soften(stem, 38.0)
        parts.append(stem)

        mat = petal_mats[int(_hash01(i * 7 + 5, seed) * len(petal_mats)) % len(petal_mats)]

        parts.append(_move(K.sphere(
            f"{asset_id}_blossom{i}_centre", radius=petal_len * 0.30,
            segments=6, ring_count=3, location=(0.0, 0.0, 0.0), material=stamen_mat,
        ), head))

        for p in range(petals):
            # petals fan from near-vertical to nearly horizontal and are
            # staggered in roll, so the blossom has a cup, not a star silhouette
            pa = azimuth + (p / petals) * 360.0 + 24.0 * (_hash01(i * 17 + p, seed) - 0.5)
            tilt = math.radians(16.0 + 44.0 * (p / max(1, petals - 1))
                                + 10.0 * (_hash01(i * 17 + p + 3, seed) - 0.5))
            out = Vector((math.cos(pa) * math.cos(tilt),
                          math.sin(pa) * math.cos(tilt), math.sin(tilt)))
            petal = K.sphere(
                f"{asset_id}_blossom{i}_petal{p}", radius=1.0,
                segments=6, ring_count=3, location=(0.0, 0.0, 0.0), material=mat,
                scale=(petal_len * 0.62, petal_len * 0.30, petal_len * 0.11),
            )
            parts.append(_place(petal, out, head + out * (petal_len * 0.72)))

    _assert_rests_on_zero(parts, asset_id)
    return P.finish(parts)


def build_grass_patch(
    asset_id: str,
    *,
    blades: int = 12,
    sections: int = 3,
    height: float = 0.48,
    spread: float = 0.62,
    blade_w: float = 0.030,
    clumps: int = 8,
    foliage_rgb: Sequence[float] = (0.24, 0.46, 0.17),
    seed: int = 0,
) -> list[bpy.types.Object]:
    """A tuft of grass: 12 blades of 3 bevelled plates each, over a low mound.

    Each blade is a chain of thin plates whose tilt increases toward the tip, so
    the blade arcs instead of standing as a spike, and each plate's base is
    anchored on the previous plate's tip so the chain cannot sink below the
    mound surface.

    Every plate's thin axis is RADIAL (`flat_axis=outward`), not vertical. With
    a vertical thin axis a near-vertical blade is a degenerate track — the
    direction it points is the axis it is flattened along — and the fallback
    axis would flip between blades, so the tuft shows its edge at some azimuths
    and its face at others. Radially-thin plates present their faces outward
    all the way round, and fan the tuft the way bent grass actually lies.
    """
    K.reset_scene()

    if blades < 6 or blades > 20:
        raise ValueError(f"{asset_id}: a grass patch wants 6-20 blades, got {blades}")
    if sections < 2:
        raise ValueError(f"{asset_id}: a blade needs at least 2 sections, got {sections}")

    green = P.foliage(f"{asset_id}_blade", foliage_rgb)
    dark = P.foliage(f"{asset_id}_clump",
                     (foliage_rgb[0] * 0.7, foliage_rgb[1] * 0.8, foliage_rgb[2] * 0.7))
    parts: list[bpy.types.Object] = []

    mound_r = spread * 0.48
    mound_h = height * 0.16
    parts.append(_move(K.sphere(
        f"{asset_id}_mound", radius=1.0, segments=12, ring_count=6,
        location=(0.0, 0.0, 0.0), material=dark,
        scale=(mound_r, mound_r * 0.94, mound_h),
    ), (0.0, 0.0, mound_h)))

    def surface_z(radius: float) -> float:
        """Height of the squashed-ellipsoid mound at ground radius `radius`."""
        en = min(1.0, radius / max(mound_r, 1e-6))
        return mound_h * (1.0 + math.sqrt(max(0.0, 1.0 - en * en)))

    # basal tufts — small squashed spheres the blades visibly grow out of
    n_clumps = max(1, clumps)
    for c in range(clumps):
        azimuth = math.radians(c * (360.0 / n_clumps)
                               + 40.0 * (_hash01(c * 5 + 1, seed) - 0.5))
        rr = mound_r * (0.25 + 0.70 * _hash01(c * 5 + 2, seed))
        blob = K.sphere(
            f"{asset_id}_clump{c}",
            radius=blade_w * (1.1 + 0.7 * _hash01(c * 5 + 3, seed)),
            segments=6, ring_count=4, location=(0.0, 0.0, 0.0), material=dark,
            scale=(1.6, 1.6, 0.9),
        )
        parts.append(_move(blob, Vector((math.cos(azimuth) * rr, math.sin(azimuth) * rr,
                                         surface_z(rr) + blade_w * 0.2))))

    for i in range(blades):
        azimuth = math.radians(
            i * (360.0 / blades)
            + 58.0 * (_hash01(i * 7 + 1, seed) - 0.5)
            + seed * 11.0
        )
        rr = mound_r * (0.12 + 0.86 * _hash01(i * 7 + 2, seed))
        base = Vector((math.cos(azimuth) * rr, math.sin(azimuth) * rr, surface_z(rr)))

        length = height * (0.68 + 0.62 * _hash01(i * 7 + 3, seed))
        width = blade_w * (0.78 + 0.5 * _hash01(i * 7 + 4, seed))
        # the blade leaves the ground near-vertical and falls away toward the tip
        elev0 = math.radians(80.0 + 8.0 * _hash01(i * 7 + 5, seed))
        elev1 = math.radians(14.0 + 26.0 * _hash01(i * 7 + 6, seed))
        outward = Vector((math.cos(azimuth), math.sin(azimuth), 0.0))

        prev = Vector((0.0, 0.0, 0.0))
        for j in range(sections):
            t = 0.34 + 0.66 * ((j + 1) / sections)
            elev = elev0 + (elev1 - elev0) * t
            along = Vector((outward.x * math.cos(elev), outward.y * math.cos(elev),
                            math.sin(elev))) * (length * t)
            anchor = base + prev
            chord = along - prev
            if chord.length < 1e-5:
                raise ValueError(f"{asset_id} blade {i} section {j} is degenerate")

            # Sections overlap forward so the blade has no gaps, but the LAST
            # section must not: its taper factor (1.0 - 0.42*(j+1)) has gone
            # negative by then, and a cube whose Y size is negative is mirrored
            # through its own centre, which inverts every face and turns the tip
            # inside-out. Measured: all 12 blades' final section wound backwards,
            # each with a tiny negative signed volume.
            taper = max(0.18, 1.0 - 0.42 * (j + 1))
            span = chord.length * (1.00 if j == sections - 1 else 1.30)
            plate = K.cube(
                f"{asset_id}_blade{i}_s{j}",
                # a blade is `width * 0.20` thick, so the chamfer must stay
                # under half that or it eats the plate; Blender would clamp it
                # silently, leaving a chamfered-out blade rather than a chamfered
                # edge, which is not the same defect at a different size.
                size=(span, width * taper, width * 0.20),
                location=(0.0, 0.0, 0.0), material=green,
                bevel=width * 0.05, bevel_segments=1,
            )
            parts.append(_aim(plate, chord, anchor, span, flat_axis=outward))
            prev = along

    _assert_rests_on_zero(parts, asset_id)
    return P.finish(parts)


# ==========================================================================
# registry tables
# ==========================================================================
#
# `registry.all_assets` reads these as `(asset_id, builder, kwargs)` and calls
# `builder(asset_id, **kwargs)`, which is why the two lists sit at module level
# rather than being assembled at import time by a discovery scan.

PALMS: list[tuple[str, Callable, dict]] = [
    # 9.8 m of stem plus the crown arc = ~10.2 m overall, leaning hard toward
    # -X: the boulevard palm that sets the silhouette of the whole street.
    # `height` is the STEM, not the asset — the fronds add the rest above it.
    ("palm_tall", build_palm, dict(
        height=9.8, lean=2.7, lean_deg=192.0, lean_bias=0.32,
        trunk_r0=0.40, trunk_r1=0.19, trunk_segments=13,
        fronds=9, frond_len=3.35, frond_spread=42.0, frond_droop=64.0,
        skirt=6, coconuts=5, seed=1,
        frond_rgb=(0.15, 0.38, 0.19), bark_rgb=(0.44, 0.34, 0.24),
    )),
    # 4.0 m of stem, ~4.1 m overall, near upright and stouter: a median palm
    # you can see over a parked car. 7 short fronds, fewer coconuts, heavier
    # skirt than the tall palm.
    ("palm_short", build_palm, dict(
        height=4.0, lean=0.95, lean_deg=-40.0, lean_bias=0.46,
        trunk_r0=0.30, trunk_r1=0.17, trunk_segments=9,
        fronds=7, frond_len=2.15, frond_spread=52.0, frond_droop=54.0,
        skirt=7, coconuts=3, seed=2,
        frond_rgb=(0.21, 0.45, 0.22), bark_rgb=(0.38, 0.31, 0.24),
        coconut_rgb=(0.33, 0.25, 0.15),
    )),
    # 3.4 m of stem, ~3.5 m overall, only gently off plumb: a courtyard palm
    # with a second, lower whorl of heavy fronds hanging past the top of the
    # stem. 11 fronds plus that lower whorl is what "fuller crown" means in
    # geometry rather than in scale.
    ("palm_bushy", build_palm, dict(
        height=3.4, lean=0.75, lean_deg=155.0, lean_bias=0.52,
        trunk_r0=0.27, trunk_r1=0.16, trunk_segments=8,
        fronds=11, frond_len=2.05, frond_spread=64.0, frond_droop=70.0,
        lower_whorl=5, lower_whorl_len=1.75,
        skirt=7, coconuts=4, seed=3,
        frond_rgb=(0.18, 0.43, 0.20), bark_rgb=(0.35, 0.29, 0.22),
        coconut_rgb=(0.28, 0.21, 0.14),
    )),
]


NATURE: list[tuple[str, Callable, dict]] = [
    ("flower_cluster", build_flower_cluster, dict(
        blossoms=14, petals=5, height=0.62, spread=0.46,
        base_r=0.030, petal_len=0.055, leaves=7, seed=5,
        foliage_rgb=(0.19, 0.40, 0.18),
        petal_rgbs=((0.95, 0.36, 0.46), (0.97, 0.72, 0.30),
                    (0.93, 0.52, 0.78), (0.99, 0.93, 0.72)),
        stamen_rgb=(0.98, 0.84, 0.26),
    )),
    ("grass_patch", build_grass_patch, dict(
        blades=12, sections=3, height=0.48, spread=0.62,
        blade_w=0.030, clumps=8, seed=9, foliage_rgb=(0.24, 0.46, 0.17),
    )),
]