"""3A vehicle builder — cars, buses, aircraft, boats.

A 3A car is not a box with wheels. It reads as manufactured because of four
things, all of which this module implements:

1. **Bevelled edges everywhere.** A razor 90-degree edge catches the specular
   highlight as a hard line; a 2-3 cm chamfer rolls it off. This is the single
   biggest silhouette-vs-low-poly tell.
2. **Layered materials.** Paint is metallic + low roughness; glass is
   transmissive; tyres are rough non-metal; chrome is metallic + mirror. One
   material per part, never one material for the whole car.
3. **Massing first.** A coupe's greenhouse is narrower than its body and set
   back from the nose. Get the massing wrong and no amount of surface detail
   reads correctly.
4. **Distinct parts.** body / glass / tyres / hubs / lights are separate
   objects, because the renderer animates wheels and picks parts by name.
"""

from __future__ import annotations

import math
from typing import Sequence

import bpy

from .. import kernel as K

# Every car is authored Z-up with -Y forward; the exporter rotates to Y-up.
# Wheels therefore spin about local X after the Z-up→Y-up mapping.


def _paint(name: str, rgb: Sequence[float], metallic: float = 0.85, roughness: float = 0.22):
    return K.pbr_material(name, rgb, roughness=roughness, metallic=metallic)


def _glass(name: str = "glass"):
    return K.pbr_material(
        name, (0.62, 0.78, 0.86), roughness=0.03, metallic=0.0,
        alpha=0.22, transmission=0.92, ior=1.52,
    )


def _rubber(name: str = "rubber"):
    return K.pbr_material(name, (0.035, 0.035, 0.04), roughness=0.88, metallic=0.0)


def _chrome(name: str = "chrome"):
    return K.pbr_material(name, (0.86, 0.88, 0.92), roughness=0.08, metallic=1.0)


def _light(name: str, rgb, strength: float = 4.0):
    return K.pbr_material(name, rgb, roughness=0.25, metallic=0.0,
                          emission=rgb, emission_strength=strength)


def _dark_trim(name: str = "trim"):
    return K.pbr_material(name, (0.07, 0.07, 0.08), roughness=0.55, metallic=0.15)


def _wheel(
    prefix: str,
    centre: Sequence[float],
    radius: float,
    width: float,
    rim_ratio: float = 0.62,
    segments: int = 28,
) -> tuple[bpy.types.Object, bpy.types.Object]:
    """One wheel: a tyre torus-ish cylinder plus a separate hub disc.

    Returns (tyre, hub) so the caller keeps them separately addressable —
    the renderer spins the hub for rolling and names parts `tyres` / `hubs`.
    """
    x, y, z = centre

    # tyre: a cylinder laid on its side, i.e. rotating 90 deg about Y so its
    # axis runs along X (the wheel's axle in authoring space).
    tyre = K.cylinder(
        f"tyre_{prefix}", radius=radius, depth=width, vertices=segments,
        location=(x, y, z), material=_rubber(f"rubber_{prefix}"), smooth=True,
    )
    tyre.rotation_euler = (0.0, math.radians(90.0), 0.0)
    K.transform_apply(tyre)

    hub = K.cylinder(
        f"hub_{prefix}", radius=radius * rim_ratio, depth=width * 1.06, vertices=segments,
        location=(x, y, z), material=_chrome(f"chrome_{prefix}"), smooth=True,
    )
    hub.rotation_euler = (0.0, math.radians(90.0), 0.0)
    K.transform_apply(hub)

    # a 5-spoke pattern makes the hub read as a wheel rather than a disc
    for i in range(5):
        # Built at the world origin, rotated there, THEN translated to the hub.
        # Rotating after placement spins the part around the car instead of
        # around its own axle, which throws spokes metres below the ground.
        spoke = K.cube(
            f"spoke_{prefix}_{i}",
            size=(width * 0.5, radius * rim_ratio * 0.86, radius * 0.13),
            location=(0.0, 0.0, 0.0),
            material=_chrome(f"chrome_{prefix}_{i}"),
            bevel=0.008,
        )
        spoke.rotation_euler = (math.radians(i * 72.0), 0.0, 0.0)
        K.transform_apply(spoke)
        spoke.location = (x, y, z)
        K.transform_apply(spoke)
    return tyre, hub


