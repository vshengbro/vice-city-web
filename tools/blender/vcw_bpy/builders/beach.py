"""3A beach builder — boardwalk decking, deck chairs, surfboards, umbrellas.

THE bugs this module exists to avoid
------------------------------------
Beach props are the props a camera walks *up to*. Every asset family in the
library is built for a 40 m shot; these four are built for a 1.5 m shot, which
changes what "correct" means:

1. **A surfboard is not a stretched box.** A box has no outline, no rocker and
   no fin, and at 1.5 m the player is looking straight down the stringer. So
   the board is lofted: ~26 cross-sections stacked along the length, each at
   its own width, half-thickness and *rocker offset*, which is what turns a
   flat plank into a shape you can see curving away at both ends.

2. **An umbrella canopy is not a cone.** A cone is a rigid solid; fabric hangs.
   Each panel is a grid whose Z falls by `sag * sin(pi * a)` between two ribs,
   so the eight panels scallop instead of meeting in a taut point, and each rib
   stays a crisp ridge the fabric is stretched over. That scallop is the entire
   difference between "beach umbrella" and "traffic cone with fabric".

3. **A deck plank is not a box either.** The two long edges are eased, the ends
   are rounded, the top carries raised grain lines and the screws stand proud
   of the surface. Four bevelled cubes stacked read as a bench prop; a plank
   with grain and fasteners reads as decking.

4. **Fibreglass is not painted wood.** Same builder, different response: the
   board is `P.paint` at roughness 0.12 so it takes a tight specular roll-off
   along the rails. A board at the chair's roughness reads as painted MDF.

Materials come from the shared palette (`P.fabric` for canvas, `P.metal` for
the aluminium pole, `P.paint` for both the wood and the fibreglass) so the
umbrella canopy and the deck chair can never disagree about what cotton duck
or painted pine is.

Coordinate contract: Z-up, +X right, -Y forward, metres, lowest point at z=0.
Every builder ends in `_ground()`, which measures the LOWEST point across ALL
parts — not per object — and drops the whole asset by it. Grounding parts
individually would collapse a chair: every leg would be pushed to z=0 on its
own and the seat would fall through the frame.
"""

from __future__ import annotations

import math
from typing import Callable, Sequence

import bmesh
import bpy
from mathutils import Vector

from .. import kernel as K
from .. import palette as P


# ---------------------------------------------------------------------------
# materials — shared palette only, never a private material definition
# ---------------------------------------------------------------------------


def _canvas(name: str, rgb, rough: float = 0.88) -> bpy.types.Material:
    """Sun-bleached cotton duck. Very rough, never specular, two-sided."""
    return P.fabric(f"{name}_canvas", rgb, rough=rough)


def _painted_wood(name: str, rgb, rough: float = 0.40) -> bpy.types.Material:
    """Sealed painted timber: matte-ish, a faint sheen, no metallic."""
    return P.paint(f"{name}_wood", rgb, metallic=0.0, roughness=rough)


def _aluminium(name: str, rgb=(0.68, 0.70, 0.74), rough: float = 0.30) -> bpy.types.Material:
    """Anodised pole stock. Metalness high, roughness low enough to streak."""
    return P.metal(f"{name}_alu", rgb, rough=rough, metal=0.95)


def _fibreglass(name: str, rgb, rough: float = 0.12) -> bpy.types.Material:
    """Glass-on-foam board. Glossy PBR: roughness 0.12 and no metallic.

    The rail is a 20 mm half-width cylinder, so a tight specular roll-off is
    the whole read. Raise this roughness to the chair's and the board turns
    into painted MDF at close range.
    """
    return P.paint(f"{name}_fibreglass", rgb, metallic=0.0, roughness=rough)


# ---------------------------------------------------------------------------
# curve helpers
# ---------------------------------------------------------------------------


def _table_slope(table: Sequence[tuple[float, float]], i: int) -> float:
    """Finite-difference slope of a control table at sample `i`."""
    j0 = max(0, i - 1)
    j1 = min(len(table) - 1, i + 1)
    du = table[j1][0] - table[j0][0]
    if abs(du) < 1e-9:
        return 0.0
    return (table[j1][1] - table[j0][1]) / du


def _hermite(table: Sequence[tuple[float, float]], u: float) -> float:
    """Cubic Hermite through a (u, value) control table, clamped at the ends.

    Smoothstep would be shorter, and wrong: it forces the slope to zero at
    every control value, so a surfboard outline built from it comes out as
    seven stacked bulges instead of one continuous curve. Finite-difference
    tangents keep the outline monotone in the right direction throughout.
    """
    if u <= table[0][0]:
        return table[0][1]
    if u >= table[-1][0]:
        return table[-1][1]
    for i in range(len(table) - 1):
        u0, v0 = table[i]
        u1, v1 = table[i + 1]
        if u0 <= u and u <= u1:
            span = u1 - u0
            t = (u - u0) / span
            m0 = _table_slope(table, i) * span
            m1 = _table_slope(table, i + 1) * span
            h00 = 2.0 * t ** 3 - 3.0 * t ** 2 + 1.0
            h10 = t ** 3 - 2.0 * t ** 2 + t
            h01 = -2.0 * t ** 3 + 3.0 * t ** 2
            h11 = t ** 3 - t ** 2
            return h00 * v0 + h10 * m0 + h01 * v1 + h11 * m1
    return table[-1][1]


# ---------------------------------------------------------------------------
# mesh helpers
# ---------------------------------------------------------------------------


def _oriented(obj: bpy.types.Object) -> bpy.types.Object:
    """Force a closed shell's winding outward, measured rather than assumed.

    Deliberately NOT `K.signed_volume`: that helper runs
    `bmesh.ops.recalc_face_normals` before measuring, which re-points a
    watertight shell outward no matter how it was wound — so it cannot report
    a negative volume and therefore cannot detect an inside-out shell. This
    measures the volume of the mesh AS AUTHORED and reverses the faces when
    it comes out negative.
    """
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    flip = bm.calc_volume(signed=True) < 0.0
    if flip:
        bmesh.ops.reverse_faces(bm, faces=bm.faces)
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()
    return obj


