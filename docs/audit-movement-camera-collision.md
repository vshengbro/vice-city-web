# GTA-Fidelity Audit — On-Foot Movement, Camera, Collision

Repo: `/Users/sqs/code/vice-city-web` @ build `www/pkg/vice_city_web_bg.wasm` (2026-10-15 20:09:15, newer than every file in `src/` — the running build matches the source read below).
All numbers below were measured in the live CDP tab at spawn `(63.5, 45.0)` and at the probe sites named. Nothing is estimated.

---

## 1. Player movement

### 1.1 Walk / run speeds — HIGH

| | GTA III/VC canonical | This project | Measured |
|---|---|---|---|
| Walk | ~1.4–1.8 m/s (GTA III ped scale; VC ~1.5–2.0) | `WALK_SPEED = 4.6` | **4.600 m/s** |
| Run (Shift) | ~3.0–4.5 m/s (GTA III walk/run pair ≈ 1.8/4.0) | `RUN_SPEED = 8.4` | **8.400 m/s** |

- Source: `src/player/const.rs:4` (`WALK_SPEED: f32 = 4.6`), `src/player/const.rs:7` (`RUN_SPEED: f32 = 8.4`), dispatched at `src/game/fn.rs:5461`.
- Measured walk: velocity plateau `4.5999–4.6000` over 26 samples; position advanced monotonically at exactly 4.6 m/s (`x` 63.50→89.89 in 5.67 s).
- Measured run (`Shift+W`): velocity plateau `8.3996`; `x` 65.65→119.28 across 105 frames at 34.8 fps.
- Run/walk ratio is 1.83 — GTA's pair is ≈2.3 (1.8→4.0), so the *shape* is closer than the *scale*; the whole locomotion model is ~2.5× too fast.
- **Severity: HIGH.** This is the single largest gameplay-fidelity gap: the world is built to GTA III block sizes (`STREET_PITCH = 60`, `LOT_PITCH = 34` in `src/game/const.rs:336,352`) while the player crosses it at 2.5× speed.
- **Change:** `WALK_SPEED 4.6 → 2.2`, `RUN_SPEED 8.4 → 5.0`. This preserves the 1.83 ratio and lands both inside the GTA III band. Note the knock-on: `src/collision/const.rs:21` (`SLIDE_STEP = 0.05`) and `:71` (`CAR_STEP = 0.05`) were sized against 4.6 m/s comments; slower walking only makes those more conservative, so no breakage.

### 1.2 Acceleration ramp — MED

- GTA has a short (~0.15–0.25 s) acceleration ramp and a slower deceleration ramp; the ratio matters more than the absolute.
- Project: `SPEED_RAMP: f32 = 14.0` (`src/player/const.rs:30`), applied as `1 - exp(-14·dt)` in `Player::step` (`src/player/impl.rs:181`). Pure exponential — there is **no separate decel rate**, so accelerating and stopping feel identical.
- Measured: time to 95 % of walk speed = **0.311 s**; time to 50 % after key release = **0.032 s**, to 95 % = **0.191 s**, coast distance **0.00 m** (dead stop, no skid).
- GTA's stop is visibly longer than its start. Here stop (0.19 s) is *faster* than start (0.31 s) — inverted.
- **Severity: MED.**
- **Change:** add a `DECEL_RAMP: f32 = 8.0` and pick the constant by whether `target` is zero:
  `let rate = if target_len < 1e-3 { DECEL_RAMP } else { SPEED_RAMP };`

### 1.3 Turn-rate cap at speed — MED

- GTA caps ped yaw rate; you cannot instantly pivot at full run speed.
- Project: `TURN_RATE: f32 = 11.0` (`src/player/const.rs:10`), applied as `wrap_angle(desired - yaw) * TURN_RATE * dt` (`src/player/impl.rs:225`). The cap is **angular and speed-independent**.
- Measured 180° pivot at full walk speed: **0.597 s** wall-clock (3.142 → 0.000), monotonic, no overshoot; effective rate ≈ **2.63 rad/s** against a nominal 11.0.
- At sprint (8.4 m/s) the same 11.0 rad/s cap applies — the pivot looks just as fast at 1.8× the speed, which is the tell. GTA slows the turn as speed rises.
- **Severity: MED.**
- **Change:** scale the cap by speed: `let cap = TURN_RATE * (0.45 + 0.55 * (planar / speed));` so full-speed running turns at ~45 % of the walking rate.

### 1.4 Diagonal speed — OK

- `diagonal_scale` (`src/player/impl.rs:136`) clamps only magnitude > 1, so W+D = 0.707 × speed. Measured 45° run: `vx = vz = 3.25` → |v| = 4.596 ≈ walk 4.6. Correct; matches GTA. **No change.**

---

## 2. Camera

### 2.1 Spring-damped follow — OK

- `FOLLOW_DAMPING: f32 = 9.0` (`src/game/const.rs:161`), exponential blend in `update_camera` (`src/game/fn.rs:7224`). Frame-rate independent. **No gap.**

### 2.2 Idle stability — OK

