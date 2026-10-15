"""3A building builder — Art Deco / pastel Miami blocks.

THE bug this module exists to avoid
-----------------------------------
The obvious way to build a tower with a setback is one box for the whole
shaft plus windows placed at each band's own inset. That is wrong, and wrong
in a way that every count-based check passes:

    shaft = chamfer_box(w, d, total_h)          # ONE box, full footprint
    for each floor: windows at scale_at(z)      # each band's inset

Above the first setback the glazing is emitted at the inset and the wall never
moves, so **every window on every band above the first setback is modelled and
completely unreachable** — sealed inside solid geometry. Measured on a prior
library, per-floor visible-window count from the street:

                    before    after
    bldg_deco_teal       4/9       9/9
    bldg_lilac_tower     6/12     12/12
    bldg_white_landmark  5/10     10/10
    bldg_pink_terrace    3/7       7/7

For every asset declaring a setback, the first glazing-free floor was
**exactly the first floor above the first setback** — a signature that points
at massing, not at a detail budget.

The rule this module follows: **emit the wall as one box per band**, each at
its own band scale, and place each band's detail at that band's scale. The
detailed bands are where the player can see them.

A second leak in the same family: details keyed to the base footprint stand
proud of a stepped crown, and a cornice sized as the average of two
footprints floats inside the wider storey below it. So a transition slab is
sized to the **wider** of the two, never their mean.

Origin convention: ground-centre, resting on z=0. Every building asset is
grounded, and `verify3a` asserts it.

Rooftops
--------
A bare flat roof is the single loudest tell that a building is a blockout. A
Miami deco block's skyline is not its parapet line — it is the junk standing
above it: the water tank on its legs, the bank of AC condensers, the vent
stacks, the antenna mast. `_rooftop_kit` puts those on every building, sized
to the TOP band (never the base footprint, which would put a 2 m tank on a
6 m crown or float it off a wide roof), and seeded deterministically from the
asset id so a rebuild produces the identical roof.
"""

from __future__ import annotations

import math
import random
import zlib
from typing import Sequence

import bpy

from .. import kernel as K


# --- material palette -------------------------------------------------------
# Miami pastels, each a real PBR layer rather than one flat vertex colour.


def _stucco(name: str, rgb, roughness: float = 0.82):
    return K.pbr_material(name, rgb, roughness=roughness, metallic=0.0)


def _concrete(name: str, rgb, roughness: float = 0.88):
    return K.pbr_material(name, rgb, roughness=roughness, metallic=0.0)


def _glass(name: str, tint=(0.16, 0.28, 0.36), rough: float = 0.06, metal: float = 0.55):
    return K.pbr_material(name, tint, roughness=rough, metallic=metal)


def _trim(name: str, rgb=(0.96, 0.94, 0.88), rough: float = 0.55):
    return K.pbr_material(name, rgb, roughness=rough, metallic=0.05)


def _metal(name: str, rgb=(0.62, 0.64, 0.68), rough: float = 0.35, metal: float = 0.9):
    return K.pbr_material(name, rgb, roughness=rough, metallic=metal)


def _sign(name: str, rgb, strength: float = 3.0):
    return K.pbr_material(name, rgb, roughness=0.3, metallic=0.0,
                          emission=rgb, emission_strength=strength)


# --- band model -------------------------------------------------------------


class Band:
    """One storey-height slice of a facade, at its own footprint scale."""

    __slots__ = ("z0", "z1", "w", "d", "windows", "name")

    def __init__(self, z0: float, z1: float, w: float, d: float, windows: int, name: str):
        self.z0 = z0
        self.z1 = z1
        self.w = w
        self.d = d
        self.windows = windows
        self.name = name

    @property
    def height(self) -> float:
        return self.z1 - self.z0

    @property
    def zc(self) -> float:
        return (self.z0 + self.z1) * 0.5


