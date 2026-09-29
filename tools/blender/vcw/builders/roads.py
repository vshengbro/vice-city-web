"""ROAD / GROUND SURFACE assets -- the tiled skins that break up the street.

The runtime has no textures, no specular term and no PBR: two polygons look
different ONLY when they differ in normal or in albedo.  So every asset in this
module is built the one way that is free -- a **coarse cell grid whose cells
carry different ``face_colors``**.  A 6x6 grid over 12 m costs 72 triangles and
buys 36 independent albedo values: the same trick as a 12-triangle box with six
painted sides, scaled up to a surface.

Three rules shape everything here, and all three come from how these assets are
used rather than from how they look.

1. **The tile edge is a hard boundary.**  A chamfered or bevelled tile edge is
   the classic way to break a repeating surface: the chamfer catches the light
   on one side of the seam and the neighbour's catches it on the other, so a
   straight kerbline turns into a row of bright and dark notches.  Nothing here
   is chamfered; a tile is plain flat quads out to its outermost edge.
2. **The pattern is centred and periodic.**  Cell colours come from a table
   indexed by ``(i, j)``, so the same cell sits on the same colour on both
   sides of every seam no matter how the table is retuned -- the pattern cannot
   drift out of phase.  Every asset is symmetric about its own origin, and
   every non-grid detail is held clear of the tile edge so a repeat never
   clips one in half.
3. **Tiles are thin.**  12-30 mm keeps them clear of the procedural ground
   plane in ``src/`` while still giving a real lip to catch a low sun.

Nothing here glows.  Lane paint, crossings and the manhole cover all carry an
all-zero ``emissive`` on purpose: road markings are retroreflective, not light
sources, and a night pass that made every white stripe a lamp post would read
as a bug, not as paint.

Authoring is Blender-style Z-up, centred on X = 0, resting on z = 0.
``build_all`` returns nine assets, all in category ``road``.
"""

import math

from .. import core as C

# --------------------------------------------------------------------------
# palette
#
# Asphalt sits in the 0.05-0.09 luminance band on purpose.  Below ~0.04 it
# crushes to black under the ambient term and the cell grid stops reading;
# above ~0.11 it stops looking like tarmac and starts looking like concrete.
# The four values are separated by ~0.006, well under one 8-bit step per
# channel but comfortably over the 4-decimal colour quantisation the exporter
# writes, so adjacent cells stay distinguishable in the JSON.
# --------------------------------------------------------------------------

ASPHALT = ((0.074, 0.076, 0.081),
           (0.062, 0.063, 0.068),
           (0.086, 0.088, 0.092),
           (0.068, 0.071, 0.075))
ASPHALT_PATCH = (0.046, 0.047, 0.050)      # tar-sealed repair, darker
ASPHALT_CRACK = (0.028, 0.028, 0.031)      # hairline crack, near-black

# 6x6 asphalt cell table.  Rows run -Y -> +Y, columns -X -> +X.
ASPHALT_CELLS = (
    (0, 1, 0, 3, 1, 0),
    (2, 0, 3, 0, 1, 2),
    (1, 3, 1, 2, 0, 3),
    (0, 2, 0, 1, 3, 1),
    (3, 1, 2, 0, 2, 0),
    (0, 3, 0, 2, 1, 3),
)

# -- concrete sidewalk ------------------------------------------------------
# Five slab greys 0.02-0.03 apart, grout a full stop darker, and a kerb whose
# top is one stop lighter than the slabs while its riser is a stop darker --
# the two-value kerb is what makes it read as a raised edge rather than as a
# painted stripe.
CONCRETE = ((0.500, 0.490, 0.470),
            (0.460, 0.455, 0.440),
            (0.530, 0.520, 0.500),
            (0.440, 0.435, 0.425),
            (0.485, 0.478, 0.462))
CONCRETE_SIDE = (0.400, 0.394, 0.380)      # the tile's own outer edge
GROUT = (0.160, 0.155, 0.148)              # narrow dark joint between slabs
KERB_TOP = (0.600, 0.590, 0.565)           # lighter top face
KERB_FACE = (0.330, 0.325, 0.312)          # darker vertical riser

# 6x6 slab table.  The last row sits against the kerb, so it is weighted
# towards the lighter greys -- the course next to a raised kerb is the one a low
# sun rakes across, and baking that into the palette means the tile still reads
# correctly from a camera that never sees the kerb's own riser.
CONCRETE_CELLS = (
    (0, 2, 0, 1, 3, 4),
    (2, 4, 1, 0, 2, 4),
    (1, 0, 2, 3, 4, 1),
    (0, 3, 4, 2, 0, 3),
    (3, 1, 2, 4, 0, 2),
    (2, 4, 3, 1, 0, 2),
)

