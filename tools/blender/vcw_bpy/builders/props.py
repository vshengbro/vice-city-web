"""3A street-prop builder — the fourteen small props that dress a block.

Same three rules as the vehicle builder, restated for props:

1. **Bevelled edges everywhere.** A razor 90-degree cube edge catches the
   specular highlight as a hard line; a 2-5 cm chamfer rolls it off. On a small
   object the camera walks past, that chamfer is the single strongest
   low-poly tell there is.
2. **One material per part.** Painted steel, cast iron, rubber, glass and lit
   lenses are five different Principled responses; one material over the whole
   prop is exactly what makes a street asset read as an untextured blockout.
3. **Standalone, grounded props.** No ground plane, no kerb, no road surface —
   the ramp, kerb and pavement belong to the renderer, not to an asset library.
   Every prop here rests with its lowest point on z=0 in authoring space (Z-up,
   +X right, -Y forward), so the renderer can drop it on any surface it likes.

Colour and lit elements come from the shared palette (`P.*`) rather than from
locally invented materials, so a traffic cone's orange and a taxi's orange
cannot drift apart.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy

from .. import kernel as K
from .. import palette as P

# ---------------------------------------------------------------------------
# shared placement helpers
# ---------------------------------------------------------------------------


def _at(
    obj: bpy.types.Object,
    location: Sequence[float],
    rotation: Sequence[float] = (0.0, 0.0, 0.0),
) -> bpy.types.Object:
    """Orient a part, THEN move it into place.

    Setting `rotation_euler` on a part that is already offset from the world
    origin swings that part around the world origin instead of around itself,
    which throws it metres off the prop. So: zero the part's own transform,
    rotate it about its own centre, bake, then translate to `location` and
    bake again.

    The zeroing matters: `transform_apply` bakes whatever location the part
    was constructed with, so a part built at (0, 0, 2.46) and then placed at
    (0, 0, 2.46) would end up at z=4.92. Every part in this module is created
    wherever it is convenient and placed exactly once, through this helper.
    """
    obj.location = (0.0, 0.0, 0.0)
    obj.scale = (1.0, 1.0, 1.0)
    obj.rotation_euler = tuple(rotation)
    K.transform_apply(obj)
    obj.location = tuple(location)
    K.transform_apply(obj)
    return obj


def _scaled(obj: bpy.types.Object, scale: Sequence[float]) -> bpy.types.Object:
    """Bake a non-uniform scale (an ellipsoid, an oval handle) into the mesh."""
    obj.scale = tuple(scale)
    K.transform_apply(obj)
    return obj


def _lift(part: bpy.types.Object, dz: float) -> bpy.types.Object:
    """Raise an already-built part along z, keeping its baked geometry."""
    part.location = (part.location.x, part.location.y, part.location.z + dz)
    K.transform_apply(part)
    return part


def _side(sx: float, sy: float) -> str:
    """`l`/`r` and `f`/`b` tags, so a mirrored part has a real name."""
    return f"{'l' if sx < 0 else 'r'}{'b' if sy > 0 else 'f'}"


# ---------------------------------------------------------------------------
# 1. park bench
# ---------------------------------------------------------------------------


def build_bench(
    asset_id: str,
    *,
    length: float = 1.90,
    seat_height: float = 0.46,
    wood_rgb: Sequence[float] = (0.42, 0.24, 0.11),
    frame_rgb: Sequence[float] = (0.08, 0.19, 0.17),
) -> list[bpy.types.Object]:
    """A slatted park bench: cast frame, timber seat and raked back, on feet."""
    K.reset_scene()

    wood = P.paint(f"{asset_id}_wood", wood_rgb, metallic=0.0, roughness=0.74)
    frame = P.paint(f"{asset_id}_frame", frame_rgb, metallic=0.45, roughness=0.42)
    bolt = P.metal(f"{asset_id}_bolt", (0.52, 0.54, 0.57), rough=0.38, metal=0.9)

    parts: list[bpy.types.Object] = []
    half = length * 0.5
    pad_h = 0.035
    leg_x = half - 0.16
    top = pad_h + seat_height           # the seat rails sit on the leg tops
    rake = math.radians(9.0)            # how far the back leans away

    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            fx, fy = sx * leg_x, sy * 0.21
            tag = _side(sx, sy)
            parts.append(K.cylinder(
                f"{asset_id}_foot_{tag}", radius=0.055, depth=pad_h, vertices=16,
                location=(fx, fy, pad_h * 0.5), material=frame,
            ))
            parts.append(K.cube(
                f"{asset_id}_leg_{tag}", size=(0.075, 0.075, seat_height),
                location=(fx, fy, pad_h + seat_height * 0.5),
                material=frame, bevel=0.02,
            ))

    # stretchers: a rail under the seat, a low bar between the legs
    for sy in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_rail_{'b' if sy > 0 else 'f'}",
            size=(length - 0.24, 0.05, 0.05), location=(0.0, sy * 0.21, top - 0.025),
            material=frame, bevel=0.015,
        ))
    parts.append(K.cube(
        f"{asset_id}_stretcher", size=(length - 0.34, 0.04, 0.04),
        location=(0.0, 0.0, 0.17), material=frame, bevel=0.012,
    ))

    # seat: four timbers with real gaps, so the silhouette reads as slatted
    for i in range(4):
        parts.append(K.cube(
            f"{asset_id}_seat_{i}", size=(length, 0.115, 0.045),
            location=(0.0, -0.19 + i * 0.1267, top + 0.0225),
            material=wood, bevel=0.012,
        ))

    # back posts rake back; the slats follow the rake so the join is closed
    back_y = 0.235
    post_h = 0.52
    for sx in (-1.0, 1.0):
        tag = "l" if sx < 0 else "r"
        post = K.cube(
            f"{asset_id}_back_post_{tag}", size=(0.065, 0.065, post_h),
            location=(0.0, 0.0, 0.0), material=frame, bevel=0.02,
        )
        _at(post, (sx * leg_x, back_y - post_h * 0.5 * math.sin(rake), top + post_h * 0.5),
            (-rake, 0.0, 0.0))
        parts.append(post)
        # armrest + its upright
        parts.append(K.cube(
            f"{asset_id}_arm_{tag}", size=(0.06, 0.40, 0.055),
            location=(sx * leg_x, 0.02, top + 0.23), material=wood, bevel=0.018,
        ))
        parts.append(K.cube(
            f"{asset_id}_arm_post_{tag}", size=(0.055, 0.055, 0.23),
            location=(sx * leg_x, 0.17, top + 0.115), material=frame, bevel=0.015,
        ))

    for i in range(3):
        dz = 0.17 + i * 0.155
        slat = K.cube(
            f"{asset_id}_back_slat_{i}", size=(length - 0.10, 0.045, 0.115),
            location=(0.0, 0.0, 0.0), material=wood, bevel=0.014,
        )
        _at(slat, (0.0, back_y + dz * math.tan(rake), top + dz), (-rake, 0.0, 0.0))
        parts.append(slat)

    # through-bolts at the rail/arm joints
    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            b = K.cylinder(
                f"{asset_id}_bolt_{_side(sx, sy)}", radius=0.018, depth=0.10,
                vertices=10, location=(0.0, 0.0, 0.0), material=bolt,
            )
            _at(b, (sx * leg_x, sy * 0.21, top - 0.025), (math.radians(90.0), 0.0, 0.0))
            parts.append(b)

    # A bench without end caps and slat fasteners reads as a plank on two
    # sticks; these are the parts the camera sees at arm's length.
    for sx in (-1.0, 1.0):
        tag = "l" if sx < 0 else "r"
        # cast end cap closing each leg pair
        cap = K.cube(
            f"{asset_id}_end_cap_{tag}", size=(0.10, 0.50, 0.055),
            location=(sx * leg_x, 0.0, top - 0.10), material=frame, bevel=0.02,
        )
        parts.append(cap)
        for i in range(4):
            parts.append(K.sphere(
                f"{asset_id}_slat_screw_{tag}_{i}", radius=1.0, segments=8, ring_count=6,
                location=(sx * (leg_x - 0.02), -0.19 + i * 0.1267, top + 0.048),
                material=bolt, scale=(0.016, 0.010, 0.006),
            ))
        # foot pad under each leg so the frame meets the ground cleanly
        for sy in (-1.0, 1.0):
            parts.append(K.cylinder(
                f"{asset_id}_pad_{_side(sx, sy)}", radius=0.072, depth=0.022,
                vertices=12, location=(sx * leg_x, sy * 0.21, 0.011), material=bolt,
            ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 2. construction barrier
# ---------------------------------------------------------------------------


def build_construction_barrier(
    asset_id: str,
    *,
    width: float = 2.00,
    height: float = 1.00,
    board_rgb: Sequence[float] = (0.93, 0.92, 0.88),
    stripe_rgb: Sequence[float] = (0.95, 0.38, 0.06),
    frame_rgb: Sequence[float] = (0.86, 0.50, 0.08),
) -> list[bpy.types.Object]:
    """A striped A-frame roadwork barricade with a battery beacon on top."""
    K.reset_scene()

    board = P.paint(f"{asset_id}_board", board_rgb, metallic=0.0, roughness=0.58)
    stripe = P.paint(f"{asset_id}_stripe", stripe_rgb, metallic=0.0, roughness=0.52)
    frame = P.paint(f"{asset_id}_frame", frame_rgb, metallic=0.35, roughness=0.46)
    lens = P.emissive(f"{asset_id}_beacon", (1.0, 0.55, 0.05), strength=5.0)

    parts: list[bpy.types.Object] = []
    leg_x = width * 0.5 - 0.14
    half_d = 0.30
    lean = math.atan2(half_d, height)
    leg_len = math.hypot(half_d, height) + 0.06

    # feet
    for sy in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_foot_{'b' if sy > 0 else 'f'}",
            size=(width, 0.26, 0.08), location=(0.0, sy * half_d, 0.04),
            material=frame, bevel=0.025,
        ))
        for sx in (-1.0, 1.0):
            parts.append(K.cube(
                f"{asset_id}_footpad_{_side(sx, sy)}", size=(0.14, 0.30, 0.05),
                location=(sx * leg_x, sy * half_d, 0.10), material=frame, bevel=0.018,
            ))

    # A-frame legs, front pair leaning back and rear pair leaning forward
    for sx in (-1.0, 1.0):
        tag = "l" if sx < 0 else "r"
        for sy in (-1.0, 1.0):
            leg = K.cube(
                f"{asset_id}_leg_{tag}_{'b' if sy > 0 else 'f'}",
                size=(0.06, 0.06, leg_len), location=(0.0, 0.0, 0.0),
                material=frame, bevel=0.018,
            )
            _at(leg, (sx * leg_x, sy * half_d * 0.5, height * 0.5 + 0.04),
                (math.copysign(lean, sy), 0.0, 0.0))
            parts.append(leg)

    # two hazard boards per face, with diagonal stripes laid on top of them
    for sy in (-1.0, 1.0):
        face = "b" if sy > 0 else "f"
        for row, z in enumerate((0.44, 0.82)):
            parts.append(K.cube(
                f"{asset_id}_board_{face}_{row}", size=(width - 0.06, 0.05, 0.20),
                location=(0.0, sy * 0.05, z), material=board, bevel=0.02,
            ))
            for i in range(5):
                s = K.cube(
                    f"{asset_id}_stripe_{face}_{row}_{i}", size=(0.10, 0.02, 0.21),
                    location=(0.0, 0.0, 0.0), material=stripe, bevel=0.008,
                )
                _at(s, ((i - 2) * 0.36, sy * 0.077, z), (0.0, math.radians(34.0), 0.0))
                parts.append(s)

    # diagonal braces between the leg pairs
    for sx in (-1.0, 1.0):
        brace = K.cube(
            f"{asset_id}_brace_{'l' if sx < 0 else 'r'}", size=(0.04, 0.04, 0.52),
            location=(0.0, 0.0, 0.0), material=frame, bevel=0.012,
        )
        _at(brace, (sx * leg_x, 0.0, 0.30), (math.radians(52.0), 0.0, 0.0))
        parts.append(brace)

    # battery beacon clamped to the top of the front-left leg
    parts.append(K.cone(
        f"{asset_id}_beacon_base", radius1=0.075, radius2=0.055, depth=0.07,
        vertices=16, location=(-leg_x, -half_d * 0.5, height + 0.07),
        material=frame,
    ))
    parts.append(K.sphere(
        f"{asset_id}_beacon_lens", radius=0.062, segments=16, ring_count=8,
        location=(-leg_x, -half_d * 0.5, height + 0.17), material=lens,
        scale=(1.0, 1.0, 1.25),
    ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 3. commercial dumpster
# ---------------------------------------------------------------------------


def build_dumpster(
    asset_id: str,
    *,
    width: float = 1.10,
    length: float = 1.90,
    height: float = 1.28,
    body_rgb: Sequence[float] = (0.10, 0.26, 0.20),
    lid_rgb: Sequence[float] = (0.07, 0.19, 0.15),
) -> list[bpy.types.Object]:
    """A wheelie dumpster: tapered body, two lid halves, ribs, castors, placard."""
    K.reset_scene()

    body_mat = P.paint(f"{asset_id}_body", body_rgb, metallic=0.55, roughness=0.48)
    lid_mat = P.paint(f"{asset_id}_lid", lid_rgb, metallic=0.55, roughness=0.42)
    trim = P.metal(f"{asset_id}_trim", (0.58, 0.60, 0.63), rough=0.36, metal=0.95)
    rubber = P.rubber(f"{asset_id}_tyre")
    placard = P.paint(f"{asset_id}_placard", (0.94, 0.93, 0.88), metallic=0.0, roughness=0.45)

    parts: list[bpy.types.Object] = []
    wheel_r = 0.10
    frame_h = 0.08
    base_z = wheel_r * 2.0                     # castors carry the frame clear
    body_h = height - base_z - 0.30
    body_z = base_z + frame_h + body_h * 0.5

    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            w = K.cylinder(
                f"{asset_id}_wheel_{_side(sx, sy)}", radius=wheel_r, depth=0.09,
                vertices=16, location=(0.0, 0.0, 0.0), material=rubber,
            )
            _at(w, (sx * (width * 0.5 - 0.10), sy * (length * 0.5 - 0.24), wheel_r),
                (0.0, math.radians(90.0), 0.0))
            parts.append(w)

    parts.append(K.cube(
        f"{asset_id}_frame", size=(width * 0.96, length * 0.96, frame_h),
        location=(0.0, 0.0, base_z + frame_h * 0.5), material=trim, bevel=0.02,
    ))
    parts.append(K.cube(
        f"{asset_id}_body", size=(width, length, body_h), location=(0.0, 0.0, body_z),
        material=body_mat, bevel=0.05, bevel_segments=3,
    ))
    # shoulder: the top course is inset, which is what gives a skip its taper
    parts.append(K.cube(
        f"{asset_id}_shoulder",
        size=(width * 0.93, length * 0.99, 0.14),
        location=(0.0, 0.0, body_z + body_h * 0.5 + 0.07),
        material=body_mat, bevel=0.035,
    ))
    rim_z = body_z + body_h * 0.5 + 0.14
    parts.append(K.cube(
        f"{asset_id}_rim", size=(width + 0.06, length + 0.06, 0.07),
        location=(0.0, 0.0, rim_z + 0.035), material=trim, bevel=0.025,
    ))

    # Two lid halves. Each is HALF the bin's length and centred on its own half,
    # not a full-length lid pushed out along y — that would overhang the body by
    # half a metre and read as a plate hovering off the back of the skip.
    lid_len = length * 0.50
    for sy in (-1.0, 1.0):
        tag = "b" if sy > 0 else "f"
        lid = K.cube(
            f"{asset_id}_lid_{tag}", size=(width * 0.53, lid_len, 0.06),
            location=(0.0, 0.0, 0.0), material=lid_mat, bevel=0.022,
        )
        tilt = math.radians(-4.0) if sy > 0 else math.radians(2.0)
        _at(lid, (0.0, sy * lid_len * 0.5, rim_z + 0.10), (tilt, 0.0, 0.0))
        parts.append(lid)
        parts.append(K.cube(
            f"{asset_id}_lid_handle_{tag}", size=(0.20, 0.05, 0.045),
            location=(0.0, sy * (length * 0.5 - 0.02), rim_z + 0.12),
            material=trim, bevel=0.012,
        ))
        hinge = K.cylinder(
            f"{asset_id}_hinge_{tag}", radius=0.028, depth=width * 0.5, vertices=12,
            location=(0.0, 0.0, 0.0), material=trim,
        )
        _at(hinge, (0.0, sy * (length * 0.5 + 0.01), rim_z + 0.06),
            (0.0, math.radians(90.0), 0.0))
        parts.append(hinge)

    # vertical stiffener ribs down both flanks
    for sx in (-1.0, 1.0):
        for i in range(3):
            parts.append(K.cube(
                f"{asset_id}_rib_{'l' if sx < 0 else 'r'}_{i}",
                size=(0.035, 0.06, body_h * 0.74),
                location=(sx * (width * 0.5 + 0.012), (i - 1) * length * 0.28, body_z),
                material=body_mat, bevel=0.012,
            ))

    # front placard and a lifting bar across the top of the front face
    parts.append(K.cube(
        f"{asset_id}_placard", size=(0.34, 0.02, 0.26),
        location=(0.0, -(length * 0.5 + 0.012), body_z + 0.12),
        material=placard, bevel=0.012,
    ))
    parts.append(K.cube(
        f"{asset_id}_lift_bar", size=(width * 0.86, 0.05, 0.05),
        location=(0.0, -(length * 0.5 + 0.02), body_z - body_h * 0.34),
        material=trim, bevel=0.015,
    ))

    # A skip has visible hardware: drop-down tailgate hinges, fork pockets on
    # the flanks and a latch bar, all of which is what stops it reading as a
    # bevelled crate.
    for sx in (-1.0, 1.0):
        for i in range(2):
            parts.append(K.cube(
                f"{asset_id}_fork_pocket_{'l' if sx < 0 else 'r'}_{i}",
                size=(0.06, 0.30, 0.10),
                location=(sx * (width * 0.5 + 0.02), (i - 0.5) * length * 0.44,
                          base_z + frame_h * 0.5),
                material=trim, bevel=0.018,
            ))
    for sx in (-1.0, 1.0):
        pin = K.cylinder(
            f"{asset_id}_hinge_pin_{'l' if sx < 0 else 'r'}", radius=0.030,
            depth=0.10, vertices=12, location=(0.0, 0.0, 0.0), material=trim,
        )
        _at(pin, (sx * width * 0.26, length * 0.5 + 0.015, base_z + 0.22),
            (0.0, math.radians(90.0), 0.0))
        parts.append(pin)
    parts.append(K.cube(
        f"{asset_id}_latch_bar", size=(width * 0.80, 0.045, 0.045),
        location=(0.0, -(length * 0.5 + 0.045), base_z + 0.22),
        material=trim, bevel=0.014,
    ))
    for i in range(4):
        a = math.radians(45.0 + i * 90.0)
        bead = K.torus(
            f"{asset_id}_rim_bead_{i}", major_radius=0.030, minor_radius=0.010,
            major_segments=12, minor_segments=6,
            location=(0.0, 0.0, 0.0), material=trim,
        )
        _at(bead, (math.cos(a) * width * 0.42, math.sin(a) * length * 0.42,
                   rim_z + 0.075))
        parts.append(bead)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 4. fire hydrant
# ---------------------------------------------------------------------------


def build_fire_hydrant(
    asset_id: str,
    *,
    height: float = 0.78,
    body_rgb: Sequence[float] = (0.86, 0.14, 0.10),
    cap_rgb: Sequence[float] = (0.94, 0.80, 0.18),
) -> list[bpy.types.Object]:
    """A dry-barrel hydrant: flanged barrel, bonnet, three nozzles, a chain."""
    K.reset_scene()

    body_mat = P.paint(f"{asset_id}_body", body_rgb, metallic=0.25, roughness=0.44)
    cap_mat = P.paint(f"{asset_id}_cap", cap_rgb, metallic=0.35, roughness=0.40)
    nut = P.metal(f"{asset_id}_nut", (0.70, 0.72, 0.75), rough=0.28, metal=0.95)

    parts: list[bpy.types.Object] = []
    h = height
    barrel_r = 0.105

    # ground flange -> barrel -> bonnet flange -> dome
    parts.append(K.cylinder(
        f"{asset_id}_ground_flange", radius=0.20, depth=0.06, vertices=20,
        location=(0.0, 0.0, 0.03), material=body_mat,
    ))
    parts.append(K.cone(
        f"{asset_id}_flange_cone", radius1=0.165, radius2=0.135, depth=0.09,
        vertices=20, location=(0.0, 0.0, 0.105), material=body_mat,
    ))
    parts.append(K.cone(
        f"{asset_id}_barrel_lower", radius1=barrel_r * 1.28, radius2=barrel_r * 1.06,
        depth=h * 0.38, vertices=20, location=(0.0, 0.0, 0.15 + h * 0.19),
        material=body_mat,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_barrel_upper", radius=barrel_r, depth=h * 0.30, vertices=20,
        location=(0.0, 0.0, 0.15 + h * 0.52), material=body_mat,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_bonnet_flange", radius=barrel_r * 1.30, depth=0.05, vertices=20,
        location=(0.0, 0.0, h * 0.80), material=body_mat,
    ))
    parts.append(K.cone(
        f"{asset_id}_bonnet", radius1=barrel_r * 1.22, radius2=barrel_r * 0.52,
        depth=h * 0.16, vertices=20, location=(0.0, 0.0, h * 0.90),
        material=body_mat,
    ))
    parts.append(K.sphere(
        f"{asset_id}_dome", radius=1.0, segments=20, ring_count=10,
        location=(0.0, 0.0, h * 0.975), material=body_mat,
        scale=(0.072, 0.072, 0.058),
    ))
    parts.append(K.cylinder(
        f"{asset_id}_operating_nut", radius=0.030, depth=0.055, vertices=16,
        location=(0.0, 0.0, h * 1.02), material=nut,
    ))
    parts.append(K.sphere(
        f"{asset_id}_nut_cap", radius=1.0, segments=12, ring_count=6,
        location=(0.0, 0.0, h * 1.055), material=nut, scale=(0.030, 0.030, 0.018),
    ))

    # bonnet bolts around the flange
    for i in range(4):
        a = math.radians(45.0 + i * 90.0)
        parts.append(K.sphere(
            f"{asset_id}_bolthead_{i}", radius=1.0, segments=8, ring_count=6,
            location=(math.cos(a) * 0.105, math.sin(a) * 0.105, h * 0.845),
            material=nut, scale=(0.016, 0.016, 0.012),
        ))

    # two side nozzles with pentagon caps
    for sx in (-1.0, 1.0):
        tag = "l" if sx < 0 else "r"
        noz = K.cylinder(
            f"{asset_id}_nozzle_{tag}", radius=0.050, depth=0.11, vertices=16,
            location=(0.0, 0.0, 0.0), material=body_mat,
        )
        _at(noz, (sx * (barrel_r + 0.05), 0.0, h * 0.58), (0.0, math.radians(90.0), 0.0))
        parts.append(noz)
        cap = K.cylinder(
            f"{asset_id}_nozzle_cap_{tag}", radius=0.058, depth=0.042, vertices=5,
            location=(0.0, 0.0, 0.0), material=cap_mat,
        )
        _at(cap, (sx * (barrel_r + 0.115), 0.0, h * 0.58), (0.0, math.radians(90.0), 0.0))
        parts.append(cap)

    # the big front pumper nozzle
    front = K.cylinder(
        f"{asset_id}_pumper", radius=0.062, depth=0.13, vertices=16,
        location=(0.0, 0.0, 0.0), material=body_mat,
    )
    _at(front, (0.0, -(barrel_r + 0.06), h * 0.46), (math.radians(90.0), 0.0, 0.0))
    parts.append(front)
    front_cap = K.cylinder(
        f"{asset_id}_pumper_cap", radius=0.072, depth=0.05, vertices=6,
        location=(0.0, 0.0, 0.0), material=cap_mat,
    )
    _at(front_cap, (0.0, -(barrel_r + 0.145), h * 0.46), (math.radians(90.0), 0.0, 0.0))
    parts.append(front_cap)

    # cap chains: alternating links, a real torus each
    for tag, x0 in (("l", -0.105), ("p", 0.0)):
        for i in range(3):
            link = K.torus(
                f"{asset_id}_chain_{tag}_{i}", major_radius=0.022, minor_radius=0.007,
                major_segments=12, minor_segments=6, location=(0.0, 0.0, 0.0),
                material=nut,
            )
            ax = math.radians(90.0 if i % 2 else 0.0)
            _at(link, (x0, -0.085 - i * 0.012, h * 0.72 - i * 0.055),
                (ax, math.radians(28.0), 0.0))
            parts.append(link)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 5. mailbox
# ---------------------------------------------------------------------------


def build_mailbox(
    asset_id: str,
    *,
    post_height: float = 0.95,
    body_rgb: Sequence[float] = (0.18, 0.34, 0.52),
    post_rgb: Sequence[float] = (0.32, 0.34, 0.36),
) -> list[bpy.types.Object]:
    """A barrel-top curbside mailbox on a post, with a signal flag and a slot."""
    K.reset_scene()

    body_mat = P.paint(f"{asset_id}_body", body_rgb, metallic=0.30, roughness=0.40)
    post_mat = P.paint(f"{asset_id}_post", post_rgb, metallic=0.60, roughness=0.44)
    trim = P.metal(f"{asset_id}_trim", (0.72, 0.74, 0.77), rough=0.26, metal=0.95)
    flag_mat = P.paint(f"{asset_id}_flag", (0.88, 0.12, 0.10), metallic=0.1, roughness=0.42)
    placard = P.paint(f"{asset_id}_placard", (0.95, 0.94, 0.90), metallic=0.0, roughness=0.40)

    parts: list[bpy.types.Object] = []
    body_w = 0.46
    body_d = 0.34
    body_h = 0.24
    base_h = 0.06
    body_z = post_height + body_h * 0.5

    parts.append(K.cylinder(
        f"{asset_id}_base_plate", radius=0.135, depth=base_h, vertices=20,
        location=(0.0, 0.0, base_h * 0.5), material=post_mat,
    ))
    parts.append(K.cone(
        f"{asset_id}_base_collar", radius1=0.10, radius2=0.075, depth=0.08,
        vertices=16, location=(0.0, 0.0, base_h + 0.04), material=post_mat,
    ))
    parts.append(K.cube(
        f"{asset_id}_post", size=(0.11, 0.11, post_height - base_h),
        location=(0.0, 0.0, base_h + (post_height - base_h) * 0.5),
        material=post_mat, bevel=0.025,
    ))

    # barrel top: a cylinder on its side, squashed in Y so it exactly meets the
    # box corners — the equator of the ellipse lands on the box's top corners,
    # so the joint is flush rather than a stuck-on half pipe.
    barrel = K.cylinder(
        f"{asset_id}_barrel", radius=body_w * 0.5, depth=body_w, vertices=32,
        location=(0.0, 0.0, 0.0), material=body_mat,
    )
    _scaled(barrel, (1.0, body_d / body_w, 1.0))
    _at(barrel, (0.0, 0.0, body_z + body_h * 0.5), (math.radians(90.0), 0.0, 0.0))
    parts.append(barrel)

    parts.append(K.cube(
        f"{asset_id}_body", size=(body_w, body_d, body_h), location=(0.0, 0.0, body_z),
        material=body_mat, bevel=0.045, bevel_segments=3,
    ))

    # lid seam across the top of the door, plus the door itself
    parts.append(K.cube(
        f"{asset_id}_lid_seam", size=(body_w + 0.01, 0.035, 0.05),
        location=(0.0, -(body_d * 0.5 + 0.005), body_z + body_h * 0.5 - 0.03),
        material=trim, bevel=0.012,
    ))
    parts.append(K.cube(
        f"{asset_id}_door", size=(body_w - 0.05, 0.035, body_h * 0.82),
        location=(0.0, -(body_d * 0.5 + 0.012), body_z - 0.01),
        material=body_mat, bevel=0.035, bevel_segments=3,
    ))
    # pull handle: a bar standing off the door on two stubs
    for sx in (-1.0, 1.0):
        stub = K.cylinder(
            f"{asset_id}_handle_stub_{'l' if sx < 0 else 'r'}", radius=0.012,
            depth=0.05, vertices=10, location=(0.0, 0.0, 0.0), material=trim,
        )
        _at(stub, (sx * 0.11, -(body_d * 0.5 + 0.045), body_z - 0.02),
            (math.radians(90.0), 0.0, 0.0))
        parts.append(stub)
    bar = K.cylinder(
        f"{asset_id}_handle", radius=0.014, depth=0.26, vertices=12,
        location=(0.0, 0.0, 0.0), material=trim,
    )
    _at(bar, (0.0, -(body_d * 0.5 + 0.07), body_z - 0.02), (0.0, math.radians(90.0), 0.0))
    parts.append(bar)

    # mail slot with a hood over it
    parts.append(K.cube(
        f"{asset_id}_slot_hood", size=(0.20, 0.06, 0.035),
        location=(0.0, -(body_d * 0.5 + 0.03), body_z + 0.06),
        material=trim, bevel=0.012,
    ))
    parts.append(K.cube(
        f"{asset_id}_slot", size=(0.16, 0.02, 0.03),
        location=(0.0, -(body_d * 0.5 + 0.018), body_z + 0.042),
        material=K.pbr_material(f"{asset_id}_slot_dark", (0.03, 0.03, 0.035), roughness=0.9),
    ))

    # signal flag on the right flank, plus a name plate and rivets
    parts.append(K.cube(
        f"{asset_id}_flag_arm", size=(0.03, 0.03, 0.19),
        location=(body_w * 0.5 + 0.03, 0.0, body_z + 0.05),
        material=flag_mat, bevel=0.008,
    ))
    parts.append(K.cube(
        f"{asset_id}_flag_vane", size=(0.025, 0.10, 0.07),
        location=(body_w * 0.5 + 0.03, 0.055, body_z + 0.11),
        material=flag_mat, bevel=0.008,
    ))
    parts.append(K.cube(
        f"{asset_id}_plate", size=(0.14, 0.015, 0.06),
        location=(0.0, -(body_d * 0.5 + 0.032), body_z - 0.06),
        material=placard, bevel=0.008,
    ))
    for sx in (-1.0, 1.0):
        for sz in (-1.0, 1.0):
            parts.append(K.sphere(
                f"{asset_id}_rivet_{'l' if sx < 0 else 'r'}{'b' if sz > 0 else 't'}",
                radius=1.0, segments=10, ring_count=6,
                location=(sx * 0.17, -(body_d * 0.5 + 0.022), body_z + sz * 0.06),
                material=trim, scale=(0.013, 0.009, 0.013),
            ))
    for sx in (-1.0, 1.0):
        h = K.cylinder(
            f"{asset_id}_lid_hinge_{'l' if sx < 0 else 'r'}", radius=0.016,
            depth=0.06, vertices=10, location=(0.0, 0.0, 0.0), material=trim,
        )
        _at(h, (sx * 0.15, body_d * 0.5 - 0.02, body_z + body_h * 0.5 - 0.03),
            (0.0, math.radians(90.0), 0.0))
        parts.append(h)

    # collector bracket, a second hinge leaf and the door stop, so the box is
    # not a barrel balanced on a stick
    for i in range(4):
        a = math.radians(30.0 + i * 40.0)
        lug = K.cube(
            f"{asset_id}_collector_lug_{i}", size=(0.05, 0.05, 0.035),
            location=(math.cos(a) * body_w * 0.26, math.sin(a) * body_w * 0.26,
                      body_z - body_h * 0.5 - 0.02),
            material=trim, bevel=0.008,
        )
        parts.append(lug)
    parts.append(K.cube(
        f"{asset_id}_door_stop", size=(0.05, 0.07, 0.05),
        location=(0.0, -(body_d * 0.5 + 0.02), body_z - body_h * 0.5 + 0.03),
        material=trim, bevel=0.012,
    ))
    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            parts.append(K.cylinder(
                f"{asset_id}_base_bolt_{_side(sx, sy)}", radius=0.014, depth=0.03,
                vertices=10,
                location=(sx * 0.095, sy * 0.095, base_h + 0.015), material=trim,
            ))

    # raised street name plate on the door and a drip cap under the barrel
    parts.append(K.cube(
        f"{asset_id}_name_plate", size=(0.22, 0.012, 0.055),
        location=(0.0, -(body_d * 0.5 + 0.036), body_z + 0.01),
        material=placard, bevel=0.006,
    ))
    drip = K.cube(
        f"{asset_id}_drip_cap", size=(body_w + 0.03, body_d + 0.03, 0.022),
        location=(0.0, 0.0, 0.0), material=trim, bevel=0.008,
    )
    _at(drip, (0.0, 0.0, body_z + body_h * 0.5 - 0.012))
    parts.append(drip)
    for i in range(3):
        parts.append(K.sphere(
            f"{asset_id}_post_rivet_{i}", radius=1.0, segments=10, ring_count=6,
            location=(0.0, -0.058, 0.30 + i * 0.22), material=trim,
            scale=(0.014, 0.010, 0.014),
        ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 6. manhole cover
# ---------------------------------------------------------------------------


def build_manhole_cover(
    asset_id: str,
    *,
    diameter: float = 0.84,
    frame_rgb: Sequence[float] = (0.22, 0.20, 0.19),
    iron_rgb: Sequence[float] = (0.13, 0.13, 0.14),
) -> list[bpy.types.Object]:
    """A cast-iron manhole cover: outer frame ring, ribbed lid, slotted panel.

    The slotted centre is real geometry built by `P.grate` — a manhole is one
    of the few street props the camera stands directly over, so the slots have
    to be holes in the mesh rather than a texture.
    """
    K.reset_scene()

    frame = P.paint(f"{asset_id}_frame", frame_rgb, metallic=0.55, roughness=0.62)
    iron = P.paint(f"{asset_id}_iron", iron_rgb, metallic=0.70, roughness=0.55)
    bezel = P.metal(f"{asset_id}_bezel", (0.46, 0.44, 0.42), rough=0.48, metal=0.85)

    parts: list[bpy.types.Object] = []
    r = diameter * 0.5

    parts.append(K.cylinder(
        f"{asset_id}_frame_plate", radius=r, depth=0.05, vertices=36,
        location=(0.0, 0.0, 0.025), material=frame,
    ))
    parts.append(K.torus(
        f"{asset_id}_frame_ring", major_radius=r * 0.92, minor_radius=0.035,
        major_segments=36, minor_segments=10, location=(0.0, 0.0, 0.05),
        material=frame,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_lid", radius=r * 0.82, depth=0.06, vertices=36,
        location=(0.0, 0.0, 0.03), material=iron,
    ))
    parts.append(K.torus(
        f"{asset_id}_lid_rib", major_radius=r * 0.70, minor_radius=0.026,
        major_segments=32, minor_segments=10, location=(0.0, 0.0, 0.058),
        material=iron,
    ))

    # square access frame around the slotted panel
    panel = 0.40
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_bezel_{'l' if sx < 0 else 'r'}",
            size=(0.05, panel + 0.10, 0.05),
            location=(sx * (panel * 0.5 + 0.025), 0.0, 0.068),
            material=bezel, bevel=0.014,
        ))
    for sy in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_bezel_{'b' if sy > 0 else 'f'}",
            size=(panel - 0.10, 0.05, 0.05),
            location=(0.0, sy * (panel * 0.5 + 0.025), 0.068),
            material=bezel, bevel=0.014,
        ))

    # the slots themselves, set down inside the bezel
    for slot in P.grate(
        f"{asset_id}_slots", iron, height=0.05, slats=9, bar=0.026, along="x",
        size=(panel, panel),
    ):
        parts.append(_lift(slot, 0.038))

    for i in range(6):
        a = math.radians(i * 60.0)
        parts.append(K.cylinder(
            f"{asset_id}_bolt_{i}", radius=0.022, depth=0.035, vertices=12,
            location=(math.cos(a) * r * 0.74, math.sin(a) * r * 0.74, 0.058),
            material=bezel,
        ))
    parts.append(K.cube(
        f"{asset_id}_pick_hole", size=(0.10, 0.02, 0.03),
        location=(-r * 0.60, 0.0, 0.062), material=iron, bevel=0.006,
    ))
    parts.append(K.cube(
        f"{asset_id}_hinge_lug", size=(0.14, 0.09, 0.05),
        location=(r * 0.74, 0.0, 0.062), material=bezel, bevel=0.014,
    ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 7. newsstand
# ---------------------------------------------------------------------------


def build_newsstand(
    asset_id: str,
    *,
    width: float = 1.55,
    depth: float = 0.95,
    height: float = 2.05,
    body_rgb: Sequence[float] = (0.16, 0.30, 0.42),
    trim_rgb: Sequence[float] = (0.88, 0.86, 0.80),
    sign_rgb: Sequence[float] = (1.0, 0.34, 0.42),
) -> list[bpy.types.Object]:
    """A magazine kiosk: cabinet, serving hatch, magazine rack, awning, lit sign."""
    K.reset_scene()

    body_mat = P.paint(f"{asset_id}_body", body_rgb, metallic=0.25, roughness=0.52)
    trim = P.paint(f"{asset_id}_trim", trim_rgb, metallic=0.20, roughness=0.46)
    glass = P.glass(f"{asset_id}_glass")
    sign = P.emissive(f"{asset_id}_sign", sign_rgb, strength=4.5)
    bulb = P.emissive(f"{asset_id}_bulb", (1.0, 0.88, 0.62), strength=5.0)
    darks = P.paint(f"{asset_id}_dark", (0.06, 0.06, 0.07), metallic=0.1, roughness=0.70)
    covers = [
        P.paint(f"{asset_id}_cover{i}", c, metallic=0.0, roughness=0.34)
        for i, c in enumerate((
            (0.95, 0.28, 0.32), (0.25, 0.62, 0.85), (0.98, 0.78, 0.22),
            (0.42, 0.78, 0.40), (0.72, 0.36, 0.82),
        ))
    ]

    parts: list[bpy.types.Object] = []
    w, d = width, depth
    cab_h = 1.10
    cab_top = 0.20 + cab_h
    front = -d * 0.5

    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            parts.append(K.cylinder(
                f"{asset_id}_foot_{_side(sx, sy)}", radius=0.045, depth=0.08,
                vertices=12, location=(sx * (w * 0.5 - 0.12), sy * (d * 0.5 - 0.12), 0.04),
                material=trim,
            ))
    parts.append(K.cube(
        f"{asset_id}_plinth", size=(w, d, 0.12), location=(0.0, 0.0, 0.14),
        material=trim, bevel=0.03,
    ))
    parts.append(K.cube(
        f"{asset_id}_cabinet", size=(w * 0.97, d * 0.95, cab_h),
        location=(0.0, 0.0, 0.20 + cab_h * 0.5), material=body_mat,
        bevel=0.04, bevel_segments=3,
    ))

    # serving counter and the hatch frame above it
    parts.append(K.cube(
        f"{asset_id}_counter", size=(w * 0.99, 0.34, 0.07),
        location=(0.0, front + 0.12, cab_top + 0.06), material=trim, bevel=0.025,
    ))
    hatch_z = cab_top + 0.48
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_hatch_jamb_{'l' if sx < 0 else 'r'}",
            size=(0.10, 0.10, 0.60), location=(sx * 0.44, front - 0.02, hatch_z),
            material=trim, bevel=0.022,
        ))
    parts.append(K.cube(
        f"{asset_id}_hatch_head", size=(0.98, 0.10, 0.10),
        location=(0.0, front - 0.02, hatch_z + 0.30), material=trim, bevel=0.022,
    ))
    parts.append(K.cube(
        f"{asset_id}_hatch_glass", size=(0.78, 0.03, 0.48),
        location=(0.0, front - 0.03, hatch_z), material=glass,
    ))

    # magazine rack: three tilted shelves, each loaded with covers
    for row in range(3):
        z = 0.42 + row * 0.24
        shelf = K.cube(
            f"{asset_id}_shelf_{row}", size=(w * 0.86, 0.22, 0.03),
            location=(0.0, 0.0, 0.0), material=darks, bevel=0.01,
        )
        _at(shelf, (0.0, front - 0.06, z), (math.radians(-16.0), 0.0, 0.0))
        parts.append(shelf)
        for i in range(4):
            x = (i - 1.5) * w * 0.19
            mag = K.cube(
                f"{asset_id}_mag_{row}_{i}", size=(w * 0.16, 0.012, 0.20),
                location=(0.0, 0.0, 0.0), material=covers[(row + i) % len(covers)],
                bevel=0.004,
            )
            _at(mag, (x, front - 0.10, z + 0.055), (math.radians(-16.0), 0.0, 0.0))
            parts.append(mag)

    # side panels break up the slab
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_side_panel_{'l' if sx < 0 else 'r'}",
            size=(0.04, d * 0.72, 0.78),
            location=(sx * (w * 0.5 - 0.03), 0.06, 0.20 + cab_h * 0.5),
            material=trim, bevel=0.02,
        ))

    # awning over the hatch, on two brackets
    awn = K.cube(
        f"{asset_id}_awning", size=(w * 0.94, 0.55, 0.05),
        location=(0.0, 0.0, 0.0), material=trim, bevel=0.02,
    )
    _at(awn, (0.0, front - 0.24, cab_top + 0.86), (math.radians(-14.0), 0.0, 0.0))
    parts.append(awn)
    for sx in (-1.0, 1.0):
        br = K.cube(
            f"{asset_id}_bracket_{'l' if sx < 0 else 'r'}", size=(0.05, 0.52, 0.05),
            location=(0.0, 0.0, 0.0), material=trim, bevel=0.012,
        )
        _at(br, (sx * w * 0.40, front - 0.22, cab_top + 0.72),
            (math.radians(38.0), 0.0, 0.0))
        parts.append(br)

    # lit sign box on the roof, plus two canopy bulbs
    parts.append(K.cube(
        f"{asset_id}_sign_box", size=(w * 0.86, 0.10, 0.42),
        location=(0.0, front - 0.06, height + 0.18), material=darks, bevel=0.03,
    ))
    parts.append(K.cube(
        f"{asset_id}_sign_face", size=(w * 0.80, 0.04, 0.34),
        location=(0.0, front - 0.12, height + 0.18), material=sign,
    ))
    for sx in (-1.0, 1.0):
        x = sx * w * 0.26
        parts.append(K.cone(
            f"{asset_id}_canopy_shade_{'l' if sx < 0 else 'r'}", radius1=0.10,
            radius2=0.04, depth=0.10, vertices=14,
            location=(x, front - 0.42, cab_top + 0.74), material=trim,
        ))
        parts.append(K.sphere(
            f"{asset_id}_canopy_bulb_{'l' if sx < 0 else 'r'}", radius=1.0,
            segments=14, ring_count=8, location=(x, front - 0.42, cab_top + 0.66),
            material=bulb, scale=(0.07, 0.07, 0.055),
        ))

    # roof slab and fascia
    parts.append(K.cube(
        f"{asset_id}_roof", size=(w + 0.18, d + 0.18, 0.12),
        location=(0.0, 0.0, height - 0.06), material=trim, bevel=0.035,
    ))
    parts.append(K.cube(
        f"{asset_id}_fascia", size=(w + 0.22, d + 0.22, 0.06),
        location=(0.0, 0.0, height - 0.15), material=body_mat, bevel=0.025,
    ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 8. palm planter
# ---------------------------------------------------------------------------


def build_palm_planter(
    asset_id: str,
    *,
    planter: float = 1.25,
    wall_h: float = 0.62,
    fronds: int = 9,
    trunk_h: float = 1.95,
    frame_rgb: Sequence[float] = (0.74, 0.70, 0.62),
    leaf_rgb: Sequence[float] = (0.16, 0.40, 0.18),
) -> list[bpy.types.Object]:
    """A concrete planter with a real palm: banded trunk and a crown of fronds."""
    K.reset_scene()

    stone = P.paint(f"{asset_id}_stone", frame_rgb, metallic=0.0, roughness=0.80)
    soil = P.paint(f"{asset_id}_soil", (0.16, 0.12, 0.09), metallic=0.0, roughness=0.92)
    trunk_mat = P.foliage(f"{asset_id}_trunk", (0.28, 0.21, 0.14), rough=0.80)
    leaf = P.foliage(f"{asset_id}_leaf", leaf_rgb, rough=0.66)
    leaf_dark = P.foliage(f"{asset_id}_leaf_dark", (0.10, 0.28, 0.13), rough=0.70)
    nut = P.foliage(f"{asset_id}_coconut", (0.34, 0.25, 0.14), rough=0.72)

    parts: list[bpy.types.Object] = []
    pw = planter * 0.5
    t = 0.10                                    # wall thickness
    base_h = 0.10

    parts.append(K.cube(
        f"{asset_id}_base", size=(planter, planter, base_h),
        location=(0.0, 0.0, base_h * 0.5), material=stone, bevel=0.03,
    ))
    for sy in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_wall_{'b' if sy > 0 else 'f'}",
            size=(planter, t, wall_h),
            location=(0.0, sy * (pw - t * 0.5), base_h + wall_h * 0.5),
            material=stone, bevel=0.035,
        ))
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_wall_{'l' if sx < 0 else 'r'}",
            size=(t, planter - t * 2.0, wall_h),
            location=(sx * (pw - t * 0.5), 0.0, base_h + wall_h * 0.5),
            material=stone, bevel=0.035,
        ))
    # coping course: a wider rim is what stops the planter reading as a box
    for sy in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_coping_{'b' if sy > 0 else 'f'}",
            size=(planter + 0.06, t + 0.08, 0.06),
            location=(0.0, sy * (pw - t * 0.5), base_h + wall_h + 0.02),
            material=stone, bevel=0.025,
        ))
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_coping_{'l' if sx < 0 else 'r'}",
            size=(t + 0.08, planter - t * 2.0 + 0.06, 0.06),
            location=(sx * (pw - t * 0.5), 0.0, base_h + wall_h + 0.02),
            material=stone, bevel=0.025,
        ))
    parts.append(K.cube(
        f"{asset_id}_soil", size=(planter - t * 2.0, planter - t * 2.0, 0.10),
        location=(0.0, 0.0, base_h + wall_h - 0.05), material=soil, bevel=0.02,
    ))
    parts.append(K.cube(
        f"{asset_id}_plaque", size=(0.26, 0.02, 0.16),
        location=(0.0, -(planter * 0.5 + 0.012), base_h + wall_h * 0.55),
        material=stone, bevel=0.012,
    ))

    # trunk: stacked tapered sections with raised banding between them
    trunk_z0 = base_h + wall_h - 0.02
    segs = 4
    for i in range(segs):
        r0 = 0.150 * (1.0 - 0.42 * i / segs)
        r1 = 0.150 * (1.0 - 0.42 * (i + 1) / segs)
        h = trunk_h / segs
        parts.append(K.cone(
            f"{asset_id}_trunk_{i}", radius1=r0, radius2=r1, depth=h, vertices=16,
            location=(0.0, 0.0, trunk_z0 + h * (i + 0.5)), material=trunk_mat,
        ))
        parts.append(K.torus(
            f"{asset_id}_trunk_band_{i}", major_radius=r1 * 1.12, minor_radius=0.022,
            major_segments=16, minor_segments=6,
            location=(0.0, 0.0, trunk_z0 + h * (i + 1)), material=trunk_mat,
        ))

    crown_z = trunk_z0 + trunk_h + 0.04
    for i in range(fronds):
        az = math.radians(i * (360.0 / fronds))
        droop = math.radians(-16.0 - 9.0 * (i % 3))
        reach = 0.30 + 0.07 * (i % 3)
        mat = leaf if i % 2 == 0 else leaf_dark
        frond = K.sphere(
            f"{asset_id}_frond_{i}", radius=1.0, segments=16, ring_count=8,
            location=(0.0, 0.0, 0.0), material=mat, scale=(0.92, 0.19, 0.075),
        )
        _at(frond,
            (math.cos(az) * reach, math.sin(az) * reach, crown_z + 0.04 * (i % 2)),
            (0.0, droop, az))
        parts.append(frond)
        # a shorter inner frond fills the gap between two long ones
        inner = K.sphere(
            f"{asset_id}_frond_inner_{i}", radius=1.0, segments=12, ring_count=8,
            location=(0.0, 0.0, 0.0), material=mat, scale=(0.50, 0.17, 0.070),
        )
        _at(inner,
            (math.cos(az) * 0.16, math.sin(az) * 0.16, crown_z + 0.14),
            (0.0, math.radians(-34.0), az + 0.28))
        parts.append(inner)

    for i in range(3):
        a = math.radians(200.0 + i * 60.0)
        parts.append(K.sphere(
            f"{asset_id}_coconut_{i}", radius=1.0, segments=12, ring_count=8,
            location=(math.cos(a) * 0.13, math.sin(a) * 0.13, crown_z - 0.06),
            material=nut, scale=(0.058, 0.058, 0.052),
        ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 9. parking meter
# ---------------------------------------------------------------------------


def build_parking_meter(
    asset_id: str,
    *,
    height: float = 1.34,
    body_rgb: Sequence[float] = (0.62, 0.66, 0.68),
    head_rgb: Sequence[float] = (0.86, 0.30, 0.16),
) -> list[bpy.types.Object]:
    """A single-space parking meter: post, head, solar hood, dial and card pad."""
    K.reset_scene()

    post_mat = P.paint(f"{asset_id}_post", body_rgb, metallic=0.65, roughness=0.42)
    head_mat = P.paint(f"{asset_id}_head", head_rgb, metallic=0.35, roughness=0.40)
    bezel = P.metal(f"{asset_id}_bezel", (0.80, 0.82, 0.85), rough=0.22, metal=0.95)
    glass = P.glass(f"{asset_id}_glass", tint=(0.70, 0.84, 0.88), rough=0.04)
    digits = P.emissive(f"{asset_id}_digits", (1.0, 0.66, 0.18), strength=3.0)
    led = P.emissive(f"{asset_id}_led", (0.20, 1.0, 0.45), strength=4.0)
    darks = K.pbr_material(f"{asset_id}_dark", (0.05, 0.05, 0.06), roughness=0.75)

    parts: list[bpy.types.Object] = []
    head_z = height - 0.18
    front = -0.12

    parts.append(K.cylinder(
        f"{asset_id}_base", radius=0.15, depth=0.07, vertices=20,
        location=(0.0, 0.0, 0.035), material=post_mat,
    ))
    parts.append(K.cone(
        f"{asset_id}_collar", radius1=0.095, radius2=0.065, depth=0.09,
        vertices=16, location=(0.0, 0.0, 0.115), material=post_mat,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_post", radius=0.055, depth=height - 0.28, vertices=16,
        location=(0.0, 0.0, 0.16 + (height - 0.28) * 0.5), material=post_mat,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_mid_band", radius=0.068, depth=0.05, vertices=16,
        location=(0.0, 0.0, 0.34), material=bezel,
    ))
    for i in range(3):
        a = math.radians(i * 120.0)
        parts.append(K.cylinder(
            f"{asset_id}_anchor_{i}", radius=0.018, depth=0.03, vertices=10,
            location=(math.cos(a) * 0.11, math.sin(a) * 0.11, 0.075),
            material=bezel,
        ))

    # head: body, recessed display, hood, coin gear and card pad
    parts.append(K.cube(
        f"{asset_id}_head_body", size=(0.20, 0.24, 0.36),
        location=(0.0, 0.0, head_z), material=head_mat, bevel=0.035,
        bevel_segments=3,
    ))
    parts.append(K.cone(
        f"{asset_id}_head_cap", radius1=0.13, radius2=0.085, depth=0.06,
        vertices=16, location=(0.0, 0.0, height + 0.01), material=head_mat,
    ))
    parts.append(K.cube(
        f"{asset_id}_display_bezel", size=(0.17, 0.03, 0.11),
        location=(0.0, front - 0.005, head_z + 0.09), material=bezel, bevel=0.012,
    ))
    parts.append(K.cube(
        f"{asset_id}_display_glass", size=(0.14, 0.02, 0.08),
        location=(0.0, front - 0.022, head_z + 0.09), material=glass,
    ))
    for i in range(3):
        parts.append(K.cube(
            f"{asset_id}_digit_{i}", size=(0.028, 0.012, 0.045),
            location=((i - 1) * 0.042, front - 0.034, head_z + 0.09),
            material=digits, bevel=0.004,
        ))
    hood = K.cube(
        f"{asset_id}_solar_hood", size=(0.21, 0.13, 0.035),
        location=(0.0, 0.0, 0.0), material=head_mat, bevel=0.012,
    )
    _at(hood, (0.0, front - 0.045, head_z + 0.175), (math.radians(18.0), 0.0, 0.0))
    parts.append(hood)

    parts.append(K.cube(
        f"{asset_id}_coin_slot", size=(0.03, 0.03, 0.09),
        location=(0.052, front - 0.01, head_z - 0.03), material=darks, bevel=0.006,
    ))
    knob = K.cylinder(
        f"{asset_id}_coin_knob", radius=0.030, depth=0.05, vertices=14,
        location=(0.0, 0.0, 0.0), material=bezel,
    )
    _at(knob, (0.052, front - 0.045, head_z - 0.10), (math.radians(90.0), 0.0, 0.0))
    parts.append(knob)
    parts.append(K.cube(
        f"{asset_id}_card_pad", size=(0.11, 0.03, 0.07),
        location=(-0.045, front - 0.008, head_z - 0.03), material=darks, bevel=0.01,
    ))
    parts.append(K.torus(
        f"{asset_id}_card_ring", major_radius=0.055, minor_radius=0.012,
        major_segments=16, minor_segments=6,
        location=(-0.045, front - 0.02, head_z - 0.03), material=bezel,
    ))
    parts.append(K.sphere(
        f"{asset_id}_status_led", radius=1.0, segments=12, ring_count=8,
        location=(0.0, front - 0.02, head_z + 0.16), material=led,
        scale=(0.022, 0.018, 0.022),
    ))
    parts.append(K.cube(
        f"{asset_id}_service_panel", size=(0.02, 0.14, 0.20),
        location=(0.0, 0.125, head_z - 0.02), material=darks, bevel=0.008,
    ))
    conduit = K.cylinder(
        f"{asset_id}_conduit", radius=0.020, depth=0.30, vertices=10,
        location=(0.0, 0.0, 0.0), material=bezel,
    )
    _at(conduit, (0.0, 0.10, head_z - 0.30), (0.0, 0.0, 0.0))
    parts.append(conduit)

    # Wire cage over the display and a locked lower door: without them the
    # head is a bare box, and a meter the player can smash needs a visible
    # protection cage.
    for i in range(5):
        t = (i / 4.0 - 0.5)
        bar = K.cylinder(
            f"{asset_id}_cage_bar_{i}", radius=0.006, depth=0.09, vertices=8,
            location=(0.0, 0.0, 0.0), material=bezel,
        )
        _at(bar, (t * 0.15, front - 0.045, head_z + 0.09),
            (math.radians(90.0), 0.0, 0.0))
        parts.append(bar)
    for i in range(2):
        rail = K.cube(
            f"{asset_id}_cage_rail_{i}", size=(0.17, 0.012, 0.012),
            location=(0.0, front - 0.045, head_z + 0.09 + (i * 2 - 1) * 0.042),
            material=bezel, bevel=0.004,
        )
        parts.append(rail)
    parts.append(K.cube(
        f"{asset_id}_lower_door", size=(0.16, 0.03, 0.14),
        location=(0.0, front - 0.004, head_z - 0.115), material=head_mat, bevel=0.012,
    ))
    key = K.cylinder(
        f"{asset_id}_door_key", radius=0.018, depth=0.025, vertices=12,
        location=(0.0, 0.0, 0.0), material=bezel,
    )
    _at(key, (0.052, front - 0.028, head_z - 0.115), (math.radians(90.0), 0.0, 0.0))
    parts.append(key)
    for i in range(3):
        tick = K.cube(
            f"{asset_id}_post_band_{i}", size=(0.12, 0.12, 0.025),
            location=(0.0, 0.0, 0.62 + i * 0.16), material=bezel, bevel=0.008,
        )
        parts.append(tick)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 10. phone booth
# ---------------------------------------------------------------------------


def build_phone_booth(
    asset_id: str,
    *,
    width: float = 0.95,
    depth: float = 0.85,
    height: float = 2.35,
    body_rgb: Sequence[float] = (0.80, 0.16, 0.12),
    sign_rgb: Sequence[float] = (0.20, 0.85, 1.0),
) -> list[bpy.types.Object]:
    """A glazed phone booth: kick panels, mullions, door glass, lit crown sign."""
    K.reset_scene()

    frame = P.paint(f"{asset_id}_frame", body_rgb, metallic=0.40, roughness=0.38)
    glass = P.glass(f"{asset_id}_glass", tint=(0.66, 0.80, 0.88), rough=0.04)
    sign = P.emissive(f"{asset_id}_sign", sign_rgb, strength=4.5)
    lamp = P.emissive(f"{asset_id}_lamp", (1.0, 0.92, 0.76), strength=5.0)
    phone = P.paint(f"{asset_id}_phone", (0.14, 0.14, 0.16), metallic=0.1, roughness=0.55)
    handset = P.paint(f"{asset_id}_handset", (0.06, 0.06, 0.07), metallic=0.0, roughness=0.42)
    darks = K.pbr_material(f"{asset_id}_dark", (0.05, 0.05, 0.06), roughness=0.75)

    parts: list[bpy.types.Object] = []
    w, d = width, depth
    plinth_h = 0.16
    post_h = height - plinth_h - 0.22
    post_z = plinth_h + post_h * 0.5
    panel_z0 = plinth_h + 0.62
    panel_h = height - panel_z0 - 0.40

    parts.append(K.cube(
        f"{asset_id}_plinth", size=(w, d, plinth_h), location=(0.0, 0.0, plinth_h * 0.5),
        material=frame, bevel=0.04,
    ))
    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            parts.append(K.cube(
                f"{asset_id}_post_{_side(sx, sy)}", size=(0.09, 0.09, post_h),
                location=(sx * (w * 0.5 - 0.045), sy * (d * 0.5 - 0.045), post_z),
                material=frame, bevel=0.022,
            ))

    # solid kick panels around the bottom third
    for sy in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_kick_{'b' if sy > 0 else 'f'}",
            size=(w - 0.18, 0.05, 0.62), location=(0.0, sy * (d * 0.5 - 0.045),
                                                  plinth_h + 0.31),
            material=frame, bevel=0.02,
        ))
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_kick_{'l' if sx < 0 else 'r'}",
            size=(0.05, d - 0.18, 0.62), location=(sx * (w * 0.5 - 0.045), 0.0,
                                                  plinth_h + 0.31),
            material=frame, bevel=0.02,
        ))

    # glazing: back and sides fixed, front is a door
    parts.append(K.cube(
        f"{asset_id}_glass_back", size=(w - 0.20, 0.03, panel_h),
        location=(0.0, d * 0.5 - 0.045, panel_z0 + panel_h * 0.5), material=glass,
    ))
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_glass_side_{'l' if sx < 0 else 'r'}",
            size=(0.03, d - 0.20, panel_h),
            location=(sx * (w * 0.5 - 0.045), 0.0, panel_z0 + panel_h * 0.5),
            material=glass,
        ))
    parts.append(K.cube(
        f"{asset_id}_glass_door", size=(w - 0.26, 0.03, panel_h),
        location=(0.0, -(d * 0.5 - 0.045), panel_z0 + panel_h * 0.5), material=glass,
    ))
    for sx in (-1.0, 1.0):
        parts.append(K.cube(
            f"{asset_id}_door_jamb_{'l' if sx < 0 else 'r'}", size=(0.05, 0.06, panel_h),
            location=(sx * (w * 0.5 - 0.10), -(d * 0.5 - 0.045),
                      panel_z0 + panel_h * 0.5),
            material=frame, bevel=0.014,
        ))
    parts.append(K.cube(
        f"{asset_id}_door_rail", size=(w - 0.15, 0.06, 0.05),
        location=(0.0, -(d * 0.5 - 0.045), panel_z0 + panel_h * 0.5), material=frame,
        bevel=0.014,
    ))
    handle = K.cylinder(
        f"{asset_id}_door_handle", radius=0.018, depth=0.26, vertices=12,
        location=(0.0, 0.0, 0.0), material=frame,
    )
    _at(handle, ((w * 0.5 - 0.16), -(d * 0.5 - 0.10), panel_z0 + panel_h * 0.5),
        (0.0, 0.0, 0.0))
    parts.append(handle)

    # roof, crown and the lit sign box above it
    parts.append(K.cube(
        f"{asset_id}_roof", size=(w + 0.14, d + 0.14, 0.16),
        location=(0.0, 0.0, height - 0.08), material=frame, bevel=0.045,
    ))
    crown = K.cone(
        f"{asset_id}_crown", radius1=(w + 0.02) * 0.5, radius2=0.16, depth=0.22,
        vertices=8, location=(0.0, 0.0, height + 0.11), material=frame,
    )
    _at(crown, (0.0, 0.0, height + 0.11), (0.0, 0.0, math.radians(22.5)))
    parts.append(crown)
    parts.append(K.cube(
        f"{asset_id}_sign_box", size=(w * 0.82, 0.10, 0.34),
        location=(0.0, -(d * 0.5 + 0.04), height - 0.06), material=frame, bevel=0.03,
    ))
    parts.append(K.cube(
        f"{asset_id}_sign_face", size=(w * 0.70, 0.04, 0.26),
        location=(0.0, -(d * 0.5 + 0.10), height - 0.06), material=sign,
    ))

    # interior: canopy lamp and the phone unit on the back wall
    parts.append(K.cube(
        f"{asset_id}_canopy", size=(w - 0.24, d - 0.24, 0.05),
        location=(0.0, 0.0, panel_z0 + panel_h - 0.04), material=frame, bevel=0.015,
    ))
    parts.append(K.cube(
        f"{asset_id}_interior_lamp", size=(0.40, 0.22, 0.05),
        location=(0.0, 0.0, panel_z0 + panel_h - 0.09), material=lamp,
    ))
    parts.append(K.cube(
        f"{asset_id}_phone_body", size=(0.34, 0.16, 0.30),
        location=(0.0, d * 0.5 - 0.16, panel_z0 + 0.42), material=phone, bevel=0.028,
    ))
    parts.append(K.cube(
        f"{asset_id}_phone_shelf", size=(0.42, 0.20, 0.04),
        location=(0.0, d * 0.5 - 0.18, panel_z0 + 0.25), material=phone, bevel=0.012,
    ))
    parts.append(K.cube(
        f"{asset_id}_handset", size=(0.10, 0.11, 0.26),
        location=(-0.10, d * 0.5 - 0.23, panel_z0 + 0.46), material=handset,
        bevel=0.03, bevel_segments=3,
    ))
    for sz in (-1.0, 1.0):
        cap = K.cylinder(
            f"{asset_id}_handset_end_{'t' if sz > 0 else 'b'}", radius=0.045,
            depth=0.07, vertices=12, location=(0.0, 0.0, 0.0), material=handset,
        )
        _at(cap, (-0.10, d * 0.5 - 0.23, panel_z0 + 0.46 + sz * 0.12),
            (0.0, 0.0, 0.0))
        parts.append(cap)
    for r in range(2):
        for c in range(3):
            parts.append(K.cube(
                f"{asset_id}_keypad_{r}{c}", size=(0.028, 0.02, 0.024),
                location=(0.05 + c * 0.036, d * 0.5 - 0.245, panel_z0 + 0.50 - r * 0.036),
                material=darks, bevel=0.005,
            ))
    parts.append(K.cube(
        f"{asset_id}_phone_cradle", size=(0.30, 0.09, 0.05),
        location=(-0.10, d * 0.5 - 0.20, panel_z0 + 0.32), material=phone, bevel=0.015,
    ))

    # Glazing beads around every panel. A pane with a raw cut edge reads as a
    # hole in the frame; a bead is what makes the glass look installed.
    for sy in (-1.0, 1.0):
        for sz in (-1.0, 1.0):
            parts.append(K.cube(
                f"{asset_id}_bead_b{sy}_{sz}", size=(w - 0.18, 0.045, 0.035),
                location=(0.0, sy * (d * 0.5 - 0.045),
                          panel_z0 + panel_h * (0.5 + sz * 0.5) - sz * 0.02),
                material=frame, bevel=0.008,
            ))
    for sx in (-1.0, 1.0):
        for sz in (-1.0, 1.0):
            parts.append(K.cube(
                f"{asset_id}_bead_s{sx}_{sz}", size=(0.045, d - 0.18, 0.035),
                location=(sx * (w * 0.5 - 0.045), 0.0,
                          panel_z0 + panel_h * (0.5 + sz * 0.5) - sz * 0.02),
                material=frame, bevel=0.008,
            ))

    # Kick-panel louvre band and the sill the door closes onto
    for i in range(5):
        parts.append(K.cube(
            f"{asset_id}_louvre_{i}", size=(0.30, 0.03, 0.025),
            location=(0.0, -(d * 0.5 - 0.075), plinth_h + 0.14 + i * 0.075),
            material=darks, bevel=0.006,
        ))
    parts.append(K.cube(
        f"{asset_id}_sill", size=(w - 0.10, 0.10, 0.05),
        location=(0.0, -(d * 0.5 - 0.02), plinth_h + 0.60),
        material=frame, bevel=0.014,
    ))
    for i in range(4):
        a = math.radians(45.0 + i * 90.0)
        parts.append(K.sphere(
            f"{asset_id}_crown_cap_{i}", radius=1.0, segments=10, ring_count=6,
            location=(math.cos(a) * 0.14, math.sin(a) * 0.14, height + 0.22),
            material=sign, scale=(0.035, 0.035, 0.028),
        ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 11. streetlight
# ---------------------------------------------------------------------------


def build_streetlight(
    asset_id: str,
    *,
    height: float = 6.40,
    arm: float = 1.70,
    column_rgb: Sequence[float] = (0.30, 0.33, 0.36),
    head_rgb: Sequence[float] = (0.56, 0.58, 0.60),
    lens_rgb: Sequence[float] = (1.0, 0.93, 0.76),
) -> list[bpy.types.Object]:
    """A cobra-head streetlight: fluted column, braced mast arm, emissive lens."""
    K.reset_scene()

    column = P.paint(f"{asset_id}_column", column_rgb, metallic=0.70, roughness=0.40)
    head_mat = P.paint(f"{asset_id}_head", head_rgb, metallic=0.65, roughness=0.34)
    trim = P.metal(f"{asset_id}_trim", (0.74, 0.76, 0.79), rough=0.22, metal=0.95)
    lens = P.emissive(f"{asset_id}_lens", lens_rgb, strength=6.0)

    parts: list[bpy.types.Object] = []
    plinth_h = 0.55
    post_top = height - 0.35
    pole_len = post_top - (plinth_h + 0.06)

    parts.append(K.cone(
        f"{asset_id}_plinth", radius1=0.28, radius2=0.20, depth=plinth_h,
        vertices=24, location=(0.0, 0.0, plinth_h * 0.5), material=column,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_base_collar", radius=0.215, depth=0.12, vertices=24,
        location=(0.0, 0.0, plinth_h + 0.06), material=column,
    ))
    parts.append(K.torus(
        f"{asset_id}_base_bead", major_radius=0.215, minor_radius=0.030,
        major_segments=24, minor_segments=8, location=(0.0, 0.0, plinth_h + 0.12),
        material=column,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_column", radius=0.10, depth=pole_len, vertices=24,
        location=(0.0, 0.0, plinth_h + 0.12 + pole_len * 0.5), material=column,
    ))
    parts.append(K.cube(
        f"{asset_id}_access_hatch", size=(0.05, 0.17, 0.26),
        location=(0.10, 0.0, 0.95), material=trim, bevel=0.015,
    ))
    parts.append(K.torus(
        f"{asset_id}_shaft_band", major_radius=0.108, minor_radius=0.022,
        major_segments=24, minor_segments=8, location=(0.0, 0.0, 1.60),
        material=column,
    ))

    # mast arm: vertical stub, elbow, horizontal reach, diagonal brace
    stub_top = post_top + 0.50
    parts.append(K.cylinder(
        f"{asset_id}_arm_stub", radius=0.078, depth=0.52, vertices=20,
        location=(0.0, 0.0, post_top + 0.26), material=column,
    ))
    parts.append(K.sphere(
        f"{asset_id}_arm_elbow", radius=0.105, segments=16, ring_count=8,
        location=(0.0, 0.0, stub_top), material=column,
    ))
    reach = K.cylinder(
        f"{asset_id}_arm_reach", radius=0.072, depth=arm, vertices=20,
        location=(0.0, 0.0, 0.0), material=column,
    )
    _at(reach, (arm * 0.5, 0.0, stub_top), (0.0, math.radians(90.0), 0.0))
    parts.append(reach)

    brace_h = stub_top - (post_top - 0.55)
    brace_run = arm * 0.48
    brace = K.cylinder(
        f"{asset_id}_arm_brace", radius=0.045,
        depth=math.hypot(brace_run, brace_h), vertices=14, location=(0.0, 0.0, 0.0),
        material=column,
    )
    _at(brace, (brace_run * 0.5, 0.0, post_top - 0.55 + brace_h * 0.5),
        (0.0, math.atan2(brace_run, brace_h), 0.0))
    parts.append(brace)
    parts.append(K.cylinder(
        f"{asset_id}_arm_clamp", radius=0.12, depth=0.09, vertices=16,
        location=(0.0, 0.0, post_top + 0.06), material=trim,
    ))

    # cobra head: neck, housing, visor and the lens itself
    head_x = arm * 0.94
    parts.append(K.cone(
        f"{asset_id}_head_neck", radius1=0.085, radius2=0.055, depth=0.18,
        vertices=16, location=(head_x, 0.0, stub_top - 0.04), material=trim,
    ))
    housing = K.sphere(
        f"{asset_id}_head_housing", radius=1.0, segments=20, ring_count=12,
        location=(0.0, 0.0, 0.0), material=head_mat, scale=(0.20, 0.36, 0.11),
    )
    _at(housing, (head_x, 0.0, stub_top - 0.10))
    parts.append(housing)
    visor = K.sphere(
        f"{asset_id}_head_visor", radius=1.0, segments=16, ring_count=10,
        location=(0.0, 0.0, 0.0), material=head_mat, scale=(0.17, 0.30, 0.055),
    )
    _at(visor, (head_x + 0.03, 0.0, stub_top - 0.03))
    parts.append(visor)
    lamp = K.sphere(
        f"{asset_id}_head_lens", radius=1.0, segments=20, ring_count=12,
        location=(0.0, 0.0, 0.0), material=lens, scale=(0.155, 0.275, 0.045),
    )
    _at(lamp, (head_x, 0.0, stub_top - 0.175))
    parts.append(lamp)
    parts.append(K.cube(
        f"{asset_id}_gear_box", size=(0.14, 0.10, 0.12),
        location=(head_x + 0.05, 0.0, stub_top + 0.03), material=trim, bevel=0.02,
    ))
    for i in range(2):
        bolt = K.cylinder(
            f"{asset_id}_arm_bolt_{i}", radius=0.018, depth=0.16, vertices=10,
            location=(0.0, 0.0, 0.0), material=trim,
        )
        _at(bolt, (arm * 0.30 + i * 0.16, 0.0, stub_top + 0.03),
            (0.0, math.radians(90.0), 0.0))
        parts.append(bolt)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 12. traffic cone
# ---------------------------------------------------------------------------


def build_traffic_cone(
    asset_id: str,
    *,
    height: float = 0.72,
    cone_rgb: Sequence[float] = (0.94, 0.32, 0.06),
    band_rgb: Sequence[float] = (0.95, 0.95, 0.93),
) -> list[bpy.types.Object]:
    """A moulded traffic cone: ribbed body, two reflective bands, square base."""
    K.reset_scene()

    cone_mat = P.paint(f"{asset_id}_cone", cone_rgb, metallic=0.0, roughness=0.52)
    band = P.paint(f"{asset_id}_band", band_rgb, metallic=0.05, roughness=0.34)
    base_mat = P.paint(f"{asset_id}_base", (0.20, 0.20, 0.21), metallic=0.0, roughness=0.78)
    label = P.paint(f"{asset_id}_label", (0.10, 0.10, 0.11), metallic=0.0, roughness=0.40)

    parts: list[bpy.types.Object] = []
    base_h = 0.09
    r_bot = 0.17
    r_top = 0.045
    body_h = height - base_h - 0.05
    body_z0 = base_h
    body_z = body_z0 + body_h * 0.5

    def radius_at(z: float) -> float:
        """Outer radius of the cone body at a given height."""
        t = (z - body_z0) / body_h
        return r_bot + (r_top - r_bot) * t

    parts.append(K.cube(
        f"{asset_id}_base", size=(0.42, 0.42, 0.05), location=(0.0, 0.0, 0.025),
        material=base_mat, bevel=0.022,
    ))
    parts.append(K.cube(
        f"{asset_id}_skirt", size=(0.44, 0.44, 0.04), location=(0.0, 0.0, 0.07),
        material=base_mat, bevel=0.018,
    ))
    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            parts.append(K.cube(
                f"{asset_id}_foot_{_side(sx, sy)}", size=(0.11, 0.11, 0.035),
                location=(sx * 0.16, sy * 0.16, 0.0175), material=base_mat, bevel=0.012,
            ))
            bolt = K.cylinder(
                f"{asset_id}_anchor_{_side(sx, sy)}", radius=0.016, depth=0.02,
                vertices=10, location=(sx * 0.19, sy * 0.19, 0.05), material=base_mat,
            )
            parts.append(bolt)

    parts.append(K.cone(
        f"{asset_id}_body", radius1=r_bot, radius2=r_top, depth=body_h, vertices=32,
        location=(0.0, 0.0, body_z), material=cone_mat,
    ))
    parts.append(K.cone(
        f"{asset_id}_tip", radius1=r_top, radius2=0.0, depth=0.05, vertices=32,
        location=(0.0, 0.0, height - 0.025), material=cone_mat,
    ))

    # moulded ribs: the detail that separates a real cone from a party hat
    for i, z in enumerate((0.14, 0.24, 0.34)):
        parts.append(K.torus(
            f"{asset_id}_rib_{i}", major_radius=radius_at(z) + 0.002, minor_radius=0.017,
            major_segments=20, minor_segments=6, location=(0.0, 0.0, z),
            material=cone_mat,
        ))

    # two reflective bands, each modelled as a flared sleeve over the body
    for i, (z0, z1) in enumerate(((0.245, 0.365), (0.465, 0.575))):
        parts.append(K.cone(
            f"{asset_id}_band_{i}",
            radius1=radius_at(z0) + 0.009, radius2=radius_at(z1) + 0.009,
            depth=z1 - z0, vertices=32, location=(0.0, 0.0, (z0 + z1) * 0.5),
            material=band,
        ))

    parts.append(K.cube(
        f"{asset_id}_label", size=(0.09, 0.012, 0.07),
        location=(0.0, -(radius_at(0.21) + 0.008), 0.21), material=label, bevel=0.005,
    ))

    # Handle boss and grip: the detail that makes this a moulded traffic cone
    # rather than a stacked pair of party hats.
    for sx in (-1.0, 1.0):
        grip = K.cube(
            f"{asset_id}_handle_grip_{'l' if sx < 0 else 'r'}", size=(0.05, 0.045, 0.09),
            location=(sx * (r_top + 0.030), 0.0, height - 0.085),
            material=cone_mat, bevel=0.014,
        )
        parts.append(grip)
    bridge = K.cube(
        f"{asset_id}_handle_bridge", size=(2 * (r_top + 0.030) + 0.05, 0.05, 0.05),
        location=(0.0, 0.0, height - 0.045), material=cone_mat, bevel=0.016,
    )
    parts.append(bridge)
    for sx in (-1.0, 1.0):
        parts.append(K.sphere(
            f"{asset_id}_grip_rib_{'l' if sx < 0 else 'r'}", radius=1.0,
            segments=10, ring_count=6,
            location=(sx * (r_top + 0.030), 0.0, height - 0.085),
            material=band, scale=(0.055, 0.048, 0.016),
        ))
    # collar where the body meets the skirt, and the four skirt chamfers
    parts.append(K.torus(
        f"{asset_id}_collar", major_radius=r_bot * 0.99, minor_radius=0.016,
        major_segments=24, minor_segments=6, location=(0.0, 0.0, base_h + 0.012),
        material=base_mat,
    ))
    for i in range(4):
        a = math.radians(45.0 + i * 90.0)
        chamfer = K.cube(
            f"{asset_id}_skirt_chamfer_{i}", size=(0.09, 0.09, 0.03),
            location=(0.0, 0.0, 0.0), material=base_mat, bevel=0.01,
        )
        _at(chamfer, (math.cos(a) * 0.165, math.sin(a) * 0.165, 0.095), (0.0, 0.0, a))
        parts.append(chamfer)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 13. traffic light
# ---------------------------------------------------------------------------


def build_trafficlight(
    asset_id: str,
    *,
    pole_height: float = 5.80,
    mast_arm: float = 2.40,
    body_rgb: Sequence[float] = (0.13, 0.14, 0.16),
    case_rgb: Sequence[float] = (0.16, 0.17, 0.19),
) -> list[bpy.types.Object]:
    """A mast-arm signal: back-to-back heads, visored lenses, backboards.

    Every lens is its own emissive material with its own colour — red, amber
    and green are what tell the player what the light is doing at a glance, and
    a single shared emissive would collapse that.
    """
    K.reset_scene()

    pole = P.paint(f"{asset_id}_pole", body_rgb, metallic=0.72, roughness=0.38)
    case = P.paint(f"{asset_id}_case", case_rgb, metallic=0.60, roughness=0.34)
    board = P.paint(f"{asset_id}_board", (0.09, 0.30, 0.16), metallic=0.2, roughness=0.56)
    trim = P.metal(f"{asset_id}_trim", (0.76, 0.78, 0.81), rough=0.24, metal=0.95)
    lens_red = P.emissive(f"{asset_id}_lens_red", (1.0, 0.09, 0.05), strength=6.0)
    lens_amber = P.emissive(f"{asset_id}_lens_amber", (1.0, 0.60, 0.05), strength=6.0)
    lens_green = P.emissive(f"{asset_id}_lens_green", (0.08, 1.0, 0.35), strength=6.0)
    lens_mats = (lens_red, lens_amber, lens_green)
    lens_tags = ("red", "amber", "green")

    parts: list[bpy.types.Object] = []
    plinth_h = 0.42
    pole_z0 = plinth_h + 0.10

    parts.append(K.cone(
        f"{asset_id}_plinth", radius1=0.30, radius2=0.19, depth=plinth_h,
        vertices=24, location=(0.0, 0.0, plinth_h * 0.5), material=pole,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_base_collar", radius=0.20, depth=0.10, vertices=24,
        location=(0.0, 0.0, plinth_h + 0.05), material=pole,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_pole", radius=0.095, depth=pole_height - pole_z0, vertices=24,
        location=(0.0, 0.0, pole_z0 + (pole_height - pole_z0) * 0.5), material=pole,
    ))
    parts.append(K.cube(
        f"{asset_id}_access_door", size=(0.05, 0.20, 0.32),
        location=(0.10, 0.0, 0.85), material=trim, bevel=0.018,
    ))
    for i in range(3):
        a = math.radians(i * 120.0 + 30.0)
        parts.append(K.cylinder(
            f"{asset_id}_anchor_{i}", radius=0.022, depth=0.05, vertices=10,
            location=(math.cos(a) * 0.23, math.sin(a) * 0.23, plinth_h + 0.02),
            material=trim,
        ))

    # mast arm off the top of the column, with a knee brace
    arm_z = pole_height - 0.60
    reach = K.cylinder(
        f"{asset_id}_mast_arm", radius=0.075, depth=mast_arm, vertices=20,
        location=(0.0, 0.0, 0.0), material=pole,
    )
    _at(reach, (mast_arm * 0.5, 0.0, arm_z), (0.0, math.radians(90.0), 0.0))
    parts.append(reach)
    brace_h = 0.70
    brace = K.cylinder(
        f"{asset_id}_knee_brace", radius=0.045,
        depth=math.hypot(mast_arm * 0.34, brace_h), vertices=14,
        location=(0.0, 0.0, 0.0), material=pole,
    )
    _at(brace, (mast_arm * 0.17, 0.0, arm_z - brace_h * 0.5),
        (0.0, math.atan2(mast_arm * 0.34, brace_h), 0.0))
    parts.append(brace)
    parts.append(K.cylinder(
        f"{asset_id}_arm_clamp", radius=0.135, depth=0.10, vertices=16,
        location=(0.0, 0.0, arm_z), material=trim,
    ))
    parts.append(K.cylinder(
        f"{asset_id}_pole_cap", radius=0.115, depth=0.07, vertices=16,
        location=(0.0, 0.0, pole_height + 0.02), material=pole,
    ))

    def _head(tag: str, cx: float, cy: float, front: float, centre_z: float) -> None:
        """One signal head facing `front` in Y (-1 faces -Y, the camera)."""
        hx, hy = cx, cy
        parts.append(K.cube(
            f"{asset_id}_case_{tag}", size=(0.36, 0.30, 1.10),
            location=(hx, hy, centre_z), material=case, bevel=0.04,
            bevel_segments=3,
        ))
        parts.append(K.cube(
            f"{asset_id}_backboard_{tag}", size=(0.62, 0.035, 1.26),
            location=(hx, hy - front * 0.17, centre_z), material=board, bevel=0.022,
        ))
        for i, z in enumerate((0.34, 0.0, -0.34)):
            visor = K.cone(
                f"{asset_id}_visor_{tag}_{lens_tags[i]}", radius1=0.105,
                radius2=0.155, depth=0.16, vertices=24, location=(0.0, 0.0, 0.0),
                material=case,
            )
            _at(visor, (hx, hy + front * 0.22, centre_z + z),
                (math.radians(-90.0 * front), 0.0, 0.0))
            parts.append(visor)

            lens = K.cylinder(
                f"{asset_id}_lens_{tag}_{lens_tags[i]}", radius=0.108, depth=0.05,
                vertices=24, location=(0.0, 0.0, 0.0), material=lens_mats[i],
            )
            _at(lens, (hx, hy + front * 0.145, centre_z + z),
                (math.radians(-90.0 * front), 0.0, 0.0))
            parts.append(lens)

            ring = K.torus(
                f"{asset_id}_lens_ring_{tag}_{lens_tags[i]}", major_radius=0.122,
                minor_radius=0.018, major_segments=20, minor_segments=6,
                location=(0.0, 0.0, 0.0), material=case,
            )
            _at(ring, (hx, hy + front * 0.165, centre_z + z),
                (math.radians(90.0), 0.0, 0.0))
            parts.append(ring)

        hanger = K.cube(
            f"{asset_id}_hanger_{tag}", size=(0.10, 0.10, 0.26),
            location=(0.0, 0.0, 0.0), material=trim, bevel=0.018,
        )
        _at(hanger, (hx, hy, centre_z + 0.68), (0.0, 0.0, 0.0))
        parts.append(hanger)

    # near head faces the camera (-Y); the far head hangs off the mast arm
    _head("near", 0.0, 0.0, -1.0, pole_height - 1.90)
    _head("far", mast_arm * 0.92, 0.0, 1.0, arm_z - 1.05)

    # pedestrian signal box under the near head
    ped_z = pole_height - 3.05
    parts.append(K.cube(
        f"{asset_id}_ped_case", size=(0.30, 0.24, 0.52),
        location=(0.0, -0.02, ped_z), material=case, bevel=0.035, bevel_segments=3,
    ))
    for i, z in enumerate((0.12, -0.12)):
        lens = K.cylinder(
            f"{asset_id}_ped_lens_{i}", radius=0.082, depth=0.045, vertices=20,
            location=(0.0, 0.0, 0.0),
            material=lens_green if i == 1 else lens_red,
        )
        _at(lens, (0.0, -0.145, ped_z + z), (math.radians(90.0), 0.0, 0.0))
        parts.append(lens)
        hood = K.cone(
            f"{asset_id}_ped_hood_{i}", radius1=0.088, radius2=0.115, depth=0.11,
            vertices=20, location=(0.0, 0.0, 0.0), material=case,
        )
        _at(hood, (0.0, -0.215, ped_z + z), (math.radians(90.0), 0.0, 0.0))
        parts.append(hood)
    parts.append(K.cube(
        f"{asset_id}_ped_arm", size=(0.08, 0.08, 0.45),
        location=(0.0, 0.02, ped_z + 0.48), material=trim, bevel=0.018,
    ))

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 14. trash bin
# ---------------------------------------------------------------------------


def build_trash_bin(
    asset_id: str,
    *,
    radius: float = 0.30,
    height: float = 0.95,
    body_rgb: Sequence[float] = (0.12, 0.30, 0.26),
    slats: int = 12,
) -> list[bpy.types.Object]:
    """A slatted street bin: ribbed tapered drum, rolled rim, bagged liner."""
    K.reset_scene()

    body_mat = P.paint(f"{asset_id}_body", body_rgb, metallic=0.45, roughness=0.48)
    rim_mat = P.metal(f"{asset_id}_rim", (0.52, 0.55, 0.58), rough=0.30, metal=0.9)
    bag = P.rubber(f"{asset_id}_liner")
    lid_mat = P.paint(f"{asset_id}_lid", (0.08, 0.22, 0.19), metallic=0.45, roughness=0.42)

    parts: list[bpy.types.Object] = []
    drum_h = height - 0.30

    parts.append(K.cylinder(
        f"{asset_id}_base_ring", radius=radius * 0.80, depth=0.05, vertices=24,
        location=(0.0, 0.0, 0.025), material=rim_mat,
    ))
    for i in range(3):
        a = math.radians(i * 120.0)
        parts.append(K.cube(
            f"{asset_id}_foot_{i}", size=(0.10, 0.10, 0.055),
            location=(math.cos(a) * radius * 0.62, math.sin(a) * radius * 0.62, 0.0275),
            material=rim_mat, bevel=0.016,
        ))

    parts.append(K.cone(
        f"{asset_id}_drum", radius1=radius * 0.88, radius2=radius, depth=drum_h,
        vertices=28, location=(0.0, 0.0, 0.05 + drum_h * 0.5), material=body_mat,
    ))
    for i in range(slats):
        a = math.radians(i * (360.0 / slats))
        rib = K.cube(
            f"{asset_id}_slat_{i}", size=(0.035, 0.055, drum_h * 0.92),
            location=(0.0, 0.0, 0.0), material=body_mat, bevel=0.012,
        )
        r = radius * 0.96
        _at(rib, (math.cos(a) * r, math.sin(a) * r, 0.05 + drum_h * 0.5),
            (0.0, 0.0, a))
        parts.append(rib)

    parts.append(K.torus(
        f"{asset_id}_rim", major_radius=radius * 0.99, minor_radius=0.032,
        major_segments=28, minor_segments=8, location=(0.0, 0.0, 0.05 + drum_h),
        material=rim_mat,
    ))

    # hinged lid, propped slightly open, with a bag bulging out of the mouth
    lid = K.cone(
        f"{asset_id}_lid", radius1=radius * 1.08, radius2=radius * 0.30, depth=0.13,
        vertices=28, location=(0.0, 0.0, 0.0), material=lid_mat,
    )
    _at(lid, (0.0, radius * 0.16, height - 0.10), (math.radians(-26.0), 0.0, 0.0))
    parts.append(lid)
    parts.append(K.sphere(
        f"{asset_id}_liner", radius=1.0, segments=20, ring_count=12,
        location=(0.0, 0.0, height - 0.14), material=bag,
        scale=(radius * 0.82, radius * 0.82, 0.20),
    ))
    knob = K.cylinder(
        f"{asset_id}_lid_knob", radius=0.055, depth=0.06, vertices=14,
        location=(0.0, 0.0, 0.0), material=rim_mat,
    )
    _at(knob, (0.0, -radius * 0.30, height + 0.08), (0.0, 0.0, 0.0))
    parts.append(knob)
    for sx in (-1.0, 1.0):
        hinge = K.cylinder(
            f"{asset_id}_hinge_{'l' if sx < 0 else 'r'}", radius=0.030, depth=0.09,
            vertices=12, location=(0.0, 0.0, 0.0), material=rim_mat,
        )
        _at(hinge, (sx * radius * 0.62, radius * 0.72, height - 0.17),
            (0.0, math.radians(90.0), 0.0))
        parts.append(hinge)
        handle = K.torus(
            f"{asset_id}_handle_{'l' if sx < 0 else 'r'}", major_radius=0.070,
            minor_radius=0.016, major_segments=16, minor_segments=6,
            location=(0.0, 0.0, 0.0), material=rim_mat,
        )
        _at(handle, (sx * (radius + 0.03), 0.0, height - 0.30),
            (0.0, math.radians(90.0), 0.0))
        parts.append(handle)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# registry — (asset_id, builder, kwargs)
# ---------------------------------------------------------------------------

PROPS: list[tuple[str, Callable[..., list[bpy.types.Object]], dict]] = [
    ("prop_bench", build_bench, dict(
        length=1.90, wood_rgb=(0.42, 0.24, 0.11), frame_rgb=(0.08, 0.19, 0.17))),
    ("prop_construction_barrier", build_construction_barrier, dict(
        width=2.00, board_rgb=(0.93, 0.92, 0.88), stripe_rgb=(0.95, 0.38, 0.06))),
    ("prop_dumpster", build_dumpster, dict(
        width=1.10, length=1.90, height=1.28,
        body_rgb=(0.10, 0.26, 0.20), lid_rgb=(0.07, 0.19, 0.15))),
    ("prop_fire_hydrant", build_fire_hydrant, dict(
        height=0.78, body_rgb=(0.86, 0.14, 0.10), cap_rgb=(0.94, 0.80, 0.18))),
    ("prop_mailbox", build_mailbox, dict(
        post_height=0.95, body_rgb=(0.18, 0.34, 0.52))),
    ("prop_manhole_cover", build_manhole_cover, dict(
        diameter=0.84, frame_rgb=(0.22, 0.20, 0.19), iron_rgb=(0.13, 0.13, 0.14))),
    ("prop_newsstand", build_newsstand, dict(
        width=1.55, depth=0.95, height=2.05,
        body_rgb=(0.16, 0.30, 0.42), sign_rgb=(1.0, 0.34, 0.42))),
    ("prop_palm_planter", build_palm_planter, dict(
        planter=1.25, wall_h=0.62, fronds=9, trunk_h=1.95)),
    ("prop_parking_meter", build_parking_meter, dict(
        height=1.34, body_rgb=(0.62, 0.66, 0.68), head_rgb=(0.86, 0.30, 0.16))),
    ("prop_phone_booth", build_phone_booth, dict(
        width=0.95, depth=0.85, height=2.35,
        body_rgb=(0.80, 0.16, 0.12), sign_rgb=(0.20, 0.85, 1.0))),
    ("prop_streetlight", build_streetlight, dict(
        height=6.40, arm=1.70, lens_rgb=(1.0, 0.93, 0.76))),
    ("prop_traffic_cone", build_traffic_cone, dict(
        height=0.72, cone_rgb=(0.94, 0.32, 0.06), band_rgb=(0.95, 0.95, 0.93))),
    ("prop_trafficlight", build_trafficlight, dict(
        pole_height=5.80, mast_arm=2.40, body_rgb=(0.13, 0.14, 0.16))),
    ("prop_trash_bin", build_trash_bin, dict(
        radius=0.30, height=0.95, body_rgb=(0.12, 0.30, 0.26), slats=12)),
]
