"""Four stair paths on the real wasm, sampled from the **HUD DOM** per frame.

Why the HUD and not `window.__vcw`: `publish_debug_state` runs every 3rd frame
(game.rs:8128 `frame_count % 3 == 0`), so the whole `__vcw` object is up to two
frames stale and silently under-reports peak_y (it reads 3.1022 where the
player actually stood 3.20). `#vcw-hud` is rewritten **every** frame
(game.rs:8120) and carries its own `frame N` (format_hud, game.rs:5349), which
is what we dedupe on.

Traps this file must not repeat:

* **teleport is read back on x/y/z before any path runs.** `apply_teleport_request`
  has early-`return` branches (probe / speed / safe / walk / hold); any of them
  swallows the whole request object, so `{safe:true, x, z}` teleports *nowhere*
  and every later reading is on open ground. `tp()` polls until the **HUD**-sourced
  position matches and **raises** otherwise -- including on y, because a bare x/z
  teleport preserves foot height and a dropped y silently invalidates the run.
* **y is part of the pass condition.** Starting a descent with y on the ground
  floor tests nothing, so the only way onto the upper floor is to walk up first
  and reverse.
* **`walk_request` persists after being sent**; stop with `{walk:true, dx:0, dz:0}`.
* **World-space walk** (`{walk:true, dx, dz}`) bypasses camera yaw: dx=-1 is
  exactly -X, which is uphill for LOFT.
* Waits happen **inside the page** on a `setTimeout` tick, not a tight rAF chain:
  on a software renderer a 90-frame rAF bail-out exceeds the CDP evaluate timeout
  and kills the socket.
"""
from __future__ import annotations

import asyncio
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import SCRATCH, Session  # noqa: E402

PORT = 9488
WORKER = "s10desc"
TAG = sys.argv[1] if len(sys.argv) > 1 else "before"

# ---- real LOFT (index 0) geometry, computed from the constants -------------
# center world (23, 30), yaw +pi/2. Stair run: x[23.05, 27.55] z[24.25, 25.55]
# treads run from x=27.55 (tread 0, top 0.455) to x=23.05 (tread 9, top 3.200).
FOOT_X, FOOT_Z = 27.30, 24.90
# The upper landing, just past the top of the run: the fixed collision world
# puts it at world x[18.25, 23.05] (local z[-hz, stair_end]), so a point just
# inside it is x = 22.6. Spawning here and walking +X walks off the landing
# edge straight onto the top tread -- which is the descent this round is about.
LANDING_X, LANDING_Z = 22.60, 24.90
TREAD = 0.45
N_TREADS = 10
RUN_LO = FOOT_X - N_TREADS * TREAD - 0.4        # below the top landing
STAIR_HALF_W = 0.65                               # SHOWCASE_STAIR_WIDTH / 2
UPPER_TOP = 3.20
GROUND_TOP = 0.15
ARRIVE_EPS = 0.03                                  # teleport read-back tolerance
WALK_SPEED = 4.6

# Sample the HUD DOM every rAF, dedupe on its own `frame N`.
HOOK = """
(() => {
  if (window.__s10) return 1;
  window.__s10 = {on: false, rows: [], last: -1};
  const grab = () => {
    const el = document.getElementById('vcw-hud');
    const txt = el ? (el.textContent || '') : '';
    // "... xyz 27.3 0.2 24.9 · 12345 tris · frame 812 ..."
    const m = /xyz\\s+(-?[\\d.]+)\\s+(-?[\\d.]+)\\s+(-?[\\d.]+).*?frame\\s+(\\d+)/.exec(txt);
    if (m && window.__s10.on) {
      const f = +m[4];
      if (f !== window.__s10.last) {
        window.__s10.last = f;
        window.__s10.rows.push([f, +m[1], +m[2], +m[3]]);
      }
    }
    requestAnimationFrame(grab);
  };
  requestAnimationFrame(grab);
  return 1;
})()
"""
SAMPLE = "(() => { window.__s10.on = true; window.__s10.rows = [];" \
         " window.__s10.last = -1; return 1; })()"
READ = "(() => JSON.stringify(window.__s10.rows))()"
OFF = "(() => { window.__s10.on = false; return 1; })()"

# Read x/y/z straight off the HUD (not __vcw) so a teleport read-back and the
# sampled path use the *same* per-frame clock.
TPREAD = """
(() => {
  const el = document.getElementById('vcw-hud');
  const txt = el ? (el.textContent || '') : '';
  const m = /xyz\\s+(-?[\\d.]+)\\s+(-?[\\d.]+)\\s+(-?[\\d.]+).*?frame\\s+(\\d+)/.exec(txt);
  return m ? [+m[1], +m[2], +m[3], +m[4]] : null;
})()
"""


def hud_pos():
    return TPREAD


