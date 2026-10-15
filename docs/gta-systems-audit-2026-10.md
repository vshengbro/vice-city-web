# GTA Systems Audit — traffic / combat / enemies / spawning / interiors

**Repo:** `/Users/sqs/code/vice-city-web` · **Build under audit:** current working tree (uncommitted mods to `src/traffic/*`, `src/combat/*`, `src/enemy/fn.rs`, `src/spawn/fn.rs`)
**Live target:** `http://localhost:8765/` (headless CDP tab) · **Date:** 2026-10-15
**Method:** source reading (read-only, no edits) + instrumented in-game measurement through the game's own debug hook (`window.__vcw`, written by `set_window_json` at `src/game/fn.rs:4508`) sampled per `requestAnimationFrame`.

> All numeric claims tagged **MEASURED** come from a live instrumented run. Constants tagged `src/…:NNN` were re-read from the working tree while writing this report. Anything not directly observed is marked **UNVERIFIED**.

---

## Headline measurements

| # | Measurement | Value |
|---|---|---|
| 1 | NPC cars moving while the player is **driving** | **0 / 9**, max displacement **0.000 m** (player car moved 85.32 m) |
| 2 | NPC cars crossing a junction | speed **exactly 11.50 m/s** at every crossing (12 crossings, 3 cars) — no deceleration |
| 3 | NPC car count in world | **10**, fixed; never spawned, never culled, never recycled |
| 4 | Furthest NPC car from player | **1792.2 m**, still simulated every frame |
| 5 | Enemy count that closed and held | **12**, all `attack`, nearest frozen at **15.7 m**, `chase` count **0** |
| 6 | Pistol fire rate | 9 rounds in **1.79 s** ≈ **5.0 shots/s** (= `PISTOL_FIRE_RATE`) |
| 7 | Pistol damage | enemy HP **1238 → 1212** = **26.0** per hit (= `PISTOL_DAMAGE`) |
| 8 | Pickup counter overflow | HUD **`pickups 212/12`**, cash **$8250** from 12 unique items |
| 9 | Pedestrian count | **18** constant; **18/18** moved (mean 4.05 m in 6 s) |
| 10 | Ped flee on player approach | 2.9 m → **16.5 m** in 4 s |

---

## 1. Traffic AI

### 1.1 — The entire NPC traffic world freezes while the player is in a car — **HIGH**

**(a) GTA.** Traffic keeps flowing around you. Oncoming cars brake, queue at lights, and cars ahead of you are moving while you drive.

**(b) This project.** `Traffic::step()` — the *only* per-frame update for all 10 NPC cars — is called exclusively in the `None` (on-foot) arm of the vehicle match:

```rust
// src/game/fn.rs:5426-5441
match driven {
    Some(index) => { …
        car.drive(forward_input, steer_input, dt, &world);   // player's car ONLY
    }
    None => {
        game.get_mut_traffic().step(dt, Some([here[0], here[2]]), &world);
    }
}
```

There is no second call site. In the driving arm the other 9 cars are never iterated, never integrated, never collided.

**(c) MEASURED.** Player entered car (index 4) and held throttle 6 s:

```
car 4 z  168.49 ->  253.81  (+85.32 m)  v  9.54 -> 24.00   PLAYER CAR
all other 9 cars: moved +0.00 m each; max NPC displacement = 0.000 m
NPC cars that moved while driving: 0/9
```
Speeds of the 9 NPC cars were **bit-identical before and after** (e.g. 11.20 → 11.20, 11.50 → 11.50) — frozen, not blocked. After pressing F, 4/9 resumed within 3 s (max 35.46 m).

**(d) Change.** Call `traffic.step(...)` unconditionally, and pass the hailer only when on foot:
```rust
let hailer = driven.is_none().then(|| [here[0], here[2]]);
game.get_mut_traffic().step(dt, hailer, &world);
if let Some(i) = driven { game.get_mut_traffic().get_car_mut(i).unwrap().drive(…); }
```

---

### 1.2 — No car-following: NPC cars cannot see or brake for the car ahead — **HIGH**