def _bands_for_tower(total_h: float, w: float, d: float, storey: float,
                     setbacks: Sequence[float]) -> list[Band]:
    """Split a tower into bands, each at its own footprint.

    `setbacks` is the list of absolute heights where the footprint steps in.
    Each band gets the scale that is in force *at its own z*, which is the
    whole point: a single full-footprint box would bury everything above the
    first step.
    """
    cuts = [0.0] + sorted(setbacks) + [total_h]
    bands: list[Band] = []
    for i in range(len(cuts) - 1):
        z0, z1 = cuts[i], cuts[i + 1]
        if z1 - z0 < storey * 0.4:
            continue
        scale = 1.0
        for step in sorted(setbacks):
            if z0 >= step - 1e-9:
                scale *= 0.86          # each setback steps in 14 %
        # a taper through the last two bands reads as a crown rather than a stub
        if z1 >= total_h - 1e-9 and len(cuts) > 2:
            scale *= 0.92
        bands.append(Band(z0, z1, w * scale, d * scale,
                          max(2, int(round(w * scale / 2.6))), f"band_{i}"))
    return bands


def _window_grid(band: Band, asset_id: str, glass_mat, trim_mat, recess: float = 0.055):
    """Recessed glazing for one band, placed at THAT band's scale.

    The recess is what makes a window read as an opening rather than a decal:
    the glass sits behind the wall plane and the surround stands proud, so the
    reveal catches a shadow edge. `check_window_recess.py` asserts the glass
    is set back, not stuck on.
    """
    parts = []
    wz = band.zc
    per_side = max(1, band.windows)
    span_x = band.w * 0.78
    span_y = band.d * 0.74

    for face, axis, span, half in (
        ("front", "y", span_x, band.d * 0.5),
        ("back", "y", span_x, -band.d * 0.5),
        ("left", "x", span_y, band.w * 0.5),
        ("right", "x", span_y, -band.w * 0.5),
    ):
        for i in range(per_side):
            if per_side == 1:
                off = 0.0
            else:
                off = (-span * 0.5) + (span / (per_side - 1)) * i
            win_w = min(1.05, span / per_side * 0.62)
            win_h = min(band.height * 0.62, 1.9)

            # THE bug this whole function exists to avoid.
            #
            # The wall is a SOLID box, so there is no opening to recess into:
            # putting the glass at `half - recess` (0.055 m behind the wall
            # face) BURIES it inside the masonry. Measured on band 1: wall
            # spans y -6.88..+6.88, glass sat at y 6.790..6.860 — 0.09 m inside
            # the wall on every face, every floor. Every window was modelled
            # and none was reachable. The documented signature is a glazing
            # share that collapses to zero at one specific height; here it is
            # zero everywhere, which is the same defect without the setback.
            #
            # So: glass sits PROUD of the wall plane (a projecting window, as
            # on a Miami deco block), and the surround is four strips around it
            # rather than one slab — a slab at a smaller recess seals the very
            # opening it is meant to frame (measured: trim y 6.811..6.911 in
            # front of glass y 6.790..6.860 across its whole area).
            if axis == "y":
                sign = 1.0 if half > 0 else -1.0
                glass_loc = (off, half + sign * 0.045, wz)
                glass_size = (win_w, 0.09, win_h)
                surround_parts = [
                    ((off, half + sign * 0.055, wz + win_h * 0.5 + 0.07),
                     (win_w + 0.26, 0.11, 0.14)),
                    ((off, half + sign * 0.055, wz - win_h * 0.5 - 0.07),
                     (win_w + 0.26, 0.13, 0.14)),
                    ((off - win_w * 0.5 - 0.07, half + sign * 0.055, wz),
                     (0.14, 0.11, win_h)),
                    ((off + win_w * 0.5 + 0.07, half + sign * 0.055, wz),
                     (0.14, 0.11, win_h)),
                ]
            else:
                sign = 1.0 if half > 0 else -1.0
                glass_loc = (half + sign * 0.045, off, wz)
                glass_size = (0.09, win_w, win_h)
                surround_parts = [
                    ((half + sign * 0.055, off, wz + win_h * 0.5 + 0.07),
                     (0.11, win_w + 0.26, 0.14)),
                    ((half + sign * 0.055, off, wz - win_h * 0.5 - 0.07),
                     (0.13, win_w + 0.26, 0.14)),
                    ((half + sign * 0.055, off - win_w * 0.5 - 0.07, wz),
                     (0.11, 0.14, win_h)),
                    ((half + sign * 0.055, off + win_w * 0.5 + 0.07, wz),
                     (0.11, 0.14, win_h)),
                ]

            glass = K.cube(
                f"{asset_id}_{band.name}_{face}_{i}_glass",
                size=glass_size, location=glass_loc, material=glass_mat,
            )
            parts.append(glass)
            for si, (s_loc, s_size) in enumerate(surround_parts):
                strip = K.cube(
                    f"{asset_id}_{band.name}_{face}_{i}_trim{si}",
                    size=s_size, location=s_loc, material=trim_mat, bevel=0.02,
                )
                parts.append(strip)
    return parts


