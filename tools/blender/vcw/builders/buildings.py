"""Category A: Art Deco / pastel Miami buildings.

All buildings share one facade kit so the set reads as a single city block:
vertical pilasters, horizontal string courses, stepped setbacks, balcony
parapets, a roof parapet, rooftop water tank + AC units, and a blank neon sign
backing board.  Colour and proportions vary per variant.

The runtime shader is ``base * (ambient + light * max(dot(n, l), 0))`` -- no
texture, no specular.  The only three ways a surface can become interesting are
therefore a different NORMAL, a different ALBEDO, or emissive, and that is the
organising principle of this file:

* every large facade volume is a ``chamfer_box`` (28 tris instead of 12), so
  each arris catches the directional light on all four sides;
* colour is ZONED per part and per face -- dark plinth, trim string courses, a
  lighter upper storey block, deep blue-grey glass, warm emissive windows and
  a concrete roof that is markedly darker than the painted walls;
* windows are REAL RECESSES: a cream frame ring standing proud of the wall
  with the glass set back behind its reveals, rather than a sticker on a wall;
* the roof carries clutter (bulkhead, aerials, vent, dishes) so the skyline
  is not a row of flat lids.

Origin is the centre of the ground footprint at z = 0 (so the JSON export puts
the building base exactly on y = 0).  ``-Y`` is the street-facing front.

Footprint half-extent is a hard engine constraint: ``src/game.rs`` culls any
building whose half-extent exceeds ``BUILDING_FOOTPRINT_GUARD = 11.0``, so no
variant may exceed 10.5 m.
"""

from .. import core as C

# Miami pastels + Art Deco cream/teal. Original palette, no reference images.
PALETTE = {
    "pink":      (0.96, 0.55, 0.66),
    "mint":      (0.62, 0.90, 0.79),
    "teal":      (0.20, 0.66, 0.64),
    "coral":     (0.98, 0.51, 0.40),
    "cream":     (0.97, 0.91, 0.76),
    "apricot":   (0.99, 0.75, 0.53),
    "lilac":     (0.79, 0.71, 0.93),
    "aqua":      (0.45, 0.82, 0.88),
    "sand":      (0.93, 0.84, 0.66),
    "white":     (0.96, 0.96, 0.94),
}

CONCRETE = (0.72, 0.70, 0.66)
CONCRETE_DK = (0.55, 0.53, 0.50)
ROOF = (0.42, 0.44, 0.47)
ROOF_DK = (0.33, 0.35, 0.38)
GLASS = (0.13, 0.26, 0.35)
GLASS_LIT = (1.00, 0.86, 0.50)
TRIM = (0.98, 0.96, 0.90)
AWNING = (0.93, 0.25, 0.36)
METAL = (0.62, 0.64, 0.66)
METAL_DK = (0.38, 0.40, 0.42)
DOOR = (0.15, 0.19, 0.25)
TANK_A = (0.68, 0.57, 0.44)
TANK_B = (0.54, 0.44, 0.33)

# How far the window surround stands proud of the wall, and how far the glass
# sits back from that surround's face.  The step between the two is what turns a
# decal into a hole: the sill faces up, the lintel faces down, and both catch
# light.
#
# SCALING.  These were sized for a turntable, not for the street.  At the old
# 90 mm/15 mm pair the reveal measured 3.0 px at 40 m and 0.8 px at 150 m, so
# the one detail that makes a facade read as architecture vanished exactly when
# the player could see the building.  At 220 mm/100 mm the reveal holds up to
# roughly 80 m and the surrounding chamfer stays sub-pixel only past it.
#
# ORDERING CONSTRAINT: ``GLASS_SET`` must stay strictly below ``FRAME_PROUD``.
# The shaft is a SOLID box -- there is no boolean hole cut in the wall -- so
# ``GLASS_SET`` cannot be a depth *into* the wall.  It is the pane's stand-off
# above the wall face, and the reveal is the difference between the two.  Get
# this backwards and the pane disappears inside solid geometry; see the sign
# note at the window loop and ``check_window_recess.py``.
FRAME_PROUD = 0.22
GLASS_SET = 0.10


def _shrub(color, k=1.0):
    return tuple(min(1.0, c * k) for c in color)


def _lit_at(f, i, face, mod=11, thresh=4):
    """Scattered, low-density lit-window mask (roughly ``thresh/mod`` of all).

    4/11 lights ~36% of the glazing.  The emissive band is the only facade cue
    that survives distance -- a 10 cm reveal disappears at 150 m, a glowing pane
    does not -- so the lit fraction is deliberately the largest single source of
    value contrast in the whole category.
    """
    return ((f * 7 + i * 3 + face * 5 + 1) % mod) < thresh


# --------------------------------------------------------------------------
# window assembly
# --------------------------------------------------------------------------

def _annulus(m, y, x0, x1, z0, z1, ow, oh, color, flip):
    """A flat rectangular ring (a picture-frame border) in the plane ``y``.

    4 quads = 8 triangles for a border that reads as a moulded surround.  It
    is an open shell, so the owning Part must declare ``outward``.
    """
    outer = ((x0 - ow, z0 - oh), (x1 + ow, z0 - oh),
             (x1 + ow, z1 + oh), (x0 - ow, z1 + oh))
    inner = ((x0, z0), (x1, z0), (x1, z1), (x0, z1))
    for j in range(4):
        a = outer[j]
        b = outer[(j + 1) % 4]
        c = inner[(j + 1) % 4]
        d = inner[j]
        pts = ((a[0], y, a[1]), (b[0], y, b[1]),
               (c[0], y, c[1]), (d[0], y, d[1]))
        if flip:
            m.quad(pts[0], pts[3], pts[2], pts[1], color)
        else:
            m.quad(pts[0], pts[1], pts[2], pts[3], color)


def _annulus_side(m, x, z0, z1, y0, y1, ow, oh, color, flip):
    """`_annulus` for a wall lying in the Z/Y plane at constant ``x``.

    The front facade ring is built in X/Z at a fixed Y; a gable or end wall
    needs the same border in Z/Y at a fixed X.  Same four quads, other axes.
    """
    outer = ((z0 - ow, y0 - oh), (z1 + ow, y0 - oh),
             (z1 + ow, y1 + oh), (z0 - ow, y1 + oh))
    inner = ((z0, y0), (z1, y0), (z1, y1), (z0, y1))
    for j in range(4):
        a = outer[j]
        b = outer[(j + 1) % 4]
        c = inner[(j + 1) % 4]
        d = inner[j]
        pts = ((x, a[0], a[1]), (x, b[0], b[1]),
               (x, c[0], c[1]), (x, d[0], d[1]))
        if flip:
            m.quad(pts[0], pts[3], pts[2], pts[1], color)
        else:
            m.quad(pts[0], pts[1], pts[2], pts[3], color)


