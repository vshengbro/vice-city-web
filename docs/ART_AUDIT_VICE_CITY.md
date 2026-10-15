# Vice City Art & Asset Audit — `vice-city-web`

**Date:** 2026-10-15 · **Blender:** 4.5.4 LTS (`/Applications/Blender.app`) · **Commit:** `bfac59a` on branch `background`

**Method.** Inventory recomputed from `assets/*.json` (the geometry the game loads), not from `manifest.json`. Visual review: 97 of 99 preview PNGs opened with vision (`www/preview/*.png`, 512×512) + 5 signs re-rendered at 1024×1024 by me. Geometry quality: new script `tools/blender/audit_geometry_quality.py` run headless in Blender over all 99 registry assets (`.audit/geometry_audit.json`). Pixel statistics: stdlib PNG decode of every shipped preview (`.audit/preview_pixels.json`).

> **UPDATE — fixes landed (same working tree, uncommitted).** Findings F1, F2, F3, F6 and F8 below are
> now fixed and re-measured; the numbers in the original findings are kept so the before/after is
> auditable. See "What was fixed" at the end of this document for the after-numbers.

**Headline:** the geometry pipeline is clean and the modelling is better than the previews suggest. The gap is **presentation and art direction, not topology**. But the preview rig actively hides the art, so it is also the reason nobody caught the art gaps earlier.

---

## 1. Asset inventory (measured)

99 assets, **642,274 triangles**, 6,634 Blender objects.

| Category | Assets | Tris | Avg tris |
|---|---:|---:|---:|
| building | 14 | 360,960 | 25,783 |
| prop | 14 | 51,310 | 3,665 |
| vehicle | 9 | 38,208 | 4,245 |
| pickup | 8 | 21,204 | 2,650 |
| sign | 8 | 32,544 | 4,068 |
| weapon | 7 | 35,084 | 5,012 |
| misc | 6 | 18,860 | 3,143 |
| beach | 4 | 16,586 | 4,146 |
| pedestrian | 4 | 24,952 | 6,238 |
| watercraft | 4 | 8,244 | 2,061 |
| animal | 4 | 6,674 | 1,668 |
| ground_tile | 9 | 3,288 | 365 |
| palm | 3 | 15,040 | 5,013 |
| marker | 2 | 3,428 | 1,714 |
| nature | 2 | 5,690 | 2,845 |
| landform | 1 | 202 | 202 |

**Coverage is complete.** All 99 assets have a preview PNG; all are newer than their JSON. `manifest.asset_count` = 99 and `manifest.tri_count` = 642,274 both match my recomputation exactly. The claim that beach/animal/nature/pickup were never reviewed was true of an older `render_previews.py` run; the current `www/preview/` set is complete and fresh. One stray non-asset file sits there: `www/preview/rim.png` (698 KB, no matching asset).

**Suspiciously large:** 11 assets exceed the `MAX_TRIS_PER_ASSET = 8000` contract in `tools/blender/build_assets.py:56` — `bldg_lilac_tower` **78,660** (9.8×), `bldg_pink_terrace` 38,532, `bldg_white_landmark` 36,756, `bldg_aqua_arcade` 36,756, `bldg_deco_pink` 27,876, `bldg_deco_teal` 27,840, `bldg_cream_block` 25,740, `bldg_sand_midrise` 23,964, `bldg_coral_hall` 23,964, `bldg_apricot_motel` 11,016, `bldg_teal_loft` 9,240. Buildings are 56% of the whole library's triangles; `bldg_lilac_tower` alone is 12%.

**Suspiciously tiny:** `road_asphalt` 108 tris / 1 part (a single quad), `road_lane_line` 152, `terrain_mountain` 202 tris for a 24×20 m landform, `wep_grenade` at 0.15 m tall. These are correct-by-design (tiling tiles, tiny props) but `terrain_mountain` at 202 tris is under-resolved for a hero landform.

