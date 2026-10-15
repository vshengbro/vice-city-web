"""I 组核心实测:楼内上楼梯,相机眼点是否留在楼内 + 三段回弹。"""

from __future__ import annotations

import asyncio
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import Session, preflight  # noqa: E402

WORKER = "i3_stairs"
PORT = 9354

CAM = """(() => { const c = window.__vcw || {}; return {
  f: c.frames, x: +c.playerX, y: +c.playerY, z: +c.playerZ,
  d: +c.cameraDist, tgt: +c.cameraDistTarget, occ: c.camOccluded,
  ex: +c.camEyeX, ey: +c.camEyeY, ez: +c.camEyeZ, clr: +c.camClearance,
  yaw: +c.cameraYaw, pitch: +c.camPitch, g: c.grounded,
  ints: (c.interiors||{}).inSolid }; })()"""

# LOFT interior envelope (world)
BX0, BX1, BZ0, BZ1 = 17.80, 28.20, 23.80, 36.20


async def ev(s: Session, e: str):
    return await s.ev(e)


async def tp(s: Session, x: float, z: float, yaw: float) -> None:
    await ev(s, "(() => { window.__vcw_teleport = null; return 1; })()")
    await s.wait_frames(1)
    await ev(s, f"(() => {{ window.__vcw_aim = {yaw}; return 1; }})()")
    await s.wait_frames(1)
    await ev(s, f"(() => {{ window.__vcw_teleport = {{x: {x}, z: {z}}}; return 1; }})()")
    await s.wait_frames(3)


def inb(c: dict) -> bool:
    return BX0 <= c["ex"] <= BX1 and BZ0 <= c["ez"] <= BZ1


def fmt(c: dict) -> str:
    return (f"f={c['f']:>4} P=({c['x']:6.2f},{c['y']:5.2f},{c['z']:6.2f}) "
            f"d={c['d']:5.2f} occ={str(c['occ']):5} eye=({c['ex']:6.2f},{c['ey']:5.2f},{c['ez']:6.2f}) "
            f"IN={inb(c)}")


async def main() -> None:
    preflight(WORKER)
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        await ev(s, "(() => { window.__vcw_teleport = {speed: 6}; return 1; })()")
        await s.wait_frames(3)
        print(f"boot ok  楼包围盒 x=[{BX0},{BX1}] z=[{BZ0},{BZ1}]\n", flush=True)

        # 楼梯最低一级在 x≈27.3, z≈24.9。上楼方向 = -X。
        # 相机在玩家背后 => yaw=0 时眼点在 -X 侧(上楼方向) => 眼点会落进楼梯井/二层楼板下方。
        # yaw=pi 时眼点在 +X 侧 => 楼下空地。
        for tag, yaw in [("yaw=0(眼点在 -X,楼梯井侧)", 0.0), ("yaw=pi(眼点在 +X,楼下侧)", math.pi)]:
            print(f"=== {tag} ===", flush=True)
            await tp(s, 27.3, 24.9, yaw)
            c = await ev(s, CAM)
            print(f"  start {fmt(c)}", flush=True)
            await s.key("KeyW", True)
            for n in range(20):
                await s.wait_frames(1)
                c = await ev(s, CAM)
                print(f"  [{n:2}] {fmt(c)}", flush=True)
            await s.key("KeyW", False)
            await s.wait_frames(2)
            print(flush=True)


if __name__ == "__main__":
    asyncio.run(main())