def _reveal_side(m, z, y0, y1, x_a, x_b, color, up):
    """End-wall sill/lintel: a horizontal band at height ``z`` spanning the
    window width in Y, running the reveal depth in X.

    Mirrors `_reveal` (one constant height plane, one axis swept) so the sill
    really is a sill and not a vertical jamb band.

    Both ends share a single Part, so the depth span is normalised to a
    positive X extent first.  The east end's depth runs toward -X and the west
    end's toward +X, which would emit opposite windings and hence disagreeing
    normals that one ``outward`` cannot express.  ``up`` picks the facing:
    sills up, lintels down.
    """
    x_lo, x_hi = (x_a, x_b) if x_b >= x_a else (x_b, x_a)
    a = (x_lo, y0, z)
    b = (x_hi, y0, z)
    c = (x_hi, y1, z)
    d = (x_lo, y1, z)
    if up:
        m.quad(a, b, c, d, color)
    else:
        m.quad(a, d, c, b, color)


def _reveal(m, x0, x1, z, y_a, y_b, color, up):
    """One window reveal: the jamb face between the frame ring and the glass.

    The quad lies in the plane ``z`` and is emitted so its normal points ``up``
    (a sill) or down (a lintel).  Four of these per window give the recess its
    shadowed inner walls.
    """
    a = (x0, y_a, z)
    b = (x1, y_a, z)
    c = (x1, y_b, z)
    d = (x0, y_b, z)
    if (y_b > y_a) == bool(up):
        m.quad(a, b, c, d, color)
    else:
        m.quad(a, d, c, b, color)


def _banded_cylinder(m, r, h, seg, center, col_a, col_b, caps=True):
    """A barrel built stave by stave, alternating two shades.

    ``C.cylinder`` takes a single colour, and a roof tank whose staves are one
    flat value is exactly the "white model" tell.  Emitting the side quads by
    hand costs the same 20 triangles and buys the banding.  Flat shaded on
    purpose: the seams are the read, and a 10-sided faceted barrel is honest
    about being built from boards.
    """
    cx, cy, cz = center
    ring = C.circle(r, seg)
    lo = [(cx + px, cy + py, cz - h * 0.5) for (px, py) in ring]
    hi = [(cx + px, cy + py, cz + h * 0.5) for (px, py) in ring]
    for j in range(seg):
        j2 = (j + 1) % seg
        m.quad(lo[j], lo[j2], hi[j2], hi[j], col_a if j % 2 == 0 else col_b)
    if caps:
        # Register the ring points once more so the cap fans can index them,
        # exactly as cylinder()/loft() do -- duplicate POSITIONS would leave
        # the cap rim used by exactly one face each, i.e. a T-junction.
        base = len(m.pos)
        for p in lo:
            m.pos.append(p)
        tbase = len(m.pos)
        for p in hi:
            m.pos.append(p)
        cb = len(m.pos)
        m.pos.append((cx, cy, cz - h * 0.5))
        ct = len(m.pos)
        m.pos.append((cx, cy, cz + h * 0.5))
        for j in range(seg):
            j2 = (j + 1) % seg
            m.tri_idx(base + j2, base + j, cb, col_b)
            m.tri_idx(tbase + j, tbase + j2, ct, col_b)
    return m


class _Slots(object):
    """Tiny 2D rejection test so roof clutter never interpenetrates.

    Every candidate is a footprint rectangle; the first one that collides with
    an already-placed footprint is simply dropped.  On the wide roofs all the
    clutter fits; on a slimmest setback tower a dish may lose, which is the
    right trade against a dish growing through a water tank.
    """

    def __init__(self):
        self.taken = []

    def free(self, x, y, hx, hy):
        for (tx, ty, thx, thy) in self.taken:
            if (abs(x - tx) < hx + thx) and (abs(y - ty) < hy + thy):
                return False
        self.taken.append((x, y, hx, hy))
        return True


# --------------------------------------------------------------------------