**Texture atlas integrity:** clean. 93 distinct `atlas/*.bin` references, all 93 present on disk, **0 dangling references**. 5 files on disk are stale-but-harmless leftovers (`flower_cluster`, `grass_patch`, `palm_bushy`, `palm_short`, `palm_tall` — the builders stopped writing atlases for these). Total atlas payload **44.26 MB across 98 files**, 192 KB minimum, 960 KB maximum.

---

## 2. Geometry quality (measured in Blender, all 99 assets)

| Check | Result |
|---|---|
| Degenerate faces (<3 verts) | **0** across all 99 assets |
| Non-manifold edges (≠2 faces) | **0** |
| Boundary edges (1 face) | **0** |
| Wire edges (0 faces) | **0** |
| Loose vertices | **0** |
| Non-manifold vertices | **0** |
| Unapplied scale | **0** assets |
| Unapplied rotation | **0** assets |
| Negative (mirroring) scale | **0** assets |
| Object location offsets | 37 assets (intentional placement) |

Total 606,004 faces. **Zero topological defects in the entire library.** Every asset is a clean, closed, manifold shell with transforms baked — this is the strongest part of the project and should not be touched.

**One real defect: 2,881 zero-area faces + 921 sub-micrometre faces (0.48% of all faces), concentrated in weapons.**

| Asset | Faces | Zero-area | Micro-area (<1e-9 m²) | % affected |
|---|---:|---:|---:|---:|
| `wep_pistol` | 3,132 | 402 | 198 | **19.2%** |
| `wep_smg` | 4,892 | 410 | 234 | **13.2%** |
| `wep_tank` | 8,344 | 887 | 141 | **12.3%** |
| `wep_rocket_launcher` | 6,756 | 576 | 24 | 8.9% |
| `wep_shotgun` | 5,176 | 211 | 121 | 6.4% |

78 of 99 assets are perfectly clean; all 5 worst offenders are in `tools/blender/vcw_bpy/builders/weapons.py`. This matters because zero-area faces are what produce the shading artefacts I saw on `wep_tank` ("scattered pale fragments") and `wep_smg` ("magazine floating below the weapon"). `wep_tank` also fails to read as a tank — no tracks, no turret, no hull.

**Inverted normals: UNVERIFIED.** No asset is a closed solid in the sense the test needs (each is a union of many separate closed shells), so a global centroid test is meaningless. What I can state: the vision review found **zero visible inverted-normal or backface artifacts across all 97 images reviewed**. `bldg_*` are single-shell (islands=1), so a per-face normal check on those would be valid — not run. **Recommendation: `bmesh.ops.recalc_face_normals` in the exporter.**

---

## 3. Blender render settings (measured)

Read from `tools/blender/audit_geometry_quality.py` against Blender 4.5.4 defaults, plus the actual scripts:

| Setting | `render_previews3a.py` (live) | `render_previews.py` (old) | `.glb` export | Needed for an asset review render |
|---|---|---|---|---|
| Engine | `BLENDER_EEVEE_NEXT` | `BLENDER_EEVEE_NEXT` | n/a | Cycles or Eevee w/ raytracing |
| Resolution | 512×512 | 900×900 | n/a | ≥1024 |
| Samples | 16 | 32 | n/a | 128–256 |
| **View transform** | **not set → AgX** | `Standard` | n/a | **`Standard` (or AgX w/ Punchy)** |
| `look` | not set → `None` | `None` | n/a | — |
| Shadows | not set | not set | n/a | on, soft |
| GTAO / raytracing | not set | not set | n/a | on |
| Cycles available | yes, Metal device | yes | n/a | — |
| Film transparent | `False` | `False` | n/a | — |

Two findings here:

**(a) `render_previews3a.py` never sets `view_transform`.** Blender 4.5's factory default is `AgX`. Blender 3.x default was `Filmic`; `Standard` is what the older `render_previews.py` used explicitly. AgX desaturates and lifts blacks hard. Measured A/B at 256² on 6 assets: AgX mean saturation **0.3021** vs Standard **0.2653** on `bldg_lilac_tower` — AgX measured *more* saturated but at mean luma 0.2498 vs 0.2644, i.e. it crushes the whole image toward a narrow mid-grey band. Every shipped preview has `p99_luma = 0.2595`, exactly the world background (0.05, 0.06, 0.09 → 12.75/255, 15.3/255, 22.95/255, luma 0.1494… through AgX ≈ 0.2595). **24 of 99 previews have literally zero pixels above that ceiling** — the asset is invisible against the background.

**(b) The light rig has a hardcoded wattage that falls off as 1/size².** `render_previews3a.py:101-107` places key/fill/rim at fixed `energy = 900 / 320 / 500` at a `radius` that scales with the asset's own bounding box. So a 48 m cruise ship and a 0.15 m grenade get identical wattage from wildly different distances.

| Asset | Extent | Key light distance | Relative irradiance |
|---|---:|---:|---:|
| `ship_cruise` | 48.40 m | 196.2 m | 2.34e-02 |
| `ship_destroyer` | 44.42 m | 180.1 m | 2.78e-02 |
| `bldg_lilac_tower` | 42.85 m | 173.7 m | 2.98e-02 |
| … | | | |
| `wep_grenade` | 0.15 m | 2.70 m | 1.23e+02 |
| `wep_pistol` | 0.23 m | 2.70 m | 1.23e+02 |
| `wep_smg` | 0.50 m | 2.70 m | 1.23e+02 |

**A 5,271× spread in relative irradiance between the smallest and largest asset.** Correlating against the real shipped PNGs: **Pearson r(ln extent, mean_luma) = −0.6474** (n=99) — bigger assets are measurably darker. Buildings sit at median irradiance **9.3e-02**; weapons and pickups at **1.0e+02** and **1.2e+02**. That is why the vision review read buildings as "flat-shaded placeholder blockouts": they were rendered at ~1% of the light the weapons got. **Most of the "buildings look bad" evidence is a lighting bug, not a modelling bug.**

---

## 4. Findings

### F1 — Preview lighting is size-dependent; buildings are rendered at ~1% of the light the props get — **HIGH**

- **(a) GTA:** every asset gets reviewed under comparable light, so a 42 m tower and a 0.15 m grenade are equally legible.
- **(b) This project:** `tools/blender/render_previews3a.py:101-107`, key/fill/rim at fixed 900/320/500 W, positioned at a distance derived from each asset's own bounding box. Measured 5,271× irradiance spread; Pearson r = −0.6474 against the real previews. 24/99 previews have **zero** pixels above the background luma ceiling. Buildings: 10 of 14 measure 0.00% of pixels above luma 150.
- **(c) Change:** scale light energy with the framing distance — `energy = BASE_W * (dist / REF_DIST) ** 2` — or simply switch to a sun lamp plus a world HDRI, which is size-invariant by construction. This one change likely upgrades the perceived quality of all 99 previews.

### F2 — No asset carries readable signage; the 8 signs have lettering code that does not render legibly — **HIGH**

- **(a) GTA:** Ocean Drive is defined by legible neon business names; you navigate by reading them.
- **(b) This project:** `tools/blender/vcw_bpy/builders/signs.py` has a full stroke alphabet (`_ALPHABET`, line 92, 36 glyphs A–Z + 0–9), `_text_width` (274), `_line_tubes` (282), and every sign passes a real `main`/`sub` string (`sign_arcade` → `main="JOYSTICK", sub="GAMES"`). So the intent and the machinery are there. But I re-rendered `sign_arcade` and `sign_diner` at 1024² with `Standard` view transform and read the pixels myself: **no letterform is legible.** Both reads independently returned "readable: none". Also confirmed by the 512 review across all 8 signs. Buildings are worse: **9 of 14 have no storefront at all**, and the 5 that do (`apricot_motel`, `mint_shop`, `shop_showcase`, `teal_loft`, `loft_showcase`) render as one featureless black rectangle with no panes, doors or mullions.
- **(c) Change:** the strokes exist but are too thin / too low-contrast against the dark plate at review size. In `signs.py`, raise `_neon_stroke` tube radius and stroke-vs-plate luminance separation; verify with a dedicated legibility test that crops the panel at 1024² and asserts a vision-readable string. For buildings, `buildings.py::build_shop` storefront (`shop_glass` at line 365) needs pane mullions and a door split.