def build_sedan(
    asset_id: str,
    paint_rgb: Sequence[float],
    *,
    length: float = 4.6,
    width: float = 1.9,
    body_height: float = 0.62,
    cabin_length: float = 2.2,
    cabin_height: float = 0.62,
    wheel_radius: float = 0.34,
    wheelbase: float = 2.7,
    track: float = 1.62,
) -> list[bpy.types.Object]:
    """A four-door saloon, Z-up, resting with tyres on z=0.

    `paint_rgb` lets one builder produce sedan / taxi / police without
    duplicating geometry.
    """
    K.reset_scene()

    paint = _paint(f"{asset_id}_paint", paint_rgb)
    glass = _glass(f"{asset_id}_glass")
    chrome = _chrome(f"{asset_id}_chrome")
    trim = _dark_trim(f"{asset_id}_trim")
    head = _light(f"{asset_id}_headlight", (1.0, 0.96, 0.86), 5.0)
    tail = _light(f"{asset_id}_taillight", (1.0, 0.12, 0.08), 3.0)

    z_body = wheel_radius + body_height * 0.5

    parts: list[bpy.types.Object] = []

    # --- lower body ------------------------------------------------------
    # Massing: the lower body is full width; the greenhouse above is narrower
    # and set back from the nose. That two-tier silhouette is what makes a car
    # read as a car rather than a slab.
    lower = K.cube(
        f"{asset_id}_body_lower",
        size=(width, length, body_height),
        location=(0.0, 0.0, z_body),
        material=paint,
        bevel=0.09,
        bevel_segments=3,
    )
    K.shade_smooth(lower, angle=math.radians(38.0))
    parts.append(lower)

    # bonnet + boot decks: slightly lower, so the profile is not one slab
    deck_front = K.cube(
        f"{asset_id}_bonnet", size=(width * 0.94, length * 0.34, body_height * 0.34),
        location=(0.0, -length * 0.30, z_body + body_height * 0.42), material=paint,
        bevel=0.06,
    )
    deck_rear = K.cube(
        f"{asset_id}_boot", size=(width * 0.94, length * 0.28, body_height * 0.30),
        location=(0.0, length * 0.31, z_body + body_height * 0.40), material=paint,
        bevel=0.06,
    )
    K.shade_smooth(deck_front, angle=math.radians(38.0))
    K.shade_smooth(deck_rear, angle=math.radians(38.0))
    parts += [deck_front, deck_rear]

    # --- greenhouse ------------------------------------------------------
    # The cabin must REST ON the lower body: the body's top face is at
    # z_body + body_height/2, so the cabin's bottom edge goes just below that.
    # Centring the cabin on `z_body + body_height + ...` instead leaves a
    # visible air gap that no amount of surface detail hides.
    cabin_w = width * 0.86
    body_top = z_body + body_height * 0.5
    cabin_z = body_top + cabin_height * 0.5 - 0.08
    cabin = K.cube(
        f"{asset_id}_cabin",
        size=(cabin_w, cabin_length, cabin_height),
        location=(0.0, length * 0.02, cabin_z),
        material=paint, bevel=0.14, bevel_segments=3,
    )
    K.shade_smooth(cabin, angle=math.radians(40.0))
    parts.append(cabin)

    # glazing sits INSIDE the cabin volume: pushing it proud would read as a
    # stuck-on box, which is exactly the defect the recess checker looks for.
    glass_w = cabin_w + 0.035
    windscreen = K.cube(
        f"{asset_id}_glass_front", size=(glass_w, 0.06, cabin_height * 0.82),
        location=(0.0, -cabin_length * 0.5 + 0.05, cabin_z + 0.02), material=glass,
    )
    rear = K.cube(
        f"{asset_id}_glass_rear", size=(glass_w, 0.06, cabin_height * 0.80),
        location=(0.0, cabin_length * 0.5 - 0.05, cabin_z + 0.02), material=glass,
    )
    side_l = K.cube(
        f"{asset_id}_glass_side_l", size=(0.06, cabin_length * 0.74, cabin_height * 0.66),
        location=(-cabin_w * 0.5 + 0.02, 0.0, cabin_z + 0.04), material=glass,
    )
    side_r = K.cube(
        f"{asset_id}_glass_side_r", size=(0.06, cabin_length * 0.74, cabin_height * 0.66),
        location=(cabin_w * 0.5 - 0.02, 0.0, cabin_z + 0.04), material=glass,
    )
    parts += [windscreen, rear, side_l, side_r]

    # --- lights ----------------------------------------------------------
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        hl = K.cube(
            f"{asset_id}_headlight_{tag}",
            size=(width * 0.26, 0.10, body_height * 0.34),
            location=(sx * width * 0.33, -length * 0.5 - 0.01, z_body + body_height * 0.08),
            material=head, bevel=0.02,
        )
        tl = K.cube(
            f"{asset_id}_taillight_{tag}",
            size=(width * 0.24, 0.09, body_height * 0.32),
            location=(sx * width * 0.34, length * 0.5 + 0.01, z_body + body_height * 0.10),
            material=tail, bevel=0.02,
        )
        parts += [hl, tl]

    # --- grille + bumpers ------------------------------------------------
    grille = K.cube(
        f"{asset_id}_grille", size=(width * 0.52, 0.07, body_height * 0.30),
        location=(0.0, -length * 0.5 - 0.02, z_body - body_height * 0.06),
        material=trim, bevel=0.015,
    )
    bumper_f = K.cube(
        f"{asset_id}_bumper_front", size=(width * 0.98, 0.10, body_height * 0.24),
        location=(0.0, -length * 0.5 - 0.03, z_body - body_height * 0.30),
        material=trim, bevel=0.03,
    )
    bumper_r = K.cube(
        f"{asset_id}_bumper_rear", size=(width * 0.98, 0.10, body_height * 0.24),
        location=(0.0, length * 0.5 + 0.03, z_body - body_height * 0.30),
        material=trim, bevel=0.03,
    )
    parts += [grille, bumper_f, bumper_r]

    # side mirrors: small, and a car without them reads as a 1990 prop
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        mirror = K.cube(
            f"{asset_id}_mirror_{tag}", size=(0.16, 0.10, 0.07),
            location=(sx * (cabin_w * 0.5 + 0.10), -cabin_length * 0.34, cabin_z + 0.04),
            material=paint, bevel=0.02,
        )
        parts.append(mirror)

    # exhaust
    exhaust = K.cylinder(
        f"{asset_id}_exhaust", radius=0.055, depth=0.16, vertices=12,
        location=(width * 0.26, length * 0.5 + 0.10, z_body - body_height * 0.28),
        material=chrome, smooth=True,
    )
    exhaust.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(exhaust)
    parts.append(exhaust)

    # --- wheels ----------------------------------------------------------
    half_track = track * 0.5
    axle_f = -wheelbase * 0.5
    axle_r = wheelbase * 0.5
    for (ax, tag_axle) in ((axle_f, "f"), (axle_r, "r")):
        for (sx, tag_side) in ((-1.0, "l"), (1.0, "r")):
            centre = (sx * half_track, ax, wheel_radius)
            tyre, hub = _wheel(f"{tag_axle}{tag_side}", centre, wheel_radius, 0.22)
            parts.append(tyre)
            parts.append(hub)
            for i in range(5):
                spoke = bpy.data.objects.get(f"spoke_{tag_axle}{tag_side}_{i}")
                if spoke is not None:
                    parts.append(spoke)

    # UV every part: a PBR master with no UVs cannot take a texture later.
    # Triangulate first so the glTF exporter can compute tangents (it skips
    # n-gons, which every cylinder cap and beveled face still is at this point).
    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)

    return parts


def _lightbar(asset_id, rgb, width, z, y):
    """Roof light bar used by police / taxi — the one part that makes a
    civilian saloon read as a service vehicle at a glance."""
    parts = []
    base = K.cube(
        f"{asset_id}_lightbar_base",
        size=(width * 0.62, 0.10, 0.05), location=(0.0, y, z),
        material=_dark_trim(f"{asset_id}_lbb"), bevel=0.012,
    )
    parts.append(base)
    for i, (dx, frac) in enumerate(((-0.17, 0.30), (0.17, 0.30))):
        lens = K.cube(
            f"{asset_id}_lightbar_lens{i}",
            size=(width * frac, 0.09, 0.07),
            location=(dx * width, y, z + 0.055),
            material=_light(f"{asset_id}_lb_lens{i}", rgb, 6.0),
            bevel=0.02,
        )
        parts.append(lens)
    return parts


