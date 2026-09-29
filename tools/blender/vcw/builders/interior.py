"""Category: enterable two-storey showcases (loft + shop).

The rest of the city is solid decorative mass.  These two are built to be
WALKED INTO: the facade is a real shell with a real hole in it, and every
surface a player can see from inside exists as solid geometry.

Geometry is authored in Blender-style Z-up metres, ``-Y`` is the street-facing
FRONT, and the asset is centred on X = 0 resting on Z = 0 so the exported
Y-up JSON puts the base exactly on y = 0.  The shell spans
``X = [-W/2, W/2]`` and ``Y = [-D/2, D/2]`` exactly, which is what makes the
origin land on the centre of the ground footprint without a re-centring pass.

PART NAMES ARE CONTRACT.  ``src/game.rs`` resolves every part of these two
assets by literal string (``shell``, ``floor_ground``, ``floor_upper``,
``stair``, ``interior_wall``, ``roof``, ``windows``, ``cornice``, ``awning``),
so renaming one silently drops that surface in-game.  Never rename, never
split into a second part with a different name.

All geometry is solid axis-aligned boxes, so every part is a CLOSED shell with
a positive signed volume and needs no declared ``outward`` reference.  The
whole asset is ~520 triangles, far under the 8,000 per-asset budget, and
carries a full colour zone per face -- which is what an untextured runtime
shader (``base * (ambient + light * max(dot(n, l), 0))``, no specular, no
PBR) actually reads.
"""

from .. import core as C

# --------------------------------------------------------------------------
# local palette -- a builder must not import a sibling builder, so these are
# defined here.  Same pastel Art Deco families as the rest of the city so the
# set reads as one block, in two schemes: lilac/cream loft, coral/aqua shop.
# --------------------------------------------------------------------------

LILAC = (0.79, 0.71, 0.93)
CREAM = (0.97, 0.91, 0.76)
CORAL = (0.98, 0.51, 0.40)
AQUA = (0.45, 0.82, 0.88)

CONCRETE = (0.72, 0.70, 0.66)
CONCRETE_DK = (0.55, 0.53, 0.50)
ROOF = (0.42, 0.44, 0.47)
ROOF_DK = (0.33, 0.35, 0.38)
GLASS = (0.13, 0.26, 0.35)
TRIM = (0.98, 0.96, 0.90)
PLASTER = (0.94, 0.91, 0.84)
FLOOR_BOARD = (0.86, 0.80, 0.70)
STAIR_TREAD = (0.89, 0.84, 0.74)
AWNING_PINK = (0.93, 0.25, 0.36)

# Floor / roof geometry is derived from these once per asset, so the two
# storeys and the doorway agree by construction instead of by coincidence.
WALL_T = 0.25
GROUND_TOP = 0.15
UPPER_BOT = 2.95
UPPER_TOP = 3.20
WALL_H = 6.40
DOOR_HALF = 0.80
DOOR_TOP = 2.45
STAIR_W = 1.30
STAIR_RISE = 0.305
STAIR_RUN = 0.45
STAIR_STEPS = 10
PART_Y0, PART_Y1 = -2.475, -2.325
PART_TOP = 2.75

# Window belt courses: (trim bottom z, band top z).  Both sit on the UPPER
# storey, clear of the doorway, so no band can ever intersect the front hole.
BANDS = ((3.70, 4.30), (5.30, 5.90))
BAND_OUT = 0.30          # how far a band stands proud of the wall
BAND_GLASS = 0.30        # upper part of each band is glass, lower part is trim


def _shade(c, k):
    return tuple(min(1.0, x * k) for x in c)


def _box(part, x0, x1, y0, y1, z0, z1, color, colors=None):
    """One axis-aligned solid box from explicit bounds (never from a centre).

    Bounds in, box out: a centre/size pair on a wall is a rounding error away
    from a 0.25 m wall being 0.24 m wide, and the doorway depends on the wall
    thickness being exactly WALL_T.
    """
    C.box(part.mesh,
          (x1 - x0, y1 - y0, z1 - z0),
          center=(0.5 * (x0 + x1), 0.5 * (y0 + y1), 0.5 * (z0 + z1)),
          color=color, colors=colors)
    return part