def _write_uvs(obj: bpy.types.Object, uvs: Sequence[tuple[float, float]]) -> None:
    """Attach one UV per vertex, resolved through the CURRENT loop order.

    `P.finish` only guarantees that a UV *layer* exists; a layer of zeros has
    degenerate tangents and a texture painted onto it collapses. Lofts and
    panels are parametric, so they get real coordinates here instead of a cube
    projection.
    """
    mesh = obj.data
    if not uvs:
        return
    layer = mesh.uv_layers[0] if mesh.uv_layers else mesh.uv_layers.new(name="UVMap")
    for loop in mesh.loops:
        u, v = uvs[loop.vertex_index]
        layer.data[loop.index].uv = (u, v)


def _mesh_object(
    name: str,
    verts: Sequence[Sequence[float]],
    faces: Sequence[Sequence[int]],
    material: bpy.types.Material | None,
) -> bpy.types.Object:
    """Build a mesh from raw rings and faces and link it to the scene."""
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([tuple(float(c) for c in v) for v in verts], [], [tuple(f) for f in faces])
    mesh.validate(verbose=False)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    K.assign_material(obj, material)
    return _oriented(obj)


def _loft(
    name: str,
    rings: Sequence[Sequence[Sequence[float]]],
    material: bpy.types.Material | None,
    *,
    cap_start: bool = True,
    cap_end: bool = True,
    uvs: Sequence[tuple[float, float]] | None = None,
) -> bpy.types.Object:
    """Bridge a stack of CLOSED rings into one watertight shell.

    Each ring needs the same point count; the bridge is one quad per
    (ring, point) pair and the two end loops are capped with an n-gon that
    `P.finish` triangulates. The capping is what lets `K.add_bevel` do a
    real end chamfer afterwards — a capped rim is an edge the bevel can roll.
    """
    count = len(rings[0])
    verts = [p for ring in rings for p in ring]
    faces: list[tuple[int, ...]] = []
    for r in range(len(rings) - 1):
        a = r * count
        b = (r + 1) * count
        for j in range(count):
            k = (j + 1) % count
            faces.append((a + j, a + k, b + k, b + j))
    if cap_start:
        faces.append(tuple(range(count - 1, -1, -1)))
    if cap_end:
        base = (len(rings) - 1) * count
        faces.append(tuple(range(base, base + count)))
    obj = _mesh_object(name, verts, faces, material)
    if uvs is not None:
        _write_uvs(obj, uvs)
    return obj


def _grid_shell(
    name: str,
    top: Sequence[Sequence[Sequence[float]]],
    bottom: Sequence[Sequence[Sequence[float]]],
    material: bpy.types.Material | None,
    uv_rows: Sequence[Sequence[tuple[float, float]]] | None = None,
) -> bpy.types.Object:
    """Closed shell from two matching (nu x nv) vertex grids.

    A fabric panel has to be a closed shell, not a one-sided sheet: a
    one-sided canopy has no interior, so its outwardness check is meaningless
    and it disappears when the player walks behind it. Two grids plus four
    boundary walls give it thickness and a real inside.
    """
    nu = len(top)
    nv = len(top[0])
    verts = [p for row in top for p in row]
    verts += [p for row in bottom for p in row]
    off = nu * nv
    faces: list[tuple[int, ...]] = []

    for i in range(nu - 1):
        for j in range(nv - 1):
            a = i * nv + j
            b = a + nv
            faces.append((a, b, b + 1, a + 1))
            faces.append((off + a, off + a + 1, off + b + 1, off + b))

    # Boundary walls. The two side walls at a given i are NOT the same quad
    # read backwards -- each one's outward normal has to point OUT of the
    # shell, and walking (top_i, bottom_i, bottom_i+1, top_i+1) on the j=0
    # edge points -tangential while the same order on the j=nv-1 edge points
    # INWARD. Sharing one vertex pattern gives a shell with 7 boundary edges
    # and 8 edges carrying four faces: open, non-manifold, and useless to the
    # outwardness check. Each side therefore gets its own ordering, chosen so
    # all six faces of a cell stay consistently wound.
    for i in range(nu - 1):                       # j = 0 and j = nv-1
        a = i * nv
        b = a + nv
        faces.append((a, off + a, off + b, b))                 # -tangential
        c = a + nv - 1
        d = b + nv - 1
        faces.append((c, d, off + d, off + c))                 # +tangential
    for j in range(nv - 1):                       # i = 0 and i = nu-1
        a = j
        faces.append((a, a + 1, off + a + 1, off + a))         # -radial
        b = (nu - 1) * nv + j
        faces.append((b, off + b, off + b + 1, b + 1))         # +radial

    obj = _mesh_object(name, verts, faces, material)
    if uv_rows is not None:
        flat = [uv for row in uv_rows for uv in row]
        _write_uvs(obj, flat + flat)
    return obj


def _plank(
    name: str,
    size: Sequence[float],
    centre: Sequence[float],
    rot_deg: Sequence[float] = (0.0, 0.0, 0.0),
    *,
    material: bpy.types.Material | None = None,
    bevel: float = 0.008,
    bevel_segments: int = 2,
) -> bpy.types.Object:
    """A bevelled box, rotated about its own centre and THEN placed.

    Rotation after placement spins a part around the world origin instead of
    around itself, which is how a backrest ends up metres below the deck.
    """
    obj = K.cube(
        name, size=size, location=(0.0, 0.0, 0.0), material=material,
        bevel=bevel, bevel_segments=bevel_segments,
    )
    obj.rotation_euler = tuple(math.radians(a) for a in rot_deg)
    K.transform_apply(obj)
    obj.location = tuple(float(c) for c in centre)
    K.transform_apply(obj)
    return obj