def build_taxi(asset_id, paint_rgb=(0.98, 0.78, 0.10)):
    """Sedan plus roof sign, checker livery and a taxi-specific plate."""
    parts = build_sedan(asset_id, paint_rgb, length=4.72, width=1.92,
                        body_height=0.66)
    roof = K.cube(
        f"{asset_id}_roof_sign",
        size=(0.74, 0.30, 0.17), location=(0.0, 0.10, 1.52),
        material=_light(f"{asset_id}_sign_glow", (1.0, 0.90, 0.42), 3.0),
        bevel=0.05, bevel_segments=2,
    )
    K.shade_smooth(roof, angle=math.radians(45.0))
    parts.append(roof)

    # checker band along both flanks: 2 rows x 6 squares
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        for row in range(2):
            for i in range(6):
                sq = K.cube(
                    f"{asset_id}_checker_{tag}_{row}_{i}",
                    size=(0.02, 0.30, 0.16),
                    location=(sx * (1.92 * 0.5 + 0.012), -0.75 + i * 0.30,
                              0.62 + row * 0.17),
                    material=_paint(f"{asset_id}_chk_{row}",
                                    (0.06, 0.06, 0.06) if row == 0 else (0.98, 0.98, 0.98),
                                    metallic=0.0, roughness=0.45),
                )
                parts.append(sq)
    for obj in parts[-24:]:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_police(asset_id):
    """Interceptor: sedan mass plus light bar, push bar and a livery stripe."""
    parts = build_sedan(asset_id, (0.94, 0.95, 0.97), length=4.88,
                        width=1.96, body_height=0.68, cabin_length=2.05)
    parts += _lightbar(asset_id, (0.25, 0.45, 1.0), 1.96, 1.55, 0.25)

    # door decal panel — dark block on both flanks
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        decal = K.cube(
            f"{asset_id}_decal_{tag}",
            size=(0.02, 1.30, 0.34),
            location=(sx * (1.96 * 0.5 + 0.014), 0.10, 0.86),
            material=_paint(f"{asset_id}_decal_mat", (0.06, 0.12, 0.30),
                            metallic=0.0, roughness=0.40),
        )
        parts.append(decal)

    # push bar
    push = K.cube(
        f"{asset_id}_push_bar", size=(1.70, 0.09, 0.42),
        location=(0.0, -4.88 * 0.5 - 0.14, 0.72),
        material=_dark_trim(f"{asset_id}_push"), bevel=0.02,
    )
    parts.append(push)
    for sx in (-1.0, 1.0):
        upright = K.cube(
            f"{asset_id}_push_up_{'l' if sx < 0 else 'r'}",
            size=(0.07, 0.09, 0.40),
            location=(sx * 0.80, -4.88 * 0.5 - 0.14, 0.72),
            material=_dark_trim(f"{asset_id}_push"), bevel=0.02,
        )
        parts.append(upright)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_coupe(asset_id):
    """Two-door fastback: shorter cabin set well back, longer tail deck."""
    parts = build_sedan(asset_id, (0.72, 0.14, 0.18), length=4.52,
                        width=1.92, body_height=0.56, cabin_length=1.62,
                        cabin_height=0.50, wheelbase=2.60)
    # fastback slope: a wedge closing the gap between roof and boot deck
    slope = K.cube(
        f"{asset_id}_fastback",
        size=(1.92 * 0.86, 1.10, 0.34),
        location=(0.0, 4.52 * 0.20, 1.16),
        material=_paint(f"{asset_id}_paint", (0.72, 0.14, 0.18)),
        bevel=0.12, bevel_segments=3,
    )
    K.shade_smooth(slope, angle=math.radians(40.0))
    parts.append(slope)
    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_pickup(asset_id):
    """Cab + bed truck. The bed must be an OPEN box: a solid block reads as a
    van, and the side walls standing proud of the bed floor is the only cue
    that says pickup."""
    K.reset_scene()
    paint = _paint(f"{asset_id}_paint", (0.16, 0.34, 0.52))
    trim = _dark_trim(f"{asset_id}_trim")
    chrome = _chrome(f"{asset_id}_chrome")
    head = _light(f"{asset_id}_headlight", (1.0, 0.96, 0.86), 5.0)
    tail = _light(f"{asset_id}_taillight", (1.0, 0.12, 0.08), 3.0)

    L, W = 5.60, 2.06
    wheel_r, wheelbase, track = 0.40, 3.30, 1.74
    z_frame = wheel_r + 0.34

    parts = []
    chassis = K.cube(f"{asset_id}_chassis", size=(W, L, 0.34),
                     location=(0.0, 0.0, z_frame), material=paint,
                     bevel=0.07, bevel_segments=2)
    K.shade_smooth(chassis, angle=math.radians(38.0))
    parts.append(chassis)

    cab_l = 1.95
    cab_y = -L * 0.5 + cab_l * 0.5 + 0.12
    cab_h = 1.00
    cab_z = z_frame + 0.17 + cab_h * 0.5 - 0.06
    cab = K.cube(f"{asset_id}_cab", size=(W * 0.96, cab_l, cab_h),
                 location=(0.0, cab_y, cab_z), material=paint,
                 bevel=0.12, bevel_segments=3)
    K.shade_smooth(cab, angle=math.radians(40.0))
    parts.append(cab)

    glass = _glass(f"{asset_id}_glass")
    parts.append(K.cube(f"{asset_id}_glass_front",
                        size=(W * 0.90, 0.05, cab_h * 0.66),
                        location=(0.0, cab_y - cab_l * 0.5 + 0.04, cab_z + 0.06),
                        material=glass))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(f"{asset_id}_glass_side_{tag}",
                            size=(0.05, cab_l * 0.70, cab_h * 0.56),
                            location=(sx * (W * 0.96 * 0.5 - 0.02), cab_y + 0.04, cab_z + 0.08),
                            material=glass))

    # --- bed: floor + 4 walls, open at the top -----------------------------
    bed_l = L * 0.5 - cab_l - 0.30
    bed_y = L * 0.5 - bed_l * 0.5 - 0.10
    floor_z = z_frame + 0.19
    bed_floor = K.cube(f"{asset_id}_bed_floor",
                       size=(W * 0.94, bed_l, 0.06), location=(0.0, bed_y, floor_z),
                       material=trim, bevel=0.015)
    parts.append(bed_floor)
    wall_h = 0.52
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        wall = K.cube(f"{asset_id}_bed_wall_{tag}",
                      size=(0.07, bed_l, wall_h),
                      location=(sx * (W * 0.94 * 0.5 - 0.035), bed_y, floor_z + wall_h * 0.5),
                      material=paint, bevel=0.02)
        parts.append(wall)
    tailgate = K.cube(f"{asset_id}_bed_tailgate", size=(W * 0.94, 0.07, wall_h),
                      location=(0.0, bed_y + bed_l * 0.5 - 0.035, floor_z + wall_h * 0.5),
                      material=paint, bevel=0.02)
    parts.append(tailgate)
    # wheel arch bulges read as a pickup from any other angle
    for (ax, tag_axle) in ((-wheelbase * 0.5, "f"), (wheelbase * 0.5, "r")):
        for sx, tag_side in ((-1.0, "l"), (1.0, "r")):
            arch = K.cube(f"{asset_id}_arch_{tag_axle}{tag_side}",
                          size=(0.10, 1.14, 0.34),
                          location=(sx * (W * 0.5 - 0.02), ax, wheel_r + 0.14),
                          material=paint, bevel=0.05)
            parts.append(arch)

    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        hl = K.cube(f"{asset_id}_headlight_{tag}", size=(W * 0.28, 0.10, 0.22),
                    location=(sx * W * 0.32, -L * 0.5 - 0.02, z_frame + 0.04),
                    material=head, bevel=0.02)
        tl = K.cube(f"{asset_id}_taillight_{tag}", size=(W * 0.26, 0.08, 0.20),
                    location=(sx * W * 0.34, L * 0.5 + 0.01, z_frame + 0.06),
                    material=tail, bevel=0.02)
        parts += [hl, tl]

    bumper = K.cube(f"{asset_id}_bumper_front", size=(W * 0.98, 0.11, 0.22),
                    location=(0.0, -L * 0.5 - 0.03, z_frame - 0.14),
                    material=trim, bevel=0.03)
    parts.append(bumper)
    grille = K.cube(f"{asset_id}_grille", size=(W * 0.54, 0.06, 0.26),
                    location=(0.0, -L * 0.5 - 0.02, z_frame - 0.02),
                    material=chrome, bevel=0.015)
    parts.append(grille)

    for (ax, tag_axle) in ((-wheelbase * 0.5, "f"), (wheelbase * 0.5, "r")):
        for (sx, tag_side) in ((-1.0, "l"), (1.0, "r")):
            centre = (sx * track * 0.5, ax, wheel_r)
            tyre, hub = _wheel(f"{tag_axle}{tag_side}", centre, wheel_r, 0.26)
            parts += [tyre, hub]
            for i in range(5):
                spoke = bpy.data.objects.get(f"spoke_{tag_axle}{tag_side}_{i}")
                if spoke is not None:
                    parts.append(spoke)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_bus(asset_id):
    """City transit bus: tall box body, window band, roof hatches, six wheels."""
    K.reset_scene()
    paint = _paint(f"{asset_id}_paint", (0.86, 0.88, 0.90), metallic=0.55, roughness=0.30)
    trim = _dark_trim(f"{asset_id}_trim")
    glass = _glass(f"{asset_id}_glass")
    head = _light(f"{asset_id}_headlight", (1.0, 0.96, 0.86), 5.0)
    tail = _light(f"{asset_id}_taillight", (1.0, 0.12, 0.08), 3.0)

    L, W, H = 10.60, 2.52, 2.86
    wheel_r, wheelbase, track = 0.46, 6.30, 2.02
    z_body = wheel_r + (H - wheel_r * 2.0) * 0.5

    parts = []
    body = K.cube(f"{asset_id}_body", size=(W, L, H), location=(0.0, 0.0, z_body),
                  material=paint, bevel=0.14, bevel_segments=2)
    K.shade_smooth(body, angle=math.radians(38.0))
    parts.append(body)

    # continuous glazing band, inset so it reads as glass not a painted stripe
    glass_z = z_body + H * 0.20
    glass_h = H * 0.34
    parts.append(K.cube(f"{asset_id}_glass_front",
                        size=(W * 0.90, 0.05, glass_h),
                        location=(0.0, -L * 0.5 + 0.03, glass_z), material=glass))
    parts.append(K.cube(f"{asset_id}_glass_rear",
                        size=(W * 0.90, 0.05, glass_h),
                        location=(0.0, L * 0.5 - 0.03, glass_z), material=glass))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(f"{asset_id}_glass_side_{tag}",
                            size=(0.05, L * 0.86, glass_h),
                            location=(sx * (W * 0.5 - 0.02), 0.0, glass_z),
                            material=glass))
    # pillars between windows: without them the band reads as one mirror
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        for i in range(6):
            pil = K.cube(f"{asset_id}_pillar_{tag}_{i}",
                         size=(0.055, 0.09, glass_h),
                         location=(sx * (W * 0.5 + 0.005), -L * 0.36 + i * (L * 0.72 / 5), glass_z),
                         material=paint)
            parts.append(pil)

    # roof hatches
    for i in range(2):
        parts.append(K.cube(f"{asset_id}_hatch_{i}",
                            size=(0.62, 0.62, 0.10),
                            location=(0.0, -2.2 + i * 4.4, z_body + H * 0.5 + 0.04),
                            material=trim, bevel=0.03))
    # AC unit at the rear roof
    parts.append(K.cube(f"{asset_id}_ac_unit", size=(W * 0.72, 1.70, 0.24),
                        location=(0.0, L * 0.30, z_body + H * 0.5 + 0.12),
                        material=trim, bevel=0.05))

    # skirt + bumpers
    parts.append(K.cube(f"{asset_id}_skirt", size=(W * 0.99, L * 0.96, 0.24),
                        location=(0.0, 0.0, z_body - H * 0.5 + 0.14),
                        material=trim, bevel=0.03))
    parts.append(K.cube(f"{asset_id}_bumper_front", size=(W * 0.99, 0.12, 0.26),
                        location=(0.0, -L * 0.5 - 0.04, wheel_r + 0.34),
                        material=trim, bevel=0.03))

    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        hl = K.cube(f"{asset_id}_headlight_{tag}", size=(0.34, 0.10, 0.22),
                    location=(sx * W * 0.34, -L * 0.5 - 0.02, wheel_r + 0.46),
                    material=head, bevel=0.02)
        tl = K.cube(f"{asset_id}_taillight_{tag}", size=(0.20, 0.08, 0.34),
                    location=(sx * W * 0.36, L * 0.5 + 0.01, wheel_r + 0.52),
                    material=tail, bevel=0.02)
        parts += [hl, tl]

    # six wheels: steering pair plus a tandem rear axle
    for (ax, tag_axle) in ((-wheelbase * 0.5, "f"), (wheelbase * 0.32, "r1"),
                           (wheelbase * 0.5, "r2")):
        for (sx, tag_side) in ((-1.0, "l"), (1.0, "r")):
            tyre, hub = _wheel(f"{tag_axle}{tag_side}",
                               (sx * track * 0.5, ax, wheel_r), wheel_r, 0.28)
            parts += [tyre, hub]
            for i in range(5):
                spoke = bpy.data.objects.get(f"spoke_{tag_axle}{tag_side}_{i}")
                if spoke is not None:
                    parts.append(spoke)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts

