use super::*;

/// 小 helper:把 `get_context` 返回的 `js_sys::Object` 转成 `WebGl2RenderingContext`。
pub(crate) trait DynIntoWebGl {
    /// dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    fn dyn_into_webgl(self) -> WebGl2RenderingContext;
}

/// 小 helper:把 `get_context` 返回的 `js_sys::Object` 转成 `CanvasRenderingContext2d`。
pub(crate) trait DynInto2d {
    /// dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    fn dyn_into_2d(self) -> euv::web_sys::CanvasRenderingContext2d;
}
