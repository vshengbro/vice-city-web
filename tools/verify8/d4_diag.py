"""D 组:复现空串的确切序列。I 组:遮挡 → 解除 → 回弹三段。"""

from __future__ import annotations

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import Session, preflight  # noqa: E402

WORKER = "d4_diag"
PORT = 9352

POS = "(() => { const c = window.__vcw || {}; return [+c.playerX, +c.playerY, +c.playerZ]; })()"
LEFT = "(() => JSON.stringify(window.__vcw_teleport))()"
PLEN = "(() => { const t = window.__vcw_probe; return typeof t === 'string' ? t.length : -1; })()"
CAM = """(() => { const c = window.__vcw || {}; return {
  f: c.frames, y: +c.playerY, d: +c.cameraDist, tgt: +c.cameraDistTarget,
  occ: c.camOccluded, ex: +c.camEyeX, ey: +c.camEyeY, ez: +c.camEyeZ }; })()"""


async def main() -> None:
    preflight(WORKER)
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        print(f"boot ok 起点={await s.ev(POS)}\n", flush=True)

        print("=== D: 逐字复刻 i2_stairs 的序列 ===", flush=True)
        seq = [
            ("{speed: 6}", "设加速"),
            ("{probe: true}", "第一次 probe"),
            ("{x: 23, z: 30}", "传送"),
            ("{probe: true}", "第二次 probe"),
        ]
        for payload, label in seq:
            await s.ev(f"(() => {{ window.__vcw_probe = ''; window.__vcw_teleport = {payload}; return 1; }})()")
            await s.wait_frames(3)
            print(f"  {label:16} {payload:18} pos={await s.ev(POS)} "
                  f"left={await s.ev(LEFT)} probeLen={await s.ev(PLEN)}", flush=True)
            await s.wait_frames(3)
            print(f"       +3f 后                 probeLen={await s.ev(PLEN)}", flush=True)

        print("\n=== I: 遮挡 → 解除 → 回弹 ===", flush=True)
        # 空旷处基线
        await s.ev("(() => { window.__vcw_teleport = {x: 33.5, z: 45}; return 1; })()")
        await s.wait_frames(20)
        base = await s.ev(CAM)
        print(f"  [空旷基线] d={base['d']:.3f} tgt={base['tgt']} occ={base['occ']} "
              f"eye=({base['ex']:.2f},{base['ey']:.2f},{base['ez']:.2f})", flush=True)

        # 进楼(遮挡)
        await s.ev("(() => { window.__vcw_teleport = {x: 23, z: 30}; return 1; })()")
        print("  --- 遮挡中(楼内) ---", flush=True)
        for n in range(8):
            await s.wait_frames(1)
            c = await s.ev(CAM)
            print(f"    [{n}] f={c['f']} d={c['d']:.3f} tgt={c['tgt']} occ={c['occ']} "
                  f"eye=({c['ex']:.2f},{c['ey']:.2f},{c['ez']:.2f})", flush=True)

        # 走出楼(解除遮挡)
        await s.ev("(() => { window.__vcw_teleport = {x: 33.5, z: 45}; return 1; })()")
        print("  --- 回弹(回到空旷) ---", flush=True)
        for n in range(12):
            await s.wait_frames(1)
            c = await s.ev(CAM)
            print(f"    [{n}] f={c['f']} d={c['d']:.3f} tgt={c['tgt']} occ={c['occ']} "
                  f"eye=({c['ex']:.2f},{c['ey']:.2f},{c['ez']:.2f})", flush=True)


if __name__ == "__main__":
    asyncio.run(main())