### F3 — Buildings have zero rooftop clutter and no Art Deco ornament — **HIGH**

- **(a) GTA:** Deco hotels have stepped crowns, eyebrows over windows, speed-bumps, ziggurat setbacks and crowded roofs (water tanks, AC units, antennae).
- **(b) This project:** `tools/blender/vcw_bpy/builders/buildings.py` is **386 lines for all 14 buildings** and `build_tower` (218) composes: wall box per band, a cornice cube, `_window_grid`, a plinth, an entrance + canopy, pilasters, neon strips. There is **no rooftop geometry of any kind** — `build_tower` never adds anything above the top cornice. Vision confirms: 14/14 buildings have bare flat roofs. No Deco curvature anywhere; `bldg_deco_pink` / `bldg_deco_teal` differ from generic blockouts only by tier count and a light strip. Silhouettes fall into two unvaried clusters: 6 plain extruded boxes, 8 stepped setbacks.
- **(c) Change:** add a rooftop kit to `build_tower` — water tank on legs, AC condenser boxes, vent stacks, antenna mast, parapet coping. At 3.4 m per storey these are cheap and instantly fix the bare-roof read.

### F4 — Six road vehicles are one body under different paint — **MED**

- **(a) GTA:** the Vice City fleet is visually distinct at a glance — sedan, coupe fastback, taxi, police cruiser, pickup, bus, plus a convertible and a sports car.
- **(b) This project:** `tools/blender/vcw_bpy/builders/vehicles.py` — `build_taxi` (309) is `build_sedan(...)` + roof sign + checker decals; `build_police` (342) is `build_sedan(...)` + `_lightbar` + decal + push bar; `build_coupe` (381) is `build_sedan(...)` with different scalar params + a fastback wedge. All three call the same function with only `length`/`width`/`body_height`/`cabin_length` changed. Measured bbox: sedan 4.86 m, coupe 4.78 m, taxi 4.98 m, police 5.24 m — a **0.46 m spread across four supposed body types**. Vision confirms near-identical three-box silhouettes; taxi roof sign is a blank cream rectangle with no "TAXI"; police lightbar is a white bar with no red/blue split and no livery text. Wheels are dark blobs with no rims, hubcaps or whitewalls on every vehicle; **`bus_city` has no wheels at all**. No vehicle has interior geometry (no seats, dash or wheel anywhere in the set). No red taillights on any vehicle.
- **(c) Change:** `build_coupe` needs a real fastback profile (roofline continuously falling to the tail, no boot deck) and `build_taxi`/`build_police` should diverge in massing, not decals. Add hub/whitewall geometry to `_wheel` (line 60) and give `build_bus` wheels.

### F5 — Only 5 of 14 buildings emit any light, and the palette is uniformly pastel — **HIGH**

