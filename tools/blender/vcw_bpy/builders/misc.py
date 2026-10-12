"""3A misc props + mission markers — the one-off street furniture.

Eight assets in two lists. Everything here is modelled from primitives in
`kernel`; nothing is traced, imported or measured off a real object, and in
particular no aircraft marking, phone-box logo or tag art belonging to
anything real is reproduced. The livery is an invented two-tone over a warm
cream, the graffiti is overlapping abstract plates, and the paint on the
helipad is a chevron and a bar rather than any registered marking.

What these six props have in common, and why each one is built the way it is:

* **MASSING before detail.** An awning is a sagging surface over a frame, not
  a striped slab; a dumpster pile is four boxes that do not agree on which way
  is up. Get the silhouette right and the materials can be crude; get the
  silhouette wrong and no amount of PBR rescues it.
* **BEVEL EVERY HARD EDGE.** See `vehicles` for the long version. A bevelled
  cube reads as a moulded plastic part; an unbeveled one reads as a blockout.
  Every box in this file is bevelled, and the thin decal plates are bevelled
  with a width scaled to their own thickness rather than a copy-pasted 2 cm.
* **PER-PART MATERIALS.** A metal frame under a fabric canopy under a painted
  panel is three different (roughness, metallic) responses, and glTF carries
  exactly those as separate primitives.
* **GROUNDED ON z = 0.** `_ground` is applied to every builder as the last
  geometric step, so the lowest vertex of the whole asset is exactly 0 in
  authoring space regardless of how the parts were stacked while building.
  Wall-mounted assets get feet or a sill so that grounding is meaningful
  rather than an arbitrary shift.

Coordinate contract: Z-up, +X right, **-Y is FRONT**, metres, lowest point
z = 0. `registry` imports `MISC` and `MARKERS` from here and calls each entry
as `fn(asset_id, **kwargs)`.
"""

from __future__ import annotations

import math
from typing import Iterable, Sequence

import bpy

from .. import kernel as K
from .. import palette as P


# ---------------------------------------------------------------------------
# shared local helpers
# ---------------------------------------------------------------------------


def _place(
    objs: Iterable[bpy.types.Object],
    rot_deg: Sequence[float] = (0.0, 0.0, 0.0),
    offset: Sequence[float] = (0.0, 0.0, 0.0),
) -> list[bpy.types.Object]:
    """Rigid-transform a group of parts about their shared LOCAL origin.

    The first `transform_apply` is load-bearing: `K.cylinder`/`K.cone`/
    `K.sphere` leave the primitive's `location` on the object while
    `K.cube` bakes it into the mesh, so without baking first the two kinds of
    part would rotate about different points and the group would come apart.
    Baking first puts every part's origin on (0, 0, 0), which makes the
    rotation below turn the whole group about the point the caller chose.

    All angles are proper rotations, so face winding and handedness survive —
    an inside-out shell cannot be introduced here.
    """
    rx, ry, rz = (math.radians(a) for a in rot_deg)
    out: list[bpy.types.Object] = []
    for obj in objs:
        if obj is None:
            continue
        K.transform_apply(obj)
        obj.rotation_euler = (rx, ry, rz)
        K.transform_apply(obj)
        obj.location = tuple(float(v) for v in offset)
        K.transform_apply(obj)
        out.append(obj)
    return out


def _ground(objs: Iterable[bpy.types.Object]) -> list[bpy.types.Object]:
    """Translate a set of parts so its lowest point rests on z = 0.

    Read off the mesh data rather than the object bounding boxes: every
    modifier these builders add is already applied (`K.add_bevel` applies,
    `shade_auto_smooth` only changes normals), so `data.vertices` is the
    final geometry and this costs one pass instead of a depsgraph evaluation
    per object.
    """
    objs = [o for o in objs if o is not None and o.type == "MESH"]
    lowest: float | None = None
    for obj in objs:
        world = obj.matrix_world
        for v in obj.data.vertices:
            z = (world @ v.co).z
            lowest = z if lowest is None else min(lowest, z)
    if lowest is None:
        return objs
    dz = -lowest
    if abs(dz) > 1e-12:
        for obj in objs:
            # No parents anywhere in this module, so matrix_world.translation
            # is exactly obj.location and a pure Z nudge is a translation.
            obj.location = (obj.location.x, obj.location.y, obj.location.z + dz)
            K.transform_apply(obj)
    return objs


def _radial_slab(
    name: str,
    size: Sequence[float],
    ang_rad: float,
    radius: float,
    z: float,
    material: bpy.types.Material,
    bevel: float = 0.0,
    bevel_segments: int = 2,
) -> bpy.types.Object:
    """A flat slab whose local +X is radial and +Y tangential, on a ring.

    Radial plates are how a painted band on a road is built here. One solid
    disc with the band cut into it is two discs at two heights, which reads as
    a wedding cake; N abutting plates lying in one plane read as paint. The
    caller sizes the tangential axis to `2*pi*r/n * 1.04` so consecutive
    plates overlap slightly and no seam shows.
    """
    obj = K.cube(
        name, size=size, location=(0.0, 0.0, 0.0), material=material,
        bevel=bevel, bevel_segments=bevel_segments,
    )
    obj.rotation_euler = (0.0, 0.0, ang_rad)
    K.transform_apply(obj)
    obj.location = (radius * math.cos(ang_rad), radius * math.sin(ang_rad), z)
    K.transform_apply(obj)
    return obj


def _merge(name: str, objs: Sequence[bpy.types.Object]) -> bpy.types.Object:
    """Join same-material parts into one object.

    A part is the unit of MATERIAL and of NAME, not of instance, so the 18
    plates of a helipad ring belong to one part: the exporter rejects
    duplicate part names, and 18 identical one-material objects would be 18
    addressable names carrying one look.
    """
    return K.join(name, list(objs))


def _ring_blades(
    prefix: str,
    centre: Sequence[float],
    radius: float,
    count: int,
    blade_size: Sequence[float],
    tilt_deg: float,
    axis: str,
    material: bpy.types.Material,
    bevel: float = 0.02,
) -> list[bpy.types.Object]:
    """`count` blades swept evenly around a hub.

    `axis` is "z" for a rotor disc lying flat over the fuselage and "x" for a
    tail rotor disc standing in the Y-Z plane. The blade is built pointing at +X (or +Z) from the origin, rotated there,
    THEN translated onto the hub. Rotating after placement would swing the
    blade around the AIRCRAFT's own centre instead of its own hub and throw
    it metres away.

    `blade_size` is the FULL extent, so it is centred on `radius/2` out from
    the hub to reach `radius` at the tip: a blade built at the origin and left
    centred there would have half of itself inside the hub.
    """
    cx, cy, cz = centre
    out: list[bpy.types.Object] = []
    step = 360.0 / count
    for i in range(count):
        ang = math.radians(step * i)
        mid = radius * 0.5
        if axis == "z":
            blade = K.cube(
                f"{prefix}_blade_{i}", size=(radius, blade_size[1], blade_size[2]),
                location=(mid, 0.0, 0.0), material=material, bevel=bevel,
            )
            blade.rotation_euler = (0.0, math.radians(tilt_deg), ang)
            K.transform_apply(blade)
            blade.location = (cx, cy, cz)
            K.transform_apply(blade)
        else:
            # Tail rotor: the disc stands in the Y-Z plane, so the blade's
            # long axis must run along Z before the sweep about X.
            blade = K.cube(
                f"{prefix}_blade_{i}", size=(blade_size[0], blade_size[1], radius),
                location=(0.0, 0.0, mid), material=material, bevel=bevel,
            )
            blade.rotation_euler = (ang, math.radians(tilt_deg), 0.0)
            K.transform_apply(blade)
            blade.location = (cx, cy, cz)
            K.transform_apply(blade)
        out.append(blade)
    return out


# ---------------------------------------------------------------------------
# 1. misc_awning
# ---------------------------------------------------------------------------


