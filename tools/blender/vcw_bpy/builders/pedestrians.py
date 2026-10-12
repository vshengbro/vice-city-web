"""3A pedestrian builder — four distinctly-dressed city characters.

Part names are an API contract, not a label
--------------------------------------------
`src/game.rs` splits `ped_suit` into `PLAYER_PARTS` and looks each entry up by
EXACT string (`push_player_part` returns `usize::MAX` on a miss), and
`src/player.rs` then derives every joint pivot from that part's own bounding
box:

    joint_pivot(arm or leg) -> [min_x, max_y, mid_z]
    joint_pivot(shoe)       -> [mid_x, max_y, mid_z]

Three consequences, and every one of them is a silent failure — the asset still
builds, still exports, still passes every shape check:

1. Each of those names must exist, exactly once, as its OWN object. One merged
   body would export as a single part named `torso`, every limb lookup would
   miss, and the player would render as an un-animated statue.
2. Because the pivot is read off the bounds, the *shape* of each part is part
   of the contract too. Rotating an arm raises its outer top corner above the
   real shoulder and drags the swing axis with it; leaning a leg drops the hip.
   So limbs are authored vertical, and any outward set is applied by `_lean`,
   a shear that leaves every vertex on the top plane exactly where the joint is.
3. **Only `PLAYER_PARTS` is drawn for the player** — anything outside that list
   is not pushed as a batch. That is why the neck is folded into the `torso`
   object instead of being modelled separately: a standalone `neck` would leave
   the head floating with a gap at the collar. It is also why garment extras
   (skirt, bib, hood, tie) are safe but the player renders in a plain
   silhouette, exactly as the shipped library did.

Nothing else may hang off a limb. `LIMB_PLAN` registers just the eight limbs
plus the two shoes, so an extra object bolted to an arm is parented to nothing
and would hang in mid-air while the arm swung. Torso and head extras never move
relative to the character's root, so those are free.

LEFT IS +X
----------
Authoring is Z-up with +X right and -Y forward, and the character faces -Y, so
the figure's LEFT hand is at POSITIVE X — the same convention the shipped
library used (`upper_arm_L` at x +0.156..+0.302). Every `(side, tag)` pair
below is therefore `((+1.0, "L"), (-1.0, "R"))`. Flipping this silently
mirrors the whole rig's left/right gait phase, which is the one bug here that
survives every other check.

Origin: soles at z = 0, crown at ~1.75 m, origin at the feet. Export rotates
`(x, z, -y)` to Y-up, which puts the soles at y = 0 in the game's space.
"""

from __future__ import annotations

import math
from typing import Sequence

import bpy

from .. import kernel as K
from .. import palette as P


# (+X is the character's LEFT, see the module docstring.)
SIDES: tuple[tuple[float, str], ...] = ((1.0, "L"), (-1.0, "R"))


# ---------------------------------------------------------------------------
# mesh shaping — one object in, one object out, no joins anywhere
# ---------------------------------------------------------------------------
#
# `K.cube` bakes its transform into the mesh, so an object's origin always
# ends up back at the world origin. That makes post-hoc shaping the only way to
# get a taper or a lean, and it is why these helpers scale about each part's
# OWN bounds centre rather than about (0, 0): a leg centred at x = +0.11 must
# taper about x = +0.11, or the whole limb walks inwards as it thins.


def _scale_at(stations: Sequence[tuple], z: float) -> tuple[float, float]:
    """The (sx, sy) profile scale at height `z`, clamped at both ends.

    A vertex sitting exactly on a shared station boundary belongs to ONE
    band. Matching it against every band in turn applies the factor twice —
    and applying it twice is not idempotent, so that vertex lands at s^2 and
    punches a dent into the silhouette exactly on every profile seam.
    """
    if z <= stations[0][0]:
        return stations[0][1], stations[0][2]
    if z >= stations[-1][0]:
        return stations[-1][1], stations[-1][2]
    for (z0, sx0, sy0), (z1, sx1, sy1) in zip(stations, stations[1:]):
        if z0 <= z <= z1:
            span = z1 - z0
            if abs(span) < 1e-9:
                return sx1, sy1
            t = (z - z0) / span
            return sx0 + (sx1 - sx0) * t, sy0 + (sy1 - sy0) * t
    return stations[-1][1], stations[-1][2]


def _profile_z(obj: bpy.types.Object, stations: Sequence[tuple[float, float, float]]) -> bpy.types.Object:
    """Scale a part's x/y cross-section piecewise-linearly along its height.

    `stations` is `[(z, sx, sy), ...]` in ascending z. This is what turns a
    box into a waist-and-chest torso, or a tapering thigh, instead of a slab;
    separate x and y factors are what let the same object taper into a round
    neck at the top while still being flat-fronted at the chest.

    Only x and y move, so the part's z extent — and therefore the `max_y` the
    renderer's `joint_pivot` reads for arms and legs — is untouched.
    """
    verts = obj.data.vertices
    if not verts:
        return obj
    cx = (min(v.co.x for v in verts) + max(v.co.x for v in verts)) * 0.5
    cy = (min(v.co.y for v in verts) + max(v.co.y for v in verts)) * 0.5
    for v in verts:
        sx, sy = _scale_at(stations, v.co.z)
        v.co.x = cx + (v.co.x - cx) * sx
        v.co.y = cy + (v.co.y - cy) * sy
    obj.data.update()
    return obj


