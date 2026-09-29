# Blender asset pipeline — NEON BAY

Everything in `assets/` is generated from the Python modules in this directory.
There are no imported models, no traced geometry, no downloaded textures, no
fonts and no logos: every triangle is computed from `vcw/core.py` at build time.

```
tools/blender/
├── build_assets.py        entry point -- writes assets/*.json + manifest.json
├── render_previews.py     renders assets/preview/*.png with Blender EEVEE
├── verify_assets.py       asserts every schema invariant; non-zero on failure
├── test_kernel.py         kernel self-tests (runs outside Blender)
├── check_window_recess.py proves building glass is set BACK, not stuck on
├── report_sizes.py        every asset's real dimensions + proportion audit
├── negative_control.py    proves the verifier actually rejects bad geometry
├── README.md              this file
└── vcw/
    ├── CONTRACT.md        authoring contract for adding a new category
    ├── core.py            geometry kernel (pure Python, no bpy)
    ├── export.py          asset -> Y-up JSON, with invariant enforcement
    └── builders/
        ├── buildings.py   14   Art Deco / pastel Miami
        ├── vehicles.py    14   road cars, city bus, aircraft, boats
        ├── pedestrians.py  4   suit, dress, overalls, streetwear (rigged)
        ├── props.py      14   street furniture
        ├── signs.py       8   neon signs
        ├── palms.py       3   tall, short, bushy
        ├── roads.py       9   tiling road / pavement / ground surfaces
        ├── misc.py        6   beach props + helicopter/helipad/graffiti
        ├── weapons.py     6   4 guns + bat + grenade
        ├── markers.py     2   objective flame, coin ring
        ├── pickups.py     8   vest, medkit, ammo, cash, O2, flare, rocket
        ├── animals.py     4   dog, cat, bird, fish
        ├── military.py    2   military hardware
        ├── watercraft.py  4   boat, yacht, ships
        ├── nature.py      2   grass patch, flower cluster
        ├── interior.py    2   interior fit-out
        └── terrain.py     1   mountain piece
```

## Requirements

Blender **4.5 LTS** at `/Applications/Blender.app/Contents/MacOS/Blender`.
Nothing else — the builders are pure Python and import only the standard library
and `vcw`.

## Regenerating everything

Run from the repository root (`/Users/sqs/code/vice-city-web`).

```bash
BL=/Applications/Blender.app/Contents/MacOS/Blender

# 1. Build every asset  ->  assets/*.json, assets/manifest.json
$BL --background --python tools/blender/build_assets.py

# 2. Render one preview per category  ->  assets/preview/*.png
$BL --background --python tools/blender/render_previews.py

# 3. Verify every invariant            ->  exits non-zero on any violation
$BL --background --python tools/blender/verify_assets.py
```

All three exit `0` on success. Each prints a per-asset table and a totals block.

### Checks that do not need Blender

```bash
cd tools/blender
python3 vcw/test_kernel.py         # geometry-kernel assertions
python3 check_window_recess.py     # building glass is recessed, not a decal
python3 report_sizes.py            # every asset's real size + proportion audit
python3 negative_control.py        # the verifier rejects deliberately broken input
```

`check_window_recess.py` exists because of a real bug this pipeline shipped
once: the glass was placed with `y_wall + sgn * GLASS_SET`, which for the street
facade (`sgn = -1`) puts the pane 15 mm **proud** of the wall. The recess
collapses into a flat sticker and it still passes every schema check, because
nothing in the schema knows what a recess is. It only showed up when someone
looked at the render. It is now a gate.

`report_sizes.py` prints every asset's bounds in human units and diffs the
31 that have a real-world reference size against it (31/31 currently pass).

### Current state

| | |
|---|---|
| Assets | **99** across 15 categories |
| Triangles | **133,151** (budget: 180,000) |
| Parts | 857 |
| Vertices | 395,098 |
| JSON on disk | ~21.6 MB |
| Previews | 12 PNGs, all non-blank (pixel std 0.059-0.198) |

Triangles by category:

| Category | Assets | Triangles | | Category | Assets | Triangles |
|---|---|---|---|---|---|---|
| `vehicle` | 14 | 42,598 | | `pickup` | 8 | 5,684 |
| `building` | 14 | 34,632 | | `sign` | 8 | 5,760 |
| `prop` | 14 | 11,350 | | `road` | 9 | 626 |
| `pedestrian` | 4 | 8,200 | | `animal` | 4 | 1,369 |
| `weapon` | 6 | 8,000 | | `beach` | 4 | 1,568 |
| `palm` | 3 | 7,046 | | `marker` | 2 | 1,728 |
| `misc` | 6 | 3,558 | | `nature` | 2 | 544 |
| | | | | `terrain` | 1 | 488 |

## Tests that need no Blender

`vcw/test_kernel.py` covers signed volume (outward winding), watertightness,
absolute positioning, flat-vs-smooth normal behaviour, and the Y-up export
matrix. `negative_control.py` deliberately inverts parts and confirms the
verifier rejects them — without it, "the verifier passes" could just mean "the
verifier does nothing".

