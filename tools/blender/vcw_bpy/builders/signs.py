"""3A neon sign builders — eight storefront / facade marquee signs.

Every sign here is *assembled*, not textured. A painted rectangle with a
bright map on it is the classic low-poly neon tell: it has no silhouette, no
thickness and no specular break-up. A 3A sign is built from five part families
and this module implements all five for every asset:

1. **`frame`** — a grounded foot, a mast and a cantilever arm, plus knee
   struts and panel brackets. A sign that floats in mid-air with no mount is
   the second-most-common tell after the razor edge.
2. **`panel`** — a bevelled backing plate, a near-black sign face standing
   proud of it, and a border rail that frames the face and carries the
   marquee lamps.
3. **`letterforms`** — real bent glass. Every stroke is a `P.neon_tube`
   emissive cylinder laid on a 3 x 5 stroke grid, so the glow has diameter,
   catches the environment and reads as tubing rather than as a decal.
4. **`crest`** — a per-venue neon pictogram above the panel (martini glass,
   coffee cup, pizza wedge, sun, crown, arrow, sparkle) so no two signs in the
   set share a silhouette.
5. **`lamps`** — marquee bulbs and, for the club, a hanging disco ball.

Why a stroke alphabet instead of a font: no font file means no missing-glyph
failure mode, every stroke lands on a single baseline, and the blocky
square-and-diagonal letterforms are exactly what a bender would make out of
one continuous length of glass. `to_track_quat` on the diagonal strokes gives
them a true arbitrary angle, so K/M/N/V/W/X/Y/Z are real glass and not
approximated by staircases.

ORIGINAL NAMES ONLY. Every venue name below is invented for this library and
every colour, pictogram and lettering scheme is drawn from maths in this file.
No real trademark, logo, brand name or trade dress is reproduced or referenced.

Coordinate contract: authored Z-up, +X right, -Y is the sign's FRONT, metres.
The panel is centred on X = 0 and the grounded foot's lowest face sits at
z = 0. The exporter rotates to Y-up with determinant +1.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy
from mathutils import Vector

from .. import kernel as K
from .. import palette as P

# ---------------------------------------------------------------------------
# sign-face kit dimensions (metres) — see module docstring
# ---------------------------------------------------------------------------

PANEL_T = 0.16                     # backing plate thickness
FACE_T = 0.05                      # sign face panel, stands proud on -Y
FACE_Y = -(PANEL_T * 0.5 + FACE_T * 0.5)    # centre of the face panel
FACE_FRONT = FACE_Y - FACE_T * 0.5          # -Y surface the tubes rest on

RAIL_T = 0.09                      # border rail section (square)
RAIL_D = 0.09
RAIL_Y = FACE_Y - 0.010            # rails sit slightly forward of the face
RAIL_FRONT = RAIL_Y - RAIL_D * 0.5

MAST_Y = 0.36                      # mast centre, behind the plate
MAST_S = 0.13
ARM_Y = 0.20
ARM_S = 0.13
FOOT_Z = 0.16
PAD = 0.22                         # panel padding around the lettering

# 3 x 5 stroke grid ------------------------------------------------------------
GLYPH_W = 3.0
GLYPH_H = 5.0
SPACE_W = 1.5

# Neon colours — an original Miami palette, chosen per venue.
HOT_PINK = (1.00, 0.16, 0.52)
ICE_CYAN = (0.14, 0.88, 1.00)
SODIUM = (1.00, 0.82, 0.18)
TANGERINE = (1.00, 0.44, 0.10)
DEEP_MAGENTA = (0.92, 0.10, 0.38)
LIME = (0.52, 1.00, 0.40)
AQUAMARINE = (0.16, 1.00, 0.78)
PLUM = (0.72, 0.26, 1.00)
COLD_WHITE = (0.94, 0.97, 1.00)

# ---------------------------------------------------------------------------
# stroke alphabet — (x0, z0, x1, z1) in grid cells, x in 0..3, z in 0..5
# Squared verticals + diagonals: this is what a glass bender makes from one
# continuous length of tube, so it reads as neon instead of as a font.
# ---------------------------------------------------------------------------

_ALPHABET: dict[str, tuple[tuple[float, float, float, float], ...]] = {
    "A": ((0, 0, 0, 5), (3, 0, 3, 5), (0, 5, 3, 5), (0, 2.5, 3, 2.5)),
    "B": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 2.5, 3, 2.5), (0, 0, 3, 0), (3, 0, 3, 5)),
    "C": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 0, 3, 0)),
    "D": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 0, 3, 0), (3, 0, 3, 5)),
    "E": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 2.5, 3, 2.5), (0, 0, 3, 0)),
    "F": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 2.5, 3, 2.5)),
    "G": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 0, 3, 0), (3, 0, 3, 2.5), (1.4, 2.5, 3, 2.5)),
    "H": ((0, 0, 0, 5), (3, 0, 3, 5), (0, 2.5, 3, 2.5)),
    "I": ((0, 5, 3, 5), (1.5, 0, 1.5, 5), (0, 0, 3, 0)),
    "J": ((0, 5, 3, 5), (3, 0, 3, 5), (0, 0, 3, 0)),
    "K": ((0, 0, 0, 5), (0, 2.5, 3, 5), (0, 2.5, 3, 0)),
    "L": ((0, 0, 0, 5), (0, 0, 3, 0)),
    "M": ((0, 0, 0, 5), (0, 5, 1.5, 2.6), (1.5, 2.6, 3, 5), (3, 0, 3, 5)),
    "N": ((0, 0, 0, 5), (0, 5, 3, 0), (3, 0, 3, 5)),
    "O": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 0, 3, 0), (3, 0, 3, 5)),
    "P": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 2.5, 3, 2.5), (3, 2.5, 3, 5)),
    "Q": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 0, 3, 0), (3, 0, 3, 5), (1.6, 1.2, 3.2, -0.4)),
    "R": ((0, 0, 0, 5), (0, 5, 3, 5), (0, 2.5, 3, 2.5), (3, 2.5, 3, 5), (1.5, 2.5, 3, 0)),
    "S": ((0, 5, 3, 5), (0, 2.5, 0, 5), (0, 2.5, 3, 2.5), (3, 0, 3, 2.5), (0, 0, 3, 0)),
    "T": ((0, 5, 3, 5), (1.5, 0, 1.5, 5)),
    "U": ((0, 0, 0, 5), (3, 0, 3, 5), (0, 0, 3, 0)),
    "V": ((0, 5, 0, 1.2), (0, 1.2, 1.5, 0), (1.5, 0, 3, 1.2), (3, 1.2, 3, 5)),
    "W": ((0, 5, 0, 0), (0, 0, 1.5, 3.2), (1.5, 3.2, 3, 0), (3, 0, 3, 5)),
    "X": ((0, 0, 0, 5), (3, 0, 3, 5)),
    "Y": ((0, 5, 1.5, 2.5), (3, 5, 1.5, 2.5), (1.5, 0, 1.5, 2.5)),
    "Z": ((0, 5, 3, 5), (3, 5, 0, 0), (0, 0, 3, 0)),
    "0": ((0, 0, 0, 5), (3, 0, 3, 5), (0, 5, 3, 5), (0, 0, 3, 0)),
    "1": ((0.6, 4.2, 1.5, 5), (1.5, 0, 1.5, 5), (0, 0, 3, 0)),
    "2": ((0, 5, 3, 5), (3, 2.5, 3, 5), (0, 2.5, 3, 2.5), (0, 0, 0, 2.5), (0, 0, 3, 0)),
    "3": ((0, 5, 3, 5), (3, 0, 3, 5), (0, 2.5, 3, 2.5)),
    "4": ((0, 1.4, 1.5, 3.6), (1.5, 0, 1.5, 5), (3, 0, 3, 5)),
    "5": ((0, 5, 3, 5), (0, 2.5, 3, 2.5), (3, 0, 3, 2.5), (0, 0, 3, 0)),
    "6": ((0, 5, 3, 5), (0, 2.5, 0, 5), (0, 2.5, 3, 2.5), (3, 0, 3, 2.5), (0, 0, 3, 0)),
    "7": ((0, 5, 3, 5), (3, 5, 0, 0)),
    "8": ((0, 0, 0, 5), (3, 0, 3, 5), (0, 5, 3, 5), (0, 0, 3, 0), (0, 2.5, 3, 2.5)),
    "9": ((0, 0, 0, 2.5), (0, 2.5, 3, 2.5), (3, 2.5, 3, 5), (0, 5, 3, 5)),
    ".": ((1.3, 0, 1.7, 0.4),),
    "-": ((0.5, 2.5, 2.5, 2.5),),
}


# ---------------------------------------------------------------------------
# small placement helpers
# ---------------------------------------------------------------------------


def _place(
    obj: bpy.types.Object,
    location: Sequence[float],
    rotation: Sequence[float] = (0.0, 0.0, 0.0),
) -> bpy.types.Object:
    """Rotate an origin-centred part about itself, THEN translate.

    Every primitive helper bakes its transform on the way out, so the object
    origin is at world zero and the mesh sits around it. Rotating first and
    translating second is therefore the only order that pivots the part about
    its own centre; doing it the other way spins it around the world origin
    and throws it metres off the sign.
    """
    obj.rotation_euler = rotation
    K.transform_apply(obj)
    obj.location = location
    K.transform_apply(obj)
    return obj


def _box(
    name: str,
    size: Sequence[float],
    location: Sequence[float],
    material: bpy.types.Material,
    bevel: float = 0.03,
) -> bpy.types.Object:
    """A bevelled box. Every hard edge in this module goes through here —
    a razor 90-degree corner is the single strongest low-poly tell."""
    return K.cube(name, size=size, location=location, material=material,
                  bevel=bevel, bevel_segments=2)


def _strut(
    name: str,
    p0: Sequence[float],
    p1: Sequence[float],
    section: float,
    material: bpy.types.Material,
    bevel: float = 0.02,
) -> bpy.types.Object:
    """A square-section bar spanning p0 -> p1, at any angle.

    `to_track_quat('X', 'Z')` maps the bar's local +X onto the span and keeps
    its local +Z as close to world up as it can, which is what a knee brace or
    a hanger rod needs.
    """
    a, b = Vector(p0), Vector(p1)
    span = b - a
    length = span.length
    if length < 1e-6:
        raise ValueError(f"_strut {name}: p0 and p1 coincide")
    euler = span.to_track_quat("X", "Z").to_euler()
    bar = K.cube(name, size=(length, section, section), location=(0.0, 0.0, 0.0),
                 material=material, bevel=bevel, bevel_segments=2)
    mid = (a + b) * 0.5
    return _place(bar, mid, euler)


# ---------------------------------------------------------------------------
# neon
# ---------------------------------------------------------------------------


def _neon_stroke(
    name: str,
    rgb: Sequence[float],
    x0: float,
    z0: float,
    x1: float,
    z1: float,
    y: float,
    radius: float = 0.030,
) -> bpy.types.Object | None:
    """One run of bent glass between two sign-face points.

    `P.neon_tube` bakes its own transform, so it is asked for an X-axis tube
    sitting on the world origin; the part is then swung about world Y by the
    stroke angle and finally pushed out to its place on the panel. The glass
    is therefore a real emissive cylinder of real diameter at any angle —
    axis-locked or diagonal — which is the whole point.

    **The swing axis is Y, not Z, and that is load-bearing.** The sign face
    lies in the world X-Z plane and the tubes stand off it along -Y, so a
    stroke's angle is a rotation about Y. The previous version rotated about
    Z, which spins the tube *out of the panel into depth* while leaving it in
    the wrong plane: measured on `sign_arcade`, a stroke authored from
    (0,0) to (0.30,0.50) came out with extent X 0.352 / Y 0.530 / Z 0.060
    instead of X 0.300 / Y 0.060 / Z 0.500 — the whole 0.58 m panel depth.
    Every glyph was therefore a fan of tubes standing at right angles to the
    board, which is why no letterform on any of the 8 signs was readable:
    the strokes overlapped each other in depth and projected as a smear of
    disconnected bars instead of as letters.
    """
    dx, dz = x1 - x0, z1 - z0
    length = math.hypot(dx, dz)
    if length < 1e-5:
        return None
    tube = P.neon_tube(name, rgb, length=length, radius=radius,
                       loc=(0.0, 0.0, 0.0), axis="x")
    return _place(tube, ((x0 + x1) * 0.5, y, (z0 + z1) * 0.5),
                  (0.0, math.atan2(dz, dx), 0.0))


def _neon_path(
    parts: list[bpy.types.Object],
    name: str,
    points: Sequence[tuple[float, float]],
    y: float,
    rgb: Sequence[float],
    radius: float = 0.030,
) -> None:
    """Sweep a run of glass through a polyline (open or closed)."""
    for i in range(len(points) - 1):
        (ax, az), (bx, bz) = points[i], points[i + 1]
        seg = _neon_stroke(f"{name}_s{i}", rgb, ax, az, bx, bz, y, radius)
        if seg is not None:
            parts.append(seg)


def _neon_ring(
    parts: list[bpy.types.Object],
    name: str,
    cx: float,
    cz: float,
    rx: float,
    rz: float,
    y: float,
    rgb: Sequence[float],
    radius: float = 0.022,
    steps: int = 10,
) -> None:
    """A glass ring, faceted into `steps` chords — how a bender actually
    makes a circle: as a polygon of straight runs."""
    pts = [
        (cx + rx * math.cos(2.0 * math.pi * i / steps),
         cz + rz * math.sin(2.0 * math.pi * i / steps))
        for i in range(steps + 1)
    ]
    _neon_path(parts, name, pts, y, rgb, radius)


# ---------------------------------------------------------------------------
# lettering
# ---------------------------------------------------------------------------


def _text_width(text: str, cell: float, tracking: float) -> float:
    """Advance width of a line, tracking included between glyphs only."""
    total = 0.0
    for ch in text.upper():
        total += (SPACE_W if ch == " " else GLYPH_W) * cell + tracking
    return max(0.0, total - tracking)


def _line_tubes(
    parts: list[bpy.types.Object],
    name: str,
    text: str,
    cell: float,
    tracking: float,
    centre_z: float,
    y: float,
    rgb: Sequence[float],
    radius: float,
) -> float:
    """Lay a line of stroke-alphabet glass, centred on X = 0, baseline at
    `centre_z`. Returns the line's width so the caller can fit the panel."""
    width = _text_width(text, cell, tracking)
    x = -width * 0.5
    for li, ch in enumerate(text.upper()):
        advance = (SPACE_W if ch == " " else GLYPH_W) * cell
        for si, (sx0, sz0, sx1, sz1) in enumerate(_ALPHABET.get(ch, ())):
            seg = _neon_stroke(
                f"{name}_c{li}s{si}", rgb,
                x + sx0 * cell, centre_z + sz0 * cell,
                x + sx1 * cell, centre_z + sz1 * cell,
                y, radius,
            )
            if seg is not None:
                parts.append(seg)
        x += advance + tracking
    return width


