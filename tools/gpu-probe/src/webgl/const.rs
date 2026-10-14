//! WebGL2 对照路径。
//!
//! 目的只有一个:**同口径**。同一个三角形顶点、同一块画布、同一套
//! 像素统计、同一段帧率测量区间,换成 WebGL2 再跑一遍。只有这样
//! 「WebGPU 更快 / 更慢」才有意义 —— 三角形小到两边都被
//! `submit` 的固定开销主导,没有对照就看不出任何东西。


use super::*;

/// Backend name reported for the WebGL2 control path.
pub(crate) const BACKEND_WEBGL2: &str = "webgl2";
pub(crate) const WEBGL_VERT_GLSL: &str = "#version 300 es
layout(location = 0) in vec2 in_position;
void main() {
    gl_Position = vec4(in_position, 0.0, 1.0);
}
";

pub(crate) const WEBGL_FRAG_GLSL: &str = "#version 300 es
precision highp float;
out vec4 out_color;
void main() {
    out_color = vec4(1.0, 0.25, 0.65, 1.0);
}
";

const TRI_XY: [f32; 6] = [-0.6, -0.5, 0.6, -0.5, 0.0, 0.6];

const BENCH_FRAMES: u32 = crate::BENCH_FRAMES;

const WARMUP_FRAMES: u32 = crate::WARMUP_FRAMES;
