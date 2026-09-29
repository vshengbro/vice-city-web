# NEON BAY — asset JSON schema

Every file in `assets/` except `manifest.json` is one asset, serialised exactly
as described here. The renderer can rely on all of these invariants — they are
asserted at build time by `tools/blender/verify_assets.py`, which exits non-zero
if any of them is violated.

---

## 1. Units and coordinate system

| Property | Value |
|---|---|
| Unit | **1 unit = 1 metre** |
| Up axis | **+Y** |
| Forward axis | **+Z** |
| Right axis | **+X** |
| Handedness | Right-handed (cross(+X, +Y) = +Z) |
| Winding | **Counter-clockwise seen from outside** (the face's front side) |
| Surface normal convention | Points **away from the solid** (outward) |

### The authoring → export transform

Assets are authored in Blender-style **Z-up** space (+X right, −Y forward,
+Z up), then converted on export by the matrix

```
json = ( x,  z, -y )
```

Its determinant is **+1**, so it is a pure rotation: **handedness and winding
order are preserved exactly**. A face that is CCW from outside before the
transform is CCW from outside after it.

Consequences a renderer can rely on:

- **JSON `y` is height.** `bounds.min[1]` is how far the asset sits above the
  street.
- **JSON `z` is forward.** A vehicle's nose is at maximum `z`.
- **Authoring −Y (the front) becomes JSON +Z.**

### Origin placement

| Category | Origin |
|---|---|
| `building` | centre of the ground footprint, `y = 0` at street level |
| `vehicle` | centre of the ground footprint, `y = 0` at the ground |
| `pedestrian` | centre of the soles, `y = 0` under the feet |
| `prop`, `palm`, `sign`, `beach` | centre of the ground footprint, `y = 0` |
| `marker` | centre of the ground footprint, `y = 0` (planted in the world, like a prop) |
| `weapon` | centre of the grip, so a renderer can attach it to a hand |
| `pickup`, `misc` | the asset's own centre |
| `misc_helipad_marking` | a ground decal; its base sits at `y = 0` |

Every ground-resting asset has `bounds.min[1] >= -0.001` (they rest *on*
`y = 0` and never sink below it). `marker_coin_ring` is the one asset in this
set whose lowest vertex is *not* its own contact surface: the coins stand on a
painted ring plate, so the plate rests on `y = 0` and the coins ride on the
plate. Its origin is still the ring centre.

### Forward direction

Vehicles and pedestrians face **+Z** in JSON space. Both are authored facing
−Y, which the export matrix maps to +Z.

---

## 2. File format

```jsonc
{
  "id":         "car_sedan",        // unique asset id, matches the filename
  "category":   "vehicle",          // see the table in §5
  "y_up":       true,               // always true; asserts the up axis
  "bounds": {
    "min":      [-2.283, 0.0, -0.925581],
    "max":      [ 2.283, 1.45, 0.925581]
  },
  "part_count": 9,
  "tri_count":  4220,
  "parts":      [ /* see below */ ]
}
```

### Top-level fields

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Asset identifier. Matches the file's basename. |
| `category` | string | One of the values in §5. |
| `y_up` | bool | Always `true`. The renderer may assert on it. |
| `bounds.min` | `[x,y,z]` | Component-wise minimum over **all** vertices of **all** parts. |
| `bounds.max` | `[x,y,z]` | Component-wise maximum, same scope. |
| `part_count` | int | Number of entries in `parts`. |
| `tri_count` | int | Total triangles, equal to `Σ part.tri_count`. |
| `parts` | array | The submeshes. Never empty. |

`bounds` is recomputed from the exported vertex positions at build time and the
build fails if the declared and actual values disagree by more than 1e-5 m.

---

## 3. Part format

A **part** is the unit of material: one Blender-style material instance with its
own base colour, emission and roughness. Faces inside a part may still carry
different colours via `face_colors`.

```jsonc
{
  "name":          "body",
  "base_color":    [0.93, 0.45, 0.55],   // linear RGB, 0..1
  "emissive":      [0.0, 0.0, 0.0],      // linear RGB, 0..1; all-zero = not glowing
  "roughness":     0.45,                 // 0 = mirror, 1 = matte
  "metallic":      0.0,                  // 0 = dielectric, 1 = metal
  "positions":     [[x,y,z], ...],       // vertices, METRES, Y-up world space
  "normals":       [[x,y,z], ...],       // one per position, unit length
  "faces":         [[i0,i1,i2], ...],    // triangle indices into positions
  "face_colors":   [[r,g,b], ...],       // exactly one per face
  "flat":          true,                 // flat vs smooth shading
  "tri_count":     4220,
  "outward_check": "closed(2 shells)"    // diagnostic, see §4
}
```

| Field | Meaning |
|---|---|
| `name` | **Unique within the asset.** A renderer that builds one material per part name would silently collide on duplicates, so the build fails if a name repeats. |
| `base_color` | Linear-space albedo. Not sRGB — apply the usual linear→sRGB transfer at display time. |
| `emissive` | Linear RGB emission. `[0,0,0]` means the part does not glow. When non-zero, combine with `base_color` using `Emission Color` + `Emission Strength`; a strength of 1.0 is the intended look. |
| `positions` | Shared within the part. Metres. Y-up. |
| `normals` | One per position. For `flat: true` parts each triangle owns its three vertices, so this equals that triangle's geometric normal exactly. For `flat: false` parts it is the area-weighted average over the part's shared-vertex surface. |
| `faces` | Triangles, CCW seen from **outside**. Indices are always in `[0, len(positions))`. No face has a repeated index. |
| `face_colors` | Parallel to `faces` — `len(face_colors) == len(faces)` always. Use this for windows, tyres, lights, stripes and paint panels that share one material. |
| `flat` | `true` = flat-shaded low-poly hard edges (the default across this set). `false` = smooth-shaded, for car bodies, palm trunks and similar. |

### Shading

- **`flat: true` (the vast majority).** Every triangle has its own three
  vertices; `normals` is exactly the triangle's geometric normal. Render
  flat-shaded for the hard-edged low-poly look. Interpolating vertex normals
  across a flat part produces the same result.
- **`flat: false` (smooth parts).** Vertices are shared across neighbouring
  triangles and `normals` interpolates. On a crease vertex the averaged normal
  can oppose one of its own faces — this is normal for smooth geometry and is
  not an error.

---

## 4. What the build guarantees

`verify_assets.py` asserts all of the following for every asset:

1. `id` and `category` are non-empty; `y_up` is `true`; `min[i] <= max[i]`.
2. Part names are unique within the asset.
3. `len(normals) == len(positions)`; `len(face_colors) == len(faces)`.
4. Every face index is an `int` in `[0, len(positions))`.
5. No face has a repeated index, and none is degenerate (zero area).
6. For every face `(i0,i1,i2)`, the geometric normal
   `normalize(cross(p1-p0, p2-p0))` agrees **in direction** with the stored
   vertex normal: `dot > 0.99` for flat parts (exact equality, since flat parts
   own their vertices) and `dot > 0` for smooth parts. This is the CCW
   guarantee.
7. All stored normals are unit length.
8. **Outward facing.** For every position-welded closed component of every
   part, the signed volume `Σ dot(p0, cross(p1, p2)) / 6` is strictly
   **positive**. A negative value means the faces are wound inward. Components
   are welded by *position* before this test, so it covers flat parts too
   (which store three vertices per triangle and would defeat an index-based
   manifold test).
9. `bounds` matches the recomputed vertex extents within 1e-4 m.
10. Ground-resting categories have `bounds.min[1] >= -0.001`.
11. All colours are within `[0, 1]`.

`outward_check` records how each part was verified:

| Value | Meaning |
|---|---|
| `closed(N shells)` | N position-welded closed shells, all proven positive-volume. |
| `closed(N shells, M open)` | As above, plus M genuinely open sub-surfaces (e.g. a one-sided decal). |
| `open-shell` | No closed component; nothing to volume-check. |
| `declared-point` / `declared-dir` | An open surface whose outward direction the builder declared explicitly. |

---

## 4a. Tiling assets (`road`)

The nine `road_*` assets are **tiles**, meant to be repeated to pave the city.
Three rules make that work, and a renderer that wants to lay them out needs to
know all three:

1. **The pattern is centred on the origin**, and the asset's XZ extent is an
   exact whole number of repeat periods, so tiling on that period puts the
   partial pattern back together exactly. Measured values:

   | Tile | Extent | Period | Periods in extent |
   |---|---|---|---|
   | `road_lane_line` | 0.15 × 6.00 m | **3.00 m** (1.50 m dash + 1.50 m gap) | 2 |
   | `road_crosswalk` | 6.00 × 4.00 m | **1.00 m** (0.45 m bar + 0.55 m gap) | 6 across, 4 deep |
   | `road_lane_line_yellow` | 0.40 × 6.00 m | solid, none needed | — |
   | `road_asphalt`, `road_sidewalk` | 12 × 12 m | 12 m (6×6 / 6×6 cell grid) | 1 |
   | `road_grass`, `road_sand` | 8 × 8 m | 8 m | 1 |

   The lane line and the crosswalk both clip the bar/dash that would otherwise
   straddle the seam into two half-bars, which is what makes the seam invisible.
2. **No bevel or chamfer on a tile's outer boundary.** A chamfer on a tile edge
   breaks tiling — the strip simply does not meet its neighbour. All the road
   assets are therefore axis-aligned flat quads, and their surface variety
   comes entirely from `face_colors`, not from normal breaks.
3. **They are very thin (0.01–0.20 m).** They are painted onto the street, not
   built up from it. Lay them a few centimetres above the base road plane to
   avoid z-fighting.

Lane lines and the crosswalk carry **`emissive` exactly 0** — they are retroreflective
paint, not light sources, and the runtime's glow pass must not pick them up.

---

## 4b. How the runtime shades these assets

`src/render.rs` shades every face with, in linear space:

```text
base  = face_color * instance_tint
lit   = base * (ambient + light_color * max(dot(normal, light_dir), 0))
      + base * emissive * emissive_gain
color = tonemap(lit)                        # hue-preserving Reinhard
color = mix(color, sky_color, fog)          # smoothstep, ~90 m to ~460 m
color = linear_to_srgb(color)
```

Three consequences for anyone authoring or reviewing assets here:

* **No specular, no PBR, no texture lookup.** `roughness` and `metallic` are
  exported and validated but are never read. The only sources of surface
  variation are the surface **normal**, the **albedo**, and **emissive**.
* **`emissive` is what drives the night look.** The engine has a separate
  additive glow pass keyed on `emissive`, and `emissive_gain` scales from 0.18
  at noon to 1.85 at night. Give anything that should glow at night (signs,
  street lamps, traffic lenses, vehicle lamps, lit windows) a non-zero
  `emissive`; give it a matching solid `base_color` too, so it still reads when
  the gain is low.
* **Flat shading is the house style**, and at these triangle counts it is the
  right call. A chamfered edge (see `core.chamfer_box`) is often worth more than
  a smoother surface, because it gives the one directional light something to
  fall across.

---

## 5. Categories

| Category | Count | Contents |
|---|---|---|
| `animal` | 4 | dog, cat, bird, fish |
| `beach` | 4 | umbrella, chair, surfboard, boardwalk plank |
| `building` | 14 | Art Deco / pastel Miami towers and low-rises |
| `interior` | 2 | interior fit-out pieces |
| `marker` | 2 | objective flame cone, rotating coin ring |
| `military` | 2 | military hardware |
| `misc` | 6 | helicopter, helipad marking, dumpster pile, graffiti board, awning, street phone |
| `nature` | 2 | grass patch, flower cluster |
| `palm` | 3 | tall, short, bushy |
| `pedestrian` | 4 | suit, dress, overalls, streetwear — rigged as 13 named parts |
| `pickup` | 8 | armour vest, health pack, ammo box, cash stack, O₂ tank, flare pack, rocket round, grenade case |
| `prop` | 14 | street furniture (incl. construction barrier, parking meter) |
| `road` | 9 | tileable asphalt / sidewalk / lane line / crosswalk / grass / sand / manhole decal / drain grate |
| `sign` | 8 | neon signs (HOTEL, BAR, DINER, PIZZA, TROPIC, CLUB, ARCADE, MOTEL) |
| `terrain` | 1 | mountain terrain piece |
| `vehicle` | 14 | street vehicles (sedan, coupe, pickup, police, taxi, city bus) + boats, ships, aircraft |
| `weapon` | 6 | pistol, SMG, shotgun, rocket launcher, bat, grenade |

Totals: **99 assets, 133,067 triangles.**

A category is a label, not a behavioural contract: nothing in the runtime
branches on it. The categories that *do* carry a hard obligation are
`building`, `prop`, `palm`, `vehicle` and `sign`, which the verifier requires to
rest on `y = 0` (see §4, check 10).

---

## 6. `manifest.json`

```jsonc
{
  "version": 1,
  "generator": "tools/blender/build_assets.py",
  "up_axis": "Y",
  "unit": "meter",
  "asset_count": 99,
  "tri_count": 133067,
  "categories": ["animal", "beach", "building", "interior", "marker",
                 "military", "misc", "nature", "palm", "pedestrian",
                 "pickup", "prop", "road", "sign", "terrain", "vehicle",
                 "weapon"],
  "assets": [
    {
      "id": "car_sedan",
      "category": "vehicle",
      "file": "car_sedan.json",     // relative to the assets/ directory
      "tri_count": 4220,
      "part_count": 9,
      "bounds": { "min": [...], "max": [...] }
    }
    // ... one entry per asset
  ]
}
```

`file` is relative to the directory containing the manifest. Load the manifest
first, then the individual files it names.

---

## 7. Minimal renderer skeleton

```python
import json

manifest = json.load(open("assets/manifest.json"))
for entry in manifest["assets"]:
    asset = json.load(open("assets/" + entry["file"]))

    for part in asset["parts"]:
        # positions/normals/faces are already in Y-up metres.
        # Blend:  color = face_color;  light += emissive * emissive_strength
        for (i0, i1, i2), face_color in zip(part["faces"], part["face_colors"]):
            tri = [part["positions"][i0], part["positions"][i1],
                   part["positions"][i2]]
            # Cross products here use the right-hand rule, so the triangles are
            # already CCW from outside and the normal is outward.
            n = normalize(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])))
            draw_triangle(tri, face_color, n, part["flat"])
```

No axis conversion is needed: the JSON is already Y-up, which is what a
typical WebGL/WebGPU renderer wants.

---

## 8. Regenerating

See `tools/blender/README.md`. In short:

```bash
/Applications/Blender.app/Contents/MacOS/Blender --background \
    --python tools/blender/build_assets.py      # write assets/*.json
/Applications/Blender.app/Contents/MacOS/Blender --background \
    --python tools/blender/render_previews.py   # write assets/preview/*.png
/Applications/Blender.app/Contents/MacOS/Blender --background \
    --python tools/blender/verify_assets.py     # assert every invariant
```

All geometry is 100 % procedurally generated from `tools/blender/vcw/`; no
external models, textures, fonts or logos are imported or traced at any point.