def _strut(
    name: str,
    p0: Sequence[float],
    p1: Sequence[float],
    section: Sequence[float],
    *,
    material: bpy.types.Material | None = None,
    bevel: float = 0.006,
) -> bpy.types.Object:
    """A square-section beam running from `p0` to `p1`.

    Built at the origin, oriented there, and only then moved — same rule as
    the vehicle spokes.
    """
    a = Vector((float(p0[0]), float(p0[1]), float(p0[2])))
    b = Vector((float(p1[0]), float(p1[1]), float(p1[2])))
    delta = b - a
    if delta.length < 1e-6:
        raise ValueError(f"_strut {name}: zero-length span")
    obj = K.cube(
        name, size=(section[0], section[1], delta.length), location=(0.0, 0.0, 0.0),
        material=material, bevel=bevel,
    )
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = delta.to_track_quat("Z", "Y")
    K.transform_apply(obj)
    obj.location = (a + b) * 0.5
    K.transform_apply(obj)
    return obj


def _spindle(
    name: str,
    p0: Sequence[float],
    p1: Sequence[float],
    r0: float,
    r1: float,
    *,
    vertices: int = 10,
    material: bpy.types.Material | None = None,
    bevel: float = 0.002,
) -> bpy.types.Object:
    """A tapered round member from `p0` (radius `r0`) to `p1` (radius `r1`).

    Bevel first, then shade: applying a bevel to a mesh that already carries
    a Smooth-by-Angle modifier means applying that modifier too, and the order
    above never has to.
    """
    a = Vector((float(p0[0]), float(p0[1]), float(p0[2])))
    b = Vector((float(p1[0]), float(p1[1]), float(p1[2])))
    delta = b - a
    obj = K.cone(
        name, radius1=r0, radius2=r1, depth=delta.length, vertices=vertices,
        location=(0.0, 0.0, 0.0), material=material, smooth=False,
    )
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = delta.to_track_quat("Z", "Y")
    K.transform_apply(obj)
    obj.location = (a + b) * 0.5
    K.transform_apply(obj)
    if bevel > 0.0:
        K.add_bevel(obj, bevel, 2)
    K.shade_smooth(obj, angle=math.radians(40.0))
    return obj


def _head(
    name: str,
    radius: float,
    depth: float,
    centre: Sequence[float],
    material: bpy.types.Material | None,
    *,
    vertices: int = 12,
    bevel: float = 0.0012,
) -> bpy.types.Object:
    """A short bevelled cylinder lying flat — screw heads and ferrules."""
    obj = K.cylinder(
        name, radius=radius, depth=depth, vertices=vertices,
        location=centre, material=material, smooth=False,
    )
    if bevel > 0.0:
        K.add_bevel(obj, bevel, 2)
    K.shade_smooth(obj, angle=math.radians(40.0))
    return obj


# ---------------------------------------------------------------------------
# grounding
# ---------------------------------------------------------------------------


def _lowest_z(obj: bpy.types.Object) -> float:
    """World-space Z of the lowest vertex of the MESH as authored.

    Reads `obj.data.vertices` through `matrix_world` rather than going
    through `K.world_bounds`: the kernel helper evaluates the depsgraph, and
    every part here still carries a location offset from its primitive call
    (`K.cylinder` never applies its transform), so an un-flushed depsgraph
    would report a stale floor.
    """
    matrix = obj.matrix_world
    return min((matrix @ v.co).z for v in obj.data.vertices)


def _ground(parts: Sequence[bpy.types.Object]) -> list[bpy.types.Object]:
    """Drop the WHOLE asset so its lowest point sits on z=0.

    Measured across every part and applied as one shared offset. Grounding
    parts individually is the bug: each leg of a chair would be pushed to the
    floor on its own and the seat would end up below the frame.
    """
    live = [p for p in parts if p is not None]
    if not live:
        return live
    bpy.context.view_layer.update()
    lowest = min(_lowest_z(obj) for obj in live)
    if abs(lowest) < 1e-6:
        return live
    for obj in live:
        obj.location.z -= lowest
    bpy.context.view_layer.update()
    for obj in live:
        K.transform_apply(obj)
    return live


# ===========================================================================
# beach_boardwalk_plank
# ===========================================================================


def _plank_section(
    width: float,
    thickness: float,
    corner: float = 0.009,
    camber: float = 0.0015,
    arc_steps: int = 4,
    top_points: int = 5,
) -> list[tuple[float, float]]:
    """One cross-section of a deck plank: a rounded, slightly cambered slab.

    Decking is eased along both long edges so it does not cut into the next
    plank, and it is cambered so rain runs off. A razor arris along the top
    face is the single thing that makes a plank read as an untextured box, so
    the corners are real arcs rather than single vertices.

    The camber NEEDS its own vertices. With only the two top corners joined
    by one edge the top face is a straight line, `camber` is applied to two
    points and changes nothing, and every raised grain line laid on the
    analytic `top_at()` curve then floats up to 1.5 mm above a flat surface.
    `top_points` is what makes the curve real geometry.
    """
    hw = width * 0.5
    r = min(corner, hw * 0.5, thickness * 0.5)
    pts: list[tuple[float, float]] = []

    def top_z(x: float) -> float:
        return thickness + camber * (1.0 - min(1.0, abs(x) / hw) ** 2)

    def arc(cx: float, cz: float, a0: float, a1: float, skip_first: bool) -> None:
        for i in range(arc_steps + 1):
            if skip_first and i == 0:
                continue
            t = math.radians(a0 + (a1 - a0) * i / arc_steps)
            pts.append((cx + r * math.cos(t), cz + r * math.sin(t)))

    arc(hw - r, r, -90.0, 0.0, False)                  # bottom-right
    pts.append((hw, thickness - r))
    arc(hw - r, thickness - r, 0.0, 90.0, True)       # top-right
    inset = hw - r
    span = [inset - 2.0 * inset * i / (top_points - 1) for i in range(top_points)]
    pts.extend((x, top_z(x)) for x in span)            # cambered top face
    arc(-hw + r, thickness - r, 90.0, 180.0, True)    # top-left
    pts.append((-hw, r))
    arc(-hw + r, r, 180.0, 270.0, True)               # bottom-left

    # Drop coincident points. With `camber=0` the first top station lands
    # exactly on the corner arc's endpoint, and a zero-length edge there
    # makes the cap n-gon degenerate and the bevel modifier produce garbage.
    clean: list[tuple[float, float]] = []
    for pt in pts:
        if not clean or abs(pt[0] - clean[-1][0]) > 1e-9 or abs(pt[1] - clean[-1][1]) > 1e-9:
            clean.append(pt)
    if len(clean) > 1 and abs(clean[0][0] - clean[-1][0]) < 1e-9 \
            and abs(clean[0][1] - clean[-1][1]) < 1e-9:
        clean.pop()
    return clean