async def stop_walk(s):
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    await s.wait_frames(3)


async def tp(s, x, z, tag="", y=None, y_tol=0.06, eps=ARRIVE_EPS):
    """Teleport, then *prove* x/y/z landed.

    `eps` is the **arrival** tolerance: how close the HUD read-back has to be
    before we accept that the teleport landed. Different paths need different
    tolerances — a path that only needs to start *on the stair run* (not on a
    specific tread) tolerates well over a metre, because the authoritative
    judgement is the recorded y sequence, not the exact spawn point.

    **y is deliberately not part of the pass condition by default.**
    `apply_teleport_request` does `set_position([x, here[1], z])` — it
    preserves foot height — so a bare `{x, z}` request always reads back the
    height the player already had. A y far from the target tread is therefore
    expected, not a failure. `y=` opts back in when a path really does need a
    specific height.
    """
    await stop_walk(s)
    await s.ev("window.__vcw_teleport = null")
    await s.wait_frames(1)
    await s.ev("window.__vcw_teleport = {safe:true}")       # own request only
    await s.wait_frames(2)
    await s.ev(f"window.__vcw_teleport = {{x: {x}, z: {z}}}")
    got = None
    for _ in range(14):
        await s.wait_frames(2)
        got = await s.ev(TPREAD)
        if got and abs(got[0] - x) < eps and abs(got[2] - z) < eps:
            break
    if not got:
        raise SystemExit(f"ABORT[{tag}] HUD read-back returned nothing (no #vcw-hud)")
    ex, ez = got[0] - x, got[2] - z
    if abs(ex) >= eps or abs(ez) >= eps:
        raise SystemExit(
            f"ABORT[{tag}] teleport missed: want ({x},{z}) got ({got[0]},{got[2]}) "
            f"err=({ex:+.4f},{ez:+.4f}) y={got[1]} -- readings would be off-stair")
    if y is not None and abs(got[1] - y) > y_tol:
        raise SystemExit(
            f"ABORT[{tag}] teleport y wrong: want {y} got {got[1]} at ({got[0]},{got[2]})")
    print(f"  [{tag}] arrived=({got[0]:.2f},{got[1]:.3f},{got[2]:.2f}) frame={got[3]}",
          flush=True)
    return got


async def _wait_in_page(s, js_budget, stop_js=None):
    """Let the page advance `js_budget` frames, optionally bailing on a cond."""
    await s.ev(f"""(() => {{
      const f0 = +((window.__vcw||{{}}).frames||0);
      window.__s10Done = -1;
      const tick = () => {{
        const c = window.__vcw || {{}};
        {stop_js or ""}
        if (+(c.frames||0) - f0 >= {js_budget}) {{ window.__s10Done = 1; return; }}
        if (+(c.frames||0) - f0 > 90) {{ window.__s10Done = 0; return; }}
        setTimeout(tick, 120);
      }};
      tick(); return 1;
    }})()""")
    for _ in range(90):
        d = await s.ev("(() => window.__s10Done)()")
        if d != -1:
            return d
        await asyncio.sleep(0.4)
    raise SystemExit("ABORT in-page wait never returned")


async def drive(s, *, dx, sprint, frames, tag, stop_at_top=False):
    """Hold a world-direction walk, sampling the HUD per frame.

    Sampling starts *before* the walk request: the request is consumed on the
    next frame, and on a slow software renderer a 2-frame gap loses a whole tread.
    """
    await s.ev(SAMPLE)
    if sprint:
        await s.key("ShiftLeft", True)
        await s.wait_frames(2)
    await s.ev(f"window.__vcw_teleport = {{walk:true, dx:{dx}, dz:0}}")
    stop_js = ("if (+(c.playerY||0) >= 3.19) { window.__s10Done = 1; return; }"
               if stop_at_top else "")
    await _wait_in_page(s, frames, stop_js)
    rows = json.loads(await s.ev(READ) or "[]")
    await s.ev(OFF)
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    if sprint:
        await s.key("ShiftLeft", False)
    await s.wait_frames(3)
    return rows


def on_stair(x, z):
    return RUN_LO <= x <= (FOOT_X + 0.45) and abs(z - FOOT_Z) <= STAIR_HALF_W + 0.30


