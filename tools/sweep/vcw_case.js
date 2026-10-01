
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
__mk({px:0, pz:0});
(async () => {
  // The marker below is substituted with a multi-line IIFE. It must not
  // appear anywhere else in this template -- not even in a comment, or the
  // substitution splits a `//` line and the injected code is parsed as
  // real statements. Substitution is a plain replace, not %-formatting:
  // the JS body is full of literal `%` from `toFixed`.
  const out = await ((async () => {
  const dx=1.0, dz=0.0, budget=8;
  const stopAfter=0.12;
  const take = () => ({px:__vcw.playerX, pz:__vcw.playerZ, py:__vcw.playerY,
    frames:__vcw.frames, respawn:__vcw.respawn, safe:__vcw.safe,
    hp:__vcw.combat.hp, walkReq:__vcw.walkReq, vel:__vcw.vel,
    inSolid:__vcw.playerInsideCollider});
  const start=take();
  const trace=[start];
  // Same frame-advance guard as `walk_frames`: a single awaited rAF does
  // not reliably show a new frame on this build, and a slide hop that
  // bails after one frame never slides at all.
  const nextFrame = async (patience) => {
    for (let k = 0; k <= patience; k++) {
      const before = __vcw.frames;
      await new Promise(r => requestAnimationFrame(() => r()));
      if (__vcw.frames !== before) return true;
    }
    return false;
  };
  for (let i=0; i<budget; i++) {
    const p=trace[trace.length-1];
    if (Math.hypot(p.px-start.px, p.pz-start.pz) >= stopAfter)
      { p.done='hop'; break; }
    window.__vcw_teleport = {walk:true, safe:true, dx:dx, dz:dz};
    if (!await nextFrame(3)) {
      const dead=take(); dead.stalled=true; trace.push(dead); break;
    }
    trace.push(take());
  }
  return {trace:trace, err:null};
})());
  console.log(JSON.stringify(out.trace.map(t => [
    +t.px.toFixed(3), +t.pz.toFixed(3), t.frames, t.done || null,
    t.stalled ? 1 : 0])));
})();