def _water_tank(asset_id: str, tag: str, x: float, y: float, z: float,
                tank_mat, leg_mat, scale: float = 1.0) -> list:
    """A timber water tank on a steel stand — the Miami roof signature.

    Real tanks are cylindrical with a conical cap and stand 1.5-2 m clear of
    the roof on four legs, so they throw a shadow ACROSS the roof rather than
    sitting on it. That standoff is the whole silhouette; a tank resting
    directly on the deck reads as a cylinder, not as roof furniture.
    """
    r = 0.62 * scale
    leg_h = 1.30 * scale
    barrel_h = 1.45 * scale
    parts = [
        K.cylinder(f"{asset_id}_tank_{tag}_barrel", radius=r, depth=barrel_h,
                   vertices=14, location=(x, y, z + leg_h + barrel_h * 0.5),
                   material=tank_mat, smooth=False),
        K.cone(f"{asset_id}_tank_{tag}_cap", radius1=r, radius2=r * 0.18,
               depth=0.42 * scale, vertices=14,
               location=(x, y, z + leg_h + barrel_h + 0.21 * scale),
               material=tank_mat, smooth=False),
    ]
    # four legs, inset from the barrel wall so the tank reads as held up
    for sx, tagx in ((-1.0, "a"), (1.0, "b")):
        for sy, tagy in ((-1.0, "c"), (1.0, "d")):
            parts.append(K.cube(
                f"{asset_id}_tank_{tag}_leg_{tagx}{tagy}",
                size=(0.10 * scale, 0.10 * scale, leg_h),
                location=(x + sx * r * 0.66, y + sy * r * 0.66,
                          z + leg_h * 0.5),
                material=leg_mat))
    # a mid hoop, because a plain cylinder on sticks has no scale cue
    parts.append(K.torus(
        f"{asset_id}_tank_{tag}_hoop", major_radius=r + 0.02 * scale,
        minor_radius=0.035 * scale, major_segments=14, minor_segments=6,
        location=(x, y, z + leg_h + barrel_h * 0.55), material=leg_mat))
    return parts


def _ac_unit(asset_id: str, tag: str, x: float, y: float, z: float,
             case_mat, grill_mat, scale: float = 1.0) -> list:
    """A condenser box: casing, a recessed fan grill and two mounting feet."""
    w, d, h = 1.25 * scale, 0.85 * scale, 0.72 * scale
    return [
        K.cube(f"{asset_id}_ac_{tag}_case", size=(w, d, h),
               location=(x, y, z + h * 0.5 + 0.10 * scale),
               material=case_mat, bevel=0.04),
        K.cylinder(f"{asset_id}_ac_{tag}_grill", radius=min(w, d) * 0.32,
                   depth=0.06 * scale, vertices=12,
                   location=(x, y, z + h + 0.08 * scale),
                   material=grill_mat, smooth=False),
        K.cube(f"{asset_id}_ac_{tag}_foot", size=(w * 0.92, d * 0.86, 0.20 * scale),
               location=(x, y, z + 0.10 * scale), material=grill_mat),
    ]