# -- paint ------------------------------------------------------------------
# Never emissive.  High roughness keeps the runtime from treating a stripe as
# anything other than the flat albedo it is.
PAINT_WHITE = (0.880, 0.880, 0.860)
PAINT_CROSS = (0.855, 0.855, 0.835)
PAINT_YELLOW = (0.900, 0.740, 0.100)

# -- ground cover -----------------------------------------------------------
GRASS_BASE = (0.200, 0.360, 0.150)
GRASS_LIGHT = (0.240, 0.420, 0.170)
GRASS_DARK = (0.170, 0.310, 0.130)
GRASS_DEEP = (0.145, 0.270, 0.115)         # shaded irregular patch
DIRT = (0.360, 0.260, 0.150)               # warm brown, bare earth

GRASS_CELLS = ((0, 1, 0, 2, 1, 0),
               (2, 0, 1, 0, 2, 0),
               (1, 2, 0, 1, 2, 1),
               (0, 1, 2, 0, 1, 0))
GRASS_PALETTE = (GRASS_BASE, GRASS_LIGHT, GRASS_DARK)

SAND_BASE = (0.800, 0.730, 0.560)
SAND_LIGHT = (0.860, 0.790, 0.620)
SAND_DARK = (0.720, 0.650, 0.490)
PEBBLE = (0.460, 0.420, 0.370)
PEBBLE_LIGHT = (0.620, 0.580, 0.510)

SAND_CELLS = GRASS_CELLS                   # same drift, different palette
SAND_PALETTE = (SAND_BASE, SAND_LIGHT, SAND_DARK)

# -- ironwork ---------------------------------------------------------------
IRON_DARK = (0.115, 0.112, 0.108)          # outer ring
IRON_MID = (0.165, 0.160, 0.152)           # transition ring
IRON_FIELD = (0.235, 0.228, 0.215)         # lighter inner field
IRON_HUB = (0.085, 0.082, 0.078)           # dark centre
IRON_RIB = (0.300, 0.295, 0.283)           # radial rib marks

DRAIN_FRAME = (0.130, 0.128, 0.124)
DRAIN_RECESS = (0.062, 0.060, 0.058)
DRAIN_BAR = (0.330, 0.325, 0.310)


# --------------------------------------------------------------------------
# geometry helpers
# --------------------------------------------------------------------------

def _slab(m, hx0, hx1, hy0, hy1, z0, z1, cells, side, bottom):
    """A closed thin slab whose TOP face is a grid of coloured cells.

    Emitted by hand rather than as ``C.box`` plus a grid laid over its top:
    a box carries a single top quad, and layering a grid on it puts two
    coplanar surfaces on top of each other so the tiles z-fight against
    themselves.  Writing the outer walls and the cell grid into one shell also
    means there is no ledge for a grazing camera to see under -- the grid's
    outer boundary IS the side wall's top edge.

    ``cells`` is a list of ``(x0, x1, y0, y1, color)`` that must tile
    [hx0, hx1] x [hy0, hy1] EXACTLY -- every point covered once, no quad
    overlapping another.  Any overlap or gap leaks the shell and the exporter's
    signed-volume check reports it.

    A note on what this shell is NOT, because it decides how it must be
    verified.  The grid subdivides the top boundary into n cells while each
    side wall is a single quad, so the perimeter carries T-junctions and the
    position-welded component test cannot close it: ``closed_components``
    reports the part as an open shell and the exporter then leaves its winding
    UNCHECKED.  That is not a defect -- the surface is geometrically closed to
    the eye and to the rasteriser -- but "unchecked" is strictly weaker than
    "proved", so the owning parts declare ``outward=("point", <centre>)``
    instead, which makes the exporter check every single triangle against a
    declared interior and prove the winding outright.

    Winding (authoring Z-up, CCW seen from outside):

        bottom   (-X,-Y) (-X,+Y) (+X,+Y) (+X,-Y)   -> -Z
        -Y side  x runs +X, z runs +Z               -> -Y
        +Y side  z runs +Z, x runs +X               -> +Y
        +X side  y runs +Y, z runs +Z               -> +X
        -X side  z runs +Z, y runs +Y               -> -X
        cell     (x0,y0) (x1,y0) (x1,y1) (x0,y1)    -> +Z
    """
    m.quad((hx0, hy0, z0), (hx0, hy1, z0), (hx1, hy1, z0), (hx1, hy0, z0),
           bottom)
    m.quad((hx0, hy0, z0), (hx1, hy0, z0), (hx1, hy0, z1), (hx0, hy0, z1),
           side)
    m.quad((hx0, hy1, z0), (hx0, hy1, z1), (hx1, hy1, z1), (hx1, hy1, z0),
           side)
    m.quad((hx1, hy0, z0), (hx1, hy1, z0), (hx1, hy1, z1), (hx1, hy0, z1),
           side)
    m.quad((hx0, hy0, z0), (hx0, hy0, z1), (hx0, hy1, z1), (hx0, hy1, z0),
           side)
    for (x0, x1, y0, y1, col) in cells:
        m.quad((x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1), col)
    return m


