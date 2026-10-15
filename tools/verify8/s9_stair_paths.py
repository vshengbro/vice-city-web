"""Four-path stair measurement on the real wasm: walk/sprint x up/down.

Discipline (each line below is a trap this file already fell into once):

* **teleport must be read back before any path runs.** `apply_teleport_request`
  has several early-`return` branches (probe / speed / safe / walk). Any of them
  swallows the *whole* request object, so `{safe:true, x, z}` teleports
  **nowhere** and the player silently sits at SPAWN_POINT (33.5, 45) — every
  reading after that is on open ground with no stairs. `tp()` polls until the
  published position matches, prints dx_err / dz_err, and **raises** otherwise.
* **sample on `frames` change, not per rAF.** `publish_debug_state` runs every
  3rd frame (`frame_count % 3 == 0`), so a per-rAF sampler records the same
  snapshot three times and inflates nothing while hiding real motion.
  Dedupe on `frames`; report m/frame as dist / frame span.
* **teleport / walk requests are one-shot**, but `walk_request` persists after.
  Send walk once per path; stop with `{walk:true, dx:0, dz:0}`.
* **world-space walk** (`{walk:true, dx, dz}`) bypasses camera yaw entirely, so
  aiming is irrelevant: dx=-1 is exactly -X, which is uphill.
"""
from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import SCRATCH, Session  # noqa: E402

PORT = 9477
WORKER = "s9stair"
TAG = sys.argv[1] if len(sys.argv) > 1 else "before"

# LOFT (index 0) stair foot in world coords. Uphill is -X.
FOOT_X, FOOT_Z = 27.3, 24.9
TREAD = 0.45
N_TREADS = 10
RUN_LO = FOOT_X - N_TREADS * TREAD - 0.4      # a little below the top landing
STAIR_HALF_W = 0.65                            # SHOWCASE_STAIR_WIDTH 1.30 / 2
UPPER_TOP = 3.20
ARRIVE_EPS = 0.02                              # teleport read-back tolerance

# One row per *published* frame: [frames, x, y, z, vy, grounded].
#
# `frames` is `game.frame_count`, incremented every rAF (game.rs:8053) but the
# snapshot is only republished every 3rd frame (`is_multiple_of(3)`,
# game.rs:8128). So consecutive rows span 3 real frames, and a row's x/y is up
# to 2 frames stale. That is why per-frame stride has to be reported as
# dist / frame span, and why "did it stall" is judged on x movement rather
# than on row count.
HOOK = """
(() => {
  if (window.__s9) return 1;
  window.__s9 = {on: false, rows: [], last: -1, raf: 0};
  const grab = () => {
    window.__s9.raf += 1;
    const c = window.__vcw || {};
    const f = +c.frames || 0;
    if (window.__s9.on && f !== window.__s9.last) {
      window.__s9.last = f;
      window.__s9.rows.push([
        f, +c.playerX || 0, +c.playerY || 0, +c.playerZ || 0,
        +c.vy || 0, c.grounded ? 1 : 0, window.__s9.raf,
      ]);
    }
    requestAnimationFrame(grab);
  };
  requestAnimationFrame(grab);
  return 1;
})()
"""
SAMPLE = ("(() => { window.__s9.on = true; window.__s9.rows = [];"
          " window.__s9.last = -1; return 1; })()")
READ = "(() => JSON.stringify(window.__s9.rows))()"
OFF = "(() => { window.__s9.on = false; return 1; })()"


async def stop_walk(s):
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    await s.wait_frames(4)


