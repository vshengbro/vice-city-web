"""Offline check of the page-side walk sampler.

`walk_frames` embeds JS that decides *when to stop walking*. A mistake in
it is invisible in a browser run -- the leg just fails later or not at all,
and the real log gives no hint which side is wrong. So replay the exact
generated source against a stub `__vcw` in node, on scenarios the real
acceptance hit: a leg that is short, one that overshoots, and one that is
wedged against a wall.
"""
import os
import subprocess
import sys

os.environ["NO_PROXY"] = "localhost,127.0.0.1"
HERE = os.path.dirname(os.path.abspath(__file__))
# `Driver` lives in sweep_lib.py; full_sweep.py only imports it. Executing
# full_sweep.py's header to reach `Driver` therefore needs a working
# `__file__` for its own `sys.path.insert`, and would re-import the driver
# anyway. Read sweep_lib.py directly -- it is the module that owns the
# page-side samplers under test.
text = open(os.path.join(HERE, "sweep_lib.py")).read()
ns: dict = {"__name__": "sweep_lib", "__file__": os.path.join(HERE,
                                                              "sweep_lib.py")}
exec(compile(text.split("async def main")[0], "sweep_lib.py", "exec"), ns)


class FakeDriver:
    """Captures the JS `walk_frames` would send, without a websocket."""

    def __init__(self):
        self.js = None

    async def ev_json(self, expr):
        self.js = expr
        return None

    walk_frames = ns["Driver"].walk_frames
    push_frames = ns["Driver"].push_frames


STUB = r"""
// A 60 Hz sim: one rAF == one 1/60 s step at 4.6 m/s == 0.0767 m.
//
// Two behaviours the real build has, both measured with probe_raf.py:
//   * `frozen`  -- a hung tab. Nothing advances, including `frames`.
//   * `staleEvery` -- the sampler is queued AHEAD of the renderer callback
//     that writes `frames`, so on every Nth vsync the sampler reads the
//     PREVIOUS counter value while the player has actually moved. That
//     stale read is the reason a single awaited rAF cannot be trusted as
//     "a frame happened", and it is what the page-side guard works around.
let S = null;

const advance = () => {
  if (S.frozen) return;          // hung tab: no counter, no motion
  S.frames++;
  S.ticks++;
  const r = window.__vcw_teleport;
  if (!r || !r.walk) return;
  const m = Math.hypot(r.dx, r.dz) || 1;
  const nx = S.px + r.dx / m * S.step, nz = S.pz + r.dz / m * S.step;
  if (S.blockedAt !== null &&
      Math.hypot(nx - S.blockedAt[0], nz - S.blockedAt[1]) < 0.4) return;
  S.px = nx; S.pz = nz;
};

globalThis.__mk = (opts) => {
  S = Object.assign({px: 0, pz: 0, py: 0, frames: 0, step: 0.0767,
                     blockedAt: null, frozen: false, staleEvery: 0}, opts);
  S.ticks = 0;
  S.rafs = 0;
  globalThis.__vcw = {
    get playerX(){return S.px}, get playerZ(){return S.pz},
    get playerY(){return S.py},
    // What the page actually reads: sometimes the previous value.
    get frames(){
      if (S.staleEvery && (S.ticks % S.staleEvery) === 1) return S.frames - 1;
      return S.frames;
    },
    get respawn(){return 0}, get safe(){return true},
    get combat(){return {hp: 100}},
    get walkReq(){return [0, 0]}, get vel(){return [0, 0]},
    get playerInsideCollider(){return false}};
  globalThis.window = globalThis;
  globalThis.requestAnimationFrame = (cb) => {
    advance();
    S.rafs++;
    return setTimeout(() => cb(performance.now()), 0);
  };
};
"""

HARNESS = r"""
(async () => {
  // The marker below is substituted with a multi-line IIFE. It must not
  // appear anywhere else in this template -- not even in a comment, or the
  // substitution splits a `//` line and the injected code is parsed as
  // real statements. Substitution is a plain replace, not %-formatting:
  // the JS body is full of literal `%` from `toFixed`.
  const out = await (@@VCWJS@@);
  console.log(JSON.stringify(out.trace.map(t => [
    +t.px.toFixed(3), +t.pz.toFixed(3), t.frames, t.done || null,
    t.stalled ? 1 : 0])));
})();
"""


def run(js: str, setup: str) -> list:
    case = os.path.join(HERE, "vcw_case.js")
    with open(case, "w") as fh:
        fh.write(STUB + setup + HARNESS.replace("@@VCWJS@@", js))
    out = subprocess.run(["node", case], capture_output=True,
                         text=True, timeout=60)
    if out.returncode != 0:
        raise SystemExit(f"node failed: {out.stderr[-2000:]}")
    import json
    return json.loads(out.stdout.strip().splitlines()[-1])


import asyncio  # noqa: E402

fails = []


