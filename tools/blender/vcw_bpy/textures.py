"""Procedural PBR texture generation.

The 99 assets ship real bpy geometry with real material scalars (metallic /
roughness / alphaMode), but until this module existed they shipped **zero
images** — a GLB with ``images: 0`` renders every surface as flat base colour.
This fills the third layer: albedo, normal and ORM maps generated from the
material's semantic surface kind.

Design constraints that shaped every function here:

* **No external assets.** Every texel is computed from noise, so the library
  is reproducible on a clean checkout and carries no licensing question.
* **One image per material, shared across assets.** A 512² RGB PNG is ~200 KB;
  per-asset copies of a handful of materials would dominate the library size.
  Materials are therefore named by *surface kind* (``paint_coral``),
  not per part, and the exporter dedupes by material name.
* **Normal maps are derived from the same height field as the albedo**, so
  the lighting and the colour agree. Generating them independently produces
  the classic "detail that fights the paint" artefact.

Everything is written into Blender's image datablocks by direct pixel writes;
no numpy, no external image library.
"""

from __future__ import annotations

import hashlib
import math

import bpy

# Texel resolution. 512 is the smallest power of two that still resolves the
# 1–2 px scratches and the asphalt grain at the distances this scene uses.
# Going to 1024 quadruples PNG size for detail that is sub-pixel past ~20 m.
TEX_SIZE: int = 512

# Textures are keyed by **material identity**, not by surface kind. Keying by
# kind alone made every painted part in the library share one datablock — three
# different colours became three images with identical content. Blender also
# de-duplicates same-named images as `name.001`, `name.002`, … so the datablock
# lookup in `_new_image` is what actually keeps the count bounded: one albedo,
# one normal and one ORM per distinct material, built once.


def _hash01(*parts: object) -> float:
    """A deterministic pseudo-random float in 0..1 from any hashable parts.

    Python's `hash()` is salted per process, so using it would make every
    rebuild produce different textures — which defeats the whole point of a
    reproducible library. blake2b is stable across runs and machines.
    """
    key: str = "|".join(str(part) for part in parts)
    digest: bytes = hashlib.blake2b(key.encode("utf-8"), digest_size=8).digest()
    return int.from_bytes(digest, "big") / float(1 << 64)


# Noise octaves are addressed by index, not by a seed integer, so the table
# above can be built once at import time instead of per call.
_OCTAVES: int = 4
_OCTAVE_SEEDS: tuple[int, ...] = tuple(977 * octave for octave in range(_OCTAVES))


def _hash_int(*parts: object) -> int:
    """A deterministic int in 0..255 from any hashable parts (stable across runs)."""
    key: str = "|".join(str(part) for part in parts)
    digest: bytes = hashlib.blake2b(key.encode("utf-8"), digest_size=4).digest()
    return int.from_bytes(digest, "big") & 255


# One 256-entry permutation table per octave. Built once at import: 4 tables ×
# 256 entries = 1024 hashes total, versus the 16-per-texel the naive version
# needed.
_PERMUTATION: tuple[list[int], ...] = tuple(
    [_hash_int(octave, index) for index in range(256)] for octave in _OCTAVE_SEEDS
)


def _lattice(perm: list[int], xi: int, yi: int) -> float:
    """One lattice corner value in 0..1 from two integer coordinates."""
    return perm[(perm[xi & 255] + (yi & 255)) & 255] / 255.0


def _value_noise(x: float, y: float, octave: int) -> float:
    """Smooth value noise in 0..1 with bilinear interpolation.

    Plain lattice noise without interpolation would produce a visible 1-texel
    grid; smoothstep on the fractional cell coordinate is the cheapest fix that
    survives being sampled at 512².
    """
    xi: int = math.floor(x)
    yi: int = math.floor(y)
    xf: float = x - xi
    yf: float = y - yi
    # Smoothstep the fraction so the lattice corners do not show through.
    u: float = xf * xf * (3.0 - 2.0 * xf)
    v: float = yf * yf * (3.0 - 2.0 * yf)
    perm: list[int] = _PERMUTATION[octave % _OCTAVES]
    n00: float = _lattice(perm, xi, yi)
    n10: float = _lattice(perm, xi + 1, yi)
    n01: float = _lattice(perm, xi, yi + 1)
    n11: float = _lattice(perm, xi + 1, yi + 1)
    return (n00 * (1.0 - u) + n10 * u) * (1.0 - v) + (n01 * (1.0 - u) + n11 * u) * v


def _fbm(x: float, y: float, octave_base: int, octaves: int = 4) -> float:
    """Fractal Brownian motion — layered value noise, normalised to 0..1."""
    total: float = 0.0
    amplitude: float = 1.0
    frequency: float = 1.0
    norm: float = 0.0
    for step in range(octaves):
        total += amplitude * _value_noise(
            x * frequency, y * frequency, octave_base + step
        )
        norm += amplitude
        amplitude *= 0.5
        frequency *= 2.0
    return total / norm