## How the pipeline is structured

### Coordinate systems

Assets are **authored** in Blender-style Z-up (`+X` right, `−Y` forward,
`+Z` up, 1 unit = 1 m) because that is what Blender and the preview renderer
want. `export.py` converts to **Y-up** with `(x, z, −y)` — determinant `+1`, so
handedness and winding order survive untouched. So in the JSON:

- `y` is **height**
- `z` is **forward** (a vehicle's nose is at max `z`)
- every asset's origin sits where a game would place it (buildings and vehicles
  on the ground, pedestrians at the soles, weapons at the grip)

See `assets/SCHEMA.md` for the full contract.

### The flat-shaded representation

`flat: true` is the default aesthetic. It means **every triangle owns its three
vertices**, which is the only representation in which a shared vertex can carry
its own face's normal instead of an average. Primitives still share vertices
internally so shells stay watertight; `Mesh.flatten()` splits them at export
time, and the exporter asserts the result is bit-exact.

### How outwardness is proven

Signed volume via the divergence theorem, over **position-welded** closed
components:

```
vol = Σ dot(p0, cross(p1, p2)) / 6      > 0  ⟹  outward
```

Welding by position matters. An index-based manifold test calls every flat part
open — it stores three vertices per triangle — which silently skips the
outwardness check on the majority of the geometry. The welded test covers
2,796 of 2,827 components; the remaining 30 are genuinely open shells (a
one-sided decal, an awning underside) and are reported as such rather than
pretended to be verified.

This set has produced two real bugs that a render would not have caught: a
torus whose `axis="Y"` branch inverted the whole shell while still looking like
a torus, and a building ground-floor band sitting 5 mm below `z = 0`.

## Adding a new category

1. Read `vcw/CONTRACT.md`.
2. Add `vcw/builders/<name>.py` exposing `build_all() -> list[Asset]`.
3. Register the module in `build_assets.py` and `render_previews.py`.
4. Run the three commands above.

Check it in isolation first — this raises on any invariant violation:

```bash
cd tools/blender && python3 -c "
import sys; sys.path.insert(0,'.')
from vcw.builders import <name>
from vcw import export
for a in <name>.build_all():
    d = export.export_asset(a)     # raises on inverted faces, dup names, ...
    lo, hi = d['bounds']['min'], d['bounds']['max']
    print('%-24s %5d tris  %5.2f x %5.2f x %5.2f m'
          % (a.id, d['tri_count'], hi[0]-lo[0], hi[2]-lo[2], hi[1]-lo[1]))
"
```

Budget: ≤ 8,000 triangles per asset; the whole set must stay under 180,000.

### Backward compatibility with the game

Two things in `src/` constrain what you may change, and both fail **silently**
if you get them wrong:

* **Asset ids** are referenced by string literal in `src/const.rs` and
  `src/game.rs`. Renaming an asset makes the placement list silently drop it.
* **The 13 pedestrian part names** (`torso`, `head`, `hair`, `upper_arm_L` …
  `shoe_R`, defined in `src/const.rs`) drive the walk cycle.
  `push_player_part()` looks each one up **by name** and returns `usize::MAX`
  if it is missing. A pedestrian asset that drops or renames a rig part loses
  that limb at runtime with no error. Adding *extra* parts is safe -- they are
  simply never bound to a batch.

Also leave the rest of a part's dimensions alone where the game measures them:
`BUILDING_FOOTPRINT_GUARD`, `PED_SCREEN_PROBE_HEIGHT` and
`CHAR_VISIBLE_MAX_SCREEN_PCT` all reject or mis-cull geometry outside expected
ranges. `report_sizes.py` exists to catch size drift before it ships.

## Blender 4.5 API notes

- The EEVEE engine id is **`BLENDER_EEVEE_NEXT`**. `"BLENDER_EEVEE"` does not
  exist in 4.5 and raises on assignment.
- Principled BSDF inputs are `Base Color`, `Emission Color`,
  `Emission Strength`, `Roughness`, `Metallic`. (`Clearcoat` → `Coat Weight`,
  `Transmission` → `Transmission Weight`.)
- **`blender --background --python` exits 0 on an uncaught exception.** Both
  entry points therefore catch, print the traceback and call `sys.exit(1)`
  explicitly — otherwise "fails loudly on error" is not actually true.
- Preview cameras are framed from the asset's **bounding sphere**
  (`dist = radius / sin(half_fov) * 1.25`), never a hard-coded distance: a
  hard-coded distance silently puts the camera inside the model once the model
  grows. Each render is then checked for blankness by pixel standard deviation,
  and a camera aimed away from the subject produces std ≈ 0.003, which the
  renderer reports as `BLANK` and fails on.

## Content note

Every asset is an original procedural model. The neon signs use generic English
words (HOTEL, BAR, DINER, PIZZA, TROPIC, CLUB, ARCADE, MOTEL) rendered with a
hand-authored stroke alphabet — no real trademark, brand, typeface or game
asset appears anywhere in this pipeline.
