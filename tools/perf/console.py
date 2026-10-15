"""读游戏自己的 console,回答「渲染后端到底起来没有」。

`const.rs:177` 里 `BACKEND_NONE = "init"`,HUD 打的是 `backend_name()`。
HUD 上写着 `init` 加上 `0 tris`,意味着 `WebGlRenderer::new()` 失败、
`game.renderer` 是 `None` —— 整个 WebGL 后端没建起来。

那种情况下游戏根本不画任何东西:测到的 rAF 节奏是**空转循环**的节奏,
拿它当「GPU 帧率」会把结论整个带偏(尤其是「阴影占 33%」这种归因)。
所以每个测量档位跑之前都要先过这道闸:后端必须是真 GL 且 tris > 0。
"""

from __future__ import annotations

import asyncio
import json
import sys

sys.path.insert(0, "/Users/sqs/code/vice-city-web/tools/perf")
from vcperf import Cdp, find_port, new_tab  # noqa: E402


async def main() -> None:
    port = find_port()
    ws_url = new_tab(port, "http://127.0.0.1:8877/index.html?res=1.0&con=probe")
    async with Cdp(ws_url) as c:
        await c.cmd("Runtime.enable")
        await c.cmd("Log.enable")
        await c.cmd("Page.enable")

        logs: list[str] = []
        n = 0
        while n < 150:
            await n_step(c, logs)
            n += 1
            await asyncio.sleep(1.0)
            f = await c.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(f, int) and f > 25:
                break

        state = await c.ev("""(() => {
          const c = window.__vcw || {};
          return {hud: c.hud || '', tris: c.tris || 0,
                  frames: c.frames || 0};
        })()""")
        print("== state ==")
        print(json.dumps(state, ensure_ascii=False, indent=2))
        print("== console ==")
        for line in logs:
            if any(k in line for k in
                   ("vcw", "backend", "renderer", "WebGL", "webgl", "GL",
                    "error", "fail", "shadow")):
                print(line[:400])


async def n_step(c: Cdp, logs: list[str]) -> None:
    import json as _json
    assert c.ws is not None
    try:
        while True:
            raw = await asyncio.wait_for(c.ws.recv(), timeout=0.01)
            msg = _json.loads(raw)
            m = msg.get("method", "")
            if m == "Runtime.consoleAPICalled":
                args = msg["params"].get("args", [])
                txt = " ".join(
                    str(a.get("value", a.get("description", ""))) for a in args
                )
                logs.append(f"[console] {txt}")
            elif m == "Log.entryAdded":
                e = msg["params"]["entry"]
                logs.append(f"[log:{e.get('level')}] {e.get('text')}")
            elif m == "Runtime.exceptionThrown":
                d = msg["params"].get("exceptionDetails", {})
                logs.append(f"[exc] {d.get('text')} {d.get('exception',{}).get('description','')}")
    except asyncio.TimeoutError:
        return


if __name__ == "__main__":
    asyncio.run(main())