def _height_for_kind(kind: str, size: int, octave: int) -> list[float]:
    """Build a height field in 0..1 for one surface kind.

    The height field is the single source of truth for that surface: the albedo
    is tinted by it and the normal map is differentiated from it. Generating
    the three maps from independent noise would make bumps and colour disagree,
    which reads as dirt that is not aligned to any physical feature.
    """
    field: list[float] = [0.0] * (size * size)
    for y in range(size):
        v: float = y / size
        for x in range(size):
            u: float = x / size
            value: float
            if kind == "asphalt":
                # Coarse aggregate plus a fine grain. The coarse octave is what
                # makes a road read as a road at a distance; without it asphalt
                # is just dark grey.
                coarse: float = _fbm(u * 9.0, v * 9.0, octave, octaves=3)
                fine: float = _fbm(u * 64.0, v * 64.0, octave + 1, octaves=2)
                value = 0.45 * coarse + 0.35 * fine
            elif kind == "concrete":
                # Broad blotching plus pitting; concrete stains in patches,
                # it does not grain uniformly.
                blotch: float = _fbm(u * 5.0, v * 5.0, octave, octaves=4)
                pits: float = _fbm(u * 40.0, v * 40.0, octave + 1, octaves=2)
                value = 0.7 * blotch + 0.3 * pits
            elif kind == "paint":
                # Car paint is near-flat with a very faint orange peel. Too
                # much and the car reads as a rough-cast toy.
                peel: float = _fbm(u * 22.0, v * 22.0, octave, octaves=3)
                value = 0.5 + 0.12 * (peel - 0.5)
            elif kind == "metal":
                # Brushed directionality: stretch the noise along U.
                brush: float = _fbm(u * 120.0, v * 3.0, octave, octaves=2)
                value = 0.5 + 0.10 * (brush - 0.5)
            elif kind == "glass":
                # Glass is flat. Any relief would fake a surface that is not
                # there; the character comes from the Fresnel term, not a map.
                value = 0.5
            elif kind == "fabric":
                # A woven cross-hatch: two perpendicular stripe fields.
                warp: float = 0.5 + 0.5 * math.sin(u * math.tau * 48.0)
                weft: float = 0.5 + 0.5 * math.sin(v * math.tau * 48.0)
                fuzz: float = _fbm(u * 70.0, v * 70.0, octave, octaves=2)
                value = 0.45 * warp + 0.35 * weft + 0.2 * fuzz
            elif kind == "foliage":
                # Leaf veining: a central rib with lateral branches.
                vein: float = abs(math.sin(u * math.tau * 12.0 + v * 26.0))
                leaf: float = _fbm(u * 14.0, v * 14.0, octave, octaves=3)
                value = 0.55 * (1.0 - vein) + 0.45 * leaf
            elif kind == "rubber":
                # Fine moulding grain; rubber reads as rubber mostly by its
                # roughness, so relief stays subtle.
                grain: float = _fbm(u * 55.0, v * 55.0, octave, octaves=2)
                value = 0.5 + 0.08 * (grain - 0.5)
            elif kind == "skin":
                # Very fine pore texture plus a slow colour drift.
                pore: float = _fbm(u * 110.0, v * 110.0, octave, octaves=2)
                drift: float = _fbm(u * 6.0, v * 6.0, octave + 1, octaves=3)
                value = 0.6 * pore + 0.4 * drift
            else:
                # Unknown kind: a neutral grain is better than a flat surface,
                # because a flat albedo is what makes procedural work look
                # unfinished.
                value = _fbm(u * 18.0, v * 18.0, octave, octaves=3)
            field[y * size + x] = value
    return field


def _normal_from_height(height: list[float], size: int, strength: float) -> list[float]:
    """Tangent-space normal map derived by central-differencing the height field.

    Wrapping the sample indices is what keeps the map seamless: without it the
    left and right edges disagree by a texel and every tiled surface shows a
    hard vertical seam.
    """
    pixels: list[float] = [0.0] * (size * size * 4)
    for y in range(size):
        yn: int = (y - 1) % size
        yp: int = (y + 1) % size
        for x in range(size):
            xn: int = (x - 1) % size
            xp: int = (x + 1) % size
            dx: float = height[y * size + xp] - height[y * size + xn]
            dy: float = height[yp * size + x] - height[yn * size + x]
            # Tangent-space normal: (-dx, -dy, 1) normalised.
            nx: float = -dx * strength
            ny: float = -dy * strength
            nz: float = 1.0
            length: float = math.sqrt(nx * nx + ny * ny + nz * nz)
            index: int = (y * size + x) * 4
            pixels[index] = (nx / length) * 0.5 + 0.5
            pixels[index + 1] = (ny / length) * 0.5 + 0.5
            pixels[index + 2] = (nz / length) * 0.5 + 0.5
            pixels[index + 3] = 1.0
    return pixels