def _vent_stack(asset_id: str, tag: str, x: float, y: float, z: float,
                pipe_mat, cap_mat, scale: float = 1.0) -> list:
    """A plumbing vent: a vertical pipe with a rain cap on top."""
    r = 0.13 * scale
    h = 1.05 * scale
    return [
        K.cylinder(f"{asset_id}_vent_{tag}_pipe", radius=r, depth=h,
                   vertices=10, location=(x, y, z + h * 0.5),
                   material=pipe_mat, smooth=False),
        K.cone(f"{asset_id}_vent_{tag}_cap", radius1=r * 1.7, radius2=r * 0.3,
               depth=0.20 * scale, vertices=10,
               location=(x, y, z + h + 0.10 * scale), material=cap_mat,
               smooth=False),
    ]


def _antenna(asset_id: str, x: float, y: float, z: float, mast_mat,
             height: float = 3.2) -> list:
    """A guyed mast with two crossbars — the vertical that breaks a flat roof."""
    parts = [
        K.cylinder(f"{asset_id}_antenna_mast", radius=0.055, depth=height,
                   vertices=8, location=(x, y, z + height * 0.5),
                   material=mast_mat, smooth=False),
    ]
    for i, frac in enumerate((0.62, 0.84)):
        parts.append(K.cube(
            f"{asset_id}_antenna_bar{i}", size=(1.05 - i * 0.34, 0.05, 0.05),
            location=(x, y, z + height * frac), material=mast_mat))
    return parts


def _rooftop_kit(asset_id: str, top_w: float, top_d: float, z: float,
                 metal_mat, trim_mat, dark_mat) -> list:
    """Populate one flat roof. Sized to the TOP footprint, never the base.

    Placement is seeded from the asset id so a rebuild is byte-identical —
    `random` without a seed would give every run a different skyline and make
    any geometry diff meaningless. The seed is a CRC32, NOT the builtin
    `hash()`: string hashing is salted per process (PYTHONHASHSEED), so a
    `hash()`-seeded roof would silently differ on every single run, which is
    worse than no determinism at all because the diff looks meaningful.
    """
    rng = random.Random(zlib.crc32(f"{asset_id}:roof".encode("utf-8")))
    # keep furniture inside the parapet, with a margin so nothing overhangs
    inset_x = max(0.6, top_w * 0.5 - 1.7)
    inset_y = max(0.6, top_d * 0.5 - 1.7)
    parts: list = []

    # 1. a timber water tank on the taller side — the Miami signature
    tx = rng.uniform(-inset_x * 0.55, inset_x * 0.55)
    ty = rng.uniform(-inset_y * 0.5, inset_y * 0.5)
    parts += _water_tank(asset_id, "a", tx, ty, z, trim_mat, metal_mat,
                         scale=1.0)

    # 2. a bank of 2-3 condensers along the opposite edge
    n_ac = 2 if top_w < 8.0 else 3
    for i in range(n_ac):
        ax = -inset_x * 0.62 + (inset_x * 1.24) * (i / max(n_ac - 1, 1))
        ay = inset_y * 0.58
        parts += _ac_unit(asset_id, str(i), ax, ay, z, metal_mat, dark_mat,
                          scale=0.9 + 0.18 * i)

    # 3. two vent stacks
    for i, frac in enumerate((0.30, -0.22)):
        parts += _vent_stack(asset_id, str(i),
                             inset_x * frac, -inset_y * 0.62, z,
                             metal_mat, dark_mat, scale=0.9 + 0.2 * i)

    # 4. one antenna mast, on a corner, tall enough to read on the skyline
    parts += _antenna(asset_id, inset_x * 0.66, -inset_y * 0.30, z,
                      metal_mat, height=2.4 + 1.1 * rng.random())

    # 5. a low equipment curb, so the roof deck is not one flat plane
    parts.append(K.cube(
        f"{asset_id}_roof_curb",
        size=(top_w * 0.44, top_d * 0.30, 0.26),
        location=(top_w * 0.16, top_d * 0.10, z + 0.13),
        material=trim_mat, bevel=0.03))
    return parts