def _wrap_band(a, x_in, x_out, y_in, y_out, z_lo, glass_h):
    """One belt course: eight solid boxes that lap the corners of the shell.

    Each course is a lower trim sub-band and an upper glass sub-band, four
    boxes each, so the glass reads as a distinct colour zone.  The four boxes
    deliberately OVERLAP at the corners instead of butting, which is what a
    real projecting string course does.  Overlap is safe for the exporter:
    every box stays a closed shell of positive volume, and adjacent boxes
    share no coincident vertices, so the welded-component split still sees
    independent manifolds rather than a T-junction.
    """
    ox0, ox1 = x_in - BAND_OUT, x_out + BAND_OUT
    oy0, oy1 = y_in - BAND_OUT, y_out + BAND_OUT
    for z_a, z_b, col in ((z_lo, z_lo + glass_h, TRIM),
                          (z_lo + glass_h, z_lo + 2.0 * glass_h, GLASS)):
        d = {"+z": _shade(col, 1.07), "-z": _shade(col, 0.80)}
        # front and back run the full outer width...
        _box(a, ox0, ox1, oy0, y_in, z_a, z_b, col, d)
        _box(a, ox0, ox1, y_out, oy1, z_a, z_b, col, d)
        # ...and left and right run the full outer depth, closing the corners.
        _box(a, ox0, x_in, oy0, oy1, z_a, z_b, col, d)
        _box(a, x_out, ox1, oy0, oy1, z_a, z_b, col, d)