def _albedo_from_height(
    height: list[float],
    size: int,
    rgb: tuple[float, float, float],
    contrast: float,
) -> list[float]:
    """Tint the material's base colour by its own height field.

    Multiplicative rather than additive: a recess should read as the same
    material in shadow, not as a different pigment, and clamping the product
    keeps highlights from blowing out on the metal kind.
    """
    pixels: list[float] = [0.0] * (size * size * 4)
    for y in range(size):
        for x in range(size):
            value: float = height[y * size + x]
            # Normalise around 0.5 so the mean colour stays the authored one.
            shade: float = 1.0 + (value - 0.5) * contrast
            index: int = (y * size + x) * 4
            for channel in range(3):
                pixels[index + channel] = min(max(rgb[channel] * shade, 0.0), 1.0)
            pixels[index + 3] = 1.0
    return pixels


def _orm_from_height(
    height: list[float],
    size: int,
    base_roughness: float,
    metallic: float,
) -> list[float]:
    """Occlusion / roughness / metallic packed into one RGB image.

    GLTF's `metallicRoughnessTexture` reads **G = roughness, B = metallic** and
    uses **R as occlusion**, so one image carries all three channels and the
    exporter needs a single texture binding.
    """
    pixels: list[float] = [0.0] * (size * size * 4)
    for y in range(size):
        for x in range(size):
            value: float = height[y * size + x]
            # Recesses are both occluded and slightly rougher — the same field
            # drives both, which is what keeps the maps coherent.
            occlusion: float = min(max(0.55 + 0.45 * value, 0.0), 1.0)
            roughness: float = min(
                max(base_roughness * (0.85 + 0.30 * (1.0 - value)), 0.04),
                1.0,
            )
            index: int = (y * size + x) * 4
            pixels[index] = occlusion
            pixels[index + 1] = roughness
            pixels[index + 2] = metallic
            pixels[index + 3] = 1.0
    return pixels


def _new_image(name: str, size: int, pixels: list[float]) -> bpy.types.Image:
    """Create a datablock and write the pixels straight into it.

    ⚠️ Must be idempotent per material. `bpy.data.images.new` **silently
    de-duplicates by appending `.001`, `.002`, …** to a name that already
    exists, so calling this once per material *slot* (rather than once per
    distinct material) minted dozens of duplicate datablocks per asset — the
    prop pilot ended up at 11.6 MB of repeated identical PNGs with names like
    `vcw_tex_paint_prop_bench_frame_orm.005`. Look up the existing datablock
    first, and callers must reuse the result for the same material.
    """
    existing: bpy.types.Image | None = bpy.data.images.get(name)
    if existing is not None:
        return existing
    image: bpy.types.Image = bpy.data.images.new(
        name, width=size, height=size, alpha=True
    )
    image.pixels.foreach_set(pixels)
    # A generated image has no file behind it; without this the glTF exporter
    # skips it because it cannot resolve a source path.
    image.use_fake_user = True
    return image


def mark_kind(material: bpy.types.Material, kind: str) -> None:
    """Tag a material with the surface family it was authored as.

    Name matching cannot do this job. Builders rename every material per part
    (`prop_fire_hydrant_body`, `veh_sedan_tyre`), so the palette prefix is
    usually absent from the final name and every surface classifies as
    `generic` — which produced a pilot where the whole library got one
    duplicated ORM image and no albedo.

    The palette knows the truth at the moment it constructs the material, so
    it writes the answer into a custom property and this module reads that
    instead of guessing. `surface_kind_for` stays as the fallback for
    materials tagged by something other than the palette.
    """
    if material is not None:
        material["vcw_surface_kind"] = kind


def mark_emissive(material: bpy.types.Material) -> None:
    """Record that this material emits light, so it is skipped by texturing."""
    if material is not None:
        material["vcw_emissive"] = 1


def kind_of(material: bpy.types.Material) -> str:
    """The tagged surface kind, falling back to name matching."""
    tagged: str = str(material.get("vcw_surface_kind", ""))
    if tagged:
        return tagged
    return surface_kind_for(material.name)