def build_awning(
    asset_id: str,
    *,
    span: float = 2.70,
    depth: float = 1.45,
    wall_z: float = 2.10,
    edge_z: float = 1.60,
    sag: float = 0.12,
    ribs: int = 5,
    canvas_rgb=(0.86, 0.22, 0.28),
    trim_rgb=(0.96, 0.93, 0.84),
    frame_rgb=(0.58, 0.60, 0.63),
) -> list[bpy.types.Object]:
    """A market awning: fabric that visibly SAGS between its ribs.

    The sag is the whole point of this asset, so it is modelled rather than
    implied. Two curves do the work:

    * along Y (wall -> front lip) the section is not a straight slope. The
      profile is `wall_z + (edge_z - wall_z)*t - belly*sin(pi*t)`, which is
      steep where it leaves the wall and flattens as it reaches the lip — the
      curve a loaded canvas takes.
    * along X the fabric dips between ribs and rises over each rib, so every
      panel is also rolled by the local slope of that dip. Without the roll
      the panels are flat tiles stepping down in a staircase; with it they
      meet edge to edge like roof panels.

    Each panel is a real bevelled box, because the surface has to stay a
    watertight set of closed shells (no `open_surface` declaration needed) and
    because a sagging cloth is exactly what a grid of taut facets models
    honestly.
    """
    K.reset_scene()

    canvas = P.fabric(f"{asset_id}_canvas", canvas_rgb, rough=0.88)
    trim = P.paint(f"{asset_id}_trim", trim_rgb, roughness=0.62)
    frame = P.metal(f"{asset_id}_frame", frame_rgb, rough=0.42, metal=0.75)
    rib_mat = P.metal(f"{asset_id}_rib", (0.44, 0.46, 0.49), rough=0.48, metal=0.85)

    half = span * 0.5
    bays = max(1, ribs - 1)
    bay_w = span / bays
    belly = 0.09

    def profile(t: float) -> float:
        return wall_z + (edge_z - wall_z) * t - belly * math.sin(math.pi * t)

    def slope_dz_dt(t: float) -> float:
        return (edge_z - wall_z) - belly * math.pi * math.cos(math.pi * t)

    def sag_at(x: float) -> float:
        u = (x + half) / bay_w
        return -sag * math.sin(math.pi * (u - math.floor(u)))

    def sag_slope(x: float) -> float:
        u = (x + half) / bay_w
        return -sag * math.pi / bay_w * math.cos(math.pi * (u - math.floor(u)))

    def pitch(t: float) -> float:
        """Rotation about +X: dz/dy = tan(pitch), and dy/dt = -depth."""
        return math.degrees(math.atan(-slope_dz_dt(t) / depth))

    parts: list[bpy.types.Object] = []

    # --- fabric panels: a roll over X times a curve down Y ----------------
    cols, rows = 20, 3
    dx = span / cols
    dy = depth / rows
    cells: list[bpy.types.Object] = []
    for r in range(rows):
        tc = (r + 0.5) / rows
        y = -depth * tc
        base_z = profile(tc)
        for c in range(cols):
            x = -half + (c + 0.5) * dx
            roll = math.degrees(math.atan(-sag_slope(x)))
            cell = K.cube(
                f"{asset_id}_canvas_{r}_{c}",
                size=(dx * 1.25, dy * 1.25, 0.038),
                location=(0.0, 0.0, 0.0), material=canvas,
                bevel=0.008, bevel_segments=1,
            )
            cell.rotation_euler = (math.radians(pitch(tc)), math.radians(roll), 0.0)
            K.transform_apply(cell)
            cell.location = (x, y, base_z + sag_at(x) - 0.019)
            K.transform_apply(cell)
            cells.append(cell)
    parts.append(_merge(f"{asset_id}_canvas", cells))

    # --- ribs: the frame the fabric hangs between -------------------------
    rib_cells: list[bpy.types.Object] = []
    for k in range(ribs):
        x = -half + k * bay_w
        for r in range(rows):
            tc = (r + 0.5) / rows
            seg = K.cube(
                f"{asset_id}_rib_{k}_{r}",
                size=(0.055, dy * 1.3, 0.05),
                location=(0.0, 0.0, 0.0), material=rib_mat,
                bevel=0.012, bevel_segments=1,
            )
            seg.rotation_euler = (math.radians(pitch(tc)), 0.0, 0.0)
            K.transform_apply(seg)
            seg.location = (x, -depth * tc, profile(tc) - 0.048)
            K.transform_apply(seg)
            rib_cells.append(seg)
    parts.append(_merge(f"{asset_id}_ribs", rib_cells))

    # --- rails, braces, posts --------------------------------------------
    parts.append(K.cube(
        f"{asset_id}_wall_rail", size=(span * 1.04, 0.075, 0.085),
        location=(0.0, 0.02, wall_z + 0.05), material=frame, bevel=0.014,
    ))
    parts.append(K.cube(
        f"{asset_id}_front_bar", size=(span * 1.04, 0.075, 0.085),
        location=(0.0, -depth - 0.02, edge_z - 0.02), material=frame, bevel=0.014,
    ))

    arm_len = math.hypot(depth, wall_z - edge_z) + 0.10
    arm_pitch = math.degrees(math.atan((wall_z - edge_z) / depth))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        x = sx * (half - 0.06)
        parts += _place(
            [K.cube(f"{asset_id}_arm_{tag}", size=(0.06, arm_len, 0.06),
                    location=(0.0, 0.0, 0.0), material=frame, bevel=0.012)],
            rot_deg=(arm_pitch, 0.0, 0.0),
            offset=(x, -depth * 0.5, (wall_z + edge_z) * 0.5 - 0.11),
        )
        parts += _place(
            [K.cube(f"{asset_id}_stub_{tag}", size=(0.09, 0.09, 0.30),
                    location=(0.0, 0.0, 0.0), material=frame, bevel=0.016)],
            rot_deg=(0.0, 0.0, 0.0),
            offset=(x, 0.06, wall_z - 0.15),
        )
        # Stall posts: the awning is a market canopy, so the front corners
        # carry real legs down to the pavement and the asset is genuinely
        # grounded rather than floating at wall-rail height.
        post_h = edge_z - 0.06
        parts.append(K.cube(
            f"{asset_id}_post_{tag}", size=(0.075, 0.075, post_h),
            location=(x, -depth + 0.04, post_h * 0.5), material=frame, bevel=0.016,
        ))
        parts.append(K.cube(
            f"{asset_id}_post_foot_{tag}", size=(0.20, 0.20, 0.045),
            location=(x, -depth + 0.04, 0.0225), material=rib_mat, bevel=0.01,
        ))

    # --- scalloped valance hanging off the front lip -----------------------
    scallops = max(4, ribs * 2)
    sw = span / scallops
    flaps: list[bpy.types.Object] = []
    for i in range(scallops):
        x = -half + (i + 0.5) * sw
        flaps.append(K.cube(
            f"{asset_id}_valance_{i}", size=(sw * 1.04, 0.030, 0.21),
            location=(x, -depth - 0.045, edge_z - 0.145),
            material=trim, bevel=0.008, bevel_segments=1,
        ))
    parts.append(_merge(f"{asset_id}_valance", flaps))

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 2. misc_dumpster_pile
# ---------------------------------------------------------------------------

_BIN_HUES = (
    (0.10, 0.34, 0.32),
    (0.52, 0.17, 0.14),
    (0.28, 0.31, 0.19),
    (0.20, 0.26, 0.44),
)


