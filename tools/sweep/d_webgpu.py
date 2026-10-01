"""探测无头 Chrome 里 WebGPU 到底可不可用。

用户要求渲染层从 WebGL2 整体迁到 euv-engine 的 WebGPU,并且**不允许**
留 WebGL 回退。所以在下结论「无头跑不了、只能有头验」之前,先拿实测说话:

- `navigator.gpu` 是否存在;
- `requestAdapter()` 能不能拿到 adapter;
- 拿不到 adapter 时,是不是因为没开 `--enable-unsafe-swiftshader`
  (无头环境没有真 GPU,WebGPU 要靠 SwiftShader 的 Vulkan 后端兜底)。

两轮:`enabled` = 脚本里已经带的 flag,`bare` = 去掉 flag 重来一次。
"""

import asyncio
import json
import os
import subprocess
import sys

import websockets

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sweep_lib import Driver  # noqa: E402

URL = "http://127.0.0.1:8765/index.html"

PROBE = """(async () => {
  const out = {hasGpu: typeof navigator.gpu !== 'undefined'};
  if (!out.hasGpu) { out.adapter = null; out.err = 'no navigator.gpu'; return out; }
  try {
    const a = await navigator.gpu.requestAdapter();
    if (!a) {
      out.adapter = null;
      out.err = 'requestAdapter() returned null';
      return out;
    }
    out.adapter = 'ok';
    try {
      const info = a.info || (a.requestAdapterInfo ? await a.requestAdapterInfo() : null);
      out.info = info ? {vendor: info.vendor, arch: info.architecture, desc: info.description} : null;
    } catch (e) { out.info = 'info-unavailable: ' + e; }
    try {
      const d = await a.requestDevice();
      out.device = !!d;
      out.limits = d ? {maxTex: d.limits.maxTextureDimension2D,
                        maxBind: d.limits.maxBindGroups} : null;
    } catch (e) { out.device = 'requestDevice failed: ' + e; }
    return out;
  } catch (e) {
    out.adapter = 'threw';
    out.err = String(e);
    return out;
  }
})()"""


async def main() -> int:
    cdp = os.environ.get("VCW_CDP", "http://127.0.0.1:9241")
    subprocess.run(["curl", "-s", "--noproxy", "*",
                    f"{cdp}/json/new?about:blank"],
                   capture_output=True, text=True, timeout=30)
    out = subprocess.run(["curl", "-s", "--noproxy", "*", f"{cdp}/json/list"],
                         capture_output=True, text=True, timeout=30).stdout
    pages = [t for t in json.loads(out) if t.get("type") == "page"]
    pages.sort(key=lambda t: t.get("id", ""), reverse=True)
    async with websockets.connect(
        pages[0]["webSocketDebuggerUrl"], max_size=8 * 1024 * 1024
    ) as ws:
        d = Driver(ws)
        await d.cmd("Page.enable")
        await d.cmd("Runtime.enable")
        await d.cmd("Network.enable")
        await d.cmd("Network.setCacheDisabled", {"cacheDisabled": True})
        await d.cmd("Page.navigate", {"url": URL})
        await asyncio.sleep(8)
        got = await d.ev(PROBE)
    print(json.dumps(got, ensure_ascii=False, indent=2))
    ok = bool(got and got.get("device"))
    print(f"  -> WebGPU usable: {ok}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
