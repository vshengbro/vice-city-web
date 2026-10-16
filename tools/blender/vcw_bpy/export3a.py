"""Export a bpy scene to the game's schema-compatible JSON, plus a PBR master.

Two artifacts per asset, and the split is forced by the consumer:

* ``assets/<id>.json`` — what ``src/mesh.rs`` actually deserialises. The
  ``MeshPart`` struct has positions / normals / faces / base_color / emissive /
  face_colors / flat and **nothing else**: no uvs, no tangents, no roughness,
  no metallic. Emitting only ``.glb`` would leave the game unable to load any
  asset at all, so this file is mandatory.
* ``assets/pbr/<id>.glb`` — the PBR master (UVs, tangents, Principled
  materials). This is the file a renderer upgrade consumes later. Shipping it
  now means the modelling work is not wasted when the vertex format grows.

Coordinate transform, from the shipped schema: authoring is Z-up, JSON is
``(x, z, -y)`` — determinant +1, so winding and handedness are preserved.
"""

from __future__ import annotations

import json
import re
import math
import os
from typing import Sequence

import bpy
from mathutils import Vector

Z_UP_TO_Y_UP = ((1.0, 0.0, 0.0), (0.0, 0.0, 1.0), (0.0, -1.0, 0.0))


def to_json_space(v: Vector) -> list[float]:
    """Authoring (x, y, z) Z-up -> JSON (x, z, -y) Y-up."""
    return [float(v.x), float(v.z), float(-v.y)]


def _material_base_color(mat: bpy.types.Material | None) -> list[float]:
    """Read the Principled Base Color out of a material, defaulting to white."""
    if mat is None:
        return [1.0, 1.0, 1.0]
    if not mat.use_nodes:
        return [1.0, 1.0, 1.0]
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    if bsdf is None or "Base Color" not in bsdf.inputs:
        return [1.0, 1.0, 1.0]
    rgba = bsdf.inputs["Base Color"].default_value
    return [float(rgba[0]), float(rgba[1]), float(rgba[2])]


def _material_emissive(mat: bpy.types.Material | None) -> list[float]:
    if mat is None or not mat.use_nodes:
        return [0.0, 0.0, 0.0]
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    if bsdf is None or "Emission Color" not in bsdf.inputs:
        return [0.0, 0.0, 0.0]
    rgba = bsdf.inputs["Emission Color"].default_value
    strength = 1.0
    if "Emission Strength" in bsdf.inputs:
        strength = float(bsdf.inputs["Emission Strength"].default_value)
    return [float(rgba[0]) * strength, float(rgba[1]) * strength, float(rgba[2]) * strength]


def _averaged_normals(positions, faces) -> list[list[float]]:
    """Area-weighted vertex normals computed from the welded face list.

    Derived from the exported geometry rather than read from Blender's corner
    normals, because the corner normals no longer line up once vertices are
    welded by position. An isolated vertex (referenced by no face) falls back
    to straight up rather than producing a zero-length vector that would
    normalize to NaN downstream.
    """
    acc = [[0.0, 0.0, 0.0] for _ in positions]

    for f in faces:
        if len(f) < 3:
            continue
        a, b, c = positions[f[0]], positions[f[1]], positions[f[2]]
        ux, uy, uz = b[0] - a[0], b[1] - a[1], b[2] - a[2]
        vx, vy, vz = c[0] - a[0], c[1] - a[1], c[2] - a[2]
        # cross product magnitude is twice the triangle area, so using the
        # cross vector directly is already area weighting
        nx = uy * vz - uz * vy
        ny = uz * vx - ux * vz
        nz = ux * vy - uy * vx
        for vi in f[:3]:
            acc[vi][0] += nx
            acc[vi][1] += ny
            acc[vi][2] += nz

    out: list[list[float]] = []
    for n in acc:
        length = math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2])
        if length < 1e-12:
            out.append([0.0, 1.0, 0.0])
        else:
            out.append([n[0] / length, n[1] / length, n[2] / length])
    return out