def _bin(
    tag: str,
    hue,
    lid_open_deg: float,
    *,
    rubber,
    metal,
) -> list[bpy.types.Object]:
    """One refuse bin, built about its own base centre at (0, 0, 0).

    The body is a four-sided cone rather than a box: real wheelie bins are
    tapered so they hang over the axle, and a straight box is the single
    fastest way to make a prop read as a blockout.
    """
    body = P.paint(f"bin_{tag}_body", hue, roughness=0.58)
    lid_mat = P.paint(f"bin_{tag}_lid", tuple(min(1.0, c * 1.5 + 0.10) for c in hue),
                      roughness=0.50)

    parts: list[bpy.types.Object] = []

    # radius is the circumradius of the square; halving it by cos(45) gives
    # the actual half-width, and rotating 45 deg squares it up with the axes.
    shell = K.cone(
        f"bin_{tag}_shell", radius1=0.45 / math.cos(math.radians(45.0)),
        radius2=0.36 / math.cos(math.radians(45.0)), depth=1.02,
        vertices=4, location=(0.0, 0.0, 0.53), material=body, smooth=False,
    )
    shell.rotation_euler = (0.0, 0.0, math.radians(45.0))
    K.transform_apply(shell)
    K.add_bevel(shell, 0.035, 2)
    parts.append(shell)

    parts.append(K.cube(
        f"bin_{tag}_rim", size=(0.94, 0.80, 0.065),
        location=(0.0, 0.0, 1.055), material=lid_mat, bevel=0.018,
    ))

    # Lid hinged at the rear top edge and swung up `lid_open_deg`.
    lid = K.cube(
        f"bin_{tag}_lid", size=(0.98, 0.84, 0.07), location=(0.0, 0.0, 0.0),
        material=lid_mat, bevel=0.02,
    )
    parts += _place([lid], rot_deg=(-lid_open_deg, 0.0, 0.0),
                    offset=(0.0, 0.40, 1.10))

    for sx, side in ((-1.0, "l"), (1.0, "r")):
        wheel = K.cylinder(
            f"bin_{tag}_wheel_{side}", radius=0.105, depth=0.085, vertices=10,
            location=(0.0, 0.0, 0.0), material=rubber, smooth=True,
        )
        wheel.rotation_euler = (0.0, math.radians(90.0), 0.0)
        K.transform_apply(wheel)
        wheel.location = (sx * 0.33, 0.27, 0.105)
        K.transform_apply(wheel)
        parts.append(wheel)
        parts.append(K.cube(
            f"bin_{tag}_handle_{side}", size=(0.07, 0.30, 0.075),
            location=(sx * 0.455, -0.02, 0.78), material=metal, bevel=0.014,
        ))

    parts.append(K.cube(
        f"bin_{tag}_latch", size=(0.16, 0.06, 0.10),
        location=(0.0, -0.42, 0.92), material=metal, bevel=0.014,
    ))
    return parts


def build_dumpster_pile(
    asset_id: str,
    *,
    bins: int = 3,
    seed_skew: float = 0.0,
) -> list[bpy.types.Object]:
    """2-4 refuse bins at varied angles, lids hanging open, plus litter.

    Every bin is grounded INDIVIDUALLY on z = 0 before the whole asset is
    grounded again. Dropping the group as a unit seats only the lowest bin on
    the pavement and leaves the others hanging in mid-air at whatever height
    their rotation put them, which reads as three tidy specimens on a shelf
    rather than as refuse someone left.
    """
    K.reset_scene()

    count = max(2, min(4, int(bins)))
    rubber = P.rubber(f"{asset_id}_rubber")
    metal = P.metal(f"{asset_id}_steel", (0.52, 0.54, 0.57), rough=0.40, metal=0.9)

    parts: list[bpy.types.Object] = []

    # (offset x, offset y, pitch about X, roll about Y, yaw about Z, lid open)
    poses = (
        (-0.66, 0.30, 0.0, 0.0, 18.0, 52.0),
        (0.58, -0.14, -74.0, 6.0, -27.0, 28.0),
        (0.16, 0.78, 4.0, 0.0, -41.0, 68.0),
        (-0.30, -0.68, 46.0, 0.0, 63.0, 40.0),
    )
    for i in range(count):
        px, py, pitch, roll, yaw, lid_open = poses[i]
        hue = _BIN_HUES[i % len(_BIN_HUES)]
        hue = tuple(min(1.0, c + seed_skew * 0.04) for c in hue)
        group = _bin(f"{asset_id}_{i}", hue, lid_open, rubber=rubber, metal=metal)
        _place(group, rot_deg=(pitch, roll, yaw), offset=(px, py, 0.0))
        _ground(group)
        parts += group

    # --- spilled litter, so the heap has a reason to exist ----------------
    bag_rubb = P.paint(f"{asset_id}_bag", (0.13, 0.13, 0.15), roughness=0.42)
    bag_blue = P.paint(f"{asset_id}_bag_blue", (0.16, 0.24, 0.38), roughness=0.44)
    for i, (x, y, r, mat) in enumerate((
        (0.44, -0.56, 0.0, bag_rubb),
        (-0.48, -0.30, 40.0, bag_blue),
    )):
        bag = K.sphere(f"{asset_id}_bag_{i}", radius=0.30, segments=12, ring_count=6,
                       location=(0.0, 0.0, 0.0), material=mat,
                       scale=(1.0, 1.15, 0.78))
        parts += _place([bag], rot_deg=(0.0, 0.0, r), offset=(x, y, 0.0))
        _ground([bag])

    crate_mats = (
        P.paint(f"{asset_id}_crate_a", (0.62, 0.48, 0.31), roughness=0.82),
        P.paint(f"{asset_id}_crate_b", (0.80, 0.76, 0.66), roughness=0.84),
        P.paint(f"{asset_id}_crate_c", (0.44, 0.46, 0.42), roughness=0.80),
    )
    crates = (
        ((0.42, 0.30, 0.20), (-0.14, -0.40), 34.0),
        ((0.30, 0.24, 0.18), (0.94, -0.26), -22.0),
        ((0.34, 0.26, 0.14), (-0.86, 0.52), 12.0),
    )
    for i, (size, (x, y), yaw) in enumerate(crates):
        crate = K.cube(f"{asset_id}_crate_{i}", size=size, location=(0.0, 0.0, 0.0),
                       material=crate_mats[i], bevel=0.016)
        parts += _place([crate], rot_deg=(0.0, 0.0, yaw), offset=(x, y, 0.0))
        _ground([crate])

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 3. misc_graffiti_board
# ---------------------------------------------------------------------------