def surface_kind_for(material_name: str) -> str:
    """Map a material name onto a surface kind.

    ⚠️ Palette materials are renamed per part by the builders, so the real name
    on disk is `<asset id>_<role>` — `prop_fire_hydrant_body`,
    `veh_sedan_chrome`, `bldg_tower_glass`. A plain ``startswith`` against the
    palette prefixes therefore matched **nothing** and every surface fell
    through to `generic`, which is how the first pilot shipped two duplicate
    `vcw_tex_generic_orm` images and no albedo at all.

    So the palette prefix is matched anywhere in the name, and the *first*
    match wins with the more specific families ordered first (`window_glass`
    before `glass`, `chrome` before `metal`).
    """
    lowered: str = material_name.lower()
    for prefix, kind in (
        ("window_glass", "glass"),
        ("chrome", "metal"),
        ("glass", "glass"),
        ("metal", "metal"),
        ("rubber", "rubber"),
        ("foliage", "foliage"),
        ("fabric", "fabric"),
        ("skin", "skin"),
        ("asphalt", "asphalt"),
        ("concrete", "concrete"),
        ("emissive", "glass"),
        ("paint", "paint"),
    ):
        if prefix in lowered:
            return kind
    return "generic"


def apply_textures(
    material: bpy.types.Material,
    rgb: tuple[float, float, float],
    roughness: float,
    metallic: float,
    size: int = TEX_SIZE,
) -> bool:
    """Attach albedo / normal / ORM images to one material.

    Returns True when textures were wired up and False when the material has
    no node tree or is emissive-only. Callers must treat the return value as
    informational: a material without textures still exports and still renders,
    it just renders flat.

    # Arguments

    - `material` - The material to texture.
    - `rgb` - Its linear base colour, used to tint the albedo.
    - `roughness` - Its base roughness, used as the ORM centre.
    - `metallic` - Its metalness, written to the ORM blue channel.
    - `size` - Texel resolution; smaller for a quick pilot.

    # Returns

    - `bool` - True when all three maps are bound.
    """
    if material is None or not material.use_nodes:
        return False
    tree: bpy.types.NodeTree = material.node_tree
    bsdf: bpy.types.Node | None = tree.nodes.get("Principled BSDF")
    if bsdf is None:
        return False

    kind: str = kind_of(material)
    # Emissive surfaces must not be tinted by an albedo map: the emissive
    # socket already carries the colour and a lit sign would go muddy.
    if kind == "glass" and material.get("vcw_emissive"):
        return False

    octave: int = int(_hash01(kind, material.name) * _OCTAVES) % _OCTAVES
    height: list[float] = _height_for_kind(kind, size, octave)
    # ⚠️ The image name must carry the **material identity**, not just the
    # surface kind. Keying on `kind` alone made every painted part in the
    # library share one datablock called `vcw_tex_paint_orm`: three different
    # colours became three images with identical content, and because
    # `bpy.data.images.new` de-duplicates by name the later ones overwrote the
    # earlier ones. The first pilot shipped 3 identical `paint_orm` images.
    prefix: str = f"vcw_tex_{kind}_{material.name}"

    contrast: float = {
        "asphalt": 0.55,
        "concrete": 0.40,
        "fabric": 0.30,
        "foliage": 0.35,
        "skin": 0.18,
        "metal": 0.22,
        "rubber": 0.20,
        "glass": 0.0,
        "paint": 0.14,
    }.get(kind, 0.28)
    normal_strength: float = {
        "asphalt": 26.0,
        "concrete": 16.0,
        "fabric": 10.0,
        "foliage": 12.0,
        "skin": 6.0,
        "metal": 7.0,
        "rubber": 8.0,
        "glass": 0.0,
        "paint": 4.0,
    }.get(kind, 10.0)

    albedo: bpy.types.Image = _new_image(
        f"{prefix}_albedo", size, _albedo_from_height(height, size, rgb, contrast)
    )
    normal: bpy.types.Image = _new_image(
        f"{prefix}_normal", size, _normal_from_height(height, size, normal_strength)
    )
    orm: bpy.types.Image = _new_image(
        f"{prefix}_orm",
        size,
        _orm_from_height(height, size, roughness, metallic),
    )

    base_tex: bpy.types.Node = tree.nodes.new("ShaderNodeTexImage")
    base_tex.image = albedo
    base_tex.name = "vcw_base_color"
    normal_tex: bpy.types.Node = tree.nodes.new("ShaderNodeTexImage")
    normal_tex.image = normal
    normal_tex.name = "vcw_normal"
    orm_tex: bpy.types.Node = tree.nodes.new("ShaderNodeTexImage")
    orm_tex.image = orm
    orm_tex.name = "vcw_orm"

    tree.links.new(base_tex.outputs["Color"], bsdf.inputs["Base Color"])
    tree.links.new(normal_tex.outputs["Color"], bsdf.inputs["Normal"])
    # GLTF packs occlusion in R, roughness in G, metallic in B; feed the same
    # image to both sockets and let the exporter emit one binding.
    tree.links.new(orm_tex.outputs["Color"], bsdf.inputs["Roughness"])
    tree.links.new(orm_tex.outputs["Color"], bsdf.inputs["Metallic"])

    material["vcw_textured"] = 1
    return True