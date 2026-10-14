// 纯 JS 的 WebGPU 三角形 —— **不经过 euv 一行代码**。
//
// ## 为什么需要这一份
//
// 探针的主路径(euv-engine 的 `WebGpuRenderer`)报 `triangle_drawn: false`
// 时,「三角形没画出来」有**两种**截然不同的成因:
//
//   A. 这台机器 / 这个 flag 组合下 WebGPU 根本画不出东西
//      —— 那么整个迁移方案在这台机器上无法验证;
//   B. euv 的渲染 / 读回链路有 bug(canvas context 拿不到正确 view、
//      `copyTextureToBuffer` 静默 no-op、buffer 没解绑 ...)
//      —— 那么硬件没问题,是引擎要修/绕。
//
// 这两种情况在 `window.__probe` 里的数字**长得一模一样**(全 0),
// 光看那份报告没法区分。所以这里用浏览器原生 API 手写一遍同样的
// 三角形 + 同样的 `copyTextureToBuffer` 读回:
// 如果它出像素 → 硬件与 flag 没问题,是 euv 的问题(B);
// 如果它也全 0 → 问题在环境(A)。
//
// 结果挂在 `window.__raw`,由 `probe.py` 读走。

window.__raw = (async () => {
  const out = { backend: "raw-webgpu-js" };
  try {
    if (typeof navigator.gpu === "undefined") {
      return { ...out, error: "no navigator.gpu" };
    }
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) {
      return { ...out, error: "requestAdapter() returned null" };
    }
    const device = await adapter.requestDevice();
    // 每一处 validation 错误都记下来。默认状态下 WebGPU 只往
    // console 写,JS 侧拿不到 —— 静默失败正是要排查的东西。
    device.addEventListener?.("uncapturederror", (e) => {
      out.uncaptured = String(e.error?.message || e.error);
    });

    const W = 256, H = 256;
    // 必须是**自己那张** canvas。`getContext("webgpu")` 一旦在某个
    // canvas 上被调用过,那张画布的 context 类型就定死了 —— 再拿它
    // 给 euv 的 `WebGpuRenderer` 用会拿到 `null`,或者反过来。
    // 两套路径各占一张画布,才能在同一页面上互为对照。
    const canvas = document.getElementById("raw-canvas");
    const context = canvas.getContext("webgpu");
    if (!context) {
      return { ...out, error: "getContext('webgpu') returned null" };
    }
    const format = navigator.gpu.getPreferredCanvasFormat();
    context.configure({ device, format, alphaMode: "opaque" });
    out.swapchain_format = format;

    // ---- WGSL:与 Rust 侧 `wgsl.rs` 逐字一致的着色器 --------------
    const module = device.createShaderModule({
      label: "raw-probe",
      code: `
struct VsOut { @builtin(position) clip_position: vec4<f32> };
@vertex fn vs_main(@location(0) in_position: vec2<f32>) -> VsOut {
  var out: VsOut;
  out.clip_position = vec4<f32>(in_position, 0.0, 1.0);
  return out;
}
struct FsOut { @location(0) color: vec4<f32> };
@fragment fn fs_main() -> FsOut {
  var out: FsOut;
  out.color = vec4<f32>(1.0, 0.25, 0.65, 1.0);
  return out;
}
`});

    const vertexData = new Float32Array([-0.6, -0.5, 0.6, -0.5, 0.0, 0.6]);
    const vbo = device.createBuffer({
      size: vertexData.byteLength,
      usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST,
    });
    device.queue.writeBuffer(vbo, 0, vertexData);

    const ibo = device.createBuffer({
      size: 12,
      usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST,
    });
    device.queue.writeBuffer(ibo, 0, new Uint32Array([0, 1, 2]));

    const buildPipeline = (colorFormat) => device.createRenderPipeline({
      layout: "auto",
      vertex: {
        module, entryPoint: "vs_main",
        buffers: [{
          arrayStride: 8,
          stepMode: "vertex",
          attributes: [{ shaderLocation: 0, offset: 0, format: "float32x2" }],
        }],
      },
      primitive: { topology: "triangle-list", cullMode: "none" },
      fragment: {
        module, entryPoint: "fs_main",
        targets: [{ format: colorFormat }],
      },
    });

    // ---- 路径 1:画到交换链(截图能看见的那张) ---------------------
    const swapPipeline = buildPipeline(format);
    {
      const encoder = device.createCommandEncoder();
      const pass = encoder.beginRenderPass({
        colorAttachments: [{
          view: context.getCurrentTexture().createView(),
          clearValue: { r: 0, g: 0, b: 0, a: 1 },
          loadOp: "clear", storeOp: "store",
        }],
      });
      pass.setPipeline(swapPipeline);
      pass.setVertexBuffer(0, vbo);
      pass.setIndexBuffer(ibo, "uint32");
      pass.drawIndexed(3, 1, 0, 0, 0);
      pass.end();
      device.queue.submit([encoder.finish()]);
      out.swapchain_drawn = true;
    }
    await device.queue.onSubmittedWorkDone();

    // ---- 路径 2:画到离屏纹理再读回(数值判据) ---------------------
    const readPipeline = buildPipeline("rgba8unorm");
    const target = device.createTexture({
      size: [W, H, 1],
      format: "rgba8unorm",
      usage: GPUTextureUsage.RENDER_ATTACHMENT |
             GPUTextureUsage.COPY_SRC,
    });
    const bytesPerRow = Math.ceil(W * 4 / 256) * 256;
    const staging = device.createBuffer({
      size: bytesPerRow * H,
      usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
    });
    {
      const encoder = device.createCommandEncoder();
      const pass = encoder.beginRenderPass({
        colorAttachments: [{
          view: target.createView(),
          clearValue: { r: 0, g: 0, b: 0, a: 1 },
          loadOp: "clear", storeOp: "store",
        }],
      });
      pass.setPipeline(readPipeline);
      pass.setVertexBuffer(0, vbo);
      pass.setIndexBuffer(ibo, "uint32");
      pass.drawIndexed(3, 1, 0, 0, 0);
      pass.end();
      encoder.copyTextureToBuffer(
        { texture: target },
        { buffer: staging, bytesPerRow, rowsPerImage: H },
        [W, H, 1]);
      device.queue.submit([encoder.finish()]);
    }
    await staging.mapAsync(GPUMapMode.READ);
    const bytes = new Uint8Array(staging.getMappedRange()).slice();
    staging.unmap();

    // 与 Rust 侧 `PixelStats::from_rgba` 完全同口径:按行跳过
    // 256 字节对齐填充,统计非黑像素。
    let nonClear = 0, maxCh = 0;
    const sum = [0, 0, 0];
    const seen = new Set();
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * bytesPerRow + x * 4;
        const r = bytes[i], g = bytes[i + 1], b = bytes[i + 2];
        if (r > maxCh) maxCh = r;
        if (g > maxCh) maxCh = g;
        if (b > maxCh) maxCh = b;
        sum[0] += r; sum[1] += g; sum[2] += b;
        if (r !== 0 || g !== 0 || b !== 0) nonClear++;
        seen.add((r << 24) | (g << 16) | (b << 8) | bytes[i + 3]);
      }
    }
    out.triangle_drawn = nonClear > 0;
    out.non_clear_pixels = nonClear;
    out.total_pixels = W * H;
    out.coverage_pct = nonClear / (W * H) * 100;
    out.mean_rgb = sum.map((v) => v / (W * H));
    out.distinct_colors = seen.size;
    out.max_channel = maxCh;
    return out;
  } catch (e) {
    return { ...out, error: String(e && e.message || e) };
  }
})();
