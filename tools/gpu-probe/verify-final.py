"""稳定的最终验收:一次启动,少打扰,多读数。

前面那些脚本每 3~6 秒发一次 `Runtime.evaluate`,在无头下会把主线程
拖死(`frames` 只有个位数),于是量到的是「渲染刚起步」的空画面 ——
`bands: 1` 是这么来的,不是 bug。

这个脚本:启动后**只**轮询一次就绪,然后一次性把所有测量做完,
中间不插任何 CDP 往返。
"""

import asyncio
import base64
import json
import os
import subprocess
import time
import urllib.request

import websockets

CHROME = (
    "/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/"
    "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
)
PORT = int(os.environ.get("VCW_PORT", "9241"))
SCRATCH = "/Users/sqs/.hermes/cache/scratch/vcw-final"
SHOTS = "/Users/sqs/code/vice-city-web/tools/gpu-probe/shots"

HOOK = r"""
(() => {
  const H = window.__H = { errors: [], frames: 0, nSetIB: 0, nDraw: 0 };
  let seq = 0;
  const rd = GPUAdapter.prototype.requestDevice;
  GPUAdapter.prototype.requestDevice = function (...a) {
    const mine = ++seq;
    const p = rd.apply(this, a);
    p.then((d) => {
      if (mine === 1) window.__DEV = d;
      d.addEventListener('uncapturederror', (e) => {
        H.errors.push('D' + mine + ' UNCAPTURED: ' + (e.error && e.error.message));
      });
      H.errors.push('D' + mine + ' captured');
    });
    return p;
  };
  // ⚠️ 转发时必须 `...args` 原样传下去。之前这里写成 `function (b)`,
  // 吞掉了第 2 个参数(format),wasm 那边的 `setIndexBuffer(buffer, fmt)`
  // 就变成了只传 1 个参数 —— 于是**探针自己制造**了
  // `2 arguments required, but only 1 present`,整帧中止、drawIndexed
  // 一次都不跑。那是测量代码的 bug,不是 wasm 的 bug。
  const si = GPURenderPassEncoder.prototype.setIndexBuffer;
  GPURenderPassEncoder.prototype.setIndexBuffer = function (...args) {
    H.nSetIB++; return si.apply(this, args);
  };
  const di = GPURenderPassEncoder.prototype.drawIndexed;
  GPURenderPassEncoder.prototype.drawIndexed = function (...a) {
    H.nDraw++; return di.apply(this, a);
  };
  const sb = GPUQueue.prototype.submit;
  GPUQueue.prototype.submit = function (a) { H.frames++; return sb.call(this, a); };
})();
"""


class Driver:
    """单一 reader 协程 + per-id Future。

    之前「主协程 recv 等回复」+「后台 drain 协程 recv 保活」会撞车:
    websockets 不允许两个 recv 并发。要静默一段时间又不能被 keepalive
    掐断,就必须**始终**有且只有一个 reader 在收,命令回复按 id 派发
    给各自的 Future。
    """

    def __init__(self, ws):
        self.ws = ws
        self.mid = 0
        self.pending = {}
        self.events = []

    def start_reader(self):
        async def reader():
            while True:
                try:
                    msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=5))
                except (asyncio.TimeoutError, TimeoutError):
                    continue
                except Exception:
                    return
                mid = msg.get("id")
                fut = self.pending.get(mid)
                if fut is not None and not fut.done():
                    fut.set_result(msg)
                elif mid is None:
                    self.events.append(msg)

        return asyncio.ensure_future(reader())

    async def cmd(self, method, params=None):
        self.mid += 1
        fut = asyncio.get_event_loop().create_future()
        self.pending[self.mid] = fut
        await self.ws.send(json.dumps({"id": self.mid, "method": method,
                                       "params": params or {}}))
        msg = await asyncio.wait_for(fut, timeout=300)
        self.pending.pop(self.mid, None)
        if "error" in msg:
            raise RuntimeError(f"{method}: {msg['error']}")
        return msg.get("result", {})

    async def ev(self, expr):
        r = await self.cmd("Runtime.evaluate",
                           {"expression": expr, "returnByValue": True,
                            "awaitPromise": True})
        res = r.get("result", {})
        if res.get("subtype") == "error":
            return {"__error": res.get("description", "?")}
        return res.get("value")


