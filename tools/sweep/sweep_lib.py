"""Full functional sweep of vice-city-web in the real browser.

One pass, many probes, machine-readable verdicts at the end.  Every check
prints PASS/FAIL with the measured number, so a failure is a fact rather
than an impression.  Nothing here trusts the Rust test suite -- these are
runtime observations against the shipped WASM.

Checks, in order:
  1. boot            scene loads, assets counted, no load error
  2. walk            W moves the player, direction matches camera heading
  3. collision       player never ends a frame inside a collider
  4. car             a car can be boarded, and it moves when the throttle is on
  5. wheel spin      the wheel basis vector rotates while driving
  6. infinite world  walking past 150 m keeps ground under the player and
                     moves the streamed-surface centre
  7. showcase        walking into the showcase door puts the player inside,
                     and the upper storey is reachable
  8. combat          firing consumes ammo
  9. pickups         walking over a pickup raises the counter
 10. hud            HUD text tracks the live state instead of a frozen snapshot
"""
import asyncio
import base64
import json
import math
import os
import subprocess

os.environ["NO_PROXY"] = "localhost,127.0.0.1"
os.environ["no_proxy"] = "localhost,127.0.0.1"

import websockets  # noqa: E402

import os
CDP = os.environ.get("VCW_CDP", "http://127.0.0.1:9223")
URL = "http://localhost:8765/index.html?fresh=fullsweep"
OUT = "/Users/sqs/.hermes/cache/scratch/vcw-cdp"

results: list[tuple[str, bool, str]] = []
held: set[str] = set()


def record(name: str, ok: bool, note: str) -> None:
    results.append((name, ok, note))
    print(f"  [{'PASS' if ok else 'FAIL'}] {name}: {note}")


