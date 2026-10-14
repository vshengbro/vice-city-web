//! crate 根级字符串常量(§1.3c 的单一来源)。
//!
//! 这里只放被 lib.rs 直接使用的字面量。`webgl` 子模块自己的常量
//! 在 `webgl/const.rs`,WGSL 着色器在 `wgsl/const.rs` —— 三个文件各
//! 管一段,互不越界。

/// 报告里 WebGPU 后端的标识,`window.__probe.webgpu` 的来源。
pub(crate) const BACKEND_WEBGPU: &str = "webgpu";

/// `js_sys::Function::call0` 抛异常时的错误文案。两次 `requestAdapter`
/// 探测走同一条路,共用一个常量。
pub(crate) const ERR_CALL_FAILED: &str = "call failed";

/// macOS 上 `getPreferredCanvasFormat()` 的返回值。
pub(crate) const FORMAT_BGRA8UNORM: &str = "bgra8unorm";

/// 离屏读回纹理的格式,也是未知交换链格式时的兜底值。
pub(crate) const FORMAT_RGBA8UNORM: &str = "rgba8unorm";
