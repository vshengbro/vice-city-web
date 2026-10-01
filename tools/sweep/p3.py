"""验收分组 p3（由 tools/verify-parallel.sh 并行调度）。

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

# ---- 6. infinite world ---------------------------------------
        await d.release()
        # Leave the car first. While driving, set_position moves the CAR and
        # the player stays glued to the seat, so every teleport below was
        # silently ignored and the stream centre never left spawn.
        await d.ev("(() => { window.__vcw_teleport = {x: 33.5, z: 45}; return true; })()")
        await wait_teleport(d, 33.5, 45.0)
        await d.down("KeyF", "f")
        await asyncio.sleep(0.4)
        await d.up("KeyF", "f")
        await wait_frames(d, 2)
        st = await d.st() or {}
        if st.get("veh"):
            print(f"    still driving: {st.get('veh')}", flush=True)
        # Teleport is the honest way to test the bound: walking is blocked
        # by building corners long before the interesting range.  The point
        # is what the game does AFTER the jump, not the journey.
        far: list = []
        for target in [(0.0, 0.0), (600.0, 0.0), (0.0, 900.0), (-1200.0, 600.0)]:
            ok = await d.ev(
                f"(() => {{ window.__vcw_teleport = {{x: {target[0]}, z: {target[1]}}};"
                f" return true; }})()")
            if not ok:
                continue
            st = await wait_teleport(d, target[0], target[1])
            st = await wait_stream_settle(d) or st
            if st:
                far.append((target, st))
        good = all(
            abs(s.get("py", -99)) < 5.0 and (s.get("tris") or 0) > 100_000
            for _, s in far)
        centres = {(s.get("streamX"), s.get("streamZ")) for _, s in far}
        record("world.infinite", bool(far) and good,
               f"{len(far)} teleports, y={[round(s.get('py', -99), 2) for _, s in far]}, "
               f"tris={[s.get('tris') for _, s in far]}")
        record("world.stream", len(centres) > 1,
               f"streamed centres visited: {sorted(centres)}")
        await d.ev("(() => { window.__vcw_teleport = {x: 33.5, z: 45}; return true; })()")
        await asyncio.sleep(1.0)


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