def build_graffiti_board(
    asset_id: str,
    *,
    width: float = 2.40,
    height: float = 1.66,
    depth: float = 0.11,
) -> list[bpy.types.Object]:
    """A boarded panel on a wall: low box, painted face, two brackets.

    The panel is a closed box, not an open plane, so the outwardness check
    decides it by signed volume and no `outward` declaration is needed — and
    the frame rails stand proud of the face, which is what catches a shadow
    edge and stops the panel reading as a decal stuck on the wall.

    The art is invented: overlapping rotated plates and two brush strokes in
    arbitrary colours. Nothing here reproduces a tag, a letterform or a piece
    of real street art.
    """
    K.reset_scene()

    ply = P.paint(f"{asset_id}_ply", (0.80, 0.74, 0.62), roughness=0.86)
    rail = P.paint(f"{asset_id}_rail", (0.24, 0.26, 0.28), roughness=0.62)
    bracket = P.metal(f"{asset_id}_bracket", (0.46, 0.48, 0.51), rough=0.40, metal=0.85)
    bolt = P.chrome(f"{asset_id}_bolt")

    half_w = width * 0.5
    half_h = height * 0.5
    face_y = -depth * 0.5

    parts: list[bpy.types.Object] = []

    parts.append(K.cube(
        f"{asset_id}_panel", size=(width, depth, height),
        location=(0.0, 0.0, half_h), material=ply, bevel=0.022,
    ))

    rail_t = 0.085
    parts.append(K.cube(
        f"{asset_id}_rail_top", size=(width + 0.10, depth + 0.05, rail_t),
        location=(0.0, 0.0, height - rail_t * 0.5), material=rail, bevel=0.014,
    ))
    parts.append(K.cube(
        f"{asset_id}_rail_bottom", size=(width + 0.10, depth + 0.05, rail_t),
        location=(0.0, 0.0, rail_t * 0.5), material=rail, bevel=0.014,
    ))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(
            f"{asset_id}_stile_{tag}", size=(rail_t, depth + 0.05, height),
            location=(sx * (half_w + rail_t * 0.5), 0.0, half_h),
            material=rail, bevel=0.014,
        ))

    # --- two mounting brackets, standing off the back face ---------------
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        x = sx * width * 0.36
        parts += _place(
            [K.cube(f"{asset_id}_bracket_{tag}", size=(0.09, 0.36, 0.48),
                    location=(0.0, 0.0, 0.0), material=bracket, bevel=0.014)],
            rot_deg=(-24.0, 0.0, 0.0),
            offset=(x, depth * 0.5 + 0.10, 0.30),
        )
        parts.append(K.cube(
            f"{asset_id}_bracket_plate_{tag}", size=(0.32, 0.055, 0.36),
            location=(x, depth * 0.5 + 0.26, 0.56), material=bracket, bevel=0.014,
        ))

    # --- original tag art: flat plates pinned to the face -----------------
    # (w, h, x, z, roll degrees, rgb)
    marks = (
        (0.98, 0.58, 0.02, 1.16, -8.0, (0.92, 0.22, 0.58)),
        (0.68, 0.78, -0.56, 1.10, 13.0, (0.16, 0.78, 0.86)),
        (0.56, 0.52, 0.62, 1.30, 21.0, (0.98, 0.82, 0.20)),
        (0.40, 0.86, 0.22, 0.84, -15.0, (0.96, 0.50, 0.14)),
        (0.30, 0.32, -0.88, 1.42, 27.0, (0.94, 0.94, 0.90)),
        (0.48, 0.28, 0.54, 0.62, -6.0, (0.24, 0.72, 0.36)),
        (0.24, 0.24, -0.32, 0.54, 19.0, (0.94, 0.42, 0.66)),
        (0.86, 0.065, -0.10, 1.44, -23.0, (0.10, 0.10, 0.12)),
        (0.70, 0.055, 0.30, 0.98, 9.0, (0.10, 0.10, 0.12)),
        (0.54, 0.050, -0.46, 0.74, -12.0, (0.96, 0.94, 0.88)),
    )
    for i, (w, h, x, z, roll, rgb) in enumerate(marks):
        parts += _place(
            [K.cube(f"{asset_id}_tag_{i}", size=(w, 0.024, h),
                    location=(0.0, 0.0, 0.0),
                    material=P.paint(f"{asset_id}_tag{i}", rgb, roughness=0.44),
                    bevel=0.006, bevel_segments=1)],
            rot_deg=(0.0, roll, 0.0),
            offset=(x, face_y - 0.008, z),
        )

    # --- fixings ----------------------------------------------------------
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        for i, z in enumerate((0.16, half_h, height - 0.16)):
            b = K.cylinder(
                f"{asset_id}_bolt_{tag}{i}", radius=0.028, depth=0.03,
                vertices=8, location=(0.0, 0.0, 0.0), material=bolt, smooth=True,
            )
            b.rotation_euler = (math.radians(90.0), 0.0, 0.0)
            K.transform_apply(b)
            b.location = (sx * width * 0.42, face_y - 0.012, z)
            K.transform_apply(b)
            parts.append(b)

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 4. misc_helipad_marking
# ---------------------------------------------------------------------------


def build_helipad_marking(
    asset_id: str,
    *,
    radius: float = 2.20,
    band: float = 0.30,
    thickness: float = 0.020,
    segments: int = 18,
) -> list[bpy.types.Object]:
    """A ground marking: a ~20 mm wafer of closed solids lying flat.

    Everything is built from closed, wafer-thin boxes and one low cylinder, so
    the outwardness check decides every element by signed volume and no
    `outward` declaration is required — a decal cannot afford to be an open
    surface, because a flat plane's signed volume is noise.

    The band is `segments` radial plates rather than a second disc one level
    higher: paint is a 2D band on a pad, and a stacked disc reads as a wedding
    cake. This is deliberately a decal-scale asset (a few hundred triangles),
    so the bevels are scaled to the 20 mm thickness instead of the 2 cm
    chamfer the big props use.
    """
    K.reset_scene()

    pad = P.paint(f"{asset_id}_pad", (0.29, 0.30, 0.32), roughness=0.86)
    paint = P.paint(f"{asset_id}_paint", (0.93, 0.93, 0.88), roughness=0.70)
    accent = P.paint(f"{asset_id}_accent", (0.95, 0.62, 0.16), roughness=0.68)

    parts: list[bpy.types.Object] = []

    parts.append(K.cylinder(
        f"{asset_id}_pad", radius=radius, depth=thickness, vertices=44,
        location=(0.0, 0.0, thickness * 0.5), material=pad, smooth=True,
    ))

    r_mid = radius * 0.845
    plates: list[bpy.types.Object] = []
    for i in range(segments):
        ang = 2.0 * math.pi * i / segments
        plates.append(_radial_slab(
            f"{asset_id}_band_{i}",
            size=(band, 2.0 * math.pi * r_mid / segments * 1.06, thickness * 0.8),
            ang_rad=ang, radius=r_mid, z=thickness + thickness * 0.4,
            material=paint, bevel=0.005, bevel_segments=1,
        ))
    parts.append(_merge(f"{asset_id}_band", plates))

    # an invented centre mark: a bar and two chevrons, not a registered letter
    marks: list[bpy.types.Object] = []
    marks.append(K.cube(
        f"{asset_id}_mark_bar", size=(0.34, 1.30, thickness * 0.7),
        location=(0.0, 0.0, thickness + thickness * 0.35),
        material=paint, bevel=0.005, bevel_segments=1,
    ))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        marks.append(K.cube(
            f"{asset_id}_mark_leg_{tag}", size=(0.28, 1.30, thickness * 0.7),
            location=(sx * 0.50, 0.0, thickness + thickness * 0.35),
            material=paint, bevel=0.005, bevel_segments=1,
        ))
    for i in range(4):
        ang = math.radians(45.0 + 90.0 * i)
        marks.append(_radial_slab(
            f"{asset_id}_tick_{i}", size=(0.34, 0.11, thickness * 0.7),
            ang_rad=ang, radius=radius * 0.93,
            z=thickness + thickness * 0.35,
            material=accent, bevel=0.005, bevel_segments=1,
        ))
    parts.append(_merge(f"{asset_id}_marks", marks))

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 5. misc_helicopter
# ---------------------------------------------------------------------------