# ---------------------------------------------------------------------------
# frame kit
# ---------------------------------------------------------------------------


def _mount_positions(width: float, masts: int) -> tuple[float, ...]:
    """Where the masts stand, given the panel width.

    Two masts are pulled inboard from the panel edges so the panel visibly
    overhangs its own frame instead of being flush with it.
    """
    if masts == 1:
        return (0.0,)
    span = max(0.45, width * 0.5 - 0.62)
    return (-span, span)


def _mount_frame(
    parts: list[bpy.types.Object],
    asset_id: str,
    *,
    width: float,
    z_centre: float,
    masts: int,
    mast_h: float,
    mast_mat: bpy.types.Material,
    foot_mat: bpy.types.Material,
    bracket_mat: bpy.types.Material,
    conduit_mat: bpy.types.Material,
) -> tuple[float, ...]:
    """Grounded foot + mast + cantilever arm + knee struts + brackets.

    Returns the X positions of the masts so the caller can hang decorations
    off them. The foot's underside is the asset's z = 0.
    """
    mast_xs = _mount_positions(width, masts)

    for mi, mx in enumerate(mast_xs):
        # foot — the grounding slab; lowest face is z = 0
        _box(f"{asset_id}_foot_{mi}",
             size=(0.44, 0.48, FOOT_Z),
             location=(mx, MAST_Y, FOOT_Z * 0.5),
             material=foot_mat, bevel=0.04)
        # a chamfered cap plate over the foot reads as a bolted base plate
        _box(f"{asset_id}_footplate_{mi}",
             size=(0.30, 0.34, 0.05),
             location=(mx, MAST_Y, FOOT_Z + 0.025),
             material=foot_mat, bevel=0.02)
        # mast
        _box(f"{asset_id}_mast_{mi}",
             size=(MAST_S, MAST_S, mast_h),
             location=(mx, MAST_Y, mast_h * 0.5),
             material=mast_mat, bevel=0.03)
        # cantilever arm, mast -> panel back
        _box(f"{asset_id}_arm_{mi}",
             size=(ARM_S, 0.34, ARM_S),
             location=(mx, ARM_Y, z_centre),
             material=mast_mat, bevel=0.03)
        # stiffener plate bolting the arm to the panel
        _box(f"{asset_id}_stiffener_{mi}",
             size=(0.26, 0.06, 0.46),
             location=(mx, PANEL_T * 0.5 + 0.03, z_centre),
             material=bracket_mat, bevel=0.02)
        # forked knee struts — the brace that stops the arm looking unsupported
        for sx, tag in ((-1.0, "a"), (1.0, "b")):
            brace = _strut(
                f"{asset_id}_brace_{mi}{tag}",
                (mx + sx * 0.05, MAST_Y - MAST_S * 0.5, z_centre - 0.68),
                (mx + sx * 0.05, 0.13, z_centre - 0.08),
                0.055, mast_mat, bevel=0.02,
            )
            parts.append(brace)

    # junction box + conduit run: the sign is wired, not floating
    jbox_z = max(0.9, z_centre - 1.05)
    _box(f"{asset_id}_junction",
         size=(0.15, 0.11, 0.24),
         location=(mast_xs[0], MAST_Y - 0.13, jbox_z),
         material=bracket_mat, bevel=0.02)
    conduit = K.cylinder(
        f"{asset_id}_conduit", radius=0.024, depth=jbox_z * 0.92, vertices=10,
        location=(0.0, 0.0, 0.0), material=conduit_mat, smooth=True,
    )
    _place(conduit, (mast_xs[0], MAST_Y - 0.07, jbox_z * 0.46))
    parts.append(conduit)

    return mast_xs