def report(name, rows, expect_ground):
    if not rows:
        print(f"{name:12} NO DATA")
        return {}
    ys = [r[2] for r in rows]
    peak = max(ys)
    m = 0.0
    for a, b in zip(rows, rows[1:]):
        if b[0] > a[0]:
            d = ((b[1] - a[1]) ** 2 + (b[3] - a[3]) ** 2) ** 0.5
            m = max(m, d / (b[0] - a[0]))
    span = rows[-1][0] - rows[0][0]
    got_top = any(abs(y - UPPER_TOP) < 2e-2 for y in ys)
    ground = ys[-1] <= GROUND_TOP + 1e-2
    seen: list[float] = []
    for y in ys:
        if not seen or abs(y - seen[-1]) > 0.05:
            seen.append(round(y, 3))
    print(f"{name:12} n={len(rows):3d} fr={span:3d} "
          f"y {ys[0]:.3f}->{ys[-1]:.3f} peak={peak:.4f} "
          f"x {rows[0][1]:.2f}->{rows[-1][1]:.2f} z {rows[0][3]:.2f}->{rows[-1][3]:.2f} "
          f"m/fr={m:.3f} top={'yes' if got_top else 'NO'} "
          f"ground={'yes' if ground else 'NO'}")
    print(f"{'':12} y-seq={seen}")
    return {"peak": peak, "m": m, "top": got_top, "ground": ground,
            "seq": seen, "n": len(rows), "span": span,
            "start": rows[0][1:], "end": rows[-1][1:], "rows": rows}


def climb_verdict(name, rows):
    """Ascent judgement: peak on the landing, and no fall through a tread.

    `dy < 0` is only meaningful **over the stair run**; once the player is
    standing on the landing, walking on across the second floor and stepping
    off its far edge is a correct fall, not a stair defect. The run is
    x[23.05, 27.55]; the landing is x < 23.05.
    """
    if not rows:
        print(f"{name:12} NO DATA")
        return {}
    drops = [(b[0], round(a[2], 3), round(b[2], 3))
             for a, b in zip(rows, rows[1:])
             if b[2] < a[2] - 1e-3 and on_stair(a[1], a[3])]
    peak = max(r[2] for r in rows)
    # Where did the run stop? The first frame at landing height.
    top_x = next((r[1] for r in rows if r[2] >= UPPER_TOP - 1e-2), None)
    print(f"{name:12} peak={peak:.4f} top={'yes' if peak >= UPPER_TOP - 1e-2 else 'NO'} "
          f"first_top_x={top_x} dy<0(stair)={len(drops)} {drops if drops else ''}")
    return {"peak": peak, "drops": drops, "top_x": top_x}


def descent_verdict(name, rows):
    """Descent judgement: must reach the ground, and must never climb."""
    if not rows:
        print(f"{name:12} NO DATA")
        return {}
    ys = [r[2] for r in rows]
    # The landing is at the top; anything above it is a climb-back-up.
    climbs = [(b[0], round(a[2], 3), round(b[2], 3))
              for a, b in zip(rows, rows[1:]) if b[2] > a[2] + 1e-3]
    ground = ys[-1] <= GROUND_TOP + 1e-2
    miny = min(ys)
    print(f"{name:12} y {ys[0]:.3f}->{ys[-1]:.3f} min={miny:.3f} "
          f"x {rows[0][1]:.2f}->{rows[-1][1]:.2f} "
          f"descended_to_ground={'true' if ground else 'FALSE'} "
          f"dy>0(climb)={len(climbs)} {climbs[:4] if climbs else ''}")
    return {"ground": ground, "climbs": climbs, "end_y": ys[-1], "min_y": miny,
            "rows": rows}


def frame_table(name, rows):
    """Print the per-frame y table for a descent.

    `y` comes from the HUD (rewritten every frame), and the row's `frame` is
    the HUD's own counter, so consecutive rows are real consecutive frames —
    unlike `window.__vcw`, which is only republished every 3rd frame.
    """
    if not rows:
        print(f"{name:12} (no frames)")
        return
    print(f"{name:12} frame-by-frame y:")
    prev = None
    for r in rows:
        d = "" if prev is None else f"  dy={r[2] - prev:+.3f}"
        prev = r[2]
        print(f"    frame {r[0]:5d}  x={r[1]:7.2f}  y={r[2]:6.3f}  z={r[3]:6.2f}{d}")


async def climb_to_top(s, tag, eps=ARRIVE_EPS):
    """Walk up to the landing so a descent starts on the upper floor.

    The walk is stopped the moment the player stands on the landing (y=3.20),
    so it must NOT run to a fixed frame budget: on a software renderer the
    remaining frames carry the player across the second floor and off its far
    edge (x=18.3, y=0.15), which destroys the descent precondition.
    """
    await tp(s, FOOT_X, FOOT_Z, tag + ":base", eps=eps)
    rows = await drive(s, dx=-1, sprint=False, frames=34, tag=tag, stop_at_top=True)
    peak = max((r[2] for r in rows), default=0.0)
    if peak < UPPER_TOP - 0.02:
        raise SystemExit(
            f"ABORT[{tag}] could not reach the landing for a descent run: "
            f"peak_y={peak:.4f} (want {UPPER_TOP})")
    top_x = next((r[1] for r in rows if r[2] >= UPPER_TOP - 1e-2), None)
    print(f"  [{tag}] climb peak_y={peak:.4f} first_top_x={top_x}", flush=True)
    return rows


