"""相机在楼梯处被压到 0.71 的真凶:绕 yaw 扫描 + 在眼点位置探针。"""

from __future__ import annotations

import asyncio
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import Session, preflight  # noqa: E402

WORKER = "i5_scan"
PORT = 9356

CAM = """(() => { const c = window.__vcw || {}; return {
  f: c.frames, d: +c.cameraDist, occ: c.camOccluded, clr: +c.camClearance,
  ex: +c.camEyeX, ey: +c.camEyeY, ez: +c.camEyeZ, pitch: +c.camPitch }; })()"""

PIECE = (r"([SW]) c=\(([-\d.]+),([-\d.]+)\) y=\[([-\d.]+),([-\d.]+)\] "
         r"x=\[([-\d.]+),([-\d.]+)\] z=\[([-\d.]+),([-\d.]+)\]")


async def aim_and_read(s: Session, yaw: float) -> dict:
    await s.ev("(() => { window.__vcw_teleport = null; return 1; })()")
    await s.wait_frames(1)
    await s.ev(f"(() => {{ window.__vcw_aim = {yaw}; return 1; }})()")
    await s.wait_frames(4)
    v = await s.ev(CAM)
    return v if isinstance(v, dict) else {}


async def main() -> None:
    preflight(WORKER)
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        await s.ev("(() => { window.__vcw_teleport = {x: 27.4, z: 24.9}; return 1; })()")
        await s.wait_frames(6)
        import re
        pat = re.compile(PIECE)
        print("站在楼梯第一级 (27.4, 24.9)，绕 yaw 扫描:\n", flush=True)
        for deg in range(0, 360, 30):
            c = await aim_and_read(s, math.radians(deg))
            ex, ey, ez = c.get("ex", 0), c.get("ey", 0), c.get("ez", 0)
            # 在眼点位置探针,看谁贴着它
            await s.ev("(() => { window.__vcw_teleport = null; return 1; })()")
            await s.wait_frames(1)
            await s.ev(f"(() => {{ window.__vcw_teleport = {{x: {ex:.2f}, z: {ez:.2f}, probe: true}}; return 1; }})()")
            await s.wait_frames(3)
            t = await s.ev("(() => window.__vcw_probe || '')()") or ""
            near = [m.groups() for m in pat.finditer(t)
                    if float(m.group(6)) - 0.9 <= ex <= float(m.group(7)) + 0.9
                    and float(m.group(8)) - 0.9 <= ez <= float(m.group(9)) + 0.9]
            await s.ev("(() => { window.__vcw_teleport = null; return 1; })()")
            await s.wait_frames(2)
            print(f"  yaw={deg:3d}  d={c.get('d'):5.2f} occ={str(c.get('occ')):5} "
                  f"clr={c.get('clr'):5.2f} eye=({ex:6.2f},{ey:5.2f},{ez:6.2f}) "
                  f"near={[f'{g[0]}x[{float(g[5]):.1f},{float(g[6]):.1f}]y[{float(g[3]):.1f},{float(g[4]):.1f}]z[{float(g[7]):.1f},{float(g[8]):.1f}]' for g in near]}",
                  flush=True)


if __name__ == "__main__":
    asyncio.run(main())
