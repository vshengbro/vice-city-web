use super::*;

/// 昼夜预设。三档:正午 / 黄昏 / 夜晚。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DayPhase {
    /// 正午:高角度强白光。
    Noon,
    /// 黄昏:低角度暖橙光。
    Dusk,
    /// 夜晚:冷月光 + 高霓虹强度。
    Night,
}

// ===========================================================================
// 自适应画质
// ===========================================================================
/// 渲染画质档位。
///
/// 降级是**单向**的:只往下走,不自动升回去。否则玩家开着车穿过一片
/// 楼群(掉到 Low)又开回空旷海面,画质会来回抖,比一直低更难受。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityTier {
    /// 全开:阴影 + SSAO + SSR + bloom。
    High,
    /// 关掉 SSR(最贵的一项),保留阴影 / SSAO / bloom。
    Medium,
    /// 只留阴影 + bloom —— SSR 与 SSAO 全关。
    Low,
}

// ===========================================================================
// 后端枚举
// ===========================================================================
/// 实际使用的渲染后端。
pub enum Renderer {
    /// WebGPU(WGSL,instancing)。构造是异步的,`?backend=webgl2` 可跳过。
    WebGpu(Box<crate::webgpu::WebGpuRenderer>),
    /// WebGL2(GLSL ES 3.00,instancing)。
    WebGl(Box<WebGlRenderer>),
    /// Canvas2D 软件渲染(背面剔除 + 画家算法)。
    Software(SoftwareRenderer),
}
