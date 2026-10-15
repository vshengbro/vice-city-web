"""从**浏览器进程**读真实 GPU 信息,回答「到底跑在真 GPU 还是软件后端」。

三个更早的探针都给了错误答案:
  * `canvas.getContext('webgl2')` 返回 null —— 画布已被游戏以自己的
    attributes 领走,再取拿不到句柄;
  * `navigator.gpu.requestAdapter()` 返回 null —— 这个游戏是 WebGL2,
    根本没走 WebGPU(HTML meta 里写明了 euv + WebGL2)。
所以这里问浏览器自己:GPU 进程在用什么驱动。

这个前提不钉死,「关掉阴影能快多少」就没有归因价值 ——
软件光栅化下顶点/填充率的瓶颈形状和真 GPU 完全不同。
"""

from __future__ import annotations

import asyncio
import json
import urllib.request

import websockets


def browser_ws() -> str:
    with urllib.request.urlopen(
        "http://127.0.0.1:9240/json/version", timeout=5
    ) as r:
        return json.loads(r.read())["webSocketDebuggerUrl"]


async def main() -> None:
    async with websockets.connect(browser_ws(), max_size=64 * 1024 * 1024) as ws:
        await ws.send(json.dumps({
            "id": 1,
            "method": "SystemInfo.getInfo",
            "params": {},
        }))
        while True:
            msg = json.loads(await ws.recv())
            if msg.get("id") == 1:
                info = msg.get("result", {})
                gpu = info.get("gpu", {})
                print(json.dumps({
                    "glRenderer": gpu.get("auxAttributes", {}).get("glRenderer"),
                    "glVendor": gpu.get("auxAttributes", {}).get("glVendor"),
                    "glVersion": gpu.get("auxAttributes", {}).get("glVersion"),
                    "glImplementationParts": gpu.get("auxAttributes", {})
                        .get("glImplementationParts"),
                    "featureStatus": gpu.get("featureStatus", {}),
                    "devices": gpu.get("devices", []),
                }, ensure_ascii=False, indent=2))
                return


if __name__ == "__main__":
    asyncio.run(main())