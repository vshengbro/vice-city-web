// 逐个排除 euv 的 WebGPU 链路,定位「三角形没画出来」的确切环节。
//
// ## 已知的失败范围
//
// `raw.js` 在同一台机器、同一组 flag、同一张页面上把三角形画出来了
// (coverage 16.51%),而 euv 的 `WebGpuRenderer` 读回全 0。所以硬件、
// flag、页面、wasm 全部无罪,问题在 euv 内部。本脚本用**同一份
// euv API**、**同一份 WGSL**、**同一组顶点**做二分:
//
//   A. 画到交换链,截图看 → 区分「draw 本身没生效」与「只有读回坏」
//   B. 画到离屏纹理,读回 → 与 A 对照,排除 readback 的锅
//   C. 每一步都检查 handle 是否 `undefined`
//
// 结论写进 `window.__bisect`,由 `probe.py` 读走。

window.__bisect = (async () => {
  const out = { steps: {} };
  const note = (k, v) => { out.steps[k] = v; };

  try {
    // ---- 与 Rust 侧完全一致的初始化顺序 ---------------------------
    if (typeof navigator.gpu === "undefined") {
      return { ...out, error: "no navigator.gpu" };
    }
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) return { ...out, error: "no adapter" };
    const device = await adapter.requestDevice();

    const canvas = document.getElementById("bisect-canvas");
    const context = canvas.getContext("webgpu");
    if (!context) return { ...out, error: "no webgpu context" };
    const format = navigator.gpu.getPreferredCanvasFormat();
    context.configure({ device, format, alphaMode: "opaque" });
    note("format", format);

    // ---- 与 `wgsl.rs` 逐字一致的 WGSL ---------------------------
    const module = device.createShaderModule({
      code: `
struct VsOut { @builtin(position) clip_position: vec4<f32> };
@vertex fn vs_main(@location(0) in_position: vec2<f32>) -> VsOut {
  var out: VsOut; out.clip_position = vec4<f32>(in_position, 0.0, 1.0); return out;
}
struct FsOut { @location(0) color: vec4<f32> };
@fragment fn fs_main() -> FsOut {
  var out: FsOut; out.color = vec4<f32>(1.0, 0.25, 0.65, 1.0); return out;
}
`});

    const vbo = device.createBuffer({
      size: 24, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
    device.queue.writeBuffer(vbo, 0,
      new Float32Array([-0.6, -0.5, 0.6, -0.5, 0.0, 0.6]));
    const ibo = device.createBuffer({
      size: 12, usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST });
    device.queue.writeBuffer(ibo, 0, new Uint32Array([0, 1, 2]));

    const buildPipeline = (f) => device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs_main",
        buffers: [{ arrayStride: 8, stepMode: "vertex",
          attributes: [{ shaderLocation: 0, offset: 0, format: "float32x2" }] }] },
      primitive: { topology: "triangle-list", cullMode: "none" },
      fragment: { module, entryPoint: "fs_main", targets: [{ format: f }] },
    });

    // ---- 步骤 A:交换链 + 截图判定 -------------------------------
    // 不用 readPixels —— WebGPU 的 canvas context 不支持。
    // 判据交给 `probe.py` 的截图(那张 `bisect-canvas.png`)。
    const swapPipeline = buildPipeline(format);
    {
      const enc = device.createCommandEncoder();
      const pass = enc.beginRenderPass({ colorAttachments: [{
        view: context.getCurrentTexture().createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear", storeOp: "store" }] });
      pass.setPipeline(swapPipeline);
      pass.setVertexBuffer(0, vbo);
      pass.setIndexBuffer(ibo, "uint32");
      pass.drawIndexed(3, 1, 0, 0, 0);
      pass.end();
      device.queue.submit([enc.finish()]);
      note("A_swapchain_submitted", true);
    }
    await device.queue.onSubmittedWorkDone();

    // ---- 步骤 B:离屏 + copyTextureToBuffer 读回 ------------------
    // 与 Rust 侧 `run_webgpu` 用**完全相同**的尺寸/对齐/参数,
    // 唯一区别是不经 euv。所以 B 成功就说明 euv 的 copy/read 有问题。
    const W = 256, H = 256;
    const readPipeline = buildPipeline("rgba8unorm");
    const target = device.createTexture({
      size: [W, H, 1], format: "rgba8unorm",
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
    const bytesPerRow = Math.ceil(W * 4 / 256) * 256;   // 1024
    const staging = device.createBuffer({
      size: bytesPerRow * H,
      usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
    {
      const enc = device.createCommandEncoder();
      const pass = enc.beginRenderPass({ colorAttachments: [{
        view: target.createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear", storeOp: "store" }] });
      pass.setPipeline(readPipeline);
      pass.setVertexBuffer(0, vbo);
      pass.setIndexBuffer(ibo, "uint32");
      pass.drawIndexed(3, 1, 0, 0, 0);
      pass.end();
      enc.copyTextureToBuffer(
        { texture: target },
        { buffer: staging, bytesPerRow, rowsPerImage: H },
        [W, H, 1]);
      device.queue.submit([enc.finish()]);
    }
    await staging.mapAsync(GPUMapMode.READ);
    const bytes = new Uint8Array(staging.getMappedRange()).slice();
    staging.unmap();

    let nonClear = 0, maxCh = 0;
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * bytesPerRow + x * 4;
        if (bytes[i] || bytes[i + 1] || bytes[i + 2]) nonClear++;
        maxCh = Math.max(maxCh, bytes[i], bytes[i + 1], bytes[i + 2]);
      }
    }
    note("B_offscreen_non_clear", nonClear);
    note("B_offscreen_max_channel", maxCh);
    note("B_offscreen_drawn", nonClear > 0);
    return out;
  } catch (e) {
    return { ...out, error: String((e && e.message) || e) };
  }
})();