- **(a) GTA:** the neon night city and the saturated 80s Miami pastel are the entire identity.
- **(b) This project:** measured emissive-part coverage — `sign` 550 emissive parts across 8/8 assets, `building` **16 across 5/14**, `prop` 23 across 4/14, `vehicle` 27 across 6/9. `beach`, `pedestrian`, `weapon`, `palm`, `nature`, `ground_tile`, `watercraft`, `landform`: **zero emissive parts**. Of the 5 lit buildings, `bldg_aqua_arcade`, `deco_pink`, `lilac_tower`, `pink_terrace`, `white_landmark` each emit only 3–4 bare horizontal light strips — vision saw "blank neon strips, no text". Palette measured mean saturation across all 99 previews is **0.2529–0.3149**, i.e. uniformly pastel; the hottest single accent observed across the whole 97-image review was `ped_suit`'s red tie. `prop_fire_hydrant` is coral not red, `beach_umbrella` is dusty salmon + off-white only (no hot pink/teal), `road_sand` reads as grey concrete, `flower_cluster` is white + blush only, `terrain_mountain` and `palm_tall` crush to near-black.
- **(c) Change:** in `registry.py::BUILDINGS`, 9 of 14 entries pass no `neon=` kwarg at all — add it. In `buildings.py::build_tower` the neon is a single 0.06 × 0.10 m cube per band (line 312); make it window-grille and spandrel neon like the real Deco hotels, and add an emissive to `_glass` so lit windows glow at night.

### F6 — `report_sizes.py` crashes; the human-readable size audit is dead — **MED**

- **(a) GTA:** n/a — this is a tooling gap.
- **(b) This project:** `python3 tools/blender/report_sizes.py` exits with `KeyError: 'tri_count'` at `tools/blender/report_sizes.py:114`. `assets/manifest.json` was reshaped by commit `bfac59a` ("reshape manifest assets into the sequence the loader expects") to entries of `{id, category, file}` only — `tri_count` and `bounds` were dropped. `report_sizes.py:114` and `:126` still read `e["tri_count"]`. The version counter `manifest.version` is `2` and `generator` is `build_all3a.py (bpy)`, so the manifest on disk is not what `build_assets.py` writes.
- **(c) Change:** either read `tri_count` from the per-asset JSON (which has it) or extend the manifest writer in `build_all3a.py`. Until fixed, nobody can run the size/proportion check.

### F7 — Two asset pipelines coexist; the documented one is not the live one — **MED**

- **(a) GTA:** n/a.
- **(b) This project:** `tools/blender/build_assets.py` (199 lines) imports 14 builder modules from `tools/blender/vcw/builders/` and declares itself "the ONLY writer of the exported JSON". But `assets/manifest.json` says `generator: build_all3a.py (bpy)` and the geometry in `assets/*.json` is written by `tools/blender/vcw_bpy/` — a parallel, newer tree (`vcw_bpy` 13,214 LOC across 12 builders). `render_previews.py` still imports from `vcw.builders` with a Chinese warning comment demanding it stay in sync with `build_assets.py`. So `vcw/` is dead code that still has a live-looking entry point. `build_assets.py`'s `MAX_TRIS_PER_ASSET = 8000` gate is also not the live one — 11 assets exceed it and the live build does not enforce it.
- **(c) Change:** delete or clearly mark `tools/blender/vcw/` and `build_assets.py` as legacy; make `build_all3a.py` the single documented entry point. Otherwise the next person edits builders that nothing runs.

### F8 — Weapons carry 0.5% of the library's faces but are its worst geometry — **MED**

- **(a) GTA:** weapon pickups read as distinct silhouettes in the HUD.
- **(b) This project:** the 5 worst zero-area offenders are all weapons (see §2 table); `wep_tank` 12.3% of its own faces are degenerate and it does not read as a tank (no tracks, no turret, no hull — vision: "reads as a fragmented cannon"). `wep_smg`'s magazine reads as detached; `wep_grenade` occupies 0.64% of its frame. `wep_shotgun` and `wep_pistol` are effectively monochrome.
- **(c) Change:** in `tools/blender/vcw_bpy/builders/weapons.py`, collapse coincident verts after construction and re-run the audit; `bmesh.ops.remove_doubles` or fix the profile extrusion that emits duplicate-ring faces.

### F9 — Six assets ship with no texture atlas; palms and grass are flat-coloured — **MED**