def building(asset_id, key, width, depth, floors, floor_h=3.4,
             setbacks=(), pilasters=4, balcony_rows=(), cornice=True,
             ground_accent=True, base_color=None, trim=None,
             win_pitch=3.05, bal_pitch=4.6,
             lit_mod=11, lit_thresh=4, upper_floor=0, clutter=True,
             rear_recess=False, ac_units=3, crown="auto"):
    """Assemble one Art Deco building.

    ``setbacks`` is a sequence of ``(height_above_ground, scale)`` -- above the
    given height the footprint shrinks to ``scale`` of its original half-extent,
    which produces the classic stepped tower silhouette.  These now cut the
    SHAFT into real bands; previously they moved the trim but left the wall
    full width, which stranded the upper-floor glazing inside solid geometry.

    ``upper_floor`` is where the lighter upper-storey colour zone starts.  It
    is DERIVED from the floor count when left at 0 -- a 9-storey tower used to
    get no upper mass at all simply because nobody remembered to pass it, and
    that is what left the top half of most towers a single dead colour field.

    ``crown`` selects the mechanical top treatment: ``"louvre"`` (five deep
    slots), ``"vents"`` (three shallow ones) or ``"none"``.  ``"auto"`` picks
    ``"louvre"`` for a tall tower and ``"vents"`` for a low one, so the twelve
    variants do not all ship the same belt.

    The remaining keyword arguments tune detail density.  They exist so the
    category can spend its triangle budget where it buys the most, and so a
    12-storey tower and a 2-storey motel do not ship the same amount of
    geometry for the same silhouette.
    """
    a = C.Asset(asset_id, "building")
    wall = base_color or PALETTE[key]
    trim_c = trim or TRIM

    # Derive the upper-storey start from the actual floor count.  Two thirds
    # of the way up is the classic Deco division: a base block in the main
    # colour, a lighter tower above it.  Before this, 10 of the 12 variants
    # passed upper_floor=0 and got no zone at all.
    if upper_floor <= 0 and floors >= 4:
        upper_floor = max(2, int(round(floors * 0.66)))
    if upper_floor >= floors:
        upper_floor = max(1, floors - 1)
    if crown == "auto":
        crown = "louvre" if floors >= 6 else "vents"

    def scale_at(z):
        """Footprint scale at height z, after any setbacks."""
        s = 1.0
        for h, sc in setbacks:
            if z >= h:
                s = min(s, sc)
        return s

    def top_scale():
        s = 1.0
        for _h, sc in setbacks:
            s = min(s, sc)
        return s

    total_h = floors * floor_h

    # Height of each setback, ascending, with a 0.0 base.  These are the
    # boundaries of the tower's storey BANDS.
    band_starts = [0.0] + sorted(h for (h, _sc) in setbacks)

    def band_of(z):
        """Index of the band containing z."""
        i = 0
        for k, h in enumerate(band_starts):
            if z >= h - 1e-9:
                i = k
        return i

    # ---- main shaft ------------------------------------------------------
    # WHY ONE BOX PER BAND, NOT ONE BOX FOR THE WHOLE TOWER.
    #
    # The shaft used to be a single ``chamfer_box(width, depth, total_h)`` --
    # full footprint, full height, with ``setbacks`` applied to nothing.  Every
    # other part (windows, string courses, balconies) IS placed at
    # ``scale_at(z)``, so above the first setback the windows were emitted at
    # the inset while the wall they were supposed to sit in never moved.  The
    # glazing was therefore sealed inside solid geometry, and the visible wall
    # above the setback was one full-width dead field.  A +Y ray cast from the
    # street measured exactly that: on bldg_deco_teal only 4 of 9 floors had
    # any visible glazing, and the first floor that lost it was the first
    # floor above the first setback -- for all six setback variants, and for
    # none of the three that declare none.
    #
    # Emitting a chamfered box per band makes the ziggurat real geometry, so
    # the upper-floor glazing lands on a wall that is actually there.
    shaft = C.Part("shaft", base_color=wall)
    for bi, z_lo in enumerate(band_starts):
        z_hi = band_starts[bi + 1] if bi + 1 < len(band_starts) \
            else total_h
        if z_hi - z_lo < 1e-6:
            continue
        s = scale_at(z_lo + 1e-4)
        bw, bd = width * s, depth * s
        # Each upper band overlaps the one below by 1 cm at its BASE, so the
        # step reads as a solid shoulder instead of two boxes meeting in a
        # plane.  The overlap is added at the bottom, never the top: adding
        # it at the bottom would push the ground band 5 mm below z = 0 and
        # the verifier's ground-resting check would fail the whole asset.
        z_bot = z_lo - 0.01 if bi > 0 else z_lo
        # Part names must be UNIQUE within an asset: the verifier rejects a
        # duplicate, and a renderer binding materials by name would collide.
        seg = C.Part("shaft_band%d" % bi, base_color=wall) \
            if bi > 0 else shaft
        C.chamfer_box(seg.mesh, (bw, bd, z_hi - z_bot),
                      center=(0, 0, (z_hi + z_bot) * 0.5), color=wall,
                      bevel=0.18, colors={"+z": ROOF})
        if bi > 0:
            a.add(seg)
    if shaft.mesh.faces:
        a.add(shaft)

    # Lighter upper storey block on the tall variants: a colour zone, not
    # geometry.  It is now emitted PER BAND and at that band's own setback
    # scale, so the lighter mass actually wraps the wall that exists at that
    # height.  Previously it was one full-base-footprint slab from
    # ``upper_floor`` to the roof, which on a ziggurat tower stood proud of the
    # narrower upper bands like a collar around a chimney -- and, where the
    # bands were full width, it buried the crown outright.
    if upper_floor > 0:
        up = C.Part("upper_walls", base_color=_shrub(wall, 1.09))
        z_start = upper_floor * floor_h
        for bi, z_lo in enumerate(band_starts):
            z_hi = band_starts[bi + 1] if bi + 1 < len(band_starts) \
                else total_h
            if z_hi <= z_start or z_lo >= total_h - 0.5:
                continue
            s = scale_at(z_lo + 1e-4)
            # Wrap this band only, and stop 1 cm short of its top so the
            # band above (a different colour zone) is not overlapped.
            z0 = max(z_lo, z_start)
            if z_hi - z0 < 0.5:
                continue
            C.chamfer_box(up.mesh,
                          (width * s + 0.05, depth * s + 0.05, z_hi - z0),
                          center=(0, 0, (z_hi + z0) * 0.5),
                          color=_shrub(wall, 1.09), bevel=0.05,
                          colors={"+z": ROOF})
        if up.mesh.faces:
            a.add(up)

    # vertical pilasters -- the signature Deco fluting.
    # All pilasters share ONE part: they are the same colour and material, and
    # one part keeps the part count sane on a 12-storey facade.  They are
    # emitted PER BAND at that band's own width, because a full-height
    # full-width pilaster on a ziggurat tower hangs out past the setback as a
    # row of floating fins with nothing behind them.
    pil = C.Part("pilasters", base_color=_shrub(wall, 1.06))
    for bi, z_lo in enumerate(band_starts):
        z_hi = band_starts[bi + 1] if bi + 1 < len(band_starts) \
            else total_h
        if z_hi - z_lo < 0.5:
            continue
        s = scale_at(z_lo + 1e-4)
        bw, bd = width * s, depth * s
        band_h = z_hi - z_lo
        for i in range(pilasters):
            t = (i + 0.5) / pilasters - 0.5          # -0.5 .. +0.5
            x = t * bw * 0.94
            w = bw * 0.030
            C.chamfer_box(pil.mesh, (w, bd + 0.16, band_h - 0.06),
                          center=(x, 0, (z_hi + z_lo) * 0.5),
                          color=_shrub(wall, 1.06), bevel=0.035)
    a.add(pil)

    # horizontal string courses every floor (one shared part).  The lighter
    # top face and darker soffit are free face_colors; the chamfer is what
    # actually makes the band read from a distance.
    courses = C.Part("courses", base_color=trim_c)
    for f in range(1, floors):
        z = f * floor_h
        s = scale_at(z)
        w2, d2 = width * s, depth * s
        C.chamfer_box(courses.mesh, (w2 + 0.18, d2 + 0.18, 0.18),
                      center=(0, 0, z), color=trim_c, bevel=0.05,
                      colors={"+z": _shrub(trim_c, 1.06),
                              "-z": _shrub(trim_c, 0.82)})
    if courses.mesh.faces:
        a.add(courses)

    # ---- setback shoulders: the exposed roof of each lower step ----------
    setback_slab = C.Part("setback_slabs", base_color=CONCRETE)
    terrace_rail = C.Part("terrace_rails", base_color=CONCRETE)
    for h, s in sorted(setbacks):
        prev = 1.0
        for h2, s2 in sorted(setbacks):
            if h2 < h:
                prev = s2
        if prev <= s:
            continue
        w_out, d_out = width * prev, depth * prev
        w_in, d_in = width * s, depth * s
        # The slab caps the LOWER step, so it must span the FULL OUTER
        # footprint -- not the average of the outer and inner footprints.
        # Averaging left the terrace roof suspended in the middle of the wall
        # instead of reading as the exposed roof of the storey below.
        slab_w, slab_d = w_out, d_out
        C.chamfer_box(setback_slab.mesh, (slab_w, slab_d, 0.24),
                      center=(0, 0, h - 0.12), color=CONCRETE, bevel=0.06,
                      colors={"+z": ROOF_DK, "-z": CONCRETE_DK})
        # a small parapet wall around the exposed terrace
        for sgn in (-1, 1):
            C.box(terrace_rail.mesh, (w_out, 0.18, 0.52),
                  center=(0, sgn * (d_out * 0.5 - 0.09), h + 0.26),
                  color=CONCRETE, colors={"+z": _shrub(CONCRETE, 1.12)})
            C.box(terrace_rail.mesh, (0.18, d_out - 0.36, 0.52),
                  center=(sgn * (w_out * 0.5 - 0.09), 0, h + 0.26),
                  color=CONCRETE, colors={"+z": _shrub(CONCRETE, 1.12)})

    if setback_slab.mesh.faces:
        a.add(setback_slab)
        a.add(terrace_rail)

    # ---- windows: real recesses, not decals -----------------------------
    # Glass lives in one of two parts so the lit subset can carry an emissive
    # and glow at night; the frame ring, the sills and the lintels are three
    # more, each an open shell with a single declared outward direction.
    windows = C.Part("windows", base_color=GLASS, roughness=0.25)
    window_lit = C.Part("window_lit", base_color=GLASS_LIT, roughness=0.25,
                        emissive=(0.90, 0.74, 0.40))
    # An annulus is a single-plane open shell, so each side needs its own
    # Part: one reference direction cannot describe a ring facing -Y and a
    # ring facing +Y in the same part.
    frames = C.Part("window_frames", base_color=trim_c,
                    outward=("dir", (0.0, -1.0, 0.0)))
    frames_rear = C.Part("window_rear_frames", base_color=trim_c,
                         outward=("dir", (0.0, 1.0, 0.0)))
    sills = C.Part("window_sills", base_color=trim_c,
                   outward=("dir", (0.0, 0.0, 1.0)))
    lintels = C.Part("window_lintels", base_color=_shrub(trim_c, 0.86),
                     outward=("dir", (0.0, 0.0, -1.0)))
    windows_side = C.Part("windows_side", base_color=GLASS, roughness=0.25)
    window_side_lit = C.Part("window_side_lit", base_color=GLASS_LIT,
                             roughness=0.25, emissive=(0.90, 0.74, 0.40))
    # 东/西两端面此前只有一块裸玻璃:正面有框、窗台、过梁,侧面什么都没有,
    # 于是侧立面在街上读成一块空板。侧框朝 +X/-X,故单列一组。
    # 两个端面朝向相反,而一个 Part 只允许一个 outward 方向,所以东西两面
    # 各要一组框;东西两端共用窗台/过梁方向(都朝上/下)。
    frames_side_e = C.Part("window_side_frames_e", base_color=trim_c,
                           outward=("dir", (1.0, 0.0, 0.0)))
    frames_side_w = C.Part("window_side_frames_w", base_color=trim_c,
                           outward=("dir", (-1.0, 0.0, 0.0)))
    sills_side = C.Part("window_side_sills", base_color=trim_c,
                        outward=("dir", (0.0, 0.0, 1.0)))
    lintels_side = C.Part("window_side_lintels", base_color=_shrub(trim_c, 0.86),
                          outward=("dir", (0.0, 0.0, -1.0)))

    ww, wh = 1.10, 1.62
    fw = 0.13                                    # frame border width
    for f in range(floors):
        z = f * floor_h + floor_h * 0.5
        s = scale_at(z)
        w2, d2 = width * s, depth * s
        cols = max(2, int(round(w2 * 0.84 / win_pitch)))
        for cidx in range(cols):
            t = (cidx + 0.5) / cols - 0.5
            x = t * w2 * 0.84
            # south (front, -Y) and north (+Y) facades.  Every window of an
            # asset shares ONE of the five parts above; per-instance parts
            # would all share a name and collide when a renderer binds
            # materials by name.
            for fi, sgn in enumerate((-1, 1)):
                y_wall = sgn * d2 * 0.5
                y_f = y_wall + sgn * FRAME_PROUD
                # The glass must sit BETWEEN the wall face and the frame ring,
                # i.e. OUTBOARD of the solid shaft but behind the proud frame.
                #
                # Sign: `sgn` is -1 for the street facade, where "further out"
                # means MORE negative y.  So standing the pane off the wall is
                # `+sgn * GLASS_SET`.  The mirror-image spelling (`-sgn`) drove
                # the pane to y = -4.485 when the solid shaft already extends to
                # -4.500 -- burying the entire glazing inside the wall, where
                # the raycast probe measured 0.0% visibility and deleting every
                # glass part changed 0.2-0.6% of pixels.  A negative sign here
                # does not make a subtle recess; it makes the windows not exist.
                y_g = y_wall + sgn * GLASS_SET
                lit = _lit_at(f, cidx, fi, lit_mod, lit_thresh)
                # glass: a solid panel whose OUTER face is GLASS_SET proud of
                # the wall, i.e. FRAME_PROUD - GLASS_SET behind the frame ring.
                gp = window_lit if lit else windows
                gc = GLASS_LIT if lit else GLASS
                # The 0.12 m pane body runs INWARD from that outer face
                # (direction -sgn, toward the building centre), so its centre
                # is y_g - sgn * 0.06 and it ends up seated in a ~20 mm rebate
                # rather than floating free of the wall.
                C.box(gp.mesh, (ww, 0.12, wh),
                      center=(x, y_g - sgn * 0.06, z), color=gc)
                if sgn > 0 and not rear_recess:
                    continue
                ring = frames_rear if sgn > 0 else frames
                _annulus(ring.mesh, y_f, x - ww * 0.5, x + ww * 0.5,
                         z - wh * 0.5, z + wh * 0.5, fw, fw, trim_c,
                         flip=(sgn > 0))
                _reveal(sills.mesh, x - ww * 0.5, x + ww * 0.5, z - wh * 0.5,
                        y_f, y_g, trim_c, up=True)
                _reveal(lintels.mesh, x - ww * 0.5, x + ww * 0.5,
                        z + wh * 0.5, y_f, y_g, _shrub(trim_c, 0.86), up=False)
            # east/west ends -- one recessed light per storey per end, now
            # framed like the front: a 20 mm rebate, a surround ring, a sill
            # and a lintel.  `_annulus` and `_reveal` are written for the
            # X/Z plane facing +Y, so pass the end-wall axes swapped and the
            # face inset already resolved by the caller.
            # 端面此前每层只有一扇固定位置的窗,无论楼有多深 —— 沿街看过去
            # 侧面就是一大片只开一个小洞的墙。现在按深度排一整排,间距与
            # 正面一致,侧立面才有和正面相同的节奏。
            for fi2, sx in enumerate((-1, 1)):
                ring_m = frames_side_e if sx > 0 else frames_side_w
                side_cols: int = max(1, int(round(d2 * 0.82 / win_pitch)))
                for sidx in range(side_cols):
                    lit = _lit_at(f, 3 + fi2, sx, lit_mod, lit_thresh)
                    sp = window_side_lit if lit else windows_side
                    sc = GLASS_LIT if lit else GLASS
                    sw, sh = 0.90, 1.42
                    y_end: float = ((sidx + 0.5) / side_cols - 0.5) * d2 * 0.82
                    x_wall: float = sx * (w2 * 0.5 - 0.02)
                    C.box(sp.mesh, (0.12, sw, sh),
                          center=(x_wall, y_end, z), color=sc)
                    # ring sits just outboard of the glass, facing +/-X
                    _annulus_side(ring_m.mesh, x_wall + sx * 0.06,
                                  z - sh * 0.5, z + sh * 0.5,
                                  y_end - sw * 0.5, y_end + sw * 0.5,
                                  fw, fw, trim_c, flip=(sx < 0))
                    # sill below and lintel above, spanning the reveal depth
                    depth_a: float = x_wall
                    depth_b: float = x_wall - sx * 0.12
                    _reveal_side(sills_side.mesh, z - sh * 0.5,
                                 y_end - sw * 0.5, y_end + sw * 0.5,
                                 depth_a, depth_b, trim_c, up=True)
                    _reveal_side(lintels_side.mesh, z + sh * 0.5,
                                 y_end - sw * 0.5, y_end + sw * 0.5,
                                 depth_a, depth_b,
                                 _shrub(trim_c, 0.86), up=False)

    for p in (windows, window_lit, frames, frames_rear, sills, lintels,
              windows_side, window_side_lit,
              frames_side_e, frames_side_w, sills_side, lintels_side):
        if p.mesh.faces:
            a.add(p)

    # ---- balcony parapets ------------------------------------------------
    if balcony_rows:
        balconies = C.Part("balconies", base_color=trim_c)
        rail_dk = _shrub(trim_c, 0.80)
        for f in balcony_rows:
            z0 = f * floor_h + floor_h * 0.18
            s = scale_at(z0)
            w2, d2 = width * s, depth * s
            n_bal = max(1, int(round(w2 * 0.84 / bal_pitch)))
            for i in range(n_bal):
                t = (i + 0.5) / n_bal - 0.5
                x = t * w2 * 0.84
                y = -(d2 * 0.5 + 0.44)
                # slab with a darker soffit so the ledge reads from below
                C.box(balconies.mesh, (2.2, 0.92, 0.12),
                      center=(x, y, z0), color=trim_c,
                      colors={"-z": _shrub(trim_c, 0.58),
                              "+z": _shrub(trim_c, 0.92)})
                # low solid parapet
                C.box(balconies.mesh, (2.2, 0.11, 0.62),
                      center=(x, y - 0.405, z0 + 0.37), color=trim_c,
                      colors={"-z": rail_dk, "+z": _shrub(trim_c, 1.05)})
                for sx in (-1, 1):
                    C.box(balconies.mesh, (0.11, 0.92, 0.62),
                          center=(x + sx * 1.045, y, z0 + 0.37), color=trim_c,
                          colors={"-z": rail_dk, "+z": _shrub(trim_c, 1.05)})
        a.add(balconies)

    # ---- mechanical crown: louvre band + rooftop plant -------------------
    # WHY.  Once the setbacks are real, the tower narrows near the top and the
    # last few floors are the part a player sees against the sky.  Left alone
    # they are just more of the same wall colour -- the "large, uninterrupted
    # volume" complaint again, one band higher up.  A service zone reads
    # differently from a habitable storey precisely because it is not the same
    # thing: narrow horizontal louvres instead of windows, plus plant on the
    # roof.  Both survive at gameplay distance, which a 22 cm window reveal
    # does not.
    #
    # The treatment is a CHOICE per asset (see ``crown``), not one band stamped
    # on everything: 14 identical mechanical belts would be its own kind of
    # bland.
    #
    # ANCHORED TO THE TOP OF THE TOWER.  A mechanical penthouse is, by
    # definition, the top of the building -- so the band goes on the last
    # floors, not at the ``upper_floor`` colour break two thirds of the way
    # up.  (The first attempt put it there and left the genuinely topmost
    # storey a bare wall with the sign board on it, which is the exact
    # complaint one band lower.)
    mech_h = 0.34 if crown == "louvre" else 0.26
    louvre_rows = 5 if crown == "louvre" else 3
    mech_floor = max(2, floors - 2)          # first floor of the crown zone
    me_z0 = max(0.0, mech_floor * floor_h)
    # The louvre band sits on the lowest floors that are still habitable --
    # INSIDE the fenestrated zone, so it replaces window rows rather than
    # adding a band to an otherwise dead upper shaft.
    if crown != "none" and floors >= 4 and mech_floor >= 1:
        # The crown zone runs from the first crown floor all the way to just
        # under the cornice, so the top storeys are a service band rather
        # than the same wall colour one more time.
        #
        # PER-HEIGHT PLACEMENT, NOT ONE SCALE FOR THE WHOLE BAND.  A setback
        # can cut the band in half: placing every blade at the narrowest
        # scale the band touches drove them 2.3 m INSIDE the wider lower
        # storey (measured: blades at y=-2.20 where the wall was at -4.50),
        # i.e. sealed in solid geometry and invisible.  Each blade is placed
        # at the scale of its OWN height, so it always sits on the wall that
        # exists there.
        zc0 = mech_floor * floor_h
        zc1 = total_h - 0.9
        if zc1 - zc0 > 1.0:
            louvre = C.Part("mech_louvres", base_color=METAL_DK,
                            metallic=0.35)
            slot = C.Part("mech_louvre_slots", base_color=GLASS,
                          roughness=0.3)
            pitch = (zc1 - zc0) / float(louvre_rows + 1)
            for r in range(louvre_rows):
                zl = zc0 + pitch * (r + 0.5)
                s_r = scale_at(zl)
                bw, bd = width * s_r, depth * s_r
                # front and back: a recessed dark slot with a proud blade,
                # i.e. the same stand-off trick as a window frame but 10x
                # wider and 6x shorter, so it reads as a slot not a window
                for sgn in (-1, 1):
                    y_wall = sgn * bd * 0.5
                    C.box(louvre.mesh, (bw * 0.80, 0.26, mech_h),
                          center=(0, y_wall + sgn * 0.09, zl), color=METAL_DK,
                          colors={"+z": _shrub(METAL_DK, 1.22)})
                    C.box(slot.mesh, (bw * 0.76, 0.10, mech_h * 0.42),
                          center=(0, y_wall + sgn * 0.17, zl), color=GLASS)
                # the two ends, narrower
                for sx in (-1, 1):
                    x_wall = sx * bw * 0.5
                    C.box(louvre.mesh, (0.26, bd * 0.76, mech_h),
                          center=(x_wall + sx * 0.09, 0, zl), color=METAL_DK,
                          colors={"+z": _shrub(METAL_DK, 1.22)})
                    C.box(slot.mesh, (0.10, bd * 0.72, mech_h * 0.42),
                          center=(x_wall + sx * 0.17, 0, zl), color=GLASS)
            a.add(louvre)
            a.add(slot)

            # a service floor band capping the louvre zone, sized to the
            # wall at its OWN height
            band2 = C.Part("mech_band_cap", base_color=_shrub(trim_c, 0.72))
            s_cap = scale_at(zc1)
            C.chamfer_box(band2.mesh, (width * s_cap + 0.26,
                                      depth * s_cap + 0.26, 0.26),
                          center=(0, 0, zc1 + 0.13), color=_shrub(trim_c, 0.72),
                          bevel=0.05,
                          colors={"+z": _shrub(trim_c, 0.92)})
            a.add(band2)

    # ---- ground floor: dark plinth, shopfront band, real entrance -------
    if ground_accent:
        # A 1.2 m plinth in a much darker shade of the wall.  Free: one part,
        # one chamfered box, and it stops the base of the building from being
        # the same value as the top of the building.
        band_h = 1.20
        plinth_c = _shrub(wall, 0.58)
        band = C.Part("ground_band", base_color=plinth_c)
        C.chamfer_box(band.mesh, (width + 0.12, depth + 0.12, band_h),
                      center=(0, 0, band_h * 0.5), color=plinth_c, bevel=0.05,
                      colors={"+z": _shrub(plinth_c, 1.18)})
        a.add(band)

        y_wall = -depth * 0.5

        # entrance: a dark doorway set back inside a trim surround, reached by
        # two steps, under a canopy on two posts.
        ent = C.Part("entrance", base_color=DOOR)
        C.box(ent.mesh, (1.52, 0.16, 2.46),
              center=(0, y_wall - 0.07, 1.23), color=DOOR)
        for sx in (-1, 1):
            C.box(ent.mesh, (0.24, 0.30, 2.82),
                  center=(sx * 0.88, y_wall - 0.15, 1.41), color=trim_c,
                  colors={"-z": _shrub(trim_c, 0.78)})
        C.box(ent.mesh, (2.00, 0.30, 0.32),
              center=(0, y_wall - 0.15, 2.66), color=trim_c,
              colors={"-z": _shrub(trim_c, 0.70), "+z": _shrub(trim_c, 1.06)})
        # steps -- the lowest one's bottom face is exactly on z = 0
        C.box(ent.mesh, (2.90, 1.05, 0.17),
              center=(0, y_wall - 0.55, 0.085), color=CONCRETE,
              colors={"+z": _shrub(CONCRETE, 1.12)})
        C.box(ent.mesh, (2.40, 0.55, 0.34),
              center=(0, y_wall - 0.30, 0.17), color=CONCRETE,
              colors={"+z": _shrub(CONCRETE, 1.12)})
        a.add(ent)

        # canopy on two posts (this is the part the engine used to hang the
        # neon sign from, so the name stays).
        aw = C.Part("awning", base_color=AWNING)
        C.chamfer_box(aw.mesh, (3.40, 1.50, 0.16),
                      center=(0, y_wall - 0.85, 2.95), color=AWNING,
                      bevel=0.05, colors={"-z": _shrub(AWNING, 0.70)})
        C.box(aw.mesh, (3.40, 0.10, 0.34),
              center=(0, y_wall - 1.58, 2.76), color=_shrub(AWNING, 0.86))
        for sx in (-1, 1):
            C.box(aw.mesh, (0.13, 0.13, 2.88),
                  center=(sx * 1.48, y_wall - 1.42, 1.44), color=TRIM,
                  colors={"-z": _shrub(TRIM, 0.70)})
        a.add(aw)

    # ---- roof: cornice, deck, parapet + coping, tank, AC, clutter --------
    rw, rd = width * top_scale(), depth * top_scale()
    cornice_top = total_h

    if cornice:
        cor = C.Part("cornice", base_color=trim_c)
        C.chamfer_box(cor.mesh, (rw + 0.40, rd + 0.40, 0.22),
                      center=(0, 0, total_h + 0.11), color=trim_c, bevel=0.06,
                      colors={"+z": ROOF})
        a.add(cor)
        cornice_top = total_h + 0.22

    # roof deck: a raised dark pad.  Every top-facing horizontal surface on
    # the building -- this, the setback slab tops and the shaft cap -- is
    # concrete grey, several stops darker than any painted wall.
    deck = C.Part("roof_deck", base_color=ROOF_DK)
    C.chamfer_box(deck.mesh, (rw + 0.16, rd + 0.16, 0.14),
                  center=(0, 0, cornice_top + 0.07), color=ROOF_DK, bevel=0.05)
    a.add(deck)
    deck_top = cornice_top + 0.14

    par = C.Part("parapet", base_color=trim_c)
    ph = 0.66
    for sgn in (-1, 1):
        C.box(par.mesh, (rw + 0.28, 0.18, ph),
              center=(0, sgn * (rd * 0.5 + 0.05), cornice_top + ph * 0.5),
              color=trim_c)
        C.box(par.mesh, (0.18, rd - 0.24, ph),
              center=(sgn * (rw * 0.5 + 0.05), 0, cornice_top + ph * 0.5),
              color=trim_c)
    a.add(par)

    # two-tone roofline: a lighter coping cap over the darker parapet so the
    # silhouette has a bright edge instead of stopping dead.
    cap = C.Part("parapet_cap", base_color=_shrub(TRIM, 1.0))
    cap_c = _shrub(trim_c, 1.04)
    for sgn in (-1, 1):
        C.chamfer_box(cap.mesh, (rw + 0.40, 0.30, 0.12),
                      center=(0, sgn * (rd * 0.5 + 0.05),
                              cornice_top + ph + 0.06),
                      color=cap_c, bevel=0.04)
        C.chamfer_box(cap.mesh, (0.30, rd - 0.02, 0.12),
                      center=(sgn * (rw * 0.5 + 0.05), 0,
                              cornice_top + ph + 0.06),
                      color=cap_c, bevel=0.04)
    a.add(cap)

    # rooftop water tank on a steel frame, banded stave by stave
    tank_r = min(rw, rd) * 0.21
    tx = rw * 0.22
    ty = rd * 0.15
    tz = deck_top
    legs = C.Part("tank_legs", base_color=METAL_DK, metallic=0.5)
    for sx in (-1, 1):
        for sy in (-1, 1):
            C.box(legs.mesh, (0.11, 0.11, 0.92),
                  center=(tx + sx * tank_r * 0.60, ty + sy * tank_r * 0.60,
                          tz + 0.46), color=METAL_DK,
                  colors={"+z": _shrub(METAL_DK, 1.30)})
    a.add(legs)

    tank = C.Part("water_tank", base_color=TANK_A, roughness=0.85)
    _banded_cylinder(tank.mesh, tank_r, 1.42, 10,
                     (tx, ty, tz + 1.63), TANK_A, TANK_B)
    a.add(tank)

    tank_lid = C.Part("water_tank_lid", base_color=(0.48, 0.40, 0.32),
                      roughness=0.85)
    C.cone(tank_lid.mesh, tank_r * 1.06, 0.32, 10,
           center=(tx, ty, tz + 2.50), color=(0.48, 0.40, 0.32))
    a.add(tank_lid)

    # rooftop AC condensers -- chamfered so the light breaks on their edges
    acs = C.Part("ac_units", base_color=METAL)
    n_ac = ac_units if rw > 7.0 else max(1, ac_units - 1)
    for i in range(n_ac):
        ax = -rw * 0.28 + i * rw * 0.30
        ay = -rd * 0.28
        C.chamfer_box(acs.mesh, (1.15, 0.95, 0.80),
                      center=(ax, ay, tz + 0.40), color=METAL, bevel=0.05,
                      colors={"+z": _shrub(METAL, 0.80)})
        C.cylinder(acs.mesh, 0.30, 0.10, 6, center=(ax, ay, tz + 0.85),
                   color=METAL_DK)
    a.add(acs)

    # ---- rooftop vent bank ----------------------------------------------
    # A mechanical crown needs something on the roof to justify the louvre
    # band under it.  Curbs + a flared cone cap, the way real roof plant is
    # built, and all of it is silhouette against the sky -- the part of a
    # tower a player actually reads from a block away.  Capped at 3 units so
    # the slimmest setback roof does not turn into a forest of pipes.
    if crown != "none" and rw > 3.5:
        curb = C.Part("vent_curbs", base_color=CONCRETE_DK)
        stack = C.Part("vent_stacks", base_color=METAL)
        cowl = C.Part("vent_cowls", base_color=METAL_DK, metallic=0.45)
        n_vent = 3 if rw > 8.0 else 2
        for i in range(n_vent):
            vx = (-rw * 0.16 + i * rw * 0.30) if n_vent > 1 else 0.0
            vy = rd * 0.30
            vh = 0.95 + 0.28 * (i % 2)
            vr = 0.26 if rw > 6.0 else 0.21
            C.chamfer_box(curb.mesh, (vr * 3.0, vr * 3.0, 0.26),
                          center=(vx, vy, tz + 0.13), color=CONCRETE_DK,
                          bevel=0.04, colors={"+z": _shrub(CONCRETE_DK, 1.18)})
            C.cylinder(stack.mesh, vr, vh, 8,
                       center=(vx, vy, tz + 0.26 + vh * 0.5), color=METAL)
            # flared cap: a truncated cone, so it is a solid of revolution
            # rather than a floating disc
            C.cone(cowl.mesh, vr * 1.55, 0.26, 8,
                   center=(vx, vy, tz + 0.26 + vh + 0.13),
                   color=METAL_DK, radius_top=vr * 1.55)
        a.add(curb)
        a.add(stack)
        a.add(cowl)

    if clutter:
        # Reserve the footprints the fixed roof furniture already owns, then
        # place the loose clutter into whatever is left.  Candidates that do
        # not fit are dropped rather than pushed into a neighbour.
        slots = _Slots()
        for (px, py, phx, phy) in ((rw * 0.22, rd * 0.14, tank_r * 1.2,
                                    tank_r * 1.2),
                                   (-rw * 0.28, -rd * 0.28, 0.60, 0.48),
                                   (-rw * 0.28 + rw * 0.60, -rd * 0.28,
                                    0.60, 0.48),
                                   (-rw * 0.28 + rw * 1.20, -rd * 0.28,
                                    0.60, 0.48)):
            slots.taken.append((px, py, phx, phy))

        # stair bulkhead / roof access hut -- the tall silhouette element
        hx, hy = -rw * 0.22, rd * 0.16
        if slots.free(hx, hy, 1.16, 0.94):
            hut = C.Part("roof_hut", base_color=_shrub(wall, 0.80))
            C.chamfer_box(hut.mesh, (2.20, 1.80, 2.05),
                          center=(hx, hy, deck_top + 1.03),
                          color=_shrub(wall, 0.80), bevel=0.06, colors={"+z": ROOF})
            a.add(hut)
            hut_cap = C.Part("roof_hut_cap", base_color=trim_c)
            C.chamfer_box(hut_cap.mesh, (2.40, 2.00, 0.14),
                          center=(hx, hy, deck_top + 2.12), color=trim_c, bevel=0.04)
            a.add(hut_cap)

        # vent stack: a short pipe with a cowl
        vx, vy = -rw * 0.36, rd * 0.40
        if slots.free(vx, vy, 0.28, 0.28):
            vents = C.Part("roof_vents", base_color=METAL_DK, metallic=0.4)
            C.cylinder(vents.mesh, 0.17, 0.92, 6, center=(vx, vy, deck_top + 0.46),
                       color=METAL_DK)
            C.cylinder(vents.mesh, 0.26, 0.10, 6, center=(vx, vy, deck_top + 0.97),
                       color=_shrub(METAL_DK, 1.25))
            a.add(vents)

        # two TV aerials
        aer = C.Part("roof_aerials", base_color=METAL, metallic=0.6)
        for (ax, ay, ah) in ((rw * 0.36, -rd * 0.40, 1.85),
                             (rw * 0.40, -rd * 0.26, 1.35)):
            if not slots.free(ax, ay, 0.34, 0.34):
                continue
            C.cylinder(aer.mesh, 0.035, ah, 4,
                       center=(ax, ay, deck_top + ah * 0.5), color=METAL)
            C.box(aer.mesh, (0.05, 0.62, 0.05),
                  center=(ax, ay, deck_top + ah * 0.84), color=METAL)
        if aer.mesh.faces:
            a.add(aer)

        # satellite dishes on short masts
        dish = C.Part("roof_dishes", base_color=_shrub(TRIM, 0.92))
        for (dx, dy) in ((-rw * 0.42, -rd * 0.18), (rw * 0.04, rd * 0.40)):
            if not slots.free(dx, dy, 0.46, 0.46):
                continue
            C.box(dish.mesh, (0.10, 0.10, 0.34),
                  center=(dx, dy, deck_top + 0.17), color=METAL_DK)
            C.cone(dish.mesh, 0.44, 0.13, 7, center=(dx, dy, deck_top + 0.42),
                   color=_shrub(TRIM, 0.92), radius_top=0.13)
        if dish.mesh.faces:
            a.add(dish)

    # blank neon sign backing board on the front facade (signs attach to this).
    # Mounted on whichever setback band contains this height, so on a ziggurat
    # the board lands on the wall it is actually in front of.  It is held BELOW
    # the mechanical crown: a 4 m blank board across the service band hides the
    # louvres on the storeys the player actually looks at.
    sb = C.Part("sign_board", base_color=METAL_DK, roughness=0.8)
    sb_z = total_h * 0.74
    if crown != "none":
        sb_z = min(sb_z, (mech_floor - 1) * floor_h - 1.6)
    sb_z = max(sb_z, 6.0)
    sbs = scale_at(sb_z)
    sb_w = min(width * sbs * 0.52, 4.2)
    sb_y = -(depth * sbs * 0.5 + 0.14)
    C.chamfer_box(sb.mesh, (sb_w, 0.24, 2.10), center=(0, sb_y, sb_z),
                  color=METAL_DK, bevel=0.05,
                  colors={"-y": _shrub(METAL_DK, 0.62)})
    for sgn in (-1, 1):
        C.box(sb.mesh, (0.10, 0.36, 0.10),
              center=(sgn * sb_w * 0.42, sb_y + 0.02, sb_z - 0.95),
              color=METAL_DK)
    a.add(sb)

    # ---- corner buttresses: vertical articulation on the upper mass ------
    # The audit measured buildings at 3.34x colour modulation against palms
    # at 34.8x -- the flattest major category -- because every large face
    # carried one value.  A corner buttress is the cheapest articulation that
    # works at gameplay distance: it changes the NORMAL on two vertical
    # edges and takes a distinctly darker value, so the tower corner reads as
    # a corner even when the facade is in shadow.
    if floors >= 4:
        bt = C.Part("corner_buttresses", base_color=_shrub(wall, 0.74))
        bz0 = upper_floor * floor_h
        s_b = scale_at(bz0 + 1e-4)
        bw2, bd2 = width * s_b, depth * s_b
        bt_h = total_h - bz0
        if bt_h > 1.5:
            for sx in (-1, 1):
                for sy in (-1, 1):
                    C.chamfer_box(
                        bt.mesh, (bw2 * 0.17, bd2 * 0.17, bt_h - 0.01),
                        center=(sx * bw2 * 0.415, sy * bd2 * 0.415,
                                bz0 + bt_h * 0.5),
                        color=_shrub(wall, 0.74), bevel=0.06,
                        colors={"+z": _shrub(wall, 0.88)})
            a.add(bt)
    return a