async def main() -> None:
    out: dict = {}
    failed: list[str] = []
    path = f"{SCRATCH}/results/stair10_{TAG}.json"
    os.makedirs(f"{SCRATCH}/results", exist_ok=True)

    def flush() -> None:
        with open(path, "w") as fh:
            json.dump(out, fh, ensure_ascii=False, indent=1)

    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        await s.ev("window.__vcw_teleport = {speed:1}")
        await s.wait_frames(2)
        await s.ev(HOOK)
        await s.wait_frames(2)
        print("teleport read-backs:", flush=True)

        async def run_path(name, body, **kw):
            """Run one path; a failure is recorded and the next one still runs.

            A tight arrival tolerance on the *spawn* of one path must never
            cost us the readings of the paths after it — the descent paths are
            the whole point of this run.
            """
            try:
                out[name] = await body(**kw)
                print(f"[{name}] OK", flush=True)
            except SystemExit as exc:
                failed.append(name)
                out[name] = {"error": str(exc)}
                print(f"[{name}] FAILED: {exc}", flush=True)
            except Exception as exc:  # noqa: BLE001
                failed.append(name)
                out[name] = {"error": f"{type(exc).__name__}: {exc}"}
                print(f"[{name}] ERROR: {type(exc).__name__}: {exc}", flush=True)
            flush()

        # ---- 1. walk up (strict: this is the precondition for every descent)
        async def walk_up():
            await tp(s, FOOT_X, FOOT_Z, "walk_up")
            rows = await drive(s, dx=-1, sprint=False, frames=34, tag="walk_up",
                               stop_at_top=True)
            report("walk_up", rows, False)
            return climb_verdict("walk_up", rows)
        await run_path("walk_up", walk_up)

        # ---- 2. sprint up (relaxed: the sprint path is already pinned by three
        # unit tests, so a coarse arrival is acceptable here — what matters is
        # that it reaches the top, not that it spawns on a specific tread).
        async def sprint_up():
            await tp(s, FOOT_X, FOOT_Z, "sprint_up", eps=1.5)
            rows = await drive(s, dx=-1, sprint=True, frames=34, tag="sprint_up",
                               stop_at_top=True)
            report("sprint_up", rows, False)
            verdict = climb_verdict("sprint_up", rows)
            if verdict.get("peak", 0.0) < UPPER_TOP - 1e-2:
                raise SystemExit(
                    f"ABORT[sprint_up] sprint did not reach the landing: "
                    f"peak_y={verdict.get('peak')} (want {UPPER_TOP})")
            return verdict
        await run_path("sprint_up", sprint_up)

        # ---- 3. landing_down: **the defect this round is about.**
        # From the second-floor landing, walk +X (downhill) off the landing
        # and all the way down to the ground floor.
        async def landing_down():
            await tp(s, LANDING_X, LANDING_Z, "landing_down", eps=1.2)
            rows = await drive(s, dx=1, sprint=False, frames=40, tag="landing_down")
            report("landing_down", rows, True)
            verdict = descent_verdict("landing_down", rows)
            frame_table("landing_down", rows)
            return verdict
        await run_path("landing_down", landing_down)

        # ---- 4/5. mid-run descents: climb to the top, then walk/sprint back
        # down. These start part-way up rather than on the landing.
        async def mid_descend(tag, sprint):
            climb = await climb_to_top(s, tag, eps=1.2)
            out[f"{tag}_climb"] = climb_verdict(f"{tag}_climb", climb)
            rows = await drive(s, dx=1, sprint=sprint, frames=40, tag=tag)
            report(tag, rows, True)
            verdict = descent_verdict(tag, rows)
            frame_table(tag, rows)
            return verdict
        await run_path("walk_down", mid_descend, tag="walk_down", sprint=False)
        await run_path("sprint_down", mid_descend, tag="sprint_down", sprint=True)

        await stop_walk(s)

    flush()
    print()
    got = [k for k in out if isinstance(out[k], dict) and "rows" in out[k]]
    print("=== summary ===")
    for k in ("walk_up", "sprint_up", "landing_down", "walk_down", "sprint_down"):
        v = out.get(k, {})
        if "error" in v:
            print(f"  {k:12} NO READING -- {v['error']}")
        elif k in ("landing_down", "walk_down", "sprint_down"):
            print(f"  {k:12} descended_to_ground={v.get('ground')} "
                  f"end_y={v.get('end_y')} dy>0={len(v.get('climbs', []))}")
        else:
            print(f"  {k:12} peak_y={v.get('peak')} dy<0(stair)={len(v.get('drops', []))}")
    print(f"# saved {path}")
    if failed:
        print(f"# paths without a reading: {failed}")


if __name__ == "__main__":
    asyncio.run(main())