def _profile_y(obj: bpy.types.Object, stations: Sequence[tuple[float, float, float]]) -> bpy.types.Object:
    """Scale a part's x/z cross-section along its depth — a shoe's toe taper.

    The same idea as `_profile_z` rotated onto the Y axis. Stations run from
    the toe (-Y, forward) to the heel (+Y).
    """
    verts = obj.data.vertices
    if not verts:
        return obj
    cx = (min(v.co.x for v in verts) + max(v.co.x for v in verts)) * 0.5
    cz = (min(v.co.z for v in verts) + max(v.co.z for v in verts)) * 0.5
    for v in verts:
        sx, sz = _scale_at(stations, v.co.y)
        v.co.x = cx + (v.co.x - cx) * sx
        v.co.z = cz + (v.co.z - cz) * sz
    obj.data.update()
    return obj


def _drop_to(objs: Sequence[bpy.types.Object], limit: float) -> None:
    """Push a group of objects down until none exceeds `limit` in z.

    The ~1.75 m silhouette top is load-bearing: the acceptance probe in
    `src/config/const.rs` measures `ped_suit` against it, and anything that
    pokes above it silently grows the player. A swept quiff or a top-knot is
    exactly that kind of overshoot, so every head extra is clamped rather than
    trusted. Shifting whole objects keeps the extras intact — a head with no
    quiff simply does not move, because its top is already under the limit.
    """
    tops = [
        (obj, max(v.co.z for v in obj.data.vertices))
        for obj in objs
        if obj is not None and obj.data.vertices
    ]
    if not tops:
        return
    highest = max(z for _, z in tops)
    delta = highest - limit
    if delta <= 0.0:
        return
    for obj, _ in tops:
        for v in obj.data.vertices:
            v.co.z -= delta
        obj.data.update()


def _lean(obj: bpy.types.Object, z_ref: float, tan_lean: float) -> bpy.types.Object:
    """Shear x by height, about a plane at `z_ref`, with the top held still.

    A rotation would be the obvious choice and it is the wrong one here: the
    renderer takes an arm's shoulder pivot from the TOP of the bounding box, and
    rotating an outward-leaning arm lifts its outer top corner above the real
    joint, putting the swing axis in mid-air. A shear shifts every vertex on
    the top plane by exactly zero, so `max_y` is unchanged and the pivot stays
    on the shoulder.

    Sign follows `SIDES`: +tan leans the bottom towards +X (the left arm), so
    pass `tan(radians(angle)) * side`. `_joint_x` integrates the same shear so
    the next segment starts exactly where this one ends.
    """
    for v in obj.data.vertices:
        v.co.x += (z_ref - v.co.z) * tan_lean
    obj.data.update()
    return obj


def _placed(
    name: str,
    size: Sequence[float],
    location: Sequence[float],
    material: bpy.types.Material,
    *,
    rotation: Sequence[float] = (0.0, 0.0, 0.0),
    bevel: float = 0.014,
    segments: int = 2,
) -> bpy.types.Object:
    """A bevelled box rotated at the world origin, THEN moved into place.

    `K.cube` bakes its location, so the object's origin is (0, 0, 0) on
    return; assigning `rotation_euler` at that point spins the box around the
    character's feet instead of around itself, throwing a shoulder strap metres
    underground. Build at the origin, rotate there, then translate — the order
    the vehicle wheel spokes use.
    """
    obj = K.cube(name, size=size, location=(0.0, 0.0, 0.0), material=material,
                 bevel=bevel, bevel_segments=segments)
    if any(rotation):
        obj.rotation_euler = tuple(rotation)
        K.transform_apply(obj)
    obj.location = tuple(location)
    K.transform_apply(obj)
    return obj


def _limb(
    name: str,
    x: float,
    z_low: float,
    z_high: float,
    half_w: float,
    depth: float,
    material: bpy.types.Material,
    profile: Sequence[tuple[float, float]],
    *,
    lean: float = 0.0,
    bevel: float = 0.030,
    segments: int = 2,
) -> bpy.types.Object:
    """One arm or leg segment as its own bevelled, tapered, sheared object.

    `x` is the centre of the TOP face (the joint); the shear carries the bottom
    face out to `x ± (z_high - z_low) * tan(lean)`.

    The bevel is clamped to a fraction of the thinnest axis: a fixed 3 cm
    chamfer on a 7.2 cm-wide forearm is not a chamfer, it is a collapsed mesh,
    and Blender silently eats it.
    """
    width = half_w * 2.0
    chamfer = min(bevel, min(width, depth) * 0.30)
    obj = K.cube(name, size=(width, depth, z_high - z_low),
                 location=(x, 0.0, (z_low + z_high) * 0.5),
                 material=material, bevel=chamfer, bevel_segments=segments)
    _profile_z(obj, [(z0, s, s) for z0, s in profile])
    if lean:
        _lean(obj, z_high, lean)
    K.shade_smooth(obj, angle=math.radians(35.0))
    return obj


def _joint_x(m: dict) -> dict:
    """Every joint's |x| in the sheared pose, integrated from the root outward.

    `_lean` shifts the bottom of a limb by `(z_top - z_bot) * tan(lean)`, so a
    segment's lower joint is NOT at the same x as its upper joint. Hardcoding
    `elbow_x`/`knee_x`/`ankle_x` as independent numbers is how a chain ends up
    with a visible step at the elbow or knee — the two boxes stop sharing a
    face. Integrating the same shear the geometry uses is the only way the two
    stay in step.
    """
    arm = math.tan(math.radians(m["arm_lean_deg"]))
    fore = math.tan(math.radians(m["arm_lean_deg"] * 0.45))
    leg = math.tan(math.radians(m["leg_lean_deg"]))
    shin = math.tan(math.radians(m["leg_lean_deg"] * 0.4))

    elbow_x = m["shoulder_x"] + (m["shoulder_z"] - m["elbow_z"]) * arm
    knee_x = m["hip_x"] + (m["hip_z"] - m["knee_z"]) * leg
    return {
        "shoulder_x": m["shoulder_x"],
        "elbow_x": elbow_x,
        "wrist_x": elbow_x + (m["elbow_z"] - m["wrist_z"]) * fore,
        "hip_x": m["hip_x"],
        "knee_x": knee_x,
        "ankle_x": knee_x + (m["knee_z"] - m["ankle_z"]) * shin,
    }