class Driver:
    def __init__(self, ws):
        self.ws = ws
        self.mid = 0

    async def cmd(self, method: str, params: dict | None = None) -> dict:
        self.mid += 1
        await self.ws.send(json.dumps(
            {"id": self.mid, "method": method, "params": params or {}}))
        while True:
            msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=300))
            if msg.get("id") == self.mid:
                return msg.get("result", {})

    async def ev(self, expr: str):
        r = await self.cmd("Runtime.evaluate", {
            "expression": expr, "returnByValue": True, "awaitPromise": True})
        res = r.get("result", {})
        return res.get("value") if "value" in res else res.get("description")

    async def ev_json(self, expr: str):
        """Evaluate and JSON-decode, or None if the page threw.

        A thrown expression comes back as `{"result": {"description": ...}}`,
        so `ev` hands back a *string* on error. `walk_to` then does
        `st.get("px")` and dies with
        `AttributeError: 'str' object has no attribute 'get'` -- an error
        page presents as a bug in the walker, which is a miserable hour to
        spend. `ev_json` is the boundary that keeps the two apart.

        The wrapper is ASYNC and `await`s the expression. That is not
        decoration: a plain `(() => { try { return EXPR } catch ... })()`
        around a promise-returning `EXPR` returns the *promise*, and
        `JSON.stringify` of a promise is `undefined` -- so the whole call
        silently yielded None and every leg fell straight through to the
        wall slide. The `await` is inside the try, so a rejected promise
        is reported the same way a synchronous throw is.

        The rejection has to be unconditional, not just for `__vcwErr`. A
        page that legitimately evaluates to an array or a scalar, or a
        CDP-level `description` string, used to sail straight through and
        crash the caller one frame later. Every consumer of this boundary
        does `.get()` on the result, so the contract belongs here, once.
        """
        raw = await self.ev("(async () => { try {"
                            f" return JSON.stringify(await ({expr}));"
                            " } catch (e) {"
                            " return JSON.stringify({__vcwErr: String(e)});"
                            "}})()")
        if not isinstance(raw, str):
            return None
        try:
            out = json.loads(raw)
        except json.JSONDecodeError:
            return None
        if isinstance(out, dict) and "__vcwErr" in out:
            print(f"      [page error] {out['__vcwErr']}", flush=True)
            return None
        if not isinstance(out, dict):
            return None
        return out

    async def walk_frames(self, tx: float, tz: float, step: float,
                          budget: int, arrive: float = 0.35) -> dict | None:
        """Walk toward (tx, tz) for up to `budget` frames, sampled per frame.

        One `Runtime.evaluate` for the whole chunk. Measured on this box, a
        round trip costs ~670 ms because the renderer main thread is busy,
        and the old loop spent **five** of them per stride
        (`st` + `ev` + `settle`'s two + `st`). The sim only advanced 0.2-0.9 m
        per stride while the protocol cost more than the walking did.

        So the aim-and-step loop runs in the page, where it costs nothing,
        and comes back as one array of per-frame samples. What it does NOT
        do is decide anything: arrival, stall counting, the step schedule
        and the wall slide all still live in `walk_to`, and the arrival
        threshold is passed in rather than hard-coded so that predicate
        keeps exactly one owner. The page only re-aims and records, so the
        collision and slide behaviour is untouched.

        `walk` is persistent Rust state -- `apply_teleport_request` keeps
        walking in the last direction until the script clears it -- so
        re-writing the same vector every frame is a no-op. The step
        magnitude is a direction hint only: `Player::step` uses it as a
        unit direction and takes the speed from `WALK_SPEED`. It is still
        sent unchanged so the wire request stays byte-identical.
        """
        js = f"""(async () => {{
  const tx={tx!r}, tz={tz!r}, step={step!r};
  const budget={int(budget)}, arrive={arrive!r};
  const take = () => ({{px:__vcw.playerX, pz:__vcw.playerZ, py:__vcw.playerY,
    frames:__vcw.frames, respawn:__vcw.respawn, safe:__vcw.safe,
    hp:__vcw.combat.hp, walkReq:__vcw.walkReq, vel:__vcw.vel,
    inSolid:__vcw.playerInsideCollider}});
  const trace=[take()];
  // Wait for the game's own counter to actually tick, not just for one
  // rAF to fire. Measured on this build: after a single awaited rAF the
  // counter is unchanged in 2 of 6 samples, because the sampler is queued
  // ahead of the renderer callback that writes it. Treating that as a
  // dead loop bailed out after ONE frame and walked 0.19 m. So: keep
  // yielding until `frames` moves, and only call it stalled after
  // `patience` extra frames have gone by with no tick at all.
  const nextFrame = async (patience) => {{
    for (let k = 0; k <= patience; k++) {{
      const before = __vcw.frames;
      await new Promise(r => requestAnimationFrame(() => r()));
      if (__vcw.frames !== before) return true;
    }}
    return false;
  }};
  for (let i=0; i<budget; i++) {{
    const p=trace[trace.length-1];
    const dx=tx-p.px, dz=tz-p.pz, g=Math.hypot(dx,dz);
    if (!(g>1e-6)) {{ p.done='arrive'; break; }}
    // Same predicate `walk_to` uses, just evaluated in-page so the walk
    // stops on the frame it lands instead of one round trip later.
    if (g < arrive) {{ p.done='arrive'; break; }}
    window.__vcw_teleport = {{walk:true, safe:true,
      dx:dx/g*step, dz:dz/g*step}};
    if (!await nextFrame(3)) {{
      const dead=take(); dead.stalled=true; trace.push(dead); break;
    }}
    const now=take();
    now.gap=g;
    trace.push(now);
  }}
  return {{trace:trace, frames:__vcw.frames, err:null}};
}})()"""
        return await self.ev_json(js)

    async def push_frames(self, dx: float, dz: float, budget: int,
                          stop_after: float) -> dict | None:
        """Walk in a FIXED direction for up to `budget` frames.

        The wall slide's one distinguishing property is that it must *not*
        re-aim: it holds an oblique heading so the player rounds the corner,
        and a re-aim toward the goal would just push them back into the wall
        it is sliding along. So this is deliberately a different primitive
        from `walk_frames` -- constant heading, no target -- and the hop
        length is passed in from Python rather than decided here, exactly as
        the slide's `0.12 m` stopping test was before.
        """
        js = f"""(async () => {{
  const dx={dx!r}, dz={dz!r}, budget={int(budget)};
  const stopAfter={stop_after!r};
  const take = () => ({{px:__vcw.playerX, pz:__vcw.playerZ, py:__vcw.playerY,
    frames:__vcw.frames, respawn:__vcw.respawn, safe:__vcw.safe,
    hp:__vcw.combat.hp, walkReq:__vcw.walkReq, vel:__vcw.vel,
    inSolid:__vcw.playerInsideCollider}});
  const start=take();
  const trace=[start];
  // Same frame-advance guard as `walk_frames`: a single awaited rAF does
  // not reliably show a new frame on this build, and a slide hop that
  // bails after one frame never slides at all.
  const nextFrame = async (patience) => {{
    for (let k = 0; k <= patience; k++) {{
      const before = __vcw.frames;
      await new Promise(r => requestAnimationFrame(() => r()));
      if (__vcw.frames !== before) return true;
    }}
    return false;
  }};
  for (let i=0; i<budget; i++) {{
    const p=trace[trace.length-1];
    if (Math.hypot(p.px-start.px, p.pz-start.pz) >= stopAfter)
      {{ p.done='hop'; break; }}
    window.__vcw_teleport = {{walk:true, safe:true, dx:dx, dz:dz}};
    if (!await nextFrame(3)) {{
      const dead=take(); dead.stalled=true; trace.push(dead); break;
    }}
    trace.push(take());
  }}
  return {{trace:trace, err:null}};
}})()"""
        return await self.ev_json(js)

    async def down(self, code: str, key: str) -> None:
        if code in held:
            return
        held.add(code)
        await self.cmd("Input.dispatchKeyEvent", {
            "type": "rawKeyDown", "code": code, "key": key,
            "windowsVirtualKeyCode": 0, "nativeVirtualKeyCode": 0})

    async def up(self, code: str, key: str) -> None:
        if code not in held:
            return
        held.discard(code)
        await self.cmd("Input.dispatchKeyEvent", {
            "type": "keyUp", "code": code, "key": key,
            "windowsVirtualKeyCode": 0, "nativeVirtualKeyCode": 0})

    async def release(self) -> None:
        for code, key in [("KeyW", "w"), ("KeyA", "a"), ("KeyS", "s"),
                          ("KeyD", "d"), ("KeyF", "f"), ("Space", " ")]:
            await self.up(code, key)

    async def shot(self, name: str) -> None:
        r = await self.cmd("Page.captureScreenshot", {"format": "png"})
        data = r.get("data")
        if data:
            with open(f"{OUT}/{name}", "wb") as handle:
                handle.write(base64.b64decode(data))

    async def hud(self) -> dict:
        return await self.ev(
            "(() => { const h=document.getElementById('vcw-hud');"
            " if(!h) return null; const t=h.textContent;"
            " const g=(re,d)=>{const m=t.match(re);return m?m[1]:d;};"
            " return {"
            " x:+g(/xyz ([-0-9.]+)/,0), y:+g(/xyz [-0-9.]+ ([-0-9.]+)/,0),"
            " z:+g(/xyz [-0-9.]+ [-0-9.]+ ([-0-9.]+)/,0),"
            " hp:+g(/HP ([0-9.]+)/,0), cash:+g(/\\$([0-9]+)/,0),"
            " ammo:g(/([0-9]+) \\/ ([0-9]+)/,'0'),"
            " pickups:+g(/pickups ([0-9]+)/,0),"
            " mode:g(/phase [^·]*· ([^·]*)·/,''), raw:t };"
            "})()")

    async def st(self) -> dict | None:
        return await self.ev_json(
            "({px:__vcw.playerX, py:__vcw.playerY, pz:__vcw.playerZ,"
            " yaw:__vcw.cameraYaw, veh:__vcw.playerVehicle,"
            " speed:__vcw.carDriveSpeed, inSolid:__vcw.playerInsideCollider,"
            " frames:__vcw.frames, tris:__vcw.tris,"
            " streamX:__vcw.streamX, streamZ:__vcw.streamZ,"
            " wheel:__vcw.wheel, wheelBatches:__vcw.wheelBatches,"
            " carN:__vcw.carX.length, showOff:__vcw.showcaseOffset,"
            " loadErr:__vcw.loadError, loaded:__vcw.loadedAssets,"
            " shapes:__vcw.collisionShapes, kills:__vcw.kills,"
            " walkReq:__vcw.walkReq, vel:__vcw.vel, respawn:__vcw.respawn, safe:__vcw.safe, hp:__vcw.combat.hp,"
            " door:__vcw.door, route:__vcw.route,"
            " probe:__vcw.probe})")