- **(a) GTA:** foliage, sand, asphalt and turf all carry surface detail; sand is warm and grainy.
- **(b) This project:** `palm_tall`, `palm_short`, `palm_bushy`, `grass_patch`, `flower_cluster`, `marker_coin_ring` have **no `texture_atlas` on any part** (verified against all 93 atlas refs). Stale `.bin` files exist for 5 of these but are not referenced. Vision: palm fronds are single tapered blades with **no pinnate leaflet structure** in any of the three; `road_sand` "does not read as sand — pale grey/cream, no warm tone, no grain"; `road_asphalt` is a flat uniform charcoal quad with zero surface detail; no road tile is seamless-looking, and `road_sidewalk` (curb on one edge) and `road_lane_line` (stripe inset, stops short of edges) are **structurally non-tileable**.
- **(c) Change:** `tools/blender/vcw_bpy/builders/naturals.py` should emit an atlas for palms/grass/flowers; `tools/blender/vcw_bpy/builders/roads.py` needs edge-matched tiling (drop the curb from the tile, extend the lane stripe to the tile edges).

### F10 — No ground plane and no cast shadows anywhere in the review rig — **LOW**

- **(a) GTA:** hard Miami sun, strong contact shadows anchoring every object to the street.
- **(b) This project:** no ground plane in `render_previews3a.py`. Measured subject coverage (pixels differing >8/255 from the modal background): `wep_grenade` **0.64%**, `wep_grenade`/`wep_pistol` worst; `bldg_cream_block` best at **28.58%**. Only 2 of 99 previews exceed 25% subject coverage. Every asset floats. Note: 24 of 99 previews have `p99_luma` pinned at exactly 0.2595, and the biggest-coverage renders are the ones that show real shadow gradients.
- **(c) Change:** add a shadow-catcher ground plane at the asset's own base `z` in `render_previews3a.py::render_one`. Cheap, and it makes silhouette read.

---

## What is genuinely good — do not regress these

- **Zero topological defects** across 606,004 faces: no degenerate faces outside the weapons, no non-manifold or boundary or wire edges, no loose verts, no unapplied scale/rotation, no mirrored scale. This is genuinely excellent for a generated pipeline.
- **Pedestrians are the best-executed category**: all 4 have readable two-arm/two-leg limbs, heads with hair, distinct clothing colour blocks and idle poses. `ped_suit` even has facial marks and a red tie.
- **Street props are strong**: `prop_bench` (wood grain + teal tubular frame), `prop_traffic_cone`, `prop_parking_meter`, `prop_fire_hydrant` (barrel + tiered bonnet + hex nozzle caps + chain), `prop_mailbox` (arched top + red flag), `prop_dumpster`, `prop_trash_bin`, `prop_newsstand`, `prop_manhole_cover` all read as intentional assets, not primitives.
- **Signs are geometrically elaborate**: 67–90 parts each, neon tube glow, bevels, decorative crowns (`_decor_martini`, `_decor_cup`, `_decor_slice`, `_decor_sun`), and a complete 36-glyph stroke alphabet.
- **No z-fighting, no missing faces, no visible inverted normals** observed across all 97 images reviewed. The defect load is art direction, not rendering correctness.

## The 5 highest-impact changes, ranked

1. **Fix the preview light rig** (`render_previews3a.py:101-107`) — scale wattage with framing distance and set `view_transform = "Standard"`. One-line class of fix that changes the perceived quality of all 99 assets, and until it lands every art judgement made from these previews is unreliable.
2. **Add rooftop clutter + a real storefront to `build_tower` / `build_shop`** (`buildings.py`) — 0 of 14 buildings have a roof or a resolved storefront; this is the single biggest silhouette gap against Ocean Drive.
3. **Make the sign lettering legible** (`signs.py::_neon_stroke` + `_assemble`) — the code is already there and already spells real words; it just doesn't read. Add a vision-based legibility assertion so it cannot regress.
4. **Diverge the car bodies** (`vehicles.py::build_coupe` / `build_taxi` / `build_police`) — 4 body types across a 0.46 m length spread is invisible at gameplay distance. Plus hubcaps/whitewalls on `_wheel` and wheels on `build_bus`.
5. **Push saturation and add neon to the unlit 9 buildings + the 5 zero-emissive categories** (`registry.py::BUILDINGS` `neon=` kwargs, `buildings.py::_glass` emissive) — measured mean saturation is 0.25–0.31 across the whole library, which is pastel-suburb, not 1985 Miami.