# ---------------------------------------------------------------------------
# head group — `head`, `hair` and the face, all static relative to the root
# ---------------------------------------------------------------------------


def _head_group(m: dict, skin: bpy.types.Material, hair: bpy.types.Material,
                eye: bpy.types.Material):
    """Skull, face features and the hair cap. Returns (parts, hair_object)."""
    r = m["head_r"]
    sy, sz = m["head_y"], m["head_z_scale"]
    hz = m["head_z"]

    head = K.sphere("head", radius=r, segments=32, ring_count=20,
                    location=(0.0, 0.0, hz), material=skin,
                    scale=(1.0, sy, sz))
    # Narrow the jaw, leave the crown alone. The first station sits AT the
    # sphere's lowest point so the chin actually tapers — one station above it
    # leaves a full-width ring sitting on the neck, reading as a jaw jutting
    # out at the one place a jaw is expected to be narrow.
    _profile_z(head, [(hz - r * sz, 0.62, 0.62),
                      (hz - r * sz * 0.38, 0.92, 0.92),
                      (hz + r * sz * 0.30, 1.0, 1.0),
                      (hz + r * sz, 1.0, 1.0)])

    parts: list[bpy.types.Object] = [head]
    front = -r * sy

    # A nose wedge, a dark eye band, brows and ears: a few dozen triangles that
    # stop the head being a blank ovoid at gameplay distance.
    parts.append(_placed("nose", (r * 0.30, r * 0.34, r * 0.34),
                         (0.0, front + r * 0.08, hz - r * 0.12), skin,
                         bevel=r * 0.09, segments=2))
    parts.append(_placed("eye_band", (r * 1.02, r * 0.22, r * 0.20),
                         (0.0, front + r * 0.14, hz + r * 0.10), eye,
                         bevel=r * 0.05))
    for side, tag in SIDES:
        parts.append(_placed(f"brow_{tag}", (r * 0.38, r * 0.16, r * 0.12),
                             (side * r * 0.30, front + r * 0.18, hz + r * 0.30),
                             hair,
                             rotation=(0.0, side * math.radians(7.0), 0.0),
                             bevel=r * 0.04))
        parts.append(_placed(f"ear_{tag}", (r * 0.13, r * 0.26, r * 0.30),
                             (side * r * 0.97, r * 0.02, hz - r * 0.10), skin,
                             bevel=r * 0.05))

    # Hair cap: set BACK and slightly wider than the skull, so its front sits
    # behind the brow and the head reads as hair-over-skull rather than a
    # helmet with the features painted on. Top pinned 2 mm under the crown.
    crown = hz + r * sz
    hair_top = crown - 0.002
    cap = K.sphere("hair", radius=r * 1.085, segments=28, ring_count=14,
                   location=(0.0, r * 0.10, hair_top - r * 1.085 * m["hair_z"]),
                   material=hair, scale=(1.0, m["hair_y"], m["hair_z"]))
    return parts, cap


def _hair_style(m: dict, hair: bpy.types.Material) -> list[bpy.types.Object]:
    """The per-character hair marker. Head extras, so they never need to swing."""
    r = m["head_r"]
    hz = m["head_z"]
    out: list[bpy.types.Object] = []
    style = m["hair_style"]

    if style == "side_part":
        # A quiff raked back off the brow — most of the difference between
        # "suit" and "civilian" at 20 m. Its top is clamped to the crown below.
        out.append(_placed("quiff", (r * 1.30, r * 0.90, r * 0.34),
                           (r * 0.30, -r * 0.30, hz + r * 0.92), hair,
                           rotation=(math.radians(-16.0), math.radians(9.0), 0.0),
                           bevel=r * 0.11))
    elif style == "bob":
        # Jaw-length side falls plus a back panel: the bob silhouette.
        for side, tag in SIDES:
            out.append(_placed(f"bob_{tag}", (r * 0.16, r * 0.66, r * 0.52),
                               (side * r * 0.90, r * 0.04, hz - r * 0.18), hair,
                               bevel=r * 0.06))
        out.append(_placed("bob_back", (r * 1.34, r * 0.34, r * 1.24),
                           (0.0, r * 0.74, hz - r * 0.30), hair,
                           bevel=r * 0.16))
    elif style == "crop":
        out.append(_placed("fringe", (r * 1.30, r * 0.30, r * 0.20),
                           (0.0, -r * 0.74, hz + r * 0.30), hair,
                           bevel=r * 0.08))
    return out


def _cap(m: dict, cap_mat: bpy.types.Material, band: bpy.types.Material,
         *, backwards: bool) -> list[bpy.types.Object]:
    """A flat cap as its own objects — head extras, so they are free to exist."""
    r = m["head_r"]
    top = m["head_z"] + r * m["head_z_scale"] - 0.012
    dome = K.sphere("cap", radius=r * 1.10, segments=24, ring_count=10,
                    location=(0.0, 0.0, top - r * 1.10 * 0.58),
                    material=cap_mat, scale=(1.0, 1.0, 0.58))
    sign = 1.0 if backwards else -1.0
    return [
        dome,
        _placed("cap_peak", (r * 1.66, r * 1.46, r * 0.20),
                (0.0, sign * r * 1.34, top - r * 0.88), cap_mat,
                bevel=r * 0.08),
        _placed("cap_band", (r * 1.82, r * 1.70, r * 0.14),
                (0.0, sign * r * 1.32, top - r * 0.78), band,
                bevel=r * 0.06),
    ]


