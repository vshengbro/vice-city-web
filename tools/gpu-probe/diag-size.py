"""三件事一次问清。

1. **canvas 每帧有没有被改尺寸** —— 改尺寸会把 canvas 重置成透明,
   而症状正是「读回 alpha 0、截图里只有页面底色」。这是目前唯一
   能同时解释「我的 clear 能落盘」与「游戏的帧什么都不产生」的机制:
   我的 clear 落盘后立刻读回,游戏的下一帧再把它清掉。
2. **getCurrentTexture() 每次是不是新对象** —— 缓存住 view 就不 present。
3. **在 rAF 里、游戏那一帧之后立刻读回** —— 排除「读回时机不对」。
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
SCRATCH = "/Users/sqs/.hermes/cache/scratch/vcw-diag-size"
SHOTS = "/Users/sqs/code/vice-city-web/tools/gpu-probe/shots"

HOOK = r"""
(() => {
  const H = window.__H = { resizes: [], gct: [], errors: [], frames: 0 };
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

  // ⚠️ 尺寸赋值钩子:canvas 一旦被改尺寸,绘制缓冲会被**清成透明**,
  // 这正是 alpha=0 的成因。
  const dw = Object.getOwnPropertyDescriptor(
    HTMLCanvasElement.prototype, 'width');
  const dh = Object.getOwnPropertyDescriptor(
    HTMLCanvasElement.prototype, 'height');
  Object.defineProperty(HTMLCanvasElement.prototype, 'width', {
    get: dw.get,
    set(v) {
      if (this.id === 'vcw-canvas') {
        H.resizes.push({prop:'width', to:v, from:dw.get.call(this),
                        t:Math.round(performance.now())});
        if (H.resizes.length > 30) H.resizes.shift();
      }
      return dw.set.call(this, v);
    },
  });
  Object.defineProperty(HTMLCanvasElement.prototype, 'height', {
    get: dh.get,
    set(v) {
      if (this.id === 'vcw-canvas') {
        H.resizes.push({prop:'height', to:v, from:dh.get.call(this),
                        t:Math.round(performance.now())});
        if (H.resizes.length > 30) H.resizes.shift();
      }
      return dh.set.call(this, v);
    },
  });

  const gt = GPUCanvasContext.prototype.getCurrentTexture;
  GPUCanvasContext.prototype.getCurrentTexture = function () {
    const t = gt.call(this);
    H.gct.push({w:t.width, h:t.height,
                cw:this.canvas?this.canvas.width:null,
                ch:this.canvas?this.canvas.height:null});
    if (H.gct.length > 30) H.gct.shift();
    return t;
  };

  const sb = GPUQueue.prototype.submit;
  GPUQueue.prototype.submit = function (a) { H.frames++; return sb.call(this, a); };

  // 逐批记录:哪个 index buffer(≈mesh)、几个实例、firstInstance。
  // 用来确认「地面批次到底有没有被画」—— 画面缺地面时,这里能直接
  // 看到地面那个 mesh 是缺席还是画了 0 实例。
  const si = GPURenderPassEncoder.prototype.setIndexBuffer;
  GPURenderPassEncoder.prototype.setIndexBuffer = function (b) {
    H.nSetIB = (H.nSetIB || 0) + 1;
    window.__CURIB = b; return si.call(this, b);
  };
  const sv = GPURenderPassEncoder.prototype.setVertexBuffer;
  GPURenderPassEncoder.prototype.setVertexBuffer = function (s, b) {
    H.nSetVB = (H.nSetVB || 0) + 1; return sv.call(this, s, b);
  };
  const ep = GPURenderPassEncoder.prototype.end;
  GPURenderPassEncoder.prototype.end = function () { H.nEnd = (H.nEnd || 0) + 1; return ep.call(this); };
  const di = GPURenderPassEncoder.prototype.drawIndexed;
  GPURenderPassEncoder.prototype.drawIndexed = function (
    idxCount, instCount, firstIdx, baseVtx, firstInst) {
    H.drawn = H.drawn || {};
    if (!window.__IBIDS) window.__IBIDS = new Map();
    if (!window.__IBIDS.has(window.__CURIB))
      window.__IBIDS.set(window.__CURIB, window.__IBIDS.size + 1);
    const id = window.__IBIDS.get(window.__CURIB);
    const p = H.drawn[id] || { draws: 0, inst: 0, idxCount: idxCount, firstInst: [] };
    p.draws++;
    p.inst += instCount;
    p.idxCount = idxCount;
    if (p.firstInst.length < 3) p.firstInst.push(firstInst);
    H.drawn[id] = p;
    return di.call(this, idxCount, instCount, firstIdx, baseVtx, firstInst);
  };
})();
"""


class Driver:
    def __init__(self, ws):
        self.ws = ws
        self.mid = 0

    async def cmd(self, method, params=None):
        self.mid += 1
        await self.ws.send(json.dumps({"id": self.mid, "method": method,
                                       "params": params or {}}))
        while True:
            msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=300))
            if msg.get("id") == self.mid:
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


READBACK = """
  (async()=>{
    const c=document.getElementById('vcw-canvas');
    const cv=document.createElement('canvas');cv.width=c.width;cv.height=c.height;
    const g=cv.getContext('2d');g.drawImage(c,0,0);
    const d=g.getImageData(0,0,4,1).data;
    return [d[0],d[1],d[2],d[3]];
  })()