def check(name, cond, detail=""):
    print(f"  {'ok  ' if cond else 'FAIL'} {name} {detail}")
    if not cond:
        fails.append(name)


async def main():
    fd = FakeDriver()

    # 1. short leg: 1.2 m, must stop on arrival, not run the budget out.
    await fd.walk_frames(1.2, 0.0, 1.0, 20, arrive=0.35)
    tr = run(fd.js, "__mk({px:0, pz:0});")
    last = tr[-1]
    check("short leg arrives", any(t[3] == "arrive" for t in tr),
          f"frames={len(tr)} last={last}")
    check("short leg stops near target",
          abs(last[0] - 1.2) < 0.4, f"x={last[0]}")
    # The regression this whole guard exists for: with one-in-three rAF
    # callbacks dropped, a single-rAF wait bails after ONE sample and the
    # leg crawls a fraction of a metre.
    check("short leg survives dropped rAF callbacks", len(tr) >= 8,
          f"samples={len(tr)} (a bare rAF wait gives 2-3)")

    # 1b. The real regression: a stale counter read on every 2nd vsync.
    #     A single-rAF wait sees "no change" and bails after one sample.
    await fd.walk_frames(3.0, 0.0, 1.0, 20, arrive=0.35)
    tr = run(fd.js, "__mk({px:0, pz:0, staleEvery:2});")
    check("stale counter: many samples", len(tr) >= 12,
          f"samples={len(tr)}")
    check("stale counter: covers the leg", tr[-1][0] > 0.5,
          f"x={tr[-1][0]}")
    check("stale counter: not a false stall", not any(t[4] for t in tr),
          f"stalled={sum(t[4] for t in tr)}")

    # 2. long leg: 10 m, must use most of the budget and keep going.
    await fd.walk_frames(10.0, 0.0, 1.0, 20, arrive=0.35)
    tr = run(fd.js, "__mk({px:0, pz:0});")
    check("long leg advances", tr[-1][0] > 1.0, f"x={tr[-1][0]}")
    check("long leg honours budget", len(tr) <= 21, f"samples={len(tr)}")
    check("long leg per-frame monotone",
          all(tr[i + 1][0] > tr[i][0] for i in range(len(tr) - 1)))

    # 3. A wall: the position freezes but FRAMES KEEP ADVANCING. That is
    #    the case the Python stall counter exists for (0.30 m per stride),
    #    so the page must report the frozen position and must NOT claim
    #    `stalled` -- the two are different faults and conflating them would
    #    make a wedged player look like a hung tab.
    await fd.walk_frames(10.0, 0.0, 1.0, 20, arrive=0.35)
    tr = run(fd.js, "__mk({px:0, pz:0, blockedAt:[0,0]});")
    check("wall: position frozen", tr[-1][0] == 0.0, f"x={tr[-1][0]}")
    check("wall: frames still advance", tr[-1][2] > 0, f"f={tr[-1][2]}")
    check("wall: not reported as page-stall", not any(t[4] for t in tr),
          f"n={len(tr)}")
    # And the Python stall predicate must actually fire on this trace,
    # otherwise a wedged leg would spin the whole budget silently.
    dists = [((tr[i + 1][0] - tr[i][0]) ** 2 +
              (tr[i + 1][1] - tr[i][1]) ** 2) ** 0.5 for i in range(len(tr) - 1)]
    check("wall: python stall counter would fire",
          sum(1 for dd in dists if dd < 0.30) >= 3,
          f"below-threshold strides={sum(1 for dd in dists if dd < 0.30)}")

    # 3b. A hung page: frames do not advance at all. Here the page-side
    #     guard is what must fire, or the walker waits out the full budget.
    await fd.walk_frames(10.0, 0.0, 1.0, 20, arrive=0.35)
    tr = run(fd.js, "__mk({px:0, pz:0, frozen:true});")
    check("frozen page: reported stalled", any(t[4] for t in tr),
          f"n={len(tr)}")
    check("frozen page: bails early", len(tr) < 21, f"samples={len(tr)}")

    # 4. push_frames holds a FIXED heading (no re-aim) and stops at the hop.
    await fd.push_frames(1.0, 0.0, 8, 0.12)
    tr = run(fd.js, "__mk({px:0, pz:0});")
    check("push stops at hop length",
          any(t[3] == "hop" for t in tr), f"last={tr[-1]}")
    check("push stays on heading", all(abs(t[1]) < 1e-9 for t in tr))

    # 5. ev_json must survive a throwing expression.
    class Boom(FakeDriver):
        async def ev(self, expr):
            return '"__vcw is not defined"'
    got = await Boom().ev_json("__vcw.nope()")
    check("ev_json returns None on page error", got is None, f"got={got!r}")

    print("\n" + ("ALL PAGE-SIDE CHECKS PASS" if not fails
                  else f"FAILURES: {fails}"))
    return 1 if fails else 0


sys.exit(asyncio.run(main()))