def showcase(asset_id, width, depth, wall, accent, awning=False):
    """Assemble one enterable two-storey building.

    ``width`` / ``depth`` are the FULL outer footprint, and the shell is built
    outward-in from them, so the asset stays centred on X = 0 and its bounds
    are exactly +-width/2 by +-depth/2 -- well inside the engine's 11.0 m
    footprint guard with room to spare for the projecting cornice.
    """
    a = C.Asset(asset_id, "building")

    x_in = -0.5 * width + WALL_T
    x_out = 0.5 * width - WALL_T
    y_in = -0.5 * depth + WALL_T
    y_out = 0.5 * depth - WALL_T

    # exterior faces are the wall colour, interior faces are pale plaster:
    # a free colour zone that makes the inside read as a room.
    ext = wall
    inn = PLASTER
    solid = {"+z": _shade(ext, 1.05), "-z": _shade(ext, 0.55)}
    both = dict(solid)
    both["+x"] = inn
    both["-x"] = inn
    both["+y"] = inn
    both["-y"] = inn

    # ---- 1. shell: four walls, the front one built in three pieces so the
    #         doorway is a REAL hole rather than a dark decal ---------------
    shell = a.part("shell", base_color=ext)
    # LEFT: exterior faces -x.
    _box(shell, x_in - WALL_T, x_in, y_in - WALL_T, y_out + WALL_T,
         0.0, WALL_H, ext, {"-x": _shade(ext, 1.05), "+x": inn,
                             "+y": inn, "-y": inn,
                             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)})
    # RIGHT: exterior faces +x.
    _box(shell, x_out, x_out + WALL_T, y_in - WALL_T, y_out + WALL_T,
         0.0, WALL_H, ext, {"+x": _shade(ext, 1.05), "-x": inn,
                             "+y": inn, "-y": inn,
                             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)})
    # BACK: exterior faces +y.
    _box(shell, x_in - WALL_T, x_out + WALL_T, y_out, y_out + WALL_T,
         0.0, WALL_H, ext, {"+y": _shade(ext, 1.05), "-y": inn,
                             "+x": inn, "-x": inn,
                             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)})
    # FRONT (-Y), in three boxes around a 1.60 x 2.30 m opening.
    front = {"-y": _shade(ext, 1.05), "+y": inn, "+x": inn, "-x": inn,
             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)}
    _box(shell, x_in - WALL_T, -DOOR_HALF, y_in - WALL_T, y_in,
         0.0, WALL_H, ext, front)
    _box(shell, DOOR_HALF, x_out + WALL_T, y_in - WALL_T, y_in,
         0.0, WALL_H, ext, front)
    _box(shell, -DOOR_HALF, DOOR_HALF, y_in - WALL_T, y_in,
         DOOR_TOP, WALL_H, ext, front)

    # ---- 2. ground floor slab -------------------------------------------
    fg = a.part("floor_ground", base_color=FLOOR_BOARD)
    _box(fg, x_in, x_out, y_in, y_out, 0.0, GROUND_TOP, FLOOR_BOARD,
         {"+z": FLOOR_BOARD, "-z": CONCRETE_DK})

    # ---- 3. upper floor: an L, not a slab, so the stairwell is a real well -
    x_stair_in = x_out - STAIR_W
    y_stair_bot = y_in + 0.20
    y_stair_top = y_stair_bot + STAIR_STEPS * STAIR_RUN
    fu = a.part("floor_upper", base_color=FLOOR_BOARD)
    _box(fu, x_in, x_stair_in, y_in, y_out, UPPER_BOT, UPPER_TOP,
         FLOOR_BOARD, {"+z": FLOOR_BOARD, "-z": _shade(FLOOR_BOARD, 0.78)})
    _box(fu, x_stair_in, x_out, y_stair_top, y_out, UPPER_BOT, UPPER_TOP,
         FLOOR_BOARD, {"+z": FLOOR_BOARD, "-z": _shade(FLOOR_BOARD, 0.78)})

    # ---- 4. stair: one box per step, the top step flush with UPPER_TOP ----
    st = a.part("stair", base_color=STAIR_TREAD)
    for i in range(STAIR_STEPS):
        top = GROUND_TOP + (i + 1) * STAIR_RISE
        _box(st, x_stair_in, x_out, y_stair_bot + i * STAIR_RUN,
             y_stair_bot + (i + 1) * STAIR_RUN, GROUND_TOP, top,
             STAIR_TREAD, {"+z": _shade(STAIR_TREAD, 1.04),
                           "-z": _shade(STAIR_TREAD, 0.72)})

    # ---- 5. interior partition (ground storey only, head height below the
    #         upper slab so the ceiling reads) ------------------------------
    iw = a.part("interior_wall", base_color=accent)
    _box(iw, x_in, x_stair_in - 1.70, PART_Y0, PART_Y1,
         GROUND_TOP, PART_TOP, accent,
         {"+z": _shade(accent, 1.10), "-z": _shade(accent, 0.74)})

    # ---- 6. roof slab -----------------------------------------------------
    rf = a.part("roof", base_color=ROOF)
    _box(rf, x_in - WALL_T, x_out + WALL_T, y_in - WALL_T, y_out + WALL_T,
         WALL_H, WALL_H + 0.20, ROOF,
         {"+z": ROOF_DK, "-z": _shade(ROOF, 0.86)})

    # ---- 7. window belt courses (solid boxes, upper storey only) ----------
    win = a.part("windows", base_color=GLASS, roughness=0.25)
    for z_lo, _z_hi in BANDS:
        _wrap_band(win, x_in, x_out, y_in, y_out, z_lo, BAND_GLASS)

    # ---- 8. cornice + parapet ring (5 boxes) ------------------------------
    cn = a.part("cornice", base_color=TRIM)
    _box(cn, x_in - 0.35, x_out + 0.35, y_in - 0.35, y_out + 0.35,
         WALL_H - 0.35, WALL_H - 0.10, TRIM,
         {"+z": _shade(TRIM, 1.06), "-z": _shade(TRIM, 0.80)})
    pz0, pz1 = WALL_H + 0.20, WALL_H + 0.75
    px0, px1 = -0.5 * width, 0.5 * width
    py0, py1 = -0.5 * depth, 0.5 * depth
    pt = 0.20
    pcol = {"+z": CONCRETE, "-z": _shade(CONCRETE, 0.80)}
    _box(cn, px0, px1, py0, py0 + pt, pz0, pz1, CONCRETE, pcol)
    _box(cn, px0, px1, py1 - pt, py1, pz0, pz1, CONCRETE, pcol)
    _box(cn, px0, px0 + pt, py0 + pt, py1 - pt, pz0, pz1, CONCRETE, pcol)
    _box(cn, px1 - pt, px1, py0 + pt, py1 - pt, pz0, pz1, CONCRETE, pcol)

    # ---- 9. awning over the doorway (shop only) --------------------------
    if awning:
        aw = a.part("awning", base_color=AWNING_PINK)
        _box(aw, -1.5, 1.5, y_in - 0.9, y_in, 2.50, 2.62, AWNING_PINK,
             {"+z": _shade(AWNING_PINK, 1.12),
              "-z": _shade(AWNING_PINK, 0.62)})

    return a


def build_all():
    """The two enterable showcase buildings."""
    return [
        # Loft showcase: lilac walls, cream trim, no awning.  12 x 10 m.
        showcase("bldg_loft_showcase", 12.0, 10.0, LILAC, CREAM),
        # Shop showcase: coral walls, aqua accent, hot-pink awning.  10 x 9 m.
        showcase("bldg_shop_showcase", 10.0, 9.0, CORAL, AQUA, awning=True),
    ]
