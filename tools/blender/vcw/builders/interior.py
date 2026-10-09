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
# How far the glass is set BACK from the surrounding wall face.  A belt course
# that stands proud is a painted stripe; one whose glazing is set back into a
# rebate reads as a window because the reveal's sill and lintel catch the
# directional light.  0.10 m keeps the pane behind the wall plane while still
# leaving a 0.20 m of projecting trim above and below it.
BAND_RECESS = 0.10
EDGE_BEVEL = 0.05       # arris chamfer on every shell / floor / stair box


def _shade(c, k):
    return tuple(min(1.0, x * k) for x in c)


def _box(part, x0, x1, y0, y1, z0, z1, color, colors=None, bevel=0.0):
    """One axis-aligned solid box from explicit bounds (never from a centre).

    Bounds in, box out: a centre/size pair on a wall is a rounding error away
    from a 0.25 m wall being 0.24 m wide, and the doorway depends on the wall
    thickness being exactly WALL_T.

    ``bevel`` chamfers every arris.  It is the only lever this file had for
    making a surface interesting: the runtime shades with one directional light
    and no texture, so a hard 90 deg edge has no value break at all and every
    wall, floor and stair box rendered as one dead planar field -- the reason
    these two assets read as flat vector illustration.  A chamfer puts a strip
    at an intermediate normal on all four sides of each edge, so the arris
    catches light without needing a single extra material.
    """
    if bevel > 0.0:
        # chamfer_box takes a size + centre, and sizes a bevel off its own
        # shortest edge, so clamp: a 0.25 m wall cannot carry a 0.05 m chamfer
        # on both ends without degenerating.
        b = min(bevel, 0.24 * min(x1 - x0, y1 - y0, z1 - z0))
        C.chamfer_box(part.mesh,
                      (x1 - x0, y1 - y0, z1 - z0),
                      center=(0.5 * (x0 + x1), 0.5 * (y0 + y1),
                              0.5 * (z0 + z1)),
                      color=color, bevel=b, colors=colors)
        return part
    C.box(part.mesh,
          (x1 - x0, y1 - y0, z1 - z0),
          center=(0.5 * (x0 + x1), 0.5 * (y0 + y1), 0.5 * (z0 + z1)),
          color=color, colors=colors)
    return part


