//! 探针用的 WGSL。
//!
//! 刻意做成**最小**:一个 `@vertex` 把 NDC 透传,一个 `@fragment`
//! 返回固定品红。没有任何 uniform / texture / bind group —— 这样
//! 「三角形没画出来」就只可能来自几何、剔除或深度,不会掺进
//! 绑定不匹配这类噪声(那类失败在 WebGPU 上是静默零片元)。
//!
//! 顶点/片元合成**一个 module** 再取两个 entry point,省一次编译。

/// 顶点着色器:输入 `@location(0) vec2<f32>`(NDC),原样输出。
///
/// `@builtin(position)` 会在片元阶段变成像素坐标,由 fragment 忽略。
use super::*;
pub(crate) const VERT_WGSL: &str = r#"
struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
/// Body of the `vs_main` free function.
///
/// # Arguments
///
/// - `vec2<f32>` - A `vec2<f32>` parameter.
///
/// # Returns
///
/// - `VsOut` - A `VsOut` value.
fn vs_main(@location(0) in_position: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip_position = vec4<f32>(in_position, 0.0, 1.0);
    return out;
}
"#;

/// 片元着色器:全屏返回同一个品红。
///
/// 选品红(1.0, 0.25, 0.65)而不是纯白,是为了让「非黑像素」这个
/// 判据在 8bit 量化后依然没有歧义。
pub(crate) const FRAG_WGSL: &str = r#"
struct FsOut {
    @location(0) color: vec4<f32>,
};

@fragment
/// Body of the `fs_main` free function.
///
/// # Returns
///
/// - `FsOut` - A `FsOut` value.
fn fs_main() -> FsOut {
    var out: FsOut;
    out.color = vec4<f32>(1.0, 0.25, 0.65, 1.0);
    return out;
}
"#;