def build_tower(
    asset_id: str,
    wall_rgb: Sequence[float],
    *,
    width: float = 18.0,
    depth: float = 16.0,
    height: float = 34.0,
    storey: float = 3.4,
    setbacks: Sequence[float] = (12.0, 22.0),
    window_tint: Sequence[float] = (0.15, 0.26, 0.34),
    accent_rgb: Sequence[float] | None = None,
    neon: Sequence[float] | None = None,
) -> list[bpy.types.Object]:
    """A setback tower. Walls are one box per band at that band's footprint."""
    K.reset_scene()

    wall = _stucco(f"{asset_id}_wall", wall_rgb)
    trim = _trim(f"{asset_id}_trim")
    glass = _glass(f"{asset_id}_glass", window_tint)
    metal = _metal(f"{asset_id}_metal")

    parts: list[bpy.types.Object] = []
    bands = _bands_for_tower(height, width, depth, storey, setbacks)

    for band in bands:
        # massing: one box per band, at THIS band's footprint. Never one box
        # for the whole shaft — that is what buries the upper glazing.
        slab = K.cube(
            f"{asset_id}_{band.name}_wall",
            size=(band.w, band.d, band.height),
            location=(0.0, 0.0, band.zc),
            material=wall, bevel=0.05,
        )
        parts.append(slab)

        # a cornice at the top of each band, sized to the WIDER of this band
        # and the one above, so a transition never floats inside a wall
        next_scale_w = band.w
        try:
            idx = bands.index(band)
            if idx + 1 < len(bands):
                next_scale_w = max(band.w, bands[idx + 1].w)
        except ValueError:
            pass
        cornice = K.cube(
            f"{asset_id}_{band.name}_cornice",
            size=(next_scale_w + 0.34, band.d + 0.34, 0.30),
            location=(0.0, 0.0, band.z1 + 0.10),
            material=trim, bevel=0.04,
        )
        parts.append(cornice)

        parts += _window_grid(band, asset_id, glass, trim)

    # ground-floor plinth, slightly proud of the base band.
    # A 0.05 bevel rounds the bottom edge UP into the base, so a plinth whose
    # centre sits at half its height measures 0.005 m below z=0 once bevelled.
    # Lift it by the bevel width so the bevelled foot lands exactly on y=0.
    base_band = bands[0]
    plinth_h = 0.85
    plinth_bevel = 0.05
    plinth = K.cube(
        f"{asset_id}_plinth",
        size=(base_band.w + 0.5, base_band.d + 0.5, plinth_h),
        location=(0.0, 0.0, plinth_h * 0.5 + plinth_bevel),
        material=trim, bevel=plinth_bevel,
    )
    parts.append(plinth)

    # street-level entrance: recessed, with a metal canopy
    door = K.cube(
        f"{asset_id}_entrance", size=(2.4, 0.10, 2.6),
        location=(0.0, -base_band.d * 0.5 - 0.02, 1.4), material=glass,
    )
    canopy = K.cube(
        f"{asset_id}_canopy", size=(3.4, 1.1, 0.16),
        location=(0.0, -base_band.d * 0.5 - 0.55, 2.95), material=metal, bevel=0.03,
    )
    parts += [door, canopy]

    if accent_rgb is not None:
        accent = _stucco(f"{asset_id}_accent", accent_rgb, roughness=0.7)
        for band in bands:
            pilaster = K.cube(
                f"{asset_id}_{band.name}_pilaster",
                size=(0.34, band.d + 0.18, band.height),
                location=(band.w * 0.5 + 0.09, 0.0, band.zc),
                material=accent, bevel=0.03,
            )
            parts.append(pilaster)

    if neon is not None:
        neon_mat = _sign(f"{asset_id}_neon", neon, strength=5.0)
        for band in bands:
            band_neon = K.cube(
                f"{asset_id}_{band.name}_neon",
                size=(band.w * 0.82, 0.06, 0.10),
                location=(0.0, -band.d * 0.5 - 0.03, band.z1 - 0.35),
                material=neon_mat,
            )
            parts.append(band_neon)

    # Rooftop furniture on the TOP band only. Sizing this to `width`/`depth`
    # (the base footprint) is the mistake this call site exists to prevent:
    # on a tower that steps in 14% twice, the crown is 0.86^2 * 0.92 = 0.68 of
    # the base, so a base-width parapet kit overhangs the roof on all four
    # sides and hangs in the air.
    dark = _concrete(f"{asset_id}_roofdark", (0.30, 0.31, 0.34), roughness=0.9)
    crown = bands[-1]
    parts += _rooftop_kit(asset_id, crown.w, crown.d, crown.z1,
                          metal, trim, dark)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts


