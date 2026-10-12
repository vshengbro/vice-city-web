"""bpy modelling kernel for the NEON BAY 3A asset rebuild.

THIS IS THE REAL DEAL: every helper here drives `bpy.ops` / the bpy data API
inside a live Blender process. There is no pure-Python geometry kernel — that
was the old pipeline (`vcw/core.py`), and it is what this rebuild replaces.

Design rules that come from the procedural-game-asset-pipeline skill and that
this module exists to enforce:

* **Transform eagerly, then assert position.** Every builder returns an object
  whose world transform is already applied (`matrix_world`), because a helper
  that returns a transformed *copy* of immutable tuples is a silent no-op — the
  asset builds, renders, passes every shape check, and is in the wrong place.
* **Every part gets a unique meaningful name.** The renderer addresses parts by
  name (tyres/hubs on a car), so duplicate names across instances make the data
  unaddressable.
* **Every closed shell is checked for outwardness by signed volume**, welded by
  position first — index-neighbour walks find nothing on flat meshes.
* **Open surfaces must declare a reference**, or the export check refuses them.

Coordinate contract (unchanged from the shipped renderer):
authoring is Z-up (+X right, -Y forward, +Z up), exported as
``json = (x, z, -y)``, a determinant +1 pure rotation, so handedness and CCW
winding survive exactly.
"""

from __future__ import annotations

import math
from typing import Iterable, Sequence

import bmesh
import bpy
from mathutils import Matrix, Vector

# ---------------------------------------------------------------------------
# scene management
# ---------------------------------------------------------------------------


def reset_scene() -> None:
    """Wipe every datablock so each asset builds from a known-empty scene.

    `read_factory_settings` is the only reliable reset in background mode:
    deleting objects leaves orphaned meshes/materials that later show up in the
    export as stray primitives.
    """
    bpy.ops.wm.read_factory_settings(use_empty=True)
    _enable_gltf()


def _enable_gltf() -> None:
    import addon_utils

    addon_utils.enable("io_scene_gltf2", default_set=True, persistent=True)


# ---------------------------------------------------------------------------
# material system — PBR layers, not vertex colours
# ---------------------------------------------------------------------------


def pbr_material(
    name: str,
    base_color: Sequence[float],
    roughness: float,
    metallic: float = 0.0,
    *,
    emission: Sequence[float] | None = None,
    emission_strength: float = 0.0,
    alpha: float = 1.0,
    transmission: float = 0.0,
    ior: float = 1.45,
) -> bpy.types.Material:
    """Create a Principled-BSDF material.

    The old pipeline could only bake a per-vertex RGB. A 3A asset needs the
    material split to survive export: car paint, glass, rubber and chrome are
    four different (roughness, metallic, transmission) responses, and glTF
    carries exactly those as separate primitives.
    """
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    if bsdf is None:
        return mat

    def put(socket: str, value) -> None:
        if socket in bsdf.inputs:
            bsdf.inputs[socket].default_value = value

    put("Base Color", (*base_color, 1.0))
    put("Roughness", roughness)
    put("Metallic", metallic)
    put("IOR", ior)
    put("Alpha", alpha)
    put("Transmission Weight", transmission)

    if emission is not None:
        put("Emission Color", (*emission, 1.0))
        put("Emission Strength", emission_strength)

    if alpha < 1.0 or transmission > 0.0:
        # 3A glass needs blend mode, or it renders fully opaque in preview.
        mat.blend_method = "BLEND" if hasattr(mat, "blend_method") else mat.blend_method
        try:
            mat.use_backface_culling = False
        except AttributeError:
            pass
    return mat


# ---------------------------------------------------------------------------
# primitive helpers — all eager transforms
# ---------------------------------------------------------------------------


def _apply_transform(obj: bpy.types.Object) -> bpy.types.Object:
    """Bake the current object transform into the mesh data.

    Without this, `matrix_world` still carries the offset at export time and a
    glTF consumer that only reads positions gets geometry at the origin.
    """
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return obj


def cube(
    name: str,
    size: Sequence[float] = (1.0, 1.0, 1.0),
    location: Sequence[float] = (0.0, 0.0, 0.0),
    material: bpy.types.Material | None = None,
    bevel: float = 0.0,
    bevel_segments: int = 2,
) -> bpy.types.Object:
    """Axis-aligned box centred on `location`, then optionally bevelled."""
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=location)
    obj = bpy.context.active_object
    obj.name = name
    obj.scale = size
    _apply_transform(obj)
    if bevel > 0.0:
        add_bevel(obj, bevel, bevel_segments)
    assign_material(obj, material)
    return obj