**(a) GTA.** Cars maintain headway and brake for the vehicle in front; queues form at junctions.

**(b) This project.** The complete AI speed controller is a lerp toward a scalar cruise target. Its only brake input is the *player* standing at the kerb hailing a taxi:

```rust
// src/traffic/impl.rs:328-338
let target: f32 = if hailer { 0.0 } else { self.get_cruise() };
let approach: f32 = if hailer { (HAUL_BRAKE * dt).min(1.0) }
                     else { (DRIVE_ACCEL * dt / self.get_cruise().max(1.0)).min(1.0) };
let speed: f32 = self.get_speed();
self.set_speed(speed + (target - speed) * approach);
```
Constants: `CRUISE_SPEED = 11.5` (`src/traffic/const.rs:108`), `HAUL_RANGE = 26.0` (`:118`), `HAUL_BRAKE = 3.4` (`:121`).
No forward-vehicle query, no headway/time-gap term, no IDM. Grep for `idm|headway|car_ahead|lead_car|time_to_collision` over `src/traffic/` → 0 hits. The only car-to-car interaction is a positional push (no speed response) in `src/game/fn.rs:6733-6807`.

**(c) MEASURED.** Because cars are kinematic with no leader detection, the only thing keeping them from interpenetrating is the separation push. Measured minimum same-lane gaps never fell below **4.56 m** (the `2×2.283 m` footprint radius) — cars touching distance but never braking. With traffic frozen while driving (§1.1) and no leader logic, the jam forms the moment §1.1 is fixed.

**(d) Change.** Per frame, project each car's position 1 s ahead along its lane axis, find the nearest car ahead within that window on the same lane, and clamp speed with a simple linear time-gap law: `v_desired = min(CRUISE_SPEED, gap / 1.5)`.

---

### 1.3 — No traffic lights: cars cross every junction at full cruise — **HIGH**

**(a) GTA.** Traffic lights cycle per junction; cross traffic stops.

**(b) This project.** `prop_trafficlight` is placed as a **static decorative mesh** at junction corners (`src/game/fn.rs:1058-1071`) and is never referenced by traffic logic. `ROUTE_PICK = 0.62` (`src/traffic/const.rs:147`) is a dice roll that *teleports* lane state at the junction:
```rust
// src/traffic/impl.rs:410-455 — choose_at_junction
let turn: bool = self.next_route_draw() < ROUTE_PICK;
```
No light state, phase, cycle, stop line, or red/green/amber exists anywhere in `src/`.

**(c) MEASURED.** Instrumented every frame and detected each moment a car's Z crossed a street centreline (`STREET_PITCH = 60.0`, `src/game/const.rs:336`):

```
car4: 4 crossings, speed AT crossing min=11.50 max=11.50 mean=11.50
car7: 4 crossings, speed AT crossing min=11.50 max=11.50 mean=11.50
car9: 4 crossings, speed AT crossing min=11.50 max=11.50 mean=11.50
```
**12/12 crossings at exactly 11.50 m/s** — zero variance, i.e. no junction logic of any kind executes.

**(d) Change.** Give each junction a phase `t = (world_time + street_phase_offset) % period` with a per-axis offset so NS/EW are opposed; clamp `target_speed` to 0 when the car is within `BRAKE_DIST` of the stop line on red.

---

### 1.4 — No despawn / respawn / streaming: 10 cars for the whole infinite world — **HIGH**

**(a) GTA.** Traffic is recycled around the player — a busy city at any moment, empty when you leave.

**(b) This project.** `populate()` is called exactly once at `src/game/fn.rs:8781`, hard-clears and refills the array, and nothing ever removes a car:
```rust
// src/traffic/impl.rs:644-658
self.get_cars_mut().clear();
for (asset, lane_x, start_z, direction) in lanes { … push(TrafficCar::new(…)) }
```
Grep for `despawn|recycle|respawn|pool` in `src/traffic/` → **0 hits**. Count is the literal array length of `TRAFFIC_LANE_PLAN` (`src/game/const.rs:590-641`) = **10**; no `CAR_MAX` constant exists. `TRAFFIC_HALF = 600.0` (`src/game/const.rs:551`) looks like a streaming radius but is used only in a startup log assertion (`src/game/fn.rs:8774-8780`). All 10 cars are updated and drawn every frame with no distance culling (`src/game/fn.rs:7809-7823`).

