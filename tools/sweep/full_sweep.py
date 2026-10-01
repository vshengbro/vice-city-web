"""完整验收：12 项一次跑完（串行，慢）。

并行版本见 tools/verify-parallel.sh，它把下面这些检查项切成 6 组，
每组一个独立 Chrome 实例。串行在这里只作为「一次性全跑」的参考实现。
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

# ---- 1. boot -------------------------------------------------
        s0 = await d.st()
        record("boot", bool(s0 and not s0.get("loadErr") and s0["carN"] > 0),
               f"assets={s0.get('loaded')} cars={s0.get('carN')} "
               f"tris={s0.get('tris')} loadErr={s0.get('loadErr')!r}")

# ---- 2/3. walk + collision -----------------------------------
        before = await d.st()
        walk: list = []
        await walk_until(d, "w", "KeyW", lambda s: s["frames"] > (before["frames"] or 0) + 45,
                         24, walk)
        after = await d.st()
        moved: float = math.dist([before["px"], before["pz"]],
                                 [after["px"], after["pz"]])
        record("walk", moved > 1.0,
               f"moved {moved:.1f} m in ~14 s "
               f"({before['px']:.1f},{before['pz']:.1f}) -> "
               f"({after['px']:.1f},{after['pz']:.1f})")
        record("collision", after.get("inSolid") is False,
               f"playerInsideCollider={after.get('inSolid')} "
               f"shapes={after.get('shapes')}")

# ---- 4/5. car + wheel spin -----------------------------------
        await d.release()
        boarded = False
        for _ in range(90):
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
        record("car.board", boarded, "boarded" if boarded else "never boarded")

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

# ---- 10. HUD liveness ----------------------------------------
        h1 = await d.hud()
        await d.release()
        await d.down("KeyW", "w")
        await asyncio.sleep(2.5)
        await d.up("KeyW", "w")
        h2 = await d.hud()
        record("hud.live", bool(h1 and h2 and h1.get("raw") != h2.get("raw")),
               "HUD text changed after moving" if h1 and h2
               and h1.get("raw") != h2.get("raw") else "HUD text is static")
        await d.shot("sweep_final.png")

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
