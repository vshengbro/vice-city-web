"""D 组决定性测试:请求是否被消费 + 坐标是否被传送。"""

from __future__ import annotations

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import Session, preflight  # noqa: E402

WORKER = "d3_diag"
PORT = 9352

POS = "(() => { const c = window.__vcw || {}; return [+c.playerX, +c.playerY, +c.playerZ]; })()"
LEFT = "(() => JSON.stringify(window.__vcw_teleport))()"
PLEN = "(() => { const t = window.__vcw_probe; return typeof t === 'string' ? t.length : -1; })()"
CLEAR = "(() => { window.__vcw_probe = ''; return 1; })()"
NULLIT = "(() => { window.__vcw_teleport = null; return 1; })()"


async def ev(s: Session, expr: str):
    return await s.ev(expr)


async def main() -> None:
    preflight(WORKER)
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        print(f"boot ok  起点={await ev(s, POS)}\n", flush=True)

        print("--- 1. {probe:true}: 请求是否被消费 ---", flush=True)
        await ev(s, CLEAR)
        await ev(s, "(() => { window.__vcw_teleport = {probe: true}; return 1; })()")
        for n in (2, 5, 10):
            await s.wait_frames(n)
            print(f"  +{n}f  leftover={await ev(s, LEFT)}  probeLen={await ev(s, PLEN)}", flush=True)

        print("\n--- 2. 探针内容是否会被每帧覆盖 ---", flush=True)
        await ev(s, "(() => { window.__vcw_probe = '@@MARK@@'; return 1; })()")
        await s.wait_frames(4)
        print(f"  标记 4 帧后: {str(await ev(s, '(() => window.__vcw_probe)()'))!r:.80}", flush=True)

        print("\n--- 3. {x,z,probe}: 是否真的传送 ---", flush=True)
        await ev(s, NULLIT)
        await ev(s, "(() => { window.__vcw_teleport = {x: 60, z: 60, probe: true}; return 1; })()")
        await s.wait_frames(6)
        print(f"  目标(60,60)  pos={await ev(s, POS)}  leftover={await ev(s, LEFT)}  probeLen={await ev(s, PLEN)}", flush=True)

        print("\n--- 4. 清通道后单独发 {x,z} ---", flush=True)
        await ev(s, NULLIT)
        await s.wait_frames(2)
        await ev(s, "(() => { window.__vcw_teleport = {x: 60, z: 60}; return 1; })()")
        await s.wait_frames(6)
        print(f"  目标(60,60)  pos={await ev(s, POS)}  leftover={await ev(s, LEFT)}", flush=True)

        print("\n--- 5. probe 字段 truthiness ---", flush=True)
        for v in ("1", "'x'", "0", "true", "false", "null"):
            await ev(s, NULLIT)
            await s.wait_frames(2)
            await ev(s, CLEAR)
            await ev(s, f"(() => {{ window.__vcw_teleport = {{probe: {v}}}; return 1; }})()")
            await s.wait_frames(3)
            print(f"  probe:{v:>6} -> probeLen={await ev(s, PLEN)}  leftover={await ev(s, LEFT)}", flush=True)


if __name__ == "__main__":
    asyncio.run(main())
