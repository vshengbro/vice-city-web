"""一组 flag 到底能不能拿到 WebGPU adapter —— 单次判定。

`flag-sweep.sh` 每组 flag 起一个 Chrome,然后用这个脚本问一句话:
`requestAdapter()` 会不会 resolve 出东西。

## 为什么不直接用 `d_webgpu.py`

那个脚本测的是**页面**的 WebGPU 状态(依赖 vice-city-web 的 wasm 已经
在跑)。这里要测的是**这台机器 + 这组 flag** 能不能提供 WebGPU,页面
里有没有 wasm 无关 —— 判据必须与环境变量、与具体页面解耦,否则换一组
flag 得到的差异就分不清是环境还是页面。

## 判据分级

| 级别 | 含义 |
|---|---|
| `NO_NAVIGATOR_GPU` | 页面里根本没有 `navigator.gpu`(非安全上下文,或 flag 把它关了) |
| `ADAPTER_NULL` | `navigator.gpu` 在,但 `requestAdapter()` 返回 null —— 没有任何可用 GPU |
| `DEVICE_FAIL` | 拿到 adapter 了,但 `requestDevice()` 失败 |
| `OK` | adapter + device 都拿到,并报出 vendor / architecture |

只有 `OK` 才算这组 flag 可用。注意 `NO_NAVIGATOR_GPU` 与
`ADAPTER_NULL` 的区别很重要:前者说明 flag 太狠(把接口都关了),
后者说明硬件栈没起来(接口在,但底下没卡)—— 两者要分开报,否则
「换哪条 flag」这个问题就答不了。
"""

import asyncio
import json
import os
import sys

import websockets

CDP = os.environ.get("VCW_CDP", "http://127.0.0.1:9251")
# `navigator.gpu` 只在 secure context 里暴露。CDP `/json/new` 开的
# `about:blank` 是 opaque origin,拿不到接口 —— 必须导航到一个
# `http://127.0.0.1` 页面(W3C 规定 localhost 是 potentially
# trustworthy 的 origin)。这一条踩过的坑:用 about:blank 测 8 组 flag
# 全部报 `NO_NAVIGATOR_GPU`,是 origin 问题,不是 flag 问题。
PAGE = os.environ.get("VCW_SWEEP_PAGE",
                       "http://127.0.0.1:8879/blank.html")

# 在**已有**实例里开一个新 tab(不新开窗口),跑完关掉。
PROBE = """(async () => {
  const out = {hasGpu: typeof navigator.gpu !== 'undefined'};
  if (!out.hasGpu) return JSON.stringify({...out, level: 'NO_NAVIGATOR_GPU'});
  let a;
  try { a = await navigator.gpu.requestAdapter(); }
  catch (e) { return JSON.stringify({...out, level: 'ADAPTER_THROW', err: String(e)}); }
  if (!a) return JSON.stringify({...out, level: 'ADAPTER_NULL'});
  let d;
  try { d = await a.requestDevice(); }
  catch (e) { return JSON.stringify({...out, level: 'DEVICE_THROW', err: String(e)}); }
  if (!d) return JSON.stringify({...out, level: 'DEVICE_NULL'});
  const info = a.info || (a.requestAdapterInfo ? await a.requestAdapterInfo() : null);
  return JSON.stringify({
    ...out, level: 'OK',
    vendor: info ? info.vendor : null,
    architecture: info ? info.architecture : null,
    description: info ? info.description : null,
    device: info ? info.device : null,
    maxTex2D: d.limits.maxTextureDimension2D,
    maxBindGroups: d.limits.maxBindGroups,
    maxStorageBufferBindingSize: d.limits.maxStorageBufferBindingSize,
    maxComputeWorkgroupSizeX: d.limits.maxComputeWorkgroupSizeX,
    features: Array.from(d.features).sort(),
  });
})()"""


def curl(path: str, put: bool = False) -> str:
    import subprocess
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
        return res.get("value") if "value" in res else None


async def main() -> int:
    raw = curl("/json/new?about:blank", put=True)
    try:
        tab = json.loads(raw)
    except json.JSONDecodeError:
        print("TAB-OPEN-FAILED")
        return 2
    ws_url = tab.get("webSocketDebuggerUrl")
    target_id = tab.get("id")
    if not ws_url:
        print("TAB-OPEN-FAILED")
        return 2

    try:
        async with websockets.connect(ws_url, max_size=8 * 1024 * 1024) as ws:
            d = Driver(ws)
            await d.cmd("Runtime.enable")
            await d.cmd("Page.enable")
            # 导航到 localhost 页面 —— secure context 是 `navigator.gpu`
            # 存在的前提,见上面 `PAGE` 的注释。
            await d.cmd("Page.navigate", {"url": PAGE})
            # 不睡死一个数:等 `document.readyState` 变 complete,最多 10 s。
            for _ in range(40):
                await asyncio.sleep(0.25)
                state = await d.ev("String(document.readyState)")
                if state == "complete":
                    break
            got = await d.ev(PROBE)
    finally:
        curl(f"/json/close/{target_id}")

    if not got:
        print("EVAL-FAILED")
        return 1
    data = json.loads(got)
    level = data.get("level", "?")
    if level == "OK":
        print(f"{level:<18} vendor={data.get('vendor')} "
              f"arch={data.get('architecture')} desc={data.get('description')}")
        print(f"{'':<18} maxTex2D={data.get('maxTex2D')} "
              f"maxBindGroups={data.get('maxBindGroups')} "
              f"maxStorageBuf={data.get('maxStorageBufferBindingSize')} "
              f"maxCwgX={data.get('maxComputeWorkgroupSizeX')}")
        feats = data.get("features") or []
        print(f"{'':<18} features={feats if feats else '(none)'}")
    else:
        extra = f" err={data.get('err')}" if data.get("err") else ""
        print(f"{level}{extra}")
    return 0 if level == "OK" else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