async def walk_until(d: Driver, key: str, code: str, want, limit: int,
                     samples: list) -> bool:
    """Hold a key until `want(sample)` holds or `limit` steps elapse."""
    await d.down(code, key)
    try:
        for _ in range(limit):
            await asyncio.sleep(0.35)
            s = await d.st()
            if s:
                samples.append(s)
                if want(s):
                    return True
    finally:
        await d.up(code, key)
    return False


async def wait_teleport(d: "Driver", x: float, z: float, cap: int = 40) -> dict | None:
    """Poll until the player actually stands near (x, z).

    requestAnimationFrame is throttled in headless Chrome (~1 fps in this
    harness), and `apply_teleport_request` runs at the END of a frame, so a
    teleport requested now is only visible after the NEXT frame. Waiting a
    fixed number of seconds is therefore a race; waiting for frames is
    worse because `frames` is read after the teleport already landed.
    """
    for _ in range(cap):
        await asyncio.sleep(0.3)
        st = await d.st()
        if not isinstance(st, dict):
            continue
        gap = math.hypot((st.get("px") or 0) - x, (st.get("pz") or 0) - z)
        # Half a body width. The old 200 m cut-off was a stand-in for "has
        # the teleport had a chance to happen yet" back when the page ran
        # at 1 fps; at 60 Hz the next frame has already landed, so 200 m
        # made *every* wait succeed on the first poll -- the caller then
        # walked from the spawn point to a waypoint 15 m away and reported
        # the spawn as the result.
        if gap < 0.5:
            return st
    return await d.st()