def build_helicopter(
    asset_id: str,
    *,
    length: float = 11.0,
    hull_rgb=(0.90, 0.87, 0.79),
    accent_rgb=(0.90, 0.34, 0.30),
    stripe_rgb=(0.16, 0.42, 0.52),
    blades: int = 4,
) -> list[bpy.types.Object]:
    """A light utility helicopter, ~11 m nose to tail-fin.

    ORIGINAL design: a cream hull with one coral shoulder stripe and a teal
    boom band. No national roundel, no registration, no operator mark and no
    silhouette copied off any real airframe — this is a generic short-rotor
    utility bird that happens to be a readable silhouette at gameplay
    distance.

    Massing rules that make it read as a helicopter rather than a plane with a
    propeller:

    * the rotor mast sits ABOVE and FORWARD of the cabin hump, not on the
      nose;
    * the rotor disc is wider than the fuselage, which is what any real
      short-rotor machine looks like from the front;
    * the skids stand clear of the belly on two cross-tubes with four splayed
      legs, so the fuselage appears to hang above the ground;
    * the tail rotor is in the Y-Z plane on the fin's leading edge, and the
      fin is canted 12 degrees aft.
    """
    K.reset_scene()

    hull = P.paint(f"{asset_id}_hull", hull_rgb, roughness=0.26, metallic=0.12)
    accent = P.paint(f"{asset_id}_accent", accent_rgb, roughness=0.24, metallic=0.15)
    stripe = P.paint(f"{asset_id}_stripe", stripe_rgb, roughness=0.30, metallic=0.2)
    dark = P.paint(f"{asset_id}_dark", (0.10, 0.11, 0.13), roughness=0.52, metallic=0.3)
    steel = P.metal(f"{asset_id}_steel", (0.60, 0.62, 0.66), rough=0.34, metal=0.92)
    rotor_mat = P.paint(f"{asset_id}_rotor", (0.14, 0.15, 0.17), roughness=0.42, metallic=0.35)
    glazing = P.glass(f"{asset_id}_glass")
    nav = P.emissive(f"{asset_id}_nav", (1.0, 0.22, 0.16), strength=4.0)
    beacon = P.emissive(f"{asset_id}_beacon", (1.0, 0.85, 0.35), strength=3.0)

    # -----------------------------------------------------------------
    # PROPORTIONS
    #
    # Everything below is in metres and is scaled by `s` from the requested
    # overall length. They are NOT `length * k` multipliers on individual
    # parts: those look harmless and are not, because every body part EXTENDS
    # past the anchor it is placed on. Scaling each piece independently drifts
    # the nose sphere forward of the nose anchor and the skids aft of the
    # boom, so a length=11.0 helicopter measures 12.5 m nose to tail.
    #
    # The single anchor rule here: `nose` is the Y of the very front of the
    # aircraft and `tail` is the Y of the very back of the fin, so
    # `tail - nose == length` holds by construction.
    # -----------------------------------------------------------------
    s = length / 11.0                      # 1.0 at the nominal 11 m bird

    nose = -length * 0.50                 # front of the nose cone
    tail = nose + length                  # rear of the tail fin

    cabin_y = nose + length * 0.215       # centre of the cabin hump
    cabin_rx = 0.90 * s                   # fuselage half-width
    cabin_ry = length * 0.215
    cabin_rz = 0.83 * s

    canopy_y = nose + length * 0.105
    fairing_y = nose + length * 0.255

    boom_y0 = nose + length * 0.385       # boom root, at the back of the cabin
    boom_y1 = tail - length * 0.135       # boom tip, inside the fin
    boom_len = boom_y1 - boom_y0
    boom_z = 1.72 * s

    mast_y = nose + length * 0.355        # forward of cabin centre, over the hump
    rotor_z = 2.62 * s                    # the rotor disc plane
    disc = length * 0.94                  # rotor diameter, 10.3 m at nominal

    skid_z = 0.0
    skid_y0 = nose + length * 0.115       # toe, well under the nose
    skid_len = length * 0.475

    parts: list[bpy.types.Object] = []

    # --- fuselage ---------------------------------------------------------
    cab = K.sphere(
        f"{asset_id}_cabin", radius=1.0, segments=22, ring_count=11,
        location=(0.0, cabin_y, 1.28 * s), material=hull,
        scale=(cabin_rx, cabin_ry, cabin_rz),
    )
    parts.append(cab)

    # Nose cone: its leading edge lands exactly on `nose`, so the radius
    # offset below IS its length. Placing the sphere at `nose` and letting it
    # extend forward is what pushed the real length to 12.5 m.
    nose_r = length * 0.072
    parts.append(K.sphere(
        f"{asset_id}_nose", radius=1.0, segments=18, ring_count=9,
        location=(0.0, nose + nose_r, 1.24 * s), material=hull,
        scale=(length * 0.049, nose_r, length * 0.043),
    ))

    # The shoulder band is a flattened torus-section laid over the cabin: a
    # separate coloured volume rather than a recolour, because per-face colour
    # does not exist in this pipeline.
    parts.append(K.torus(
        f"{asset_id}_shoulder", major_radius=cabin_rx * 0.99,
        minor_radius=length * 0.013, major_segments=26, minor_segments=8,
        location=(0.0, nose + length * 0.135, 1.62 * s), material=accent,
    ))

    # --- cockpit glazing --------------------------------------------------
    parts.append(K.sphere(
        f"{asset_id}_cockpit_glass", radius=1.0, segments=20, ring_count=10,
        location=(0.0, canopy_y, 1.62 * s), material=glazing,
        scale=(length * 0.062, length * 0.095, length * 0.050),
    ))
    parts.append(K.cube(
        f"{asset_id}_canopy_frame", size=(length * 0.126, 0.05, 0.055),
        location=(0.0, nose + length * 0.043, 1.56 * s),
        material=dark, bevel=0.012,
    ))

    # --- engine fairing + mast --------------------------------------------
    parts.append(K.sphere(
        f"{asset_id}_fairing", radius=1.0, segments=16, ring_count=8,
        location=(0.0, fairing_y, 1.86 * s), material=hull,
        scale=(length * 0.044, length * 0.082, length * 0.036),
    ))

    parts.append(K.cylinder(
        f"{asset_id}_mast", radius=length * 0.012, depth=rotor_z - 1.86 * s,
        vertices=14, location=(0.0, mast_y, (rotor_z + 1.86 * s) * 0.5),
        material=steel, smooth=True,
    ))
    parts.append(K.cone(
        f"{asset_id}_hub_cap", radius1=length * 0.026, radius2=length * 0.009,
        depth=0.34 * s, vertices=14, location=(0.0, mast_y, rotor_z + 0.30 * s),
        material=dark, smooth=True,
    ))

    # --- main rotor -------------------------------------------------------
    parts.append(K.cylinder(
        f"{asset_id}_rotor_hub", radius=length * 0.030, depth=0.24 * s,
        vertices=14, location=(0.0, mast_y, rotor_z), material=steel, smooth=True,
    ))
    parts += _ring_blades(
        f"{asset_id}_rotor", centre=(0.0, mast_y, rotor_z),
        radius=disc * 0.5, count=max(3, blades),
        blade_size=(0.0, length * 0.026, 0.05),
        tilt_deg=-2.0, axis="z", material=rotor_mat, bevel=0.018,
    )
    for i in range(max(3, blades)):
        # Phase must match `_ring_blades` (it sweeps from 0 with no offset), or
        # the grip hangs in the empty gap between two blades instead of on one.
        ang = 2.0 * math.pi * i / max(3, blades)
        parts.append(K.cylinder(
            f"{asset_id}_rotor_grip_{i}", radius=length * 0.014,
            depth=0.22 * s, vertices=10,
            location=(disc * 0.06 * math.cos(ang),
                      mast_y + disc * 0.06 * math.sin(ang),
                      rotor_z - disc * 0.06 * math.sin(math.radians(2.0))),
            material=steel, smooth=True,
        ))

    # --- tail boom, fin, stabiliser, tail rotor --------------------------
    boom = K.cone(
        f"{asset_id}_boom", radius1=length * 0.028, radius2=length * 0.010,
        depth=boom_len, vertices=14, location=(0.0, 0.0, 0.0), material=hull,
        smooth=True,
    )
    boom.rotation_euler = (math.radians(-90.0), 0.0, 0.0)
    K.transform_apply(boom)
    boom.location = (0.0, boom_y0 + boom_len * 0.5, boom_z)
    K.transform_apply(boom)
    parts.append(boom)

    band = K.cylinder(
        f"{asset_id}_boom_band", radius=length * 0.0205, depth=length * 0.042,
        vertices=14, location=(0.0, 0.0, 0.0), material=stripe, smooth=True,
    )
    band.rotation_euler = (math.radians(-90.0), 0.0, 0.0)
    K.transform_apply(band)
    band.location = (0.0, boom_y0 + boom_len * 0.34, boom_z + 0.035 * s)
    K.transform_apply(band)
    parts.append(band)

    # Fin: canted 13 deg aft, with its trailing edge on the `tail` anchor.
    fin_len_y = length * 0.098
    fin_len_z = length * 0.150
    fin_cant = -13.0
    fin_cy = tail - fin_len_y * 0.42
    fin_cz = boom_z + fin_len_z * 0.40
    fin = K.cube(
        f"{asset_id}_fin", size=(0.12, fin_len_y, fin_len_z),
        location=(0.0, 0.0, 0.0), material=hull, bevel=0.05, bevel_segments=3,
    )
    parts += _place([fin], rot_deg=(fin_cant, 0.0, 0.0),
                    offset=(0.0, fin_cy, fin_cz))
    K.shade_smooth(fin, angle=math.radians(40.0))

    parts += _place(
        [K.cube(f"{asset_id}_fin_cap", size=(0.10, fin_len_y * 0.74, fin_len_z * 0.26),
                location=(0.0, 0.0, 0.0), material=accent, bevel=0.03)],
        rot_deg=(fin_cant, 0.0, 0.0),
        offset=(0.0, fin_cy - fin_len_y * 0.06, fin_cz + fin_len_z * 0.50),
    )

    stab_y = boom_y0 + boom_len * 0.80
    parts.append(K.cube(
        f"{asset_id}_stabiliser", size=(length * 0.150, 0.28, 0.085),
        location=(0.0, stab_y, boom_z - 0.12 * s),
        material=hull, bevel=0.025,
    ))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(
            f"{asset_id}_stab_endplate_{tag}", size=(0.07, 0.32, 0.22),
            location=(sx * length * 0.072, stab_y, boom_z - 0.06 * s),
            material=accent, bevel=0.02,
        ))

    tail_hub = (-length * 0.024, fin_cy - fin_len_y * 0.22, boom_z + fin_len_z * 0.42)
    tr_hub = K.cylinder(
        f"{asset_id}_tail_hub", radius=length * 0.011, depth=0.26 * s,
        vertices=10, location=(0.0, 0.0, 0.0), material=steel, smooth=True,
    )
    tr_hub.rotation_euler = (0.0, math.radians(90.0), 0.0)
    K.transform_apply(tr_hub)
    tr_hub.location = tail_hub
    K.transform_apply(tr_hub)
    parts.append(tr_hub)
    parts += _ring_blades(
        f"{asset_id}_tail_rotor", centre=tail_hub, radius=length * 0.100,
        count=3, blade_size=(0.05, 0.19, 0.0), tilt_deg=0.0, axis="x",
        material=rotor_mat, bevel=0.014,
    )

    # --- landing skids ----------------------------------------------------
    # `track` is the outer skid offset; the legs splay INWARD from it to the
    # belly, which is what makes the machine look like it is standing rather
    # than hovering on two rails.
    track = length * 0.108
    skid_cy = skid_y0 + skid_len * 0.5
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        x = sx * track
        parts.append(K.cube(
            f"{asset_id}_skid_{tag}", size=(0.13, skid_len, 0.15),
            location=(x, skid_cy, skid_z + 0.075), material=steel, bevel=0.05,
            bevel_segments=3,
        ))
        # Toe: the skid turns up at the front, ending at skid_y0.
        parts += _place(
            [K.cube(f"{asset_id}_skid_toe_{tag}",
                    size=(0.13, length * 0.080, 0.15),
                    location=(0.0, 0.0, 0.0), material=steel, bevel=0.05,
                    bevel_segments=3)],
            rot_deg=(-30.0, 0.0, 0.0),
            offset=(x, skid_y0 + length * 0.020, skid_z + 0.30),
        )
        parts.append(K.cube(
            f"{asset_id}_skid_pad_{tag}", size=(0.19, length * 0.095, 0.055),
            location=(x, skid_cy + skid_len * 0.08, skid_z + 0.028),
            material=dark, bevel=0.016,
        ))
        for i, fy in enumerate((-0.30, 0.24)):
            parts += _place(
                [K.cube(f"{asset_id}_leg_{tag}{i}", size=(0.08, 0.10, 0.30),
                        location=(0.0, 0.0, 0.0), material=steel, bevel=0.02)],
                rot_deg=(0.0, sx * 26.0, 0.0),
                offset=(x * 0.60, skid_cy + skid_len * fy, skid_z + 0.30),
            )
        parts.append(K.sphere(
            f"{asset_id}_nav_{tag}", radius=0.05, segments=10, ring_count=6,
            location=(x + sx * 0.03, skid_y0 + 0.12, 0.36),
            material=nav if sx < 0 else beacon,
        ))

    for i, fy in enumerate((-0.30, 0.24)):
        cross = K.cylinder(
            f"{asset_id}_cross_tube_{i}", radius=0.06, depth=track * 1.55,
            vertices=10, location=(0.0, 0.0, 0.0), material=steel, smooth=True,
        )
        cross.rotation_euler = (0.0, math.radians(90.0), 0.0)
        K.transform_apply(cross)
        cross.location = (0.0, skid_cy + skid_len * fy, skid_z + 0.30)
        K.transform_apply(cross)
        parts.append(cross)

    # --- exhaust ----------------------------------------------------------
    exh = K.cylinder(
        f"{asset_id}_exhaust", radius=length * 0.015, depth=length * 0.050,
        vertices=12, location=(0.0, 0.0, 0.0), material=steel, smooth=True,
    )
    exh.rotation_euler = (math.radians(96.0), 0.0, 0.0)
    K.transform_apply(exh)
    exh.location = (length * 0.028, fairing_y + length * 0.045, 1.90 * s)
    K.transform_apply(exh)
    parts.append(exh)

    parts.append(K.sphere(
        f"{asset_id}_tail_beacon", radius=0.048, segments=10, ring_count=6,
        location=(0.0, fin_cy - fin_len_y * 0.42, fin_cz + fin_len_z * 0.52),
        material=nav,
    ))

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 6. misc_street_phone
# ---------------------------------------------------------------------------