"""


async def main():
    proc = launch()
    try:
        target = json.loads(urllib.request.urlopen(urllib.request.Request(
            f"http://127.0.0.1:{PORT}/json/new?about:blank", method="PUT"),
            timeout=30).read())
        async with websockets.connect(target["webSocketDebuggerUrl"],
                                      max_size=80 * 1024 * 1024) as ws:
            drv = Driver(ws)
            await drv.cmd("Runtime.enable")
            await drv.cmd("Page.enable")
            await drv.cmd("Page.addScriptToEvaluateOnNewDocument", {"source": HOOK})

            async def pump():
                while True:
                    try:
                        json.loads(await asyncio.wait_for(ws.recv(), timeout=2))
                    except (asyncio.TimeoutError, TimeoutError):
                        continue
                    except Exception:
                        return

            t = asyncio.ensure_future(pump())
            await drv.cmd("Page.navigate", {"url":
                       "http://127.0.0.1:8877/index.html?res=1.0&cb=" + str(time.time())})

            # 必须等到**资产传完**再测:前几帧 GPU 侧还没网格,量到的
            # 是「什么都没画」的空画面,会误判成 bug。
            deadline = time.time() + 180
            ok = False
            while time.time() < deadline:
                await asyncio.sleep(5)
                # ⚠️ 键名是 `frames` / `gpuSubmitted`(不是 frame/gpu)——
                # 键名写错会让就绪判断永远失败,量到的是前几帧的空画面。
                s = await drv.ev(
                    "(()=>{const v=window.__vcw;if(!v)return null;"
                    "return {f:v.frames, g:v.gpuSubmitted};})()")
                if s and s.get("f", 0) > 600 and s.get("g", 0) > 0:
                    ok = True
                    break
            out = {}
            out["ready"] = {"ok": ok, "state": s}
            out["resizes"] = await drv.ev("(()=>window.__H.resizes||[])()")
            out["gct_sample"] = await drv.ev("(()=>window.__H.gct||[])()")
            out["submits"] = await drv.ev("(()=>window.__H.frames)()")
            # 逐列看垂直结构:真实场景沿竖直方向会有明显变化,
            # 几块巨大的平色块则每列几乎不变。
            out["columns"] = await drv.ev("""
              (()=>new Promise(res=>{
                const c=document.getElementById('vcw-canvas');
                const cv=document.createElement('canvas');cv.width=c.width;cv.height=c.height;
                const g=cv.getContext('2d');
                requestAnimationFrame(()=>{
                  g.drawImage(c,0,0);
                  const rows=[];
                  for(const x of [80,320,640,960,1200]){
                    const d=g.getImageData(x,0,1,c.height).data;
                    const uniq=new Set(); let mn=[255,255,255],mx=[0,0,0];
                    for(let i=0;i<d.length;i+=4){
                      uniq.add((d[i]>>4)+','+(d[i+1]>>4)+','+(d[i+2]>>4));
                      for(let k=0;k<3;k++){
                        mn[k]=Math.min(mn[k],d[i+k]); mx[k]=Math.max(mx[k],d[i+k]); } }
                    rows.push({x, bands:uniq.size,
                               min:mn, max:mx,
                               top:[d[0],d[1],d[2]],
                               mid:[d[(c.height>>1)*4],d[(c.height>>1)*4+1],d[(c.height>>1)*4+2]],
                               bot:[d[(c.height-1)*4],d[(c.height-1)*4+1],d[(c.height-1)*4+2]]});
                  }
                  res(rows);
                });
              }))()
            """)
            out["errors"] = await drv.ev("(()=>window.__H.errors||[])()")
            # 哪些 mesh 真的被画了、画了几个实例 —— 地面批次在不在里面。
            out["drawn"] = await drv.ev("(()=>window.__H.drawn||null)()")
            out["counts"] = await drv.ev("(()=>{const H=window.__H;return "
                "{nSetIB:H.nSetIB,nSetVB:H.nSetVB,nEnd:H.nEnd,frames:H.frames};})()")
            out["readback_now"] = await drv.ev(READBACK)

            # 在 rAF 里、游戏那一帧渲染完之后立刻读回
            out["readback_in_raf"] = await drv.ev("""
              (()=>new Promise(res=>{
                requestAnimationFrame(()=>{
                  // 游戏的 rAF 注册在更早,同一帧里先跑,所以这里读到的
                  // 就是「游戏刚刚 submit 完」的那张画布。
                  const c=document.getElementById('vcw-canvas');
                  const cv=document.createElement('canvas');
                  cv.width=c.width;cv.height=c.height;
                  const g=cv.getContext('2d');g.drawImage(c,0,0);
                  const d=g.getImageData(0,0,c.width,1).data;
                  let mx=0,mc=0;for(let i=0;i<d.length;i+=4){
                    const s=d[i]+d[i+1]+d[i+2]+d[i+3];
                    if(s>mx){mx=s;mc=i/4;} }
                  res({first:[d[0],d[1],d[2],d[3]],
                       maxSum:mx, atX:mc, w:c.width, h:c.height});
                });
              }))()
            """)

            # 连续多帧在 rAF 内读回:确认是**持续**的真实内容,不是偶发。
            out["raf_series"] = await drv.ev("""
              (()=>new Promise(res=>{
                const c=document.getElementById('vcw-canvas');
                const cv=document.createElement('canvas');cv.width=c.width;cv.height=c.height;
                const g=cv.getContext('2d');
                const out=[]; let n=0;
                const tick=()=>{
                  g.clearRect(0,0,cv.width,cv.height);
                  g.drawImage(c,0,0);
                  const d=g.getImageData(0,0,cv.width,cv.height).data;
                  const nn=d.length/4;
                  let su=[0,0,0,0],distinct=new Set();
                  for(let i=0;i<d.length;i+=4){
                    su[0]+=d[i];su[1]+=d[i+1];su[2]+=d[i+2];su[3]+=d[i+3];
                    if(n===0||distinct.size<400)
                      distinct.add((d[i]>>3)+','+(d[i+1]>>3)+','+(d[i+2]>>3)); }
                  out.push({avg:su.map(v=>Math.round(v/nn)),
                            colors:distinct.size});
                  if(++n>=8) return res(out);
                  requestAnimationFrame(tick);
                };
                requestAnimationFrame(tick);
              }))()
            """)

            rect = await drv.ev(
                "(()=>{const c=document.getElementById('vcw-canvas');"
                "const r=c.getBoundingClientRect();"
                "return {x:r.x,y:r.y,width:r.width,height:r.height};})()")
            os.makedirs(SHOTS, exist_ok=True)
            shot = await drv.cmd("Page.captureScreenshot", {"format": "png", "clip": {
                "x": rect["x"], "y": rect["y"], "width": rect["width"],
                "height": rect["height"], "scale": 1}})
            with open(os.path.join(SHOTS, "webgpu-slice.png"), "wb") as fh:
                fh.write(base64.b64decode(shot["data"]))
            t.cancel()
            print(json.dumps(out, indent=2, ensure_ascii=False))
    finally:
        subprocess.run(["pkill", "-9", "-f", f"remote-debugging-port={PORT}"],
                       capture_output=True)
        proc.kill()


if __name__ == "__main__":
    asyncio.run(main())