# --------------------------------------------------------------------------
# the 12 building variants
# --------------------------------------------------------------------------

def build_all():
    """Return the ordered list of building assets."""
    out = []

    # 1. classic pastel Deco hotel, 5 storeys, mild setback
    out.append(building(
        "bldg_deco_pink", "pink", 12.0, 10.0, 5,
        setbacks=((11.0, 0.78),), pilasters=5, balcony_rows=(1, 2, 3, 4),
        lit_mod=9, lit_thresh=2, upper_floor=3, bal_pitch=4.0,
        crown="vents"))

    # 2. tall teal tower with a pronounced ziggurat top
    out.append(building(
        "bldg_deco_teal", "teal", 11.0, 9.0, 9,
        setbacks=((14.0, 0.82), (23.0, 0.62), (27.0, 0.44)), pilasters=4,
        balcony_rows=(2, 4, 6), upper_floor=6, bal_pitch=4.0,
        crown="louvre"))

    # 3. wide cream apartment block, 4 storeys, no setback
    out.append(building(
        "bldg_cream_block", "cream", 18.0, 11.0, 4,
        pilasters=7, balcony_rows=(1, 2, 3), win_pitch=3.2, bal_pitch=3.6,
        lit_mod=13, lit_thresh=3, crown="vents"))

    # 4. mint corner shop, 3 storeys -- too short for a service zone
    out.append(building(
        "bldg_mint_shop", "mint", 10.0, 8.0, 3,
        pilasters=4, balcony_rows=(2,), win_pitch=2.9, bal_pitch=4.6,
        rear_recess=True, crown="none"))

    # 5. coral art-deco hall with a single setback and awning
    out.append(building(
        "bldg_coral_hall", "coral", 14.0, 12.0, 4,
        setbacks=((9.5, 0.80),), pilasters=5, balcony_rows=(1, 3),
        win_pitch=3.1, bal_pitch=4.4, rear_recess=True, crown="vents"))

    # 6. apricot low-rise motel block, 2 storeys -- the short end of the range
    out.append(building(
        "bldg_apricot_motel", "apricot", 16.0, 9.0, 2, floor_h=3.3,
        pilasters=6, balcony_rows=(1,), win_pitch=3.6, bal_pitch=7.0,
        lit_mod=9, lit_thresh=1, crown="none"))

    # 7. lilac slim high-rise, 12 storeys, double setback -- the tall end
    out.append(building(
        "bldg_lilac_tower", "lilac", 9.5, 8.5, 12, floor_h=3.3,
        setbacks=((20.0, 0.85), (32.0, 0.70)), pilasters=4,
        balcony_rows=(3, 5, 7, 9, 11), upper_floor=8, win_pitch=3.1,
        bal_pitch=4.2, lit_mod=11, lit_thresh=2, ac_units=2,
        crown="louvre"))

    # 8. aqua aquarium-style block with deep balconies
    out.append(building(
        "bldg_aqua_arcade", "aqua", 15.0, 12.0, 5,
        pilasters=6, balcony_rows=(1, 2, 3, 4), upper_floor=3,
        win_pitch=3.1, bal_pitch=3.8, rear_recess=True, crown="vents"))

    # 9. sand-coloured 6-storey mid-rise
    out.append(building(
        "bldg_sand_midrise", "sand", 13.0, 10.0, 6,
        setbacks=((13.0, 0.88),), pilasters=5, balcony_rows=(2, 3, 4, 5),
        upper_floor=4, win_pitch=3.0, bal_pitch=3.8, lit_mod=13, lit_thresh=3,
        crown="louvre"))

    # 10. white deco landmark with a ziggurat crown
    out.append(building(
        "bldg_white_landmark", "white", 12.5, 12.5, 10,
        setbacks=((16.0, 0.85), (26.0, 0.66), (31.0, 0.46)), pilasters=4,
        balcony_rows=(2, 4, 6, 8), upper_floor=7, win_pitch=3.2,
        bal_pitch=4.0, lit_mod=9, lit_thresh=2, crown="louvre"))

    # 11. pink twin-setback 7 storey
    out.append(building(
        "bldg_pink_terrace", "pink", 14.0, 10.0, 7,
        setbacks=((10.0, 0.86), (19.0, 0.68)), pilasters=5,
        balcony_rows=(1, 3, 5), upper_floor=5, win_pitch=3.1, bal_pitch=4.2,
        rear_recess=True, crown="louvre"))

    # 12. teal industrial-loft, 3 storeys, wide and shallow.  Width is held at
    # 17 m (8.5 m half-extent) so the whole asset stays clear of the engine's
    # 11.0 m footprint guard; the 20 m version was culled from the city.
    out.append(building(
        "bldg_teal_loft", "teal", 17.0, 9.0, 3,
        pilasters=7, balcony_rows=(2,), win_pitch=3.5, bal_pitch=6.6,
        lit_mod=7, lit_thresh=1, crown="none"))

    return out
