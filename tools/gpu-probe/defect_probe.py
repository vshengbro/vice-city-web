#!/usr/bin/env python3
"""在 headless Chrome 里跑 `defect-repro.html`,把三个 claim 的实测结果打出来。

纪律同 `run.sh`:独立 Chrome for Testing 二进制、`--headless=new`、
复用已有实例开新 tab(不新开窗口)、**不带** `--use-angle=swiftshader`
也不带 `--disable-gpu`(两者都会让 `requestAdapter()` 返回 null,
那样测出来的会是 flag 问题而不是 euv 的缺陷)。

复用现有实例:调用方先用 `run.sh` 之类的方式把 Chrome 拉起来
(VCW_CDP 指向它),本脚本只负责开 tab → 导航 → 求值 → 关 tab。
"""
import asyncio
import json
import os
import subprocess
import sys

import websockets

CDP = os.environ.get("VCW_CDP", "http://127.0.0.1:9251")
PORT = CDP.rsplit(":", 1)[-1]
PAGE = os.environ.get("VCW_DEFECT_PAGE",
                     "http://127.0.0.1:8879/defect-repro.html")


def curl(path: str, put: bool = False) -> str:
    cmd = ["curl", "-s", "--noproxy", "*"]
    if put:
        cmd += ["-X", "PUT"]
    cmd.append(f"{CDP}{path}")
    return subprocess.run(cmd, capture_output=True, text=True,
                          timeout=30).stdout


class Driver:
    def __init__(self, ws):
        self.ws = ws
        self.mid = 0

    async def cmd(self, method: str, params: dict | None = None) -> dict:
        self.mid += 1
        await self.ws.send(json.dumps(
            {"id": self.mid, "method": method, "params": params or {}}))
        while True:
            msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=120))
            if msg.get("id") == self.mid:
                return msg.get("result", {})

    async def ev(self, expr: str):
        r = await self.cmd("Runtime.evaluate", {
            "expression": expr, "returnByValue": True, "awaitPromise": True})
        res = r.get("result", {})
        if res.get("subtype") == "error":
            return {"__evalError": res.get("description")}
        return res.get("value") if "value" in res else None


async def main() -> int:
    raw = curl("/json/new?about:blank", put=True)
    try:
        tab = json.loads(raw)
    except json.JSONDecodeError:
        print("TAB-OPEN-FAILED")
        return 2
    ws_url, target_id = tab.get("webSocketDebuggerUrl"), tab.get("id")
    if not ws_url:
        print("TAB-OPEN-FAILED")
        return 2

    try:
        async with websockets.connect(ws_url, max_size=8 * 1024 * 1024) as ws:
            d = Driver(ws)
            await d.cmd("Runtime.enable")
            await d.cmd("Page.enable")
            # 收集 console 与未捕获异常:页面若在设 __defectRepro 前就炸了,
            # 这是唯一能看见死因的地方(WebGPU 侧抛的 TypeError 走 console)。
            await d.cmd("Log.enable")
            console: list[str] = []
            page_errors: list[str] = []

            async def pump(n: int = 120) -> None:
                """抽干事件队列,记下 console / exception,顺带当节流。"""
                for _ in range(n):
                    try:
                        msg = json.loads(await asyncio.wait_for(ws.recv(), timeout=0.25))
                    except asyncio.TimeoutError:
                        return
                    m = msg.get("method")
                    p = msg.get("params", {})
                    if m == "Runtime.consoleAPICalled":
                        txt = " ".join(str(a.get("value", a.get("description", "")))
                                       for a in p.get("args", []))
                        console.append(f"[{p.get('type')}] {txt}"[:500])
                    elif m == "Runtime.exceptionThrown":
                        d0 = p.get("exceptionDetails", {})
                        page_errors.append(str(d0.get("exception", {})
                                               .get("description")
                                               or d0.get("text"))[:500])
                    elif m == "Log.entryAdded":
                        e = p.get("entry", {})
                        console.append(f"[log:{e.get('level')}] {e.get('text')}"[:500])

            await pump(20)
            await d.cmd("Page.navigate", {"url": PAGE})
            await pump(40)
            for _ in range(40):
                await asyncio.sleep(0.25)
                if await d.ev("String(document.readyState)") == "complete":
                    break
            # 页面脚本是 async,等 window.__defectRepro 落定,最多 20 s。
            # 注意别写成 `if got:` —— JSON.stringify(null) 是字符串 "null",
            # 真值,会立刻 break 掉。必须显式比 "null"。
            got = None
            for _ in range(80):
                got = await d.ev("JSON.stringify(window.__defectRepro || null)")
                if got and got != "null":
                    break
                await pump(2)
                await asyncio.sleep(0.25)
            await pump(10)
            if not got or got == "null":
                got = json.dumps({
                    "EVAL_FAILED": "window.__defectRepro never set",
                    "console": console, "pageErrors": page_errors,
                    "body": await d.ev("document.body.innerText.slice(0,400)"),
                }, ensure_ascii=False)
    finally:
        curl(f"/json/close/{target_id}")

    if not got:
        print("EVAL-FAILED (页面没跑完或 __defectRepro 未设置)")
        return 1
    print(json.dumps(json.loads(got), indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
