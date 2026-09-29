# Builder authoring contract (NEON BAY asset pipeline)

Read this before writing a builder. It is the contract every category already
satisfies — follow it and your file will be picked up without touching anything
else.

## 0. The four rules the ENGINE imposes on you

These are not style. Breaking one is a SILENT failure in `src/` — the asset
still loads, the build still exits 0, and the thing simply is not in the game.

1. **Never rename or remove an asset `id`.** `src/game.rs` looks every
   placement up by literal id string. A missing id is pushed onto a `failed`
   vector and logged; the prop is then skipped with `let ... else { continue }`.
   Nothing surfaces to the player. `ped_suit` is special-cased: if it vanishes,
   `spawn_player_traffic_pickups` never runs and **the player is invisible**.
2. **Never rename a `ped_suit` part name.** `PLAYER_PARTS` in `src/game.rs`
   matches all 13 by exact string to drive the walk cycle, and
   `joint_pivot` derives the rotation axis from each part's own local bbox. A
   name that no longer matches yields `usize::MAX` and that limb silently
   disappears. The 13 are:
   `torso, head, hair, upper_arm_L, lower_arm_L, upper_arm_R, lower_arm_R,
   upper_leg_L, lower_leg_L, upper_leg_R, lower_leg_R, shoe_L, shoe_R`
   Extra parts on a pedestrian are harmless (only `ped_suit` is split by name),
   so **add detail inside those 13 parts**, or as new clearly-named extras.
3. **Every asset in `GROUND_CATEGORIES` must rest exactly on z = 0** — verify
   checks `bounds.min[1] >= -0.001`. Buildings, props, palms, vehicles, signs.
4. **`BUILDING_FOOTPRINT_GUARD = 11.0` (src/game.rs).** A building whose
   footprint half-extent exceeds 11 m is **silently culled from the city
   layout** and never placed. Max legal half-extent is 11.0 m; stay under 10.5.

## 1. What the runtime actually shades with

`src/render.rs`, both the WebGL fragment shader and the CPU `shade_face`:

```text
base   = albedo * tint                     // per-vertex colour × per-instance tint
lit    = base * (ambient + light_color * max(dot(normal, light_dir), 0))
       + base * emissive * emissive_gain
color  = tonemap(lit)                      // hue-preserving Reinhard, then sRGB
color  = mix(color, sky_color, fog)        // smoothstep over 90-460 m
```

**There is no specular term, no texture, no PBR, and no reflection.** So the
only things that can make two adjacent polygons look different are:

* **a different normal** — this is why `chamfer_box` exists. A large flat wall
  renders as one dead value. A 3 cm chamfer puts a strip at an intermediate
  normal on all four sides of every arris and is what makes an untextured box
  read as a solid. It costs 28 triangles instead of 12 and is the single
  cheapest quality win in this pipeline.
* **a different albedo** — `face_colors` is free. Zone your materials.
* **emissive** — cheap, and the whole night look depends on it. Anything that
  glows gets BOTH a solid `base_color` and a non-zero `emissive`, so it still
  reads when the runtime scales the glow down.

`roughness` and `metallic` are exported and validated but **never read by the
runtime**. Set them for documentation; do not spend triangles chasing them.

Consequence: "make it look less like a white model" means *add normal breaks
and colour zones*, not add triangles uniformly.

## 2. Triangle budget — think in INSTANTIATED triangles

The per-asset cap of 8,000 is the easy constraint. The binding one is how many
times the engine instances each asset (counts below are derived from the
placement loops in `src/game.rs`, `CITY_HALF = 150`, 4 street lines):

| prefix | placements | share of all instantiated tris |
|---|---|---|
| `bldg_` | 135 (many culled) | ~46% |
| `palm_` | 106 | ~30% |
| `prop_streetlight` | 72 | ~6.5% |
| `ped_` | 56 | ~6% |
| `prop_trafficlight` | 64 | ~4.7% |
| `sign_` | 36 | ~3.6% |
| `car_` / `truck_` | traffic fleet | <1% |
| weapons, pickups, markers, misc, beach | ~1-9 | <1% |

**Buildings and palms are ~76% of the world's triangles.** So:

* Detail belongs where instantiation count is LOW and the camera is CLOSE —
  vehicles, `ped_suit`, weapons, pickups, markers, signs, street furniture.
* Buildings and palms get **chamfers and colour zones**, which are nearly free,
  and must NOT get a big raw triangle increase. Their count is multiplied by
  100+.
* Spend the "free" budget on assets the engine instantiates only a handful of
  times.

## 3. Shape of a builder module

File: `tools/blender/vcw/builders/<category>.py`
Exports exactly one function: `build_all() -> list[core.Asset]`

```python
from .. import core as C

def build_all():
    return [_sedan(), _coupe(), ...]
```

A builder MUST NOT import `bpy`, `json`, or `os`. It only builds geometry and
returns `Asset` objects. The exporter, the manifest and the renders are handled
by `build_assets.py`.

## Coordinate systems