def launch():
    os.makedirs(SCRATCH, exist_ok=True)
    subprocess.run(["pkill", "-9", "-f", f"remote-debugging-port={PORT}"],
                   capture_output=True)
    time.sleep(2)
    profile = os.path.join(SCRATCH, "profile")
    subprocess.run(["rm", "-rf", profile], check=False)
    proc = subprocess.Popen(
        [CHROME, "--headless=new", "--no-sandbox",
         "--disable-component-update", "--disable-default-apps", "--disable-sync",
         "--no-first-run", "--no-default-browser-check", "--disable-extensions",
         "--metrics-recording-only", "--mute-audio", "--disable-breakpad",
         "--disable-background-networking",
         f"--remote-debugging-port={PORT}", "--remote-allow-origins=*",
         f"--user-data-dir={profile}", "--hide-scrollbars",
         "--window-size=1280,720",
         "--disable-background-timer-throttling",
         "--disable-renderer-backgrounding", "about:blank"],
        stdout=open(os.path.join(SCRATCH, "chrome.log"), "wb"),
        stderr=subprocess.STDOUT)
    for _ in range(40):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=2)
            return proc
        except Exception:
            time.sleep(1)
    proc.kill()
    raise SystemExit("FATAL: CDP never came up")


# 一次 evaluate 做完所有测量 —— 中间没有 CDP 往返。
MEASURE = """
(()=>new Promise(res=>{
  const c=document.getElementById('vcw-canvas');
  const cv=document.createElement('canvas');cv.width=c.width;cv.height=c.height;
  const g=cv.getContext('2d');
  const out={w:c.width,h:c.height,cols:[],samples:[]};
  let n=0;
  const tick=()=>{
    g.clearRect(0,0,cv.width,cv.height);
    g.drawImage(c,0,0);
    if(n===0){
      for(const x of [80,320,640,960,1200]){
        const d=g.getImageData(x,0,1,c.height).data;
        const u=new Set(); let mn=[255,255,255],mx=[0,0,0];
        for(let i=0;i<d.length;i+=4){
          u.add((d[i]>>4)+','+(d[i+1]>>4)+','+(d[i+2]>>4));
          for(let k=0;k<3;k++){mn[k]=Math.min(mn[k],d[i+k]);mx[k]=Math.max(mx[k],d[i+k]);}}
        out.cols.push({x,bands:u.size,mn,mx});
      }
      // 全图色带数
      const all=g.getImageData(0,0,c.width,c.height).data;
      const A=new Set(); let alpha0=0;
      for(let i=0;i<all.length;i+=4){
        A.add((all[i]>>4)+','+(all[i+1]>>4)+','+(all[i+2]>>4));
        if(all[i+3]===0) alpha0++;
      }
      out.totalBands=A.size; out.alpha0Frac=+(alpha0/(all.length/4)).toFixed(4);
    }
    const px=g.getImageData(0,0,4,1).data;
    out.samples.push([px[0],px[1],px[2],px[3]]);
    if(++n>=6) return res(out);
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}))()
"""


