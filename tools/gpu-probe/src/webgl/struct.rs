//! WebGL2 对照路径。
//!
//! 目的只有一个:**同口径**。同一个三角形顶点、同一块画布、同一套
//! 像素统计、同一段帧率测量区间,换成 WebGL2 再跑一遍。只有这样
//! 「WebGPU 更快 / 更慢」才有意义 —— 三角形小到两边都被
//! `submit` 的固定开销主导,没有对照就看不出任何东西。


use super::*;

pub struct WebglReport {
    pub backend: String,
    pub adapter: Option<String>,
    pub triangle_drawn: bool,
    pub non_clear_pixels: u32,
    pub total_pixels: u32,
    pub coverage: f64,
    pub mean_rgb: [f64; 3],
    pub distinct_colors: u32,
    pub max_channel: u8,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// 冷启动总耗时(含建 context + 编译着色器),与稳态 `fps` 分开看。
    pub total_ms: f64,
    pub errors: Vec<String>,
}