# ---------------------------------------------------------------------------
# Air
# ---------------------------------------------------------------------------

def _fin(asset_id, tag, loc, size, mat, lean=0.0):
    """A tapered control surface built as a thin wedge.

    Airfoils and stabilisers must taper and lean. A rectangular slab of
    constant thickness and constant section is the giveaway that a "wing" is a
    box, so every surface here leans and is given a thinner trailing edge.
    """
    obj = K.cube(f"{asset_id}_{tag}", size=size, location=loc, material=mat,
                 bevel=min(size) * 0.18)
    if lean:
        obj.rotation_euler = (0.0, math.radians(lean), 0.0)
        K.transform_apply(obj)
    return obj


def build_airplane(asset_id, prop=True):
    """Single-engine light aircraft: tapered wing, tailplane, fin, prop."""
    K.reset_scene()
    body = _paint(f"{asset_id}_body", (0.95, 0.96, 0.98), metallic=0.70, roughness=0.26)
    accent = _paint(f"{asset_id}_accent", (0.10, 0.42, 0.78), metallic=0.60, roughness=0.30)
    glass = _glass(f"{asset_id}_glass")
    dark = _dark_trim(f"{asset_id}_trim")
    metal = _chrome(f"{asset_id}_metal")

    parts = []
    fuse = K.cube(f"{asset_id}_fuselage", size=(1.10, 7.60, 1.24),
                  location=(0.0, 0.0, 1.32), material=body,
                  bevel=0.34, bevel_segments=3)
    K.shade_smooth(fuse, angle=math.radians(50.0))
    parts.append(fuse)
    tailcone = K.cube(f"{asset_id}_tailcone", size=(0.52, 3.10, 0.72),
                      location=(0.0, 2.60, 1.06), material=body,
                      bevel=0.26, bevel_segments=2)
    K.shade_smooth(tailcone, angle=math.radians(50.0))
    parts.append(tailcone)
    nose = K.cube(f"{asset_id}_nose", size=(0.86, 1.30, 0.94),
                  location=(0.0, -4.10, 1.32), material=accent,
                  bevel=0.30, bevel_segments=3)
    K.shade_smooth(nose, angle=math.radians(50.0))
    parts.append(nose)
    parts.append(K.cube(f"{asset_id}_canopy", size=(0.92, 1.90, 0.62),
                        location=(0.0, -2.10, 1.74), material=glass,
                        bevel=0.26, bevel_segments=3))

    parts.append(_fin(asset_id, "wing_l", (-2.90, -0.30, 1.16),
                      (5.20, 1.62, 0.22), body, lean=-4.0))
    parts.append(_fin(asset_id, "wing_r", (2.90, -0.30, 1.16),
                      (5.20, 1.62, 0.22), body, lean=4.0))
    parts.append(_fin(asset_id, "tail_l", (-1.20, 3.90, 1.24),
                      (2.20, 1.00, 0.16), accent, lean=-3.0))
    parts.append(_fin(asset_id, "tail_r", (1.20, 3.90, 1.24),
                      (2.20, 1.00, 0.16), accent, lean=3.0))
    parts.append(_fin(asset_id, "fin", (0.0, 4.00, 1.86), (0.14, 1.30, 1.30), accent))
    parts.append(_fin(asset_id, "fin_pad", (0.0, 4.06, 0.92), (0.10, 0.72, 0.50), metal))

    cowl = K.cylinder(f"{asset_id}_cowl", radius=0.52, depth=0.70, vertices=20,
                      location=(0.0, -4.60, 1.32), material=accent, smooth=True)
    cowl.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(cowl)
    parts.append(cowl)
    hub = K.cylinder(f"{asset_id}_prop_hub", radius=0.16, depth=0.30, vertices=12,
                     location=(0.0, -5.05, 1.32), material=dark, smooth=True)
    hub.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(hub)
    parts.append(hub)
    if prop:
        for i in range(3):
            # build AT the hub, spin about the hub, THEN move to the engine.
            # Setting rotation_euler on a part whose transform is already
            # baked turns it about the world origin instead, which put the
            # blades 5 m under the aircraft.
            blade = K.cube(f"{asset_id}_prop_blade{i}", size=(0.16, 0.10, 1.90),
                           location=(0.0, 0.0, 0.0), material=dark, bevel=0.03)
            K.rotate_about(blade, (math.radians(90.0), 0.0, math.radians(i * 120.0)))
            K.place(blade, (0.0, -5.10, 1.32))
            parts.append(blade)
    else:
        ring = K.cylinder(f"{asset_id}_intake_ring", radius=0.46, depth=0.16,
                          vertices=20, location=(0.0, -5.02, 1.32), material=metal, smooth=True)
        ring.rotation_euler = (math.radians(90.0), 0.0, 0.0)
        K.transform_apply(ring)
        parts.append(ring)

    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        strut = K.cube(f"{asset_id}_gear_{tag}", size=(0.10, 0.10, 0.86),
                       location=(0.0, 0.0, 0.0), material=metal, bevel=0.02)
        K.rotate_about(strut, (math.radians(16.0), 0.0, 0.0))
        K.place(strut, (sx * 1.70, -1.40, 0.48))
        parts.append(strut)
        wheel = K.cylinder(f"{asset_id}_gear_wheel_{tag}", radius=0.26, depth=0.16,
                           vertices=16, location=(sx * 1.86, -1.24, 0.26),
                           material=dark, smooth=True)
        wheel.rotation_euler = (0.0, math.radians(90.0), 0.0)
        K.transform_apply(wheel)
        parts.append(wheel)
    nose_leg = K.cube(f"{asset_id}_gear_nose", size=(0.09, 0.09, 0.62),
                      location=(0.0, 0.0, 0.0), material=metal, bevel=0.02)
    K.rotate_about(nose_leg, (math.radians(12.0), 0.0, 0.0))
    K.place(nose_leg, (0.0, -3.30, 0.36))
    parts.append(nose_leg)
    nose_wheel = K.cylinder(f"{asset_id}_gear_nose_wheel", radius=0.20, depth=0.13,
                            vertices=14, location=(0.0, -3.20, 0.20), material=dark, smooth=True)
    nose_wheel.rotation_euler = (0.0, math.radians(90.0), 0.0)
    K.transform_apply(nose_wheel)
    parts.append(nose_wheel)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_jet(asset_id):
    """Twin-engine jet: swept wings, twin tails, underwing nacelles."""
    parts = build_airplane(asset_id, prop=False)
    body = _paint(f"{asset_id}_body", (0.30, 0.32, 0.36), metallic=0.80, roughness=0.22)
    metal = _chrome(f"{asset_id}_metal")
    glass = _glass(f"{asset_id}_glass")

    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        nac = K.cylinder(f"{asset_id}_nac_{tag}", radius=0.46, depth=2.30, vertices=20,
                         location=(sx * 2.30, -1.30, 0.94), material=body, smooth=True)
        nac.rotation_euler = (math.radians(90.0), 0.0, 0.0)
        K.transform_apply(nac)
        parts.append(nac)
        parts.append(K.cube(f"{asset_id}_pylon_{tag}", size=(0.16, 1.60, 0.44),
                            location=(sx * 2.30, -0.60, 1.06), material=body, bevel=0.05))
        intake = K.cylinder(f"{asset_id}_intake_{tag}", radius=0.42, depth=0.14,
                            vertices=20, location=(sx * 2.30, -2.46, 0.94),
                            material=metal, smooth=True)
        intake.rotation_euler = (math.radians(90.0), 0.0, 0.0)
        K.transform_apply(intake)
        parts.append(intake)
        # twin vertical tails - one fin does not read as a twin-tail jet
        parts.append(_fin(asset_id, f"vtail_{tag}", (sx * 1.05, 4.10, 2.10),
                          (0.13, 1.60, 1.90), body, lean=sx * -6.0))

    parts.append(K.cube(f"{asset_id}_canopy_jet", size=(0.98, 2.60, 0.72),
                        location=(0.0, -2.40, 1.86), material=glass,
                        bevel=0.30, bevel_segments=3))
    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_helicopter(asset_id):
    """Utility helicopter: fuselage pod, tail boom, main + tail rotor, skids."""
    K.reset_scene()
    body = _paint(f"{asset_id}_body", (0.14, 0.16, 0.20), metallic=0.55, roughness=0.34)
    accent = _paint(f"{asset_id}_accent", (0.90, 0.62, 0.10), metallic=0.50, roughness=0.32)
    glass = _glass(f"{asset_id}_glass")
    dark = _dark_trim(f"{asset_id}_trim")
    metal = _chrome(f"{asset_id}_metal")

    parts = []
    pod = K.cube(f"{asset_id}_pod", size=(1.66, 3.30, 1.72), location=(0.0, -0.60, 1.58),
                 material=body, bevel=0.46, bevel_segments=3)
    K.shade_smooth(pod, angle=math.radians(52.0))
    parts.append(pod)
    boom = K.cylinder(f"{asset_id}_boom", radius=0.26, depth=3.40, vertices=14,
                      location=(0.0, 2.90, 1.62), material=body, smooth=True)
    boom.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(boom)
    parts.append(boom)
    parts.append(K.cube(f"{asset_id}_canopy", size=(1.44, 1.70, 1.12),
                        location=(0.0, -1.72, 1.72), material=glass, bevel=0.44, bevel_segments=3))
    parts.append(K.cube(f"{asset_id}_chin", size=(1.10, 0.90, 0.52),
                        location=(0.0, -2.40, 1.14), material=glass, bevel=0.22, bevel_segments=2))
    parts.append(K.cube(f"{asset_id}_deck", size=(1.24, 2.10, 0.46),
                        location=(0.0, 0.30, 2.54), material=accent, bevel=0.14, bevel_segments=2))
    parts.append(K.cube(f"{asset_id}_exhaust", size=(0.44, 0.30, 0.26),
                        location=(0.0, 1.44, 2.58), material=dark, bevel=0.06))

    parts.append(K.cylinder(f"{asset_id}_mast", radius=0.14, depth=0.60, vertices=12,
                            location=(0.0, 0.30, 3.06), material=metal, smooth=True))
    parts.append(K.cube(f"{asset_id}_hub", size=(0.44, 0.44, 0.22),
                        location=(0.0, 0.30, 3.40), material=dark, bevel=0.06))
    for i in range(4):
        blade = K.cube(f"{asset_id}_mblade{i}", size=(7.20, 0.30, 0.05),
                       location=(0.0, 0.30, 3.50), material=dark, bevel=0.015)
        blade.rotation_euler = (0.0, 0.0, math.radians(i * 90.0 + 8.0))
        K.transform_apply(blade)
        parts.append(blade)

    parts.append(_fin(asset_id, "fin", (0.0, 4.16, 2.30), (0.12, 1.00, 1.50), accent))
    for i in range(3):
        tb = K.cube(f"{asset_id}_tblade{i}", size=(0.06, 1.30, 0.16),
                    location=(0.0, 0.0, 0.0), material=dark, bevel=0.012)
        K.rotate_about(tb, (math.radians(i * 120.0), 0.0, 0.0))
        K.place(tb, (0.16, 4.16, 2.32))
        parts.append(tb)
    thub = K.cylinder(f"{asset_id}_thub", radius=0.09, depth=0.22, vertices=10,
                      location=(0.16, 4.16, 2.32), material=metal, smooth=True)
    thub.rotation_euler = (0.0, math.radians(90.0), 0.0)
    K.transform_apply(thub)
    parts.append(thub)
    parts.append(_fin(asset_id, "hstab", (0.0, 3.90, 1.40), (2.60, 0.72, 0.12), body))

    # A skid is a CYLINDER, so its lowest point is centre_z - radius, not
    # centre_z. Sitting the rail at 0.16 left the whole aircraft 0.09 m in the
    # air — a hovering helicopter with no skids touching the ground. The rails
    # rest on z=0, and the struts are re-hung to meet them.
    skid_r = 0.07
    skid_z = skid_r
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        rail = K.cylinder(f"{asset_id}_skid_{tag}", radius=skid_r, depth=3.40,
                          vertices=12, location=(sx * 0.98, -0.70, skid_z),
                          material=metal, smooth=True)
        rail.rotation_euler = (math.radians(90.0), 0.0, 0.0)
        K.transform_apply(rail)
        parts.append(rail)
        for y in (-1.60, 0.20):
            strut = K.cube(f"{asset_id}_strut_{tag}_{'f' if y < 0 else 'r'}",
                           size=(0.10, 0.10, 0.86), location=(0.0, 0.0, 0.0),
                           material=metal, bevel=0.02)
            K.rotate_about(strut, (0.0, math.radians(sx * -12.0), 0.0))
            K.place(strut, (sx * 0.98, y, skid_z + 0.44))
            parts.append(strut)
    for y in (-1.10, 0.70):
        cross = K.cylinder(f"{asset_id}_cross_{'f' if y < 0 else 'r'}", radius=0.055,
                           depth=1.96, vertices=10,
                           location=(0.0, y, skid_z + 0.055),
                           material=metal, smooth=True)
        cross.rotation_euler = (0.0, math.radians(90.0), 0.0)
        K.transform_apply(cross)
        parts.append(cross)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


