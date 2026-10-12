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