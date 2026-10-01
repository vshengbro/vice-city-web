"""验收分组 p4（由 tools/verify-parallel.sh 并行调度）。

只跑这一组需要的检查项。公共驱动在 sweep_lib.py。
"""

import asyncio
import json
import math
import os
import subprocess
import sys

import websockets

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sweep_lib import (  # noqa: E402
    CDP, URL, Driver, record, results, walk_until, walk_to, wait_frames,
    wait_teleport, wait_stream_settle,
)


async def main() -> None:
    # Always open a fresh tab. Reusing `pages[0]` picks up whatever the
    # previous run left behind, and a tab whose renderer was killed with
    # the browser accepts the WebSocket then immediately closes it --
    # which surfaces as ConnectionClosedError with no useful stack.
    subprocess.run(["curl", "-s", "--noproxy", "*", "-X", "PUT",
                    f"{CDP}/json/new?about:blank"],
                   capture_output=True, text=True, timeout=30)
    out = subprocess.run(["curl", "-s", "--noproxy", "*", f"{CDP}/json/list"],
                         capture_output=True, text=True, timeout=30).stdout
    pages = [t for t in json.loads(out) if t.get("type") == "page"]
    # Newest first: a reused profile leaves stale tabs behind, and the one
    # this run just opened is the only one with a live renderer.
    pages.sort(key=lambda t: t.get("id", ""), reverse=True)
    async with websockets.connect(pages[0]["webSocketDebuggerUrl"],
                                  max_size=128 * 1024 * 1024) as ws:
        d = Driver(ws)
        await d.cmd("Runtime.enable")
        await d.cmd("Page.enable")
        # python3 -m http.server sends Last-Modified, and Chrome's
        # heuristic caching happily reuses yesterday's .wasm. Bypass the
        # cache outright: a stale WASM makes every Rust-side fix look
        # like it had no effect, which is exactly the wrong conclusion.
        await d.cmd("Network.enable")
        await d.cmd("Network.setCacheDisabled", {"cacheDisabled": True})
        # Headless SwiftShader is slow enough that a stale target can be
        # attached without noticing; focus emulation keeps the renderer
        # from parking the page in a low-frequency state.
        await d.cmd("Emulation.setFocusEmulationEnabled", {"enabled": True})
        # Mouse events go to the canvas via a Rust listener; the page must
        # be frontmost or Chrome routes them to another window.
        await d.cmd("Page.bringToFront")
        await d.cmd("Page.navigate", {"url": URL})

        booted = False
        for _ in range(90):
            await asyncio.sleep(1.0)
            n = await d.ev("typeof __vcw !== 'undefined' ? __vcw.carX.length : -1")
            if isinstance(n, int) and n > 0:
                booted = True
                break
        if not booted:
            record("boot", False, "page never exposed __vcw")
            return
        await asyncio.sleep(2.0)

# ---- 7. showcase ---------------------------------------------
        # The vertical system (stair rise, slab support, step-down) is the
        # thing under test, and driving it through synthetic input is not
        # viable: WASD is camera-relative, and neither CDP mouse events nor
        # synthetic MouseEvents reach the canvas listeners in this headless
        # build. So the route Rust already uses for its end-to-end stair
        # test is published as world waypoints and stepped one at a time.
        # Every metre between waypoints still goes through the real
        # Player::step + step_vertical integration.
        await d.release()
        await d.ev("(() => { window.__vcw_teleport = {safe: true};"
                   " return true; })()")
        await d.ev("(() => { window.__vcw_teleport = {x: 33.5, z: 45}; return true; })()")
        await wait_teleport(d, 33.5, 45.0)
        for _ in range(4):
            await d.down("KeyF", "f")
            await asyncio.sleep(0.35)
            await d.up("KeyF", "f")
            await wait_frames(d, 2)
            if not (await d.st() or {}).get("veh"):
                break
        route = json.loads(await d.ev("JSON.stringify(__vcw.route)") or "[]")
        await d.ev("(() => { window.__vcw_teleport = {safe: true};"
                   " return true; })()")
        if not route:
            record("showcase.climb", False, "no waypoint route published")
        else:
            print(f"    route: {len(route)} waypoints", flush=True)
            trace: list = []
            for index, (tx, tz) in enumerate(route):
                st = await d.st() or {}
                here = (st.get("px") or 0.0, st.get("pz") or 0.0)
                if index == 0:
                    await d.ev(
                        f"(() => {{ window.__vcw_teleport = {{x: {tx},"
                        f" z: {tz}}}; return true; }})()")
                    await wait_teleport(d, tx, tz)
                    st = await wait_frames(d, 4) or (await d.st())
                else:
                    # Walk the leg instead of teleporting: the stair rise is
                    # driven by the vertical integration seeing consecutive
                    # steps under the feet, so a jump straight onto the top
                    # landing just falls back to the ground.
                    #
                    # Do NOT sub-divide into a chain of intermediate points.
                    # The route is axis-aligned, but the player is never
                    # exactly on a waypoint, so linear interpolation turns
                    # every leg into a diagonal -- and a diagonal inside this
                    # building always jams against the facade and shoves the
                    # walker back onto the street. The stair still works: each
                    # walk_to issues many short walk requests, so the feet
                    # still see consecutive steps.
                    await walk_to(d, tx, tz, trace)
                    st = await d.st() or {}
                if st:
                    trace.append(st)
                    print(f"      wp{index} ({tx:.1f},{tz:.1f}) from "
                          f"({here[0]:.1f},{here[1]:.1f}) -> "
                          f"({st.get('px'):.1f},{st.get('pz'):.1f}) "
                          f"y={st.get('py'):.2f}", flush=True)
            best = max((t.get("py") or 0) for t in trace) if trace else 0.0
            record("showcase.climb", best > 2.5,
                   f"walked {len(route)} waypoints, best y={best:.2f} m "
                   f"(upper slab is 2.6-3.2 m)")
            await d.shot("sweep_showcase.png")



        # ---- summary --------------------------------------------------
        await d.release()
        print("\n=== summary ===")
        bad = 0
        for name, ok, note in results:
            if not ok:
                bad += 1
            print(f"{'ok  ' if ok else 'FAIL'}  {name:18} {note}")
        print(f"\n{len(results) - bad}/{len(results)} passed")

asyncio.run(main())

