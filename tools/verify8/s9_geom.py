"""Dump the REAL stair slab boxes the wasm is using, from the live page.

The unit-test fixture `slab_world()` is a hand-written stand-in; the in-game
stair is built by `push_showcase_interior` from the asset span/half/yaw. This
reads the actual AABBs so a fix is validated against the geometry that ships.
"""
from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from vlib import Session  # noqa: E402

PORT = 9488
WORKER = "s9geo"


async def main() -> None:
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=25)
        # nearInteriors only lists Walls. Slabs are what the stair is made of,
        # so walk the whole thing via the route probe instead: ask the game to
        # report the support height along the actual stair centre line.
        out = await s.ev("""(() => {
          const c = window.__vcw || {};
          return JSON.stringify({interiors: c.interiors, keys: Object.keys(c)});
        })()""")
        print("interiors blob:")
        print(json.dumps(json.loads(out or "{}").get("interiors"),
                         ensure_ascii=False, indent=1)[:4000])


if __name__ == "__main__":
    asyncio.run(main())
