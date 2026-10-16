"""3A hand-held weapon builder — a pistol, an SMG, a pump shotgun, two
shoulder launchers, a bat and a frag grenade.

**Every design here is original.** Nothing in this module traces, references
or proportions against a real manufacturer's product: no trade dress, no
brand marking, no receiver profile lifted off a catalogue. What is modelled
is the set of *functional primitives* every firearm of a class shares —
receiver, barrel, grip, trigger guard, sight rail, magazine — because those
primitives are what make a shape legible as "a gun" at all. Beyond that the
proportions are chosen for a game prop, not for a real weapon.

A weapon reads as **machined**, never as a box, because of four things:

1. **Chamfered hard edges.** A razor 90-degree edge catches the specular
   highlight as a hard line; an 8-25 mm chamfer rolls it off. On props this
   small the chamfer IS the silhouette. Every box here enters through
   `_rbox`, which clamps the bevel into that band — a chamfer below 8 mm
   disappears at gameplay distance, and one above 25 mm on a 30 mm part
   eats the whole edge.
2. **Per-part material zones.** Dark gunmetal (`P.metal`, low roughness) for
   machined parts, matte polymer (`P.fabric` / `P.paint`) for the furniture
   a hand touches, a near-mirror `P.metal` for the one bright accent. A
   weapon painted one flat colour is a blockout no matter how many triangles
   it has.
3. **Legible parts.** A pistol with no slide / no trigger guard / no sights
   is a blue-grey lump. Each builder names its parts after what they are, so
   the silhouette carries the read at 30 m.
4. **Massing before detail.** Bore height, barrel length and the furniture
   that lands on the ground are the proportions the eye checks first.

Coordinate contract (inherited, unchanged): authoring is **Z-up**, **-Y is
forward**, metres, centred on X = 0. Every weapon is authored level and
`_ground` drops the finished assembly so its **lowest point rests exactly on
z = 0** — the part that lands on the ground is whatever the numbers make it
(magwell base plate, grip heel, launcher grip), and it is measured off the
real vertex data rather than declared.

Built with real `bpy` through `kernel` helpers only: every primitive comes
from a `K.*` call, every rotation and translation is applied with
`K.transform_apply` so nothing is left in an unapplied object transform, and
the tail step is `P.finish(parts)` (UV layer + triangulation, without which
the glTF exporter silently drops tangent space on every n-gon).

Every part is sized so that adjoining parts **overlap by 5-10 mm** rather
than meeting on a coincident plane. Two shells that share an exact face
z-fight in the preview render and read as a seam through the model; a shell
that falls short of its neighbour floats, which reads worse. The overlap
sizes below are chosen from those two rules, not by eye.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bpy

from .. import kernel as K
from .. import palette as P

# Authoring frame: Z-up, -Y forward, X centred. Every builder below quotes
# its bore height and its furniture heights against this frame.

BEVEL_MIN = 0.008
BEVEL_MAX = 0.025


# ---------------------------------------------------------------------------
# materials — three zones, so the albedo itself does the reading
# ---------------------------------------------------------------------------

def _gunmetal(name: str, rgb=(0.185, 0.195, 0.215), rough: float = 0.26):
    """Dark gunmetal: receivers, barrels, launcher tubes.

    Low roughness on purpose — the chamfers have to roll a highlight rather
    than snap to a line, and a rough gunmetal is indistinguishable from
    primer-grey plastic at gameplay distance.
    """
    return P.metal(name, rgb=rgb, rough=rough, metal=0.95)


def _polymer(name: str, rgb=(0.095, 0.095, 0.115), rough: float = 0.74):
    """Matte polymer — grips, stocks, handguards. What a hand actually touches."""
    return P.fabric(name, rgb=rgb, rough=rough)


def _bright(name: str):
    """Chrome-equivalent: the one mirror-bright machined part on a weapon.

    Bell mouths, muzzle crowns, pins and seams are where a model reads as
    *turned* rather than *extruded*, so they get the mirror response.
    """
    return P.metal(name, rgb=(0.80, 0.83, 0.88), rough=0.09, metal=1.0)


def _accent(name: str, rgb: Sequence[float]):
    """A single deliberate paint stripe. Abstract geometry, never a marking."""
    return P.paint(name, rgb=rgb, metallic=0.0, roughness=0.58)


def _bore(name: str):
    """Near-black, rough: a bore, an ejection port, a trigger recess.

    The recess read comes from albedo, not from an actual hole — these are
    solid parts, because a real boolean cut on a 6 mm feature is a
    triangulated mess the exporter would smear.
    """
    return P.metal(name, rgb=(0.035, 0.035, 0.040), rough=0.62, metal=0.50)


def _bevel(width: float) -> float:
    """Clamp a chamfer into the band that reads at gameplay distance."""
    return max(BEVEL_MIN, min(width, BEVEL_MAX))


def _safe_bevel(width: float, size: Sequence[float]) -> float:
    """Clamp a chamfer so it cannot exceed the geometry it is cut into.

    `_bevel` alone is the bug this function exists to fix. It clamps only
    into an absolute 8-25 mm band, which is right for a 30 cm receiver and
    catastrophic for a 4 mm serration rib: a bevel wider than the part's own
    smallest dimension collapses the whole edge ring onto a plane, and the
    next `transform_apply` quantises those coincident verts into literally
    identical positions. The mesh then carries zero-area quads, which
    triangulate into zero-area triangles — the "scattered pale fragments" on
    `wep_tank` and the "detached magazine" on `wep_smg`.

    Measured on `wep_pistol`: 402 zero-area faces across 25 parts, every one
    of them a `_block` thinner than 2x its own chamfer. Capping the chamfer at
    40% of the smallest dimension (a normal manufacturing rule — you cannot
    chamfer more than half an edge without destroying it) removes the cause
    rather than welding the symptom afterwards.
    """
    smallest = min(abs(float(a)) for a in size) or BEVEL_MIN
    return max(1e-4, min(_bevel(width), smallest * 0.4))


# ---------------------------------------------------------------------------
# geometry helpers on top of the kernel primitives
# ---------------------------------------------------------------------------

def _rbox(
    name: str,
    size: Sequence[float],
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    rot: Sequence[float] = (0.0, 0.0, 0.0),
    bevel: float = 0.012,
    bevel_segments: int = 2,
) -> bpy.types.Object:
    """A bevelled box rotated about its OWN centre, then placed.

    `K.cube` is axis-aligned only, and a weapon is full of raked parts: a
    pistol grip leans back, a shotgun comb slopes down to the butt, a dust
    cover steps. Rotating after placement swings the part about the WEAPON's
    origin instead of its own, which throws it tens of centimetres out of
    the asset; so the box is built at the world origin, rotated and applied
    there, and only then moved into position.

    `rot` is in DEGREES, and a POSITIVE X rotation is what leans a grip
    "back" in this frame: rotating about X by +a maps the part's top (+Z) to
    -Y, i.e. toward the muzzle. A grip rakes with its top FORWARD of its
    heel, so grips here take a positive rake. (A magazine segment does the
    opposite — its bottom swings rearward — so magazine segments take a
    negative one.)
    """
    obj = K.cube(name, size=size, location=(0.0, 0.0, 0.0), material=material,
                 bevel=_safe_bevel(bevel, size), bevel_segments=bevel_segments)
    obj.rotation_euler = tuple(math.radians(a) for a in rot)
    K.transform_apply(obj)
    obj.location = location
    K.transform_apply(obj)
    return obj


def _block(
    name: str,
    size: Sequence[float],
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    bevel: float = 0.008,
) -> bpy.types.Object:
    """An axis-aligned chamfered box — the plain case `_rbox` covers."""
    return _rbox(name, size, location, material, bevel=bevel, bevel_segments=1)


def _tube(
    name: str,
    radius: float,
    length: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    vertices: int = 32,
) -> bpy.types.Object:
    """A cylinder laid along the authoring forward axis (-Y).

    `K.cylinder` builds its mesh on the world Z axis and leaves the placement
    in `obj.location`; barrels and launcher tubes run along Y here, so the
    object is rotated 90 degrees about X (which maps +Z to -Y) and the whole
    transform applied.
    """
    obj = K.cylinder(name, radius=radius, depth=length, vertices=vertices,
                     location=(0.0, 0.0, 0.0), material=material, smooth=True)
    obj.location = location
    obj.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(obj)
    return obj


def _flare(
    name: str,
    r_rear: float,
    r_front: float,
    length: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    vertices: int = 40,
) -> bpy.types.Object:
    """A genuine cone along -Y: `r_rear` at the back, `r_front` at the muzzle.

    `K.cone`'s radius1 sits on the mesh's -Z end and radius2 on its +Z end.
    Rotating +90 degrees about X sends -Z to +Y and +Z to -Y, so the FRONT
    radius is radius2 — which is why every flare below passes its larger,
    forward-facing radius as `radius2`. This is what makes a launcher bell a
    real bell rather than a cylinder with a fatter cylinder stuck on the end.
    """
    obj = K.cone(name, radius1=r_rear, radius2=r_front, depth=length,
                 vertices=vertices, location=(0.0, 0.0, 0.0),
                 material=material, smooth=True)
    obj.location = location
    obj.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(obj)
    return obj


def _post(
    name: str,
    radius: float,
    length: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    vertices: int = 24,
) -> bpy.types.Object:
    """A cylinder standing on the world Z axis — knobs, posts, pins."""
    return K.cylinder(name, radius=radius, depth=length, vertices=vertices,
                      location=location, material=material, smooth=True)


def _pin(
    name: str,
    radius: float,
    depth: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    axis: str = "x",
    vertices: int = 16,
) -> bpy.types.Object:
    """A small cylinder driven through a part along X, Y or Z.

    Pins and bosses are the details that stop a weapon looking CAD-default,
    and they are the one place a part is legitimately built on a non-Z axis
    by hand rather than through `_tube` / `_post`.
    """
    obj = _post(name, radius, depth, location, material, vertices=vertices)
    if axis == "x":
        obj.rotation_euler = (0.0, math.radians(90.0), 0.0)
    elif axis == "y":
        obj.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(obj)
    return obj


def _bow(
    name: str,
    major: float,
    minor: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    guard_height: float = 0.95,
    guard_length: float = 1.35,
    flatten: float = 0.45,
    major_segments: int = 28,
    minor_segments: int = 10,
) -> bpy.types.Object:
    """A flattened loop in the Y-Z plane — a trigger guard.

    A `K.torus` is built in the XY plane about the world Z axis. Rotating +90
    degrees about Y moves it into the Y-Z plane, which is the plane a trigger
    guard actually occupies (the barrel runs along Y, so the guard is a loop
    around the Z-up sweep under the frame).

    The object scale then controls the three axes **after** that rotation, in
    this order — NOT as "x, y, z":

    * ``scale.x`` -> the loop's extent in world **Z**  (how TALL the guard is)
    * ``scale.y`` -> the loop's extent in world **Y**  (how LONG it is)
    * ``scale.z`` -> the tube's thickness in world **X** (how FLAT it is)

    A guard as round as its source torus reads as a bicycle chain; flattened
    to ~45% on X it reads as a stamped, milled part.
    """
    obj = K.torus(name, major_radius=major, minor_radius=minor,
                  major_segments=major_segments, minor_segments=minor_segments,
                  location=(0.0, 0.0, 0.0), material=material)
    obj.scale = (guard_height, guard_length, flatten)
    obj.rotation_euler = (0.0, math.radians(90.0), 0.0)
    obj.location = location
    K.transform_apply(obj)
    return obj


def _hoop(
    name: str,
    major: float,
    minor: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    stretch: float = 1.0,
    major_segments: int = 20,
    minor_segments: int = 6,
) -> bpy.types.Object:
    """A ring standing in the X-Z plane, i.e. facing forward down -Y.

    Rotating a `K.torus` +90 degrees about X sends its plane from XY to XZ
    and its axis from +Z to -Y, which is exactly a front-sight hoop or a pull
    ring seen from behind.
    """
    obj = K.torus(name, major_radius=major, minor_radius=minor,
                  major_segments=major_segments, minor_segments=minor_segments,
                  location=(0.0, 0.0, 0.0), material=material)
    obj.scale = (stretch, 1.0, 1.0)
    obj.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    obj.location = location
    K.transform_apply(obj)
    return obj


def _radial_bar(
    name: str,
    *,
    offset: float,
    y: float,
    length: float,
    thickness: float,
    width: float,
    phi_deg: float,
    material: bpy.types.Material | None,
    bevel: float = 0.008,
    z: float = 0.0,
) -> bpy.types.Object:
    """A bar running parallel to Y at angular station `phi_deg` around Y.

    Round furniture — handguards, launcher tubes, flared mouths — carries its
    detail as a ring of bars at fixed angular stations, which is what makes a
    tube read as a machined shroud. `offset` is the bar's CENTRE distance
    from the Y axis, so a bar hugging a surface of radius R wants
    `offset = R + thickness / 2` (or slightly less, to bite in).

    The bar is built at the world origin, swung about the Y axis to its
    station, and only then pushed out to `offset`; translating first and
    rotating after would swing it through the tube's own axis and bury it.

    **`z` is the axis height and it defaults to 0, which is a trap.** This
    helper builds in a plane through the WORLD ORIGIN, so a caller with a tube
    whose axis sits at bore height gets a ring of bars down at z=0 unless it
    passes `z` explicitly. `build_tank`'s 16 bell gussets were built that way
    and ended up scattered around the origin at radii of 0.006-0.174 m,
    reading in the preview as loose pale chips falling off the launcher.
    Every caller passes `z` now; the default stays 0 so the existing
    z-at-origin callers keep working unchanged.
    """
    phi = math.radians(phi_deg)
    obj = K.cube(name, size=(thickness, length, width),
                 location=(0.0, 0.0, 0.0), material=material,
                 bevel=_safe_bevel(bevel, (thickness, length, width)),
                 bevel_segments=1)
    obj.rotation_euler = (0.0, -phi, 0.0)
    K.transform_apply(obj)
    obj.location = (offset * math.cos(phi), y, z + offset * math.sin(phi))
    K.transform_apply(obj)
    return obj


def _rail(
    prefix: str,
    length: float,
    location: Sequence[float],
    material: bpy.types.Material | None,
    *,
    teeth: int = 7,
    pitch: float = 0.013,
    width: float = 0.030,
    height: float = 0.008,
    tooth: float = 0.005,
) -> list[bpy.types.Object]:
    """An accessory rail: a base plate plus real teeth.

    The teeth are geometry, not a texture. On a hand prop the camera gets
    close enough that a painted rail reads as a smudge, and the tooth row is
    what tells the eye "this thing takes accessories" — i.e. that it is a
    modern machined weapon rather than a stick.
    """
    x, y, z = location
    parts = [_block(f"{prefix}_base", (width, length, height), location, material)]
    for i in range(teeth):
        parts.append(_block(
            f"{prefix}_tooth{i}",
            (width * 0.64, tooth, height * 0.85),
            (x,
             y - length * 0.5 + pitch * (i + 0.6),
             z + height * 0.8),
            material))
    return parts


def _ribbed_grip(
    prefix: str,
    *,
    radius: float,
    y: float,
    z_bottom: float,
    length: float,
    material: bpy.types.Material | None,
    ribs: int = 5,
    rib_height: float = 0.010,
) -> list[bpy.types.Object]:
    """A vertical launcher grip: a ribbed sleeve spanning z_bottom..top.

    `z_bottom` is the SOLE of the grip, stated directly rather than derived
    from a centre, because a launcher stands on its grips and the grip sole
    is what has to land on z = 0.
    """
    z_centre = z_bottom + length * 0.5
    parts = [_post(f"{prefix}_sleeve", radius, length,
                   (0.0, y, z_centre), material, vertices=28)]
    pitch = length / (ribs + 1)
    for i in range(ribs):
        parts.append(_post(f"{prefix}_rib{i}", radius * 1.10, rib_height,
                           (0.0, y, z_bottom + pitch * (i + 1)),
                           material, vertices=24))
    return parts


def _ground(parts: Sequence[bpy.types.Object]) -> list[bpy.types.Object]:
    """Drop the assembly so its lowest point rests exactly on z = 0.

    Each weapon is authored level and lands on whichever furniture is lowest
    — a magwell base plate, a grip heel, a launcher grip — and which part that
    is falls out of the arithmetic rather than being declared. So the drop is
    MEASURED off the real vertex data instead of assumed: one world-space read
    per vertex, one translate, applied through the kernel so the offset is
    baked into the mesh rather than left as an object location.

    (`K.assert_grounded` asserts this per OBJECT, so on a multi-part asset
    only the single lowest part satisfies it — this walks all parts together,
    which is what the contract means for a multi-part prop.)
    """
    meshes = [o for o in parts if o is not None and o.type == "MESH"]
    if not meshes:
        return list(parts)
    bpy.context.view_layer.update()
    lowest = min((obj.matrix_world @ v.co).z for obj in meshes for v in obj.data.vertices)
    drop = -lowest
    if abs(drop) < 1e-9:
        return meshes
    for obj in meshes:
        obj.location = (obj.location.x, obj.location.y, obj.location.z + drop)
        K.transform_apply(obj)
    return meshes


# ---------------------------------------------------------------------------
# 1. pistol — generic service handgun
# ---------------------------------------------------------------------------

def build_pistol(
    asset_id: str,
    *,
    bore_height: float = 0.122,
    muzzle_y: float = -0.112,
    grip_rake: float = 18.0,
    accent: Sequence[float] = (0.86, 0.30, 0.12),
) -> list[bpy.types.Object]:
    """A generic semi-automatic pistol: slide, frame, raked grip, sights.

    Proportion: the slide is ~74% of the overall length and the bore sits at
    roughly three quarters of the total height, so the weapon's mass is up
    and forward — the silhouette that says "pistol" before any detail lands.
    The magwell base plate is the broad flat face that rests on z = 0.
    """
    K.reset_scene()

    steel = _gunmetal(f"{asset_id}_gunmetal")
    polymer = _polymer(f"{asset_id}_polymer")
    bright = _bright(f"{asset_id}_bright")
    stripe = _accent(f"{asset_id}_accent", accent)
    dark = _bore(f"{asset_id}_bore")

    bh = bore_height
    slide_len = 0.170
    slide_cy = muzzle_y + slide_len * 0.5 + 0.005
    rake = grip_rake

    parts: list[bpy.types.Object] = []

    # --- slide + barrel: the machined upper, everything above the frame ----
    parts.append(_rbox(f"{asset_id}_slide", (0.036, slide_len, 0.034),
                       (0.0, slide_cy, bh), steel, bevel=0.014, bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_slide_crown", (0.030, 0.014, 0.020),
                       (0.0, muzzle_y - 0.008, bh + 0.004), steel,
                       bevel=0.010, bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_bushing", (0.026, 0.010, 0.026),
                       (0.0, muzzle_y - 0.001, bh), bright,
                       bevel=0.008, bevel_segments=2))
    parts.append(_tube(f"{asset_id}_barrel_crown", 0.0086, 0.016,
                       (0.0, muzzle_y - 0.004, bh), bright, vertices=28))
    parts.append(_tube(f"{asset_id}_bore", 0.0060, 0.006,
                       (0.0, muzzle_y - 0.007, bh), dark, vertices=24))
    # ejection port + slide release: the two recesses that make an upper read
    # as a slide rather than a brick
    parts.append(_rbox(f"{asset_id}_ejection_port", (0.008, 0.034, 0.016),
                       (0.017, slide_cy + 0.045, bh + 0.004), dark,
                       bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_slide_release", (0.009, 0.016, 0.010),
                       (0.017, slide_cy + 0.010, bh - 0.008), steel,
                       bevel=0.008, bevel_segments=1))
    # machining marks down the rear of each side: shallow raised ribs
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        for i in range(5):
            parts.append(_block(
                f"{asset_id}_serration_{tag}{i}",
                (0.004, 0.005, 0.024),
                (sx * 0.017, slide_cy + 0.052 + i * 0.0085, bh),
                steel))

    # --- frame: the polymer lower the slide rides on -----------------------
    parts.append(_rbox(f"{asset_id}_frame", (0.032, 0.172, 0.030),
                       (0.0, slide_cy, bh - 0.030), steel, bevel=0.012,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_dust_cover", (0.030, 0.030, 0.016),
                       (0.0, slide_cy + 0.056, bh - 0.014), polymer,
                       bevel=0.010, bevel_segments=2))
    parts.append(_block(f"{asset_id}_accessory_rail", (0.024, 0.044, 0.006),
                        (0.0, slide_cy + 0.056, bh - 0.024), dark))
    parts.append(_block(f"{asset_id}_frame_strut", (0.006, 0.060, 0.012),
                        (-0.016, slide_cy + 0.020, bh - 0.034), polymer))

    # --- sights: a blade up front, a notched block behind ------------------
    parts.append(_rbox(f"{asset_id}_sight_front", (0.008, 0.012, 0.011),
                       (0.0, muzzle_y + 0.028, bh + 0.022), steel,
                       bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_sight_rear", (0.026, 0.014, 0.012),
                       (0.0, slide_cy + 0.060, bh + 0.022), steel,
                       bevel=0.008, bevel_segments=1))
    parts.append(_block(f"{asset_id}_sight_notch", (0.008, 0.008, 0.008),
                        (0.0, slide_cy + 0.060, bh + 0.024), dark))

    # --- trigger + guard ---------------------------------------------------
    parts.append(_rbox(f"{asset_id}_trigger", (0.008, 0.010, 0.024),
                       (0.0, slide_cy + 0.030, bh - 0.048), bright,
                       rot=(10.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    guard_z = bh - 0.058
    parts.append(_bow(f"{asset_id}_trigger_guard", 0.0175, 0.0042,
                      (0.0, slide_cy + 0.024, guard_z), bright,
                      guard_height=0.95, guard_length=1.30, flatten=0.45))
    parts.append(_block(f"{asset_id}_guard_bridge", (0.020, 0.016, 0.016),
                        (0.0, slide_cy + 0.030, guard_z + 0.016), steel))

    # --- grip + magazine: the raked polymer mass under the frame ----------
    grip_cy = 0.062
    grip_cz = bh - 0.077
    parts.append(_rbox(f"{asset_id}_grip", (0.030, 0.050, 0.110),
                       (0.0, grip_cy, grip_cz), polymer,
                       rot=(rake, 0.0, 0.0), bevel=0.016, bevel_segments=3))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_rbox(
            f"{asset_id}_grip_panel_{tag}", (0.005, 0.038, 0.070),
            (sx * 0.014, grip_cy + 0.010, grip_cz - 0.004), dark,
            rot=(rake, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    # the magwell base plate: the broad flat face that rests on the ground
    parts.append(_rbox(f"{asset_id}_magwell", (0.032, 0.054, 0.018),
                       (0.0, grip_cy - 0.006, 0.009), polymer,
                       rot=(rake, 0.0, 0.0), bevel=0.012, bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_mag_lip", (0.034, 0.050, 0.005),
                       (0.0, grip_cy - 0.009, 0.000), bright,
                       rot=(rake, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_backstrap", (0.026, 0.018, 0.098),
                       (0.0, grip_cy - 0.018, grip_cz + 0.012), steel,
                       rot=(rake, 0.0, 0.0), bevel=0.012, bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_beavertail", (0.026, 0.026, 0.020),
                       (0.0, grip_cy - 0.034, grip_cz - 0.038), polymer,
                       bevel=0.010, bevel_segments=2))

    # --- small controls: the details that stop it looking CAD-default -----
    parts.append(_pin(f"{asset_id}_mag_release", 0.005, 0.022,
                      (-0.017, grip_cy + 0.012, grip_cz + 0.040), bright))
    parts.append(_pin(f"{asset_id}_takedown_pin", 0.005, 0.022,
                      (0.0, slide_cy - 0.056, bh - 0.034), bright))
    parts.append(_rbox(f"{asset_id}_hammer", (0.010, 0.014, 0.018),
                       (0.0, slide_cy + 0.086, bh - 0.004), steel,
                       bevel=0.008, bevel_segments=2))
    parts.append(_block(f"{asset_id}_stripe", (0.034, 0.008, 0.006),
                        (0.0, slide_cy + 0.020, bh + 0.019), stripe))

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# 2. SMG — compact machine pistol
# ---------------------------------------------------------------------------

def build_smg(
    asset_id: str,
    *,
    bore_height: float = 0.215,
    muzzle_y: float = -0.302,
    stock_y: float = 0.186,
    accent: Sequence[float] = (0.20, 0.78, 0.72),
) -> list[bpy.types.Object]:
    """A generic compact SMG: shrouded short barrel, railed receiver, stick mag.

    Proportion: an SMG is defined by the relationship between a SHORT barrel
    and a LONG magazine well, with the optic close over the action. The curved
    stick magazine drops well below the grip heel — a real 30-round stick mag
    hangs ~40 mm lower than the grip — and its base plate is what rests on
    z = 0, which is also why an SMG reads as an SMG.
    """
    K.reset_scene()

    steel = _gunmetal(f"{asset_id}_gunmetal")
    polymer = _polymer(f"{asset_id}_polymer")
    bright = _bright(f"{asset_id}_bright")
    stripe = _accent(f"{asset_id}_accent", accent)
    dark = _bore(f"{asset_id}_bore")

    bh = bore_height
    parts: list[bpy.types.Object] = []

    # --- receiver pair: upper (railed) over lower (magazine well) ---------
    parts.append(_rbox(f"{asset_id}_upper", (0.044, 0.230, 0.048),
                       (0.0, -0.025, bh), steel, bevel=0.014, bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_lower", (0.038, 0.150, 0.054),
                       (0.0, 0.006, bh - 0.048), steel, bevel=0.014,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_lower_step", (0.040, 0.060, 0.020),
                       (0.0, 0.048, bh - 0.076), polymer, bevel=0.010,
                       bevel_segments=2))
    parts.append(_block(f"{asset_id}_ejection_port", (0.008, 0.048, 0.022),
                        (0.021, 0.020, bh + 0.004), dark))
    parts.append(_block(f"{asset_id}_charging_handle", (0.010, 0.028, 0.012),
                        (0.024, 0.060, bh + 0.004), bright))
    parts.append(_rbox(f"{asset_id}_selector", (0.010, 0.010, 0.026),
                       (0.021, -0.020, bh - 0.036), steel,
                       rot=(0.0, 18.0, 0.0), bevel=0.008, bevel_segments=1))

    # --- barrel + vented shroud: the part that says "machine pistol" ------
    parts.append(_tube(f"{asset_id}_barrel", 0.0105, 0.140,
                       (0.0, -0.205, bh), steel, vertices=36))
    parts.append(_flare(f"{asset_id}_muzzle_cone", 0.0105, 0.0152, 0.030,
                        (0.0, muzzle_y + 0.016, bh), bright, vertices=36))
    parts.append(_tube(f"{asset_id}_muzzle_bore", 0.0090, 0.006,
                       (0.0, muzzle_y + 0.001, bh), dark, vertices=24))
    parts.append(_tube(f"{asset_id}_shroud", 0.0190, 0.126,
                       (0.0, -0.199, bh), polymer, vertices=36))
    for i in range(6):
        parts.append(_radial_bar(
            f"{asset_id}_shroud_rib{i}", offset=0.0190, y=-0.199,
            length=0.112, thickness=0.008, width=0.015,
            phi_deg=30.0 * i, material=steel, z=bh))
    for tag, yy in (("front", -0.258), ("rear", -0.140)):
        parts.append(_tube(f"{asset_id}_shroud_band_{tag}", 0.0212, 0.010,
                           (0.0, yy, bh), bright, vertices=36))

    # --- magazine well + curved stick magazine ---------------------------
    parts.append(_rbox(f"{asset_id}_magwell", (0.030, 0.058, 0.038),
                       (0.0, -0.014, bh - 0.082), polymer, bevel=0.012,
                       bevel_segments=2))
    for i, (rake, cz, cy) in enumerate((
        (8.0, 0.092, -0.020),
        (18.0, 0.050, -0.034),
        (28.0, 0.012, -0.053),
    )):
        parts.append(_rbox(f"{asset_id}_mag_seg{i}", (0.026, 0.050, 0.052),
                           (0.0, cy, cz), polymer,
                           rot=(-rake, 0.0, 0.0), bevel=0.012, bevel_segments=2))
        parts.append(_rbox(f"{asset_id}_mag_window{i}", (0.006, 0.030, 0.014),
                           (0.0, cy - 0.012, cz + 0.010), dark,
                           rot=(-rake, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_mag_base", (0.030, 0.048, 0.014),
                       (0.0, -0.064, -0.018), polymer,
                       rot=(-28.0, 0.0, 0.0), bevel=0.010, bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_mag_base_lip", (0.032, 0.044, 0.005),
                       (0.0, -0.067, -0.025), bright,
                       rot=(-28.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))

    # --- pistol grip + trigger group -------------------------------------
    grip_cy, grip_cz = 0.082, bh - 0.112
    parts.append(_rbox(f"{asset_id}_grip", (0.030, 0.046, 0.108),
                       (0.0, grip_cy, grip_cz), polymer,
                       rot=(14.0, 0.0, 0.0), bevel=0.016, bevel_segments=3))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_rbox(
            f"{asset_id}_grip_panel_{tag}", (0.005, 0.034, 0.064),
            (sx * 0.014, grip_cy + 0.008, grip_cz - 0.004), dark,
            rot=(14.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_grip_heel", (0.032, 0.046, 0.016),
                       (0.0, grip_cy - 0.012, grip_cz - 0.056), polymer,
                       bevel=0.010, bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_trigger", (0.008, 0.010, 0.024),
                       (0.0, 0.046, bh - 0.096), bright,
                       rot=(10.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_bow(f"{asset_id}_trigger_guard", 0.0190, 0.0044,
                      (0.0, 0.038, bh - 0.102), bright,
                      guard_height=1.00, guard_length=1.30, flatten=0.45))
    parts.append(_block(f"{asset_id}_safety_block", (0.020, 0.014, 0.028),
                        (0.0, 0.062, bh - 0.074), polymer))

    # --- stock: a tube into a shoulder piece -----------------------------
    parts.append(_tube(f"{asset_id}_stock_tube", 0.0125, 0.090,
                       (0.0, 0.112, bh - 0.026), polymer, vertices=28))
    parts.append(_rbox(f"{asset_id}_stock", (0.034, 0.086, 0.058),
                       (0.0, 0.150, bh - 0.028), polymer, bevel=0.014,
                       bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_cheek_rest", (0.030, 0.060, 0.014),
                       (0.0, 0.146, bh + 0.004), polymer, bevel=0.010,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_butt_pad", (0.038, 0.018, 0.070),
                       (0.0, stock_y, bh - 0.030), P.rubber(f"{asset_id}_rubber"),
                       bevel=0.012, bevel_segments=2))

    # --- sights: rail + rear block + hooped front post --------------------
    parts += _rail(f"{asset_id}_rail", 0.150, (0.0, -0.050, bh + 0.026),
                   steel, teeth=9, pitch=0.014, width=0.030, height=0.008)
    parts.append(_rbox(f"{asset_id}_rear_sight", (0.032, 0.022, 0.024),
                       (0.0, 0.036, bh + 0.044), steel, bevel=0.010,
                       bevel_segments=2))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_block(
            f"{asset_id}_rear_aperture_{tag}", (0.007, 0.010, 0.010),
            (sx * 0.008, 0.036, bh + 0.044), dark))
    parts.append(_post(f"{asset_id}_front_post", 0.0035, 0.018,
                       (0.0, -0.118, bh + 0.037), bright, vertices=16))
    parts.append(_hoop(f"{asset_id}_front_hoop", 0.0105, 0.0024,
                       (0.0, -0.118, bh + 0.034), steel))
    parts.append(_block(f"{asset_id}_stripe", (0.042, 0.010, 0.006),
                        (0.0, -0.070, bh + 0.023), stripe))
    parts.append(_pin(f"{asset_id}_takedown_pin", 0.005, 0.048,
                      (0.0, -0.090, bh - 0.034), bright))

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# 3. pump shotgun
# ---------------------------------------------------------------------------

def build_shotgun(
    asset_id: str,
    *,
    bore_height: float = 0.150,
    muzzle_y: float = -0.422,
    butt_y: float = 0.292,
) -> list[bpy.types.Object]:
    """A generic pump-action shotgun: long barrel over a magazine tube.

    Proportion: a shotgun is a LONG tube with a second, shorter tube slung
    beneath it, a slab receiver between them, and a comb that drops to the
    butt. The two soles — the grip heel and the recoil pad — are set to the
    same height so the weapon sits on the ground on two contact points
    instead of balancing on the grip.
    """
    K.reset_scene()

    steel = _gunmetal(f"{asset_id}_gunmetal")
    polymer = _polymer(f"{asset_id}_polymer")
    bright = _bright(f"{asset_id}_bright")
    rubber = P.rubber(f"{asset_id}_rubber")
    dark = _bore(f"{asset_id}_bore")

    bh = bore_height
    tube_z = bh - 0.043
    parts: list[bpy.types.Object] = []

    # --- barrel over magazine tube ---------------------------------------
    parts.append(_tube(f"{asset_id}_barrel", 0.0145, 0.360,
                       (0.0, -0.220, bh), steel, vertices=40))
    parts.append(_flare(f"{asset_id}_muzzle_crown", 0.0145, 0.0182, 0.024,
                        (0.0, muzzle_y + 0.012, bh), bright, vertices=40))
    parts.append(_tube(f"{asset_id}_barrel_bore", 0.0130, 0.008,
                       (0.0, muzzle_y + 0.001, bh), dark, vertices=28))
    parts.append(_tube(f"{asset_id}_mag_tube", 0.0118, 0.300,
                       (0.0, -0.195, tube_z), steel, vertices=32))
    parts.append(_flare(f"{asset_id}_mag_tube_cap", 0.0118, 0.0075, 0.026,
                        (0.0, -0.356, tube_z), bright, vertices=28))
    parts.append(_tube(f"{asset_id}_barrel_band", 0.0196, 0.018,
                       (0.0, -0.092, bh - 0.021), bright, vertices=36))
    parts.append(_tube(f"{asset_id}_gas_block", 0.0175, 0.048,
                       (0.0, -0.070, bh - 0.016), steel, vertices=32))
    # a longitudinal rib along the top of the barrel — the ribbed-barrel
    # silhouette, generic in form
    parts.append(_block(f"{asset_id}_barrel_rib", (0.010, 0.300, 0.006),
                        (0.0, -0.235, bh + 0.014), steel))

    # --- pump fore-end: the ribbed polymer sleeve over the mag tube ------
    # the slide bar bridges the receiver to the fore-end, or the pump floats
    parts.append(_block(f"{asset_id}_pump_slide", (0.026, 0.160, 0.014),
                        (0.0, -0.118, tube_z), steel))
    parts.append(_tube(f"{asset_id}_fore_end", 0.0208, 0.118,
                       (0.0, -0.172, tube_z), polymer, vertices=36))
    for i in range(7):
        parts.append(_tube(f"{asset_id}_fore_rib{i}", 0.0224, 0.006,
                           (0.0, -0.218 + i * 0.0152, tube_z), polymer,
                           vertices=32))
    for tag, yy in (("front", -0.232), ("rear", -0.112)):
        parts.append(_tube(f"{asset_id}_fore_band_{tag}", 0.0232, 0.010,
                           (0.0, yy, tube_z), bright, vertices=36))

    # --- receiver slab: the machined heart between barrel and stock -------
    parts.append(_rbox(f"{asset_id}_receiver", (0.036, 0.118, 0.050),
                       (0.0, 0.016, bh - 0.034), steel, bevel=0.014,
                       bevel_segments=3))
    parts.append(_block(f"{asset_id}_action_rib", (0.026, 0.086, 0.010),
                        (0.0, 0.014, bh - 0.006), steel))
    parts.append(_block(f"{asset_id}_ejection_port", (0.008, 0.062, 0.020),
                        (0.018, 0.034, bh - 0.034), dark))
    parts.append(_block(f"{asset_id}_loading_port", (0.024, 0.046, 0.008),
                        (0.0, -0.008, bh - 0.062), dark))
    parts.append(_block(f"{asset_id}_shell_latch", (0.012, 0.012, 0.028),
                        (0.018, -0.030, bh - 0.046), bright))

    # --- trigger group ----------------------------------------------------
    parts.append(_rbox(f"{asset_id}_trigger", (0.008, 0.010, 0.024),
                       (0.0, 0.006, bh - 0.076), bright,
                       rot=(10.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_bow(f"{asset_id}_trigger_guard", 0.0195, 0.0045,
                      (0.0, -0.002, bh - 0.082), bright,
                      guard_height=1.00, guard_length=1.30, flatten=0.45))
    parts.append(_block(f"{asset_id}_guard_bridge", (0.022, 0.020, 0.016),
                        (0.0, 0.006, bh - 0.060), steel))

    # --- grip: raked polymer handhold with a FLAT heel sole ---------------
    grip_cy = 0.086
    grip_cz = bh - 0.078
    parts.append(_rbox(f"{asset_id}_grip", (0.030, 0.052, 0.104),
                       (0.0, grip_cy, grip_cz), polymer,
                       rot=(16.0, 0.0, 0.0), bevel=0.016, bevel_segments=3))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_rbox(
            f"{asset_id}_grip_panel_{tag}", (0.005, 0.040, 0.062),
            (sx * 0.014, grip_cy + 0.008, grip_cz + 0.004), dark,
            rot=(16.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_grip_heel", (0.032, 0.052, 0.024),
                       (0.0, grip_cy - 0.010, 0.012), polymer,
                       bevel=0.012, bevel_segments=2))

    # --- stock: a comb that drops to a flat recoil pad sole --------------
    parts.append(_rbox(f"{asset_id}_stock", (0.034, 0.150, 0.058),
                       (0.0, 0.188, bh - 0.062), polymer,
                       rot=(7.0, 0.0, 0.0), bevel=0.016, bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_stock_comb", (0.030, 0.090, 0.016),
                       (0.0, 0.180, bh - 0.028), polymer, bevel=0.010,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_stock_heel_plate", (0.036, 0.044, 0.052),
                       (0.0, butt_y - 0.030, 0.031), polymer, bevel=0.012,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_recoil_pad", (0.040, 0.024, 0.062),
                       (0.0, butt_y, 0.031), rubber, bevel=0.014,
                       bevel_segments=3))
    for i in range(3):
        parts.append(_block(
            f"{asset_id}_pad_ridge{i}", (0.036, 0.005, 0.012),
            (0.0, butt_y + 0.012, 0.014 + i * 0.016), dark))

    # --- bead sight + sling swivels --------------------------------------
    parts.append(_post(f"{asset_id}_bead_base", 0.0060, 0.014,
                       (0.0, 0.062, bh + 0.008), bright, vertices=16))
    parts.append(K.sphere(f"{asset_id}_bead", radius=1.0, segments=16,
                          ring_count=8, location=(0.0, 0.062, bh + 0.020),
                          material=bright,
                          scale=(0.0045, 0.0045, 0.0045)))
    parts.append(_hoop(f"{asset_id}_swivel_front", 0.0100, 0.0022,
                       (0.0, -0.338, tube_z - 0.012), bright))
    parts.append(_hoop(f"{asset_id}_swivel_rear", 0.0100, 0.0022,
                       (0.0, 0.244, bh - 0.086), bright))

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# 4. shoulder launcher — sealed tube, rear venturi, flared muzzle
# ---------------------------------------------------------------------------

def build_rocket_launcher(
    asset_id: str,
    *,
    bore_height: float = 0.200,
    muzzle_y: float = -0.536,
    venturi_y: float = 0.360,
    accent: Sequence[float] = (0.88, 0.66, 0.18),
) -> list[bpy.types.Object]:
    """A generic shoulder-fired tube launcher: sealed tube, rear venturi,
    flared muzzle, optic block, two forward grips, blast deflector.

    Proportion: the launcher is a thick tube whose *silhouette changes along
    its length* — a venturi cone at the back, a straight sealed body, a
    flared muzzle. That profile change is the whole read, which is why the
    rear and front sections are real cones (`_flare`) and not stacked
    cylinders. The two vertical grips share one sole height, so the launcher
    stands on z = 0 on two contact points like a bipod.
    """
    K.reset_scene()

    steel = _gunmetal(f"{asset_id}_gunmetal")
    polymer = _polymer(f"{asset_id}_polymer")
    bright = _bright(f"{asset_id}_bright")
    stripe = _accent(f"{asset_id}_accent", accent)
    lens = P.glass(f"{asset_id}_lens")
    rubber = P.rubber(f"{asset_id}_rubber")

    bh = bore_height
    tube_r = 0.0520
    tube_bottom = bh - tube_r
    grip_z0 = 0.012
    parts: list[bpy.types.Object] = []

    # --- the tube: venturi cone, sealed body, flared muzzle ---------------
    # the venturi tapers from the tube's own radius down to a narrow tail
    parts.append(_flare(f"{asset_id}_venturi", 0.0300, tube_r, 0.110,
                        (0.0, venturi_y - 0.055, bh), steel, vertices=44))
    parts.append(_tube(f"{asset_id}_venturi_rim", 0.0300, 0.016,
                       (0.0, venturi_y + 0.004, bh), bright, vertices=44))
    parts.append(_tube(f"{asset_id}_tube", tube_r, 0.760,
                       (0.0, -0.100, bh), steel, vertices=48))
    parts.append(_flare(f"{asset_id}_muzzle_bell", tube_r, 0.0705, 0.090,
                        (0.0, muzzle_y + 0.048, bh), steel, vertices=48))
    parts.append(_tube(f"{asset_id}_muzzle_rim", 0.0705, 0.018,
                       (0.0, muzzle_y + 0.009, bh), bright, vertices=48))
    parts.append(_tube(f"{asset_id}_muzzle_bore", 0.0660, 0.006,
                       (0.0, muzzle_y + 0.001, bh), rubber, vertices=40))
    for tag, yy in (("front", -0.430), ("rear", 0.220)):
        parts.append(_tube(f"{asset_id}_tube_band_{tag}", 0.0562, 0.020,
                           (0.0, yy, bh), bright, vertices=44))

    # --- blast deflector: a wedge plate behind the muzzle plus two wings --
    parts.append(_rbox(f"{asset_id}_blast_plate", (0.092, 0.016, 0.120),
                       (0.0, muzzle_y + 0.036, bh + 0.062), steel,
                       rot=(38.0, 0.0, 0.0), bevel=0.012, bevel_segments=2))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_rbox(
            f"{asset_id}_blast_wing_{tag}", (0.014, 0.090, 0.078),
            (sx * 0.050, muzzle_y + 0.060, bh + 0.030), steel,
            rot=(30.0, 0.0, 0.0), bevel=0.012, bevel_segments=2))

    # --- side rails with real teeth --------------------------------------
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_block(f"{asset_id}_side_rail_{tag}",
                            (0.012, 0.420, 0.026),
                            (sx * 0.055, -0.060, bh - 0.014), steel))
        for i in range(7):
            parts.append(_block(
                f"{asset_id}_side_tooth_{tag}{i}",
                (0.010, 0.008, 0.018),
                (sx * 0.060, -0.252 + i * 0.016, bh - 0.014), steel))

    # --- top optic: base, housing, lens, eyecup ---------------------------
    parts += _rail(f"{asset_id}_top_rail", 0.170, (0.0, -0.060, bh + 0.054),
                   steel, teeth=8, pitch=0.016, width=0.038, height=0.010)
    parts.append(_rbox(f"{asset_id}_optic_mount", (0.044, 0.046, 0.024),
                       (0.0, -0.060, bh + 0.068), steel, bevel=0.010,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_optic_housing", (0.048, 0.140, 0.052),
                       (0.0, -0.060, bh + 0.106), steel, bevel=0.014,
                       bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_optic_eyecup", (0.054, 0.026, 0.058),
                       (0.0, 0.014, bh + 0.106), polymer, bevel=0.012,
                       bevel_segments=2))
    parts.append(_tube(f"{asset_id}_optic_lens", 0.0205, 0.008,
                       (0.0, -0.128, bh + 0.106), lens, vertices=32))
    parts.append(_tube(f"{asset_id}_optic_lens_rim", 0.0240, 0.010,
                       (0.0, -0.124, bh + 0.106), bright, vertices=32))
    parts.append(_block(f"{asset_id}_reticle_block", (0.020, 0.014, 0.020),
                        (0.0, -0.018, bh + 0.106), bright))

    # --- front and rear grips: both soles level, so the tube rests on both
    for tag, yy, rr in (("front", -0.170, 0.0175), ("rear", 0.150, 0.0185)):
        parts += _ribbed_grip(f"{asset_id}_grip_{tag}", radius=rr, y=yy,
                              z_bottom=grip_z0, length=tube_bottom - grip_z0 - 0.006,
                              material=polymer, ribs=4)
        # the bridge is deliberately tall: it must bite into BOTH the tube's
        # underside and the top of the grip sleeve
        parts.append(_rbox(f"{asset_id}_grip_bridge_{tag}",
                           (0.040, 0.034, 0.036),
                           (0.0, yy, tube_bottom - 0.002), steel,
                           bevel=0.010, bevel_segments=2))

    # --- trigger assembly + pistol grip + shoulder pad -------------------
    parts.append(_rbox(f"{asset_id}_trigger_housing", (0.030, 0.052, 0.048),
                       (0.0, 0.086, 0.152), steel, bevel=0.012,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_trigger", (0.010, 0.012, 0.030),
                       (0.0, 0.078, 0.140), bright,
                       rot=(12.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_bow(f"{asset_id}_trigger_guard", 0.0210, 0.0050,
                      (0.0, 0.070, 0.126), bright,
                      guard_height=1.05, guard_length=1.30, flatten=0.45))
    parts.append(_rbox(f"{asset_id}_pistol_grip", (0.032, 0.048, 0.100),
                       (0.0, 0.136, 0.090), polymer,
                       rot=(14.0, 0.0, 0.0), bevel=0.014, bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_pistol_grip_cap", (0.034, 0.048, 0.014),
                       (0.0, 0.144, 0.042), polymer, bevel=0.010,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_shoulder_pad", (0.074, 0.076, 0.032),
                       (0.0, 0.392, bh - 0.028), polymer, bevel=0.016,
                       bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_shoulder_face", (0.070, 0.020, 0.062),
                       (0.0, 0.428, bh - 0.028), rubber, bevel=0.014,
                       bevel_segments=2))

    # --- foresight, stripe, sling eyes ----------------------------------
    parts.append(_post(f"{asset_id}_front_sight_post", 0.0045, 0.026,
                       (0.0, muzzle_y + 0.098, bh + 0.076), bright, vertices=16))
    parts.append(_block(f"{asset_id}_front_sight_guard", (0.034, 0.010, 0.026),
                        (0.0, muzzle_y + 0.098, bh + 0.064), steel))
    parts.append(_tube(f"{asset_id}_stripe_band", 0.0548, 0.026,
                       (0.0, -0.330, bh), stripe, vertices=44))
    for tag, yy, zz in (("front", muzzle_y + 0.040, bh - 0.060),
                        ("rear", 0.300, bh - 0.030)):
        parts.append(_hoop(f"{asset_id}_sling_eye_{tag}", 0.0115, 0.0024,
                           (0.0, yy, zz), bright))

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# 5. long launcher tube — venturi rear, deep bell mouth
# ---------------------------------------------------------------------------

def build_tank(
    asset_id: str,
    *,
    bore_height: float = 0.235,
    muzzle_y: float = -0.610,
    venturi_y: float = 0.430,
    grip_colour: Sequence[float] = (0.13, 0.14, 0.17),
    accent: Sequence[float] = (0.90, 0.52, 0.14),
) -> list[bpy.types.Object]:
    """A long shoulder-fired launcher tube: venturi rear, deep conical bell
    mouth, optic housing, grip and trigger assembly, blast deflector.

    Deliberately a different design language from `build_rocket_launcher`,
    not a variant of it: this one is LONGER and slimmer, its front section is
    a long steep bell whose radius more than doubles rather than a short lip,
    its rear venturi is a long taper rather than a stub, and the optic is a
    raked housing with a rubber eyecup instead of a straight tube.

    The bell and the venturi are real cones (`_flare`) with a big radius
    ratio, because "a cylinder with a slightly fatter cylinder stuck on the
    end" is the single most common tell that a launcher was blockout
    geometry. Two rings of radial gussets sit on the outside of the flare —
    placed at *stations* with the local cone radius, since a straight rib
    cannot follow a cone and would float off its surface.

    The bell's rear edge is buried ~40 mm INSIDE the tube, and at that depth
    the cone's radius still equals the tube's, so the flare grows out of the
    tube with no step and no gap.
    """
    K.reset_scene()

    steel = _gunmetal(f"{asset_id}_gunmetal")
    bell = P.metal(f"{asset_id}_bell", rgb=(0.72, 0.75, 0.80), rough=0.11,
                   metal=1.0)
    polymer = P.paint(f"{asset_id}_grip", rgb=grip_colour, metallic=0.0,
                      roughness=0.70)
    panel = _polymer(f"{asset_id}_panel")
    stripe = _accent(f"{asset_id}_accent", accent)
    lens = P.glass(f"{asset_id}_lens")
    rubber = P.rubber(f"{asset_id}_rubber")

    bh = bore_height
    tube_r = 0.0455
    tube_bottom = bh - tube_r
    grip_z0 = 0.012

    # the tube runs y = tube_front .. tube_rear; the bell grows forward off it
    tube_front, tube_rear = -0.340, 0.300
    bell_len, bell_r = 0.230, 0.1010
    bell_front, bell_rear = muzzle_y, muzzle_y + bell_len
    vent_len, vent_r = 0.160, 0.0198
    vent_front, vent_rear = venturi_y - vent_len, venturi_y

    parts: list[bpy.types.Object] = []

    # --- the tube: sealed body, long venturi taper, deep bell mouth ------
    parts.append(_tube(f"{asset_id}_tube", tube_r, tube_rear - tube_front,
                       (0.0, (tube_front + tube_rear) * 0.5, bh), steel,
                       vertices=48))
    parts.append(_flare(f"{asset_id}_venturi", vent_r, tube_r, vent_len,
                        (0.0, (vent_front + vent_rear) * 0.5, bh), steel,
                        vertices=48))
    parts.append(_tube(f"{asset_id}_venturi_rim", vent_r, 0.018,
                       (0.0, venturi_y, bh), bell, vertices=44))
    parts.append(_flare(f"{asset_id}_bell_mouth", tube_r, bell_r, bell_len,
                        (0.0, (bell_front + bell_rear) * 0.5, bh), bell,
                        vertices=52))
    parts.append(_tube(f"{asset_id}_bell_lip", bell_r, 0.018,
                       (0.0, bell_front + 0.009, bh), bell, vertices=52))
    parts.append(_tube(f"{asset_id}_bell_bore", 0.0880, 0.010,
                       (0.0, bell_front + 0.012, bh), rubber, vertices=44))
    # gusset rings on the flare: offset = the cone's LOCAL radius there, so
    # each block bites into the cone instead of floating off it.
    #
    # `_radial_bar` builds around the origin, and the bell's axis is at
    # z = bh — so every gusset MUST be raised to the bore height afterwards.
    # It was not, and the result was 16 loose blocks scattered around the
    # world origin at radii of 0.006-0.174 m instead of a ring at 0.078 m:
    # in the preview they read as detached pale chips raining off the
    # launcher ("scattered pale fragments", 887 zero-area faces nearby).
    # Every other radial detail in this module goes through `_radial_bar`
    # too, so the fix belongs in the helper — see its own docstring.
    for station, y_local in enumerate((bell_front + 0.095, bell_front + 0.165)):
        frac = (y_local - bell_rear) / (bell_front - bell_rear)
        r_local = tube_r + (bell_r - tube_r) * frac
        for i in range(8):
            parts.append(_radial_bar(
                f"{asset_id}_bell_gusset{station}_{i}",
                offset=r_local + 0.004, y=y_local, length=0.030,
                thickness=0.010, width=0.020, phi_deg=45.0 * i,
                material=bell, bevel=0.010, z=bh))
    for tag, yy in (("front", -0.200), ("rear", 0.240)):
        parts.append(_tube(f"{asset_id}_tube_band_{tag}", 0.0500, 0.018,
                           (0.0, yy, bh), bell, vertices=44))
    # rear pressure vents: a ring of blocks around the venturi taper
    vent_local = vent_front + 0.042
    vent_local_r = vent_r + (tube_r - vent_r) * (
        (vent_local - vent_rear) / (vent_front - vent_rear))
    for i in range(6):
        parts.append(_radial_bar(
            f"{asset_id}_vent_block{i}", offset=vent_local_r + 0.005,
            y=vent_local, length=0.026, thickness=0.014, width=0.020,
            phi_deg=60.0 * i, material=steel, z=bh))

    # --- blast deflector: wedge plate plus two canted wings --------------
    parts.append(_rbox(f"{asset_id}_blast_plate", (0.104, 0.018, 0.132),
                       (0.0, muzzle_y + 0.028, bh + 0.070), steel,
                       rot=(42.0, 0.0, 0.0), bevel=0.012, bevel_segments=2))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_rbox(
            f"{asset_id}_blast_wing_{tag}", (0.016, 0.104, 0.088),
            (sx * 0.056, muzzle_y + 0.058, bh + 0.034), steel,
            rot=(34.0, 0.0, 0.0), bevel=0.012, bevel_segments=2))

    # --- optic housing: raked block, lens, eyecup, mount -----------------
    parts += _rail(f"{asset_id}_top_rail", 0.190, (0.0, -0.020, bh + 0.047),
                   steel, teeth=9, pitch=0.016, width=0.036, height=0.010)
    parts.append(_rbox(f"{asset_id}_optic_mount", (0.046, 0.046, 0.024),
                       (0.0, -0.020, bh + 0.066), steel, bevel=0.010,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_optic_housing", (0.046, 0.152, 0.054),
                       (0.0, -0.030, bh + 0.108), steel, bevel=0.014,
                       bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_optic_rake", (0.040, 0.034, 0.048),
                       (0.0, 0.048, bh + 0.096), steel,
                       rot=(14.0, 0.0, 0.0), bevel=0.012, bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_optic_eyecup", (0.052, 0.028, 0.060),
                       (0.0, 0.074, bh + 0.086), rubber, bevel=0.012,
                       bevel_segments=2))
    parts.append(_tube(f"{asset_id}_optic_lens", 0.0198, 0.008,
                       (0.0, -0.104, bh + 0.108), lens, vertices=32))
    parts.append(_tube(f"{asset_id}_optic_lens_rim", 0.0236, 0.010,
                       (0.0, -0.100, bh + 0.108), bell, vertices=32))
    parts.append(_block(f"{asset_id}_adjust_knob", (0.016, 0.016, 0.022),
                        (0.026, -0.030, bh + 0.140), bell))

    # --- handrail down each side of the tube -----------------------------
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_block(f"{asset_id}_hand_rail_{tag}",
                            (0.012, 0.460, 0.024),
                            (sx * 0.049, -0.070, bh - 0.014), steel))
        for i in range(8):
            parts.append(_block(
                f"{asset_id}_hand_tooth_{tag}{i}",
                (0.010, 0.007, 0.016),
                (sx * 0.054, -0.276 + i * 0.016, bh - 0.014), steel))

    # --- grips, trigger assembly, shoulder rest -------------------------
    for tag, yy, rr in (("front", -0.230, 0.0168), ("rear", 0.120, 0.0178)):
        parts += _ribbed_grip(f"{asset_id}_grip_{tag}", radius=rr, y=yy,
                              z_bottom=grip_z0,
                              length=tube_bottom - grip_z0 - 0.006,
                              material=polymer, ribs=5)
        parts.append(_rbox(f"{asset_id}_grip_bridge_{tag}",
                           (0.038, 0.032, 0.034),
                           (0.0, yy, tube_bottom - 0.002), steel,
                           bevel=0.010, bevel_segments=2))

    parts.append(_rbox(f"{asset_id}_trigger_block", (0.032, 0.056, 0.050),
                       (0.0, 0.058, 0.164), steel, bevel=0.012,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_trigger", (0.010, 0.012, 0.032),
                       (0.0, 0.050, 0.152), bell,
                       rot=(12.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_bow(f"{asset_id}_trigger_guard", 0.0220, 0.0052,
                      (0.0, 0.042, 0.138), bell,
                      guard_height=1.05, guard_length=1.30, flatten=0.45))
    parts.append(_rbox(f"{asset_id}_pistol_grip", (0.034, 0.050, 0.104),
                       (0.0, 0.112, 0.098), polymer,
                       rot=(15.0, 0.0, 0.0), bevel=0.014, bevel_segments=3))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_rbox(
            f"{asset_id}_pistol_panel_{tag}", (0.005, 0.038, 0.062),
            (sx * 0.016, 0.120, 0.094), panel,
            rot=(15.0, 0.0, 0.0), bevel=0.008, bevel_segments=1))
    parts.append(_rbox(f"{asset_id}_pistol_cap", (0.036, 0.048, 0.014),
                       (0.0, 0.122, 0.046), polymer, bevel=0.010,
                       bevel_segments=2))
    parts.append(_rbox(f"{asset_id}_shoulder_rest", (0.082, 0.082, 0.038),
                       (0.0, 0.400, bh - 0.020), polymer, bevel=0.016,
                       bevel_segments=3))
    parts.append(_rbox(f"{asset_id}_shoulder_face", (0.078, 0.020, 0.070),
                       (0.0, 0.444, bh - 0.020), rubber, bevel=0.014,
                       bevel_segments=2))

    # --- foresight, stripe, data plate, sling eyes ----------------------
    parts.append(_post(f"{asset_id}_front_sight_post", 0.0048, 0.028,
                       (0.0, muzzle_y + 0.280, bh + 0.074), bell, vertices=16))
    parts.append(_block(f"{asset_id}_front_sight_guard", (0.036, 0.010, 0.028),
                        (0.0, muzzle_y + 0.280, bh + 0.060), steel))
    parts.append(_tube(f"{asset_id}_stripe_band", 0.0482, 0.030,
                       (0.0, -0.140, bh), stripe, vertices=44))
    parts.append(_block(f"{asset_id}_data_plate", (0.006, 0.054, 0.026),
                        (0.052, -0.300, bh + 0.020), bell))
    for tag, yy, zz in (("front", muzzle_y + 0.060, bh - 0.066),
                        ("rear", 0.372, bh - 0.028)):
        parts.append(_hoop(f"{asset_id}_sling_eye_{tag}", 0.0118, 0.0025,
                           (0.0, yy, zz), bell))

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# 6. bat
# ---------------------------------------------------------------------------

def build_bat(
    asset_id: str,
    *,
    height: float = 0.740,
    wood=(0.62, 0.36, 0.17),
    wood_dark=(0.40, 0.22, 0.10),
) -> list[bpy.types.Object]:
    """An ash-style bat, authored STANDING on its knob.

    The bat is the one weapon that is not a weapon-shaped tube: it is a
    turned wooden form, so it gets *turned* rather than machined — a revolved
    knob, a conical shoulder, a cylindrical barrel and a domed end, all on
    the world Z axis, which is exactly what `K.cone` / `K.cylinder` /
    `K.sphere` already are and therefore what they are for. It is authored
    upright (handle down) because that is how the game stages a melee pickup,
    and its knob sole is the broad flat face that rests on z = 0.

    Colours are plain wood tones — no logo, no brand stripe.
    """
    K.reset_scene()

    timber = P.paint(f"{asset_id}_wood", rgb=wood, metallic=0.0, roughness=0.66)
    timber_dk = P.paint(f"{asset_id}_wood_dark", rgb=wood_dark, metallic=0.0,
                        roughness=0.74)
    tape = P.fabric(f"{asset_id}_tape", rgb=(0.13, 0.14, 0.16), rough=0.90)
    band = _accent(f"{asset_id}_band", (0.88, 0.72, 0.20))

    H = height
    parts: list[bpy.types.Object] = []

    # --- knob: the sole, resting on z = 0 --------------------------------
    parts.append(K.sphere(f"{asset_id}_knob", radius=1.0, segments=32,
                          ring_count=16, location=(0.0, 0.0, 0.024),
                          material=timber,
                          scale=(0.0310, 0.0310, 0.0240)))
    parts.append(K.torus(f"{asset_id}_knob_ring", major_radius=0.0268,
                         minor_radius=0.0040, major_segments=20,
                         minor_segments=8, location=(0.0, 0.0, 0.042),
                         material=timber_dk))

    # --- grip: wrapped handle, tapering up toward the shoulder -----------
    parts.append(_post(f"{asset_id}_grip_wrap", 0.0248, 0.150,
                       (0.0, 0.0, 0.112), tape, vertices=32))
    for i in range(4):
        parts.append(_post(f"{asset_id}_wrap_rib{i}", 0.0258, 0.009,
                           (0.0, 0.0, 0.062 + i * 0.030), tape, vertices=28))
    parts.append(K.cone(f"{asset_id}_grip_upper", radius1=0.0216, radius2=0.0192,
                        depth=0.110, vertices=36, location=(0.0, 0.0, 0.232),
                        material=timber, smooth=True))
    parts.append(K.cone(f"{asset_id}_shoulder", radius1=0.0192, radius2=0.0338,
                        depth=0.078, vertices=40, location=(0.0, 0.0, 0.326),
                        material=timber, smooth=True))

    # --- barrel: the long cylindrical striking surface -------------------
    barrel_r = 0.0338
    barrel_bottom = 0.365
    barrel_top = H - 0.031
    barrel_len = barrel_top - barrel_bottom
    parts.append(_post(f"{asset_id}_barrel", barrel_r, barrel_len,
                       (0.0, 0.0, (barrel_bottom + barrel_top) * 0.5), timber,
                       vertices=44))
    parts.append(K.cone(f"{asset_id}_barrel_taper", radius1=barrel_r,
                        radius2=0.0272, depth=0.026, vertices=44,
                        location=(0.0, 0.0, H - 0.048), material=timber,
                        smooth=True))
    parts.append(K.sphere(f"{asset_id}_barrel_end", radius=1.0, segments=28,
                          ring_count=14, location=(0.0, 0.0, H - 0.020),
                          material=timber,
                          scale=(0.0292, 0.0292, 0.0200)))

    # --- two plain lacquer bands: abstract decoration, never a marking ----
    for tag, zz in (("lower", barrel_bottom + 0.030), ("upper", barrel_bottom + 0.052)):
        parts.append(K.torus(f"{asset_id}_band_{tag}", major_radius=0.0344,
                             minor_radius=0.0034, major_segments=24,
                             minor_segments=8, location=(0.0, 0.0, zz),
                             material=band))
    parts.append(K.torus(f"{asset_id}_barrel_ring", major_radius=barrel_r,
                         minor_radius=0.0032, major_segments=24, minor_segments=8,
                         location=(0.0, 0.0, H - 0.080), material=timber_dk))

    # --- two shallow longitudinal grain ridges, one per side ------------
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_block(f"{asset_id}_grain_{tag}",
                            (0.006, 0.010, barrel_len * 0.72),
                            (sx * 0.0332, 0.0, (barrel_bottom + barrel_top) * 0.5),
                            timber_dk))

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# 7. frag grenade
# ---------------------------------------------------------------------------

def build_grenade(
    asset_id: str,
    *,
    body_radius: float = 0.0445,
    body_colour=(0.26, 0.31, 0.23),
    band_colour=(0.80, 0.66, 0.20),
) -> list[bpy.types.Object]:
    """A generic hand-thrown frag: cast ellipsoidal body, fuze well, safety
    lever, pull pin.

    The body is a UV sphere squashed on Z — a CAST form, not a machined one,
    so it takes a smooth-shaded ellipsoid rather than a chamfered box, and the
    lever / pin / rings are the only bright machined parts on it. The body
    rests on z = 0 directly (its centre is one radius up, so the sole lands
    on z = 0 by construction), and the pull ring is modelled on BOTH sides so
    the silhouette survives being viewed from any angle.
    """
    K.reset_scene()

    body_mat = P.paint(f"{asset_id}_body", rgb=body_colour, metallic=0.0,
                       roughness=0.60)
    body_dk = P.paint(f"{asset_id}_body_dark", rgb=(0.16, 0.20, 0.15),
                      metallic=0.0, roughness=0.70)
    steel = P.metal(f"{asset_id}_steel", rgb=(0.72, 0.74, 0.78), rough=0.18,
                    metal=1.0)
    band = _accent(f"{asset_id}_band", band_colour)

    r = body_radius
    body_cz = r * 1.04
    parts: list[bpy.types.Object] = []

    # --- cast body: an ellipsoid with a parting seam and two lacquer bands
    parts.append(K.sphere(f"{asset_id}_body", radius=1.0, segments=40,
                          ring_count=22, location=(0.0, 0.0, body_cz),
                          material=body_mat,
                          scale=(r, r * 0.94, r * 1.04)))
    parts.append(K.torus(f"{asset_id}_body_seam", major_radius=r * 0.995,
                         minor_radius=0.0026, major_segments=36, minor_segments=8,
                         location=(0.0, 0.0, body_cz), material=body_dk))
    for tag, zz in (("hi", body_cz + r * 0.16), ("lo", body_cz - r * 0.22)):
        parts.append(K.torus(f"{asset_id}_body_band_{tag}", major_radius=r * 1.005,
                             minor_radius=0.0034, major_segments=36,
                             minor_segments=8, location=(0.0, 0.0, zz),
                             material=band))
    parts.append(K.sphere(f"{asset_id}_top_dome", radius=1.0, segments=28,
                          ring_count=12, location=(0.0, 0.0, body_cz + r * 0.94),
                          material=body_dk,
                          scale=(0.0210, 0.0210, 0.0140)))

    # --- fuze well + fuze ------------------------------------------------
    neck_z = body_cz + r * 1.02
    parts.append(K.cone(f"{asset_id}_fuze_neck", radius1=0.0230, radius2=0.0168,
                        depth=0.020, vertices=28, location=(0.0, 0.0, neck_z),
                        material=body_dk, smooth=True))
    parts.append(_post(f"{asset_id}_fuze_body", 0.0166, 0.030,
                       (0.0, 0.0, neck_z + 0.024), steel, vertices=28))
    parts.append(K.cone(f"{asset_id}_fuze_cap", radius1=0.0166, radius2=0.0104,
                        depth=0.016, vertices=28,
                        location=(0.0, 0.0, neck_z + 0.046), material=steel,
                        smooth=True))
    parts.append(_post(f"{asset_id}_fuze_crown", 0.0058, 0.010,
                       (0.0, 0.0, neck_z + 0.058), body_dk, vertices=16))

    # --- safety lever (spoon), pivot boss and pull pin ------------------
    spoon_z = body_cz + r * 0.52
    parts.append(_rbox(f"{asset_id}_spoon", (0.0075, 0.034, 0.062),
                       (0.0, -0.018, spoon_z), steel,
                       rot=(9.0, 0.0, 0.0), bevel=0.008, bevel_segments=2))
    parts.append(_hoop(f"{asset_id}_spoon_ring", 0.0108, 0.0022,
                       (0.0, -0.032, spoon_z - 0.036), steel))
    parts.append(_pin(f"{asset_id}_spoon_pivot", 0.0042, 0.036,
                      (0.0, -0.010, spoon_z + 0.034), steel, vertices=14))
    for sx, tag in ((-1.0, "l"), (1.0, "r")):
        parts.append(_pin(f"{asset_id}_pin_collar_{tag}", 0.0062, 0.012,
                          (sx * 0.0175, 0.0, spoon_z + 0.034), steel,
                          vertices=16))
        parts.append(_hoop(f"{asset_id}_pull_ring_{tag}", 0.0110, 0.0019,
                           (sx * 0.0295, 0.0, spoon_z + 0.034), steel))

    # --- cord / lanyard eye: one small detail, never a marking -----------
    parts.append(K.torus(f"{asset_id}_lanyard_eye", major_radius=0.0082,
                         minor_radius=0.0018, major_segments=16, minor_segments=6,
                         location=(0.0, 0.024, body_cz + r * 0.70), material=steel))
    parts[-1].rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(parts[-1])

    return P.finish(_ground(parts))


# ---------------------------------------------------------------------------
# the registry entry — (asset_id, builder, kwargs)
# ---------------------------------------------------------------------------

WEAPONS: list[tuple[str, Callable[..., list[bpy.types.Object]], dict]] = [
    ("wep_bat", build_bat, dict(
        height=0.740,
        wood=(0.62, 0.36, 0.17),
        wood_dark=(0.40, 0.22, 0.10),
    )),
    ("wep_grenade", build_grenade, dict(
        body_radius=0.0445,
        body_colour=(0.26, 0.31, 0.23),
        band_colour=(0.80, 0.66, 0.20),
    )),
    ("wep_pistol", build_pistol, dict(
        bore_height=0.122,
        muzzle_y=-0.112,
        grip_rake=18.0,
        accent=(0.86, 0.30, 0.12),
    )),
    ("wep_rocket_launcher", build_rocket_launcher, dict(
        bore_height=0.200,
        muzzle_y=-0.536,
        venturi_y=0.360,
        accent=(0.88, 0.66, 0.18),
    )),
    ("wep_shotgun", build_shotgun, dict(
        bore_height=0.150,
        muzzle_y=-0.422,
        butt_y=0.292,
    )),
    ("wep_smg", build_smg, dict(
        bore_height=0.215,
        muzzle_y=-0.302,
        stock_y=0.186,
        accent=(0.20, 0.78, 0.72),
    )),
    ("wep_tank", build_tank, dict(
        bore_height=0.235,
        muzzle_y=-0.610,
        venturi_y=0.430,
        grip_colour=(0.13, 0.14, 0.17),
        accent=(0.90, 0.52, 0.14),
    )),
]