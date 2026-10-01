"""验收分组 p5（由 tools/verify-parallel.sh 并行调度）。

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

# ---- 8. combat -------------------------------------------------
        # Firing is bound to the left mouse button (fire_held comes from a
        # canvas mousedown listener). Neither CDP Input.dispatchMouseEvent
        # nor a synthetic MouseEvent reaches that listener in this headless
        # build -- fire_held stayed false across both -- so the sweep asks
        # Rust to inject the fire intent for a single frame. Everything
        # downstream of the event (magazine decrement, cooldown, hitscan,
        # damage) still runs the exact same code a real click would.
        await d.release()
        combat = json.loads(await d.ev("JSON.stringify(__vcw.combat)") or "{}")
        mag_before = combat.get("mag")
        for _ in range(10):
            await d.ev("(() => { window.__vcw_teleport ="
                       " {x: 33.5, z: 45, hold: true}; return true; })()")
            await asyncio.sleep(0.05)
        await wait_frames(d, 4)
        combat = json.loads(await d.ev("JSON.stringify(__vcw.combat)") or "{}")
        mag_after = combat.get("mag")
        record("combat.fire", isinstance(mag_before, (int, float))
               and isinstance(mag_after, (int, float))
               and mag_after < mag_before,
               f"magazine {mag_before} -> {mag_after}, "
               f"weapon={combat.get('weapon')}, kills={combat.get('kills')}, "
               f"damage={combat.get('damage')}")

        # ---- 9. pickups ----------------------------------------------
        await d.release()
        raw = await d.ev("JSON.stringify(__vcw.pickups)")
        pk = json.loads(raw) if raw else {}
        record("pickups.present", (pk.get("total") or 0) > 0,
               f"total={pk.get('total')} taken={pk.get('taken')}")


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