**(c) MEASURED.** Left the player stationary and sampled 668 frames; the three moving cars receded monotonically:
```
car4  z  903.90 -> 1207.85 m   dist from player max 1760.0 m
car7  z  907.35 -> 1211.30 m   dist from player max 1766.7 m
car9  z  932.86 -> 1236.82 m   dist from player max 1792.2 m
```
Still simulated at **1792 m**. Cars 0/2/3/5/6 sat at speed 0.00 (the hailer rule, §1.5). Simultaneously, closest car to the player was 15.7 m away and only 3 of 10 cars were anywhere near the player.

**(d) Change.** Despawn a car beyond `CAR_DESPAWN_DIST` (suggest 140.0 m) and re-seed it on a lane within `CAR_SPAWN_MIN..CAR_SPAWN_MAX` (suggest 60–110 m) outside the camera frustum. Raise `TRAFFIC_LANE_PLAN` to ~24 entries so density stays constant as the car recycles.

---

### 1.5 — The only brake in the AI is a taxi hailer, and it parks cars for as long as you stand there — **MED**

**(a) GTA.** Traffic slows and flows; no single pedestrian freezes a whole street.

**(b) This project.** Exactly the nearest car within `HAUL_RANGE = 26.0` stops (`src/traffic/impl.rs:736-750`), with `HAUL_BRAKE = 3.4` lerping it to a full stop. The single-car restriction is deliberate (comment at `:736-737`) to avoid gridlock.

**(c) MEASURED.** Standing at (25.8, 34.8), 5 of 10 cars were frozen at **speed 0.00 for 542/542 consecutive sampled frames** (z frozen to 2 decimals the whole time): cars 0, 2, 3, 5, 6. Relocating the player 565 m away restored all of them — they re-accelerated from 0.00 → 8.40 → 11.50 within ~6 s, confirming the freeze is the hailer rule and not a permanent deadlock.

**(d) Change.** Make the hailer wait bounded: stop for at most `HAUL_MAX_WAIT` (suggest 6.0 s), and only if the car is travelling toward the player along its lane (`direction * dot(player - car, lane_forward) > 0`).

---

### 1.6 — Lane following is a closed-form grid, not a road network — **MED**

**(a) GTA.** Cars follow navigation lanes derived from the road mesh, change lanes, curve through junctions.

**(b) This project.** Routing state is 3 scalars (`src/traffic/struct.rs:43-55`: `route_street: i32`, `route_side: f32`, `route_axis: i32`) resolved analytically against an infinite integer grid: `street_axis(i) = STREET_PITCH * i` with `STREET_PITCH = 60.0` (`src/game/const.rs:336`), `lane_axis(street, side) = street_axis + side * LANE_OFFSET` with `LANE_OFFSET = 3.5` (`src/traffic/const.rs:5`). Movement is pure axis-aligned translation force-snapped to the lane centre every frame (`src/traffic/impl.rs:352-403`); yaw comes from a 4-case lookup, not steering (`:279-289`). At a junction the car picks a new lane and snaps position — **no turning arc is ever simulated for AI cars**; the bicycle/drift model is player-only (`drive`, `:473-587`). Lane changes: absent. Overtaking: absent.

**(c) MEASURED.** Every car held a perfectly constant cross-coordinate for the whole run: `carX = [63.5 ×5, -56.5 ×5]`, never deviated by more than rounding. Cars reaching a junction showed a discontinuous position snap rather than an arc.

**(d) Change.** Represent each street as a lane polyline with per-junction approach/exit tangents so cars steer through the turn; keep the analytic grid as the generator but interpolate heading along the polyline.

---

## 2. Pedestrians

### 2.1 — Peds move and flee the player, but cannot be shot — **HIGH (as a gap)**

**(a) GTA.** Peds walk sidewalk paths, panic and scatter from gunfire, can be killed; shooting them is a crime.

