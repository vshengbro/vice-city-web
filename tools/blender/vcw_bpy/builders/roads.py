"""ROAD / TERRAIN tiles — the surfaces the city map is paved with.

These are the ONE family in the library that is not scenery. Every other
asset is placed a handful of times; a road tile is repeated across every
street of the map, so its cost is multiplied by a number nobody chose:

    2000 triangles x a 4 m tile = a 200 m x 200 m block of road costing
    100k triangles on its own, and the city has dozens of blocks.

So the rule here is not "make it look rich" — it is "make it cheap enough
that being wrong about the repeat count does not kill the frame". A plain
slab is 12 triangles; a bevelled one is 68; the zebra crossing is seven of
them. Nothing here reaches the 2000-triangle road budget, because the whole
point of the budget is that a tile must never approach it.

**The z = 0 contract**
Every tile's highest surface is exactly z = 0 and every tile hangs downward
from there, so a road laid across the map has its carriageway on one plane
no matter which tile it came from. That is what makes the tiles tile. It
also means detail CANNOT be stacked upwards, so the pattern used throughout
is inverted from the obvious one:

    obvious : field at z = 0, paint piled on top of it  (+2 mm)
    here    : paint at z = 0, field recessed below it    (-4 mm)

The paint — lane stripes, zebra bars, a grate's bars, a manhole's ring — is
the part that touches z = 0, and the surface it sits on is dropped by
`RECESS`. Two consequences, both wanted:

* No two faces of an asset are ever coplanar, so nothing z-fights. A paint
  quad laid on a field quad at the same height is the single most reliable
  way to ship a broken street, and it is invisible until the day it ships.
* The tile's top is a single flat plane, so the 6 mm edge chamfer every
  slab carries is the only thing that ever breaks up a seam, and it breaks
  it by 6 mm rather than by a lit ledge.

Parts interpenetrate rather than butt: the field slab runs from `-thickness`
up to `-RECESS`, and the accent part on top of it runs from a shallower
depth up to 0. They share volume, share no plane, and the accent always wins
the depth comparison because it is nearer the camera.

**Ground plane**: the renderer generates the world's ground itself. Nothing
in this module is a world-sized plane; every asset here is a single tile or
one landform, sized in metres in the module's own kwargs.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy

from .. import kernel as K
from .. import palette as P


# ---------------------------------------------------------------------------
# tile constants
# ---------------------------------------------------------------------------

TILE_SIZE = 4.0          # default footprint of a tiling surface tile, metres
TILE_THICKNESS = 0.040   # total slab depth, measured DOWNWARD from z = 0
FIELD_THICKNESS = 0.036  # depth of a recessed field slab
ACCENT_THICKNESS = 0.036 # depth of an accent part sitting on a field
RECESS = 0.004           # how far a field surface drops below the tile's z = 0
EDGE_BEVEL = 0.006       # light chamfer so a tiled run shows no razor seam

# Asphalt sits low on purpose: below ~0.04 it crushes to black under the
# ambient term and the whole road reads as a hole, above ~0.11 it stops
# looking like tarmac and starts looking like concrete.
ASPHALT_RGB = (0.074, 0.076, 0.081)
ASPHALT_DARK_RGB = (0.052, 0.053, 0.058)
PAINT_WHITE_RGB = (0.880, 0.880, 0.860)
PAINT_YELLOW_RGB = (0.900, 0.740, 0.100)
CONCRETE_RGB = (0.500, 0.490, 0.470)
CONCRETE_DARK_RGB = (0.400, 0.394, 0.380)
KERB_RGB = (0.600, 0.590, 0.565)
GRASS_RGB = (0.180, 0.420, 0.200)
GRASS_LIGHT_RGB = (0.240, 0.440, 0.210)
GRASS_DEEP_RGB = (0.130, 0.320, 0.140)
SAND_RGB = (0.800, 0.730, 0.560)
SAND_LIGHT_RGB = (0.860, 0.790, 0.620)
PEBBLE_RGB = (0.520, 0.470, 0.410)
IRON_RGB = (0.150, 0.148, 0.142)
IRON_LIGHT_RGB = (0.235, 0.230, 0.220)
ROCK_RGB = (0.300, 0.290, 0.270)
ROCK_LIT_RGB = (0.400, 0.385, 0.350)
SCREE_RGB = (0.225, 0.215, 0.200)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------


def _slab(
    asset_id: str,
    tag: str,
    sx: float,
    sy: float,
    *,
    top: float,
    thickness: float,
    material: bpy.types.Material | None,
    cx: float = 0.0,
    cy: float = 0.0,
    bevel: float = EDGE_BEVEL,
    segments: int = 2,
) -> bpy.types.Object:
    """One closed box whose TOP face lies at `top`, filling downward.

    `K.cube` takes a centre, which is the wrong number for a tile: every
    surface here is authored as a height and has to be arrived at by
    subtracting half a thickness, and doing that by hand at each call site is
    exactly how a tile ends up with its top face at +0.02 and a visible step
    across the seam.
    """
    return K.cube(
        f"{asset_id}_{tag}",
        size=(sx, sy, thickness),
        location=(cx, cy, top - thickness * 0.5),
        material=material,
        bevel=bevel,
        bevel_segments=segments,
    )


def _sculpt(
    obj: bpy.types.Object,
    *,
    location: Sequence[float],
    scale: Sequence[float] | None = None,
    rot_z: float = 0.0,
) -> bpy.types.Object:
    """Scale, spin and THEN move a primitive into place.

    Every transform is applied to the mesh immediately, and the translation
    is the LAST step. Setting `location` on an object whose scale and
    rotation are still live scales and spins the offset too, which throws a
    mountain spur several metres sideways and a few metres underground.
    """
    if scale is not None:
        obj.scale = scale
    if rot_z:
        obj.rotation_euler = (0.0, 0.0, math.radians(rot_z))
    K.transform_apply(obj)
    obj.location = tuple(location)
    K.transform_apply(obj)
    return obj


def _drop(objects: Sequence[bpy.types.Object], dz: float) -> list[bpy.types.Object]:
    """Sink already-built parts by `dz` in Z, baking the offset in.

    Used to seat the shared palette grate into a tile: `P.grate` authors its
    bars standing up from its own local z = 0, and the tile wants them
    *below* z = 0 so the frame's rim is what the camera sees.

    The offset is set on `location` and then applied rather than read back
    and offset. `K.cube` calls `transform_apply(location=True)`, so a
    finished part has `location == (0, 0, 0)` with the position already in
    its mesh data — reading `location.z` to learn how high a part sits
    always yields zero, and a "drop" built on it silently moves nothing.
    """
    out = []
    for obj in objects:
        if obj is None:
            continue
        obj.location = (obj.location.x, obj.location.y, obj.location.z - dz)
        K.transform_apply(obj)
        out.append(obj)
    return out


# ---------------------------------------------------------------------------
# 1. road_asphalt — the tile the whole carriageway is made of
# ---------------------------------------------------------------------------


def build_road_asphalt(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
    roughness: float = 0.95,
) -> list[bpy.types.Object]:
    """One square of tarmac: dark, very rough, top face on z = 0.

    Deliberately the cheapest asset in the library, and the only one that is
    a SINGLE part. Asphalt has no detail to speak of, and the two obvious
    ways to fake some both make the street worse:

    * Extra parts (patches, cracks) must either sit proud of z = 0 — which
      puts a 4 mm step on a surface meant to be dead flat — or be recessed,
      in which case they are inside the slab and never seen.
    * Splitting the tile into sub-slabs of two tones puts a chamfered seam
      every couple of metres, and a grid of seams reads as paving, not tarmac.

    So it is one bevelled slab. Roughness does the work instead: at 0.95 with
    a 0.074 albedo the sun never leaves a highlight on it, which is what the
    eye actually uses to decide whether a surface is tarmac or plastic.
    """
    K.reset_scene()

    asphalt = P.paint(f"{asset_id}_asphalt", ASPHALT_RGB, roughness=roughness)
    parts = [
        _slab(
            asset_id, "surface", size, size,
            top=0.0, thickness=thickness, material=asphalt,
            bevel=0.008, segments=2,
        )
    ]
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 2. road_sidewalk — paving plus the kerb that separates it from the road
# ---------------------------------------------------------------------------


def build_road_sidewalk(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
    recess: float = 0.015,
) -> list[bpy.types.Object]:
    """A pavement tile: two paving panels, a recessed field, a kerb along -Y.

    The kerb is the reason this asset is not just another slab. It sits on
    the tile's -Y edge — the edge the carriageway is on — and it is the only
    part touching z = 0 across its full width, so it reads as a raised lip
    dividing pavement from road even though the whole tile is 4 cm deep.

    The recess under the paving is 15 mm, not the usual 4: a kerb is 100 mm
    tall in the world and this tile cannot be, so all the step it can offer
    is the difference between its top and the paving's. Spending the whole
    available depth on it is what makes the tile read as a kerb rather than
    as a stripe painted along the pavement.

    Both paving panels are lifted to z = 0 with the field between and behind
    them dropped by `recess`, which is the whole paving read: slabs standing
    clear of their joints.
    """
    K.reset_scene()

    field_mat = P.paint(f"{asset_id}_field", CONCRETE_DARK_RGB, roughness=0.92)
    slab_mat = P.paint(f"{asset_id}_paving", CONCRETE_RGB, roughness=0.90)
    kerb_mat = P.paint(f"{asset_id}_kerb", KERB_RGB, roughness=0.88)

    parts: list[bpy.types.Object] = []

    # recessed field: everything the raised parts sit on
    parts.append(
        _slab(asset_id, "field", size, size,
              top=-recess, thickness=thickness, material=field_mat)
    )

    # two paving panels, clear of every tile edge so a repeat never clips one
    panel_w = size * 0.42
    panel_d = size * 0.36
    for sx, cx, cy in (
        ("panel_a", -size * 0.24, size * 0.22),
        ("panel_b", size * 0.25, -size * 0.20),
    ):
        parts.append(
            _slab(asset_id, sx, panel_w, panel_d,
                  top=0.0, thickness=ACCENT_THICKNESS, material=slab_mat,
                  cx=cx, cy=cy)
        )

    # the kerb, flush with the tile's -Y boundary so repeats make one run
    kerb_d = size * 0.10
    parts.append(
        _slab(asset_id, "kerb", size, kerb_d,
              top=0.0, thickness=ACCENT_THICKNESS, material=kerb_mat,
              cy=-size * 0.5 + kerb_d * 0.5,
              bevel=0.008, segments=2)
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 3-4. lane markings
# ---------------------------------------------------------------------------


def build_road_lane_line(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
    width: float = 0.15,
    dash_ratio: float = 0.5,
) -> list[bpy.types.Object]:
    """One dash of a white centre line, half the tile long.

    `dash_ratio` is the fraction of the tile's repeat pitch that is painted,
    so a caller re-laying the lane at a different tile size gets the same
    50%-duty dash rather than a stripe of some arbitrary length. The dash is
    centred in the tile on purpose: a dash split across the seam fuses with
    its opposite number from the neighbour, and a centred one either fits or
    is visibly wrong, never half-stepped.
    """
    K.reset_scene()

    asphalt = P.paint(f"{asset_id}_asphalt", ASPHALT_RGB, roughness=0.95)
    paint = P.paint(f"{asset_id}_paint", PAINT_WHITE_RGB, roughness=0.60)

    parts = [
        _slab(asset_id, "asphalt", size, size,
              top=-RECESS, thickness=thickness, material=asphalt),
        _slab(asset_id, "stripe", width, size * dash_ratio,
              top=0.0, thickness=ACCENT_THICKNESS, material=paint,
              bevel=0.004, segments=1),
    ]
    return P.finish(parts)


def build_road_lane_line_yellow(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
    width: float = 0.12,
    gap: float = 0.28,
) -> list[bpy.types.Object]:
    """A solid double-yellow centre line: two stripes, unbroken, full length.

    Solid where the white line is dashed, because a solid double yellow is
    the marking that says "you may not cross". Both stripes run the tile's
    full length, so the repeat has no visible end at all — the tile's extent
    IS the stripe's period.
    """
    K.reset_scene()

    asphalt = P.paint(f"{asset_id}_asphalt", ASPHALT_RGB, roughness=0.95)
    paint = P.paint(f"{asset_id}_paint", PAINT_YELLOW_RGB, roughness=0.60)

    parts = [
        _slab(asset_id, "asphalt", size, size,
              top=-RECESS, thickness=thickness, material=asphalt)
    ]
    for tag, sign in (("stripe_l", -1.0), ("stripe_r", 1.0)):
        cx = sign * (width + gap) * 0.5
        parts.append(
            _slab(asset_id, tag, width, size,
                  top=0.0, thickness=ACCENT_THICKNESS, material=paint,
                  cx=cx, bevel=0.004, segments=1)
        )
    return P.finish(parts)


# ---------------------------------------------------------------------------
# 5. road_crosswalk — the zebra
# ---------------------------------------------------------------------------


def build_road_crosswalk(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
    bars: int = 4,
    bar_width: float = 0.45,
    depth_ratio: float = 0.75,
) -> list[bpy.types.Object]:
    """A zebra crossing: N+1 bar positions per tile, the outer two split.

    The bar pitch is exactly `size / bars`, and bar k sits at
    ``-size/2 + pitch * k``. That identity is what forces the half bars.
    With every bar sitting fully inside the tile the geometry's X extent
    shrinks by half a bar at each end, so the bounding box no longer equals
    the repeat period and a caller that infers the pitch from the footprint
    puts every stripe out of step with its neighbour's. So the outermost
    bar positions are placed ON the tile boundary and CLIPPED, rather than
    pulled inside: tiled, each half joins the neighbouring tile's half into
    one unbroken bar.

    Every bar is clipped the same way against the tile bounds, so the pitch
    stays uniform all the way across. Nudging the two edge bars inward until
    they fit — the obvious alternative — is what produces the defect this
    avoids: it leaves one 5 cm gap at each end and a 1 m gap everywhere else,
    and a zebra with two suspiciously narrow gaps is visibly a tile.

    `bars` counts positions per tile, not painted stripes, so a full-width
    crossing has bars + 1 of them. The bars run the tile's full depth, which
    is what makes it read as crossing something even when the road tile it
    is laid on repeats the other way.
    """
    K.reset_scene()

    asphalt = P.paint(f"{asset_id}_asphalt", ASPHALT_RGB, roughness=0.95)
    paint = P.paint(f"{asset_id}_paint", PAINT_WHITE_RGB, roughness=0.60)

    parts = [
        _slab(asset_id, "asphalt", size, size,
              top=-RECESS, thickness=thickness, material=asphalt)
    ]

    half = bar_width * 0.5
    pitch = size / bars
    depth = size * depth_ratio
    span = size * 0.5

    for k in range(bars + 1):
        centre = -span + pitch * k
        # clip against the tile, so an edge bar becomes exactly half a bar
        x0 = max(-span, centre - half)
        x1 = min(span, centre + half)
        if x1 - x0 <= 1e-6:
            continue
        parts.append(
            _slab(asset_id, f"bar{k}", x1 - x0, depth,
                  top=0.0, thickness=ACCENT_THICKNESS, material=paint,
                  cx=(x0 + x1) * 0.5, bevel=0.004, segments=1)
        )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 6. road_drain_grate — the only asset here built with the shared palette grate
# ---------------------------------------------------------------------------


def build_road_drain_grate(
    asset_id: str,
    *,
    size: tuple[float, float] = (1.20, 0.80),
    frame: float = 0.12,
    height: float = 0.05,
    slats: int = 7,
) -> list[bpy.types.Object]:
    """A storm drain: a kerbed frame at z = 0 with `P.grate` sunk inside it.

    The frame is four rails around a recessed bedding slab, rather than one
    slab with a hole cut in it. The cut version needs either a boolean or a
    top face split into four regions against four unbroken side walls, and
    the split leaves T-junctions round the rim — which is the geometry the
    outwardness check cannot close, so the shell's winding silently stops
    being verified. Four rails on a plain bed are six closed boxes that all
    verify, and cost less than the hole would.

    `P.grate` authors its bars standing up from its own local z = 0, so the
    whole assembly is dropped by `_drop` until its bars sit just under the
    frame's rim. The bars then read as slats in a recess — which is what a
    drain is — instead of as a rack sitting on top of the road.
    """
    K.reset_scene()

    frame_mat = P.metal(f"{asset_id}_frame", IRON_RGB, rough=0.55, metal=0.60)
    bar_mat = P.metal(f"{asset_id}_bars", IRON_LIGHT_RGB, rough=0.45, metal=0.65)

    sx, sy = size
    parts: list[bpy.types.Object] = []

    # bedding: the recessed surface the whole assembly stands in, so the
    # frame has a floor to be proud OF rather than a hole down to nothing.
    parts.append(
        _slab(asset_id, "bed", sx, sy,
              top=-RECESS * 4.0, thickness=TILE_THICKNESS, material=frame_mat)
    )

    # frame rails: two across the full width, two between them. The rails
    # overshoot the bed by a millimetre on every side so the solids
    # interpenetrate — two butted boxes share a plane, and a shared plane
    # is two coplanar faces z-fighting for the same pixels.
    parts.append(
        _slab(asset_id, "rail_s", sx + 0.002, frame,
              top=0.0, thickness=ACCENT_THICKNESS, material=frame_mat,
              cy=sy * 0.5 - frame * 0.5)
    )
    parts.append(
        _slab(asset_id, "rail_n", sx + 0.002, frame,
              top=0.0, thickness=ACCENT_THICKNESS, material=frame_mat,
              cy=-sy * 0.5 + frame * 0.5)
    )
    parts.append(
        _slab(asset_id, "rail_w", frame, sy + 0.002,
              top=0.0, thickness=ACCENT_THICKNESS, material=frame_mat,
              cx=-sx * 0.5 + frame * 0.5)
    )
    parts.append(
        _slab(asset_id, "rail_e", frame, sy + 0.002,
              top=0.0, thickness=ACCENT_THICKNESS, material=frame_mat,
              cx=sx * 0.5 - frame * 0.5)
    )

    # the shared slotted grate, dropped until its bars clear the rim by 3 mm
    inner = (sx - frame * 2.0 - 0.02, sy - frame * 2.0 - 0.02)
    grate = P.grate(f"{asset_id}_grate", bar_mat, height=height,
                    slats=slats, bar=0.035, along="x", size=inner)
    parts += _drop(grate, height + 0.003)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 7. road_manhole_decal — a low disc with a raised ring
# ---------------------------------------------------------------------------


def build_road_manhole_decal(
    asset_id: str,
    *,
    size: float = 1.00,
    thickness: float = TILE_THICKNESS,
    ring_radius: float = 0.30,
    ring_thickness: float = 0.03,
    segments: int = 20,
) -> list[bpy.types.Object]:
    """A cast-iron cover: recessed field disc, proud ring, proud hub.

    Three concentric parts rather than one disc with a raised rim, because
    the ring has to be ABOVE the field for the cover to read as a cover, and
    the field has to be below z = 0 for the tile to stay flat. A torus ring
    whose centre is exactly one minor-radius below zero puts its crest at
    zero with no arithmetic to get wrong, which is why the ring is a torus
    rather than a bevelled cylinder that would have to be placed by eye.

    The hub keeps the ring from reading as a washer. A cover with a hole in
    it is a drain; a cover with a boss in it is a cover.
    """
    K.reset_scene()

    iron = P.metal(f"{asset_id}_iron", IRON_RGB, rough=0.58, metal=0.60)
    iron_light = P.metal(f"{asset_id}_iron_light", IRON_LIGHT_RGB,
                         rough=0.48, metal=0.65)

    radius = size * 0.44
    parts: list[bpy.types.Object] = []

    # the field disc: the tile's flat surface, dropped below the ring crest
    parts.append(
        K.cylinder(
            f"{asset_id}_field", radius=radius, depth=thickness,
            vertices=segments,
            location=(0.0, 0.0, -RECESS - thickness * 0.5),
            material=iron, smooth=True,
        )
    )

    # the raised ring: crest at z = 0, so its centre sits one minor radius
    # below zero and its underside at exactly -2 * ring_thickness.
    parts.append(
        K.torus(
            f"{asset_id}_ring", major_radius=ring_radius,
            minor_radius=ring_thickness, major_segments=segments,
            minor_segments=6, location=(0.0, 0.0, -ring_thickness),
            material=iron_light,
        )
    )

    # the hub, inside the ring's opening and flush with the crest
    parts.append(
        K.cylinder(
            f"{asset_id}_hub", radius=ring_radius * 0.34, depth=thickness,
            vertices=segments,
            location=(0.0, 0.0, -thickness * 0.5),
            material=iron_light, smooth=True,
        )
    )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# 8-9. ground cover: grass and sand
# ---------------------------------------------------------------------------


def build_road_grass(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
) -> list[bpy.types.Object]:
    """A park verge tile: a recessed field with two raised grass patches.

    Two tones of the same green, not two greens. The patch edges land at
    z = 0 and the field between them sits 4 mm down, so the boundary reads
    as a change of height under a low sun — which is how a lawn actually
    shows a mowing pattern — instead of a hard-edged colour change with no
    light in it at all.

    Both patches are held clear of every tile edge and of each other, because
    a patch clipped in half by a seam repeats that half forever.
    """
    K.reset_scene()

    field_mat = P.foliage(f"{asset_id}_field", GRASS_DEEP_RGB, rough=0.94)
    light_mat = P.foliage(f"{asset_id}_light", GRASS_RGB, rough=0.92)
    pale_mat = P.foliage(f"{asset_id}_pale", GRASS_LIGHT_RGB, rough=0.92)

    parts: list[bpy.types.Object] = [
        _slab(asset_id, "field", size, size,
              top=-RECESS, thickness=thickness, material=field_mat)
    ]
    parts.append(
        _slab(asset_id, "patch_a", size * 0.65, size * 0.50,
              top=0.0, thickness=ACCENT_THICKNESS, material=light_mat,
              cx=-size * 0.09, cy=size * 0.10)
    )
    parts.append(
        _slab(asset_id, "patch_b", size * 0.30, size * 0.25,
              top=0.0, thickness=ACCENT_THICKNESS, material=pale_mat,
              cx=size * 0.30, cy=-size * 0.30, bevel=0.005)
    )
    return P.finish(parts)


def build_road_sand(
    asset_id: str,
    *,
    size: float = TILE_SIZE,
    thickness: float = TILE_THICKNESS,
) -> list[bpy.types.Object]:
    """A beach tile: a recessed field, two lifted tones and three pebbles.

    Sand with no grain reads as a sheet of paper, and the tiles carry no
    texture, so the grain has to be geometry. Three 15 cm pebbles at z = 0
    are all it takes: large enough to have a silhouette when the camera comes
    down to the beach, small enough that a repeat does not turn into a
    pattern you can count. Anything more starts costing triangles per tile
    for detail that is below the aliasing threshold from street height.
    """
    K.reset_scene()

    field_mat = P.paint(f"{asset_id}_field", SAND_RGB, roughness=0.96)
    tone_mat = P.paint(f"{asset_id}_tone", SAND_LIGHT_RGB, roughness=0.96)
    pebble_mat = P.paint(f"{asset_id}_pebble", PEBBLE_RGB, roughness=0.80)

    parts: list[bpy.types.Object] = [
        _slab(asset_id, "field", size, size,
              top=-RECESS, thickness=thickness, material=field_mat)
    ]
    parts.append(
        _slab(asset_id, "tone_a", size * 0.70, size * 0.45,
              top=0.0, thickness=ACCENT_THICKNESS, material=tone_mat,
              cx=-size * 0.05, cy=size * 0.13)
    )
    parts.append(
        _slab(asset_id, "tone_b", size * 0.40, size * 0.30,
              top=0.0, thickness=ACCENT_THICKNESS, material=tone_mat,
              cx=size * 0.15, cy=-size * 0.30, bevel=0.005)
    )

    # grit: three pebbles, clear of both tones and of every tile edge
    for tag, (px, py, pw, pd) in (
        ("pebble_a", (-size * 0.42, -size * 0.38, 0.15, 0.13)),
        ("pebble_b", (size * 0.42, size * 0.22, 0.12, 0.11)),
        ("pebble_c", (-size * 0.40, size * 0.42, 0.13, 0.12)),
    ):
        parts.append(
            _slab(asset_id, tag, pw, pd,
                  top=0.0, thickness=ACCENT_THICKNESS, material=pebble_mat,
                  cx=px, cy=py, bevel=0.008, segments=2)
        )

    return P.finish(parts)


# ---------------------------------------------------------------------------
# TERRAIN — the one asset here that is not flat
# ---------------------------------------------------------------------------


def build_terrain_mountain(
    asset_id: str,
    *,
    width: float = 24.0,
    depth: float = 20.0,
    height: float = 10.0,
    sides: int = 8,
) -> list[bpy.types.Object]:
    """A low-poly landform: stacked tapering masses over a broad footing.

    Built in the XY plane like every other asset here — it just happens to
    have relief — and it RESTS on z = 0 rather than hanging from it. It is
    placed on the map, not tiled across it, so it has no repeat pitch to
    respect and no seam to hide; what it does have to respect is the frame,
    because a terrain asset that costs a few hundred triangles per instance
    is the cheapest way to lose the road tiles' budget on scenery nobody
    drives up to.

    The silhouette comes from three frustums of falling radius, each offset
    from the one below it, so the ridge drifts sideways as it climbs instead
    of rising as a cone. That drift is the whole difference between "a
    mountain" and "a pile". Faceted (never smooth-shaded) because a smoothed
    8-sided cone reads as a cylinder, and the facet edges are what catch the
    low sun and describe the slope.

    Two outcrops and two boulders sit at the base to break the footing's
    silhouette, which is otherwise a perfectly straight line on the ground.
    """
    K.reset_scene()

    rock = P.paint(f"{asset_id}_rock", ROCK_RGB, roughness=0.95)
    rock_lit = P.paint(f"{asset_id}_rock_lit", ROCK_LIT_RGB, roughness=0.92)
    scree = P.paint(f"{asset_id}_scree", SCREE_RGB, roughness=0.98)

    span = min(width, depth)
    parts: list[bpy.types.Object] = []

    # --- the footing: the mountain's contact with the ground ---------------
    foot_h = height * 0.06
    footing = K.cube(
        f"{asset_id}_footing", size=(width, depth, foot_h),
        location=(0.0, 0.0, foot_h * 0.5), material=rock, bevel=0.18,
        bevel_segments=1,
    )
    parts.append(footing)

    # --- three tapering masses, each smaller and offset from the last ------
    #
    # Each mass is sunk a little into the one below it (`SEAT`) rather than
    # butted against it. Two solids that meet at an exact plane are two
    # coplanar faces z-fighting, and here the meeting plane is the widest,
    # most camera-facing face in the whole asset — the artifact would be a
    # flickering ring right across the mountain's mid-slope. Interpenetrating
    # by a couple of centimetres removes the shared plane entirely.
    #
    # Mass i+1's base radius is also kept slightly UNDER mass i's top radius
    # for the same reason: it guarantees the upper mass sits entirely within
    # the lower one's footprint, so no part of it ever hangs in the air.
    z = foot_h
    masses = (
        # (base radius scale of `span`, top radius scale, height fraction,
        #  x offset, y offset, z spin, material)
        (0.52, 0.30, 0.42, 0.00, 0.00, 22.0, rock),
        (0.30, 0.14, 0.30, -0.06, 0.04, -15.0, rock),
        (0.14, 0.00, 0.22, -0.09, 0.07, 40.0, rock_lit),
    )
    seat = height * 0.012
    for i, (r0, r1, hf, ox, oy, spin, mat) in enumerate(masses):
        h = height * hf
        # the first mass still sinks into the footing; the rest sink into
        # whichever mass is below them, which is the same overlap either way
        bottom = z - (seat if i else seat * 2.0)
        mass = K.cone(
            f"{asset_id}_mass{i}", radius1=span * r0, radius2=span * r1,
            depth=h + seat, vertices=sides, location=(0.0, 0.0, 0.0),
            material=mat, smooth=False,
        )
        _sculpt(
            mass,
            location=(width * ox, depth * oy, bottom + (h + seat) * 0.5),
            scale=(1.0, 0.82, 1.0),
            rot_z=spin,
        )
        parts.append(mass)
        z = bottom + h + seat

    # --- outcrops: shoulders of rock where the slope meets the ground ------
    for tag, (ox, oy, r0, r1, hf, spin) in (
        ("outcrop_e", (0.33, 0.30, 0.11, 0.05, 0.10, 10.0)),
        ("outcrop_w", (-0.30, -0.28, 0.10, 0.04, 0.08, -35.0)),
    ):
        h = height * hf
        outcrop = K.cone(
            f"{asset_id}_{tag}", radius1=span * r0, radius2=span * r1,
            depth=h, vertices=6, location=(0.0, 0.0, 0.0),
            material=scree, smooth=False,
        )
        _sculpt(
            outcrop,
            location=(width * ox, depth * oy, h * 0.5),
            scale=(1.25, 0.90, 1.0),
            rot_z=spin,
        )
        parts.append(outcrop)

    # --- boulders resting on z = 0, breaking the footing's straight edge ----
    # Cones, not spheres: `K.sphere` smooth-shades unconditionally, and one
    # smooth blob among faceted rock reads as a different material entirely.
    for tag, (ox, oy, r, hf, spin) in (
        ("boulder_a", (-0.40, 0.33, 0.90, 0.75, 18.0)),
        ("boulder_b", (0.34, -0.38, 0.70, 0.65, -50.0)),
    ):
        h = r * 1.5 * hf
        boulder = K.cone(
            f"{asset_id}_{tag}", radius1=r, radius2=r * 0.45,
            depth=h, vertices=7, location=(0.0, 0.0, 0.0),
            material=scree, smooth=False,
        )
        _sculpt(
            boulder,
            location=(width * ox, depth * oy, h * 0.5),
            scale=(1.2, 0.9, 1.0),
            rot_z=spin,
        )
        parts.append(boulder)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# registry tables — (asset_id, builder_callable, kwargs)
# ---------------------------------------------------------------------------

ROADS: list[tuple[str, Callable, dict]] = [
    ("road_asphalt", build_road_asphalt, dict(size=4.0, thickness=0.04)),
    ("road_crosswalk", build_road_crosswalk, dict(size=4.0, thickness=0.04)),
    ("road_drain_grate", build_road_drain_grate, dict(size=(1.2, 0.8))),
    ("road_grass", build_road_grass, dict(size=4.0, thickness=0.04)),
    ("road_lane_line", build_road_lane_line, dict(size=4.0, thickness=0.04)),
    ("road_lane_line_yellow", build_road_lane_line_yellow,
     dict(size=4.0, thickness=0.04)),
    ("road_manhole_decal", build_road_manhole_decal,
     dict(size=1.0, thickness=0.04)),
    ("road_sand", build_road_sand, dict(size=4.0, thickness=0.04)),
    ("road_sidewalk", build_road_sidewalk, dict(size=4.0, thickness=0.04)),
]

TERRAIN: list[tuple[str, Callable, dict]] = [
    ("terrain_mountain", build_terrain_mountain,
     dict(width=24.0, depth=20.0, height=10.0)),
]