async def settle(d: "Driver", polls: int = 4) -> dict | None:
    """Advance the simulation by a fixed number of render opportunities.

    Distance-based waits do not work under the accelerated acceptance rate:
    one rendered frame can advance seconds of simulated time, so
    "wait until N metres moved" is satisfied by the first frame and the rest
    of the leg never happens. A fixed poll count is the honest unit.

    The wait is expressed as *frames*, not `asyncio.sleep`. Both used to
    disagree badly: a frame costs ~0.25 s of wall clock under headless
    throttling, so `sleep(0.08)` between polls was both far too short
    (the walk request had not been read yet) and pure dead time when the
    renderer was busy. Counting `__vcw.frames` waits for the thing that
    actually matters -- the simulation moving -- and costs nothing extra,
    because the `d.st()` that samples the result is a round trip we make
    anyway.
    """
    start = (await d.st() or {}).get("frames") or 0
    # `polls * 4` can be zero for a `polls=0` call, which would leave `st`
    # unbound at the return below. Seed it, so the fallback is always a
    # real sample rather than a NameError.
    st = await d.st()
    for _ in range(polls * 4):
        await asyncio.sleep(0.05)
        st = await d.st()
        if isinstance(st, dict) and (st.get("frames") or 0) >= start + polls:
            return st
    return st