def _canonical_order(positions, faces):
    """Renumber vertices into a build-order-independent sequence.

    Blender does not promise a stable `loop_triangles` order between runs, so
    two identical builds emit the same triangles in a different sequence. The
    geometry is equal as a multiset, but every JSON file differs byte-wise and
    `git diff` shows every asset as changed on each rebuild.

    Sorting faces by their own vertex coordinates and then renumbering in
    first-encounter order makes the output depend only on the geometry.
    """
    key = [tuple(round(c, 6) for c in p) for p in positions]
    order = sorted(range(len(faces)), key=lambda i: sorted(key[v] for v in faces[i]))
    remap: dict[int, int] = {}
    out_positions: list[list[float]] = []
    out_faces: list[list[int]] = []
    for fi in order:
        face = []
        for v in faces[fi]:
            nv = remap.get(v)
            if nv is None:
                nv = len(out_positions)
                remap[v] = nv
                out_positions.append(positions[v])
            face.append(nv)
        out_faces.append(face)
    return out_positions, out_faces


def _object_to_part(obj: bpy.types.Object, matrix: Vector | None = None) -> dict:
    """Convert one evaluated object into a MeshPart dict.

    Vertices are de-duplicated **by position only**. Keying the map on
    position+normal instead looks harmless and is actively wrong: a smooth-
    shaded or beveled mesh stores a different normal per corner, so the weld
    never merges and every triangle becomes 3 independent vertices. The shell
    stops being closed, the outwardness signed-volume test collapses from the
    true 5.36 to a meaningless -0.06, and a correctly-modelled car is reported
    as inside-out. Position-only welding is what keeps the shell closed.
    """
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        mesh.calc_loop_triangles()

        world = evaluated.matrix_world
        positions: list[list[float]] = []
        index_map: dict[tuple, int] = {}

        def vertex_id(co) -> int:
            key = (round(co.x, 6), round(co.y, 6), round(co.z, 6))
            idx = index_map.get(key)
            if idx is None:
                idx = len(positions)
                index_map[key] = idx
                positions.append(to_json_space(co))
            return idx

        faces: list[list[int]] = []
        for tri in mesh.loop_triangles:
            ids = []
            for loop_index in tri.loops:
                loop = mesh.loops[loop_index]
                co = world @ mesh.vertices[loop.vertex_index].co
                ids.append(vertex_id(co))
            if len(set(ids)) == 3:
                faces.append(ids)

        positions, faces = _canonical_order(positions, faces)

        # `flat: true` makes the renderer derive normals per face, so a
        # smooth normal set is not needed here and would break the shell.
        # Emit averaged vertex normals from the welded positions instead.
        normals = _averaged_normals(positions, faces)

        # `flat: true` makes the renderer ignore these and use face normals, so
        # they are emitted for tooling but do not need to be smooth-correct.
        slot = obj.data.materials[0] if len(obj.data.materials) else None
        part = {
            "name": obj.name,
            "base_color": _material_base_color(slot),
            "emissive": _material_emissive(slot),
            "positions": positions,
            "normals": normals,
            "faces": faces,
            "flat": True,
        }
        return part
    finally:
        evaluated.to_mesh_clear()


# Part names the RENDERER looks up by exact string. `src/game.rs` has tests
# that panic with "asset is missing the <part> part" when a name does not
# appear, so these are an API contract, not a label: a builder may name its
# parts descriptively, but the exported document must expose these.
#
# Each entry maps (asset_id, contract name) -> the builder's part-name suffix.
# The suffix match is exact-or-prefixed, so `<aid>_fin` serves `surfboard_fin`.
# Keeping this in the exporter rather than in each builder means a builder can
# be renamed freely without silently breaking the consumer.
# The value is a REGEX matched against the whole part name, not a suffix:
# a suffix test matches `stringer`/`leash_plug` for `board` and nothing at all
# for `trim`, because the tower's strips are named `_trim0.._trim3`.
PART_NAME_CONTRACT: dict[tuple[str, str], str] = {
    ("beach_surfboard", "surfboard_deck"): r"^beach_surfboard_board$",
    ("beach_surfboard", "surfboard_fin"): r"^beach_surfboard_fin$",
    ("animal_bird", "animal_bird_beak"): r"^animal_bird_beak",
    ("animal_bird", "animal_bird_head"): r"^animal_bird_head$",
    # every per-window trim strip on a tower is a side frame
    ("bldg_lilac_tower", "window_side_frames"): r"^bldg_lilac_tower_.*_trim\d+$",
}