def build_street_phone(
    asset_id: str,
    *,
    shell_rgb=(0.86, 0.30, 0.24),
    trim_rgb=(0.95, 0.93, 0.86),
    height: float = 2.42,
) -> list[bpy.types.Object]:
    """A classic glazed booth: frame, glazed panels, roof, handset.

    Four corner posts carry a plinth, three glazed walls and a roof with a
    fascia and a finial; the fourth side (-Y) is the doorway, so the camera
    can see in and find the handset. The handset hangs on the back panel in a
    cradle with a coiled cord, because a phone with no handset is a cabinet.
    """
    K.reset_scene()

    shell = P.paint(f"{asset_id}_shell", shell_rgb, roughness=0.46, metallic=0.15)
    trim = P.paint(f"{asset_id}_trim", trim_rgb, roughness=0.52)
    steel = P.metal(f"{asset_id}_steel", (0.56, 0.58, 0.61), rough=0.36, metal=0.9)
    dark = P.paint(f"{asset_id}_dark", (0.10, 0.10, 0.12), roughness=0.48)
    glazing = P.glass(f"{asset_id}_glass")
    dial = P.emissive(f"{asset_id}_dial", (0.35, 0.85, 0.80), strength=2.0)

    half_x, half_y = 0.60, 0.54
    post_h = height - 0.30
    parts: list[bpy.types.Object] = []

    parts.append(K.cube(
        f"{asset_id}_plinth", size=(half_x * 2.2, half_y * 2.2, 0.12),
        location=(0.0, 0.0, 0.06), material=trim, bevel=0.02,
    ))

    for sx, tx in ((-1.0, "l"), (1.0, "r")):
        for sy, ty in ((-1.0, "f"), (1.0, "b")):
            parts.append(K.cube(
                f"{asset_id}_post_{tx}{ty}", size=(0.10, 0.10, post_h),
                location=(sx * half_x, sy * half_y, 0.12 + post_h * 0.5),
                material=shell, bevel=0.018,
            ))

    # --- glazed panels ----------------------------------------------------
    glass_h = post_h - 0.34
    glass_z = 0.12 + 0.16 + glass_h * 0.5
    parts.append(K.cube(
        f"{asset_id}_glass_back", size=(half_x * 2.0, 0.040, glass_h),
        location=(0.0, half_y - 0.02, glass_z), material=glazing,
    ))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(
            f"{asset_id}_glass_side_{tag}", size=(0.040, half_y * 1.78, glass_h),
            location=(sx * (half_x - 0.02), 0.0, glass_z), material=glazing,
        ))
    # a half-height door leaf swung in on the -X side of the opening
    parts += _place(
        [K.cube(f"{asset_id}_glass_door", size=(0.040, half_y * 0.80, glass_h * 0.82),
                location=(0.0, 0.0, 0.0), material=glazing)],
        rot_deg=(0.0, 0.0, -26.0),
        offset=(-half_x * 0.52, -half_y * 0.52, glass_z - glass_h * 0.09),
    )

    # --- roof -------------------------------------------------------------
    parts.append(K.cube(
        f"{asset_id}_roof", size=(half_x * 2.6, half_y * 2.6, 0.15),
        location=(0.0, 0.0, height - 0.075), material=shell, bevel=0.035,
        bevel_segments=3,
    ))
    fascia = 0.11
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(
            f"{asset_id}_fascia_{tag}", size=(0.07, half_y * 2.6, fascia),
            location=(sx * half_x * 1.3, 0.0, height - 0.20),
            material=trim, bevel=0.014,
        ))
        parts.append(K.cube(
            f"{asset_id}_fascia_{tag}f", size=(half_x * 2.6, 0.07, fascia),
            location=(0.0, sx * half_y * 1.3, height - 0.20),
            material=trim, bevel=0.014,
        ))
    finial = K.cone(
        f"{asset_id}_finial", radius1=0.09, radius2=0.015, depth=0.20, vertices=10,
        location=(0.0, 0.0, height + 0.10), material=steel, smooth=True,
    )
    parts.append(finial)

    # --- payphone unit on the back panel ----------------------------------
    unit_z = glass_z + glass_h * 0.18
    parts.append(K.cube(
        f"{asset_id}_unit", size=(0.38, 0.19, 0.54),
        location=(0.13, half_y - 0.13, unit_z), material=dark, bevel=0.022,
    ))
    parts.append(K.cube(
        f"{asset_id}_keypad", size=(0.15, 0.028, 0.20),
        location=(0.09, half_y - 0.235, unit_z - 0.03), material=trim, bevel=0.008,
    ))
    parts.append(K.cube(
        f"{asset_id}_number_strip", size=(0.28, 0.026, 0.065),
        location=(0.13, half_y - 0.235, unit_z + 0.16), material=dial, bevel=0.008,
    ))
    parts.append(K.cube(
        f"{asset_id}_coin_door", size=(0.07, 0.026, 0.19),
        location=(0.29, half_y - 0.235, unit_z - 0.02), material=trim, bevel=0.008,
    ))

    # --- handset in its cradle, with a coiled cord ------------------------
    cradle_z = unit_z - 0.36
    parts.append(K.cube(
        f"{asset_id}_cradle", size=(0.15, 0.11, 0.06),
        location=(0.13, half_y - 0.245, cradle_z + 0.06), material=trim, bevel=0.012,
    ))
    body = K.cube(
        f"{asset_id}_handset", size=(0.085, 0.085, 0.24), location=(0.0, 0.0, 0.0),
        material=dark, bevel=0.03, bevel_segments=3,
    )
    parts += _place([body], rot_deg=(7.0, 0.0, 0.0),
                    offset=(0.13, half_y - 0.285, cradle_z - 0.08))
    for i, (dz, cap) in enumerate(((0.115, 0.135), (-0.115, 0.115))):
        cap_obj = K.cube(
            f"{asset_id}_handset_cap_{i}", size=(cap, cap, 0.075),
            location=(0.0, 0.0, 0.0), material=dark, bevel=0.026, bevel_segments=3,
        )
        parts += _place([cap_obj], rot_deg=(7.0, 0.0, 0.0),
                        offset=(0.13, half_y - 0.285, cradle_z - 0.08 + dz))

    cord: list[bpy.types.Object] = []
    for i in range(6):
        t = i / 5.0
        link = K.cube(
            f"{asset_id}_cord_{i}", size=(0.028, 0.028, 0.075),
            location=(0.0, 0.0, 0.0), material=dark, bevel=0.008,
        )
        cord += _place(
            [link],
            rot_deg=(0.0, math.degrees(16.0 * math.cos(math.pi * (i + 0.5) * 1.2)), 0.0),
            offset=(0.13 + 0.055 * math.sin(math.pi * i),
                    half_y - 0.30 + 0.05 * math.sin(math.pi * 1.4 * i),
                    cradle_z - 0.22 - 0.055 * t),
        )
    parts += cord

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 7. marker_coin_ring
# ---------------------------------------------------------------------------