# ---------------------------------------------------------------------------
# the shared figure — the 13 parts the renderer addresses, built the same way
# for every character so a change to the rig fixes all four at once
# ---------------------------------------------------------------------------


def _figure(asset_id: str, m: dict, pal: dict) -> list[bpy.types.Object]:
    """Build the 13 parts the renderer addresses, plus the character's extras."""
    skin = P.skin(f"{asset_id}_skin", pal["skin"])
    top = P.fabric(f"{asset_id}_garment", pal["garment"], rough=0.84)
    bottom = P.fabric(f"{asset_id}_trousers", pal["trousers"], rough=0.88)
    accent = P.paint(f"{asset_id}_accent", pal["accent"], metallic=0.0, roughness=0.62)
    shoe_mat = P.paint(f"{asset_id}_shoe", pal["shoe"], metallic=0.0, roughness=0.38)
    hair_mat = P.fabric(f"{asset_id}_hair", pal["hair"], rough=0.74)
    eye = P.paint(f"{asset_id}_eye", pal["eye"], metallic=0.0, roughness=0.35)
    extra_mats = {
        key: P.paint(f"{asset_id}_{key}", rgb, metallic=0.0, roughness=0.58)
        for key, rgb in pal["extras"].items()
    }

    m = dict(m)
    m.update(_joint_x(m))
    parts: list[bpy.types.Object] = []
    arm_mat = skin if m["bare_arms"] else top
    leg_mat = skin if m["bare_legs"] else bottom

    # --- head -------------------------------------------------------------
    head_parts, hair_obj = _head_group(m, skin, hair_mat, eye)
    parts += head_parts
    parts.append(hair_obj)
    # The hair extras are clamped to the skull crown so the silhouette top is
    # the head's, at ~1.75 m, whatever marker style sits on it.
    hair_style = _hair_style(m, hair_mat)
    _drop_to(hair_style, m["head_z"] + m["head_r"] * m["head_z_scale"] - 0.002)
    parts += hair_style

    # --- torso, with the neck folded in ----------------------------------
    # One object, because `neck` is not in PLAYER_PARTS: modelled separately
    # the player's head would float above a gap at the collar. Separate x/y
    # profile factors are what let the same box be broad at the chest and
    # round at the throat.
    half_w = m["torso_half"]
    depth = m["torso_depth"]
    z0 = m["torso_z"][0]
    z1 = m["neck_z1"]
    neck_sx = (m["neck_r"] * 2.0) / (half_w * 2.0)
    neck_sy = (m["neck_r"] * 2.0) / depth
    torso = K.cube("torso", size=(half_w * 2.0, depth, z1 - z0),
                   location=(0.0, 0.0, (z0 + z1) * 0.5), material=top,
                   bevel=0.048, bevel_segments=3)
    _profile_z(torso, [
        (z0, 1.02, 1.02),
        (m["waist_z"], 0.90, 0.94),
        (m["chest_z"], 1.00, 1.00),
        (m["shoulder_z"], 1.02, 1.00),
        (m["neck_z0"] + 0.010, neck_sx * 1.30, neck_sy * 1.18),
        (m["neck_z0"] + 0.058, neck_sx, neck_sy),
        (z1, neck_sx, neck_sy),
    ])
    K.shade_smooth(torso, angle=math.radians(36.0))
    parts.append(torso)

    # --- arms -------------------------------------------------------------
    # Vertical, tapered, sheared. `joint_pivot` takes the shoulder from the
    # `max_y` of these bounds, so the top plane must stay on the shoulder.
    arm_lean = math.tan(math.radians(m["arm_lean_deg"]))
    fore_lean = math.tan(math.radians(m["arm_lean_deg"] * 0.45))
    for side, tag in SIDES:
        parts.append(_limb(
            f"upper_arm_{tag}",
            x=side * m["shoulder_x"], z_low=m["elbow_z"], z_high=m["shoulder_z"],
            half_w=m["upper_arm_half"], depth=m["arm_depth"], material=arm_mat,
            profile=[(m["elbow_z"], 0.74), (m["shoulder_z"], 1.0)],
            lean=arm_lean * side,
        ))
        parts.append(_limb(
            f"lower_arm_{tag}",
            x=side * m["elbow_x"], z_low=m["wrist_z"], z_high=m["elbow_z"] + 0.008,
            half_w=m["lower_arm_half"], depth=m["forearm_depth"], material=arm_mat,
            profile=[(m["wrist_z"], 0.62), (m["elbow_z"], 1.0)],
            lean=fore_lean * side,
        ))

    # --- legs -------------------------------------------------------------
    leg_lean = math.tan(math.radians(m["leg_lean_deg"]))
    shin_lean = math.tan(math.radians(m["leg_lean_deg"] * 0.4))
    for side, tag in SIDES:
        parts.append(_limb(
            f"upper_leg_{tag}",
            x=side * m["hip_x"], z_low=m["knee_z"], z_high=m["hip_z"],
            half_w=m["upper_leg_half"], depth=m["thigh_depth"], material=leg_mat,
            profile=[(m["knee_z"], 0.80), (m["hip_z"] - 0.10, 1.0), (m["hip_z"], 0.94)],
            lean=leg_lean * side, bevel=0.034, segments=3,
        ))
        parts.append(_limb(
            f"lower_leg_{tag}",
            x=side * m["knee_x"], z_low=m["ankle_z"], z_high=m["knee_z"] + 0.006,
            half_w=m["lower_leg_half"], depth=m["shin_depth"], material=leg_mat,
            profile=[(m["ankle_z"], 0.68), (m["knee_z"], 1.0)],
            lean=shin_lean * side,
        ))

    # --- shoes ------------------------------------------------------------
    # ONE object each, sole on z = 0, top at the ankle, so `max_y` is the ankle
    # and the foot pivot the renderer derives lands exactly where the shin ends.
    sw, sl, sh = m["shoe_half"], m["shoe_len"], m["shoe_h"]
    toe_y = -sl * 0.5
    heel_y = sl * 0.5
    shoe_lean = math.tan(math.radians(m["shoe_lean_deg"]))
    for side, tag in SIDES:
        shoe = K.cube(
            f"shoe_{tag}",
            size=(sw * 2.0, sl, sh),
            location=(side * m["ankle_x"], toe_y + sl * 0.5, sh * 0.5),
            material=shoe_mat, bevel=0.030, bevel_segments=3,
        )
        # Narrow to the toe (-Y) and to the ankle opening: two silhouettes out
        # of one box, and the top face stays on `shoe_h` so the ankle pivot the
        # renderer reads off `max_y` is still the ankle.
        # The z factor tapers the shoe's HEIGHT at the toe, and _profile_y
        # scales about the part's own mid-height cz = sh/2. A toe factor below
        # 1.0 therefore pulls the sole UP off z=0 and the character stands on
        # nothing — measured at +1.05 mm on a 85 mm shoe. The shoe's own height
        # must stay full at the sole, so the toe narrows in X only and the
        # taper is carried by the heel station instead.
        _profile_y(shoe, [(toe_y, 0.62, 1.00),
                          (toe_y + sl * 0.55, 1.02, 1.00),
                          (heel_y, 0.94, 0.96)])
        # The sole ring must stay at 1.00: a scale of 1.06 here widens the
        # bottom face past the box half-height and pushes it 6% below z=0, so
        # the character stands on nothing and the grounding check fails by
        # ~1.4 mm on every build. The sole is full width; only the ankle
        # opening narrows.
        _profile_z(shoe, [(0.0, 1.00, 1.00), (sh * 0.45, 1.02, 1.02), (sh, 0.78, 0.82)])
        _lean(shoe, sh, shoe_lean * side)
        K.shade_smooth(shoe, angle=math.radians(38.0))
        parts.append(shoe)

    # --- garment extras (torso / head only — see the module docstring) -----
    hook = m.get("extras")
    if hook is not None:
        parts += hook(m, pal, top, bottom, accent, extra_mats)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 1. ped_suit — formal, narrow, navy on charcoal. The player rig.