def _contract_names(asset_id: str, obj_name: str, taken: set) -> None:
    """Emit `contract` duplicates of a part the renderer looks up by name.

    A contract name that matches several source parts (every trim strip on a
    tower) is duplicated across all of them, so the consumer's extent scan sees
    the whole group rather than one arbitrary strip.
    """
    for (aid, contract), suffix in PART_NAME_CONTRACT.items():
        if aid != asset_id:
            continue
        if not re.match(suffix, obj_name):
            continue
        name = contract
        n = 2
        while name in taken:
            name = f"{contract}_{n}"
            n += 1
        taken.add(name)
        yield name


def build_asset_dict(
    asset_id: str,
    category: str,
    objects: Sequence[bpy.types.Object],
    *,
    include_normals: bool = True,
) -> dict:
    """Assemble the full asset document from a set of objects."""
    parts: list[dict] = []
    names_seen: set[str] = set()

    for obj in objects:
        if obj is None or obj.type != "MESH":
            continue
        # Duplicate part names make the data unaddressable for the renderer,
        # which looks parts up by name (tyres/hubs/body on a vehicle).
        name = obj.name
        if name in names_seen:
            suffix = 2
            while f"{name}_{suffix}" in names_seen:
                suffix += 1
            name = f"{name}_{suffix}"
            names_seen.add(name)
            obj.name = name
        else:
            names_seen.add(name)

        part = _object_to_part(obj)
        # _object_to_part uses obj.name; the dedup above may have renamed the
        # part, so carry the final name across explicitly.
        part["name"] = name
        if not include_normals:
            part.pop("normals", None)
        parts.append(part)

        # Duplicate this part under any contract name the renderer looks up.
        # Same geometry, different key: the consumer's name lookups and its
        # per-part iteration both keep working.
        for alias in _contract_names(asset_id, name, names_seen):
            parts.append({**part, "name": alias})

    tri_count = sum(len(p.get("faces", [])) for p in parts)

    # bounds computed from the JSON-space positions, so the value the renderer
    # reads matches the value the game will actually see.
    all_pts = [pt for p in parts for pt in p.get("positions", [])]
    if all_pts:
        lo = [min(pt[i] for pt in all_pts) for i in range(3)]
        hi = [max(pt[i] for pt in all_pts) for i in range(3)]
    else:
        lo = hi = [0.0, 0.0, 0.0]

    return {
        "id": asset_id,
        "category": category,
        "y_up": True,
        "bounds": {"min": lo, "max": hi},
        "part_count": len(parts),
        "tri_count": tri_count,
        "parts": parts,
    }


def write_asset_json(asset: dict, assets_dir: str) -> str:
    """Write `<assets_dir>/<id>.json`, returning the path."""
    os.makedirs(assets_dir, exist_ok=True)
    path = os.path.join(assets_dir, f"{asset['id']}.json")
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(asset, fh, separators=(",", ":"), ensure_ascii=False)
    return path