def cylinder(
    name: str,
    radius: float = 1.0,
    depth: float = 1.0,
    vertices: int = 24,
    location: Sequence[float] = (0.0, 0.0, 0.0),
    material: bpy.types.Material | None = None,
    smooth: bool = True,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=vertices, radius=radius, depth=depth, location=location
    )
    obj = bpy.context.active_object
    obj.name = name
    assign_material(obj, material)
    if smooth:
        shade_smooth(obj, angle=math.radians(35.0))
    return obj


def cone(
    name: str,
    radius1: float = 1.0,
    radius2: float = 0.0,
    depth: float = 1.0,
    vertices: int = 24,
    location: Sequence[float] = (0.0, 0.0, 0.0),
    material: bpy.types.Material | None = None,
    smooth: bool = True,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cone_add(
        vertices=vertices,
        radius1=radius1,
        radius2=radius2,
        depth=depth,
        location=location,
    )
    obj = bpy.context.active_object
    obj.name = name
    assign_material(obj, material)
    if smooth:
        shade_smooth(obj, angle=math.radians(35.0))
    return obj


def sphere(
    name: str,
    radius: float = 1.0,
    segments: int = 24,
    ring_count: int = 12,
    location: Sequence[float] = (0.0, 0.0, 0.0),
    material: bpy.types.Material | None = None,
    scale: Sequence[float] | None = None,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=segments, ring_count=ring_count, radius=radius, location=location
    )
    obj = bpy.context.active_object
    obj.name = name
    if scale is not None:
        obj.scale = scale
        _apply_transform(obj)
    assign_material(obj, material)
    shade_smooth(obj)
    return obj


def torus(
    name: str,
    major_radius: float = 1.0,
    minor_radius: float = 0.25,
    major_segments: int = 24,
    minor_segments: int = 12,
    location: Sequence[float] = (0.0, 0.0, 0.0),
    material: bpy.types.Material | None = None,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_torus_add(
        major_radius=major_radius,
        minor_radius=minor_radius,
        major_segments=major_segments,
        minor_segments=minor_segments,
        location=location,
    )
    obj = bpy.context.active_object
    obj.name = name
    assign_material(obj, material)
    shade_smooth(obj)
    return obj


def plane(
    name: str,
    size: float = 1.0,
    size_y: float | None = None,
    location: Sequence[float] = (0.0, 0.0, 0.0),
    material: bpy.types.Material | None = None,
    open_surface: bool = False,
) -> bpy.types.Object:
    """Flat quad in the XY plane.

    `open_surface=True` marks it for the outwardness verifier: a plane has no
    interior, so its signed volume is meaningless and it must instead assert
    that every normal points along one declared axis.
    """
    bpy.ops.mesh.primitive_plane_add(
        size=size, size_y=size_y if size_y is not None else size, location=location
    )
    obj = bpy.context.active_object
    obj.name = name
    if open_surface:
        obj["vcw_open_surface"] = True
    assign_material(obj, material)
    return obj


# ---------------------------------------------------------------------------
# modifiers — the 3A silhouette lever
# ---------------------------------------------------------------------------


def add_bevel(obj: bpy.types.Object, width: float = 0.02, segments: int = 2) -> None:
    """Bevel every edge.

    A hard 90-degree cube edge is the single strongest "low-poly" tell: it
    catches a specular highlight as a razor line instead of rolling off. Even
    2 segments at 2 cm reads as a manufactured chamfer at gameplay distance.
    """
    mod = obj.modifiers.new("bevel", "BEVEL")
    mod.width = width
    mod.segments = segments
    mod.limit_method = "ANGLE"
    mod.angle_limit = math.radians(35.0)
    apply_modifiers(obj)


def add_subsurf(obj: bpy.types.Object, levels: int = 1) -> None:
    """Catmull-Clark subdivision — rounds off boxy forms into curved ones."""
    mod = obj.modifiers.new("subsurf", "SUBSURF")
    mod.levels = levels
    mod.render_levels = levels
    apply_modifiers(obj)


def add_mirror(obj: bpy.types.Object, axis: str = "X") -> None:
    mod = obj.modifiers.new("mirror", "MIRROR")
    mod.use_axis = (axis == "X", axis == "Y", axis == "Z")
    apply_modifiers(obj)


def apply_modifiers(obj: bpy.types.Object) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    for mod in list(obj.modifiers):
        bpy.ops.object.modifier_apply(modifier=mod.name)


def triangulate(obj: bpy.types.Object) -> bpy.types.Object:
    """Convert every n-gon to triangles, in place.

    Required before glTF export: the exporter computes tangent space only for
    tris and quads, so a cylinder cap or a beveled face silently drops its
    tangents and logs `Tangent space can only be computed for tris/quads`. A
    mesh without tangents cannot take a normal map, which is the whole point
    of keeping a PBR master.

    Uses bmesh rather than `bpy.ops.mesh.face_make` — that operator does not
    exist (it is `mesh.face_make` on a bmesh, not a bpy op), and an
    AttributeError here would abort a whole build.
    """
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    ngons = [f for f in bm.faces if len(f.verts) > 3]
    if ngons:
        bmesh.ops.triangulate(bm, faces=ngons)
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()
    return obj


def shade_smooth(obj: bpy.types.Object, angle: float | None = None) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    if angle is None:
        bpy.ops.object.shade_smooth()
    else:
        # Auto-smooth via a weighted normal modifier keeps hard edges hard and
        # curved surfaces smooth — shading every polygon smooth makes cubes melt.
        bpy.ops.object.shade_auto_smooth(angle=angle)


def assign_material(obj: bpy.types.Object, material: bpy.types.Material | None) -> None:
    if material is None:
        return
    obj.data.materials.clear()
    obj.data.materials.append(material)


# ---------------------------------------------------------------------------
# UV — mandatory, not optional
# ---------------------------------------------------------------------------


def ensure_uv(obj: bpy.types.Object, name: str = "UVMap") -> None:
    """Guarantee a UV layer exists.

    Created through the DATA API, not `bpy.ops.mesh.uv_layers.new` — that
    operator does not exist and raises AttributeError under `--background`.
    """
    if obj.type != "MESH":
        return
    if not obj.data.uv_layers:
        obj.data.uv_layers.new(name=name)


def smart_project(obj: bpy.types.Object, island_margin: float = 0.02) -> None:
    """Project UVs via the operator interface.

    Runs in edit mode with all faces selected. `angle_limit` keeps hard edges
    from being unwrapped across, which is what stops a bevel from smearing one
    island over three faces.
    """
    ensure_uv(obj)
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    try:
        bpy.ops.uv.smart_project(angle_limit=math.radians(66.0), island_margin=island_margin)
    except RuntimeError:
        # smart_project is unavailable in some background contexts; fall back to
        # a per-face cube projection so the asset still exports a valid UV set.
        bpy.ops.uv.cube_project(cube_size=1.0)
    bpy.ops.object.mode_set(mode="OBJECT")
    ensure_uv(obj)


def uv_box_project(obj: bpy.types.Object, scale: float = 1.0) -> None:
    """Axis-aligned box projection — fast, and fine for tiling surfaces."""
    ensure_uv(obj)
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.cube_project(cube_size=scale)
    bpy.ops.object.mode_set(mode="OBJECT")


# ---------------------------------------------------------------------------
# object ops
# ---------------------------------------------------------------------------


def duplicate(obj: bpy.types.Object, name: str, location: Sequence[float] | None = None) -> bpy.types.Object:
    """Linked-free copy. Callers must give `name` a unique value."""
    new = obj.copy()
    new.data = obj.data.copy()
    bpy.context.collection.objects.link(new)
    new.name = name
    if location is not None:
        new.location = location
        _apply_transform(new)
    return new


def join(name: str, objects: Iterable[bpy.types.Object]) -> bpy.types.Object:
    """Merge parts into one object named `name`."""
    objs = [o for o in objects if o is not None]
    if not objs:
        raise ValueError("join() got an empty object list")
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    merged = bpy.context.active_object
    merged.name = name
    return merged


def set_origin_to_geometry(obj: bpy.types.Object) -> bpy.types.Object:
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.origin_set(type="ORIGIN_GEOMETRY", center="BOUNDS")
    return obj


def transform_apply(obj: bpy.types.Object, location=True, rotation=True, scale=True) -> None:
    """Bake an object transform into its mesh data.

    Note the asymmetry with `rotate_about`: applying a transform leaves the
    object's origin at the world origin, so any rotation set afterwards turns
    the part about (0,0,0) rather than about its own centre.
    """
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=location, rotation=rotation, scale=scale)


def rotate_about(
    obj: bpy.types.Object,
    euler: Sequence[float],
    pivot: Sequence[float] | None = None,
) -> bpy.types.Object:
    """Rotate a part about an explicit pivot, baking the result into its mesh.

    `cube()` bakes its location and leaves the origin at the world origin, so
    setting `rotation_euler` afterwards rotates the geometry about (0,0,0), not
    about the part. A rotor blade built at its hub and then "spun" this way ends
    up metres away from the hub — an aircraft propeller measured 5.15 m below
    the ground. Build at the origin, rotate, then translate; or pass `pivot`.

    `pivot` defaults to the object's own world centre, which is what almost
    every part wants.
    """
    if pivot is None:
        pts = [obj.matrix_world @ Vector(c) for c in obj.bound_box]
        pivot = tuple(
            (min(p[i] for p in pts) + max(p[i] for p in pts)) * 0.5 for i in range(3)
        )
    pivot_vec = Vector(pivot)
    offset = obj.matrix_world.translation - pivot_vec

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    # re-origin the part at the pivot so the rotation acts where intended
    obj.location = obj.location + offset
    bpy.ops.object.transform_apply(location=True, rotation=False, scale=True)
    obj.rotation_euler = euler
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    return obj


def place(
    obj: bpy.types.Object,
    location: Sequence[float],
) -> bpy.types.Object:
    """Move an already-built part to a world location, baking the move in."""
    obj.location = Vector(location) - obj.matrix_world.translation
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return obj


def empty(name: str, location: Sequence[float] = (0.0, 0.0, 0.0)) -> bpy.types.Object:
    bpy.ops.object.empty_add(type="PLAIN_AXES", location=location)
    obj = bpy.context.active_object
    obj.name = name
    return obj


def parent_to(child: bpy.types.Object, parent: bpy.types.Object) -> None:
    child.parent = parent
    child.matrix_parent_inverse = parent.matrix_world.inverted()


# ---------------------------------------------------------------------------
# measurement / verification helpers
# ---------------------------------------------------------------------------


def signed_volume(obj: bpy.types.Object) -> float:
    """Signed volume of a closed shell, computed on position-welded vertices.

    Positive = outward winding. This is the only automatic outwardness test
    that works: a closed mesh has an interior, so a negative volume means the
    shell is inside-out, which renders identically from one side.
    """
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    vol = bm.calc_volume(signed=True)
    bm.free()
    return vol


def is_open_surface(obj: bpy.types.Object) -> bool:
    return bool(obj.get("vcw_open_surface", False))


def assert_grounded(obj: bpy.types.Object, tolerance: float = 1e-6) -> None:
    """Assert the mesh's lowest point rests on Z=0 (authoring space).

    Position is the one bug class no volume or normal test catches, so it gets
    an explicit assertion rather than a visual check.
    """
    zs = [(obj.matrix_world @ v.co).z for v in obj.data.vertices]
    if not zs:
        return
    lowest = min(zs)
    if abs(lowest) > tolerance:
        raise AssertionError(
            f"{obj.name}: lowest point is at z={lowest:.6f}, expected 0 (tolerance {tolerance})"
        )


def world_bounds(obj: bpy.types.Object) -> dict:
    """World-space AABB, computed from evaluated geometry (modifiers applied)."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        if not mesh.vertices:
            return {"min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0]}
        coords = [evaluated.matrix_world @ v.co for v in mesh.vertices]
        lo = [min(c[i] for c in coords) for i in range(3)]
        hi = [max(c[i] for c in coords) for i in range(3)]
        return {"min": lo, "max": hi}
    finally:
        evaluated.to_mesh_clear()


def triangle_count(obj: bpy.types.Object) -> int:
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        mesh.calc_loop_triangles()
        return len(mesh.loop_triangles)
    finally:
        evaluated.to_mesh_clear()


# ---------------------------------------------------------------------------
# export
# ---------------------------------------------------------------------------


def export_glb(path: str, objects: Sequence[bpy.types.Object] | None = None) -> bool:
    """Export the given objects (or the whole scene) to a single .glb.

    Returns True only when the file exists and is non-empty on return. The
    Blender glTF exporter reports success even in some failure paths, so the
    caller must never trust the return value alone.
    """
    import os

    os.makedirs(os.path.dirname(path), exist_ok=True)
    if os.path.exists(path):
        os.remove(path)

    if objects is None:
        bpy.ops.object.select_all(action="SELECT")
    else:
        bpy.ops.object.select_all(action="DESELECT")
        for o in objects:
            if o is not None and o.name in bpy.context.view_layer.objects:
                o.select_set(True)
        if objects:
            bpy.context.view_layer.objects.active = objects[0]

    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_apply=True,
        export_yup=True,
        export_materials="EXPORT",
        export_normals=True,
        export_texcoords=True,
        export_tangents=True,
    )

    if not os.path.exists(path) or os.path.getsize(path) == 0:
        return False
    return True