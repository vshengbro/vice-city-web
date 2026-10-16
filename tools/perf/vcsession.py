"""共用 CDP 会话:**一个 Chrome 实例、一个 tab**,页面切换用 Page.navigate。

硬约束:禁止新开浏览器窗口 / 多个 tab。之前那版每次测量都
`/json/new` 开一个新 tab,跑十几轮就攒下一堆 tab —— 违反约束,而且
headless Chrome 的 GPU 资源在多 tab 下会互相抢,帧率读数会漂。

所以这个模块负责:
  * 找到/复用唯一的测量 tab(用 URL 里的 `perf=` 标记认领);
  * 复用它 navigate 到不同页面;
  * 提供 CDP 调用封装。

`ensure_measure_tab()` 会在已有 tab 里找带 `perf=` 的那个,找不到才建
**一个**,之后永远复用它。
"""

from __future__ import annotations

import asyncio
import json
import time
import urllib.request

import websockets

BASE_URL = "http://127.0.0.1:8877/index.html"
PORT = 9240
SCRATCH = "/Users/sqs/.hermes/cache/scratch/vcw-perf"

MEASURE_MARK = "perf="


def _get(path: str) -> list | dict:
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}{path}", timeout=6) as r:
        return json.loads(r.read())


def _put(path: str):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", method="PUT")
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())


def page_ws() -> str:
    """测量 tab 的 ws url:复用已有的,否则建**一个**并记住它。"""
    try:
        tabs = _get("/json/list")
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"chrome on :{PORT} unreachable: {exc}") from exc

    mine = [t for t in tabs
            if t.get("type") == "page" and MEASURE_MARK in t.get("url", "")]
    if mine:
        # 挑最后一个(最新建的)测量 tab,其余测量 tab 关掉,保持单 tab。
        keep = mine[-1]
        for extra in mine[:-1]:
            _put(f"/json/close/{extra['id']}")
        return keep["webSocketDebuggerUrl"]

    created = _put("/json/new?about:blank")
    return created["webSocketDebuggerUrl"]


class Cdp:
    """最小 CDP 客户端。"""

    def __init__(self, ws_url: str | None = None) -> None:
        self.ws_url = ws_url or page_ws()
        self.ws = None
        self._n = 0

    async def __aenter__(self) -> "Cdp":
        self.ws = await websockets.connect(
            self.ws_url, max_size=256 * 1024 * 1024
        )
        return self

    async def __aexit__(self, *exc) -> None:
        if self.ws:
            await self.ws.close()

    async def cmd(self, method: str, **params) -> dict:
        assert self.ws is not None
        self._n += 1
        mid = self._n
        await self.ws.send(
            json.dumps({"id": mid, "method": method, "params": params})
        )
        while True:
            msg = json.loads(await self.ws.recv())
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    async def ev(self, expr: str):
        r = await self.cmd(
            "Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True
        )
        if r.get("exceptionDetails"):
            return None
        return r.get("result", {}).get("value")

    async def goto(self, url: str) -> None:
        """在**同一个 tab** 里换页面 —— 不新开 tab,更不新开窗口。"""
        await self.cmd("Page.navigate", url=url)

    async def boot(self, tag: str, seconds: int = 180, min_frames: int = 25) -> bool:
        await self.cmd("Runtime.enable")
        await self.cmd("Page.enable")
        await self.cmd("Network.enable")
        await self.cmd("Network.setCacheDisabled", cacheDisabled=True)
        await self.cmd("Emulation.setFocusEmulationEnabled", enabled=True)
        await self.cmd("Page.bringToFront")
        self.ws_url_ready = True
        await self.goto(f"{BASE_URL}?res=1.0&perf={tag}")
        for _ in range(seconds):
            await asyncio.sleep(1.0)
            n = await self.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(n, int) and n > min_frames:
                return True
        return False


def browser_ws() -> str:
    with urllib.request.urlopen(
        f"http://127.0.0.1:{PORT}/json/version", timeout=6
    ) as r:
        return json.loads(r.read())["webSocketDebuggerUrl"]


async def gpu_info() -> dict:
    """浏览器级的真实 GPU 信息(vsync 已解除时也要确认)。"""
    async with websockets.connect(
        browser_ws(), max_size=64 * 1024 * 1024
    ) as ws:
        await ws.send(json.dumps({
            "id": 1, "method": "SystemInfo.getInfo", "params": {}
        }))
        while True:
            msg = json.loads(await ws.recv())
            if msg.get("id") == 1:
                g = msg.get("result", {}).get("gpu", {})
                a = g.get("auxAttributes", {})
                return {
                    "glRenderer": a.get("glRenderer"),
                    "glVendor": a.get("glVendor"),
                    "glImplementationParts": a.get("glImplementationParts"),
                    "webgl": g.get("featureStatus", {}).get("webgl"),
                }


if __name__ == "__main__":
    import asyncio

    print(json.dumps(asyncio.run(gpu_info()), ensure_ascii=False, indent=2))
    print("# measure tab:", page_ws())
    _ = time