# ---------------------------------------------------------------------------


def _suit_extras(m, pal, top, bottom, accent, mats):
    """Lapel, shirt panel, tie, belt — the four zones that make it a suit."""
    half_w = m["torso_half"]
    depth = m["torso_depth"]
    chest_z = m["chest_z"]
    waist_z = m["waist_z"]
    front = -depth * 0.5
    out: list[bpy.types.Object] = []

    # Turned-down collar, sitting on the shoulder line.
    out.append(K.torus("collar", major_radius=m["neck_r"] * 1.55,
                       minor_radius=m["neck_r"] * 0.42,
                       major_segments=20, minor_segments=8,
                       location=(0.0, 0.0, m["neck_z0"] + 0.030), material=accent))
    # Shirt wedge down the centre of the chest.
    out.append(_placed("shirt_panel", (half_w * 0.66, 0.030, (chest_z - waist_z) * 1.05),
                       (0.0, front - 0.012, (chest_z + waist_z) * 0.5 + 0.02),
                       accent, bevel=0.010))
    # Lapels: one raked per side, mirrored by the ±X rotation.
    for side, tag in SIDES:
        out.append(_placed(f"lapel_{tag}", (0.062, 0.034, 0.185),
                           (side * 0.058, front - 0.020, chest_z + 0.020), top,
                           rotation=(0.0, side * math.radians(11.0), 0.0),
                           bevel=0.012))
    # Tie + knot.
    out.append(_placed("tie", (0.050, 0.020, 0.230),
                       (0.0, front - 0.028, chest_z - 0.075), mats["tie"], bevel=0.008))
    out.append(_placed("tie_knot", (0.046, 0.026, 0.052),
                       (0.0, front - 0.030, chest_z + 0.062), mats["tie"], bevel=0.010))
    # Waist belt + buckle, then the jacket hem band below it.
    out.append(_placed("belt", (half_w * 1.92, depth * 0.90, 0.042),
                       (0.0, 0.0, waist_z), mats["belt"], bevel=0.010))
    out.append(_placed("buckle", (0.052, 0.024, 0.050),
                       (0.0, front - 0.020, waist_z), mats["buckle"], bevel=0.008))
    out.append(_placed("hem_band", (half_w * 1.86, depth * 0.88, 0.026),
                       (0.0, 0.0, m["torso_z"][0] + 0.022), mats["belt"], bevel=0.008))
    for side, tag in SIDES:
        out.append(_placed(f"pocket_{tag}", (0.070, 0.018, 0.050),
                           (side * half_w * 0.72, front - 0.006, waist_z - 0.075),
                           mats["belt"], bevel=0.006))
        # The cuff sits on the FOREARM, whose x is the sheared `wrist_x`, not
        # the shoulder x — placed on the wrong column it floats in mid-air.
        out.append(_placed(f"cuff_{tag}",
                           (m["lower_arm_half"] * 2.0, m["forearm_depth"] * 0.96, 0.026),
                           (side * m["wrist_x"], 0.0, m["wrist_z"] + 0.070),
                           accent, bevel=0.008))
    return out


