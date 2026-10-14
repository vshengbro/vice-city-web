//! WebGL2 对照路径。
//!
//! 目的只有一个:**同口径**。同一个三角形顶点、同一块画布、同一套
//! 像素统计、同一段帧率测量区间,换成 WebGL2 再跑一遍。只有这样
//! 「WebGPU 更快 / 更慢」才有意义 —— 三角形小到两边都被
//! `submit` 的固定开销主导,没有对照就看不出任何东西。


use super::*;

impl WebglReport {
    /// Backend identifier this control report was produced by.
    ///
    /// # Returns
    ///
    /// - `&str` - Always `webgl2`.
    pub(crate) fn get_backend(&self) -> &str {
        &self.backend
    }

    /// adapter 描述;WebGL2 恒为 `None`(WebGL2 没这个概念)。
    ///
    /// # Returns
    ///
    /// - `&Option<String>` - Always `None` for WebGL2.
    pub(crate) fn get_adapter(&self) -> &Option<String> {
        &self.adapter
    }

    /// 判据:非清屏色像素数 > 0。
    ///
    /// # Returns
    ///
    /// - `bool` - `true` when at least one pixel differs from the clear
    ///   colour.
    pub(crate) fn get_triangle_drawn(&self) -> bool {
        self.triangle_drawn
    }

/// Count of pixels that differ from the clear colour.
///
    /// # Returns
    ///
    /// - `u32` - Count of pixels that are not the clear colour.
    pub(crate) fn get_non_clear_pixels(&self) -> u32 {
        self.non_clear_pixels
    }

/// Total pixels in the readback, clear colour included.
///
    /// # Returns
    ///
    /// - `u32` - Total pixels in the readback, clear colour included.
    pub(crate) fn get_total_pixels(&self) -> u32 {
        self.total_pixels
    }

    /// 非清屏像素占全图百分比。
    ///
    /// # Returns
    ///
    /// - `f64` - Percentage of the image covered by the triangle.
    pub(crate) fn get_coverage(&self) -> f64 {
        self.coverage
    }

/// Per-channel mean computed over the whole readback.
///
    /// # Returns
    ///
    /// - `[f64; 3]` - Per-channel mean over the whole readback.
    pub(crate) fn get_mean_rgb(&self) -> [f64; 3] {
        self.mean_rgb
    }

/// Number of distinct RGBA values seen in the readback.
///
    /// # Returns
    ///
    /// - `u32` - Number of distinct RGBA values seen in the readback.
    pub(crate) fn get_distinct_colors(&self) -> u32 {
        self.distinct_colors
    }

/// Largest channel value found anywhere in the readback.
///
    /// # Returns
    ///
    /// - `u8` - Largest channel value anywhere in the readback.
    pub(crate) fn get_max_channel(&self) -> u8 {
        self.max_channel
    }

/// Submit-rate FPS measured over the benchmark window.
///
    /// # Returns
    ///
    /// - `f64` - Submit-rate FPS over the measured window.
    pub(crate) fn get_fps(&self) -> f64 {
        self.fps
    }

/// Canvas width in pixels.
///
    /// # Returns
    ///
    /// - `u32` - Canvas width in pixels.
    pub(crate) fn get_width(&self) -> u32 {
        self.width
    }

/// Canvas height in pixels.
///
    /// # Returns
    ///
    /// - `u32` - Canvas height in pixels.
    pub(crate) fn get_height(&self) -> u32 {
        self.height
    }

    /// 冷启动总耗时(含建 context + 编译着色器)。
    ///
    /// # Returns
    ///
    /// - `f64` - Milliseconds from entry to pixel readback.
    pub(crate) fn get_total_ms(&self) -> f64 {
        self.total_ms
    }

/// Diagnostics collected during the run.
///
    /// # Returns
    ///
    /// - `&[String]` - Collected diagnostics, `GL_RENDERER` always included.
    pub(crate) fn get_errors(&self) -> &[String] {
        &self.errors
    }

    /// 转成 JS 对象,字段名与 `crate::WebglReport::to_js_webgl2` 一致。
    ///
    /// 所有字段读取都走上面的 accessor(§17.12);JS 键名是 `probe.py`
    /// 经 CDP 读取的协议,改名等于改协议,所以这里只改读取路径。
    ///
    /// # Returns
    ///
    /// - `JsValue` - A `JsValue` value.
    pub fn to_js_webgl2(&self) -> JsValue {
        let out: JsValue = js_sys::Object::new().into();
        crate::set(&out, "backend", &JsValue::from_str(self.get_backend()));
        crate::set(
            &out,
            "adapter",
            &match self.get_adapter() {
                Some(text) => JsValue::from_str(text),
                None => JsValue::NULL,
            },
        );
        crate::set(
            &out,
            "triangle_drawn",
            &JsValue::from_bool(self.get_triangle_drawn()),
        );
        crate::set(
            &out,
            "non_clear_pixels",
            &JsValue::from_f64(f64::from(self.get_non_clear_pixels())),
        );
        crate::set(
            &out,
            "total_pixels",
            &JsValue::from_f64(f64::from(self.get_total_pixels())),
        );
        crate::set(&out, "coverage_pct", &JsValue::from_f64(self.get_coverage()));
        let mean: JsValue = js_sys::Array::of3(
            &JsValue::from_f64(self.get_mean_rgb()[0]),
            &JsValue::from_f64(self.get_mean_rgb()[1]),
            &JsValue::from_f64(self.get_mean_rgb()[2]),
        )
        .into();
        crate::set(&out, "mean_rgb", &mean);
        crate::set(
            &out,
            "distinct_colors",
            &JsValue::from_f64(f64::from(self.get_distinct_colors())),
        );
        crate::set(
            &out,
            "max_channel",
            &JsValue::from_f64(f64::from(self.get_max_channel())),
        );
        crate::set(&out, "fps", &JsValue::from_f64(self.get_fps()));
        crate::set(&out, "width", &JsValue::from_f64(f64::from(self.get_width())));
        crate::set(&out, "height", &JsValue::from_f64(f64::from(self.get_height())));
        crate::set(&out, "total_ms", &JsValue::from_f64(self.get_total_ms()));
        let errors: JsValue = self
            .get_errors()
            .iter()
            .map(|message: &String| JsValue::from_str(message))
            .collect::<js_sys::Array>()
            .into();
        crate::set(&out, "errors", &errors);
        out
    }
}
