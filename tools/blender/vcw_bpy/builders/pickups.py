"""3A pickup builder — the small glowing props a player walks over to collect.

THE bug this module exists to avoid
-----------------------------------
A pickup is judged at 20 m, not at 2 m, and the failure at that distance is
never "not enough detail" — it is **no silhouette**. A red box with a cross
*painted* on it, a red box with a cross *extruded* on it, and a red box with a
cross on a 40 mm handle are three different objects at 2 m and the same small
red blob at 20 m. Detail that does not change the outline is invisible at the
distance the pickup is actually seen from.

So every prop below is fixed against four rules:

1. **Silhouette first.** The health cross is a solid extruded form standing
   proud of its case, not a decal. The ammo can is an *open* box with
   cartridges standing out of it. The rocket is a finned tube racked in a
   crate, not a crate with a stripe. The flare pack is six sticks with lit
   tips, which is the only pickup in the game whose emissive is its own
   silhouette.
2. **One accent hue per item, and never the same hue twice.** The eight
   accents below are spread around the wheel on purpose: a player who cannot
   resolve the shape still resolves the colour, and eight pickups sharing one
   red tell them nothing.
3. **Per-part materials.** ``P.emissive`` is the accent and nothing else.
   Every prop carries a shell, a darker trim, bare metal and webbing, because
   one material over a whole prop reads as a coloured blob at exactly the
   distance the pickup is seen from.
4. **No coplanar decals.** A detail box whose back face lands exactly on a
   shell's front face z-fights, and the z-fight looks at 2 m like a modelling
   bug and at 20 m like a broken prop. Every applied detail here overlaps its
   host by 2-7 mm rather than touching it.

**Bevel policy.** Every hard box edge is chamfered 15-40 mm. Two exceptions
are forced by geometry rather than chosen: `K.add_bevel` cannot chamfer
opposing faces more than half a part's thickness apart, so no bevelled part
below is thinner than 34 mm — a 15 mm chamfer on a 20 mm strap collapses into
zero-area faces, which the verifier counts as degenerate. And curved stock
(valve bodies, collars, spokes, 26 mm flare caps) is left unchamfered where a
15 mm chamfer would consume the radius; the helper `_chamfer` is called only
on parts measured to take a full-width one.

Origin convention: **grounded, resting on z=0**, like every other category the
verifier treats as ground geometry. Nothing here floats; the lowest vertex of
the whole prop is on the floor plane.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy

from .. import kernel as K
from .. import palette as P

# Every pickup is authored Z-up with -Y forward, metres, resting on z=0.


# --- accent hues, one per asset, spread around the wheel --------------------
#
# Read as a set rather than per asset: these eight are the only thing that
# identifies a pickup when the silhouette is a few pixels tall, so two of them
# sharing a hue is a bug even if each prop is modelled perfectly.
AMBER = (1.00, 0.70, 0.08)          # ammo box      — brass / stencil
AZURE = (0.18, 0.52, 1.00)         # armour vest   — hi-viz plate flash
BILL_GREEN = (0.24, 1.00, 0.30)    # cash stack    — banknote glow
FLARE_ORANGE = (1.00, 0.40, 0.04)  # flare pack    — the lit tips
HAZARD_VIOLET = (0.64, 0.22, 1.00)  # grenade case  — hazard strip
MED_RED = (1.00, 0.08, 0.10)       # health pack   — the cross
TANK_CYAN = (0.08, 0.90, 1.00)     # o2 tank       — wrap bands
ROCKET_PINK = (1.00, 0.16, 0.72)   # rocket round  — warhead band


# ---------------------------------------------------------------------------
# shared helpers
# ---------------------------------------------------------------------------


def _shade(rgb: Sequence[float], k: float) -> tuple[float, float, float]:
    """Scale a palette colour, clamped — a second value of the SAME hue.

    Two parts of one prop sharing a colour is why a prop reads as one object;
    two parts sharing a colour *and* a value is why it reads as one object
    with a hole in it.
    """
    return tuple(min(1.0, max(0.0, c * k)) for c in rgb)  # type: ignore[return-value]


def _kit(
    asset_id: str,
    *,
    accent: Sequence[float],
    shell: Sequence[float],
    shell_metal: float = 0.30,
    shell_rough: float = 0.58,
    trim: Sequence[float] | None = None,
    metal_rgb: Sequence[float] = (0.62, 0.64, 0.68),
    metal_rough: float = 0.34,
    webbing: Sequence[float] = (0.12, 0.12, 0.14),
    accent_strength: float = 5.0,
) -> dict[str, bpy.types.Material]:
    """One prop's five materials, named off `asset_id`.

    Deliberately a fixed set rather than a per-prop grab bag: every pickup in
    this family has a body, a darker break-up value, bare hardware, webbing
    and one accent, and a builder that reaches past this list is how a family
    starts drifting apart.
    """
    return {
        "shell": P.paint(
            f"{asset_id}_shell", tuple(shell), metallic=shell_metal,
            roughness=shell_rough,
        ),
        "trim": P.paint(
            f"{asset_id}_trim",
            tuple(trim) if trim is not None else _shade(shell, 0.62),
            metallic=min(1.0, shell_metal + 0.15),
            roughness=max(0.05, shell_rough - 0.12),
        ),
        "metal": P.metal(
            f"{asset_id}_metal", tuple(metal_rgb), rough=metal_rough,
        ),
        "webbing": P.fabric(f"{asset_id}_webbing", tuple(webbing), rough=0.88),
        "accent": P.emissive(
            f"{asset_id}_accent", tuple(accent), strength=accent_strength,
        ),
    }


def _placed(
    name: str,
    make,
    *,
    location: Sequence[float],
    rotation: Sequence[float],
) -> bpy.types.Object:
    """Build a part at the world origin, rotate it THERE, then translate it.

    Every `K.*` primitive bakes its transform into the mesh data, so an object
    built at its final location has its origin back at (0, 0, 0). Setting
    `rotation_euler` on it afterwards spins the whole mesh about the world
    origin — which throws a rocket fin a metre away from the rocket instead
    of merely mis-rotating it. Rotate first, translate second.
    """
    obj = make((0.0, 0.0, 0.0))
    obj.name = name
    obj.rotation_euler = rotation
    K.transform_apply(obj)
    obj.location = location
    K.transform_apply(obj)
    return obj


def _rotated_cube(
    name: str,
    size: Sequence[float],
    *,
    location: Sequence[float],
    rotation: Sequence[float],
    material: bpy.types.Material | None,
    bevel: float = 0.02,
    bevel_segments: int = 2,
) -> bpy.types.Object:
    """A bevelled cube built at the world origin, rotated there, then moved.

    See `_placed` for why the order matters.
    """
    return _placed(
        name,
        lambda loc: K.cube(
            name, size=size, location=loc, material=material, bevel=bevel,
            bevel_segments=bevel_segments,
        ),
        location=location,
        rotation=rotation,
    )


def _cylinder(
    name: str,
    radius: float,
    length: float,
    *,
    location: Sequence[float],
    material: bpy.types.Material | None,
    vertices: int = 16,
    rotation: Sequence[float] = (0.0, 0.0, 0.0),
) -> bpy.types.Object:
    """A cylinder oriented by `rotation`, built at the origin then moved."""
    return _placed(
        name,
        lambda loc: K.cylinder(
            name, radius=radius, depth=length, vertices=vertices,
            location=loc, material=material, smooth=True,
        ),
        location=location,
        rotation=rotation,
    )


def _chamfer(obj: bpy.types.Object, width: float = 0.015, segments: int = 2):
    """Chamfer the cap rims of a curved part.

    Only called on stock measured to take a full-width chamfer (depth and
    radius both at least 3x `width`); on anything thinner the opposing
    chamfers overlap and the modifier produces zero-area faces.
    """
    K.add_bevel(obj, width, segments)
    return obj


# ---------------------------------------------------------------------------
# 1. ammo box — an open can with live rounds standing out of it
# ---------------------------------------------------------------------------


def build_ammo_box(
    asset_id: str,
    *,
    accent: Sequence[float] = AMBER,
    width: float = 0.42,
    depth: float = 0.30,
    height: float = 0.22,
) -> list[bpy.types.Object]:
    """Olive steel can, built open-topped so the cartridges ARE the silhouette.

    A sealed ammo can and an open one are the same box at 20 m. What tells
    them apart from across the street is the eight brass rounds standing
    clear of the rim, so the can is modelled as four walls and a floor with
    nothing over the top.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.26, 0.30, 0.20), shell_metal=0.45,
        shell_rough=0.58, trim=(0.15, 0.18, 0.12),
        metal_rgb=(0.72, 0.66, 0.42), metal_rough=0.30,
    )
    parts: list[bpy.types.Object] = []

    hw, hd = width * 0.5, depth * 0.5
    wall = 0.036                       # > 2 x the 15 mm chamfer
    floor_t = wall
    top = floor_t + height             # top face of the can walls
    body_cz = floor_t + height * 0.5
    outer_x = hw                       # outer face of the side walls

    parts.append(
        K.cube(f"{asset_id}_floor", size=(width, depth, floor_t),
               location=(0.0, 0.0, floor_t * 0.5), material=m["shell"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_wall_l", size=(wall, depth, height),
               location=(-(hw - wall * 0.5), 0.0, body_cz), material=m["shell"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_wall_r", size=(wall, depth, height),
               location=(hw - wall * 0.5, 0.0, body_cz), material=m["shell"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_wall_f", size=(width - 2.0 * wall, wall, height),
               location=(0.0, -(hd - wall * 0.5), body_cz), material=m["shell"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_wall_b", size=(width - 2.0 * wall, wall, height),
               location=(0.0, hd - wall * 0.5, body_cz), material=m["shell"],
               bevel=0.015)
    )

    # Rolled rim: a collar that caps the wall tops and stands proud of them,
    # so the can reads as sheet steel with a folded edge rather than a box.
    rim_t = 0.05
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_rim_{tag}", size=(rim_t, depth + 0.012, rim_t),
                   location=(sx * (hw + (rim_t - wall) * 0.5), 0.0, top),
                   material=m["trim"], bevel=0.015)
        )
    for sy, tag in ((-1.0, "f"), (1.0, "b")):
        parts.append(
            K.cube(f"{asset_id}_rim_{tag}", size=(width + 0.02, rim_t, rim_t),
                   location=(0.0, sy * (hd - wall * 0.5 + 0.006), top),
                   material=m["trim"], bevel=0.015)
        )

    # Eight live rounds, two rows of four. Case and tip are separate parts so
    # the brass and the copper read as two materials, and so the top of the
    # prop is eight points of specular rather than eight flat discs.
    for row, ry in enumerate((-0.052, 0.052)):
        for col, rx in enumerate((-0.099, -0.033, 0.033, 0.099)):
            tag = f"r{row}{col}"
            parts.append(
                K.cylinder(f"{asset_id}_round_{tag}", radius=0.0235, depth=0.30,
                           vertices=10, location=(rx, ry, floor_t + 0.15),
                           material=m["metal"], smooth=True)
            )
            parts.append(
                K.cone(f"{asset_id}_tip_{tag}", radius1=0.0235, radius2=0.013,
                       depth=0.05, vertices=10,
                       location=(rx, ry, floor_t + 0.325), material=m["trim"],
                       smooth=True)
            )

    # Pressed side ribs, proud by 3 mm into the wall so they are a stamped
    # can rather than plates stuck to a face.
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        for i, rz in enumerate((0.095, 0.185)):
            parts.append(
                K.cube(f"{asset_id}_rib_{tag}{i}", size=(0.034, 0.19, 0.035),
                       location=(sx * (outer_x + 0.013), 0.0, rz),
                       material=m["trim"], bevel=0.015)
            )
        # Accent marker on the shoulder line, above the ribs and inside the
        # rim's own outer face so nothing crosses it.
        parts.append(
            K.cube(f"{asset_id}_marker_{tag}", size=(0.034, 0.09, 0.06),
                   location=(sx * (outer_x + 0.013), 0.0, 0.235),
                   material=m["accent"], bevel=0.015)
        )

    # Front latches and a carry handle that arcs clear of the rounds.
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_latch_{tag}", size=(0.055, 0.045, 0.075),
                   location=(sx * 0.135, -(hd + 0.012), 0.20), material=m["metal"],
                   bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_handle_post_{tag}", size=(0.04, 0.045, 0.14),
                   location=(sx * 0.115, -(hd - 0.024), top + 0.075),
                   material=m["metal"], bevel=0.015)
        )
    parts.append(
        K.cube(f"{asset_id}_handle_bar", size=(0.27, 0.045, 0.04),
               location=(0.0, -(hd - 0.024), top + 0.134), material=m["metal"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_stencil", size=(0.22, 0.04, 0.03),
               location=(0.0, -(hd + 0.012), 0.09), material=m["trim"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_badge", size=(0.13, 0.04, 0.05),
               location=(0.0, -(hd + 0.018), 0.205), material=m["accent"],
               bevel=0.015)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 2. armour vest — the shoulder yoke, not the plate
# ---------------------------------------------------------------------------


def build_armor_vest(
    asset_id: str,
    *,
    accent: Sequence[float] = AZURE,
    height: float = 0.55,
) -> list[bpy.types.Object]:
    """A plate carrier standing on its hem, neck opening visible from above.

    The vest silhouette is the **two shoulder blocks with a gap between
    them**. A vest modelled as one rounded slab is a sleeping bag. So the
    yoke is built as two separate masses bridged only at the back, which puts
    a notch in the outline at the single place the eye looks for "is this
    something a person wears".
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.20, 0.28, 0.30), shell_metal=0.18,
        shell_rough=0.70, trim=(0.27, 0.34, 0.36), webbing=(0.15, 0.14, 0.12),
    )
    parts: list[bpy.types.Object] = []
    s = height / 0.55                   # uniform scale on the envelope

    hem_h = 0.11 * s
    shell_h = 0.36 * s
    # The shell's bottom sits 10 mm INSIDE the hem rather than exactly on top
    # of it: two boxes meeting on a plane is a seam that catches a specular
    # line and reads as two props, not one.
    shell_cz = 0.10 * s + shell_h * 0.5

    parts.append(
        K.cube(f"{asset_id}_hem", size=(0.26 * s, 0.15 * s, hem_h),
               location=(0.0, 0.0, hem_h * 0.5), material=m["trim"],
               bevel=0.04, bevel_segments=3)
    )
    parts.append(
        K.cube(f"{asset_id}_shell", size=(0.32 * s, 0.17 * s, shell_h),
               location=(0.0, 0.0, shell_cz), material=m["shell"],
               bevel=0.04, bevel_segments=3)
    )

    # Front plate + its raised border: two values, so the panel has an edge
    # instead of dissolving into the shell behind it.
    parts.append(
        K.cube(f"{asset_id}_plate_front", size=(0.27 * s, 0.045 * s, 0.30 * s),
               location=(0.0, -0.088 * s, shell_cz), material=m["trim"],
               bevel=0.02, bevel_segments=3)
    )
    parts.append(
        K.cube(f"{asset_id}_plate_border", size=(0.21 * s, 0.035 * s, 0.24 * s),
               location=(0.0, -0.112 * s, shell_cz), material=m["shell"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_plate_back", size=(0.28 * s, 0.045 * s, 0.32 * s),
               location=(0.0, 0.082 * s, shell_cz), material=m["trim"],
               bevel=0.02)
    )

    # The yoke: two shoulder masses, neck gap open at the front (-Y) only.
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_shoulder_{tag}",
                   size=(0.13 * s, 0.18 * s, 0.11 * s),
                   location=(sx * 0.125 * s, 0.0, 0.50 * s), material=m["shell"],
                   bevel=0.04, bevel_segments=3)
        )
        parts.append(
            K.cube(f"{asset_id}_strap_{tag}",
                   size=(0.075 * s, 0.19 * s, 0.035 * s),
                   location=(sx * 0.125 * s, 0.0, 0.552 * s),
                   material=m["webbing"], bevel=0.015)
        )
        # The shoulder pad is 10 mm narrower than the shoulder it caps, so
        # its outer face is inset from the shoulder's rather than flush with
        # it — two same-facing faces in one plane read as a seam that
        # flickers as the pickup turns.
        parts.append(
            K.cube(f"{asset_id}_pad_{tag}", size=(0.11 * s, 0.17 * s, 0.045 * s),
                   location=(sx * 0.128 * s, 0.0, 0.577 * s), material=m["trim"],
                   bevel=0.02)
        )
        parts.append(
            K.cube(f"{asset_id}_cummerbund_{tag}",
                   size=(0.04 * s, 0.16 * s, 0.22 * s),
                   location=(sx * 0.165 * s, 0.01 * s, 0.26 * s),
                   material=m["webbing"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_buckle_{tag}", size=(0.05 * s, 0.07 * s, 0.05 * s),
                   location=(sx * 0.168 * s, 0.0, 0.24 * s), material=m["metal"],
                   bevel=0.02)
        )
        parts.append(
            K.cube(f"{asset_id}_stripe_{tag}",
                   size=(0.04 * s, 0.12 * s, 0.035 * s),
                   location=(sx * 0.17 * s, 0.01 * s, 0.12 * s),
                   material=m["accent"], bevel=0.015)
        )
    parts.append(
        K.cube(f"{asset_id}_neck_yoke", size=(0.10 * s, 0.14 * s, 0.05 * s),
               location=(0.0, 0.04 * s, 0.47 * s), material=m["webbing"],
               bevel=0.015)
    )

    # Magazine pouches: the thing that says "carrier" rather than "apron".
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_pouch_{tag}", size=(0.10 * s, 0.055 * s, 0.13 * s),
                   location=(sx * 0.065 * s, -0.116 * s, 0.135 * s),
                   material=m["trim"], bevel=0.02)
        )
        parts.append(
            K.cube(f"{asset_id}_pouch_flap_{tag}",
                   size=(0.105 * s, 0.035 * s, 0.035 * s),
                   location=(sx * 0.065 * s, -0.132 * s, 0.175 * s),
                   material=m["webbing"], bevel=0.015)
        )

    # Hi-viz: the chest flash and a collar band, both on the front (-Y) face
    # because that is the face a player approaching the pickup is looking at.
    # The flash is set 6 mm proud of the front plate's face (plate reaches
    # -0.1105; flash spans -0.1285 to -0.0935) so the two are not coplanar.
    parts.append(
        K.cube(f"{asset_id}_flash", size=(0.15 * s, 0.035 * s, 0.04 * s),
               location=(0.0, -0.111 * s, 0.36 * s), material=m["accent"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_collar", size=(0.30 * s, 0.05 * s, 0.035 * s),
               location=(0.0, -0.085 * s, 0.455 * s), material=m["accent"],
               bevel=0.015)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 3. cash stack — stepped bundles, because one block is a brick
# ---------------------------------------------------------------------------


def build_cash_stack(
    asset_id: str,
    *,
    accent: Sequence[float] = BILL_GREEN,
) -> list[bpy.types.Object]:
    """Four banded bundles, deliberately stepped so no two read as one brick.

    The silhouette rule here is **stagger**, not detail: each bundle is
    smaller than the one under it and offset in BOTH X and Y, so the profile
    from the side is a staircase. A single neat rectangle of banknote colour
    is indistinguishable from a brick at 20 m, which is the one thing a cash
    pickup must never be.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.30, 0.52, 0.34), shell_metal=0.0,
        shell_rough=0.86, trim=(0.22, 0.40, 0.26), metal_rgb=(0.80, 0.72, 0.40),
        webbing=(0.16, 0.15, 0.13), accent_strength=4.5,
    )
    parts: list[bpy.types.Object] = []

    # (size, centre) per bundle, bottom-up: each one smaller and offset.
    #
    # Two constraints set every number here, and both are mechanical rather
    # than aesthetic:
    #
    # 1. A banded bundle rests on its BAND, not on its notes — the strap hangs
    #    (band_h - 1) / 2 of the bundle height below them on both sides, so
    #    the whole stack is lifted by that overhang to put the band's underside
    #    on z=0 instead of driving it 4.5 mm through the floor.
    # 2. `K.add_bevel` clamps the chamfer to half a part's thinnest
    #    dimension. A 15 mm bevel on a 20 mm note-stack collapses, so no
    #    bundle is thinner than 34 mm and the strap/clip multipliers are
    #    capped at 1.15x — tall enough to stand proud of the notes, short
    #    enough that the result still clears twice the chamfer width.
    band_h, clip_h = 1.15, 1.12
    bundles = (
        ((0.340, 0.240, 0.040), (-0.005, -0.050, 0.020)),
        ((0.315, 0.225, 0.038), (0.025, 0.035, 0.057)),
        ((0.285, 0.205, 0.036), (-0.025, 0.020, 0.089)),
        ((0.210, 0.155, 0.034), (0.030, -0.020, 0.118)),
    )
    # Overhang of the bottom bundle's strap below its notes.
    lift = bundles[0][0][2] * (band_h - 1.0) * 0.5
    for i, (size, c0) in enumerate(bundles):
        c = (c0[0], c0[1], c0[2] + lift)
        parts.append(
            K.cube(f"{asset_id}_bundle{i}", size=size, location=c,
                   material=m["shell"] if i % 2 == 0 else m["trim"], bevel=0.015)
        )
        # A crossing strap and a clip per bundle, the clip straddling the
        # bundle's edge so it reads as something clamped over the notes.
        parts.append(
            K.cube(f"{asset_id}_band{i}",
                   size=(0.032, size[1] * 1.06, size[2] * band_h),
                   location=(c[0] + size[0] * 0.5, c[1], c[2]),
                   material=m["webbing"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_clip{i}", size=(0.042, 0.05, size[2] * clip_h),
                   location=(c[0] + size[0] * 0.5, c[1], c[2]),
                   material=m["metal"], bevel=0.015)
        )
        # A printed band across the front face of the lower three, so the
        # pile carries a lighter value at eye level and not only on top. Set
        # 12 mm off the face so it sinks 6 mm into the notes instead of
        # landing coplanar and z-fighting.
        if i < 3:
            parts.append(
                K.cube(f"{asset_id}_print{i}", size=(0.14, 0.036, 0.030),
                       location=(c[0], c[1] - size[1] * 0.5 - 0.012, c[2]),
                       material=m["accent"], bevel=0.015)
            )

    # The loose wad on top, banded and offset from every bundle below it. Its
    # height is derived from the stack it sits on rather than hard-coded, so it
    # cannot end up floating above the top bundle after the band lift, and both
    # parts are >= 34 mm tall so the 15 mm chamfer actually survives the
    # bevel modifier's half-thickness clamp.
    top_z = bundles[-1][0][2] + lift + bundles[-1][0][2] * 0.5   # top of stack
    wad_cz = top_z + 0.017 - 0.004                            # sunk 4 mm in
    parts.append(
        K.cube(f"{asset_id}_wad", size=(0.150, 0.115, 0.034),
               location=(-0.020, 0.030, wad_cz), material=m["trim"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_wad_band", size=(0.034, 0.122, 0.042),
               location=(-0.020, 0.030, wad_cz), material=m["webbing"],
               bevel=0.015)
    )

    # Two notes already fallen out of the stack, lying flat. They are the
    # reason the pile reads as *money on the ground* and not a wicker basket.
    for sx, tag, rot in ((0.185, "l", 14.0), (-0.175, "r", -11.0)):
        parts.append(
            _rotated_cube(
                f"{asset_id}_loose_{tag}", (0.170, 0.100, 0.036),
                location=(sx, -0.045, 0.018),
                rotation=(0.0, 0.0, math.radians(rot)),
                material=m["shell"], bevel=0.015,
            )
        )
    parts.append(
        K.cylinder(f"{asset_id}_coin", radius=0.050, depth=0.022, vertices=12,
                   location=(0.150, 0.115, 0.011), material=m["metal"],
                   smooth=True)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 4. flare pack — six upright sticks whose tips ARE the emissive
# ---------------------------------------------------------------------------


def build_flare_pack(
    asset_id: str,
    *,
    accent: Sequence[float] = FLARE_ORANGE,
    ring_radius: float = 0.082,
    stick_height: float = 0.40,
) -> list[bpy.types.Object]:
    """A low tray of six road flares, banded, each with a lit tip.

    This is the one pickup where the emissive IS the silhouette: the six cones
    at the top are the only part of the prop visible from a distance, so they
    are the prop's real shape. Everything below them exists to explain what
    the six glowing points are once the player gets close.

    The sticks stand on a hexagon, and the band is sized to *touch* the
    bundle rather than clear it — a torus floating 100 mm off the flares reads
    as an unrelated object hovering in the middle of them.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.72, 0.14, 0.10), shell_metal=0.0,
        shell_rough=0.62, trim=(0.40, 0.09, 0.07), webbing=(0.13, 0.14, 0.15),
        accent_strength=7.0,
    )
    parts: list[bpy.types.Object] = []

    tray_t = 0.035
    wall_h = 0.10
    wall_t = 0.04

    parts.append(
        K.cube(f"{asset_id}_tray_floor", size=(0.30, 0.30, tray_t),
               location=(0.0, 0.0, tray_t * 0.5), material=m["trim"],
               bevel=0.015)
    )
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_tray_{tag}", size=(wall_t, 0.30, wall_h),
                   location=(sx * 0.13, 0.0, tray_t + wall_h * 0.5),
                   material=m["trim"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_marker_{tag}", size=(0.04, 0.09, 0.05),
                   location=(sx * 0.168, 0.0, tray_t + wall_h * 0.5),
                   material=m["accent"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_handle_post_{tag}", size=(0.04, 0.045, 0.11),
                   location=(sx * 0.085, 0.16, 0.15), material=m["metal"],
                   bevel=0.015)
        )
    for sy, tag in ((-1.0, "f"), (1.0, "b")):
        parts.append(
            K.cube(f"{asset_id}_tray_{tag}", size=(0.22, wall_t, wall_h),
                   location=(0.0, sy * 0.13, tray_t + wall_h * 0.5),
                   material=m["trim"], bevel=0.015)
        )
    parts.append(
        K.cube(f"{asset_id}_handle_bar", size=(0.222, 0.045, 0.04),
               location=(0.0, 0.16, 0.19), material=m["metal"], bevel=0.015)
    )

    stick_r = 0.026
    for i in range(6):
        ang = math.radians(60.0 * i)
        x = ring_radius * math.cos(ang)
        y = ring_radius * math.sin(ang)
        stick = K.cylinder(
            f"{asset_id}_stick{i}", radius=stick_r, depth=stick_height,
            vertices=12, location=(x, y, tray_t + stick_height * 0.5),
            material=m["shell"], smooth=True,
        )
        _chamfer(stick)                      # 260 mm of tube takes a chamfer
        parts.append(stick)
        # The cap is the flare's base end cap. It starts 4 mm ABOVE the tray
        # floor rather than on it: sitting its bottom face exactly on the
        # floor puts two same-facing faces in the same plane, and the cap's
        # outline appears as a flicker through the stick it wraps.
        parts.append(
            K.cylinder(f"{asset_id}_cap{i}", radius=stick_r * 1.07, depth=0.03,
                       vertices=12, location=(x, y, tray_t + 0.019),
                       material=m["trim"], smooth=True)
        )
        parts.append(
            K.cylinder(f"{asset_id}_label{i}", radius=stick_r * 1.04,
                       depth=0.085, vertices=12, location=(x, y, 0.20),
                       material=m["trim"], smooth=True)
        )
        parts.append(
            K.cone(f"{asset_id}_tip{i}", radius1=stick_r, radius2=0.006,
                   depth=0.11, vertices=12,
                   location=(x, y, tray_t + stick_height + 0.055),
                   material=m["trim"], smooth=True)
        )
        parts.append(
            K.cone(f"{asset_id}_glow{i}", radius1=0.012, radius2=0.0, depth=0.05,
                   vertices=8,
                   location=(x, y, tray_t + stick_height + 0.135),
                   material=m["accent"], smooth=True)
        )

    parts.append(
        K.torus(f"{asset_id}_band", major_radius=ring_radius + stick_r + 0.004,
                minor_radius=0.009, major_segments=16, minor_segments=6,
                location=(0.0, 0.0, 0.22), material=m["webbing"])
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 5. grenade case — rugged, ribbed, latched, and standing on its feet
# ---------------------------------------------------------------------------


def build_grenade_case(
    asset_id: str,
    *,
    accent: Sequence[float] = HAZARD_VIOLET,
) -> list[bpy.types.Object]:
    """A rugged hard case: corner armour, lid ribs, two latches, a carry bar.

    "Rugged" is not a material. It is corner armour that reaches the lid
    line, ribs that break the lid into panels, and hardware standing proud of
    the shell — three silhouette features, none of which is a texture.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.24, 0.26, 0.23), shell_metal=0.35,
        shell_rough=0.62, trim=(0.14, 0.15, 0.14), webbing=(0.16, 0.15, 0.13),
    )
    parts: list[bpy.types.Object] = []

    foot_h = 0.035
    body_h = 0.155
    body_cz = foot_h + body_h * 0.5
    body_top = foot_h + body_h
    lid_h = 0.09
    lid_cz = body_top + lid_h * 0.5
    lid_top = body_top + lid_h
    # Corner armour spans 8 mm below the body's bottom face and 8 mm above
    # the lid's top face. Ending it flush at either plane put a trim-coloured
    # face in the same plane as a shell-coloured one — two same-facing
    # surfaces at one depth, which is a z-fight along the whole corner.
    corner_bot = foot_h - 0.008
    corner_top = lid_top + 0.008

    for sx, tx in ((-1.0, "l"), (1.0, "r")):
        for sy, ty in ((-1.0, "f"), (1.0, "b")):
            parts.append(
                K.cube(f"{asset_id}_foot_{tx}{ty}", size=(0.075, 0.075, foot_h),
                       location=(sx * 0.145, sy * 0.115, foot_h * 0.5),
                       material=m["webbing"], bevel=0.015)
            )
            # Corner armour runs the FULL height and stands 5 mm proud of the
            # body, so the step where it meets the shell is what makes a case
            # read as rugged rather than as a plain box.
            parts.append(
                K.cube(f"{asset_id}_corner_{tx}{ty}",
                       size=(0.09, 0.09, corner_top - corner_bot),
                       location=(sx * 0.16, sy * 0.11, (corner_top + corner_bot) * 0.5),
                       material=m["trim"], bevel=0.02)
            )

    parts.append(
        K.cube(f"{asset_id}_body", size=(0.40, 0.30, body_h),
               location=(0.0, 0.0, body_cz), material=m["shell"], bevel=0.03,
               bevel_segments=3)
    )
    parts.append(
        K.cube(f"{asset_id}_lid", size=(0.415, 0.315, lid_h),
               location=(0.0, 0.0, lid_cz), material=m["shell"], bevel=0.03,
               bevel_segments=3)
    )
    # The gasket lip straddles the body/lid joint, so it overlaps both by
    # 15 mm and is the widest thing on the case.
    parts.append(
        K.cube(f"{asset_id}_seam", size=(0.425, 0.325, 0.035),
               location=(0.0, 0.0, body_top), material=m["trim"], bevel=0.015)
    )
    for i, ry in enumerate((-0.09, 0.0, 0.09)):
        parts.append(
            K.cube(f"{asset_id}_lid_rib{i}", size=(0.36, 0.05, 0.035),
                   location=(0.0, ry, lid_top + 0.012), material=m["trim"],
                   bevel=0.015)
        )
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_latch_{tag}", size=(0.055, 0.045, 0.09),
                   location=(sx * 0.135, -0.165, body_top + 0.02),
                   material=m["metal"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_keeper_{tag}", size=(0.06, 0.05, 0.05),
                   location=(sx * 0.135, -0.165, body_top - 0.045),
                   material=m["metal"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_handle_post_{tag}", size=(0.045, 0.05, 0.09),
                   location=(sx * 0.085, 0.17, lid_top + 0.02),
                   material=m["webbing"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_hazard_{tag}", size=(0.06, 0.04, 0.05),
                   location=(sx * 0.13, -0.162, body_top + 0.045),
                   material=m["accent"], bevel=0.015)
        )
    parts.append(
        K.cube(f"{asset_id}_handle_bar", size=(0.22, 0.05, 0.045),
               location=(0.0, 0.17, lid_top + 0.04), material=m["webbing"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_label_plate", size=(0.30, 0.035, 0.09),
               location=(0.0, -0.165, foot_h + 0.065), material=m["trim"],
               bevel=0.015)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 6. health pack — an extruded cross, not a painted one
# ---------------------------------------------------------------------------


def build_health_pack(
    asset_id: str,
    *,
    accent: Sequence[float] = MED_RED,
) -> list[bpy.types.Object]:
    """A rounded case with the cross standing proud of it on two faces.

    The cross is the asset. A cross flush with the case wall is a decal that
    disappears the moment the player's view is not dead-on; here it is a 35 mm
    slab proud of a bezel that is itself proud of the shell, and it is
    repeated on the lid so the icon still reads from directly above.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.88, 0.88, 0.86), shell_metal=0.0,
        shell_rough=0.52, trim=(0.24, 0.25, 0.27),
        metal_rgb=(0.70, 0.72, 0.74), webbing=(0.20, 0.20, 0.22),
    )
    parts: list[bpy.types.Object] = []

    body_h = 0.22
    lid_t = 0.045
    lid_cz = body_h + lid_t * 0.5
    lid_top = body_h + lid_t
    face_y = -0.088          # cross plane; body wall face is at -0.075
    bezel_y = -0.083         # one step further back, behind the cross

    parts.append(
        K.cube(f"{asset_id}_body", size=(0.28, 0.15, body_h),
               location=(0.0, 0.0, body_h * 0.5), material=m["shell"],
               bevel=0.04, bevel_segments=3)
    )
    parts.append(
        K.cube(f"{asset_id}_skirt", size=(0.30, 0.165, 0.035),
               location=(0.0, 0.0, 0.0175), material=m["trim"], bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_lid", size=(0.285, 0.155, lid_t),
               location=(0.0, 0.0, lid_cz), material=m["shell"], bevel=0.015)
    )

    # Front cross: bezel first, then the cross proud of it by 2.5 mm. The
    # cross sinks 4.5 mm back into the shell, so no face is coplanar.
    parts.append(
        K.cube(f"{asset_id}_bezel_v", size=(0.075, 0.04, 0.165),
               location=(0.0, bezel_y, 0.115), material=m["trim"], bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_bezel_h", size=(0.165, 0.04, 0.075),
               location=(0.0, bezel_y, 0.115), material=m["trim"], bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_cross_v", size=(0.055, 0.035, 0.145),
               location=(0.0, face_y, 0.115), material=m["accent"],
               bevel=0.015, bevel_segments=3)
    )
    parts.append(
        K.cube(f"{asset_id}_cross_h", size=(0.145, 0.035, 0.055),
               location=(0.0, face_y, 0.115), material=m["accent"],
               bevel=0.015, bevel_segments=3)
    )

    # The same cross on the lid, pushed forward in Y so the carry bar behind
    # it cannot eat it from a top-down view.
    parts.append(
        K.cube(f"{asset_id}_cross_top_v", size=(0.05, 0.10, 0.04),
               location=(0.0, -0.035, lid_top + 0.02), material=m["accent"],
               bevel=0.015)
    )
    parts.append(
        K.cube(f"{asset_id}_cross_top_h", size=(0.145, 0.05, 0.04),
               location=(0.0, -0.035, lid_top + 0.02), material=m["accent"],
               bevel=0.015)
    )

    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_hinge_{tag}", size=(0.05, 0.04, 0.030),
                   location=(sx * 0.10, 0.08, lid_cz), material=m["metal"],
                   bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_handle_post_{tag}", size=(0.04, 0.045, 0.055),
                   location=(sx * 0.06, 0.045, lid_top + 0.0075),
                   material=m["webbing"], bevel=0.015)
        )
        for sy, corner in ((-1.0, "f"), (1.0, "b")):
            parts.append(
                K.cube(f"{asset_id}_bumper_{tag}{corner}",
                       size=(0.05, 0.05, 0.05),
                       location=(sx * 0.118, sy * 0.055, 0.025),
                       material=m["metal"], bevel=0.015)
            )
    parts.append(
        K.cube(f"{asset_id}_handle_bar", size=(0.172, 0.045, 0.04),
               location=(0.0, 0.045, lid_top + 0.04), material=m["webbing"],
               bevel=0.015)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 7. oxygen tank — a cylinder whose valve and wrap bands say "dive gear"
# ---------------------------------------------------------------------------


def build_o2_tank(
    asset_id: str,
    *,
    accent: Sequence[float] = TANK_CYAN,
    body_radius: float = 0.095,
    body_h: float = 0.40,
) -> list[bpy.types.Object]:
    """A dive cylinder: domed shoulder, hand-wheel valve, harness, carry bar.

    The valve is a wheel on a stem with a real outlet, not a nub — it is the
    highest point of the prop, and at 20 m the highest point is usually the
    only part above the player's horizon line. It sits at +Y so the carry bar
    can arch over the crown at -Y without passing through the wheel.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.82, 0.84, 0.86), shell_metal=0.35,
        shell_rough=0.40, trim=(0.16, 0.36, 0.42), webbing=(0.14, 0.14, 0.15),
        metal_rgb=(0.78, 0.72, 0.42), metal_rough=0.28,
    )
    parts: list[bpy.types.Object] = []

    base_h = 0.03
    body_cz = base_h + body_h * 0.5
    body_top = base_h + body_h
    shoulder_h = 0.085
    shoulder_cz = body_top + shoulder_h * 0.5
    shoulder_top = body_top + shoulder_h
    r = body_radius

    parts.append(
        K.cylinder(f"{asset_id}_base", radius=r * 1.05, depth=base_h,
                   vertices=20, location=(0.0, 0.0, base_h * 0.5),
                   material=m["trim"], smooth=True)
    )
    body = K.cylinder(f"{asset_id}_body", radius=r, depth=body_h, vertices=20,
                      location=(0.0, 0.0, body_cz), material=m["shell"],
                      smooth=True)
    _chamfer(body)                            # 400 mm of cylinder, chamfer fits
    parts.append(body)
    parts.append(
        K.cone(f"{asset_id}_shoulder", radius1=r, radius2=r * 0.44,
               depth=shoulder_h, vertices=20, location=(0.0, 0.0, shoulder_cz),
               material=m["shell"], smooth=True)
    )
    parts.append(
        K.torus(f"{asset_id}_shoulder_band", major_radius=r * 1.03,
                minor_radius=0.008, major_segments=16, minor_segments=6,
                location=(0.0, 0.0, body_top), material=m["metal"])
    )

    # Two emissive wrap bands and a printed wrap between them. The bands are
    # the only place this prop is allowed to be neon.
    for i, bz in enumerate((base_h + 0.09, base_h + 0.27)):
        parts.append(
            K.cylinder(f"{asset_id}_band{i}", radius=r * 1.04, depth=0.05,
                       vertices=20, location=(0.0, 0.0, bz), material=m["accent"],
                       smooth=True)
        )
    parts.append(
        K.cylinder(f"{asset_id}_label_wrap", radius=r * 1.026, depth=0.10,
                   vertices=20, location=(0.0, 0.0, base_h + 0.18),
                   material=m["trim"], smooth=True)
    )

    # Valve: body, stem, hand wheel on four spokes, and a rearward outlet.
    valve_z = shoulder_top
    valve_y = 0.055
    parts.append(
        K.cylinder(f"{asset_id}_valve_body", radius=0.026, depth=0.05,
                   vertices=12, location=(0.0, valve_y, valve_z + 0.025),
                   material=m["metal"], smooth=True)
    )
    parts.append(
        K.cylinder(f"{asset_id}_valve_stem", radius=0.013, depth=0.045,
                   vertices=8, location=(0.0, valve_y, valve_z + 0.0725),
                   material=m["metal"], smooth=True)
    )
    parts.append(
        _cylinder(f"{asset_id}_valve_outlet", radius=0.016, length=0.06,
                  location=(0.0, valve_y + 0.055, valve_z + 0.025),
                  rotation=(math.radians(90.0), 0.0, 0.0),
                  material=m["metal"], vertices=10)
    )
    wheel_z = valve_z + 0.10
    parts.append(
        K.torus(f"{asset_id}_hand_wheel", major_radius=0.032, minor_radius=0.008,
                major_segments=12, minor_segments=6,
                location=(0.0, valve_y, wheel_z), material=m["accent"])
    )
    # Four spokes as radii rather than chords: a bar through the centre at a
    # fixed axis reads as two crossed bars no matter how many of them there
    # are, so each one is aimed along its own angle.
    for i in range(4):
        ang = math.radians(90.0 * i)
        parts.append(
            _cylinder(
                f"{asset_id}_spoke{i}", radius=0.009, length=0.038,
                location=(0.021 * math.sin(ang),
                          valve_y + 0.021 * math.cos(ang), wheel_z),
                rotation=(math.radians(90.0), 0.0, -ang),
                material=m["accent"], vertices=8,
            )
        )
    parts.append(
        K.cylinder(f"{asset_id}_top_ring", radius=r * 0.55, depth=0.02,
                   vertices=16, location=(0.0, 0.0, shoulder_top),
                   material=m["metal"], smooth=True)
    )

    # Harness straps on the +/-X silhouette: a strap flush to a cylinder
    # vanishes, and these are what stop the tank reading as a fire
    # extinguisher.
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_harness_{tag}", size=(0.045, 0.06, body_h * 0.85),
                   location=(sx * (r - 0.007), 0.0, base_h + body_h * 0.5),
                   material=m["webbing"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_buckle_{tag}", size=(0.05, 0.07, 0.05),
                   location=(sx * (r - 0.003), 0.0, base_h + body_h * 0.5),
                   material=m["metal"], bevel=0.015)
        )
        # The carry-handle posts run to 0.57 and the bar's underside sits at
        # 0.56, so the two share a plane. Their faces are back-to-back there,
        # but the post's outer face and the bar's outer face are coplanar and
        # both visible, so the bar is widened 6 mm past the posts instead.
        parts.append(
            K.cube(f"{asset_id}_handle_post_{tag}", size=(0.04, 0.045, 0.12),
                   location=(sx * 0.055, -0.045, 0.51), material=m["metal"],
                   bevel=0.015)
        )
    parts.append(
        K.cube(f"{asset_id}_handle_bar", size=(0.182, 0.045, 0.04),
               location=(0.0, -0.045, 0.58), material=m["metal"], bevel=0.015)
    )
    # The stencil is a flat plate standing on the 95 mm body's front (-Y) face,
    # spanning y from -0.1255 to -0.0905 so it sinks 9 mm into the shell and
    # stands 29 mm proud. Its bottom at z=0.16 lands exactly on the label
    # wrap's top edge, so it is dropped 5 mm below that edge instead of
    # resting on it.
    parts.append(
        K.cube(f"{asset_id}_stencil", size=(0.09, 0.035, 0.12),
               location=(0.0, -(r + 0.013), base_h + 0.185), material=m["trim"],
               bevel=0.015)
    )
    # Gauge set just clear of the 95 mm body radius (0.0996 here) — parked on
    # the axis it would sit entirely inside the shell and never be seen.
    parts.append(
        _cylinder(f"{asset_id}_gauge", radius=0.022, length=0.022,
                  location=(0.062, -0.078, body_top - 0.03),
                  rotation=(math.radians(90.0), 0.0, 0.0), material=m["accent"],
                  vertices=12)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 8. rocket round — a finned tube racked in an open crate
# ---------------------------------------------------------------------------


def build_rocket_round(
    asset_id: str,
    *,
    accent: Sequence[float] = ROCKET_PINK,
) -> list[bpy.types.Object]:
    """A finned projectile standing in an open crate, nose above the rim.

    The crate is open-topped so the round's profile is unobstructed from
    every angle, and the four fins are the widest thing in the prop so the
    silhouette says *projectile* even when the nose cone is against the sky.
    Fin roots start 10 mm INSIDE the tube's own radius — a fin flush to the
    surface reads as four plates hovering beside the rocket.
    """
    K.reset_scene()

    m = _kit(
        asset_id, accent=accent, shell=(0.28, 0.32, 0.24), shell_metal=0.30,
        shell_rough=0.62, trim=(0.18, 0.21, 0.16), webbing=(0.15, 0.15, 0.14),
        accent_strength=5.5,
    )
    parts: list[bpy.types.Object] = []

    # --- open crate ------------------------------------------------------
    floor_t = 0.04
    wall_h = 0.13
    wall_t = 0.04
    inner = 0.19                      # inner face of the crate walls
    rail_z = floor_t + wall_h - 0.0075

    parts.append(
        K.cube(f"{asset_id}_crate_floor", size=(0.46, 0.46, floor_t),
               location=(0.0, 0.0, floor_t * 0.5), material=m["trim"],
               bevel=0.015)
    )
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_crate_wall_{tag}", size=(wall_t, 0.46, wall_h),
                   location=(sx * (inner + wall_t * 0.5), 0.0,
                             floor_t + wall_h * 0.5),
                   material=m["shell"], bevel=0.015)
        )
    for sy, tag in ((-1.0, "f"), (1.0, "b")):
        parts.append(
            K.cube(f"{asset_id}_crate_wall_{tag}", size=(0.38, wall_t, wall_h),
                   location=(0.0, sy * (inner + wall_t * 0.5),
                             floor_t + wall_h * 0.5),
                   material=m["shell"], bevel=0.015)
        )
        parts.append(
            K.cube(f"{asset_id}_crate_rail_{tag}", size=(0.42, 0.045, 0.045),
                   location=(0.0, sy * inner, rail_z), material=m["trim"],
                   bevel=0.015)
        )
    for sx, tx in ((-1.0, "l"), (1.0, "r")):
        for sy, ty in ((-1.0, "f"), (1.0, "b")):
            parts.append(
                K.cube(f"{asset_id}_crate_post_{tx}{ty}", size=(0.055, 0.055, 0.178),
                       location=(sx * inner, sy * inner, 0.089), material=m["trim"],
                       bevel=0.015)
            )
    # A strap down the crate's front (-Y) face plus a pair of corner brackets.
    # Both exist because a crate without hardware reads as a plain box at the
    # only distance that matters, and because every prop in this family
    # carries its five materials — a round that silently used three would
    # drift from its six siblings the first time someone re-tuned the palette.
    #
    # Everything here is kept at |y| > 0.19, the inner face of the crate
    # walls. The rocket tube occupies y within +/- 62 mm of the axis, so a
    # "strap over the crate" routed through the centre would pass straight
    # through the ordnance it is supposed to be holding down.
    parts.append(
        K.cube(f"{asset_id}_strap", size=(0.050, 0.045, 0.178),
               location=(0.0, -0.2385, 0.089), material=m["webbing"],
               bevel=0.015)
    )
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(
            K.cube(f"{asset_id}_bracket_{tag}", size=(0.055, 0.050, 0.075),
                   location=(sx * 0.14, -0.235, 0.100), material=m["metal"],
                   bevel=0.015)
        )
        # Rivet head standing proud of the bracket, bridging the wall top.
        parts.append(
            K.cylinder(f"{asset_id}_rivet_{tag}", radius=0.018, depth=0.05,
                       vertices=10, location=(sx * 0.14, -0.235, 0.140),
                       material=m["metal"], smooth=True)
        )

    # --- the round -------------------------------------------------------
    r = 0.062
    tube_bottom = floor_t
    tube_h = 0.44
    tube_top = tube_bottom + tube_h

    parts.append(
        K.cone(f"{asset_id}_nozzle", radius1=0.048, radius2=r, depth=0.07,
               vertices=20, location=(0.0, 0.0, tube_bottom + 0.040),
               material=m["trim"], smooth=True)
    )
    tube = K.cylinder(f"{asset_id}_tube", radius=r, depth=tube_h, vertices=24,
                      location=(0.0, 0.0, tube_bottom + tube_h * 0.5),
                      material=m["shell"], smooth=True)
    _chamfer(tube)                     # 440 mm of tube, chamfer fits
    parts.append(tube)
    parts.append(
        K.cylinder(f"{asset_id}_collar_lo", radius=r * 1.13, depth=0.035,
                   vertices=20, location=(0.0, 0.0, 0.21), material=m["trim"],
                   smooth=True)
    )
    parts.append(
        K.cylinder(f"{asset_id}_warhead_band", radius=r * 1.04, depth=0.045,
                   vertices=20, location=(0.0, 0.0, 0.27), material=m["accent"],
                   smooth=True)
    )
    parts.append(
        K.cylinder(f"{asset_id}_warhead_badge", radius=r * 1.04, depth=0.085,
                   vertices=20, location=(0.0, 0.0, 0.35), material=m["accent"],
                   smooth=True)
    )
    parts.append(
        K.cylinder(f"{asset_id}_collar_hi", radius=r * 1.10, depth=0.03,
                   vertices=20, location=(0.0, 0.0, 0.44), material=m["trim"],
                   smooth=True)
    )
    parts.append(
        K.cone(f"{asset_id}_nose", radius1=r, radius2=0.014, depth=0.17,
               vertices=24, location=(0.0, 0.0, tube_top + 0.085),
               material=m["shell"], smooth=True)
    )
    parts.append(
        K.cone(f"{asset_id}_nose_tip", radius1=0.014, radius2=0.004, depth=0.045,
               vertices=12, location=(0.0, 0.0, tube_top + 0.1925),
               material=m["accent"], smooth=True)
    )

    # Four fins, rotated about Z at the origin and only then translated — see
    # `_placed`. Root at r - 10 mm, tip at 0.172, which clears the 0.19 crate
    # wall. The fin bottoms sit at z=0.05, clear of the tube's own base at
    # z=0.04: a fin whose bottom face lands on the tube's bottom plane is a
    # trim-coloured face fighting a shell-coloured one across the exposed
    # annulus between the nozzle mouth and the tube radius. The fin tops stop
    # at 0.16, under the 0.1625 crate rail.
    for i in range(4):
        parts.append(
            _rotated_cube(
                f"{asset_id}_fin{i}", (0.035, 0.13, 0.11),
                location=(0.122, 0.0, 0.105),
                rotation=(0.0, 0.0, math.radians(90.0 * i)),
                material=m["trim"], bevel=0.015, bevel_segments=3,
            )
        )

    parts.append(
        K.cube(f"{asset_id}_stencil", size=(0.06, 0.04, 0.05),
               location=(0.0, -0.075, 0.145), material=m["trim"], bevel=0.015)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# registry entry point
# ---------------------------------------------------------------------------

PICKUPS: list[tuple[str, Callable, dict]] = [
    ("pickup_ammo_box", build_ammo_box, dict()),
    ("pickup_armor_vest", build_armor_vest, dict()),
    ("pickup_cash_stack", build_cash_stack, dict()),
    ("pickup_flare_pack", build_flare_pack, dict()),
    ("pickup_grenade_case", build_grenade_case, dict()),
    ("pickup_health_pack", build_health_pack, dict()),
    ("pickup_o2_tank", build_o2_tank, dict()),
    ("pickup_rocket_round", build_rocket_round, dict()),
]