def build_suit(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    # Reset before anything else: a stale object from the previous asset keeps
    # its name, and the renderer would then find two `torso` parts.
    K.reset_scene()

    pal = dict(
        garment=(0.15, 0.17, 0.26),
        accent=(0.93, 0.94, 0.97),
        trousers=(0.12, 0.13, 0.17),
        shoe=(0.055, 0.055, 0.065),
        skin=(0.80, 0.62, 0.48),
        hair=(0.14, 0.11, 0.09),
        eye=(0.10, 0.09, 0.11),
        extras=dict(tie=(0.72, 0.10, 0.18), belt=(0.09, 0.09, 0.11),
                    buckle=(0.86, 0.88, 0.92)),
    )
    pal.update(kwargs)

    m = dict(
        head_z=1.6325, head_r=0.104, head_y=1.06, head_z_scale=1.13,
        hair_style="side_part", hair_y=1.02, hair_z=0.90,
        neck_r=0.052, neck_z0=1.408, neck_z1=1.562,
        shoulder_z=1.432, shoulder_x=0.219,
        elbow_z=1.096,
        wrist_z=0.770,
        hip_z=0.946, hip_x=0.105,
        knee_z=0.485,
        ankle_z=0.098,
        waist_z=1.100, chest_z=1.300,
        torso_z=(0.845, 1.455), torso_half=0.205, torso_depth=0.245,
        upper_arm_half=0.073, lower_arm_half=0.052, arm_depth=0.118,
        forearm_depth=0.098,
        upper_leg_half=0.092, lower_leg_half=0.077, thigh_depth=0.180,
        shin_depth=0.148,
        shoe_half=0.062, shoe_len=0.255, shoe_h=0.112,
        arm_lean_deg=5.0, leg_lean_deg=2.0, shoe_lean_deg=1.0,
        bare_arms=False, bare_legs=False,
        extras=_suit_extras,
    )
    return _figure(asset_id, m, pal)


# ---------------------------------------------------------------------------
# 2. ped_dress — narrow bodice over an A-line skirt, bare arms and legs
# ---------------------------------------------------------------------------


def _dress_extras(m, pal, top, bottom, accent, mats):
    """Waistband, bodice seams, shoulder straps and the A-line skirt itself."""
    half_w = m["torso_half"]
    depth = m["torso_depth"]
    waist_z = m["waist_z"]
    bust_z = m["bust_z"]
    front = -depth * 0.5
    out: list[bpy.types.Object] = []

    out.append(K.torus("collar", major_radius=m["neck_r"] * 1.50,
                       minor_radius=m["neck_r"] * 0.38,
                       major_segments=20, minor_segments=8,
                       location=(0.0, 0.0, m["neck_z0"] + 0.030), material=mats["trim"]))
    out.append(_placed("bodice_band", (half_w * 1.66, depth * 0.92, 0.030),
                       (0.0, 0.0, bust_z - 0.110), mats["trim"], bevel=0.009))
    out.append(_placed("waistband", (half_w * 1.94, depth * 0.96, 0.046),
                       (0.0, 0.0, waist_z), mats["trim"], bevel=0.012))
    for side, tag in SIDES:
        out.append(_placed(f"strap_{tag}", (0.030, 0.055, 0.215),
                           (side * half_w * 0.52, -0.030, bust_z + 0.095),
                           mats["trim"], bevel=0.008))
        out.append(_placed(f"bust_seam_{tag}", (0.024, 0.026, 0.150),
                           (side * half_w * 0.44, front - 0.006, bust_z - 0.030),
                           mats["trim"], bevel=0.006))

    # The skirt: a truncated cone flaring down from the waist. Its top radius
    # is wider than the bare hips at that height, so the thighs start INSIDE it
    # and never poke through the flare.
    bot_z, top_z = m["skirt_z"]
    bot_r, top_r = m["skirt_r"]
    out.append(K.cone("skirt", radius1=bot_r, radius2=top_r, depth=top_z - bot_z,
                      vertices=24, location=(0.0, 0.0, (bot_z + top_z) * 0.5),
                      material=top, smooth=True))
    out.append(K.torus("skirt_hem", major_radius=bot_r * 0.99, minor_radius=0.026,
                       major_segments=24, minor_segments=8,
                       location=(0.0, 0.0, bot_z + 0.020), material=mats["trim"]))
    # Four panel seams up the flare: the cue that says "sewn", not "cone".
    for i in range(4):
        ang = math.radians(28.0 + 90.0 * i)
        out.append(_placed(
            f"skirt_seam_{i}", (0.018, 0.014, (top_z - bot_z) * 0.88),
            (math.cos(ang) * (bot_r + top_r) * 0.5,
             math.sin(ang) * (bot_r + top_r) * 0.5,
             (bot_z + top_z) * 0.5),
            mats["trim"], bevel=0.004))
    return out


def build_dress(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    # Reset first: see `build_suit`.
    K.reset_scene()

    pal = dict(
        garment=(0.94, 0.23, 0.45),
        accent=(0.98, 0.93, 0.88),
        trousers=(0.86, 0.68, 0.56),
        shoe=(0.88, 0.48, 0.52),
        skin=(0.86, 0.68, 0.56),
        hair=(0.42, 0.18, 0.10),
        eye=(0.16, 0.12, 0.14),
        extras=dict(trim=(0.98, 0.93, 0.88), band=(0.86, 0.16, 0.38)),
    )
    pal.update(kwargs)

    m = dict(
        head_z=1.6300, head_r=0.101, head_y=1.05, head_z_scale=1.14,
        hair_style="bob", hair_y=1.02, hair_z=0.90,
        neck_r=0.045, neck_z0=1.382, neck_z1=1.538,
        shoulder_z=1.406, shoulder_x=0.168,
        elbow_z=1.088,
        wrist_z=0.780,
        hip_z=0.930, hip_x=0.094,
        knee_z=0.480,
        ankle_z=0.094,
        waist_z=1.048, chest_z=1.270, bust_z=1.290,
        torso_z=(0.948, 1.425), torso_half=0.170, torso_depth=0.212,
        upper_arm_half=0.047, lower_arm_half=0.036, arm_depth=0.098,
        forearm_depth=0.082,
        upper_leg_half=0.078, lower_leg_half=0.062, thigh_depth=0.156,
        shin_depth=0.124,
        shoe_half=0.052, shoe_len=0.225, shoe_h=0.100,
        skirt_z=(0.585, 1.050), skirt_r=(0.305, 0.150),
        arm_lean_deg=4.0, leg_lean_deg=2.0, shoe_lean_deg=1.0,
        bare_arms=True, bare_legs=True,
        extras=_dress_extras,
    )
    return _figure(asset_id, m, pal)


# ---------------------------------------------------------------------------
# 3. ped_overalls — widest torso: bib + shoulder straps over a work shirt
# ---------------------------------------------------------------------------


def _overalls_extras(m, pal, top, bottom, accent, mats):
    """Denim bib, two shoulder straps, a tool belt and a cap."""
    half_w = m["torso_half"]
    depth = m["torso_depth"]
    front = -depth * 0.5
    waist_z = m["waist_z"]
    bib_top = m["bib_top"]
    out: list[bpy.types.Object] = []

    # The bib is a flat plate on the shirt's chest, and the shirt shows above
    # and below it — that two-material split is the whole overalls read.
    out.append(_placed("bib", (m["bib_half"] * 2.0, 0.040, bib_top - waist_z + 0.235),
                       (0.0, front - 0.022, (bib_top + waist_z) * 0.5 + 0.118),
                       mats["denim"], bevel=0.014))
    out.append(_placed("bib_pocket", (m["bib_half"] * 1.44, 0.024, 0.110),
                       (0.0, front - 0.046, bib_top - 0.075), mats["denim"],
                       bevel=0.010))
    for side, tag in SIDES:
        # Each strap is ONE object: a bar over the shoulder, raked in Y so it
        # follows the trapezius instead of standing off it.
        out.append(_placed(f"strap_{tag}", (0.052, 0.040, bib_top - waist_z + 0.080),
                           (side * m["bib_half"] * 0.66, front + 0.010,
                            (bib_top + waist_z) * 0.5 + 0.040),
                           mats["denim"],
                           rotation=(math.radians(-9.0), 0.0, 0.0),
                           bevel=0.012))
        out.append(_placed(f"strap_buckle_{tag}", (0.056, 0.034, 0.048),
                           (side * m["bib_half"] * 0.66, front - 0.030, waist_z + 0.090),
                           mats["buckle"], bevel=0.008))
        out.append(_placed(f"tool_pouch_{tag}", (0.072, 0.050, 0.108),
                           (side * m["hip_x"] * 1.42, -depth * 0.40, waist_z - 0.045),
                           mats["tool"], bevel=0.012))
    out.append(_placed("tool_belt", (half_w * 1.96, depth * 0.94, 0.062),
                       (0.0, 0.0, waist_z + 0.010), mats["tool"], bevel=0.014))
    out.append(_placed("tool_handle", (0.030, 0.026, 0.140),
                       (half_w * 0.62, depth * 0.24, waist_z + 0.090),
                       mats["buckle"],
                       rotation=(math.radians(11.0), 0.0, 0.0), bevel=0.006))
    out += _cap(m, mats["cap"], mats["cap_band"], backwards=False)
    return out


def build_overalls(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    # Reset first: see `build_suit`.
    K.reset_scene()

    pal = dict(
        garment=(0.86, 0.67, 0.36),
        accent=(0.30, 0.42, 0.64),
        trousers=(0.24, 0.38, 0.62),
        shoe=(0.36, 0.24, 0.14),
        skin=(0.64, 0.46, 0.34),
        hair=(0.18, 0.14, 0.10),
        eye=(0.12, 0.10, 0.09),
        extras=dict(denim=(0.24, 0.38, 0.62), cap=(0.82, 0.30, 0.22),
                    cap_band=(0.62, 0.20, 0.15), tool=(0.46, 0.31, 0.16),
                    buckle=(0.80, 0.74, 0.40)),
    )
    pal.update(kwargs)

    m = dict(
        head_z=1.6280, head_r=0.104, head_y=1.06, head_z_scale=1.13,
        hair_style="crop", hair_y=1.02, hair_z=0.90,
        neck_r=0.058, neck_z0=1.404, neck_z1=1.558,
        shoulder_z=1.428, shoulder_x=0.232,
        elbow_z=1.086,
        wrist_z=0.782,
        hip_z=0.938, hip_x=0.118,
        knee_z=0.478,
        ankle_z=0.100,
        waist_z=1.096, chest_z=1.300, bib_top=1.430,
        torso_z=(0.858, 1.452), torso_half=0.220, torso_depth=0.268,
        bib_half=0.150,
        upper_arm_half=0.076, lower_arm_half=0.058, arm_depth=0.134,
        forearm_depth=0.116,
        upper_leg_half=0.110, lower_leg_half=0.092, thigh_depth=0.208,
        shin_depth=0.176,
        shoe_half=0.068, shoe_len=0.278, shoe_h=0.112,
        arm_lean_deg=5.0, leg_lean_deg=2.0, shoe_lean_deg=1.0,
        bare_arms=False, bare_legs=False,
        extras=_overalls_extras,
    )
    return _figure(asset_id, m, pal)


# ---------------------------------------------------------------------------
# 4. ped_streetwear — boxy hoodie over baggy cargo legs; the widest figure
# ---------------------------------------------------------------------------


def _streetwear_extras(m, pal, top, bottom, accent, mats):
    """Hood bunched at the neck, kangaroo pocket, ribbed hem, backwards cap."""
    half_w = m["torso_half"]
    depth = m["torso_depth"]
    front = -depth * 0.5
    waist_z = m["waist_z"]
    hem_z = m["torso_z"][0]
    out: list[bpy.types.Object] = []

    # Hood: a flattened shell sitting BEHIND the neck (+Y) so the shoulders and
    # the front of the hoodie still read instead of being swallowed by it.
    out.append(K.sphere("hood", radius=half_w * 0.80, segments=24, ring_count=12,
                        location=(0.0, depth * 0.46, m["neck_z0"] - 0.055),
                        material=top, scale=(1.0, 0.72, 0.94)))
    out.append(_placed("hood_lining", (half_w * 0.92, 0.042, 0.108),
                       (0.0, front - 0.010, m["neck_z0"] + 0.040),
                       mats["rib"], bevel=0.012))
    for side, tag in SIDES:
        out.append(_placed(f"drawstring_{tag}", (0.013, 0.013, 0.165),
                           (side * 0.042, front - 0.026, m["neck_z0"] - 0.020),
                           mats["lace"], bevel=0.004))
        out.append(_placed(f"drawstring_tip_{tag}", (0.019, 0.019, 0.026),
                           (side * 0.042, front - 0.026, m["neck_z0"] - 0.112),
                           mats["lace"], bevel=0.006))
        # Shoulder seams: what makes a hoodie read as flat-knit cotton.
        out.append(_placed(f"shoulder_seam_{tag}", (0.015, depth * 0.82, 0.015),
                           (side * half_w * 0.86, 0.0, m["chest_z"] + 0.090),
                           mats["lining"], bevel=0.004))
    # Kangaroo pocket with its two openings, and the ribbed waistband.
    out.append(_placed("pocket", (half_w * 1.48, 0.046, 0.190),
                       (0.0, front - 0.020, waist_z - 0.145), mats["rib"], bevel=0.012))
    for side, tag in SIDES:
        out.append(_placed(f"pocket_opening_{tag}", (0.013, 0.022, 0.152),
                           (side * half_w * 0.46, front - 0.038, waist_z - 0.142),
                           mats["lining"], bevel=0.004))
    out.append(_placed("rib", (half_w * 1.94, depth * 0.94, 0.052),
                       (0.0, 0.0, hem_z + 0.030), mats["rib"], bevel=0.014))
    out.append(_placed("rib_stitch", (half_w * 1.96, depth * 0.96, 0.014),
                       (0.0, 0.0, hem_z + 0.062), mats["lace"], bevel=0.004))
    out += _cap(m, mats["cap"], mats["cap_band"], backwards=True)
    return out


def build_streetwear(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    # Reset first: see `build_suit`.
    K.reset_scene()

    pal = dict(
        garment=(0.24, 0.58, 0.62),
        accent=(0.20, 0.48, 0.52),
        trousers=(0.22, 0.24, 0.30),
        shoe=(0.92, 0.92, 0.94),
        skin=(0.72, 0.52, 0.40),
        hair=(0.10, 0.10, 0.12),
        eye=(0.11, 0.10, 0.12),
        extras=dict(rib=(0.20, 0.48, 0.52), lining=(0.14, 0.34, 0.38),
                    lace=(0.95, 0.95, 0.92), cap=(0.18, 0.20, 0.26),
                    cap_band=(0.12, 0.13, 0.17)),
    )
    pal.update(kwargs)

    m = dict(
        head_z=1.6280, head_r=0.105, head_y=1.06, head_z_scale=1.13,
        hair_style="tail", hair_y=1.02, hair_z=0.90,
        neck_r=0.060, neck_z0=1.372, neck_z1=1.552,
        shoulder_z=1.388, shoulder_x=0.252,
        elbow_z=1.070,
        wrist_z=0.756,
        hip_z=0.916, hip_x=0.130,
        knee_z=0.462,
        ankle_z=0.100,
        waist_z=1.120, chest_z=1.300,
        torso_z=(0.762, 1.432), torso_half=0.238, torso_depth=0.282,
        upper_arm_half=0.086, lower_arm_half=0.072, arm_depth=0.146,
        forearm_depth=0.128,
        upper_leg_half=0.126, lower_leg_half=0.112, thigh_depth=0.232,
        shin_depth=0.206,
        shoe_half=0.074, shoe_len=0.290, shoe_h=0.108,
        arm_lean_deg=6.0, leg_lean_deg=2.5, shoe_lean_deg=1.5,
        bare_arms=False, bare_legs=False,
        extras=_streetwear_extras,
    )
    return _figure(asset_id, m, pal)


# ---------------------------------------------------------------------------
# registry entry
# ---------------------------------------------------------------------------

PEDESTRIANS: list[tuple[str, object, dict]] = [
    ("ped_suit", build_suit, dict(
        garment=(0.15, 0.17, 0.26),
        accent=(0.93, 0.94, 0.97),
        trousers=(0.12, 0.13, 0.17),
        shoe=(0.055, 0.055, 0.065),
        skin=(0.80, 0.62, 0.48),
    )),
    ("ped_dress", build_dress, dict(
        garment=(0.94, 0.23, 0.45),
        accent=(0.98, 0.93, 0.88),
        trousers=(0.86, 0.68, 0.56),
        shoe=(0.88, 0.48, 0.52),
        skin=(0.86, 0.68, 0.56),
    )),
    ("ped_overalls", build_overalls, dict(
        garment=(0.86, 0.67, 0.36),
        accent=(0.30, 0.42, 0.64),
        trousers=(0.24, 0.38, 0.62),
        shoe=(0.36, 0.24, 0.14),
        skin=(0.64, 0.46, 0.34),
    )),
    ("ped_streetwear", build_streetwear, dict(
        garment=(0.24, 0.58, 0.62),
        accent=(0.20, 0.48, 0.52),
        trousers=(0.22, 0.24, 0.30),
        shoe=(0.92, 0.92, 0.94),
        skin=(0.72, 0.52, 0.40),
    )),
]