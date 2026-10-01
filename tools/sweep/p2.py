"""验收分组 p2（由 tools/verify-parallel.sh 并行调度）。

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

# ---- 4/5. car + wheel spin -----------------------------------
        await d.release()
        boarded = False
        trace = []
        # 预算**按帧算**,不按墙钟算。原来写死 90 轮 × 0.22 s ≈ 20 s,
        # 但无头探针现在走真 GPU 栈(metal-3),页面只有几 fps,20 s 里
        # 玩家其实只挪了几米 —— 车明明已经 1.6 m 了,循环却先耗尽。
        # 于是 `car.board` 在换 flag 之后假性失败。
        # 改成:只要帧号还在推进就继续走,给个硬上限兜底。
        last_frame = None
        stuck = 0
        for _ in range(240):
            st = await d.st()
            veh = st.get("veh")
            if isinstance(veh, (int, float)) and veh >= 0:
                boarded = True
                break
            near = await d.ev(
                "(() => { let best=1e9,bx=0,bz=0;"
                " for (let i=0;i<__vcw.carX.length;i++){"
                "  const d=Math.hypot(__vcw.carX[i]-__vcw.playerX,"
                "                    __vcw.carZ[i]-__vcw.playerZ);"
                "  if(d<best){best=d;bx=__vcw.carX[i];bz=__vcw.carZ[i];}}"
                " return {d:best,x:bx,z:bz};})()")
            if not near or not st:
                break
            f = st.get("frames") or st.get("frame")
            if isinstance(f, int):
                # 连续 40 轮帧号不动 = 页面卡死,不是「走得慢」,收手。
                if last_frame is not None and f <= last_frame:
                    stuck += 1
                    if stuck > 40:
                        break
                else:
                    stuck = 0
                last_frame = f
            trace.append([round(st.get("px") or 0, 2), round(st.get("pz") or 0, 2),
                          round(near["d"], 2), f])
            if near["d"] < 2.2:
                await d.down("KeyF", "f")
                await d.up("KeyF", "f")
                await asyncio.sleep(0.5)
                continue
            for c, k in [("KeyW", "w"), ("KeyA", "a"), ("KeyS", "s"), ("KeyD", "d")]:
                await d.up(c, k)
            if near["z"] - st["pz"] < -0.6:
                await d.down("KeyW", "w")
            elif near["z"] - st["pz"] > 0.6:
                await d.down("KeyS", "s")
            if near["x"] - st["px"] < -0.6:
                await d.down("KeyA", "a")
            elif near["x"] - st["px"] > 0.6:
                await d.down("KeyD", "d")
            await asyncio.sleep(0.22)
        await d.release()
        record("car.board", boarded,
               "boarded" if boarded else
               f"never boarded; last 5 of {len(trace)}: {trace[-5:]}")

        speed: list = []
        wheels: list = []
        await d.down("KeyW", "w")
        for i in range(16):
            await asyncio.sleep(0.3)
            st = await d.st()
            if not st:
                continue
            speed.append(st.get("speed") or 0.0)
            wheels.append(st.get("wheel"))
            if (st.get("speed") or 0) > 8.0:
                break
        await d.up("KeyW", "w")
        record("car.drive", bool(speed) and max(speed) > 3.0,
               f"peak speed {max(speed) if speed else 0:.1f} m/s over {len(speed)} samples")
        turned = False
        for w in wheels:
            if isinstance(w, list) and len(w) >= 6:
                vec = [w[4], w[5]]
                if abs(vec[0]) > 0.25 or vec[1] < 0.7:
                    turned = True
                    break
        sample = next((w for w in wheels if isinstance(w, list) and len(w) >= 6), None)
        record("wheel.spin", turned,
               f"Y basis {sample[4:6] if sample else None} (parked is [0,1])")


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