async def tp(s, x, z, tag="", y=None):
    """Teleport and *prove* it landed. Raises on any mismatch.

    `y` is optional: `apply_teleport_request` preserves the current foot height
    (`set_position([x, here[1], z])`), so a bare x/z teleport can never place the
    player on the upper floor — the descent run must walk *up* first and then
    reverse, otherwise it starts on the ground floor and tests nothing.
    """
    await stop_walk(s)
    await s.ev("window.__vcw_teleport = null")
    await s.wait_frames(1)
    # safe mode as its own request: combined with x/z it trips the early
    # return at game.rs:4560 and the teleport is dropped entirely.
    await s.ev("window.__vcw_teleport = {safe:true}")
    await s.wait_frames(2)
    await s.ev(f"window.__vcw_teleport = {{x: {x}, z: {z}}}")
    got = None
    for _ in range(12):          # publish is 1-in-3 frames; allow for that
        await s.wait_frames(2)
        got = await s.ev(
            "(() => { const c = window.__vcw || {};"
            " return [+c.playerX, +c.playerZ, +c.playerY, +c.frames || 0]; })()"
        )
        if got and abs(got[0] - x) < ARRIVE_EPS and abs(got[1] - z) < ARRIVE_EPS:
            break
    if not got:
        raise SystemExit(f"ABORT[{tag}] teleport read-back returned nothing")
    dx_err, dz_err = got[0] - x, got[1] - z
    if abs(dx_err) >= ARRIVE_EPS or abs(dz_err) >= ARRIVE_EPS:
        raise SystemExit(
            f"ABORT[{tag}] teleport did not land: want ({x}, {z}) "
            f"got ({got[0]}, {got[1]}) dx_err={dx_err:+.4f} dz_err={dz_err:+.4f} "
            f"y={got[2]} — readings would be on open ground"
        )
    if y is not None and abs(got[2] - y) > 0.05:
        raise SystemExit(
            f"ABORT[{tag}] teleport height wrong: want y={y} got y={got[2]} "
            f"at ({got[0]}, {got[1]})"
        )
    print(f"  [{tag}] arrived=({got[0]:.2f},{got[1]:.2f}) y={got[2]:.3f} "
          f"dx_err={dx_err:+.4f} dz_err={dz_err:+.4f} frame={got[3]}", flush=True)
    return got


async def climb_to_top(s, tag, strict=True):
    """Walk up to the landing so a descent can start on the upper floor.

    The upper landing is reached a frame or two *before* the top of the run
    (peak_y hits 3.20 while x is still inside the run, then the walk continues
    across the floor and eventually steps off the far edge). So accept any peak
    within a tread of UPPER_TOP, and only insist on it when `strict`.
    """
    await tp(s, FOOT_X, FOOT_Z, tag + ":base")
    rows = await drive(s, dx=-1, sprint=False, frames=22, tag=tag,
                       stop_at_top=True)
    peak = max((r[2] for r in rows), default=0.0)
    ok = peak >= UPPER_TOP - 0.31
    if strict and not ok:
        raise SystemExit(
            f"ABORT[{tag}] could not reach the top for the descent run: "
            f"peak_y={peak:.4f} (want {UPPER_TOP})"
        )
    print(f"  [{tag}] climb peak_y={peak:.4f} ({'at top' if ok else 'BELOW TOP'})",
          flush=True)
    return rows


async def drive(s, *, dx, sprint, frames, tag, stop_at_top=False):
    """Hold a world-direction walk for `frames` published frames, sampling.

    **Start sampling before sending the walk request.** The walk request is
    consumed on the very next frame, so a 2-frame gap between the request and
    SAMPLE loses the first two frames — and on a 1.7 fps renderer that is the
    whole first tread.

    `stop_at_top` cuts the walk as soon as the player is standing on the upper
    landing. Without it the run continues across the floor and off the far edge
    (walk_up ends at x=18.3, y=0.15 — a fall, not the stair), which destroys
    the descent precondition.

    The wait happens **inside the page**, keyed on `window.__vcw.frames`.
    Polling from Python once per frame costs a CDP round trip per frame and made
    a 34-frame run take minutes; a single evaluate that returns when the
    counter advances is one round trip total.
    """
    await s.ev(SAMPLE)
    if sprint:
        await s.key("ShiftLeft", True)
        await s.wait_frames(2)
    await s.ev(f"window.__vcw_teleport = {{walk:true, dx:{dx}, dz:0}}")
    if stop_at_top:
        # A *resolved-on-rAF* promise still keeps the renderer main thread busy
        # between frames; on a 1.7 fps software renderer a 90-frame bail-out can
        # exceed the CDP evaluate timeout and kill the socket (seen:
        # ConnectionClosedError). Yield with setTimeout instead of a tight rAF
        # chain, and keep the budget small.
        await s.ev("""(() => {
            const f0 = +((window.__vcw||{}).frames||0);
            const tick = () => {
              const c = window.__vcw || {};
              if (+(c.frames||0) - f0 > 40) { window.__s9Done = 0; return; }
              if (+(c.playerY||0) >= 3.19) { window.__s9Done = 1; return; }
              setTimeout(tick, 120);
            };
            window.__s9Done = -1;
            tick();
            return 1;
          })()""")
        for _ in range(60):
            d = await s.ev("(() => window.__s9Done)()")
            if d != -1:
                break
            await asyncio.sleep(0.5)
    else:
        budget = frames
        await s.ev("""(() => {
            const f0 = +((window.__vcw||{}).frames||0);
            const want = BUDGET;
            const tick = () => {
              if (+((window.__vcw||{}).frames||0) - f0 >= want) {
                window.__s9Done = 1;
                return;
              }
              setTimeout(tick, 120);
            };
            window.__s9Done = -1;
            tick();
            return 1;
          })()""".replace("BUDGET", str(budget)))
        for _ in range(60):
            d = await s.ev("(() => window.__s9Done)()")
            if d != -1:
                break
            await asyncio.sleep(0.5)
    rows = json.loads(await s.ev(READ) or "[]")
    await s.ev(OFF)
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    if sprint:
        await s.key("ShiftLeft", False)
    await s.wait_frames(3)
    return rows


