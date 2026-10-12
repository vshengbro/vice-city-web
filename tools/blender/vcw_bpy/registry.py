"""The single registry of every asset this pipeline builds.

Exactly one module owns this file. When several workers build asset families
in parallel, each owning one new builder module, two workers editing the
registry collide non-deterministically and the collision surfaces as a build
failure neither brief predicted — so builders write their own module and the
registry imports them, and the registry is edited in one place only.

Every id here is the **shipped** id from `assets/*.json`; `build_all.py`
asserts the registry set equals the on-disk set, so a typo or a missing family
fails the build instead of silently shipping 98 of 99 assets.
"""

from __future__ import annotations

from typing import Callable

from . import palette as P  # noqa: F401  (build modules rely on it being loaded)
from .builders import animals, beach, buildings, misc, naturals, pedestrians
from .builders import pickups, props, roads, signs, vehicles, weapons


def _tower(asset_id, wall, *, accent=None, neon=None, **kw):
    return buildings.build_tower(asset_id, wall, accent_rgb=accent, neon=neon, **kw)


def _shop(asset_id, wall, awning, **kw):
    return buildings.build_shop(asset_id, wall, awning, **kw)


BUILDINGS: list[tuple[str, Callable, dict]] = [
    ("bldg_lilac_tower", _tower, dict(
        wall=(0.72, 0.60, 0.86), accent=(0.96, 0.92, 0.99), neon=(0.85, 0.30, 1.0),
        width=17.0, depth=16.0, height=42.6, storey=3.4, setbacks=(11.0, 22.0, 32.0))),
    ("bldg_white_landmark", _tower, dict(
        wall=(0.96, 0.95, 0.93), accent=(0.88, 0.86, 0.82), neon=(0.45, 0.85, 1.0),
        width=20.0, depth=19.0, height=38.0, storey=3.6, setbacks=(13.0, 25.0))),
    ("bldg_pink_terrace", _tower, dict(
        wall=(0.98, 0.66, 0.76), accent=(1.0, 0.86, 0.90), neon=(1.0, 0.35, 0.70),
        width=22.0, depth=18.0, height=26.0, storey=3.3, setbacks=(10.5, 18.0))),
    ("bldg_sand_midrise", _tower, dict(
        wall=(0.90, 0.80, 0.62), accent=(0.98, 0.92, 0.78),
        width=19.0, depth=17.0, height=24.0, storey=3.2, setbacks=(12.0,))),
    ("bldg_aqua_arcade", _tower, dict(
        wall=(0.48, 0.84, 0.86), accent=(0.90, 0.98, 0.99), neon=(0.20, 0.95, 1.0),
        width=21.0, depth=17.0, height=30.0, storey=3.3, setbacks=(12.5, 21.0))),
    ("bldg_deco_teal", _tower, dict(
        wall=(0.36, 0.76, 0.72), accent=(0.94, 0.98, 0.96),
        width=16.0, depth=15.0, height=29.0, storey=3.2, setbacks=(11.5, 20.0))),
    ("bldg_deco_pink", _tower, dict(
        wall=(0.95, 0.60, 0.78), accent=(1.0, 0.90, 0.95), neon=(1.0, 0.45, 0.80),
        width=16.0, depth=15.0, height=27.0, storey=3.2, setbacks=(11.5, 20.0))),
    ("bldg_cream_block", _tower, dict(
        wall=(0.95, 0.92, 0.84), accent=(0.88, 0.84, 0.76),
        width=20.0, depth=16.0, height=22.0, storey=3.2, setbacks=(11.0,))),
    ("bldg_coral_hall", _tower, dict(
        wall=(0.98, 0.68, 0.56), accent=(1.0, 0.88, 0.80),
        width=19.0, depth=16.0, height=25.0, storey=3.3, setbacks=(12.0,))),
    ("bldg_apricot_motel", _shop, dict(
        wall=(0.98, 0.80, 0.58), awning=(0.95, 0.42, 0.38),
        width=18.0, depth=13.0, height=8.4)),
    ("bldg_mint_shop", _shop, dict(
        wall=(0.72, 0.94, 0.84), awning=(0.24, 0.72, 0.62),
        width=12.0, depth=11.0, height=7.0)),
    ("bldg_teal_loft", _shop, dict(
        wall=(0.44, 0.72, 0.78), awning=(0.98, 0.92, 0.70),
        width=14.0, depth=12.0, height=8.0)),
    ("bldg_loft_showcase", _shop, dict(
        wall=(0.92, 0.86, 0.80), awning=(0.55, 0.72, 0.90),
        width=12.0, depth=10.0, height=6.4)),
    ("bldg_shop_showcase", _shop, dict(
        wall=(0.90, 0.78, 0.70), awning=(0.95, 0.60, 0.40),
        width=10.0, depth=9.0, height=5.8)),
]


# Vessels float; everything else in VEHICLES sits on the ground. The verifier
# applies a hard y=0 rule to ground assets and a waterline rule to these, so
# the category has to be per-asset rather than per-module.
WATERCRAFT = {"veh_boat", "yacht_sailboat", "ship_cruise", "ship_destroyer"}


def all_assets() -> list[tuple[str, str, Callable, dict]]:
    """Every asset as (id, category, builder, kwargs), in a stable order."""
    out: list[tuple[str, str, Callable, dict]] = []
    for aid, fn, kw in BUILDINGS:
        out.append((aid, "building", fn, kw))
    for aid, fn, kw in vehicles.VEHICLES:
        out.append((aid, "watercraft" if aid in WATERCRAFT else "vehicle", fn, kw))
    for group, cat in (
        (pedestrians.PEDESTRIANS, "pedestrian"),
        (props.PROPS, "prop"),
        (signs.SIGNS, "sign"),
        (pickups.PICKUPS, "pickup"),
        (weapons.WEAPONS, "weapon"),
        (naturals.PALMS, "palm"),
        (naturals.NATURE, "nature"),
        (beach.BEACH, "beach"),
        (animals.ANIMALS, "animal"),
        (misc.MISC, "misc"),
        (misc.MARKERS, "marker"),
        (roads.ROADS, "ground_tile"),
        (roads.TERRAIN, "landform"),
    ):
        for aid, fn, kw in group:
            out.append((aid, cat, fn, kw))
    return out