async def main():
    proc = launch()
    try:
        target = json.loads(urllib.request.urlopen(urllib.request.Request(
            f"http://127.0.0.1:{PORT}/json/new?about:blank", method="PUT"),
            timeout=30).read())
        async with websockets.connect(target["webSocketDebuggerUrl"],
                                      max_size=80 * 1024 * 1024) as ws:
            d = Driver(ws)
            reader = d.start_reader()
            await d.cmd("Runtime.enable")
            await d.cmd("Log.enable")
            await d.cmd("Page.enable")
            await d.cmd("Page.addScriptToEvaluateOnNewDocument", {"source": HOOK})
            await d.cmd("Page.navigate", {"url":
                       "http://127.0.0.1:8877/index.html?res=1.0&cb=" + str(time.time())})

            # 让它自跑 45 秒,期间**不发任何 CDP 命令**。reader 一直在
            # 收,所以 keepalive 不会把连接掐掉。
            await asyncio.sleep(45)

            out = {}
            out["console"] = [
                " ".join(str(a.get("value", a.get("description", "")))
                         for a in e["params"].get("args", []))
                for e in d.events if e.get("method") == "Runtime.consoleAPICalled"
            ][-25:]
            out["ready"] = await d.ev(
                "(()=>{const v=window.__vcw;return {frames:v.frames,"
                "gpuSubmitted:v.gpuSubmitted,backend:v.backend};})()")
            out["hooks"] = await d.ev("(()=>window.__H)()")
            out["measure"] = await d.ev(MEASURE)
            # ⭐ 垂直剖面:判断「是否真的上下颠倒」,以及地面在不在。
            # 天空的蓝 (≈112,178,235) 应该集中在**上半部**,地面/沥青的
            # 暗灰应该集中在**下半部**。若暗色在顶部、蓝色在底部,那就是
            # 真的翻了。
            out["vprofile"] = await d.ev("""
              (()=>new Promise(res=>{
                const c=document.getElementById('vcw-canvas');
                const cv=document.createElement('canvas');
                cv.width=c.width; cv.height=c.height;
                const g=cv.getContext('2d');
                requestAnimationFrame(()=>{
                  g.drawImage(c,0,0);
                  const d=g.getImageData(0,0,c.width,c.height).data;
                  const rows=[]; const N=16;
                  for(let i=0;i<N;i++){
                    const y0=Math.floor(c.height*i/N), y1=Math.floor(c.height*(i+1)/N);
                    let r=0,gg=0,b=0,n=0,dark=0,sky=0;
                    for(let y=y0;y<y1;y+=3){
                      for(let x=0;x<c.width;x+=3){
                        const o=(y*c.width+x)*4;
                        r+=d[o];gg+=d[o+1];b+=d[o+2];n++;
                        const lum=0.299*d[o]+0.587*d[o+1]+0.114*d[o+2];
                        if(lum<90) dark++;
                        // 天空蓝:蓝显著高于红
                        if(d[o+2]-d[o]>40 && d[o+1]>120) sky++;
                      }
                    }
                    rows.push({y0, r:Math.round(r/n), g:Math.round(gg/n),
                               b:Math.round(b/n),
                               darkPct:Math.round(100*dark/n),
                               skyPct:Math.round(100*sky/n)});
                  }
                  res(rows);
                });
              }))()
            """)
            out["errors"] = await d.ev("(()=>window.__H.errors||[])()")

            rect = await d.ev(
                "(()=>{const c=document.getElementById('vcw-canvas');"
                "const r=c.getBoundingClientRect();"
                "return {x:r.x,y:r.y,width:r.width,height:r.height};})()")
            os.makedirs(SHOTS, exist_ok=True)
            shot = await d.cmd("Page.captureScreenshot", {"format": "png", "clip": {
                "x": rect["x"], "y": rect["y"], "width": rect["width"],
                "height": rect["height"], "scale": 1}})
            with open(os.path.join(SHOTS, "webgpu-slice.png"), "wb") as fh:
                fh.write(base64.b64decode(shot["data"]))
            reader.cancel()
            print(json.dumps(out, indent=2, ensure_ascii=False))
    finally:
        subprocess.run(["pkill", "-9", "-f", f"remote-debugging-port={PORT}"],
                       capture_output=True)
        proc.kill()


if __name__ == "__main__":
    asyncio.run(main())
