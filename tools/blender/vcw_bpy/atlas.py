"""Export texture atlases in the runtime format the engine actually loads.

The engine fetches `assets/*.json` over HTTP and never looks at the GLB, so a
texture that only reaches the GLB is invisible at runtime. This module writes
the same three maps (albedo / normal / ORM) as **raw RGBA8 blobs** plus a
per-part reference, in a format the Rust side can hand straight to
`tex_image_2d` with no decoding step.

## Why raw RGBA8 and not PNG

PNG needs a real decoder in Rust — inflate, interlacing, colour types, filters.
That is a large, bug-prone dependency for bytes the GPU would upload verbatim
anyway. A 128² RGBA8 blob is 64 KiB and uploads with zero decoding, so the
trade is size for a decoder that does not exist. The texel density is already
tuned to 128², so nothing is lost on screen.

Each atlas is deduplicated per asset: one blob per **surface family**, reused by
every part that shares it, which is what keeps a 4 688-part tower at a sane
payload instead of one atlas per part.
"""

from __future__ import annotations

import json
import os
import struct
import zlib
from typing import Sequence

import bpy  # type: ignore[import-unknown]

from . import textures as TX

# Raw RGBA8 is 4 bytes per texel; the format is fixed so the Rust side can
# compute `width * height * 4` without reading a header.
BYTES_PER_TEXEL: int = 4
# Every atlas in one file is the same size, so the header stays trivial.
ATLAS_SIZE: int = TX.TEX_SIZE


def _pixels_to_bytes(pixels: Sequence[float]) -> bytes:
    """Pack a linear float RGBA list (0..1, row-major) into RGBA8 bytes."""
    out: bytearray = bytearray(len(pixels))
    for index in range(len(pixels)):
        value: float = pixels[index]
        # Clamp before the byte cast: `bytes()` of an out-of-range float raises,
        # and a negative height offset can push a channel slightly out of range.
        if value <= 0.0:
            out[index] = 0
        elif value >= 1.0:
            out[index] = 255
        else:
            out[index] = int(value * 255.0 + 0.5)
    return bytes(out)


def collect_atlases(
    objects: Sequence[bpy.types.Object],
) -> tuple[dict[str, bytes], dict[str, str]]:
    """Build one atlas per distinct surface family used by these objects.

    # Arguments

    - `objects` - The parts of one asset.

    # Returns

    - `tuple[dict[str, bytes], dict[str, str]]` - The atlas payloads keyed by
      `"<kind>/<map>"`, and each part's surface family keyed by part name so
      the asset JSON can point a part at its atlas.
    """
    atlases: dict[str, bytes] = {}
    kinds: dict[str, str] = {}
    seen: set[str] = set()
    for obj in objects:
        if obj is None:
            continue
        part_name: str = getattr(obj, "name", "")
        for slot in getattr(obj.data, "materials", []):
            if slot is None:
                continue
            kind: str = TX.kind_of(slot)
            # A part keeps its first non-emissive material, matching how the
            # GLB picks the dominant surface for a multi-material part.
            if part_name in kinds or slot.get("vcw_emissive"):
                continue
            kinds[part_name] = kind
            if kind in seen:
                continue
            seen.add(kind)
            rgb, rough, metal = _response(slot)
            for map_name, pixels in TX.maps_for(slot, rgb, rough, metal):
                atlases[f"{kind}/{map_name}"] = _pixels_to_bytes(pixels)
    return atlases, kinds


def _response(material: bpy.types.Material) -> tuple[tuple[float, float, float], float, float]:
    """(base colour, roughness, metallic) read the way Blender 4 stores them."""
    from .export3a import material_response

    return material_response(material)


def write_atlas_file(path: str, atlases: dict[str, bytes]) -> bool:
    """Write all atlases for one asset into a single `.bin`.

    Layout: `count u32`, then per atlas `key_len u32`, key bytes,
    `byte_len u32`, payload. One request per asset instead of one per map, and
    no decoding step on the Rust side beyond reading this header.
    """
    if not atlases:
        return False
    os.makedirs(os.path.dirname(path), exist_ok=True)
    body: bytearray = bytearray(struct.pack("<I", len(atlases)))
    for key in sorted(atlases):
        payload: bytes = atlases[key]
        name: bytes = key.encode("utf-8")
        body += struct.pack("<I", len(name)) + name
        body += struct.pack("<I", len(payload)) + payload
    with open(path, "wb") as handle:
        handle.write(bytes(body))
    return True


def attach_to_asset(asset_path: str, kinds: dict[str, str], atlas_rel: str) -> bool:
    """Record the atlas reference on the asset JSON, per part.

    The engine deserialises parts with `#[serde(default)]` everywhere, so this
    is additive: an asset without these fields keeps working exactly as before.
    """
    if not kinds or not os.path.exists(asset_path):
        return False
    with open(asset_path, encoding="utf-8") as handle:
        asset: dict = json.load(handle)
    touched: int = 0
    for part in asset.get("parts", []):
        kind: str | None = kinds.get(part.get("name", ""))
        if kind is None:
            continue
        part["texture_atlas"] = atlas_rel
        part["surface_kind"] = kind
        touched += 1
    if touched == 0:
        return False
    with open(asset_path, "w", encoding="utf-8") as handle:
        json.dump(asset, handle, ensure_ascii=False, separators=(",", ":"))
    return True


def write_asset_textures(
    asset_id: str,
    objects: Sequence[bpy.types.Object],
    asset_json: str,
    atlas_dir: str,
) -> tuple[int, int]:
    """Export one asset's textures.

    # Returns

    - `tuple[int, int]` - (atlas count, byte size written).
    """
    atlases, kinds = collect_atlases(objects)
    if not atlases:
        return 0, 0
    path: str = os.path.join(atlas_dir, f"{asset_id}.bin")
    if not write_atlas_file(path, atlases):
        return 0, 0
    rel: str = f"atlas/{asset_id}.bin"
    attach_to_asset(asset_json, kinds, rel)
    return len(atlases), os.path.getsize(path)