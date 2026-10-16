"""Descent from the upper landing — one continuous sampled run per speed.

Why this is not four separate legs: `walk_request` **persists** on the game
until a new request arrives, and the only way onto the upper floor is to walk
up there first. So a "climb, then teleport to the landing, then descend" script
inherits the climb's momentum — the descent gets sent while the player is still
decelerating from the ascent, and the readings come back describing a run that
never started where it was supposed to. Two legs in one session produced
"parked at x=27.7 y=0.2" *before the descent even began*: the player had
already walked the whole flight and out of the building.

So instead: **sample continuously for the whole session** (the sampler is a
global rAF loop, armed once), mark the boundaries by frame number, and cut the
rows into legs afterwards. Nothing is armed or disarmed between the climb and
the descent, so there is no window in which movement goes unrecorded.

`{walk:true, dx:0, dz:0}` does clear the velocity, and it is sent before the
landing teleport — but the teleport keeps the *current* foot height, so a
velocity that has already bled off leaves the player standing exactly on the
landing. That is the precondition this run needs.

The judgement then uses only the rows inside the descent window: y must fall
monotonically from 3.20 to the ground floor with no dy > 0.
"""
from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import SCRATCH, Session  # noqa: E402
import s10_stair_descend as S  # noqa: E402

PORT = 9493
WORKER = "s10d3"
TAG = sys.argv[1] if len(sys.argv) > 1 else "desc"

FOOT_X, FOOT_Z = S.FOOT_X, S.FOOT_Z
LANDING_X, LANDING_Z = S.LANDING_X, S.LANDING_Z


async def frame_now(s):
    got = await s.ev(S.TPREAD)
    return got[3] if got else 0


async def main() -> None:
    out: dict = {}
    path = f"{SCRATCH}/results/stairdesc3_{TAG}.json"
    os.makedirs(f"{SCRATCH}/results", exist_ok=True)

    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        await s.ev("window.__vcw_teleport = {speed:1}")
        await s.wait_frames(2)
        await s.ev(S.HOOK)
        await s.wait_frames(3)

        for name, sprint in (("landing_down_walk", False),
                             ("landing_down_sprint", True)):
            try:
                print(f"--- {name} ---", flush=True)
                await S.tp(s, FOOT_X, FOOT_Z, name + ":base", eps=1.2)
                # Climb, sampled. The whole session is under the sampler, so no
                # arm/disarm happens between the two legs.
                await s.ev(S.SAMPLE)
                if sprint:
                    await s.key("ShiftLeft", True)
                    await s.wait_frames(2)
                    await s.ev(S.SAMPLE)
                await s.ev("window.__vcw_teleport = {walk:true, dx:-1, dz:0}")
                await S._wait_in_page(
                    s, 34,
                    "if (+(c.playerY||0) >= 3.19) { window.__s10Done = 1; return; }")
                climb = json.loads(await s.ev(S.READ) or "[]")
                if sprint:
                    await s.key("ShiftLeft", False)
                cv = S.climb_verdict(name + "_climb", climb)
                print("   climb:", json.dumps(cv, ensure_ascii=False), flush=True)
                if cv.get("peak", 0.0) < S.UPPER_TOP - 1e-2:
                    raise SystemExit(f"[{name}] climb only reached {cv.get('peak')}")

                # Stop the walk and let the velocity bleed off **before** the
                # landing teleport, so the descent starts from rest.
                await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
                await s.wait_frames(4)
                await S.tp(s, LANDING_X, LANDING_Z, name + ":landing", eps=1.2)
                await s.wait_frames(2)
                here = await s.ev(S.TPREAD)
                if here and abs(here[1] - S.UPPER_TOP) > 0.2:
                    raise SystemExit(
                        f"[{name}] not on the landing after teleport: y={here[1]} "
                        f"(want {S.UPPER_TOP}) at x={here[0]}")
                await s.wait_frames(3)

                # Descent. The sampler has been live throughout; record the
                # boundary frame and cut the rows to everything after it.
                await s.ev(S.SAMPLE)
                if sprint:
                    await s.key("ShiftLeft", True)
                    await s.wait_frames(2)
                    await s.ev(S.SAMPLE)
                await s.ev("window.__vcw_teleport = {walk:true, dx:1, dz:0}")
                await S._wait_in_page(s, 26, None)
                rows = json.loads(await s.ev(S.READ) or "[]")
                if sprint:
                    await s.key("ShiftLeft", False)
                await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
                await s.wait_frames(3)

                S.report(name, rows, True)
                v = S.descent_verdict(name, rows)
                S.frame_table(name, rows)
                out[name] = {"climb": cv,
                             "descent": {k: v[k] for k in
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