def on_stair(x, z):
    return RUN_LO <= x <= (FOOT_X + 0.4) and abs(z - FOOT_Z) <= STAIR_HALF_W + 0.25


def report(name, rows, up):
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
    # dy < 0 while over the stair run == fell through a tread
    drops = [(b[0], round(a[2], 3), round(b[2], 3))
             for a, b in zip(rows, rows[1:])
             if b[2] < a[2] - 1e-3 and on_stair(a[1], a[3])]
    span = rows[-1][0] - rows[0][0]
    got_top = any(abs(y - UPPER_TOP) < 1e-3 for y in ys)
    print(f"{name:12} n={len(rows):3d} fr={span:3d} "
          f"y {ys[0]:.3f}->{ys[-1]:.3f} peak={peak:.4f} "
          f"start=({rows[0][1]:.2f},{rows[0][3]:.2f}) "
          f"end=({rows[-1][1]:.2f},{rows[-1][3]:.2f}) "
          f"m/rAF={m:.4f} top={'yes' if got_top else 'NO'} "
          f"dy<0(stair)={len(drops)}")
    return {"peak": peak, "m": m, "start_x": rows[0][1], "start_y": ys[0],
            "end_x": rows[-1][1], "end_y": ys[-1], "top": got_top,
            "drops": drops, "n": len(rows), "span": span, "rows": rows}


async def main() -> None:
    out: dict = {}
    # Save after every path. The CDP socket has died mid-run twice (a 1.7 fps
    # software renderer starves the evaluate round trip), and a crash at the
    # end used to throw away every reading taken before it.
    path = f"{SCRATCH}/results/stair_{TAG}.json"
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

        await tp(s, FOOT_X, FOOT_Z, "walk_up")
        out["walk_up"] = report("walk_up", await drive(
            s, dx=-1, sprint=False, frames=22, tag="walk_up",
            stop_at_top=True), up=True)
        flush()

        await tp(s, FOOT_X, FOOT_Z, "sprint_up")
        out["sprint_up"] = report("sprint_up", await drive(
            s, dx=-1, sprint=True, frames=22, tag="sprint_up",
            stop_at_top=True), up=True)
        flush()

        # Descending: teleport preserves foot height, so the only way to start
        # on the upper floor is to walk up there first. That climb is also the
        # precondition check for the descent run.
        await climb_to_top(s, "walk_down", strict=False)
        out["walk_down"] = report("walk_down", await drive(
            s, dx=1, sprint=False, frames=22, tag="walk_down"), up=False)
        flush()

        await climb_to_top(s, "sprint_down", strict=False)
        out["sprint_down"] = report("sprint_down", await drive(
            s, dx=1, sprint=True, frames=22, tag="sprint_down"), up=False)
        flush()
        await stop_walk(s)

    flush()
    print(f"# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())