**(b) This project.** Peds live in `combat`, not `traffic` (grep `ped` in `src/traffic/` → only Rust's `panic!` in a test). They steer in a straight line to a goal 9.0 m away and explicitly **cross** the street rather than use sidewalks — `src/spawn/fn.rs:142-146` (`行人在街上左右横穿,不是绕圈`). Flee is triggered **only by player proximity**: `src/combat/impl.rs:837-849` sets `flee_timer` when `distance < PED_ALERT_RADIUS (11.0)`. No bullet, explosion, or noise event ever calls `set_flee_timer`. `Pedestrian` has no `health` field and no `damage()` (`src/combat/struct.rs:144-194`), and the hitscan loop iterates enemies only (`src/game/fn.rs:5933`). Run-over exists but only for the player's car: `src/game/fn.rs:6461-6474`.

**(c) MEASURED.** All **18** peds moved (displacement min 1.68 m, mean 4.05 m, max 8.91 m over 6 s), gait uniform at 1. Flee works: nearest ped went 2.9 m → **16.5 m** in 4 s after the player teleported beside it. Shooting: across every firing test the ped count stayed at exactly **18** with `down=0` — no ped was ever damaged. Ped count also never fell below 18 after 6 knock-downs, confirming **no distance despawn**.

**(d) Change.** Give `Pedestrian` an HP field and include it in the hitscan loop; broadcast a `noise` event on gunfire/grenade within `PED_PANIC_RADIUS` (suggest 45 m) that sets flee with a goal *away from the noise origin* rather than away from the player; add `WANTED_PER_PEDSHOOT`.

---

### 2.2 — Fixed 18-ped population, no distance despawn — **MED**

**(a) GTA.** Ped density scales with where you are and recycles.

**(b) This project.** `PED_COUNT = 18` is a spawn batch, not a live cap; peds are replaced only wholesale by the streaming rebuild (`src/game/fn.rs:5584-5596`) at `STREAM_REBUILD_STEP = 60.0`. Spawn ring is a hardcoded `7.0 + (slot % 6) * 4.4` m → 7.0–29.0 m (`src/spawn/fn.rs:136`). The code comment at `src/spawn/fn.rs:90` says placement deliberately avoids the spawn point, but that has no effect 30+ frames later.

**(c) MEASURED.** `peds` = **18** at every sample across every test, including after driving 85 m and after knocking 6 peds down.

**(d) Change.** Despawn a ped beyond ~80 m and respawn it in the 40–70 m ring, keeping a live cap separate from the initial batch count.

---

## 3. Combat

### 3.1 — Hitscan, damage and fire rate all work and match the constants — **OK**

**(a) GTA.** Distinct per-weapon fire rates, damage, magazines and reloads.

**(b) This project.** `Weapon { Pistol, Smg, Bat, Grenade, Unarmed }` (`src/combat/enum.rs:6-21`) with per-weapon `match` tables. Player guns are **instant hitscan** — one ray against static geometry, then an O(n×m) scan over `game.get_enemies()` (`src/game/fn.rs:5928-5958`); no spatial hash (grep `spatial|broadphase|uniform_grid` → 0 hits in combat/enemy/spawn).

**(c) MEASURED.**
- Fire rate: held the mouse 1.79 s → mag 9 → 0, i.e. **5.0 shots/s** = `PISTOL_FIRE_RATE = 5.0` (`src/config/const.rs:1417`).
- Damage: enemy HP sum **1238 → 1212** = **26.0** = `PISTOL_DAMAGE = 26.0` (`:1421`). A separate aim-confirmed hit gave **100 → 48** (damage 52 at close range, consistent with falloff-free damage inside 22 m).
- Kill: `kills` reached **1**, cash **$60** = `ENEMY_CASH_DROP`.
- Weapon switching works and preserves magazines: Digit2 → `weapon=1, mag=30` (SMG), Digit3 → `weapon=2, mag=0` (Bat), Digit1 → `weapon=0, mag=9` (Pistol retained).
- Damage falloff implemented and **UNVERIFIED at range** (not measured): `falloff()` at `src/combat/fn.rs:15-27`, `FALLOFF_START = 22.0`, `FALLOFF_MIN = 0.35`, `GUN_RANGE = 90.0` (`src/config/const.rs:1431-1435`).

**(d) Change.** None for correctness. For scaling, add a uniform-grid broadphase to the enemy hit scan before the enemy count grows.

---

### 3.2 — Player weapons have zero spread; only enemies miss — **MED**

**(a) GTA.** Weapons have per-weapon accuracy; sustained fire wanders.

**(b) This project.** `fire_weapon` uses the raw aim vector with no deviation:
```rust
// src/game/fn.rs:5917-5919
let flat: f32 = pitch.cos();
let direction: Vec3 = [aim[0] * flat, pitch.sin(), aim[1] * flat];
```
`ENEMY_SPREAD = 0.07` is applied **only** to enemy fire (`src/enemy/fn.rs:221`), and enemy accuracy is a dot-product test rather than a raycast (`src/game/fn.rs:6177-6238`). The player is perfectly accurate at any range.

**(c) MEASURED.** 8 consecutive aimed pistol shots at a stationary target produced the deterministic pattern *hit, then 7 misses* — consistent with a fixed zero-spread ray plus enemies moving out of the line, not with any spread model. Enemy hit feedback (`HIT_KNOCKBACK 2.4`, hitmarker 0.16 s) did register on the hit.

**(d) Change.** Add per-weapon spread (suggest pistol 0.012 rad standing, ×1.8 moving, ×2.4 sustained after 4 rounds) applied to the yaw/pitch before the raycast.

---

### 3.3 — Grenade selected + fire button fires a 90-damage hitscan and drains SMG ammo — **MED (defect)**

**(a) GTA.** Grenades are thrown on a dedicated input.

**(b) This project.** `can_fire` returns `true` for `Weapon::Grenade` unconditionally (`src/combat/impl.rs:242`) and `Arsenal::fire` routes everything except `Bat` into `set_smg_ammo` (`:274-293`). `falloff()` exempts only `Bat` (`src/combat/fn.rs:17`), so a grenade "shot" raycasts at `GRENADE_RANGE = 24.0` for **90.0** damage while decrementing SMG ammo. The real grenade path is `throw_grenade` (`src/game/fn.rs:7356`) bound to **G** (`:3821-3824`). Separately, `set_cooldown(Weapon::Grenade.fire_rate() * GRENADE_COOLDOWN)` (`src/game/fn.rs:7387`) evaluates to `(1/1.2)*1.2 = 1.0 s`, not 1.2 s.

**(c) MEASURED.** Not directly fired (grenade tubes were 0 in the clean-state runs, and `grenadeTubes: 2` only after the pickup-recollection bug in §3.5 filled the arsenal). **UNVERIFIED at runtime** — the code path is unambiguous, so treat as high-confidence but unproven in-game.

**(d) Change.** Make `can_fire` return `false` for `Weapon::Grenade` and bind the fire button to it only while actually throwing; fix the cooldown to `set_cooldown(GRENADE_COOLDOWN)`.

---

### 3.4 — No headshots, no enemy armour, no weapon-switch cooldown — **LOW**

**(a) GTA.** Headshots multiply damage; enemies have armour tiers; switching has a small delay.

**(b) This project.** Damage is a single scalar (`Enemy::damage(f32)`, `src/combat/impl.rs:486`) against one `ENEMY_HIT_RADIUS = 0.75` sphere centred at `AIM_CHEST_HEIGHT = 1.32` (`src/game/fn.rs:6014-6030`). `set_weapon` (`src/combat/impl.rs:158-164`) only sets `current` and clears `reloading` — it never touches `cooldown`. Grep `headshot|armour|critical` in `src/` → only `PART_HEAD` asset id and the *player's* armour.

**(c) MEASURED.** Switching Pistol→SMG→Bat→Pistol back-to-back always applied instantly with no delay field observed in `__vcw.combat`.

**(d) Change.** Add a `head` sphere at ~1.65 m with a 2.5× multiplier, and a `WEAPON_SWITCH_TIME = 0.35 s`.

---

### 3.5 — **Pickup counter overflows and pays out repeatedly — HIGH (defect, measured)**

**(a) GTA.** A pickup is consumed once.

**(b) This project.** `collect_pickups` marks a **clone** as taken, never the live pickup:
```rust
// src/game/fn.rs:7328-7332
let picked: Option<Pickup> = game.get_mut_traffic().get_pickup_mut(index).cloned();
if let Some(mut picked) = picked { apply_pickup(game.get_mut_player(), &mut picked); }
game.get_mut_player().set_collected_push(index);
```
`apply_pickup` sets `pickup.set_taken(true)` (`src/traffic/fn.rs:171`) — **on the clone**, which is dropped at the end of the scope. The real `Pickup` in `get_pickups_mut()` stays `taken == false`, so the guard `if pickup.get_taken() { continue; }` (`src/game/fn.rs:7310`) never trips and the item can be re-collected forever. The counter is an append-only `Vec::len()` (`src/player/impl.rs:54-55`), while the denominator is the fixed table length (`src/game/fn.rs:4861`, 12 entries in `PICKUP_PLACEMENTS`).

**(c) MEASURED.** Teleporting onto the 12 pickup positions for 3 passes:
```
round0: pickups  71/12   (HUD showed 73/12)
round1: pickups 140/12
round2: pickups 212/12
cash $8250, grenadeTubes 2, armor 95.2
```
Counter exceeded the unique total by **200**; cash grew without bound from 12 static items.

**(d) Change.** Mark the live pickup:
```rust
apply_pickup(game.get_mut_player(), game.get_mut_traffic().get_pickup_mut(index).unwrap());
```
and make `collected` a `Vec<usize>` deduplicated (or a `[bool; N]`) so the HUD is clamped to `total`.

---

## 4. Wanted level

### 4.1 — Stars, thresholds and decay work; escalation is headcount-only — **MED**

**(a) GTA.** 0–6 stars, escalating response: patrol cars → more cops → roadblocks/SWAT, with decay only when you evade.

**(b) This project.** Continuous `heat: f32` → `stars: u32`, `WANTED_MAX = 5` (`src/config/const.rs:1490`), threshold `WANTED_PER_STAR = 34.0` (`:1500`), hard cap `170.0`. Heat sources: bullet hit `WANTED_PER_SHOT + WANTED_PER_HIT = 3.0 + 9.0 = 12.0` (`src/game/fn.rs:5970-5971`), enemy fire `1.5` (`:6186`, **added before the LOS gate at `:6210`, so blocked shots still accrue heat**), grenade hit `9.0`, carjack `22.0` (police car `44.0`), run-over `30.0`. Decay: `WANTED_COOLDOWN = 15.0` s unseen, then `0.25 × 34.0 / 15.0 ≈ 0.567` heat/s (`src/combat/impl.rs:624-643`). Hideouts cool at `HIDE_COOL_RATE = 42.0` heat/s. Response is `deploy_police` only (`src/spawn/fn.rs:47-86`) → `WANTED_COP_PER_STAR = 2` foot cops, capped at `ENEMY_WANTED_TOTAL = 8`. **Stars change only the headcount — never enemy health, speed, accuracy or fire rate.**

**(c) MEASURED.** Clean start 0 stars / heat 19.89, 7 thugs, 0 police. Firing at thugs:
```
shot  7  heat  37.4  stars=1  police=2
shot 13  heat  71.9  stars=2  police=4
(stars 3, heat 110.87, police 6)   (stars 5, heat 170.0, police 8)
```
Then teleported far away: at t≈18 s `stars=3 heat=110.9 unseen=0.0 police=6` → at t≈30 s `stars=0 heat=14.9 unseen=23.0 police=0`. Police count tracked `2 × stars` exactly. Enemies at every star level had identical `hp` (70 thug / 100 cop) and the same fire behaviour.

**(d) Change.** Scale cop stats with stars (e.g. `ENEMY_DAMAGE * (1.0 + 0.35 * stars)`, `ENEMY_FIRE_RATE * (1.0 + 0.15 * stars)`), add `WANTED_MAX = 6` with a 6th tier, and introduce escalating units (patrol cars at ≥2, roadblocks at ≥4) instead of pure foot-cop headcount. Move the `add_heat` call at `src/game/fn.rs:6186` below the LOS gate.

---

## 5. Enemy AI & spawning

### 5.1 — Enemies hold a 15–16 m firing line and never close — **MED**

**(a) GTA.** Enemies path toward you, use cover, flank, and retreat when hurt.

**(b) This project.** A real 6-state machine exists (`Idle/Patrol/Chase/Attack/Flee/Dead`, `src/combat/enum.rs:28-41`) with priority 死亡 > 逃跑 > 开火 > 追击 > 巡逻 > 站桩 (`src/enemy/fn.rs:5`). Detection is distance **and** raycast LOS at `ENEMY_SIGHT = 46.0` (`src/enemy/fn.rs:29-30`). Attack at `ENEMY_FIRE_RANGE = 26.0` with `ENEMY_STANDOFF = 9.0` (`src/enemy/fn.rs:66-78`). Movement is a raw XZ delta to the player — **no pathfinding, navmesh, or flow field anywhere in `src/`** (`src/enemy/fn.rs:83`). Cover/flank/suppression: grep → 0 hits.

**(c) MEASURED.** With the player standing still, over 10 s:
```
t=1s nearest 24.6m  chase/attack/patrol = 1/3/3
t=5s nearest 16.3m  chase/attack/patrol = 0/8/3
t=10s nearest 15.7m chase/attack/patrol = 0/12/3
```
**`chase` count fell to 0** and stayed there; 12 of 15 enemies sat in `attack` frozen at 15.7–25.9 m, never entering `ENEMY_FIRE_RANGE`'s inner region to press the attack. They *do* track: teleporting the player +30 m in Z restored the gap to 18.8 m nearest, so pursuit works — it is specifically the closing that never happens.

**(d) Change.** In `decide`, do not transition to `Attack` until `distance <= ENEMY_FIRE_RANGE * 0.6` (≈15.6 m) and keep `Chase` for the band between that and 26 m, so `move_to` actually pulls them in. Add a simple wall-slide detour when the direct XZ delta is blocked for >0.5 s.

---

### 5.2 — Spawns ignore line-of-sight, ground height and view protection — **MED**

**(a) GTA.** Enemies appear out of view or from behind cover.

**(b) This project.** Both spawners are purely polar around the player — police ring 16–27 m (`src/spawn/const.rs:28,31`), thugs 34–62 m (`:34,37`) — with **no LOS test, no frustum check, and hardcoded `y = 0.0`** (`src/spawn/fn.rs:68-82, 104-112`). Population: 7 thugs fixed, police `min(WANTED_COP_PER_STAR * stars, 8)`.

**(c) MEASURED.** Enemies were observed at **0.2–0.7 m** from the player during the fire tests (12 of 15 within 25.9 m), well inside the 16 m inner ring — i.e. the player can end up inside the spawn ring, and nothing defuses or displaces them.

**(d) Change.** Reject a candidate if `has_line_of_sight` to the camera eye, or if it is within 12 m of the player; snap `y` to `world.ground_height(x, z)`.

---

## 6. Interiors

### 6.1 — Only 2 buildings, entered by walking through a hole, with no interior world — **MED**

**(a) GTA.** Buildings have door triggers, a separate interior world with its own lighting, peds, mission-givers, save icons and shops; interiors suppress the wanted level.

**(b) This project.** Exactly **2** interiors, fixed at compile time: `type ShowcaseSpecs = [ShowcaseSpec; 2]` (`src/game/type.rs:9`), built once at `src/game/fn.rs:7740`. Locations LOFT centre (23, 30) and SHOP centre (37, 30), doors at (28.2, 30) and (32.3, 30). There is **no trigger radius and no prompt** — the doorway is a literal geometric gap between two wall piers (`src/game/fn.rs:775-785`, `SHOWCASE_DOOR_HALF = 0.80` → 1.60 m clear width). Entry is continuous walking in the same world; nothing is loaded, streamed or swapped, so state preservation is trivially moot. **Absent:** world→interior transform, interior lighting (grep `interior` in `src/render/`, `src/webgpu/` → 0), mission-givers, save icons, payphones/shops, interior-only peds/vehicles/pickups, wanted suppression indoors. **Present:** two floors + a 10-step stairwell, `blocks_sight` wired into AI fire and grenades (`src/game/fn.rs:6212-6215, 7407`), and camera occlusion indoors (`src/camera/impl.rs:403-440`).

**(c) MEASURED.** Walking in from outside via the walk channel:
```
(29.81,30.00) y=0.00    <- street
(28.32,30.00) y=0.00    <- door threshold
(26.60,30.00) y=0.15    <- stepped up onto the interior slab (SHOWCASE_GROUND_TOP=0.15)
(25.83,30.00) y=0.15    <- blocked by interior furniture; y retained
```
`interiors` reported `{"count":40,"inSolid":false}` — **`count` is the number of collision primitives in `FloorWorld.floors` (`src/game/fn.rs:4475-4477`), not the number of interiors**; it is a static 40 = 2 buildings × 20 AABBs and carries no information about which interior you are in. Attempting to climb to the upper floor (top `3.20`) never succeeded: `playerY` stayed at 0.15 and the player was stopped at x=25.83 — **stair traversal to the upper storey is UNVERIFIED/not achieved under scripted walking.** No interior id exists anywhere in the codebase (grep `in_interior|current_interior|interior_id` → 0 hits), so interiors cannot drive music, wanted suppression or mission logic.

**(d) Change.** Give `Floor` an `interior_id`, expose `window.__vcw.interiors.current` (id + name), and add a door trigger volume with an "ENTER" prompt. Suppress `update_wanted` while `current` is a valid interior id (GTA's escape behaviour). Expose the debug count as "buildings loaded" rather than raw primitives.

---

## Gap summary (ordered by impact)

| Severity | Finding | Evidence |
|---|---|---|
| HIGH | All NPC traffic freezes while the player is in a car | 0/9 cars moved, 0.000 m, `src/game/fn.rs:5426-5441` |
| HIGH | Pickups re-collectable forever; HUD shows `212/12`, cash $8250 | clone marked taken, `src/game/fn.rs:7328-7332` |
| HIGH | No car-following / no headway / no braking for the car ahead | `src/traffic/impl.rs:328-338` |
| HIGH | No traffic lights — 12/12 junction crossings at exactly 11.50 m/s | §1.3 measurement |
| HIGH | No despawn/respawn — cars persist at 1792 m, count fixed at 10 | `src/traffic/impl.rs:644-658` |
| HIGH | Pedestrians cannot be shot or killed; no gunfire panic | ped count 18, `down=0` after all fire |
| MED | Enemies never close past ~15.7 m (`chase` count 0) | 10 s observation |
| MED | Wanted response is headcount-only; no stat scaling, no 6th star, no cars/SWAT | police = 2×stars exactly |
| MED | Enemy fire adds wanted heat even when the shot is blocked | `src/game/fn.rs:6186` vs `:6210` |
| MED | Spawns ignore LOS, ground height and view protection | spawn ring as low as 0.2 m |
| MED | Player weapons have zero spread | `src/game/fn.rs:5917-5919` |
| MED | Grenade weapon slot fires a 90-dmg hitscan draining SMG ammo (code path only) | `src/combat/impl.rs:242, 274-293` |
| MED | Only 2 interiors, no trigger/prompt, no interior world | `src/game/type.rs:9` |
| MED | Peds fixed at 18, no distance despawn | `PED_COUNT = 18` |
| MED | Taxi hailer freezes the nearest car indefinitely | 5/10 cars at 0.00 m/s for 542 frames |
| LOW | No headshots, no enemy armour, no weapon-switch cooldown | `src/combat/impl.rs:158-164, 486` |
| LOW | Lane following is analytic; no turning arc, no lane change, no overtaking | `src/traffic/impl.rs:352-403` |

**UNVERIFIED:** damage falloff at range (>22 m) was not measured; grenade fire-button defect was not reproduced at runtime (no grenades in a clean state); interior upper-floor stair traversal was not achieved.