def _wrap_band(a, x_in, x_out, y_in, y_out, z_lo, glass_h):
    """One belt course: a projecting frame with the glazing set BACK in it.

    The course is no longer a solid band sitting proud of the wall.  Instead
    each elevation gets a trim sill, a trim lintel and a recessed pane:

      * sill/lintel run the full outer footprint and stand ``BAND_OUT`` proud,
        so they keep the belt-course silhouette and catch the key light on top;
      * the pane sits ``BAND_RECESS`` BEHIND the wall face between them, which
        is what turns a painted stripe into a window -- the eye reads the sill,
        the gap and the lintel as three separate depth planes.

    Measured before this change, the band sat 50 mm OUTSIDE the shell plane
    with no reveal at all (shell front z=4.500, band z=4.550), so every
    window was a flat applied decal.

    The four sides deliberately OVERLAP at the corners rather than butting, as
    a real projecting string course does.  Overlap is safe for the exporter:
    each box stays a closed shell of positive volume, and adjacent boxes share
    no coincident vertices, so the welded-component split still sees
    independent manifolds rather than a T-junction.
    """
    ox0, ox1 = x_in - BAND_OUT, x_out + BAND_OUT
    oy0, oy1 = y_in - BAND_OUT, y_out + BAND_OUT
    z_sill0, z_sill1 = z_lo, z_lo + glass_h
    z_pane0, z_pane1 = z_sill1, z_sill1 + glass_h
    z_lint0, z_lint1 = z_pane1, z_pane1 + glass_h
    sill_d = {"+z": _shade(TRIM, 1.07), "-z": _shade(TRIM, 0.80)}
    lint_d = {"+z": _shade(TRIM, 0.92), "-z": _shade(TRIM, 0.62)}
    # The pane is inset from the shell face on all four sides, so the reveal
    # between it and the projecting sill/lintel is a genuine gap on every
    # elevation rather than only on the street front.
    px0, px1 = x_in + BAND_RECESS, x_out - BAND_RECESS
    py0, py1 = y_in + BAND_RECESS, y_out - BAND_RECESS
    pane = {"+z": _shade(GLASS, 1.25), "-z": _shade(GLASS, 0.72)}

    for (fz0, fz1, fcol, fd) in ((z_sill0, z_sill1, TRIM, sill_d),
                                 (z_lint0, z_lint1, TRIM, lint_d)):
        _box(a, ox0, ox1, oy0, y_in, fz0, fz1, fcol, fd)
        _box(a, ox0, ox1, y_out, oy1, fz0, fz1, fcol, fd)
        _box(a, ox0, x_in, oy0, oy1, fz0, fz1, fcol, fd)
        _box(a, x_out, ox1, oy0, oy1, fz0, fz1, fcol, fd)

    _box(a, px0, px1, py0, py1, z_pane0, z_pane1, GLASS, pane)

    # JAMBS.  Without these the recess had a lip above and below but no vertical
    # returns, so from a near-side-on angle the whole course still read as a
    # painted stripe -- the eye needs two shadowed vertical edges to accept the
    # surface as a hole rather than a decal.  Each jamb is a thin vertical fin
    # standing proud of the wall and running the full height of the pane, set
    # just inboard of the pane edge so it frames the reveal without covering it.
    jw = 0.09
    jd = {"+z": _shade(TRIM, 1.04), "-z": _shade(TRIM, 0.70)}
    z_a, z_b = z_sill1, z_pane1
    # Front (-Y) and back (+Y): each jamb fin spans from the outer face of its
    # own band edge inward to the pane plane, so the reveal is a real gap.
    for jx0, jx1 in ((px0 - jw, px0), (px1, px1 + jw)):
        _box(a, jx0, jx1, y_in - BAND_OUT, py0, z_a, z_b, TRIM, jd)
        _box(a, jx0, jx1, py1, y_out + BAND_OUT, z_a, z_b, TRIM, jd)
    # Left (-X) and right (+X): same, turned through 90 degrees.
    for jy0, jy1 in ((py0 - jw, py0), (py1, py1 + jw)):
        _box(a, x_in - BAND_OUT, px0, jy0, jy1, z_a, z_b, TRIM, jd)
        _box(a, px1, x_out + BAND_OUT, jy0, jy1, z_a, z_b, TRIM, jd)


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
                             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)},
         bevel=EDGE_BEVEL)
    # RIGHT: exterior faces +x.
    _box(shell, x_out, x_out + WALL_T, y_in - WALL_T, y_out + WALL_T,
         0.0, WALL_H, ext, {"+x": _shade(ext, 1.05), "-x": inn,
                             "+y": inn, "-y": inn,
                             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)},
         bevel=EDGE_BEVEL)
    # BACK: exterior faces +y.
    _box(shell, x_in - WALL_T, x_out + WALL_T, y_out, y_out + WALL_T,
         0.0, WALL_H, ext, {"+y": _shade(ext, 1.05), "-y": inn,
                             "+x": inn, "-x": inn,
                             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)},
         bevel=EDGE_BEVEL)
    # FRONT (-Y), in three boxes around a 1.60 x 2.30 m opening.
    front = {"-y": _shade(ext, 1.05), "+y": inn, "+x": inn, "-x": inn,
             "+z": _shade(ext, 1.08), "-z": _shade(ext, 0.5)}
    _box(shell, x_in - WALL_T, -DOOR_HALF, y_in - WALL_T, y_in,
         0.0, WALL_H, ext, front, bevel=EDGE_BEVEL)
    _box(shell, DOOR_HALF, x_out + WALL_T, y_in - WALL_T, y_in,
         0.0, WALL_H, ext, front, bevel=EDGE_BEVEL)
    _box(shell, -DOOR_HALF, DOOR_HALF, y_in - WALL_T, y_in,
         DOOR_TOP, WALL_H, ext, front, bevel=EDGE_BEVEL)

    # ---- 2. ground floor slab -------------------------------------------
    fg = a.part("floor_ground", base_color=FLOOR_BOARD)
    _box(fg, x_in, x_out, y_in, y_out, 0.0, GROUND_TOP, FLOOR_BOARD,
         {"+z": FLOOR_BOARD, "-z": CONCRETE_DK}, bevel=EDGE_BEVEL)

    # ---- 3. upper floor: an L, not a slab, so the stairwell is a real well -
    x_stair_in = x_out - STAIR_W
    y_stair_bot = y_in + 0.20
    y_stair_top = y_stair_bot + STAIR_STEPS * STAIR_RUN
    fu = a.part("floor_upper", base_color=FLOOR_BOARD)
    _box(fu, x_in, x_stair_in, y_in, y_out, UPPER_BOT, UPPER_TOP,
         FLOOR_BOARD, {"+z": FLOOR_BOARD, "-z": _shade(FLOOR_BOARD, 0.78)},
         bevel=EDGE_BEVEL)
    _box(fu, x_stair_in, x_out, y_stair_top, y_out, UPPER_BOT, UPPER_TOP,
         FLOOR_BOARD, {"+z": FLOOR_BOARD, "-z": _shade(FLOOR_BOARD, 0.78)},
         bevel=EDGE_BEVEL)

    # ---- 4. stair: one box per step, the top step flush with UPPER_TOP ----
    st = a.part("stair", base_color=STAIR_TREAD)
    for i in range(STAIR_STEPS):
        top = GROUND_TOP + (i + 1) * STAIR_RISE
        _box(st, x_stair_in, x_out, y_stair_bot + i * STAIR_RUN,
             y_stair_bot + (i + 1) * STAIR_RUN, GROUND_TOP, top,
             STAIR_TREAD, {"+z": _shade(STAIR_TREAD, 1.04),
                           "-z": _shade(STAIR_TREAD, 0.72)},
             bevel=0.012)

    # ---- 5. interior partition (ground storey only, head height below the
    #         upper slab so the ceiling reads) ------------------------------
    iw = a.part("interior_wall", base_color=accent)
    _box(iw, x_in, x_stair_in - 1.70, PART_Y0, PART_Y1,
         GROUND_TOP, PART_TOP, accent,
         {"+z": _shade(accent, 1.10), "-z": _shade(accent, 0.74)},
         bevel=EDGE_BEVEL)

    # ---- 6. roof slab -----------------------------------------------------
    rf = a.part("roof", base_color=ROOF)
    _box(rf, x_in - WALL_T, x_out + WALL_T, y_in - WALL_T, y_out + WALL_T,
         WALL_H, WALL_H + 0.20, ROOF,
         {"+z": ROOF_DK, "-z": _shade(ROOF, 0.86)}, bevel=EDGE_BEVEL)

    # ---- 7. window belt courses (recessed reveals, upper storey only) -----
    win = a.part("windows", base_color=GLASS, roughness=0.25)
    for z_lo, _z_hi in BANDS:
        _wrap_band(win, x_in, x_out, y_in, y_out, z_lo, BAND_GLASS)

    # ---- 8. cornice + parapet ring (5 boxes) ------------------------------
    cn = a.part("cornice", base_color=TRIM)
    _box(cn, x_in - 0.35, x_out + 0.35, y_in - 0.35, y_out + 0.35,
         WALL_H - 0.35, WALL_H - 0.10, TRIM,
         {"+z": _shade(TRIM, 1.06), "-z": _shade(TRIM, 0.80)},
         bevel=EDGE_BEVEL)
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