async def step_until(d: "Driver", want_move: float, cap: int = 40) -> dict | None:
    """Wait until the player has actually moved `want_move` metres.

    Wall-clock waits are useless here: headless SwiftShader holds the page
    at 1-5 fps and MAX_FRAME_TIME clamps each of those frames to 0.25 s, so
    the sim advances only ~0.4 s per wall-clock second. Everything that
    needs the player to cover ground has to poll the position instead of
    sleeping.

    `cap` bounds the *number of polls*, and each poll sleeps 0.08 s of wall
    clock, so the default allows ~3.2 s -- which at the accelerated rate is
    several seconds of simulated time and tens of metres of travel. A leg
    that reports "moved want_move" long before the cap is not early: the
    position really did change that much.
    """
    start = await d.st() or {}
    x0, z0 = start.get("px") or 0.0, start.get("pz") or 0.0
    for _ in range(cap):
        await asyncio.sleep(0.08)
        st = await d.st()
        if not isinstance(st, dict):
            continue
        if math.hypot((st.get("px") or 0) - x0,
                      (st.get("pz") or 0) - z0) >= want_move:
            return st
    return await d.st()


async def walk_to(d: "Driver", tx: float, tz: float, trace: list) -> dict | None:
    """Walk toward a world point through the game's own movement code.

    Sub-1.5 m targets: the doorway is 1.6 m wide, so a single long leg
    aimed at the far side of the building clips the facade and the player
    is pushed off the approach line.
    """
    # Go invulnerable first. Enemy gunfire drains health over the minutes a
    # waypoint walk takes at 6% speed, and a wipe respawns the player at the
    # nearest hospital -- which is what looked like "the walker got flung
    # back to spawn" in earlier runs.
    await d.ev("(() => { window.__vcw_teleport = {safe: true};"
               " return true; })()")
    # No time scaling. At the real 60 Hz this page now renders, one frame
    # is 1/60 s of simulation, and the earlier speed:4 (added when the page
    # ran at 1 fps) advanced 4 s per frame -- 18 m of travel against a
    # 0.2 m wall, which tunnelled the walker straight through the facade.
    st = await d.st() or {}
    if st.get("veh"):
        # Still driving: KeyW is the throttle, so a "walk" request moves the
        # car instead of the player and every waypoint silently fails.
        for _ in range(10):
            await d.down("KeyF", "f")
            await asyncio.sleep(0.6)
            await d.up("KeyF", "f")
            await asyncio.sleep(0.6)
            if not (await d.st() or {}).get("veh"):
                break
        print("    [walk_to] was still in a car, dismounted", flush=True)
    # WALK_SPEED is 4.6 m/s and a poll is ~0.25 s, so a full-length stride
    # overshoots by more than a metre. Ease the step down as the gap closes
    # so the last poll lands ON the waypoint instead of past it.
    # A blocked leg is detected by "the position did not change", not by
    # "the position is wrong": at 6% sim speed several polls cover less
    # than the 0.12 m arrival threshold, so a leg that is merely slow looks
    # identical to one that is stuck. Compare consecutive samples.
    #
    # The loop below walks the page-side trace frame by frame, but it *owns*
    # every decision: the same arrival test, the same stall counters, the
    # same per-stride print, the same step schedule. The page half only
    # re-aims and records. That split is what makes this safe to speed up --
    # a faster stride through the same predicate cannot change what counts
    # as arrived or stuck, it just reads the samples sooner.
    stall: int = 0
    last = (st.get("px") or 0.0, st.get("pz") or 0.0)
    # Recent positions, for a stride-length-independent stall test.
    # WALK_SPEED 4.6 m/s at FIXED_DT 1/60 s is 0.0767 m per frame, so the
    # window has to span MORE than the 0.30 m bar or a free-running walk
    # reads as stalled: 6 samples span 0.38 m, a 27% margin. A 4-sample
    # window spans only 0.23 m and false-trips on open floor -- measured,
    # not guessed (see test_stall_window.py).
    STALL_WINDOW = 6
    window: list = [(st.get("px") or 0.0, st.get("pz") or 0.0)]
    # Why the outer loop stopped, so the step-0.4 pass is skipped only when
    # the leg is genuinely done or genuinely stuck -- and never when a chunk
    # merely failed to come back.
    stopped = ""
    for step in (1.0, 0.4):
        # Each chunk is 20 frames of simulation -- 0.33 s, and about the
        # 1.5 m of ground a full-speed stride covers. Two chunks is the old
        # 40-iteration cap, now paid for in two round trips instead of 200.
        for _ in range(2):
            chunk = await d.walk_frames(tx, tz, step, 20, arrive=0.35)
            if not chunk or not chunk.get("trace"):
                # A failed chunk leaves `samples` unbound; the outer loop must
                # not look at it, or the leg dies with UnboundLocalError
                # instead of falling through to the wall slide.
                break
            samples = chunk["trace"]
            prev = samples[0]
            for st in samples[1:]:
                trace.append(st)
                px, pz = prev.get("px") or 0.0, prev.get("pz") or 0.0
                nx, nz = st.get("px") or 0.0, st.get("pz") or 0.0
                gap = math.hypot(tx - nx, tz - nz)
                # Crossed the target: gap alone is not enough when one stride
                # is longer than the leg, because the sampler never observes
                # the minimum. Treat "moved past it" as arrived.
                if gap < 0.35:
                    await d.ev("(() => { window.__vcw_teleport ="
                               " {walk: false, dx: 0, dz: 0};"
                               " return true; })()")
                    await asyncio.sleep(0.3)
                    return st
                print(f"      ({px:.2f},{pz:.2f}) -> ({nx:.2f},{nz:.2f})"
                      f" step={step} gap={gap:.2f}"
                      f" stall={stall} rsp={st.get('respawn')}"
                      f" safe={st.get('safe')} hp={st.get('hp')}", flush=True)
                #
                # The stall test spans STALL_WINDOW samples, not one. The
                # 0.30 m threshold was tuned against the old ~0.7 m stride;
                # sampling every frame instead (~0.08 m at walking speed) put
                # every single frame under it, so the counter saturated on
                # the first tick, the leg bailed immediately, and the wall
                # slide below never ran -- which is why the slide and STUCK
                # lines disappeared from the log entirely. Measuring over a
                # window of the same ~0.3 m keeps the predicate meaning what
                # it did.
                window.append((nx, nz))
                if len(window) > STALL_WINDOW:
                    window.pop(0)
                if len(window) < STALL_WINDOW:
                    # Window still filling: `window[0]` is the start of the
                    # walk, so the displacement so far is shorter than one
                    # stride and would read as a stall. A free walk would
                    # then log three phantom stalls and trip immediately.
                    prev = st
                    continue
                wx, wz = window[0]
                moved = math.hypot(nx - wx, nz - wz)
                # Moving but in the wrong direction means a wall is shoving us
                # away from the goal, not that we are stuck against it. Count
                # that as a stall too, or the loop keeps pushing into the wall.
                closing = ((nx - wx) * (tx - wx) + (nz - wz) * (tz - wz))
                if moved < 0.30 or closing < -0.45:
                    stall += 1
                    if stall >= 3:
                        # Break HERE, not after the loop. Checking only once
                        # the chunk is exhausted let `stall` run to 18 while
                        # the walker sat against a wall, and the extra samples
                        # then counted as continued progress -- which pushed
                        # the leg past the threshold the slide decision is
                        # made on.
                        stopped = "stall"
                        break
                else:
                    stall = 0
                last = (nx, nz)
                prev = st
            if stopped or samples[-1].get("done") == "arrive" \
                    or samples[-1].get("stalled"):
                # A stall verdict belongs to the whole leg: once one chunk
                # reports the player wedged, a second chunk at the same step
                # would only re-report it, and the step-0.4 pass exists to
                # try a gentler nudge. `stalled` here is the page's "the
                # frame loop did not advance" guard, not a wall.
                if samples[-1].get("stalled") or stall >= 3:
                    stopped = "stall"
                    break
                stopped = "arrive"
        if stall >= 2 or stopped:
            break
    if stall >= 3:
        # Blocked. Re-issue the request from where the player actually is
        # rather than declaring victory: the slide that follows routinely
        # throws the walker 6-9 m, and returning a position 8 m from the
        # target made the next leg start from the far side of the street.
        await d.ev("(() => { window.__vcw_teleport ="
                   " {walk: false, dx: 0, dz: 0};"
                   " return true; })()")
        await asyncio.sleep(0.4)
        settled = await d.st() or {}
        #
        # Do NOT return here. This early exit is what made the slide and
        # STUCK diagnostics vanish: a wall shove is usually >2.5 m, so the
        # condition held on every blocked leg and the walker bailed before
        # reaching the slide below. Settle the position, then fall through
        # and let the slide decide -- that is the path that can still get
        # the player round the corner.
        left_after_shove = math.hypot(tx - (settled.get("px") or 0.0),
                                      tz - (settled.get("pz") or 0.0))
        if left_after_shove < 0.5:
            return settled
    # Ran out of attempts inside the leg: report where the player actually
    # is so the caller can see the miss instead of inheriting a position
    # that was never the target.

    # Wall slide. A fan of fixed angles does not work: at 1.9 rad the
    # direction is 62 degrees off, which is enough to send the player past
    # the partition's end and into the far room. Slide along the wall
    # instead -- pure perpendicular, no forward component -- then re-aim.
    for attempt in range(3):
        settled = await d.st() or {}
        px, pz = settled.get("px") or 0.0, settled.get("pz") or 0.0
        trace.append(settled)
        left: float = math.hypot(tx - px, tz - pz)
        # Only a genuine arrival ends the leg. The old 1.2 m cut-off made a
        # leg that stalled 1.1 m short look successful, and the *next* leg
        # then started from that half-way point, walked into the wall it was
        # already inside of, and got thrown across the block -- which is
        # what every waypoint after the first was really reporting.
        if left < 0.5:
            return settled
        await d.ev("(() => { window.__vcw_teleport = {probe: true};"
                   " return true; })()")
        await asyncio.sleep(0.3)
        stuck_probe = await d.ev("String(window.__vcw_probe || '')") or ""
        if stuck_probe:
            print(f"      STUCK {stuck_probe}", flush=True)
        print(f"      [slide {attempt}] at ({px:.2f},{pz:.2f})"
              f" {left:.2f} m from ({tx:.2f},{tz:.2f})"
              f"  [walkReq={settled.get('walkReq')}"
              f" vel={settled.get('vel')}]", flush=True)
        gx, gz = tx - px, tz - pz
        gap = math.hypot(gx, gz) or 1.0
        gx, gz = gx / gap, gz / gap
        # Slide with a **forward component plus** one perpendicular sense.
        # Pure perpendicular was the wrong shape: it slides along the wall
        # the player is already pressed against and never rounds the corner
        # into the opening. Keeping 0.45 of the goal direction means each
        # nudge makes progress AND changes which face the player is on.
        for ox, oz in ((-gz, gx), (gz, -gx)):
            ox = ox * 0.9 + gx * 0.45
            oz = oz * 0.9 + gz * 0.45
            mag = math.hypot(ox, oz) or 1.0
            ox, oz = ox / mag, oz / mag
            # Re-issue the walk request every short hop. One long slide
            # overshoots: at 2x the sim moves ~2.7 m between polls, so a
            # single step_until(0.8) keeps going until something stops it.
            # The 8 hops are now one round trip that returns the whole
            # per-frame trace, so a slide that never moves is detected on
            # the frame it stops rather than after 8 protocol round trips.
            hop = await d.push_frames(ox, oz, 8, 0.12)
            if hop and hop.get("trace"):
                hsamples = hop["trace"]
                trace.extend(hsamples)
                for h in hsamples:
                    print(f"      [hop] ({h.get('px', 0):.2f},"
                          f"{h.get('pz', 0):.2f}) f={h.get('frames')}"
                          f" vel={h.get('vel')} safe={h.get('safe')}",
                          flush=True)
            else:
                for _ in range(8):
                    await d.ev(
                        f"(() => {{ window.__vcw_teleport = {{walk: true,"
                        f" safe: true, dx: {ox:.4f}, dz: {oz:.4f}}};"
                        f" return true; }})()")
                    await settle(d, polls=3)
                    cur = await d.st() or {}
                    if math.hypot((cur.get("px") or 0) - px,
                                  (cur.get("pz") or 0) - pz) < 0.12:
                        break
            await d.ev("(() => { window.__vcw_teleport ="
                       " {walk: false, dx: 0, dz: 0}; return true; })()")
            await asyncio.sleep(0.3)
            probe = await d.st() or {}
            trace.append(probe)
            nx, nz = probe.get("px") or 0.0, probe.get("pz") or 0.0
            print(f"      [slide] {ox:+.0f},{oz:+.0f} -> ({nx:.2f},{nz:.2f})"
                  f"  {math.hypot(tx - nx, tz - nz):.2f} m left", flush=True)
            if math.hypot(tx - nx, tz - nz) < 0.7:
                return probe
            # A slide that ends further from the target than it started has
            # pushed the walker out of the building, not around the corner:
            # 8 hops of 0.6 m in one direction put them 9-11 m down the
            # street every time. Give the hop back so the next attempt
            # starts from inside the room.
            if math.hypot(tx - nx, tz - nz) > math.hypot(tx - px, tz - pz) + 1.0:
                print(f"      [slide] gave back:"
                      f" {math.hypot(tx - px, tz - pz):.2f} ->"
                      f" {math.hypot(tx - nx, tz - nz):.2f}", flush=True)
                await d.ev(
                    f"(() => {{ window.__vcw_teleport = {{x: {px:.4f},"
                    f" z: {pz:.4f}}}; return true; }})()")
                await wait_teleport(d, px, pz)
    await d.ev("(() => { window.__vcw_teleport ="
               " {walk: false, dx: 0, dz: 0}; return true; })()")
    await wait_frames(d, 2)
    return settled