def _inside(m):
    """The midpoint of a mesh's own bounding box, as a declared interior.

    Only valid for a shape that is star-shaped about that point -- a slab, a
    frame with a rectangular recess -- which is every shell built by
    :func:`_slab` and by :func:`_road_drain_grate` here.  Passing it as
    ``outward=("point", ...)`` upgrades those parts from "unchecked" to
    "proved": the exporter then measures every one of their triangles against
    this reference instead of reporting them as an open shell.
    """
    lo, hi = m.bounds()
    return (lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5


def _square_cells(hx, n, z, table, palette):
    """n x n cell quads tiling [-hx, hx]^2 at height ``z``.

    Boundaries are derived once from a single ``s``, so cell k always ends
    exactly where cell k+1 begins and the outermost cell ends exactly on the
    tile edge -- which is what lets the side walls weld.
    """
    s = 2.0 * hx / n
    out = []
    for j in range(n):
        y0 = -hx + s * j
        for i in range(n):
            x0 = -hx + s * i
            out.append((x0, x0 + s, y0, y0 + s,
                        palette[table[j][i] % len(palette)]))
    return out


# A fixed per-corner jitter table, so an "irregular" blob is irregular in the
# same way on every build instead of re-rolling on each run.  Corner 0 is
# unjittered and the other three are scaled by distinct factors > 0, so no
# blob can ever collapse to a degenerate quad.
_JIT = (0.0, 0.17, -0.11, 0.09)


def _blob(m, cx, cy, rx, ry, rot_deg, z, color, jit=1.0):
    """One irregular quad lying flat at ``z``, facing +Z.

    A rectangle is honest about being a decal; a four-corner quad with each
    corner pushed out by a different amount reads as a stain, a worn patch or
    bare earth instead.  Costs 2 triangles, same as the rectangle.
    """
    a = math.radians(rot_deg)
    ca, sa = math.cos(a), math.sin(a)
    pts = []
    for k, (sx, sy) in enumerate(((-1.0, -1.0), (1.0, -1.0),
                                  (1.0, 1.0), (-1.0, 1.0))):
        s = 1.0 + jit * _JIT[k]
        px, py = sx * rx * s, sy * ry * s
        pts.append((cx + px * ca - py * sa, cy + px * sa + py * ca, z))
    m.quad(pts[0], pts[1], pts[2], pts[3], color)
    return m


def _crack(m, x0, y0, x1, y1, width, z, color, kink=0.22):
    """A hairline crack: a thin strip that kinks once at its midpoint.

    Two quads rather than one, because a crack running dead straight reads as
    a drawn line; the kink is what makes it read as a fracture.  The two quads
    share their full-width edge at the kink, so the strip stays manifold.
    """
    dx, dy = x1 - x0, y1 - y0
    ln = math.sqrt(dx * dx + dy * dy)
    if ln < 1e-6:
        return m
    nx, ny = -dy / ln, dx / ln            # unit left-normal of the run
    hw = width * 0.5
    mx, my = (x0 + x1) * 0.5 + nx * kink, (y0 + y1) * 0.5 + ny * kink
    sr = (x0 - nx * hw, y0 - ny * hw, z)
    mr = (mx - nx * hw, my - ny * hw, z)
    er = (x1 - nx * hw, y1 - ny * hw, z)
    sl = (x0 + nx * hw, y0 + ny * hw, z)
    ml = (mx + nx * hw, my + ny * hw, z)
    el = (x1 + nx * hw, y1 + ny * hw, z)
    m.quad(sr, mr, ml, sl, color)
    m.quad(mr, er, el, ml, color)
    return m


def _paving_bands(lo, hi, n, grout):
    """Boundary positions for a centred n-slab paving grid with grout joints.

    Returns ``2n + 2`` positions, laid out as grout, slab, grout, slab ...
    grout, so the SAME joint width runs along the tile's outer edge as runs
    between the slabs.  Tiled, the two half-joints on either side of a seam
    add up to a full one and the paving grid carries straight across the
    boundary with no visible tile line -- which is the entire point of centring
    the pattern.
    """
    slab = (hi - lo - (n + 1) * grout) / n
    xs = [lo]
    for _ in range(n):
        xs.append(xs[-1] + grout)
        xs.append(xs[-1] + slab)
    xs.append(xs[-1] + grout)
    xs[0], xs[-1] = lo, hi                  # kill accumulated float drift
    return xs


# --------------------------------------------------------------------------
# 1. road_asphalt -- the 12 m tileable carriageway
# --------------------------------------------------------------------------

def _road_asphalt():
    """12 x 12 m of tarmac: 6x6 cells of four near-identical dark greys.

    This asset exists to answer one complaint -- the street read as one huge
    flat expanse of a single colour.  The fix is not more geometry, it is
    albedo variation, and the cheapest place to put albedo variation is the
    faces the tile already has.  Four greys inside a 0.024 luminance band is
    invisible as a pattern and unmistakable as texture once the tiles repeat
    across four lanes of road.

    Two extra parts sit 2-3 mm proud of that grid: three tar-sealed patch
    repairs and three hairline cracks.  They are separate PARTS rather than
    more cells because they are not periodic -- a patch that repeated on a 12 m
    lattice would read as wallpaper -- and they declare
    ``outward=("dir", +Z)`` because a lone quad has no interior for the
    exporter's signed-volume check to measure.
    """
    a = C.Asset("road_asphalt", "road")
    hx, z0, z1 = 6.0, 0.0, 0.020
    cells = _square_cells(hx, 6, z1, ASPHALT_CELLS, ASPHALT)

    surf = a.part("road_asphalt_surface", base_color=ASPHALT[0],
                  roughness=0.95, metallic=0.0)
    _slab(surf.mesh, -hx, hx, -hx, hx, z0, z1, cells,
          C.shade(ASPHALT[1], 0.92), C.shade(ASPHALT[1], 0.80))
    surf.outward = ("point", _inside(surf.mesh))

    # Patch repairs, each held at least 1 m clear of every tile edge so a
    # repeated seam never clips one in half.
    patches = C.Part("road_asphalt_patches", base_color=ASPHALT_PATCH,
                     roughness=0.90, outward=("dir", (0.0, 0.0, 1.0)))
    for (cx, cy, rx, ry, rot) in ((2.40, -3.10, 1.90, 1.30, 18.0),
                                  (-3.60, 2.20, 1.50, 1.10, -34.0),
                                  (-1.20, 4.10, 1.20, 0.85, 52.0)):
        _blob(patches.mesh, cx, cy, rx, ry, rot, z1 + 0.002, ASPHALT_PATCH)
    a.add(patches)

    cracks = C.Part("road_asphalt_cracks", base_color=ASPHALT_CRACK,
                    roughness=0.98, outward=("dir", (0.0, 0.0, 1.0)))
    for (x0, y0, x1, y1, w, k) in ((-4.80, -1.40, -1.60, -0.35, 0.05, 0.30),
                                   (0.90, 1.10, 4.60, 1.95, 0.045, -0.26),
                                   (-0.40, 5.35, 3.30, 4.15, 0.05, 0.24)):
        _crack(cracks.mesh, x0, y0, x1, y1, w, z1 + 0.003, ASPHALT_CRACK, k)
    a.add(cracks)
    return a


# --------------------------------------------------------------------------
# 2. road_sidewalk -- 12 m paving with a kerb on the -Y edge
# --------------------------------------------------------------------------

def _road_sidewalk():
    """12 x 12 m of concrete paving: five slab greys, dark grout, a kerb lip.

    The grout is drawn as real quads at the SAME height as the slabs, a shade
    darker, rather than as a recess between raised slabs.  A 3 cm step would be
    more physical, but it is invisible at street distance and it forces either
    a second grid or per-slab side walls -- and the runtime has no ambient
    occlusion, so a groove with no contact shadow in it renders as a BRIGHT
    hairline, the exact opposite of grout.

    The kerb is a single box that rises 60 mm out of the paving.  It does NOT
    butt against the paving's edge: a shared plane between two shells is two
    coplanar surfaces and they z-fight.  Instead the kerb is 250 mm deep and
    reaches 100 mm PAST the paving's -Y edge, so the two solids interpenetrate
    and no two faces of the asset are ever coplanar.  The kerb's own -Y face
    then lands on the tile boundary, where the flat quads of the paving slab
    also land, and both read as one continuous cast kerb.
    """
    a = C.Asset("road_sidewalk", "road")
    hx = 6.0
    kerb_y = -hx
    pav_y0 = -hx + 0.15                    # paving starts behind the kerb
    z0, z_pave, z_kerb = 0.0, 0.140, 0.200
    grout_w = 0.07
    n = 6

    xs = _paving_bands(-hx, hx, n, grout_w)
    ys = _paving_bands(pav_y0, hx, n, grout_w)

    cells = []
    for j in range(n):
        for i in range(n):
            cells.append((xs[2 * i + 1], xs[2 * i + 2],
                          ys[2 * j + 1], ys[2 * j + 2],
                          CONCRETE[CONCRETE_CELLS[j][i]]))
    # Grout, decomposed into exactly 14 disjoint quads.  The split matters:
    # a grout plan that crosses itself is two coplanar quads fighting for the
    # same pixels.  Every grout COLUMN runs the full height; every grout ROW
    # runs only across the slab columns.  Column quads therefore live at
    # x in a joint, row quads at x in a slab, so the two sets are disjoint --
    # and their union with the 36 slabs is the whole rectangle.
    for k in range(n + 1):
        cells.append((xs[2 * k], xs[2 * k + 1], ys[0], ys[-1], GROUT))
    for k in range(n + 1):
        cells.append((xs[1], xs[-2], ys[2 * k], ys[2 * k + 1], GROUT))

    pave = a.part("sidewalk_paving", base_color=CONCRETE[0], roughness=0.90,
                  metallic=0.0)
    _slab(pave.mesh, -hx, hx, pav_y0, hx, z0, z_pave, cells, CONCRETE_SIDE,
          C.shade(GROUT, 0.85))
    pave.outward = ("point", _inside(pave.mesh))

    kerb = a.part("sidewalk_kerb", base_color=KERB_TOP, roughness=0.88)
    C.box(kerb.mesh, (2.0 * hx, 0.250, z_kerb),
          center=(0.0, kerb_y + 0.125, z_kerb * 0.5),
          color=KERB_TOP,
          colors={"+z": KERB_TOP,
                  "-z": KERB_FACE,
                  "+y": KERB_FACE,                 # the riser above the paving
                  "-y": KERB_FACE,
                  "+x": C.shade(KERB_TOP, 0.94),
                  "-x": C.shade(KERB_TOP, 0.94)})
    return a


# --------------------------------------------------------------------------
# 3-4. lane lines
# --------------------------------------------------------------------------

LANE_H = 0.012                            # paint thickness; 12 mm


def _road_lane_line():
    """6 x 0.15 m white dashed centre line, resting on z = 0, 12 mm thick.

    The tile is 6.00 m long, twice the 3 m dash period, and it is built as a
    HALF dash at each end plus one full dash in the middle.  Those two halves
    are the same stripe as the neighbouring tile's: tiled, the 0.75 m piece at
    y = +3 fuses with the 0.75 m piece at y = -3 of the next tile into one
    unbroken 1.5 m dash, so a tiled road shows a clean 50%-duty dash every 3 m
    with no seam, no doubled stripe and no missing one.

    The two obvious alternatives both fail, for the same reason -- they break
    the agreement between the geometry and the module.  Two dashes centred on
    the tile (y = +/-0.75) leaves gaps of 0.75 m and 2.25 m, an asymmetry the eye
    reads instantly as a repeating artifact.  Two dashes at y = -2.25 and +0.75
    tiles perfectly but gives the asset a 4.50 m extent, so anything inferring
    the repeat pitch from the bounding box places every dash 1.5 m out of step.
    Splitting the end dashes is what makes both the 6 m extent and the 3 m
    period true at the same time.

    Emissive is all-zero.  Retroreflective paint is not a light source, and a
    glowing centre line turns every night frame into a runway.
    """
    a = C.Asset("road_lane_line", "road")
    paint = a.part("lane_line_paint", base_color=PAINT_WHITE, roughness=0.62,
                   metallic=0.0)
    for (y0, y1) in ((-3.00, -2.25), (-0.75, 0.75), (2.25, 3.00)):
        C.box(paint.mesh, (0.150, y1 - y0, LANE_H),
              center=(0.0, (y0 + y1) * 0.5, LANE_H * 0.5), color=PAINT_WHITE)
    return a


def _road_lane_line_yellow():
    """6 m of solid double yellow: two 0.12 m stripes 0.28 m apart.

    Solid rather than dashed, because a solid double centre line is the thing
    that says "you may not cross" -- and the 6 m tile is exactly one period of
    an unbroken stripe, so it tiles with no visible end at all.
    """
    a = C.Asset("road_lane_line_yellow", "road")
    paint = a.part("lane_line_yellow_paint", base_color=PAINT_YELLOW,
                   roughness=0.62, metallic=0.0)
    for cx in (-0.14, 0.14):
        C.box(paint.mesh, (0.120, 6.0, LANE_H),
              center=(cx, 0.0, LANE_H * 0.5), color=PAINT_YELLOW)
    return a


# --------------------------------------------------------------------------
# 5. road_crosswalk
# --------------------------------------------------------------------------

def _road_crosswalk():
    """Six 0.45 m zebra bars with 0.55 m gaps, on a 6.00 m tile -- exactly.

    The tile is 6.00 m and the bar PITCH is 1.00 m, so there are exactly six
    bar positions per tile and 6.00 is a whole multiple of the pitch.  That
    identity is the whole design: it lets the tile's bounding box EQUAL its
    repeat period, so a caller that infers the pitch from the bounding box --
    the natural thing for a tileable decal -- places every bar in step.

    Which forces one non-obvious consequence.  If all six bars sat fully
    inside the tile there would be a half-width margin of empty road at each
    end, the geometry's extent would be 5.45 m, and every bar would be half a
    metre out of phase with the next tile's.  So the bar positions are placed
    on the tile's BOUNDARY instead: five full bars at x = -2, -1, 0, 1, 2 and
    two half bars clipped flush at x = -3 and x = +3, which are the two halves
    of one bar split by the seam.  Tiled, each half joins its opposite number
    from the neighbouring tile into an unbroken 0.45 m bar -- six of them per
    period, each 0.45 m wide, each gap exactly 0.55 m.

    The gap is 0.55 rather than the nominal 0.60 because 6 x 0.45 + 6 x 0.55
    must equal the 6.00 m tile, and 0.60 would make it 6.30.  The 0.05 m the
    spec's nominal value costs is the price of a tile that tiles.
    """
    a = C.Asset("road_crosswalk", "road")
    paint = a.part("crosswalk_paint", base_color=PAINT_CROSS, roughness=0.62,
                   metallic=0.0)
    hx, hy = 3.0, 2.0
    bar_w = 0.45
    pitch = 1.00                            # 0.45 bar + 0.55 gap
    half = bar_w * 0.5
    # (-x0, +x0) spans of painted x, ordered left to right.  The first and last
    # are half bars flush with the tile edges; the rest are full bars on the
    # 1.00 m pitch.
    spans = [(-hx, -hx + half)]
    for k in range(-2, 3):
        spans.append((k * pitch - half, k * pitch + half))
    spans.append((hx - half, hx))
    for (x0, x1) in spans:
        C.box(paint.mesh, (x1 - x0, 2.0 * hy, LANE_H),
              center=((x0 + x1) * 0.5, 0.0, LANE_H * 0.5), color=PAINT_CROSS)
    return a


# --------------------------------------------------------------------------
# 6-7. ground cover
# --------------------------------------------------------------------------

def _road_grass():
    """8 x 8 m of park verge: mid green, two patch greens, bare earth.

    The 4x4 cell grid carries the broad tonal drift and stays periodic; the
    blobs are the non-repeating detail -- four shaded patches and four warm
    dirt scuffs, all of them clear of the tile edge.  Keeping the organic
    detail OUT of the grid is what stops a tiled lawn from looking like a
    chequerboard of identical stains repeating forever.
    """
    a = C.Asset("road_grass", "road")
    hx, z0, z1 = 4.0, 0.0, 0.020
    grass = a.part("road_grass_base", base_color=GRASS_BASE, roughness=0.95,
                   metallic=0.0)
    _slab(grass.mesh, -hx, hx, -hx, hx, z0, z1,
          _square_cells(hx, 4, z1, GRASS_CELLS, GRASS_PALETTE),
          GRASS_DARK, C.shade(GRASS_DARK, 0.80))
    grass.outward = ("point", _inside(grass.mesh))

    patches = C.Part("road_grass_patches", base_color=GRASS_DEEP,
                     roughness=0.95, outward=("dir", (0.0, 0.0, 1.0)))
    for (cx, cy, rx, ry, rot, col) in (
            (1.30, -1.55, 1.05, 0.78, 21.0, GRASS_DEEP),
            (-2.05, 1.70, 0.88, 0.66, -47.0, GRASS_LIGHT),
            (2.45, 2.35, 0.72, 0.58, 63.0, GRASS_DARK),
            (-0.85, -2.75, 0.62, 0.50, -12.0, GRASS_DEEP)):
        _blob(patches.mesh, cx, cy, rx, ry, rot, z1 + 0.002, col)
    a.add(patches)

    dirt = C.Part("road_grass_dirt", base_color=DIRT, roughness=0.98,
                  outward=("dir", (0.0, 0.0, 1.0)))
    for (cx, cy, rx, ry, rot) in ((-0.60, 0.95, 0.42, 0.30, 15.0),
                                  (2.90, -2.20, 0.34, 0.25, -38.0),
                                  (-2.85, -1.10, 0.30, 0.24, 51.0),
                                  (0.95, 3.05, 0.26, 0.20, 8.0)):
        _blob(dirt.mesh, cx, cy, rx, ry, rot, z1 + 0.003, DIRT)
    a.add(dirt)
    return a


def _road_sand():
    """8 x 8 m of beach sand: pale warm base, tonal drift, pebble specks.

    Sand without a grain reads as a sheet of paper, and the runtime has no
    texture to supply one -- so the grain is nine tiny lighter and darker
    quads, 5-9 cm across.  At that size they sit below the threshold where they
    alias into noise, and up close they are the only thing on the tile with any
    silhouette at all.
    """
    a = C.Asset("road_sand", "road")
    hx, z0, z1 = 4.0, 0.0, 0.018
    sand = a.part("road_sand_base", base_color=SAND_BASE, roughness=0.96,
                  metallic=0.0)
    _slab(sand.mesh, -hx, hx, -hx, hx, z0, z1,
          _square_cells(hx, 4, z1, SAND_CELLS, SAND_PALETTE),
          SAND_DARK, C.shade(SAND_DARK, 0.86))
    sand.outward = ("point", _inside(sand.mesh))

    tone = C.Part("road_sand_tone", base_color=SAND_LIGHT, roughness=0.96,
                  outward=("dir", (0.0, 0.0, 1.0)))
    for (cx, cy, rx, ry, rot, col) in (
            (1.55, -1.20, 1.35, 0.95, 24.0, SAND_LIGHT),
            (-1.90, 1.65, 1.10, 0.82, -41.0, SAND_DARK),
            (2.60, 2.10, 0.90, 0.66, 7.0, SAND_LIGHT),
            (-0.75, -2.65, 0.78, 0.58, 58.0, SAND_DARK)):
        _blob(tone.mesh, cx, cy, rx, ry, rot, z1 + 0.002, col)
    a.add(tone)

    grit = C.Part("road_sand_grit", base_color=PEBBLE, roughness=0.80,
                  outward=("dir", (0.0, 0.0, 1.0)))
    for (cx, cy, rx, ry, rot, col) in (
            (-1.10, -0.35, 0.075, 0.055, 12.0, PEBBLE),
            (-0.35, 0.95, 0.060, 0.048, -27.0, PEBBLE_LIGHT),
            (0.85, 0.10, 0.090, 0.060, 40.0, PEBBLE),
            (2.05, -1.85, 0.065, 0.050, -9.0, PEBBLE_LIGHT),
            (2.95, 0.60, 0.080, 0.058, 33.0, PEBBLE),
            (-2.35, 2.35, 0.070, 0.052, -50.0, PEBBLE),
            (-1.75, -2.95, 0.058, 0.045, 19.0, PEBBLE_LIGHT),
            (0.40, 3.20, 0.072, 0.055, -15.0, PEBBLE),
            (3.30, -2.70, 0.062, 0.048, 47.0, PEBBLE_LIGHT)):
        _blob(grit.mesh, cx, cy, rx, ry, rot, z1 + 0.003, col, jit=0.55)
    a.add(grit)
    return a


# --------------------------------------------------------------------------
# 8. road_manhole_decal
# --------------------------------------------------------------------------

def _road_manhole_decal():
    """0.9 m cast-iron cover, 20 mm proud, colour zones and 8 radial ribs.

    Zones and ribs are done ENTIRELY in ``face_colors`` on a 7x7 cell grid,
    because that is what a real cover is: one disc of metal whose zones differ
    in colour and wear, not in shape.  Building the rings as real steps would
    cost more triangles AND introduce ledges that catch the sun as bright
    rings -- the opposite of how worn iron reads.

    Zone is the CHEBYSHEV radius from the centre cell, so the rings are square.
    A cover is round, but a square decal is what has to sit flush in a square
    tile without a chamfer; at street distance the ribs carry the read.

    Ribs: a cell is a rib when its bearing from the hub falls within 0.14 rad
    of a multiple of 45 degrees AND its Chebyshev radius is at least 2, which
    yields exactly 8 radial arms running from the middle out to the rim.  The
    radius floor is what keeps the hub and the lighter inner field readable --
    at radius 1 the eight surrounding cells sit at 0, 45, 90 ... degrees
    exactly, so without the floor every one of them would be a rib and the
    inner field would vanish.
    """
    a = C.Asset("road_manhole_decal", "road")
    hx, z0, z1 = 0.45, 0.0, 0.020
    n = 7
    half = (n - 1) * 0.5
    s = 2.0 * hx / n
    spoke = math.pi / 4.0
    cells = []
    for j in range(n):
        for i in range(n):
            cx = float(i) - half
            cy = float(j) - half
            r = max(abs(cx), abs(cy))
            if r == 0.0:
                col = IRON_HUB
            elif r <= 1.0:
                col = IRON_FIELD
            elif r <= 2.0:
                col = IRON_MID
            else:
                col = IRON_DARK
            if r >= 2.0:
                ang = math.atan2(cy, cx)
                near = round(ang / spoke) * spoke
                if abs(ang - near) < 0.14:
                    col = IRON_RIB
            cells.append((-hx + s * i, -hx + s * (i + 1),
                          -hx + s * j, -hx + s * (j + 1), col))
    cover = a.part("manhole_cover", base_color=IRON_MID, roughness=0.55,
                   metallic=0.60)
    _slab(cover.mesh, -hx, hx, -hx, hx, z0, z1, cells, IRON_DARK,
          C.shade(IRON_DARK, 0.75))
    cover.outward = ("point", _inside(cover.mesh))
    return a


# --------------------------------------------------------------------------
# 9. road_drain_grate
# --------------------------------------------------------------------------

def _road_drain_grate():
    """0.6 x 0.4 m storm drain: dark recessed frame, 7 lighter bars.

    The frame is a plain 12-triangle box -- the recessed hole is implied by the
    18 mm step between the frame's own top and the bars, which sit below it and
    stop 15 mm short of every wall.  That is the cheapest thing that reads as a
    drain: a dark opening framed by a lighter lip, with a grating in it.

    The obvious alternative -- a 4-region top face with real recess walls around
    the hole -- was built and thrown out.  The top subdivides into four quads
    while the walls stay single, so the perimeter carries T-junctions, the
    position-welded component test cannot close the shell, and the exporter
    leaves the winding UNCHECKED.  A box has no T-junctions at all: the
    exporter proves its signed volume outright, and the hole is carried by the
    bars' 6 mm drop rather than by a boolean.  12 triangles for a frame that
    verifies itself beats 28 for one that does not.

    Bars are flat quads with ``outward=("dir", +Z)`` -- a lone quad has no
    interior for the volume check to measure, and a declared direction is the
    honest description of a marking painted on a surface.
    """
    a = C.Asset("road_drain_grate", "road")
    hx, hy = 0.30, 0.20
    z0, z_top = 0.0, 0.030
    frame = a.part("drain_frame", base_color=DRAIN_FRAME, roughness=0.52,
                   metallic=0.55)
    C.box(frame.mesh, (2 * hx, 2 * hy, z_top - z0),
          center=(0.0, 0.0, (z_top + z0) * 0.5), color=DRAIN_FRAME,
          colors={"+z": C.shade(DRAIN_FRAME, 1.12),
                  "-z": C.shade(DRAIN_RECESS, 0.80),
                  "+y": C.shade(DRAIN_FRAME, 0.92),
                  "-y": C.shade(DRAIN_FRAME, 0.88),
                  "+x": C.shade(DRAIN_FRAME, 0.96),
                  "-x": C.shade(DRAIN_FRAME, 0.96)})

    bars = a.part("drain_bars", base_color=DRAIN_BAR, roughness=0.45,
                  metallic=0.55, outward=("dir", (0.0, 0.0, 1.0)))
    for k in range(7):
        y0 = -0.108 + k * 0.036
        bars.mesh.quad((-0.235, y0, 0.024), (0.235, y0, 0.024),
                       (0.235, y0 + 0.024, 0.024), (-0.235, y0 + 0.024, 0.024),
                       DRAIN_BAR)
    return a


# --------------------------------------------------------------------------

def build_all():
    """Return the ordered list of road / ground-surface assets."""
    return [
        _road_asphalt(),
        _road_sidewalk(),
        _road_lane_line(),
        _road_lane_line_yellow(),
        _road_crosswalk(),
        _road_grass(),
        _road_sand(),
        _road_manhole_decal(),
        _road_drain_grate(),
    ]