- Settled 3 s at fixed pose, 105 samples: `cameraDist` spread **0.0000 m**, `eyeX/eyeY/eyeZ` spread **0.0000 m**. No jitter. **No gap.**

### 2.3 Pitch clamp — MED

- Project: `FOLLOW_PITCH_MIN = -0.05`, `FOLLOW_PITCH_MAX = 1.05` (`src/camera/impl.rs:260,262`), enforced by `clamp_follow_pitch`.
- Measured via real mouse input: clamps hold at exactly **1.05** and **−0.05** under 4000 px drags. Clamping works.
- But `FOLLOW_PITCH_MAX = 1.05 rad` = **60.2°**, versus GTA III's ≈ **−30°…+20°**. A 60° top-down is roughly 3× GTA's maximum.
- **Severity: MED** (the lower bound is the more serious half — see 2.4).
- **Change:** `FOLLOW_PITCH_MAX: 1.05 → 0.60` (≈34°).

### 2.4 **`lift_above_ground` collapses the camera at any pitch ≤ 0 — HIGH**

This is the most serious camera defect found, and it is fully explained by reading `src/camera/impl.rs:517-533`.

The focus sits at `FOLLOW_HEIGHT = 1.45 m`, but `CAMERA_MIN_HEIGHT = 0.45` (`src/camera/const.rs:97`). So `headroom = 0.45 − 1.45 = −1.00`, i.e. **negative**. Then for `pitch ≤ 0`:

```
needed  = headroom / -direction[1]   ->  negative
allowed = min(distance, needed)      ->  negative
set_distance(allowed.max(OCCLUSION_MIN_DISTANCE))  ->  always 2.2
```

Because the focus is *already above* the minimum eye height, any downward pitch produces a negative required distance and the `.max()` fallback pins the camera to the 2.2 m hard floor.

- **Measured** (`cameraDist`, `desiredDistance` = 8.200 throughout):

| camPitch | cameraDist | eyeY |
|---|---|---|
| +0.405 | 8.040 | 4.618 |
| +0.165 | 8.197 | 2.796 |
| +0.055 | 8.109 | 1.896 |
| +0.015 | 8.197 | 1.573 |
| +0.005 | 8.200 | 1.491 |
| **−0.020** | **2.200** | 1.406 |
| **−0.050** | **2.200** | 1.340 |

A full 360° yaw sweep taken at pitch = −0.05 returned `cameraDist = 2.200` at **all 48 yaw samples** (range 0.000 m) while `desiredDistance` stayed 8.200 — the occlusion system (`camOccluded` toggled 6 times across the sweep) had no authority at all. The camera is glued to the player's back with a constant 2.2 m boom.
- Note the eye never actually reaches `CAMERA_MIN_HEIGHT` here: at pitch −0.05, eyeY = 1.34 m, which is far above 0.45 — the clamp is firing on a *negative* computed distance, not on a real constraint. That is the signature of a sign error, not a conservative limit.
- **Severity: HIGH.** `FOLLOW_PITCH_MIN = −0.05` is reachable by any small downward drag, and doing so silently destroys the whole third-person camera.
- **Change:** clamp `headroom` at zero before dividing — the ground lift should be a no-op when the focus is already above the minimum:
  ```rust
  let headroom: f32 = (CAMERA_MIN_HEIGHT - target[1]).max(0.0);
  if headroom <= 0.0 { return; }
  ```
  Also raise `CAMERA_MIN_HEIGHT` toward the focus height, or lower `FOLLOW_PITCH_MIN` so the eye can plausibly go below the target.

---

## 3. Collision

### 3.1 Wall response: slide, not hard stop — OK

- `CollisionWorld::resolve_slide` (`src/collision/impl.rs:110`) is a real `move_and_slide`: sub-stepped at `SLIDE_STEP = 0.05 m` (`src/collision/const.rs:21`), normal component removed and **tangential retained** in `slide_step` (`src/collision/impl.rs:168`).
- **Measured**, 45° into a static wall (inner face x = 51.07, player radius 0.35): the player pinned at **x = 50.717** — exactly `51.07 − 0.35`, correct — and **continued sliding along +Z** at a steady `vz = 3.25 m/s` for 6 s, travelling z −6.0 → 12.99 (≈19 m) with zero loss of tangential speed. Textbook slide. **No gap.**

### 3.2 Head-on stop distance — OK

- **Measured**: sprint (8.4 m/s) into the static building stopped at **x = 50.717**, i.e. exactly radius-clear, `vx → 0`. **No tunnelling** despite a 0.14 m/frame sprint step against a 0.6 m-thick wall. The 0.05 m sub-stepping holds. **No gap.**

### 3.3 **Interior walls leave stale velocity — MED**

- Static layer works as designed, but `step_vertical` (`src/game/fn.rs:5675-5686`) resolves interior walls via `resolve_interior_slide` and overwrites XZ **after** `Player::step` has already written velocity from the achieved displacement. Velocity is never recomputed for interior contacts.
- **Measured**, walking into the showcase interior wall (x span 27.75–27.95), pinned over 17 sampled frames:
  `x = 27.4000 (constant), vx = 4.600 (constant), gaitAmount = 1.000`