def build_boardwalk_plank(
    asset_id: str,
    *,
    length: float = 1.90,
    width: float = 0.145,
    thickness: float = 0.045,
    corner: float = 0.009,
    camber: float = 0.0015,
    warp: float = 0.006,
    grain_lines: int = 4,
    screws: int = 4,
    wood_rgb: Sequence[float] = (0.84, 0.78, 0.68),
    grain_rgb: Sequence[float] = (0.72, 0.64, 0.53),
    screw_rgb: Sequence[float] = (0.58, 0.60, 0.64),
) -> list[bpy.types.Object]:
    """One deck plank: eased edges, cambered top, raised grain, proud screws.

    Extruded along Y (the walkway direction) through seven rings, with the two
    end rings lifted by `warp` — a straight extrusion is the tell that this is
    a box, and a real board laid on a deck has picked up a slight bow.
    """
    K.reset_scene()

    wood = _painted_wood(f"{asset_id}_wood", wood_rgb, rough=0.52)
    grain = _painted_wood(f"{asset_id}_grain", grain_rgb, rough=0.58)
    screw = _aluminium(f"{asset_id}_screw", screw_rgb, rough=0.38)

    half = length * 0.5
    section = _plank_section(width, thickness, corner, camber)
    count = len(section)

    rings: list[list[tuple[float, float, float]]] = []
    uvs: list[tuple[float, float]] = []
    # seven rings: the y=0 one is the floor contact, the ends bow up
    for s in range(-3, 4):
        y = half * s / 3.0
        lift = warp * (y / half) ** 2
        ring: list[tuple[float, float, float]] = []
        for (x, z) in section:
            ring.append((x, y, z + lift))
            uvs.append((x / width + 0.5, (y + half) / length))
        rings.append(ring)

    body = _loft(f"{asset_id}_plank", rings, wood, uvs=uvs)
    K.add_bevel(body, 0.004, 3)
    K.shade_smooth(body, angle=math.radians(38.0))
    parts: list[bpy.types.Object] = [body]

    def top_at(x: float) -> float:
        return thickness + camber * (1.0 - min(1.0, abs(x) / (width * 0.5)) ** 2)

    # Raised grain figure: four lengthwise ridges at slightly different
    # heights, in a darker tone. Grooves would need a boolean or a
    # self-intersecting section; a proud line catches the same light break
    # for a fraction of the build risk.
    for i in range(grain_lines):
        t = 0.0 if grain_lines == 1 else (i / (grain_lines - 1)) - 0.5
        x = t * width * 0.72
        h = 0.0030 + 0.0010 * (1.0 if i % 2 == 0 else -1) * 0.5
        parts.append(_plank(
            f"{asset_id}_grain{i}",
            size=(0.012, length * 0.955, h),
            centre=(x, 0.0, top_at(x) + h * 0.5 - 0.0018),
            material=grain, bevel=0.0008,
        ))

    # Countersunk fasteners, standing a millimetre proud of the camber.
    for i in range(screws):
        sy = -1.0 if i % 2 == 0 else 1.0
        sx = -1.0 if (i // 2) % 2 == 0 else 1.0
        x = sx * width * 0.22
        y = sy * length * 0.41
        parts.append(_head(
            f"{asset_id}_screw{i}", radius=0.009, depth=0.008,
            centre=(x, y, top_at(x) - 0.003), material=screw,
            vertices=12, bevel=0.0014,
        ))

    return P.finish(_ground(parts))


# ===========================================================================
# beach_chair
# ===========================================================================


def build_chair(
    asset_id: str,
    *,
    width: float = 0.50,
    seat_depth: float = 0.46,
    seat_top: float = 0.42,
    back_angle: float = 20.0,
    cushion: bool = True,
    frame_rgb: Sequence[float] = (0.95, 0.93, 0.86),
    slat_rgb: Sequence[float] = (0.26, 0.70, 0.66),
    canvas_rgb: Sequence[float] = (0.98, 0.44, 0.40),
) -> list[bpy.types.Object]:
    """A slatted folding deck chair: frame, slatted seat, slatted back, legs.

    The slats are real slats. A solid seat pad on a solid back reads as a
    moulded plastic garden chair; five boards with 14 mm gaps read as a chair
    you could see the beach through.
    """
    K.reset_scene()

    frame_mat = _painted_wood(f"{asset_id}_frame", frame_rgb, rough=0.38)
    slat_mat = _painted_wood(f"{asset_id}_slat", slat_rgb, rough=0.42)
    pad_mat = _canvas(f"{asset_id}_pad", canvas_rgb)

    parts: list[bpy.types.Object] = []
    hx = width * 0.5 - 0.025
    leg_top = seat_top - 0.022
    front_y = -seat_depth * 0.5 + 0.045
    rear_y = seat_depth * 0.5 - 0.045
    tilt = math.radians(back_angle)
    back_dir = (0.0, math.sin(tilt), math.cos(tilt))
    back_base = (0.0, seat_depth * 0.5 - 0.02, seat_top - 0.03)

    for sx, side in ((-1.0, "l"), (1.0, "r")):
        x = sx * hx
        # four legs
        for y, tag in ((front_y, "front"), (rear_y, "rear")):
            parts.append(_strut(
                f"{asset_id}_leg_{tag}_{side}",
                (x, y, 0.0), (x, y, leg_top), (0.048, 0.048),
                material=frame_mat, bevel=0.006,
            ))
        # seat rails
        parts.append(_strut(
            f"{asset_id}_rail_{side}",
            (x, front_y - 0.02, leg_top - 0.021), (x, rear_y + 0.02, leg_top - 0.021),
            (0.045, 0.042), material=frame_mat, bevel=0.005,
        ))
        # back posts, continuous with the rear legs
        top = tuple(back_base[i] + back_dir[i] * 0.53 for i in range(3))
        parts.append(_strut(
            f"{asset_id}_post_{side}", (x, back_base[1], back_base[2]), top,
            (0.048, 0.048), material=frame_mat, bevel=0.006,
        ))
        # arm upright + armrest, tied back into the post
        parts.append(_strut(
            f"{asset_id}_armpost_{side}",
            (x, front_y, leg_top - 0.02), (x, front_y, seat_top + 0.18),
            (0.042, 0.042), material=frame_mat, bevel=0.005,
        ))
        parts.append(_strut(
            f"{asset_id}_armrest_{side}",
            (x, front_y - 0.02, seat_top + 0.18),
            (x, top[1] - 0.02, top[2] - 0.17), (0.052, 0.036),
            material=frame_mat, bevel=0.007,
        ))

    # cross stretchers front and back
    for y, tag in ((front_y, "front"), (rear_y, "rear")):
        parts.append(_strut(
            f"{asset_id}_stretcher_{tag}",
            (-hx, y, 0.13), (hx, y, 0.13), (0.032, 0.032),
            material=frame_mat, bevel=0.004,
        ))

    # --- slatted seat ------------------------------------------------------
    slats = 5
    pitch = seat_depth / slats
    for i in range(slats):
        y = -seat_depth * 0.5 + pitch * (i + 0.5)
        parts.append(_plank(
            f"{asset_id}_seat_slat{i}",
            size=(width - 0.03, pitch * 0.86, 0.022),
            centre=(0.0, y, seat_top - 0.011),
            material=slat_mat, bevel=0.004,
        ))

    # --- slatted back ------------------------------------------------------
    bslats = 5
    for i in range(bslats):
        t = 0.075 + 0.095 * i
        centre = tuple(back_base[j] + back_dir[j] * t for j in range(3))
        parts.append(_plank(
            f"{asset_id}_back_slat{i}",
            size=(width - 0.03, 0.020, 0.070),
            centre=centre,
            rot_deg=(-back_angle, 0.0, 0.0),
            material=slat_mat, bevel=0.004,
        ))

    if cushion:
        parts.append(_plank(
            f"{asset_id}_cushion",
            size=(width - 0.05, seat_depth - 0.06, 0.034),
            centre=(0.0, 0.0, seat_top + 0.017),
            material=pad_mat, bevel=0.013, bevel_segments=3,
        ))

    return P.finish(_ground(parts))


# ===========================================================================
# beach_surfboard
# ===========================================================================

# Half-width as a fraction of the widest point, tail (u=0) to nose (u=1).
# u = 0.50 is the wide midsection; u = 1.00 is the 60 mm nose.
_BOARD_OUTLINE = (
    (0.00, 0.370), (0.10, 0.510), (0.28, 0.880), (0.46, 1.000),
    (0.56, 0.985), (0.72, 0.810), (0.86, 0.490), (0.95, 0.170),
)
# Root chord of the single centre fin, shared by the ring builder and the
# placement correction that keeps its tip out of the deck.
_FIN_BASE_CHORD = 0.15

# Half-thickness as a fraction of the board's full thickness.
_BOARD_THICK = (
    (0.00, 0.26), (0.12, 0.40), (0.30, 0.48), (0.50, 0.50),
    (0.70, 0.45), (0.88, 0.31), (1.00, 0.17),
)


def _rocker(u: float, nose: float, tail: float) -> float:
    """Longitudinal lift of a station, 0 at the midsection.

    Asymmetric on purpose: a surfboard noses up far more than it kicks its
    tail, and a symmetric parabola reads as a banana. The profile therefore
    DIPS: tail kick up, a flat section through the middle, nose lift at the
    front. The flat section is the part that carries the board's weight and
    is what `_ground` finds as the contact.
    """
    s = u - 0.5
    if s >= 0.0:
        return nose * (s / 0.5) ** 2.0
    return tail * ((-s) / 0.5) ** 2.2


def _board_ring(
    y: float,
    half_width: float,
    deck: float,
    belly: float,
    steps: int,
) -> list[tuple[float, float, float]]:
    """One cross-section of the board: domed deck, flatter belly, soft rails.

    A single superellipse exponent on both halves gives either a flat board
    with a knife rail or a domed board with a fat rail. Splitting the exponent
    per half gives a domed deck, a nearly flat belly and a rail that both
    surfaces roll into instead of meeting at a crease.
    """
    e_deck = 2.0 / 2.3
    e_belly = 2.0 / 3.0
    e_x = e_deck
    ring: list[tuple[float, float, float]] = []
    for i in range(steps):
        t = 2.0 * math.pi * i / steps
        c = math.cos(t)
        s = math.sin(t)
        x = half_width * (abs(c) ** e_x) * (1.0 if c >= 0.0 else -1.0)
        if s >= 0.0:
            z = deck * (abs(s) ** e_deck)
        else:
            z = -belly * (abs(s) ** e_belly)
        ring.append((x, y, z))
    return ring


def _foil_loop(span_points: int, thickness_ratio: float = 0.09) -> list[tuple[float, float]]:
    """A closed symmetric NACA 00xx loop: (chordwise, thickness) pairs.

    `span_points` stations from leading edge (0.0) to trailing edge (1.0);
    the trailing-edge coefficient closes the loop exactly at y = 0, so the
    ring never carries a duplicated point.
    """
    upper: list[tuple[float, float]] = []
    for i in range(span_points):
        xi = 0.5 * (1.0 - math.cos(math.pi * i / (span_points - 1)))
        yt = 5.0 * thickness_ratio * (
            0.2969 * math.sqrt(xi) - 0.1260 * xi - 0.3516 * xi * xi
            + 0.2843 * xi ** 3 - 0.1036 * xi ** 4
        )
        upper.append((xi, yt))
    loop = list(upper)
    loop.extend((xi, -yt) for (xi, yt) in reversed(upper[1:-1]))
    return loop


def _fin_rings(
    fin_height: float,
    base_chord: float,
    tip_chord: float,
    sweep: float,
    sections: int = 6,
    chord_points: int = 9,
) -> list[list[tuple[float, float, float]]]:
    """The fin as a lofted foil: shrinking, swept-back sections, canted later.

    Local frame is X = across the board, Y = fore/aft (-Y is the nose),
    Z = down. The root section sits at z = 0 and the fin grows downward, so
    the whole object can be dropped onto the belly with one translation.
    """
    loop = _foil_loop(chord_points)
    rings: list[list[tuple[float, float, float]]] = []
    for s in range(sections):
        t = s / (sections - 1)
        chord = tip_chord + (base_chord - tip_chord) * (1.0 - t) ** 0.85
        back = sweep * (t ** 1.4)
        down = fin_height * t
        ring: list[tuple[float, float, float]] = []
        for (xi, yt) in loop:
            y = -(0.42 - xi) * chord + back
            x = yt * chord
            ring.append((x, y, -down))
        rings.append(ring)
    return rings


def build_surfboard(
    asset_id: str,
    *,
    length: float = 1.90,
    max_width: float = 0.46,
    nose_width: float = 0.06,
    tail_width: float = 0.17,
    thickness: float = 0.046,
    nose_rocker: float = 0.20,
    tail_rocker: float = 0.13,
    stations: int = 26,
    fin_height: float = 0.055,
    deck_rgb: Sequence[float] = (0.99, 0.94, 0.87),
    belly_rgb: Sequence[float] = (0.99, 0.55, 0.42),
    fin_rgb: Sequence[float] = (0.10, 0.70, 0.70),
    stringer_rgb: Sequence[float] = (0.96, 0.96, 0.98),
    fin_station: float = 0.14,
    fin_cant: float = 7.0,
) -> list[bpy.types.Object]:
    """A shortboard: pointed nose, wide midsection, tapered tail, real rocker.

    THE loft, not a box: `stations` cross-sections are stacked along Y, each
    at its own half-width, its own half-thickness and — the part that matters
    — its own Z offset from `_rocker`. That progressive offset is the curve you
    see running out of the nose and lifting off the tail; a single flat slab
    can only fake it with a chamfer, and the chamfer reads as a bevel.

    The board lies deck-up on the sand. The tail rocker lifts the fin area
    clear of the ground, so the fin hangs in the air instead of burying
    itself, and `_ground` then finds the midsection belly as the contact.
    """
    K.reset_scene()

    deck_mat = _fibreglass(f"{asset_id}_deck", deck_rgb, rough=0.12)
    belly_mat = _fibreglass(f"{asset_id}_belly", belly_rgb, rough=0.14)
    fin_mat = _fibreglass(f"{asset_id}_fin", fin_rgb, rough=0.20)
    stringer_mat = _fibreglass(f"{asset_id}_stringer", stringer_rgb, rough=0.10)

    half = length * 0.5
    hw_max = max_width * 0.5
    outline = list(_BOARD_OUTLINE) + [(1.00, nose_width * 0.5 / hw_max)]

    # --- the loft ----------------------------------------------------------
    rings: list[list[tuple[float, float, float]]] = []
    uvs: list[tuple[float, float]] = []
    for s in range(stations):
        u = s / (stations - 1)
        y = half - u * length                       # nose (u=1) at -Y
        lift = _rocker(u, nose_rocker, tail_rocker)
        hw = hw_max * _hermite(outline, u)
        frac = _hermite(_BOARD_THICK, u)
        deck_t = thickness * frac
        belly_t = thickness * frac * 0.86
        ring = _board_ring(y, hw, deck_t, belly_t, 18)
        # shift the whole section up by its rocker offset
        rings.append([(x, yy, z + lift) for (x, yy, z) in ring])
        for j in range(len(ring)):
            uvs.append((u, j / len(ring)))

    # One object, two materials by face band: the deck half of every ring
    # takes the glossier deck response, the belly half the belly response.
    board = _loft(f"{asset_id}_board", rings, deck_mat, uvs=uvs)
    steps = 18
    for poly in board.data.polygons:
        if len(poly.vertices) == 4:
            # ring index from the vertex index, then half of the ring
            r = poly.vertices[0] // steps
            s = poly.vertices[0] % steps
            lifted = rings[r][s][2] - _rocker(
                (r + 0.0) / (stations - 1), nose_rocker, tail_rocker
            )
            if lifted < 0.0:
                poly.material_index = 1
    board.data.materials.append(belly_mat)
    K.add_bevel(board, 0.0025, 2)
    K.shade_smooth(board, angle=math.radians(35.0))
    parts: list[bpy.types.Object] = [board]

    def deck_z(u: float) -> float:
        frac = _hermite(_BOARD_THICK, u)
        return _rocker(u, nose_rocker, tail_rocker) + thickness * frac

    def belly_z(u: float) -> float:
        frac = _hermite(_BOARD_THICK, u)
        return _rocker(u, nose_rocker, tail_rocker) - thickness * frac * 0.86

    # --- stringer: a 22 mm line down the deck, following the rocker -------
    srings: list[list[tuple[float, float, float]]] = []
    suvs: list[tuple[float, float]] = []
    sw, sh = 0.011, 0.0022
    for s in range(stations):
        u = s / (stations - 1)
        y = half - u * length
        base = deck_z(u)
        srings.append([
            (-sw, y, base - 0.004), (sw, y, base - 0.004),
            (sw, y, base + sh), (-sw, y, base + sh),
        ])
        for j in range(4):
            suvs.append((u, j / 4.0))
    stringer = _loft(f"{asset_id}_stringer", srings, stringer_mat, uvs=suvs)
    K.add_bevel(stringer, 0.0009, 2)
    K.shade_smooth(stringer, angle=math.radians(35.0))
    parts.append(stringer)

    # --- fin on the underside, near the tail ------------------------------
    fin_rings = _fin_rings(fin_height, base_chord=_FIN_BASE_CHORD,
                           tip_chord=0.042, sweep=0.085)
    fin = _loft(f"{asset_id}_fin", fin_rings, fin_mat)
    fin.rotation_euler = (0.0, math.radians(fin_cant), 0.0)
    K.transform_apply(fin)
    # The fin's root must sit ON the belly, not above it.
    #
    # The root chord is a real aerofoil section, not a flat ring, so the fin's
    # topmost vertex stands `root_crown` above the ring's centreline. Placing
    # the fin at `belly_z` therefore buried that much of it in the deck — 22.8
    # mm of a 25 mm fin, against a 20 mm limit. Subtract the crown so the
    # highest point of the root sits flush with the belly and the rest of the
    # fin hangs below it, which is what "hanging under the deck" means.
    # `_fin_rings` already grows the fin DOWNWARD from its root ring at local
    # z=0, so the root is flush as placed. What lifts the fin's top face above
    # the belly is the CANT: rotating about Y tips the swept-forward tip up by
    # roughly sweep*sin(cant). The consumer's limit is that overhang.
    #
    # Derive the correction from the fin's own extent rather than a magic
    # number, so it tracks `sweep` and `fin_cant` if either changes.
    tip_rise = abs(math.sin(math.radians(fin_cant))) * _FIN_BASE_CHORD * 0.5
    fin.location = (0.0, half - fin_station * length,
                    belly_z(fin_station) - tip_rise)
    K.transform_apply(fin)
    K.add_bevel(fin, 0.0018, 2)
    K.shade_smooth(fin, angle=math.radians(35.0))
    parts.append(fin)

    # --- leash plug on the deck, just ahead of the fin -------------------
    plug_u = 0.045
    parts.append(_head(
        f"{asset_id}_leash_plug", radius=0.011, depth=0.007,
        centre=(0.0, half - plug_u * length, deck_z(plug_u) - 0.0022),
        material=fin_mat, vertices=12, bevel=0.0014,
    ))

    return P.finish(_ground(parts))


# ===========================================================================
# beach_umbrella
# ===========================================================================


def build_umbrella(
    asset_id: str,
    *,
    pole_height: float = 2.10,
    canopy_radius: float = 1.35,
    rim_drop: float = 0.54,
    ribs: int = 8,
    sag: float = 0.15,
    seam: float = 0.05,
    tilt: float = 0.0,
    canvas_rgb: Sequence[float] = (0.97, 0.34, 0.40),
    alt_canvas_rgb: Sequence[float] = (0.98, 0.94, 0.88),
    pole_rgb: Sequence[float] = (0.68, 0.70, 0.74),
) -> list[bpy.types.Object]:
    """A beach umbrella: two-stage pole, hub, 8 ribs, 8 drooping panels.

    THE droop. Each panel's Z falls by `sag * sin(pi * a)` between its two
    ribs and is scaled by (0.25 + 0.75b) so the fabric converges without
    crumpling at the hub. The result is eight scallops hanging off eight
    straight ribs — which is what fabric does. Without it the panels meet in a
    taut point and the asset reads as a cone with a pattern on it.

    The ribs run 11 mm BELOW the fabric line so their tops sit inside the
    panel shell, the way a real rib is sewn into a canopy: visible through the
    seam, buried under the cloth everywhere else.
    """
    K.reset_scene()

    cloth = _canvas(f"{asset_id}_canvas", canvas_rgb)
    cloth_alt = _canvas(f"{asset_id}_canvas_alt", alt_canvas_rgb)
    stock = _aluminium(f"{asset_id}_pole", pole_rgb, rough=0.28)

    hub_z_top = pole_height
    hub_z_bot = pole_height - 0.11
    hub_r_top = 0.060
    hub_r_bot = 0.032
    rim_z = pole_height - rim_drop
    panel_hub_r = hub_r_top * 0.92
    panel_hub_z = hub_z_top - 0.012
    rib_inset = 0.011

    def spine(b: float) -> tuple[float, float]:
        """(radius, z) of the fabric line at parameter b, hub -> rim."""
        rad = panel_hub_r + (canopy_radius - panel_hub_r) * b
        z = panel_hub_z + (rim_z - panel_hub_z) * b
        return rad, z

    parts: list[bpy.types.Object] = []

    # --- pole: weighted foot, lower section, collar, upper section --------
    parts.append(_head(
        f"{asset_id}_foot", radius=0.058, depth=0.035, centre=(0.0, 0.0, 0.0175),
        material=stock, vertices=20, bevel=0.006,
    ))
    parts.append(_head(
        f"{asset_id}_pole_lower", radius=0.028, depth=0.66, centre=(0.0, 0.0, 0.33),
        material=stock, vertices=20, bevel=0.003,
    ))
    parts.append(_head(
        f"{asset_id}_pole_collar", radius=0.037, depth=0.08, centre=(0.0, 0.0, 0.64),
        material=stock, vertices=20, bevel=0.004,
    ))
    parts.append(_head(
        f"{asset_id}_pole_upper", radius=0.021, depth=1.44, centre=(0.0, 0.0, 1.30),
        material=stock, vertices=20, bevel=0.003,
    ))

    # --- hub + finial ------------------------------------------------------
    hub = K.cone(
        f"{asset_id}_hub", radius1=hub_r_bot, radius2=hub_r_top, depth=0.11,
        vertices=20, location=(0.0, 0.0, hub_z_bot + 0.055),
        material=stock, smooth=False,
    )
    K.add_bevel(hub, 0.0035, 2)
    K.shade_smooth(hub, angle=math.radians(40.0))
    parts.append(hub)

    finial = K.sphere(
        f"{asset_id}_finial", radius=0.030, segments=16, ring_count=10,
        location=(0.0, 0.0, hub_z_top + 0.004), material=stock,
    )
    parts.append(finial)

    # --- ribs + canopy panels ---------------------------------------------
    step = 2.0 * math.pi / ribs
    gap = seam / max(1.0, ribs * canopy_radius)
    nu, nv = 9, 6
    half_t = 0.003

    for i in range(ribs):
        ang = i * step
        ca, sa = math.cos(ang), math.sin(ang)

        r0, z0 = spine(-0.03)
        r1, z1 = spine(1.02)
        parts.append(_spindle(
            f"{asset_id}_rib{i}",
            (r0 * ca, r0 * sa, z0 - rib_inset),
            (r1 * ca, r1 * sa, z1 - rib_inset),
            0.013, 0.008, vertices=10, material=stock, bevel=0.002,
        ))
        r2, z2 = spine(1.05)
        parts.append(_spindle(
            f"{asset_id}_ribtip{i}",
            (r1 * ca, r1 * sa, z1 - rib_inset),
            (r2 * ca, r2 * sa, z2 - rib_inset),
            0.009, 0.005, vertices=10, material=stock, bevel=0.0015,
        ))

        top: list[list[tuple[float, float, float]]] = []
        bot: list[list[tuple[float, float, float]]] = []
        uv_rows: list[list[tuple[float, float]]] = []
        for a_i in range(nu):
            a = gap + (1.0 - 2.0 * gap) * (a_i / (nu - 1))
            row_t: list[tuple[float, float, float]] = []
            row_b: list[tuple[float, float, float]] = []
            row_uv: list[tuple[float, float]] = []
            for b_i in range(nv):
                b = b_i / (nv - 1)
                rad, z = spine(b)
                z -= sag * math.sin(math.pi * a) * (0.25 + 0.75 * b)
                th = ang + a * step
                x = rad * math.cos(th)
                y = rad * math.sin(th)
                row_t.append((x, y, z + half_t))
                row_b.append((x, y, z - half_t))
                row_uv.append((a_i / (nu - 1), b_i / (nv - 1)))
            top.append(row_t)
            bot.append(row_b)
            uv_rows.append(row_uv)

        panel = _grid_shell(
            f"{asset_id}_canopy_panel{i}", top, bot,
            cloth if i % 2 == 0 else cloth_alt, uv_rows,
        )
        K.shade_smooth(panel, angle=math.radians(40.0))
        parts.append(panel)

    if tilt:
        for obj in parts:
            obj.rotation_euler = (math.radians(tilt), 0.0, 0.0)
            K.transform_apply(obj)
        # a tilted umbrella must still rest on z=0, so drop it back down
        bpy.context.view_layer.update()
        lowest = min(_lowest_z(o) for o in parts)
        for obj in parts:
            obj.location.z -= lowest
        bpy.context.view_layer.update()
        for obj in parts:
            K.transform_apply(obj)
        return P.finish(parts)

    return P.finish(_ground(parts))


# ===========================================================================
# registry entry
# ===========================================================================

BuilderFn = Callable[..., list[bpy.types.Object]]

BEACH: list[tuple[str, BuilderFn, dict]] = [
    ("beach_boardwalk_plank", build_boardwalk_plank, dict(
        length=1.90, width=0.145, thickness=0.045, corner=0.009, camber=0.0015,
        warp=0.006, grain_lines=4, screws=4,
        wood_rgb=(0.84, 0.78, 0.68), grain_rgb=(0.72, 0.64, 0.53),
        screw_rgb=(0.58, 0.60, 0.64),
    )),
    ("beach_chair", build_chair, dict(
        width=0.50, seat_depth=0.46, seat_top=0.42, back_angle=20.0, cushion=True,
        frame_rgb=(0.95, 0.93, 0.86), slat_rgb=(0.26, 0.70, 0.66),
        canvas_rgb=(0.98, 0.44, 0.40),
    )),
    ("beach_surfboard", build_surfboard, dict(
        length=1.90, max_width=0.46, nose_width=0.06, tail_width=0.17,
        thickness=0.046, nose_rocker=0.20, tail_rocker=0.13, stations=26,
        fin_height=0.055, fin_station=0.14, fin_cant=7.0,
        deck_rgb=(0.99, 0.94, 0.87), belly_rgb=(0.99, 0.55, 0.42),
        fin_rgb=(0.10, 0.70, 0.70), stringer_rgb=(0.96, 0.96, 0.98),
    )),
    ("beach_umbrella", build_umbrella, dict(
        pole_height=2.10, canopy_radius=1.35, rim_drop=0.54, ribs=8,
        sag=0.15, seam=0.05,
        canvas_rgb=(0.97, 0.34, 0.40), alt_canvas_rgb=(0.98, 0.94, 0.88),
        pole_rgb=(0.68, 0.70, 0.74),
    )),
]