**Authoring (Blender-style, Z-up):**
- `+X` right, `-Y` forward, `+Z` up, `1 unit = 1 metre`.
- Every asset is centred on `X = 0`, resting on `Z = 0`.
- The origin sits where a game would want to place the thing:
  buildings/vehicles/props → centre of the ground footprint;
  pedestrians → centre of the soles.

**Export (Y-up):** the exporter applies `(x, z, -y)` (det +1, handedness
preserved). So authoring `-Y` becomes JSON `+Z`. Vehicles and pedestrians face
authoring `-Y`.

## `core` primitives you should use

Vectors: `add sub mul dot cross normalize length lerp shade mix_color`
Shapes: `box chamfer_box cylinder cone sphere torus tube loft section_rings
        rounded_rect circle ribbon`
Parts: `Part Asset mirror_x rotate_z translate_part recolor_faces
       recolor_faces_where`
Checks: `signed_volume is_closed has_smooth_geometry`

Signatures worth knowing:

```python
C.box(m, size, center=(0,0,0), color=(r,g,b), colors={'+x':..,'-z':..,..})
C.cylinder(m, radius, height, seg=12, center=(0,0,0), color=col,
           radius_top=None, caps=True, smooth=False, axis='Z')
C.cone(m, radius, height, seg=10, center=(0,0,0), color=col,
       caps=True, radius_top=0.0)            # radius_top>0 => truncated
C.sphere(m, radius, seg_u=10, seg_v=6, center=(0,0,0), color=col,
         squash=1.0, smooth=False)
C.torus(m, R, r, seg_u=12, seg_v=6, center=(0,0,0), color=col, axis='Z')
C.tube(m, path, radius, seg=6, color=col, caps=True, smooth=False)
C.loft(m, rings, color, cap_start=True, cap_end=True, smooth=False)
C.chamfer_box(m, size, center=(0,0,0), color=col, bevel=0.03, n_corner=1,
              colors={'+x':..,'-z':..,..})   # 28 tris, edges catch the light
C.rounded_rect(hx, hy, r, n=4)   # CCW 2D profile in XY
C.ribbon(m, pts, width, up=(0,0,1), color=col)
```

Every one of these appends triangles to `m` and returns `m`, so they chain.

### Two rules that are easy to get wrong

1. **`smooth=` must match the Part's `flat` flag.** A `Part` carries one `flat`
   boolean. Build smooth geometry only into a `Part(..., flat=False)`. The
   exporter rejects a flat part containing smooth geometry.
2. **Never put an open surface in a Part without telling it what "outward" is.**
   The exporter verifies every face points out of the asset, but for an open
   surface there is no interior to measure. Pass it explicitly:
   ```python
   p = C.Part("sign_face", flat=True, outward=("dir", (0, -1, 0)))   # faces -Y
   p = C.Part("lamp_shade", flat=True, outward=("point", (0, 0, 1.0)))  # around a point
   ```
   Leave `outward=None` for closed shells (boxes, cylinders, lofts) — the
   exporter uses signed volume then, which is exact.

## Parts vs colours

A `Part` is the unit of material. `base_color` is the part's material colour;
`face_colors[i]` is the colour of face `i` and may differ (windows, tyres,
lights). `emissive` is a part-level glow colour — all zeros means no glow.

Prefer splitting into a few semantically meaningful parts (`body`, `glass`,
`tyres`, `lights`) over one part with many colours. Each part becomes a
submesh with its own material at render time.

## Budget

- Total across ALL assets must stay under ~150,000 triangles.
- Hard budget per asset: **8,000 triangles**. Aim for 300–3,500.
- Prefer one `Part` holding many triangles over many single-triangle parts.
  A building has 132 parts and that is already more than it needs — reuse a
  single part and switch `face_colors` when faces differ only in colour.
- Remember section 2: the budget that matters is
  `tri_count x placements`. Adding 500 triangles to a palm costs 53,000 in the
  world; adding 500 to a pistol costs 500.

## Verify before you claim it works

Fastest loop, no Blender needed (the kernel is pure stdlib):

```bash
cd tools/blender
python3 test_kernel.py          # kernel unit tests + exports EVERY asset
```

That test already runs the full export contract over every builder, so a green
run means every asset is geometrically valid. To inspect your own category:

```bash
cd tools/blender
python3 -c "
import sys; sys.path.insert(0,'.')
from vcw.builders import <module>
from vcw import export
tot=0
for a in <module>.build_all():
    d = export.export_asset(a)   # raises on any invariant violation
    lo,hi = d['bounds']['min'], d['bounds']['max']
    tot += d['tri_count']
    assert d['tri_count'] <= 8000, (a.id, d['tri_count'])
    print('%-24s %5d tris  %5.2f x %5.2f x %5.2f m' % (a.id, d['tri_count'], hi[0]-lo[0], hi[2]-lo[2], hi[1]-lo[1]))
print('TOTAL', tot)
"
```

That script raises on: inverted faces, non-exact flat normals, degenerate
triangles, `face_colors`/`faces` length mismatch, and bounds that disagree with
the actual vertices. If it prints without raising, your builder is correct.

Style: no licence headers, no asset-store references. Original geometry only.