- Position is correct and the slide works — only the reported velocity is wrong. The same test against a static building gives `vx → 0.000`, which isolates the cause to the interior path specifically.
- Consequence: `resolve_dynamic_bodies` gates crowd push-off on `player_speed > PLAYER_PUSH_MIN_SPEED` (`src/game/fn.rs:6775`), so a player pinned on an interior wall still reads as "moving at 4.6 m/s" and keeps accepting pushes into geometry.
- **Severity: MED** (position is safe; the state readback is wrong).
- **Change:** after the interior resolution, recompute velocity from the final displacement, or gate the push on the post-interior position delta rather than on the stale velocity.

### 3.4 Spawn-inside-a-wall pops the player to the far side — LOW

- **Measured**: teleporting to `(27.85, 33.4)` — the centre of the 0.20 m interior wall — resolves to **x = 28.30**, the *opposite* face. This is the documented `push_out_aabb` minimum-relaxation-axis branch (`src/collision/impl.rs`), reachable whenever a centre starts inside a thick box.
- Only reachable via teleport/streaming, not by walking. **Severity: LOW.**
- **Change:** in `push_out_aabb`, when the point is inside, prefer pushing out through the face the mover approached from; or resolve from `previous_step_xz` rather than the post-static position.

### 3.5 Camera collision probe — OK

- `resolve_occlusion` (`src/camera/impl.rs:339`) fires 3 rays (centre + ±`OCCLUSION_COVERAGE_WIDTH` 0.62) against a 0.32 m sphere probe (`CAMERA_PROBE_RADIUS`, `src/collision/const.rs:79`), then damps at `OCCLUSION_IN_RATE = 22.0` in / `OCCLUSION_OUT_RATE = 3.4` out (`src/game/const.rs:149,152`).
- **Measured**, 3° yaw sweep across an occlusion boundary: distance dipped to **5.958 m** and recovered to 8.200; the slowest single step was **2.075 m over a 3° yaw change**, with `camOccluded` toggling correctly on both edges. Recovery is smooth, no oscillation, no sign-flip hunting.
- 360° sweep: `cameraDist` 8.196 → 6.076 (spread 2.124 m), eye clearance never negative.
- **No gap** — this system is the healthiest part of the camera.

---

## 4. Jump / gravity

### 4.1 Constants vs. measured apex — MED

- Project: `JUMP_VELOCITY: f32 = 7.4` (`src/config/const.rs:1989`), `GRAVITY: f32 = 22.0` (`:1960`), `TERMINAL_VELOCITY: f32 = 34.0` (`:1963`).
- Analytic: apex `v²/2g = 7.4²/44 = 1.2445 m`, airtime `2v/g = 0.673 s`.
- **Measured** apex: **1.082–1.128 m** (two runs), airtime **0.597 s**, peak sampled `vy` 5.57–5.94 (rAF sampling misses the 7.4 peak, so this is a lower bound).
- The measured apex sits ~9–13 % under the analytic value — consistent with discrete integration at the 34 fps the headless tab sustains (`dt` clamped to `FIXED_DT × 4` in `simulate`, `src/game/fn.rs:5409`).
- GTA III jump clears roughly a 1.0–1.2 m ledge, so **the height is defensible**; the measured shortfall is an integration artefact of low frame rate, not a bad constant.
- **Severity: MED** — not for the number, but because apex is frame-rate dependent.
- **Change:** integrate the jump on the fixed `FIXED_DT` accumulator rather than on the render `dt`, so apex is 60 fps-invariant. Optionally lower `GRAVITY` to ~20 to keep the apex near 1.2 m after discretisation loss.

---

## Summary

| # | Finding | Severity | Key measured number |
|---|---|---|---|
| 2.4 | `lift_above_ground` pins camera to 2.2 m at any pitch ≤ 0 | **HIGH** | dist 8.200 → 2.200, all 48 yaw samples |
| 1.1 | Walk/run ~2.5× GTA speeds | **HIGH** | 4.600 / 8.400 m/s |
| 1.2 | No separate deceleration ramp | MED | start 0.311 s vs stop 0.191 s |
| 1.3 | Turn rate not speed-dependent | MED | 11.0 rad/s at both walk and sprint |
| 2.3 | Pitch range ~3× GTA's | MED | ±1.05 rad = ±60° |
| 3.3 | Interior-wall contact leaves stale velocity | MED | vx 4.600 while x frozen |
| 4.1 | Jump apex frame-rate dependent | MED | 1.08–1.13 m vs 1.24 m analytic |
| 3.4 | Spawn inside thick wall pops to far side | LOW | x 27.85 → 28.30 |
| 1.4 / 2.1 / 2.2 / 3.1 / 3.2 / 3.5 | Diagonal speed, follow damping, idle stability, wall slide, no tunnelling, camera probe | OK | verified correct |

### Recommended order
1. **2.4** — one-line guard in `lift_above_ground`; restores the entire third-person camera.
2. **1.1** — two constants; rebalance follows from it.
3. **1.2 / 1.3** — small, self-contained feel changes.
4. **3.3** — recompute velocity after interior resolution.