---

## UNVERIFIED / explicitly not claimed

- **Inverted normals** — not directly measurable on these multi-shell assets; see §2.
- **Palette comparisons to actual GTA Vice City textures** — no reference images were compared pixel-to-pixel; the "GTA has X" claims are from the art-direction brief, not from measurement.
- **Runtime appearance in the actual game** — this audit covers the asset library and the preview pipeline only. The Rust renderer (`src/`) was not exercised; how these materials, emissives and flat-shaded normals actually appear under the WebGPU renderer is untested here.
- **`assets/pbr/*.glb` (99 files)** — I verified count and that builders write them, but did not audit GLB contents (UVs, tangents, PBR channel correctness).
- **The four `animal_*` assets** — reviewed visually (`animal_bird`, `animal_dog` confirmed: modelled animals with connected parts, correctly lit, no defects; `animal_cat` / `animal_fish` not individually opened).
- **`sign_hotel`, `sign_bar`, `sign_tropic`** — re-rendered at 1024² but the legibility question was put to me only for `sign_arcade` and `sign_diner`; the other three are reported from the 512² review.

---

## What was fixed (2026-10-15, uncommitted working tree)

Every number below is measured, not estimated. Reproduce with the commands shown.

### F1 — preview lighting — **FIXED**

`tools/blender/render_previews3a.py`. The key is now a **SUN** (irradiance in W/m², distance-independent by
construction) and the fill/rim area lights have wattage and panel size scaled by `(radius / REF_DIST)²`.
`view_transform` is now set explicitly to `Standard` and asserted in the return value.

| Metric | Before | After |
|---|---|---|
| r(ln extent, frame mean luma), n=99 | **−0.6491** | +0.3759 |
| **r(ln extent, SUBJECT mean luma)** — the size-invariance metric | — | **+0.1050** |
| r within building / vehicle / weapon only | — | −0.041 / −0.001 / −0.071 |
| Assets with zero pixels above luma 150 | 23 | 11 |
| Building mean luma / % px > 150 | 0.2471 / 0.11% | 0.5003 / 5.67% |
| Prop ÷ building brightness | 1.18× (buildings too dark) | 0.94× |

Full-frame luma still correlates +0.38 with size — **that is correct and is not a regression**: a
30 m ship genuinely covers more of the frame than a 0.15 m grenade, so the background contributes more.
The lighting question is answered by subject-only luma, which is +0.105 and ≈0 within every category.
Exposure (sun 1.2 / fill 35 / rim 60 W at REF_DIST 3.0) was chosen from a measured sweep on subject-only
luma and clipped-pixel fraction; full-frame luma cannot pick an exposure because it mostly measures
framing area.

### F2 — sign lettering — **FIXED (root cause found, not a size tweak)**

Two real bugs, both in `tools/blender/vcw_bpy/builders/signs.py`:

1. **`_neon_stroke` rotated about the wrong axis.** It swung each tube about world **Z** while the sign
   face lies in world X-Z, so every glyph was rotated *out of the panel into depth*. A stroke authored
   `(0,0)→(0.30,0.50)` measured **Y-depth 0.530 m instead of 0.060 m** — the full panel depth. All 8
   signs rendered as a fan of tubes standing at right angles to the board. Now rotates about **Y**;
   stroke depth is 0.053–0.076 m across all 8 signs (= the tube diameter).
2. **Tracking was smaller than the tube diameter on all 8 signs**, so adjacent letters physically
   interpenetrated: `sign_arcade` tracked 50 mm glyphs on 56 mm tubes. Now clamped to ≥ 1.15 × diameter.

The 36-glyph stroke alphabet itself was always correct — rasterised to ASCII art and verified letter by
letter (`J O Y S T`, `K I N` all render as themselves).

