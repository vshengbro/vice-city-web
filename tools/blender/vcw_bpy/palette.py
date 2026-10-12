"""Shared palette + helpers for the 3A asset builders.

Kept in one place so parallel builders cannot drift on the material response
of, say, "glass" — a car windscreen and a tower window must not disagree about
what glass is. Every builder imports from here; none defines its own palette.
"""

from __future__ import annotations

import math

import bpy

from . import kernel as K


# --- shared PBR palette ----------------------------------------------------
# Miami-neon / pastel-decade, the same art direction the shipped library used,
# now as real Principled layers instead of baked vertex colours.

def paint(name: str, rgb, metallic: float = 0.0, roughness: float = 0.7):
    return K.pbr_material(name, rgb, roughness=roughness, metallic=metallic)


def glass(name: str, tint=(0.62, 0.78, 0.86), rough: float = 0.05):
    return K.pbr_material(name, tint, roughness=rough, metallic=0.0,
                          alpha=0.25, transmission=0.90, ior=1.52)


def window_glass(name: str, tint=(0.16, 0.28, 0.36)):
    """Facade glazing: darker and more reflective than vehicle glass, so a
    tower reads as a tower and not as a greenhouse."""
    return K.pbr_material(name, tint, roughness=0.07, metallic=0.60)


def rubber(name: str):
    return K.pbr_material(name, (0.035, 0.035, 0.04), roughness=0.88, metallic=0.0)


def chrome(name: str):
    return K.pbr_material(name, (0.86, 0.88, 0.92), roughness=0.08, metallic=1.0)


def metal(name: str, rgb=(0.62, 0.64, 0.68), rough: float = 0.35, metal: float = 0.9):
    return K.pbr_material(name, rgb, roughness=rough, metallic=metal)


def emissive(name: str, rgb, strength: float = 4.0):
    return K.pbr_material(name, rgb, roughness=0.3, metallic=0.0,
                          emission=rgb, emission_strength=strength)


def foliage(name: str, rgb=(0.18, 0.42, 0.20), rough: float = 0.72):
    """Leaves and foliage get a high roughness and two-sided shading — a leaf
    that reflects like plastic is the fastest way to make a tree look CG."""
    return K.pbr_material(name, rgb, roughness=rough, metallic=0.0)


def fabric(name: str, rgb, rough: float = 0.86):
    return K.pbr_material(name, rgb, roughness=rough, metallic=0.0)


def skin(name: str, rgb=(0.78, 0.60, 0.48)):
    return K.pbr_material(name, rgb, roughness=0.55, metallic=0.0)


def finish(parts):
    """Standard tail step for every builder: UV layer + triangulate.

    Triangulation is not optional here — the glTF exporter computes tangent
    space only for tris/quads and silently drops it for every n-gon otherwise,
    which leaves the PBR master unable to carry a normal map.
    """
    for obj in parts:
        if obj is not None and obj.type == "MESH":
            K.ensure_uv(obj)
            K.triangulate(obj)
    return parts


def lay(parts, name: str, origin_at_feet: bool = True):
    """Join parts into one object, optionally moving the origin.

    Ground assets rest on z=0 with their origin at the geometry centre;
    characters keep their origin at the feet. Origin is not one global rule
    and a renderer that needs a character's feet at y=0 and a car's axle at
    mid-body must be told both.
    """
    merged = K.join(name, parts)
    if origin_at_feet:
        K.set_origin_to_geometry(merged)
    return merged


def grate(name: str, base, *, height: float = 0.10, slats: int = 7,
          bar: float = 0.035, along: str = "x", size=(1.0, 1.0)) -> list:
    """A slotted street grate / drain: base plate plus parallel bars.

    Bars rather than a texture, because a slotted grate is one of the few
    street props the camera gets close enough to that the slots have to be
    real geometry.
    """
    parts = [K.cube(f"{name}_base", size=(size[0], size[1], height * 0.4),
                    location=(0, 0, height * 0.2), material=base, bevel=0.01)]
    for i in range(slats):
        t = (i + 0.5) / slats - 0.5
        if along == "x":
            loc = (0.0, t * size[1] * 0.86, height * 0.62)
            dims = (size[0] * 0.92, bar, height * 0.7)
        else:
            loc = (t * size[0] * 0.86, 0.0, height * 0.62)
            dims = (bar, size[1] * 0.92, height * 0.7)
        parts.append(K.cube(f"{name}_bar{i}", size=dims, location=loc, material=base))
    return parts


def neon_tube(name: str, rgb, *, length: float, radius: float = 0.045,
              loc=(0.0, 0.0, 0.0), axis: str = "x") -> bpy.types.Object:
    """A glowing neon tube — a real emissive cylinder, not a painted decal."""
    mat = emissive(f"{name}_mat", rgb, strength=6.0)
    tube = K.cylinder(f"{name}", radius=radius, depth=length, vertices=12,
                      location=loc, material=mat, smooth=True)
    if axis == "x":
        tube.rotation_euler = (0.0, math.radians(90.0), 0.0)
    elif axis == "y":
        tube.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    K.transform_apply(tube)
    return tube