# ---------------------------------------------------------------------------
# Sea
# ---------------------------------------------------------------------------

def build_boat(asset_id):
    """Open motorboat: V hull, console, windshield, outboard, seats."""
    K.reset_scene()
    hull = _paint(f"{asset_id}_hull", (0.94, 0.96, 0.98), metallic=0.40, roughness=0.22)
    deck = _paint(f"{asset_id}_deck", (0.20, 0.36, 0.50), metallic=0.30, roughness=0.50)
    accent = _paint(f"{asset_id}_accent", (0.90, 0.24, 0.20), metallic=0.45, roughness=0.28)
    glass = _glass(f"{asset_id}_glass")
    dark = _dark_trim(f"{asset_id}_trim")

    parts = []
    # three stacked slabs of decreasing width give a V section; one box reads
    # as a barge
    keel = K.cube(f"{asset_id}_keel", size=(0.90, 4.60, 0.44), location=(0.0, 0.0, 0.34),
                  material=hull, bevel=0.18)
    chine = K.cube(f"{asset_id}_chine", size=(1.90, 4.40, 0.42), location=(0.0, 0.0, 0.60),
                   material=hull, bevel=0.20)
    gunwale = K.cube(f"{asset_id}_gunwale", size=(2.34, 4.20, 0.36), location=(0.0, 0.0, 0.88),
                     material=deck, bevel=0.16)
    for o in (keel, chine, gunwale):
        K.shade_smooth(o, angle=math.radians(50.0))
        parts.append(o)
    bow = K.cube(f"{asset_id}_bow", size=(1.10, 1.50, 0.56), location=(0.0, -2.60, 1.00),
                 material=accent, bevel=0.24)
    K.shade_smooth(bow, angle=math.radians(50.0))
    parts.append(bow)

    parts.append(K.cube(f"{asset_id}_console", size=(1.10, 0.70, 0.90),
                        location=(0.0, -1.10, 1.40), material=deck, bevel=0.10))
    ws = K.cube(f"{asset_id}_windscreen", size=(1.16, 0.06, 0.52), location=(0.0, -1.42, 1.90),
                material=glass, bevel=0.03, bevel_segments=2)
    ws.rotation_euler = (math.radians(-16.0), 0.0, 0.0)
    K.transform_apply(ws)
    parts.append(ws)

    for i, y in enumerate((-0.10, 1.10)):
        parts.append(K.cube(f"{asset_id}_seat{i}", size=(1.50, 0.62, 0.42),
                            location=(0.0, y, 1.24), material=accent, bevel=0.10))
    parts.append(K.cube(f"{asset_id}_bench", size=(1.90, 0.60, 0.50),
                        location=(0.0, 1.92, 1.26), material=accent, bevel=0.12))
    parts.append(K.cube(f"{asset_id}_outboard", size=(0.54, 0.66, 0.86),
                        location=(0.0, 2.52, 0.96), material=dark, bevel=0.12))
    parts.append(K.cube(f"{asset_id}_outboard_leg", size=(0.22, 0.34, 0.90),
                        location=(0.0, 2.52, 0.24), material=dark, bevel=0.06))

    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        rail = K.cylinder(f"{asset_id}_rail_{tag}", radius=0.032, depth=1.60, vertices=8,
                          location=(sx * 0.52, -2.30, 1.42),
                          material=_chrome(f"{asset_id}_rail"), smooth=True)
        rail.rotation_euler = (0.0, math.radians(90.0), 0.0)
        K.transform_apply(rail)
        parts.append(rail)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_yacht(asset_id):
    """Sailing yacht: hull, deckhouse, mast + boom + mainsail + jib.

    SAILS ARE OPEN SURFACES. Each declares its outward direction so the
    exporter can check winding instead of silently shipping a sail that is
    inside-out from one side.
    """
    K.reset_scene()
    hull = _paint(f"{asset_id}_hull", (0.96, 0.97, 0.98), metallic=0.25, roughness=0.18)
    deck = _paint(f"{asset_id}_deck", (0.82, 0.78, 0.70), metallic=0.20, roughness=0.55)
    accent = _paint(f"{asset_id}_accent", (0.10, 0.40, 0.72), metallic=0.40, roughness=0.28)
    sail_mat = _paint(f"{asset_id}_sail", (0.98, 0.98, 0.96), metallic=0.0, roughness=0.80)
    metal = _chrome(f"{asset_id}_metal")
    glass = _glass(f"{asset_id}_glass")

    parts = []
    keel = K.cube(f"{asset_id}_keel", size=(1.30, 10.60, 0.72), location=(0.0, 0.0, 0.46),
                  material=hull, bevel=0.30)
    topsides = K.cube(f"{asset_id}_topsides", size=(3.20, 10.20, 0.80),
                      location=(0.0, 0.0, 1.10), material=hull, bevel=0.34)
    for o in (keel, topsides):
        K.shade_smooth(o, angle=math.radians(50.0))
        parts.append(o)
    bow = K.cube(f"{asset_id}_bow", size=(1.80, 3.00, 1.10), location=(0.0, -6.00, 1.30),
                 material=hull, bevel=0.44)
    K.shade_smooth(bow, angle=math.radians(50.0))
    parts.append(bow)
    parts.append(K.cube(f"{asset_id}_transom", size=(2.90, 0.60, 0.86),
                        location=(0.0, 5.10, 1.06), material=hull, bevel=0.20))
    parts.append(K.cube(f"{asset_id}_deckplate", size=(3.10, 10.10, 0.18),
                        location=(0.0, 0.0, 1.58), material=deck, bevel=0.08))

    parts.append(K.cube(f"{asset_id}_deckhouse", size=(2.30, 4.20, 0.86),
                        location=(0.0, 0.60, 2.06), material=accent, bevel=0.20))
    parts.append(K.cube(f"{asset_id}_cockpit_glass", size=(2.10, 0.06, 0.40),
                        location=(0.0, -1.52, 2.06), material=glass))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(K.cube(f"{asset_id}_port_{tag}", size=(0.06, 2.60, 0.34),
                            location=(sx * 1.16, 0.60, 2.02), material=glass))

    parts.append(K.cylinder(f"{asset_id}_mast", radius=0.10, depth=12.00, vertices=12,
                            location=(0.0, -1.10, 7.60), material=metal, smooth=True))
    boom = K.cylinder(f"{asset_id}_boom", radius=0.07, depth=4.60, vertices=10,
                      location=(0.0, 1.20, 2.30), material=metal, smooth=True)
    boom.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(boom)
    parts.append(boom)

    main = K.cube(f"{asset_id}_sail_main", size=(0.06, 3.60, 5.60), location=(0.0, 0.80, 5.20),
                  material=sail_mat)
    main["vcw_open_surface"] = "+x"
    main["vcw_open_ref"] = (0.0, 0.80, 5.20)
    parts.append(main)
    jib = K.cube(f"{asset_id}_sail_jib", size=(0.06, 2.80, 4.60), location=(0.0, -3.40, 5.00),
                 material=sail_mat)
    jib["vcw_open_surface"] = "+x"
    jib["vcw_open_ref"] = (0.0, -3.40, 5.00)
    parts.append(jib)

    stay = K.cube(f"{asset_id}_forestay", size=(0.05, 0.05, 9.00),
                  location=(0.0, 0.0, 0.0), material=metal, bevel=0.01)
    K.rotate_about(stay, (math.radians(22.0), 0.0, 0.0))
    K.place(stay, (0.0, -6.00, 4.60))
    parts.append(stay)
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        shroud = K.cube(f"{asset_id}_shroud_{tag}", size=(0.05, 0.05, 8.40),
                        location=(0.0, 0.0, 0.0), material=metal, bevel=0.01)
        K.rotate_about(shroud, (0.0, math.radians(sx * 7.0), 0.0))
        K.place(shroud, (sx * 1.30, -1.00, 5.20))
        parts.append(shroud)
        rail = K.cylinder(f"{asset_id}_rail_{tag}", radius=0.028, depth=9.00, vertices=8,
                          location=(sx * 1.56, 0.0, 2.10), material=metal, smooth=True)
        rail.rotation_euler = (math.radians(90.0), 0.0, 0.0)
        K.transform_apply(rail)
        parts.append(rail)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_cruise_ship(asset_id):
    """Large cruise ship: stepped superstructure tiers, funnel, lifeboats."""
    K.reset_scene()
    hull = _paint(f"{asset_id}_hull", (0.10, 0.14, 0.28), metallic=0.45, roughness=0.26)
    superwhite = _paint(f"{asset_id}_super", (0.96, 0.96, 0.94), metallic=0.25, roughness=0.34)
    accent = _paint(f"{asset_id}_accent", (0.92, 0.30, 0.22), metallic=0.35, roughness=0.30)
    glass = _glass(f"{asset_id}_glass")
    dark = _dark_trim(f"{asset_id}_trim")

    L, B = 46.0, 7.40
    parts = []
    keel = K.cube(f"{asset_id}_keel", size=(B * 0.62, L, 1.60), location=(0.0, 0.0, 0.90),
                  material=hull, bevel=0.60)
    sides = K.cube(f"{asset_id}_sides", size=(B, L * 0.97, 3.10), location=(0.0, 0.0, 2.90),
                   material=hull, bevel=0.80)
    bow = K.cube(f"{asset_id}_bow", size=(B * 0.42, 8.00, 3.40),
                 location=(0.0, -L * 0.5 + 2.0, 3.20), material=hull, bevel=1.10)
    transom = K.cube(f"{asset_id}_transom", size=(B * 0.92, 3.20, 3.20),
                     location=(0.0, L * 0.5 - 1.2, 3.00), material=hull, bevel=0.60)
    for o in (keel, sides, bow, transom):
        K.shade_smooth(o, angle=math.radians(50.0))
        parts.append(o)
    parts.append(K.cube(f"{asset_id}_stripe", size=(B * 1.004, L * 0.96, 0.34),
                        location=(0.0, 0.0, 1.86), material=accent))

    # four stepped tiers - one box reads as a barge
    tiers = ((4.20, B * 0.86, 30.0, -2.0), (3.40, B * 0.76, 26.0, 1.0),
             (2.60, B * 0.66, 21.0, 3.0), (1.90, B * 0.56, 15.0, 5.0))
    z = 4.50
    for i, (h, w, l, y) in enumerate(tiers):
        tier = K.cube(f"{asset_id}_tier{i}", size=(w, l, h), location=(0.0, y, z + h * 0.5),
                      material=superwhite, bevel=0.20, bevel_segments=2)
        K.shade_smooth(tier, angle=math.radians(48.0))
        parts.append(tier)
        for side in (-1.0, 1.0):
            parts.append(K.cube(f"{asset_id}_balcony{i}_{'l' if side < 0 else 'r'}",
                                size=(0.30, l * 0.94, 0.16),
                                location=(side * (w * 0.5 + 0.10), y, z + h * 0.72),
                                material=glass))
        z += h

    parts.append(K.cube(f"{asset_id}_bridge", size=(B * 0.70, 4.20, 1.30),
                        location=(0.0, -9.60, z + 0.65), material=superwhite, bevel=0.16))
    parts.append(K.cube(f"{asset_id}_bridge_glass", size=(B * 0.68, 0.08, 0.60),
                        location=(0.0, -11.70, z + 0.70), material=glass))
    z += 1.30

    parts.append(K.cube(f"{asset_id}_funnel", size=(2.60, 3.60, 3.40),
                        location=(0.0, 7.50, z + 1.70), material=accent, bevel=0.40, bevel_segments=2))
    parts.append(K.cube(f"{asset_id}_funnel_cap", size=(2.70, 3.70, 0.40),
                        location=(0.0, 7.50, z + 3.50), material=dark, bevel=0.12))
    for y, tag in ((-1.0, "f"), (6.0, "m")):
        parts.append(K.cylinder(f"{asset_id}_mast_{tag}", radius=0.14, depth=5.00, vertices=10,
                                location=(0.0, y, z + 6.00),
                                material=_chrome(f"{asset_id}_metal"), smooth=True))

    for i, (h, w, l, y) in enumerate(tiers[:3]):
        for side in (-1.0, 1.0):
            for k in range(4):
                lb = K.cube(f"{asset_id}_lifeboat{i}_{'l' if side < 0 else 'r'}_{k}",
                            size=(1.20, 2.40, 1.10),
                            location=(side * (w * 0.5 + 0.95), y - l * 0.30 + k * 2.60,
                                      5.20 + i * 3.4),
                            material=accent, bevel=0.44, bevel_segments=2)
                K.shade_smooth(lb, angle=math.radians(52.0))
                parts.append(lb)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_destroyer(asset_id):
    """Grey warship: angular hull, stepped superstructure, mast, gun turret."""
    K.reset_scene()
    hull = _paint(f"{asset_id}_hull", (0.34, 0.36, 0.38), metallic=0.60, roughness=0.42)
    deck = _paint(f"{asset_id}_deck", (0.28, 0.30, 0.32), metallic=0.40, roughness=0.58)
    dark = _dark_trim(f"{asset_id}_trim")
    metal = _chrome(f"{asset_id}_metal")
    glass = _glass(f"{asset_id}_glass")

    L, B = 38.0, 5.60
    parts = []
    hullbox = K.cube(f"{asset_id}_hull", size=(B, L, 2.40), location=(0.0, 0.0, 1.30),
                     material=hull, bevel=0.34)
    K.shade_smooth(hullbox, angle=math.radians(48.0))
    parts.append(hullbox)
    # The rake tilts the bow DOWN at the stem, so rotating a part about its own
    # centre pushes its forward end below the waterline. Rotate about the
    # AFT edge instead, which is the line the wedge actually pivots on.
    bow = K.cube(f"{asset_id}_bow", size=(B * 0.58, 9.00, 2.30),
                 location=(0.0, 0.0, 0.0), material=hull, bevel=0.40)
    K.rotate_about(bow, (math.radians(-7.0), 0.0, 0.0),
                   pivot=(0.0, 9.00 * 0.5, 0.0))
    K.place(bow, (0.0, -L * 0.5 + 2.6, 1.26))
    parts.append(bow)
    parts.append(K.cube(f"{asset_id}_deckplate", size=(B * 1.02, L * 0.96, 0.16),
                        location=(0.0, 0.0, 2.54), material=deck, bevel=0.06))

    parts.append(K.cube(f"{asset_id}_sup1", size=(B * 0.80, 12.0, 1.90),
                        location=(0.0, -3.0, 3.55), material=deck, bevel=0.16))
    parts.append(K.cube(f"{asset_id}_sup2", size=(B * 0.60, 7.4, 1.70),
                        location=(0.0, -2.0, 5.32), material=deck, bevel=0.14))
    parts.append(K.cube(f"{asset_id}_bridge_glass", size=(B * 0.61, 0.08, 0.52),
                        location=(0.0, -5.72, 5.50), material=glass))

    parts.append(K.cube(f"{asset_id}_mast", size=(0.20, 0.20, 6.20),
                        location=(0.0, 0.60, 8.80), material=metal, bevel=0.03))
    for i, (w, d) in enumerate(((3.00, 0.14), (2.00, 0.12), (1.30, 0.10))):
        parts.append(K.cube(f"{asset_id}_yard{i}", size=(w, d, 0.12),
                            location=(0.0, 0.60, 8.00 + i * 1.30), material=metal, bevel=0.02))
    parts.append(K.cube(f"{asset_id}_radar", size=(0.30, 2.40, 0.24),
                        location=(0.0, 0.60, 11.20), material=dark, bevel=0.05))

    turret = K.cube(f"{asset_id}_turret", size=(2.20, 3.00, 1.00),
                    location=(0.0, -13.0, 3.10), material=hull, bevel=0.22)
    K.shade_smooth(turret, angle=math.radians(50.0))
    parts.append(turret)
    barrel = K.cylinder(f"{asset_id}_barrel", radius=0.11, depth=4.60, vertices=10,
                        location=(0.0, -16.20, 3.30), material=metal, smooth=True)
    barrel.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(barrel)
    parts.append(barrel)

    for gx in range(3):
        for gy in range(2):
            parts.append(K.cube(f"{asset_id}_vls_{gx}_{gy}", size=(1.50, 1.50, 0.20),
                                location=(-1.9 + gx * 1.9, 12.0 + gy * 1.9, 2.70),
                                material=dark, bevel=0.04))

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


VEHICLES: list[tuple[str, Callable, dict]] = [
    ("car_sedan", build_sedan, dict(paint_rgb=(0.72, 0.16, 0.20))),
    ("car_taxi", build_taxi, dict()),
    ("car_police", build_police, dict()),
    ("car_coupe", build_coupe, dict()),
    ("truck_pickup", build_pickup, dict()),
    ("bus_city", build_bus, dict()),
    ("veh_airplane", build_airplane, dict()),
    ("veh_jet", build_jet, dict()),
    ("veh_helicopter", build_helicopter, dict()),
    ("veh_boat", build_boat, dict()),
    ("yacht_sailboat", build_yacht, dict()),
    ("ship_cruise", build_cruise_ship, dict()),
    ("ship_destroyer", build_destroyer, dict()),
]