def build_marker_coin_ring(
    asset_id: str,
    *,
    ring_radius: float = 0.86,
    coins: int = 12,
    coin_radius: float = 0.135,
    plate_rgb=(0.12, 0.84, 0.76),
    gold_rgb=(0.96, 0.76, 0.26),
    beacon_rgb=(0.30, 0.88, 1.00),
) -> list[bpy.types.Object]:
    """A pickup marker: a glowing ring plate, upright coins, a central beacon.

    Everything on it is emissive, because a marker that is only visible from
    twenty metres is not a marker. Emission strength falls off outward —
    the beacon column burns hardest, the plate carries most of the read at
    distance, and the coins glow just enough to separate as objects.

    The coins stand UPRIGHT on their rims, which is why their centres sit one
    COIN_RADIUS above the plate and not one half-thickness: a coin is a disc
    and it touches the ground with the bottom of its rim.
    """
    K.reset_scene()

    plate_mat = P.emissive(f"{asset_id}_plate", plate_rgb, strength=3.2)
    band_mat = P.emissive(f"{asset_id}_band", (0.20, 0.95, 0.88), strength=5.0)
    coin_mat = P.emissive(f"{asset_id}_coin", gold_rgb, strength=2.4)
    rim_mat = P.emissive(f"{asset_id}_rim", (0.72, 0.52, 0.14), strength=2.0)
    beacon_mat = P.emissive(f"{asset_id}_beacon", beacon_rgb, strength=9.0)
    finial_mat = P.emissive(f"{asset_id}_finial", (0.85, 0.98, 1.00), strength=12.0)

    plate_t = 0.030
    plate_r = ring_radius * 1.52
    band_w = 0.26
    band_t = 0.024
    band_top = plate_t + band_t
    coin_cz = band_top + coin_radius

    parts: list[bpy.types.Object] = []

    # --- ground plate ------------------------------------------------------
    parts.append(K.cylinder(
        f"{asset_id}_plate", radius=plate_r, depth=plate_t, vertices=40,
        location=(0.0, 0.0, plate_t * 0.5), material=plate_mat, smooth=True,
    ))

    band_seg = max(12, coins * 2)
    plates: list[bpy.types.Object] = []
    for i in range(band_seg):
        ang = 2.0 * math.pi * i / band_seg
        plates.append(_radial_slab(
            f"{asset_id}_band_{i}",
            size=(band_w, 2.0 * math.pi * ring_radius / band_seg * 1.06, band_t),
            ang_rad=ang, radius=ring_radius,
            z=plate_t + band_t * 0.5,
            material=band_mat, bevel=0.006, bevel_segments=1,
        ))
    parts.append(_merge(f"{asset_id}_band", plates))

    # --- coins, facing radially so the ring reads from any approach -------
    for i in range(coins):
        ang = 2.0 * math.pi * i / coins
        rim = K.cylinder(
            f"{asset_id}_coin_rim_{i}", radius=coin_radius, depth=0.028,
            vertices=12, location=(0.0, 0.0, 0.0), material=rim_mat, smooth=True,
        )
        face = K.cylinder(
            f"{asset_id}_coin_face_{i}", radius=coin_radius * 0.70, depth=0.046,
            vertices=10, location=(0.0, 0.0, 0.0), material=coin_mat, smooth=True,
        )
        # (ang - 90) maps the coin's +Y face normal onto the outward radial
        # direction, so every coin looks the same way round the ring.
        _place([rim, face], rot_deg=(0.0, 0.0, math.degrees(ang) - 90.0),
               offset=(ring_radius * math.cos(ang), ring_radius * math.sin(ang), coin_cz))
        parts += [rim, face]

    # --- central beacon ---------------------------------------------------
    parts.append(K.cylinder(
        f"{asset_id}_hub", radius=0.42, depth=0.06, vertices=24,
        location=(0.0, 0.0, plate_t + 0.03), material=plate_mat, smooth=True,
    ))
    mast = K.cone(
        f"{asset_id}_mast", radius1=0.30, radius2=0.14, depth=1.05, vertices=14,
        location=(0.0, 0.0, plate_t + 0.06 + 0.525), material=band_mat, smooth=True,
    )
    parts.append(mast)
    parts.append(K.cone(
        f"{asset_id}_beacon", radius1=0.17, radius2=0.0, depth=0.30, vertices=14,
        location=(0.0, 0.0, plate_t + 0.06 + 1.05 + 0.15),
        material=beacon_mat, smooth=True,
    ))
    parts.append(K.cone(
        f"{asset_id}_finial", radius1=0.09, radius2=0.0, depth=0.20, vertices=12,
        location=(0.0, 0.0, plate_t + 0.06 + 1.05 + 0.30 + 0.08),
        material=finial_mat, smooth=True,
    ))

    # --- three leaning chevrons so the disc is not a bare plate -----------
    chevrons: list[bpy.types.Object] = []
    for i in range(3):
        ang = 2.0 * math.pi * i / 3.0
        chevrons.append(_radial_slab(
            f"{asset_id}_chevron_{i}", size=(0.62, 0.13, 0.014),
            ang_rad=ang, radius=plate_r * 0.62,
            z=plate_t + 0.007, material=band_mat, bevel=0.005, bevel_segments=1,
        ))
    parts.append(_merge(f"{asset_id}_chevrons", chevrons))

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 8. marker_flame
# ---------------------------------------------------------------------------


