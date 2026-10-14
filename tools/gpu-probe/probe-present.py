"""最小 WebGPU 呈现探针:不碰游戏,在同一页面里另建一块 canvas。

如果这里能画��品红 + 三角形,说明浏览器与驱动没问题,故障在游戏那条
提交路径上;如果这里也是黑的,那就是这台机器的 WebGPU 呈现本身不行,
游戏侧再怎么改都没用。

用法:python3.11 probe-present.py [url]
"""

import asyncio
import base64
import json
import os
import subprocess
import sys
import time
import urllib.parse
import urllib.request

import websockets

CHROME = (
    "/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/"
    "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
)
PORT = int(os.environ.get("VCW_PORT", "9241"))
SHOTS = "/Users/sqs/code/vice-city-web/tools/gpu-probe/shots"

SCRIPT = r"""
(async () => {
  const out = {};
  const c = document.createElement('canvas');
  c.width = 320; c.height = 200;
  c.id = 'probe-canvas';
  c.style.cssText = 'position:fixed;left:0;top:0;z-index:99999;';
  document.body.appendChild(c);
  try {
    out.gpuTextureUsage = {
      COPY_SRC: GPUTextureUsage.COPY_SRC,
      COPY_DST: GPUTextureUsage.COPY_DST,
      TEXTURE_BINDING: GPUTextureUsage.TEXTURE_BINDING,
      STORAGE_BINDING: GPUTextureUsage.STORAGE_BINDING,
      RENDER_ATTACHMENT: GPUTextureUsage.RENDER_ATTACHMENT,
    };
    out.gpuBufferUsage = {
      MAP_READ: GPUBufferUsage.MAP_READ,
      MAP_WRITE: GPUBufferUsage.MAP_WRITE,
      COPY_SRC: GPUBufferUsage.COPY_SRC,
      COPY_DST: GPUBufferUsage.COPY_DST,
      INDEX: GPUBufferUsage.INDEX,
      VERTEX: GPUBufferUsage.VERTEX,
      UNIFORM: GPUBufferUsage.UNIFORM,
      STORAGE: GPUBufferUsage.STORAGE,
    };
    const adapter = await navigator.gpu.requestAdapter();
    out.adapter = adapter ? 'ok' : 'null';
    const device = await adapter.requestDevice();
    const ctx = c.getContext('webgpu');
    const format = navigator.gpu.getPreferredCanvasFormat();
    out.format = format;
    ctx.configure({ device, format, alphaMode: 'opaque' });
    const enc = device.createCommandEncoder();
    const tex = ctx.getCurrentTexture();
    const pass = enc.beginRenderPass({
      colorAttachments: [{
        view: tex.createView(),
        clearValue: { r: 1, g: 0, b: 1, a: 1 },
        loadOp: 'clear', storeOp: 'store'
      }]
    });
    pass.end();
    device.queue.submit([enc.finish()]);
    await device.queue.onSubmittedWorkDone();
    out.submitted = true;
  } catch (e) {
    out.error = String(e);
  }
  // read back
  const t = document.createElement('canvas');
  t.width = c.width; t.height = c.height;
  const g = t.getContext('2d');
  g.drawImage(c, 0, 0);
  const d = g.getImageData(0, 0, c.width, c.height).data;
  let counts = {};
  for (let i = 0; i < d.length; i += 4) {
    const k = d[i] + ',' + d[i+1] + ',' + d[i+2];
    counts[k] = (counts[k] || 0) + 1;
  }
  out.top = Object.entries(counts).sort((a, b) => b[1] - a[1]).slice(0, 3);
  return out;
})()
"""


def launch():
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version")
        return
    except Exception:
        pass
    subprocess.Popen(
        [
            CHROME,
            "--headless=new",
            f"--remote-debugging-port={PORT}",
            "--no-first-run",
            "--user-data-dir=/Users/sqs/.hermes/cache/scratch/chrome-probe",
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--window-size=1400,900",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    for _ in range(60):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version")
            return
        except Exception:
            time.sleep(0.5)
    raise SystemExit("chrome did not start")


class Driver:
    def __init__(self, ws):
        self.ws = ws
        self.mid = 0

    async def cmd(self, method, params=None):
        self.mid += 1
        await self.ws.send(
            json.dumps({"id": self.mid, "method": method, "params": params or {}})
        )
        while True:
            msg = json.loads(await self.ws.recv())
            if msg.get("id") == self.mid:
                return msg

    async def ev(self, expr):
        r = await self.cmd(
            "Runtime.evaluate",
            {"expression": expr, "awaitPromise": True, "returnByValue": True},
        )
        res = r.get("result", {}).get("result", {})
        return res.get("value")


async def main():
    url = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8877/index.html?res=1.0"
    launch()
    req = urllib.request.Request(
        f"http://127.0.0.1:{PORT}/json/new?{urllib.parse.quote(url)}",
        method="PUT",
    )
    tab = json.loads(urllib.request.urlopen(req).read())
    ws_url = tab["webSocketDebuggerUrl"]
    async with websockets.connect(ws_url, max_size=80_000_000) as ws:
        drv = Driver(ws)
        await drv.cmd("Runtime.enable")
        await drv.cmd("Page.enable")
        await asyncio.sleep(3)
        out = await drv.ev(SCRIPT)
        print(json.dumps(out, indent=2))
        shot = await drv.cmd("Page.captureScreenshot", {"format": "png"})
        os.makedirs(SHOTS, exist_ok=True)
        p = os.path.join(SHOTS, "probe-present.png")
        with open(p, "wb") as fh:
            fh.write(base64.b64decode(shot["data"]))
        print("shot:", p)


if __name__ == "__main__":
    asyncio.run(main())