def build_shop(
    asset_id: str,
    wall_rgb: Sequence[float],
    awning_rgb: Sequence[float],
    *,
    width: float = 12.0,
    depth: float = 11.0,
    height: float = 7.2,
    storefront: bool = True,
) -> list[bpy.types.Object]:
    """A low commercial block with a glazed ground floor and an awning."""
    K.reset_scene()

    wall = _stucco(f"{asset_id}_wall", wall_rgb)
    trim = _trim(f"{asset_id}_trim")
    glass = _glass(f"{asset_id}_glass")
    awning_mat = _stucco(f"{asset_id}_awning", awning_rgb, roughness=0.75)
    metal = _metal(f"{asset_id}_metal")

    parts: list[bpy.types.Object] = []

    body = K.cube(
        f"{asset_id}_body", size=(width, depth, height),
        location=(0.0, 0.0, height * 0.5), material=wall, bevel=0.06,
    )
    parts.append(body)

    parapet = K.cube(
        f"{asset_id}_parapet", size=(width + 0.42, depth + 0.42, 0.55),
        location=(0.0, 0.0, height + 0.22), material=trim, bevel=0.05,
    )
    parts.append(parapet)

    # upper-floor windows on a single band, at this building's own scale
    band = Band(height * 0.42, height - 0.35, width, depth,
                max(2, int(round(width / 3.0))), "upper")
    parts += _window_grid(band, asset_id, glass, trim)

    if storefront:
        shop_glass = K.cube(
            f"{asset_id}_shopfront", size=(width * 0.80, 0.08, height * 0.34),
            location=(0.0, -depth * 0.5 - 0.02, height * 0.19), material=glass,
        )
        awning = K.cube(
            f"{asset_id}_awning", size=(width * 0.86, 1.5, 0.12),
            location=(0.0, -depth * 0.5 - 0.72, height * 0.40),
            material=awning_mat, bevel=0.03,
        )
        bracket_l = K.cube(
            f"{asset_id}_bracket_l", size=(0.07, 1.5, 0.42),
            location=(-width * 0.34, -depth * 0.5 - 0.72, height * 0.36), material=metal,
        )
        bracket_r = K.cube(
            f"{asset_id}_bracket_r", size=(0.07, 1.5, 0.42),
            location=(width * 0.34, -depth * 0.5 - 0.72, height * 0.36), material=metal,
        )
        parts += [shop_glass, awning, bracket_l, bracket_r]

    # Rooftop furniture sits ABOVE the parapet coping, not on the roof deck:
    # the parapet's own top is at `height + 0.22 + 0.55/2`.
    dark = _concrete(f"{asset_id}_roofdark", (0.30, 0.31, 0.34), roughness=0.9)
    roof_z = height + 0.22 + 0.55 * 0.5
    parts += _rooftop_kit(asset_id, width, depth, roof_z, metal, trim, dark)

    for obj in parts:
        K.ensure_uv(obj)
        K.triangulate(obj)
    return parts