def _panel_kit(
    parts: list[bpy.types.Object],
    asset_id: str,
    *,
    width: float,
    height: float,
    z_centre: float,
    plate_mat: bpy.types.Material,
    face_mat: bpy.types.Material,
    rail_mat: bpy.types.Material,
) -> None:
    """Backing plate + proud sign face + four border rails."""
    # backing plate: the carcass that bolts to the arm
    _box(f"{asset_id}_plate",
         size=(width, PANEL_T, height),
         location=(0.0, 0.0, z_centre),
         material=plate_mat, bevel=0.045)
    # sign face: near-black, standing proud of the plate on -Y only so the
    # back stays flush for wall mounting
    face_w = width - 2.0 * RAIL_T
    face_h = height - 2.0 * RAIL_T
    _box(f"{asset_id}_face",
         size=(face_w, FACE_T, face_h),
         location=(0.0, FACE_Y, z_centre),
         material=face_mat, bevel=0.02)
    # border rails, standing proud of the face and framing it
    top_z = z_centre + height * 0.5 - RAIL_T * 0.5
    bot_z = z_centre - height * 0.5 + RAIL_T * 0.5
    _box(f"{asset_id}_rail_top",
         size=(width, RAIL_D, RAIL_T), location=(0.0, RAIL_Y, top_z),
         material=rail_mat, bevel=0.025)
    _box(f"{asset_id}_rail_bottom",
         size=(width, RAIL_D, RAIL_T), location=(0.0, RAIL_Y, bot_z),
         material=rail_mat, bevel=0.025)
    side_h = height - 2.0 * RAIL_T
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        _box(f"{asset_id}_rail_{tag}",
             size=(RAIL_T, RAIL_D, side_h),
             location=(sx * (width * 0.5 - RAIL_T * 0.5), RAIL_Y, z_centre),
             material=rail_mat, bevel=0.025)
    # a painted accent band across the foot of the face breaks the black hole
    _box(f"{asset_id}_band",
         size=(face_w * 0.86, 0.03, 0.055),
         location=(0.0, FACE_FRONT - 0.015,
                   z_centre - height * 0.5 + RAIL_T + 0.045),
         material=rail_mat, bevel=0.012)