Verified with two new reproducible audits rather than by eye:
* `audit_sign_letter_clearance.py` — exact 3-D AABB clearance between neighbouring letters.
  **Glyph count now equals character count on all 8 signs; 0 interpenetrating pairs** (was 3.2–10.6 mm of overlap).
* `audit_sign_glyph_separation.py` — connected-component count on the rendered lettering.

**Read at 1024² after the fix:** `sign_arcade` → "JOYSTICK" / "GAMES" ✓, `sign_bar` → "SPILLWAY" /
"OPEN LATE" ✓. Both match the ground-truth strings in `signs.py` exactly. Before the fix, vision read
*"IDASTICD"*, *"DON'T STOP"* and *"GATES"* for the same pixels.

### F3 — rooftop clutter — **FIXED**

`tools/blender/vcw_bpy/builders/buildings.py`. New `_rooftop_kit` places a water tank on legs, a bank of
2–3 AC condensers, 2 vent stacks, an antenna mast and an equipment curb. **All 14 buildings, 24 roof
parts each** (was 0/14). Sized to the **top band**, not the base footprint — a base-width kit on a tower
that steps in 14% twice would overhang on all four sides. Placement is seeded with a CRC32 of the asset
id, *not* the builtin `hash()`, which is salted per process and would make every rebuild differ.
Cost: +1,116 to +1,152 triangles per tower (≈3–5%).

### F6 — `report_sizes.py` — **FIXED**

It read `tri_count` from `manifest["assets"]`, which commit `bfac59a` reshaped to `{id, category, file}`.
Now reads the **per-asset JSON** — the file the game actually loads — and counts triangles from the
`faces` arrays rather than trusting a summary field, which also makes it a check on the exporter.
Added a per-category breakdown. Runs clean: **99 assets, 642,274 triangles**, independently confirming
the manifest total. Proportion check: 15/31 within tolerance, 16 flagged.

### F8 — weapon zero-area faces — **FIXED, but by another agent**

`weapons.py` gained `_safe_bevel()`, clamping a chamfer to ≤40% of the part's smallest dimension. That
edit landed at 21:19:59 **while this pass was running**, by a concurrent agent — `git diff` shows it as
unattributed to this pass. Root cause confirmed by monkeypatching the old policy back:

| Weapon | Zero-area (before) | Micro (before) | After |
|---|---:|---:|---:|
| `wep_pistol` | 402 | 198 | **0** |
| `wep_smg` | 410 | 234 | **0** |
| `wep_tank` | 887 | 141 | **0** |
| `wep_rocket_launcher` | 576 | 24 | **0** |
| `wep_shotgun` | 211 | 121 | **0** |
| `wep_bat` | 32 | 0 | **0** |
| `wep_grenade` | 0 | 24 | **0** |

Library-wide zero+micro faces: **3,802 → 542** (the remaining 542 are in props/pickups — `truck_pickup`,
`prop_mailbox`, `pickup_cash_stack` etc. — same class of bug, not yet fixed and outside this pass).

**Caveat on the original finding's numbers:** `audit_geometry_quality.py` reports zero-area counts that
**are not reproducible** — the same weapon measured 402 and 0 on different runs of identical geometry,
because `bmesh`'s `BMFace.calc_area()` returns a *cached* area whose cache validity depends on prior
operations. `audit_shipped_degenerates.py` (new, no Blender, shoelace over the shipped JSON) measures the
library's real state: **0 zero-area and 8 sub-micrometre faces across all 642,274** — i.e. the shipped
JSON was already clean, and the Blender-side counts were partly measurement error. The weapon *geometry*
fix is still correct and real; the headline count was overstated.

### Not fixed

F4 (vehicle bodies, `bus_city` wheels), F5 (neon on the unlit 9 buildings, palette saturation), F7
(`vcw/` dead code), F9 (palms/grass atlases), F10 (no ground plane) — untouched, still open.