def material_response(material: bpy.types.Material | None) -> tuple[tuple[float, float, float], float, float]:
    """Read (base colour, roughness, metallic) the way Blender 4 actually stores them.

    ⚠️ `Material.metallic` and `Material.roughness` still exist in Blender 4.5
    but are **dead legacy fields**: the Principled BSDF keeps the real values in
    its node tree. Reading the legacy field returns 0.0 for every material made
    through `palette.paint(..., metallic=0.35)` — which silently writes an ORM
    image whose blue channel (metalness) is zero everywhere, i.e. every metal
    ships as a dielectric. Probe-verified: the legacy field says 0.0 while the
    BSDF socket says 0.35.

    So the node tree is authoritative and the legacy field is only a fallback
    for a material with no node tree at all.
    """
    if material is None:
        return (0.8, 0.8, 0.8), 0.6, 0.0
    base: list[float] = _material_base_color(material)
    rough: float = 0.6
    metal: float = 0.0
    if material.use_nodes:
        bsdf: bpy.types.Node | None = material.node_tree.nodes.get("Principled BSDF")
        if bsdf is not None:
            if "Roughness" in bsdf.inputs:
                rough = float(bsdf.inputs["Roughness"].default_value)
            if "Metallic" in bsdf.inputs:
                metal = float(bsdf.inputs["Metallic"].default_value)
            return (base[0], base[1], base[2]), rough, metal
    rough = float(getattr(material, "roughness", 0.6) or 0.6)
    metal = float(getattr(material, "metallic", 0.0) or 0.0)
    return (base[0], base[1], base[2]), rough, metal


def write_pbr_glb(asset_id: str, objects: Sequence[bpy.types.Object], pbr_dir: str) -> str | None:
    """Write the PBR master. Returns None when the export produced no file."""
    from .kernel import export_glb
    from . import textures as TX

    os.makedirs(pbr_dir, exist_ok=True)
    path = os.path.join(pbr_dir, f"{asset_id}.glb")
    live: Sequence[bpy.types.Object] = [o for o in objects if o is not None]
    # Texture the materials *before* the export: the glTF exporter reads image
    # datablocks live off each material, so anything applied afterwards would
    # not make it into the file. Textures are keyed by surface kind rather
    # than per part, so a 4 688-part tower still only builds ~10 images.
    textured: int = 0
    for obj in live:
        for slot in getattr(obj.data, "materials", []):
            if slot is None:
                continue
            rgb, rough, metal = material_response(slot)
            if TX.apply_textures(slot, rgb, rough, metal):
                textured += 1
    if not export_glb(path, list(live)):
        return None
    if textured:
        print(f"  textured {textured} material(s) -> {os.path.basename(path)}")
    return path


def write_manifest(assets: Sequence[dict], assets_dir: str, generator: str) -> str:
    """Manifest the renderer reads first to plan memory and culling.

    `assets` MUST be a JSON **array** of `{id, category, file}` objects.

    The Rust loader deserialises it into `Manifest { assets: Vec<ManifestEntry> }`
    (`src/game/struct.rs`), so a flat `{"<id>": <tri_count>}` map fails to
    parse: every entry is skipped, the scene builds empty, and the failure
    surfaces only as a silently blank city with no error in the console
    beyond "manifest parse failed".

    History: commit `bfac59a` corrected the schema **in the committed
    JSON** but left this generator emitting the old map, so the bug was
    latent until the next `build_all3a.py` run silently reverted it. This
    function is now the single source of truth for the shape.
    """
    categories: dict[str, dict] = {}
    for a in assets:
        cat = a.get("category", "unknown")
        entry = categories.setdefault(cat, {"count": 0, "tri_count": 0})
        entry["count"] += 1
        entry["tri_count"] += int(a.get("tri_count", 0))

    manifest = {
        "version": 2,
        "generator": generator,
        "up_axis": "y",
        "unit": "metre",
        "asset_count": len(assets),
        "tri_count": sum(int(a.get("tri_count", 0)) for a in assets),
        "categories": categories,
        "assets": [
            {
                "id": a["id"],
                "category": a.get("category", "unknown"),
                "file": a["file"],
            }
            for a in assets
        ],
    }
    path = os.path.join(assets_dir, "manifest.json")
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, separators=(",", ":"), ensure_ascii=False)
    return path