async def wait_stream_settle(d: "Driver", cap: int = 40) -> dict | None:
    """Poll until the streamed centre is a multiple of the street pitch.

    `streamed_center` is only written by `step_streamed_surface`, so a
    centre that is exactly on the 60 m grid proves a rebuild happened
    around the player. Sampling too early reads the pre-jump centre.
    """
    for _ in range(cap):
        await asyncio.sleep(0.3)
        st = await d.st()
        if not isinstance(st, dict):
            continue
        cx, cz = st.get("streamX") or 0.0, st.get("streamZ") or 0.0
        if abs(cx / 60.0 - round(cx / 60.0)) < 0.01 and cx != 0.0:
            return st
    return None


async def wait_frames(d: "Driver", n: int = 8, cap: int = 40) -> dict | None:
    """Wait for n simulation frames, not n seconds.

    The headless browser throttles requestAnimationFrame hard -- a real
    run advanced only ~1 frame/second -- so every time-based wait in this
    script was racing the game loop. Frame counts are deterministic.
    """
    start = (await d.st() or {}).get("frames") or 0
    for _ in range(cap):
        await asyncio.sleep(0.35)
        st = await d.st()
        if not isinstance(st, dict):
            continue
        if (st.get("frames") or 0) >= start + n:
            return st
    return await d.st()