def _lamp(
    name: str,
    location: Sequence[float],
    radius: float,
    material: bpy.types.Material,
) -> bpy.types.Object:
    """A marquee bulb: a real emissive sphere half-sunk into the rail."""
    bulb = K.sphere(name, radius=radius, segments=10, ring_count=6,
                    location=(0.0, 0.0, 0.0), material=material)
    return _place(bulb, location)


# ---------------------------------------------------------------------------
# shared assembly
# ---------------------------------------------------------------------------


def _assemble(
    asset_id: str,
    *,
    main: str,
    sub: str,
    cell: float = 0.105,
    tracking: float = 0.055,
    sub_scale: float = 0.50,
    masts: int = 1,
    mast_h: float = 3.40,
    crest_rise: float = 0.78,
    finial: bool = True,
    tube_rgb: Sequence[float] = HOT_PINK,
    accent_rgb: Sequence[float] = ICE_CYAN,
    plate_rgb: Sequence[float] = (0.085, 0.095, 0.115),
    rail_rgb: Sequence[float] = (0.30, 0.32, 0.36),
    face_rgb: Sequence[float] = (0.030, 0.034, 0.044),
    decor: Callable[[list[bpy.types.Object], dict], None] | None = None,
) -> list[bpy.types.Object]:
    """Build one sign. Returns UV'd, triangulated parts."""
    # --- materials: one per part family, never one for the whole sign ------
    plate = P.metal(f"{asset_id}_plate_mat", rgb=plate_rgb, rough=0.48, metal=0.75)
    rail = P.paint(f"{asset_id}_rail_mat", rail_rgb, metallic=0.55, roughness=0.36)
    face = P.paint(f"{asset_id}_face_mat", face_rgb, metallic=0.10, roughness=0.58)
    mast = P.metal(f"{asset_id}_mast_mat", rgb=(0.34, 0.36, 0.40), rough=0.42, metal=0.85)
    foot = P.metal(f"{asset_id}_foot_mat", rgb=(0.26, 0.27, 0.30), rough=0.62, metal=0.70)
    bracket = P.paint(f"{asset_id}_bracket_mat", (0.11, 0.12, 0.14),
                      metallic=0.40, roughness=0.50)
    conduit = P.metal(f"{asset_id}_conduit_mat", rgb=(0.45, 0.46, 0.50),
                      rough=0.40, metal=0.90)
    bulb_mat = P.emissive(f"{asset_id}_bulb_mat", SODIUM, strength=5.0)

    # --- tube radii, then letter-spacing ----------------------------------
    # Letter-spacing must exceed the tube DIAMETER, or neighbouring glyphs
    # physically intersect and the word fuses into one continuous smear.
    # Measured before this clamp: every one of the 8 signs had tracking
    # between 3.2 mm and 10.6 mm SMALLER than 2 * radius — e.g. sign_arcade
    # tracked 50 mm glyphs on 56 mm tubes, so every pair of letters touched.
    # A neon tube is a glass rod, and no glassbender leaves two letters fused
    # at the shoulder; 1.15 x diameter is the clearance that keeps the word
    # separable at preview resolution.
    #
    # This is computed BEFORE the panel is fitted below: widening the word
    # after the panel has been sized from it would run the lettering off both
    # ends of its own board.
    main_r = max(0.018, min(0.038, cell * 0.28))
    sub_cell = cell * sub_scale
    sub_r = max(0.014, min(0.028, sub_cell * 0.28))
    accent_r = max(0.018, min(0.030, cell * 0.24))
    tracking = max(tracking, main_r * 2.0 * 1.15)
    sub_tracking = max(tracking * sub_scale, sub_r * 2.0 * 1.15)

    # --- panel sized from the lettering ------------------------------------
    main_w = _text_width(main, cell, tracking)
    sub_w = _text_width(sub, sub_cell, sub_tracking)
    width = max(main_w, sub_w) + 2.0 * PAD
    sub_h = GLYPH_H * sub_cell
    gap = 0.16
    height = PAD + GLYPH_H * cell + gap + sub_h + PAD

    z_top = mast_h - 0.16
    z_bottom = z_top - height
    z_centre = (z_top + z_bottom) * 0.5

    tube_y = FACE_FRONT - main_r - 0.006
    crest_y = FACE_FRONT - accent_r - 0.006

    parts: list[bpy.types.Object] = []

    # --- structure ---------------------------------------------------------
    _mount_frame(
        parts, asset_id,
        width=width, z_centre=z_centre, masts=masts, mast_h=mast_h,
        mast_mat=mast, foot_mat=foot, bracket_mat=bracket, conduit_mat=conduit,
    )
    _panel_kit(parts, asset_id, width=width, height=height, z_centre=z_centre,
               plate_mat=plate, face_mat=face, rail_mat=rail)

    # --- letterforms -------------------------------------------------------
    main_base = z_bottom + PAD
    sub_base = main_base + GLYPH_H * cell + gap
    _line_tubes(parts, f"{asset_id}_main", main, cell, tracking,
                main_base, tube_y, tube_rgb, main_r)
    _line_tubes(parts, f"{asset_id}_sub", sub, sub_cell, sub_tracking,
                sub_base, tube_y, accent_rgb, sub_r)

    # --- accent rules ------------------------------------------------------
    # border loop just inside the rails, in the second neon colour
    lo_x = -(width * 0.5 - RAIL_T - 0.07)
    hi_x = width * 0.5 - RAIL_T - 0.07
    lo_z = z_centre - (height * 0.5 - RAIL_T - 0.07)
    hi_z = z_centre + (height * 0.5 - RAIL_T - 0.07)
    _neon_path(parts, f"{asset_id}_border",
               [(lo_x, lo_z), (hi_x, lo_z), (hi_x, hi_z), (lo_x, hi_z), (lo_x, lo_z)],
               FACE_FRONT - accent_r - 0.006, accent_rgb, accent_r)
    # underline rule under the descriptor line
    rule_w = sub_w * 0.5 + 0.10
    _neon_path(parts, f"{asset_id}_rule",
               [(-rule_w, sub_base - 0.09), (rule_w, sub_base - 0.09)],
               FACE_FRONT - accent_r - 0.006, accent_rgb, accent_r * 0.8)

    # --- crest -------------------------------------------------------------
    zb = z_top + crest_rise * 0.30
    if crest_rise > 0.0:
        for sx, tag in ((-1.0, "l"), (1.0, "r")):
            post = _box(f"{asset_id}_crestpost_{tag}",
                        size=(0.07, 0.07, crest_rise * 0.34),
                        location=(sx * 0.34, RAIL_Y, z_top + crest_rise * 0.17),
                        material=mast, bevel=0.02)
            parts.append(post)

    # A decor hook that never runs is dead geometry nobody notices until the
    # sign renders without its crest, so the ctx is built unconditionally and
    # every builder's decor is guaranteed to execute.
    ctx = {
        "asset_id": asset_id, "W": width, "H": height,
        "z_top": z_top, "z_bottom": z_bottom, "z_centre": z_centre,
        "crest_y": crest_y, "face_y": FACE_FRONT - accent_r - 0.006,
        "tube": tube_rgb, "accent": accent_rgb,
        "r": accent_r, "base_z": zb if crest_rise > 0.0 else 0.0,
        "sub_w": sub_w,
        # the descriptor line's own baseline, so a decor that decorates AROUND
        # the text (chevrons, side rules) does not have to guess where the text
        # is — and never anchors off crest space that does not exist
        "sub_z": sub_base,
        "main_z": main_base,
        "sub_cell": sub_cell,
        "bulb_mat": bulb_mat,
        # crest sub-parts get their own materials so a crest is never one flat
        # slab of the same material as the frame around it
        "accent_metal": P.metal(f"{asset_id}_crest_metal",
                                rgb=(0.72, 0.62, 0.34), rough=0.22, metal=1.0),
        "accent_disc": P.metal(f"{asset_id}_disc_metal",
                               rgb=(0.80, 0.84, 0.90), rough=0.12, metal=1.0),
        "mast_xs": _mount_positions(width, masts),
    }
    if decor is not None:
        decor(parts, ctx)
    if finial:
        for mi, mx in enumerate(ctx["mast_xs"]):
            # A cap plate sits ON TOP of the mast: the mast spans 0..mast_h, so
            # a 0.06-thick cap is centred at mast_h + 0.03, not at mast_h - 0.03
            # (which would bury its lower half inside the mast and, for a short
            # mast, could reach below z=0).
            cap = _box(f"{asset_id}_mastcap_{mi}",
                       size=(0.21, 0.21, 0.06),
                       location=(mx, MAST_Y, mast_h + 0.03),
                       material=foot, bevel=0.02)
            parts.append(cap)
            fin = K.cone(f"{asset_id}_finial_{mi}", radius1=0.10, radius2=0.0,
                         depth=0.20, vertices=12,
                         location=(0.0, 0.0, 0.0), material=mast)
            _place(fin, (mx, MAST_Y, mast_h + 0.16))
            parts.append(fin)

    return P.finish(parts)


