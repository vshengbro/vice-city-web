"""Descent-only re-measure: from the upper landing, walk +X to the ground floor.

Why this exists: the combined run's `walk_down` stayed pinned at x=27.40 (the
bottom tread, top 0.455) for 30 frames while `landing_down` walked straight
through the same spot. Same geometry, different outcome, so the difference has
to be in how the walk was set up, not in the stairs.

**The pitfall that produced the difference:** the descent walk is issued right
after the ascent walk, and `walk_request` *persists*. `drive()` clears it with
`{walk:true, dx:0, dz:0}`, but `climb_to_top` -> `drive` returns having already
re-armed the walk for the ascent; the descent then re-sends `{walk:true,
dx:1, dz:0}` one frame later. On a ~1.7 fps renderer the player spends several
frames carrying the *ascent's* +Z/-X velocity while the new request is applied,
so it arrives at the bottom tread already pressed into the wall.

So this script **parks the player first**: stop the walk, clear it, wait for the
velocity to bleed off, confirm the player is stationary (two consecutive HUD
frames at the same x), and only then send the descent walk. It also re-teleports
onto the landing *between* the two legs so the descent cannot inherit anything
from the climb.

The ground floor of the showcase interior is top 0.150 and the bottom tread is
top 0.455, so the last step down is 0.305 m — legal, and the run must show y
settle at 0.15 (the HUD prints one decimal, so it reads "0.2").
"""
from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import SCRATCH, Session  # noqa: E402
import s10_stair_descend as S  # noqa: E402

PORT = 9491
WORKER = "s10d2"
TAG = sys.argv[1] if len(sys.argv) > 1 else "desc"

FOOT_X, FOOT_Z = S.FOOT_X, S.FOOT_Z
LANDING_X, LANDING_Z = S.LANDING_X, S.LANDING_Z
GROUND_TOP = S.GROUND_TOP


async def park(s, tag):
    """Stop the walk and wait until the player is genuinely stationary.

    Two consecutive HUD frames at the same x is the bar: a 1.7 fps renderer
    needs a couple of frames just to publish, and one matching pair can be a
    coincidence of a 0.6 m stride landing symmetrically.
    """
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    stable = 0
    last = None
    for _ in range(24):
        await s.wait_frames(2)
        got = await s.ev(S.TPREAD)
        if not got:
            continue
        if last is not None and abs(got[0] - last) < 1e-3:
            stable += 1
            if stable >= 2:
                print(f"  [{tag}] parked at x={got[0]:.2f} y={got[1]:.2f} frame={got[3]}",
                      flush=True)
                return got
        else:
            stable = 0
        last = got[0]
    print(f"  [{tag}] park: player never fully stopped, proceeding anyway", flush=True)
    return None


async def drive_sampled(s, *, dx, sprint, frames, tag):
    """Drive a walk and return only the frames recorded **during the walk**.

    `S.drive()` arms the global sampler, sends the walk, waits, then reads. That
    is correct for a single leg, but it means the sampler is live during any
    `wait_frames` that happens after it is armed — and on a ~1.7 fps renderer
    the park/teleport settling between the ascent and the descent is many
    seconds of wall clock. Those frames land in the rows and push the actual
    descent off the front of the table: the first recorded row already reads
    x=28.3, i.e. the player has left the building.

    So this arms the sampler and sends the walk back to back, with nothing in
    between, and the caller must do all its parking first.
    """
    await s.ev(S.SAMPLE)
    if sprint:
        await s.key("ShiftLeft", True)
        await s.wait_frames(2)
        # Shift alone must not be sampled either: re-arm after it is held.
        await s.ev(S.SAMPLE)
    await s.ev(f"window.__vcw_teleport = {{walk:true, dx:{dx}, dz:0}}")
    await S._wait_in_page(s, frames, None)
    rows = json.loads(await s.ev(S.READ) or "[]")
    await s.ev(S.OFF)
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    if sprint:
        await s.key("ShiftLeft", False)
    await s.wait_frames(3)
    return rows


async def main() -> None:
    out: dict = {}
    path = f"{SCRATCH}/results/stairdesc_{TAG}.json"
    os.makedirs(f"{SCRATCH}/results", exist_ok=True)

    with open(path, "w") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=1)

    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        await s.ev("window.__vcw_teleport = {speed:1}")
        await s.wait_frames(2)
        await s.ev(S.HOOK)
        await s.wait_frames(2)

        for name, sprint in (("landing_down_walk", False),
                             ("landing_down_sprint", True)):
            try:
                print(f"--- {name} ---", flush=True)
                # 1. walk up to the landing (this is also the ascent sanity check)
                await S.tp(s, FOOT_X, FOOT_Z, name + ":base", eps=1.2)
                climb = await S.drive(s, dx=-1, sprint=False, frames=34, tag=name,
                                      stop_at_top=True)
                cv = S.climb_verdict(name + "_climb", climb)
                print("   climb:", json.dumps(cv, ensure_ascii=False), flush=True)
                if cv.get("peak", 0.0) < S.UPPER_TOP - 1e-2:
                    raise SystemExit(
                        f"[{name}] climb only reached {cv.get('peak')}, cannot run descent")

                # 2. park FIRST, then re-place on the landing.
                #
                # Order matters and the obvious order is wrong. `drive()` arms
                # the per-frame sampler as its first action and only then sends
                # the walk request — so anything that waits for frames *after*
                # `drive()` has been entered (but before the request lands) is
                # sampled, and on a ~1.7 fps renderer several seconds of
                # wall-clock pass in between. Parking after arming the sampler
                # therefore burns the whole descent before recording starts:
                # the first row already reads x=28.3, i.e. the player has left
                # the building.
                await park(s, name)
                await S.tp(s, LANDING_X, LANDING_Z, name + ":landing", eps=1.2)
                await park(s, name + ":landing")

                # 3. the descent itself, from a stationary start on the landing.
                #    `drive_sampled` (not `S.drive`) so the rows cover the walk
                #    and nothing else.
                rows = await drive_sampled(s, dx=1, sprint=sprint, frames=50, tag=name)
                S.report(name, rows, True)
                v = S.descent_verdict(name, rows)
                S.frame_table(name, rows)
                out[name] = {"climb": cv, "descent": {k: v[k] for k in
                                                      ("ground", "end_y", "min_y", "climbs")},
                             "rows": rows}
                print(f"[{name}] OK", flush=True)
            except SystemExit as exc:
                out[name] = {"error": str(exc)}
                print(f"[{name}] FAILED: {exc}", flush=True)
            with open(path, "w") as fh:
                json.dump(out, fh, ensure_ascii=False, indent=1)

    print()
    print("=== summary ===")
    for k, v in out.items():
        if "error" in v:
            print(f"  {k:22} NO READING -- {v['error']}")
        else:
            d = v["descent"]
            print(f"  {k:22} descended_to_ground={d['ground']} end_y={d['end_y']} "
                  f"min_y={d['min_y']} dy>0={len(d['climbs'])}")
    print(f"# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())