def build_marker_flame(
    asset_id: str,
    *,
    height: float = 2.35,
    base_radius: float = 0.34,
    segments: int = 7,
    core_rgb=(1.00, 0.88, 0.58),
    mid_rgb=(1.00, 0.56, 0.12),
    outer_rgb=(0.98, 0.28, 0.06),
    base_rgb=(0.24, 0.23, 0.22),
) -> list[bpy.types.Object]:
    """A tall objective beacon: stacked tapered cones, strongly emissive.

    Seven closed cone segments stacked from the collar to the tip. Closed
    solids rather than one lathe so that every segment can carry its OWN
    emissive material — a part is the unit of material in glTF, so a single
    mesh could not fade from a white-hot core to a dark red tip, and a
    uniform glow is precisely what makes a flame column look like a plastic
    traffic cone.

    The stack is asymmetric on purpose: a perfectly straight lathe reads as a
    party hat. Three leaning tongues and four drifting embers break the
    silhouette, and the segment radii follow a `1/sqrt(t)`-ish falloff so the
    column tapers like a flame rather than a cone.
    """
    K.reset_scene()

    core = P.emissive(f"{asset_id}_core", core_rgb, strength=16.0)
    mid = P.emissive(f"{asset_id}_mid", mid_rgb, strength=10.0)
    outer = P.emissive(f"{asset_id}_outer", outer_rgb, strength=6.0)
    ember = P.emissive(f"{asset_id}_ember", (1.00, 0.70, 0.22), strength=7.0)
    plinth = P.metal(f"{asset_id}_plinth", base_rgb, rough=0.55, metal=0.55)
    collar = P.metal(f"{asset_id}_collar", (0.42, 0.41, 0.40), rough=0.42, metal=0.85)

    plinth_h = 0.10
    collar_h = 0.06
    z0 = plinth_h + collar_h
    z1 = height
    r0 = base_radius
    r1 = base_radius * 0.16
    n = max(4, int(segments))

    def radius_at(t: float) -> float:
        """Flame falloff: fast at the base, long and thin toward the tip."""
        return r0 + (r1 - r0) * (t ** 0.68)

    parts: list[bpy.types.Object] = []

    parts.append(K.cylinder(
        f"{asset_id}_plinth", radius=0.48, depth=plinth_h, vertices=24,
        location=(0.0, 0.0, plinth_h * 0.5), material=plinth, smooth=True,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_collar", radius=0.40, depth=collar_h, vertices=20,
        location=(0.0, 0.0, plinth_h + collar_h * 0.5), material=collar, smooth=True,
    ))

    # The stack: `n` cones sharing their end caps, hottest at the tip.
    #
    # The material is chosen from the taper POSITION, not from the segment
    # index. Indexing a fixed seven-entry ladder by `i % 7` ties the hot core
    # to whatever count the caller happened to pass: at segments=9 the tip
    # lands back on `outer` and the flame goes dark exactly where it should be
    # brightest. Keying on t means the gradient survives any retune of n.
    def band_at(t: float) -> bpy.types.Material:
        if t < 0.34:
            return outer
        if t < 0.70:
            return mid
        return core

    for i in range(n):
        t0, t1 = i / n, (i + 1) / n
        za = z0 + (z1 - z0) * t0
        zb = z0 + (z1 - z0) * t1
        seg = K.cone(
            f"{asset_id}_stack_{i}", radius1=radius_at(t0), radius2=radius_at(t1),
            depth=zb - za, vertices=14,
            location=(0.0, 0.0, (za + zb) * 0.5),
            material=band_at((t0 + t1) * 0.5), smooth=True,
        )
        parts.append(seg)

    # --- inner core column, burning through the stack --------------------
    parts.append(K.cone(
        f"{asset_id}_core", radius1=r0 * 0.52, radius2=0.0, depth=(z1 - z0) * 0.72,
        vertices=14, location=(0.0, 0.0, z0 + (z1 - z0) * 0.36),
        material=core, smooth=True,
    ))

    # --- leaning tongues --------------------------------------------------
    tongues = (
        (0.13, 0.07, -14.0, 13.0, outer),
        (-0.10, -0.11, 17.0, -10.0, mid),
        (0.04, 0.16, -9.0, -18.0, core),
    )
    for i, (dx, dy, rx, ry, mat) in enumerate(tongues):
        t = K.cone(
            f"{asset_id}_tongue_{i}", radius1=0.055, radius2=0.0, depth=0.34,
            vertices=10, location=(0.0, 0.0, 0.0), material=mat, smooth=True,
        )
        parts += _place([t], rot_deg=(rx, ry, 0.0),
                        offset=(dx, dy, z1 - 0.12 + 0.10 * i))

    # --- drifting embers --------------------------------------------------
    embers = ((0.30, 0.16, z1 - 0.05, 0.040),
              (-0.24, -0.22, z1 + 0.14, 0.032),
              (0.12, -0.34, z1 - 0.22, 0.028),
              (-0.36, 0.10, z1 - 0.34, 0.026))
    for i, (x, y, z, r) in enumerate(embers):
        parts.append(K.sphere(
            f"{asset_id}_ember_{i}", radius=r, segments=10, ring_count=5,
            location=(x, y, z), material=ember,
        ))

    _ground(parts)
    return P.finish(parts)


# ---------------------------------------------------------------------------
# registries
# ---------------------------------------------------------------------------

MISC: list[tuple[str, object, dict]] = [
    ("misc_awning", build_awning, dict(
        span=2.70, depth=1.45, wall_z=2.10, edge_z=1.60, sag=0.12, ribs=5,
        canvas_rgb=(0.86, 0.22, 0.28), trim_rgb=(0.96, 0.93, 0.84),
        frame_rgb=(0.58, 0.60, 0.63))),
    ("misc_dumpster_pile", build_dumpster_pile, dict(bins=3)),
    ("misc_graffiti_board", build_graffiti_board, dict(width=2.40, height=1.66)),
    ("misc_helicopter", build_helicopter, dict(
        length=11.0, hull_rgb=(0.90, 0.87, 0.79), accent_rgb=(0.90, 0.34, 0.30),
        stripe_rgb=(0.16, 0.42, 0.52), blades=4)),
    ("misc_helipad_marking", build_helipad_marking, dict(
        radius=2.20, band=0.30, thickness=0.020, segments=18)),
    ("misc_street_phone", build_street_phone, dict(
        shell_rgb=(0.86, 0.30, 0.24), trim_rgb=(0.95, 0.93, 0.86), height=2.42)),
]

MARKERS: list[tuple[str, object, dict]] = [
    ("marker_coin_ring", build_marker_coin_ring, dict(
        ring_radius=0.86, coins=12, coin_radius=0.135,
        plate_rgb=(0.12, 0.84, 0.76), gold_rgb=(0.96, 0.76, 0.26),
        beacon_rgb=(0.30, 0.88, 1.00))),
    ("marker_flame", build_marker_flame, dict(
        height=2.35, base_radius=0.34, segments=7,
        core_rgb=(1.00, 0.88, 0.58), mid_rgb=(1.00, 0.56, 0.12),
        outer_rgb=(0.98, 0.28, 0.06))),
]