# ---------------------------------------------------------------------------
# per-venue crests and accents — the thing that stops eight signs reading as
# eight recolours of the same object
# ---------------------------------------------------------------------------


def _decor_arcade(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Twin-mast marquee: lamp rows on both rails and chevrons flanking the
    descriptor line. No crest — the lamp rows ARE the silhouette here."""
    aid, w = ctx["asset_id"], ctx["W"]
    y = ctx["face_y"]
    span = w * 0.5 - RAIL_T - 0.12
    n = max(5, min(12, int(span * 2.0 / 0.30)))
    for zi, tag in ((ctx["z_top"], "t"), (ctx["z_bottom"], "b")):
        row_z = zi - RAIL_T * 0.5 if tag == "t" else zi + RAIL_T * 0.5
        for i in range(n):
            x = -span + (2.0 * span * i / (n - 1))
            parts.append(_lamp(f"{aid}_bulb_{tag}{i}",
                               (x, RAIL_FRONT + 0.012, row_z), 0.045,
                               ctx["bulb_mat"]))
    # inward chevrons flanking the descriptor line — anchored to the text's
    # own baseline, not to crest space this sign does not have
    cx = ctx["sub_w"] * 0.5 + 0.20
    cz = ctx["sub_z"] + GLYPH_H * 0.5 * ctx["sub_cell"]
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        s = sx * cx
        _neon_path(parts, f"{aid}_chevron_{tag}",
                   [(s + sx * 0.14, cz + 0.13), (s, cz), (s + sx * 0.14, cz - 0.13)],
                   y, ctx["accent"], 0.024)


def _decor_martini(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Bar crest: a martini glass in glass, with an accent olive."""
    aid, y, zb, r = ctx["asset_id"], ctx["crest_y"], ctx["base_z"], ctx["r"]
    _neon_path(parts, f"{aid}_crest_bowl",
               [(-0.28, zb + 0.10), (0.0, zb + 0.50), (0.28, zb + 0.10)],
               y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_rim",
               [(-0.31, zb + 0.10), (0.31, zb + 0.10)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_stem",
               [(0.0, zb + 0.10), (0.0, zb - 0.17)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_foot",
               [(-0.17, zb - 0.17), (0.17, zb - 0.17)], y, ctx["tube"], r)
    _neon_ring(parts, f"{aid}_crest_olive", 0.0, zb + 0.26, 0.055, 0.055,
               y, ctx["accent"], r * 0.7, steps=8)


def _decor_sparkle(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Club crest: a four-point sparkle, plus a real disco ball on a hanger
    slung under the arm."""
    aid, y, zb, r = ctx["asset_id"], ctx["crest_y"], ctx["base_z"], ctx["r"]
    _neon_path(parts, f"{aid}_crest_h",
               [(-0.34, zb + 0.16), (0.34, zb + 0.16)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_v",
               [(0.0, zb - 0.16), (0.0, zb + 0.48)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_d1",
               [(-0.10, zb + 0.06), (0.10, zb + 0.26)], y, ctx["accent"], r * 0.8)
    _neon_path(parts, f"{aid}_crest_d2",
               [(0.10, zb + 0.06), (-0.10, zb + 0.26)], y, ctx["accent"], r * 0.8)

    ball_z = ctx["z_bottom"] - 0.34
    hanger = K.cylinder(f"{aid}_ball_hanger", radius=0.012, depth=0.30,
                        vertices=8, location=(0.0, 0.0, 0.0),
                        material=ctx["bulb_mat"], smooth=True)
    _place(hanger, (0.0, ARM_Y, ctx["z_centre"] - 0.44))
    parts.append(hanger)
    ball = K.sphere(f"{aid}_discoball", radius=0.19, segments=20, ring_count=12,
                    location=(0.0, 0.0, 0.0), material=ctx["accent_disc"])
    _place(ball, (0.0, ARM_Y, ball_z))
    parts.append(ball)
    # facet bands, so the ball is a mirrored ball and not a plain sphere
    for ang in (0.0, 60.0, 120.0):
        band = K.torus(f"{aid}_ballband_{int(ang)}", major_radius=0.186,
                       minor_radius=0.010, major_segments=20, minor_segments=6,
                       location=(0.0, 0.0, 0.0), material=ctx["accent_disc"])
        _place(band, (0.0, ARM_Y, ball_z),
               (math.radians(90.0), 0.0, math.radians(ang)))
        parts.append(band)


def _decor_cup(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Diner crest: a coffee cup, handle and two steam curls."""
    aid, y, zb, r = ctx["asset_id"], ctx["crest_y"], ctx["base_z"], ctx["r"]
    _neon_path(parts, f"{aid}_crest_cup",
               [(-0.20, zb + 0.46), (-0.14, zb + 0.06), (0.14, zb + 0.06),
                (0.20, zb + 0.46)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_rim",
               [(-0.25, zb + 0.46), (0.25, zb + 0.46)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_handle",
               [(0.20, zb + 0.40), (0.34, zb + 0.34), (0.34, zb + 0.18),
                (0.20, zb + 0.12)], y, ctx["tube"], r * 0.9)
    _neon_path(parts, f"{aid}_crest_saucer",
               [(-0.27, zb - 0.02), (0.31, zb - 0.02)], y, ctx["accent"], r * 0.85)
    for i, sx in enumerate((-0.09, 0.09)):
        _neon_path(parts, f"{aid}_crest_steam{i}",
                   [(sx, zb + 0.56), (sx + 0.07, zb + 0.66),
                    (sx - 0.05, zb + 0.76), (sx + 0.04, zb + 0.86)],
                   y, ctx["accent"], r * 0.75)


def _decor_crown(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Hotel crest: a stepped parapet crown with lamps on each tier."""
    aid, zb = ctx["asset_id"], ctx["base_z"]
    tiers = ((0.90, 0.06), (0.62, 0.18), (0.34, 0.30))
    for i, (tw, dz) in enumerate(tiers):
        _box(f"{aid}_crest_tier{i}",
             size=(tw, 0.07, 0.12),
             location=(0.0, ctx["crest_y"] + 0.02, zb + dz),
             material=ctx["accent_metal"], bevel=0.025)
        parts.append(_lamp(f"{aid}_crest_lamp{i}",
                           (0.0, ctx["crest_y"] - 0.02, zb + dz + 0.12),
                           0.055, ctx["bulb_mat"]))
    for i, sx in enumerate((-0.32, 0.32)):
        _box(f"{aid}_crest_pinion{i}",
             size=(0.10, 0.07, 0.14),
             location=(sx, ctx["crest_y"] + 0.02, zb + 0.18 + 0.13),
             material=ctx["accent_metal"], bevel=0.02)
        parts.append(_lamp(f"{aid}_crest_pinlamp{i}",
                           (sx, ctx["crest_y"] - 0.02, zb + 0.18 + 0.26),
                           0.040, ctx["bulb_mat"]))


def _decor_arrow(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Motel crest: a vacancy arrow pointing down at the rooms below."""
    aid, y, zb, r = ctx["asset_id"], ctx["crest_y"], ctx["base_z"], ctx["r"]
    _neon_path(parts, f"{aid}_crest_shaft",
               [(0.0, zb + 0.54), (0.0, zb + 0.10)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_head",
               [(-0.17, zb + 0.26), (0.0, zb + 0.10), (0.17, zb + 0.26)],
               y, ctx["tube"], r)
    _neon_ring(parts, f"{aid}_crest_ring", 0.0, zb + 0.54, 0.10, 0.10,
               y, ctx["accent"], r * 0.75, steps=8)


def _decor_slice(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Pizza crest: a wedge with a curved crust and two accents."""
    aid, y, zb, r = ctx["asset_id"], ctx["crest_y"], ctx["base_z"], ctx["r"]
    _neon_path(parts, f"{aid}_crest_wedge",
               [(-0.26, zb), (-0.09, zb + 0.44), (0.0, zb + 0.50),
                (0.09, zb + 0.44), (0.26, zb)], y, ctx["tube"], r)
    _neon_path(parts, f"{aid}_crest_sauce",
               [(-0.15, zb + 0.26), (0.15, zb + 0.26)], y, ctx["accent"], r * 0.75)
    for i, sx in enumerate((-0.07, 0.07)):
        _neon_ring(parts, f"{aid}_crest_dash{i}", sx, zb + 0.12, 0.055, 0.045,
                   y, ctx["accent"], r * 0.6, steps=6)
    _neon_path(parts, f"{aid}_crest_crust",
               [(-0.09, zb + 0.44), (-0.04, zb + 0.48), (0.04, zb + 0.48),
                (0.09, zb + 0.44)], y, ctx["tube"], r * 1.05)


def _decor_sun(parts: list[bpy.types.Object], ctx: dict) -> None:
    """Tropic crest: a mirrored sun disc inside a neon ring, with rays."""
    aid, y, zb = ctx["asset_id"], ctx["crest_y"], ctx["base_z"]
    cz = zb + 0.30
    disc = K.cylinder(f"{aid}_crest_disc", radius=0.24, depth=0.06, vertices=24,
                      location=(0.0, 0.0, 0.0), material=ctx["accent_metal"])
    _place(disc, (0.0, y + 0.03, cz), (math.radians(90.0), 0.0, 0.0))
    parts.append(disc)
    ring = K.torus(f"{aid}_crest_ring", major_radius=0.30, minor_radius=0.048,
                   major_segments=28, minor_segments=10,
                   location=(0.0, 0.0, 0.0), material=ctx["accent_metal"])
    _place(ring, (0.0, y, cz), (math.radians(90.0), 0.0, 0.0))
    parts.append(ring)
    for i in range(8):
        a = math.radians(22.5 + i * 45.0)
        c, s = math.cos(a), math.sin(a)
        _neon_path(parts, f"{aid}_crest_ray{i}",
                   [(c * 0.37, cz + s * 0.37), (c * 0.52, cz + s * 0.52)],
                   y, ctx["tube"], 0.024)


# ---------------------------------------------------------------------------
# builders — one per venue, all invented names
# ---------------------------------------------------------------------------


def build_arcade(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="JOYSTICK", sub="GAMES",
        masts=2, cell=0.100, tracking=0.050, mast_h=3.10,
        crest_rise=0.0, finial=True,
        tube_rgb=ICE_CYAN, accent_rgb=HOT_PINK,
        plate_rgb=(0.060, 0.070, 0.090),
        rail_rgb=(0.24, 0.26, 0.32),
        decor=_decor_arcade,
        **kwargs,
    )


def build_bar(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="SPILLWAY", sub="OPEN LATE",
        masts=1, cell=0.100, tracking=0.052, mast_h=3.25,
        crest_rise=0.80,
        tube_rgb=DEEP_MAGENTA, accent_rgb=SODIUM,
        plate_rgb=(0.075, 0.060, 0.080),
        rail_rgb=(0.32, 0.28, 0.30),
        decor=_decor_martini,
        **kwargs,
    )


def build_club(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="VELVET", sub="LIVE MUSIC",
        masts=1, cell=0.135, tracking=0.065, mast_h=3.70,
        crest_rise=0.72,
        tube_rgb=PLUM, accent_rgb=ICE_CYAN,
        plate_rgb=(0.055, 0.050, 0.085),
        rail_rgb=(0.28, 0.26, 0.38),
        decor=_decor_sparkle,
        **kwargs,
    )


def build_diner(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="KETTLE", sub="ALL NIGHT",
        masts=1, cell=0.125, tracking=0.060, mast_h=3.45,
        crest_rise=1.00,
        tube_rgb=TANGERINE, accent_rgb=COLD_WHITE,
        plate_rgb=(0.090, 0.080, 0.070),
        rail_rgb=(0.38, 0.34, 0.30),
        decor=_decor_cup,
        **kwargs,
    )


def build_hotel(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="SALT HOUSE", sub="24 HRS",
        masts=1, cell=0.095, tracking=0.050, mast_h=4.20,
        crest_rise=0.60,
        tube_rgb=SODIUM, accent_rgb=COLD_WHITE,
        plate_rgb=(0.070, 0.065, 0.075),
        rail_rgb=(0.44, 0.38, 0.26),
        decor=_decor_crown,
        **kwargs,
    )


def build_motel(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="ARROYO", sub="VACANCY",
        masts=2, cell=0.120, tracking=0.058, mast_h=2.70,
        crest_rise=0.78,
        tube_rgb=AQUAMARINE, accent_rgb=TANGERINE,
        plate_rgb=(0.070, 0.080, 0.080),
        rail_rgb=(0.30, 0.34, 0.34),
        decor=_decor_arrow,
        **kwargs,
    )


def build_pizza(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="UMBER OVEN", sub="SLICES",
        masts=1, cell=0.098, tracking=0.050, mast_h=3.05,
        crest_rise=0.66,
        tube_rgb=(1.0, 0.22, 0.10), accent_rgb=SODIUM,
        plate_rgb=(0.085, 0.055, 0.050),
        rail_rgb=(0.36, 0.26, 0.22),
        decor=_decor_slice,
        **kwargs,
    )


def build_tropic(asset_id: str, **kwargs) -> list[bpy.types.Object]:
    K.reset_scene()
    return _assemble(
        asset_id,
        main="CORAL SUN", sub="COCKTAILS",
        masts=1, cell=0.096, tracking=0.050, mast_h=3.35,
        crest_rise=0.86,
        tube_rgb=LIME, accent_rgb=HOT_PINK,
        plate_rgb=(0.060, 0.080, 0.078),
        rail_rgb=(0.28, 0.34, 0.32),
        decor=_decor_sun,
        **kwargs,
    )


# ---------------------------------------------------------------------------
# registry payload: (asset_id, builder_callable, kwargs)
# ---------------------------------------------------------------------------

SIGNS: list[tuple[str, Callable, dict]] = [
    ("sign_arcade", build_arcade, dict()),
    ("sign_bar", build_bar, dict()),
    ("sign_club", build_club, dict()),
    ("sign_diner", build_diner, dict()),
    ("sign_hotel", build_hotel, dict()),
    ("sign_motel", build_motel, dict()),
    ("sign_pizza", build_pizza, dict()),
    ("sign_tropic", build_tropic, dict()),
]
