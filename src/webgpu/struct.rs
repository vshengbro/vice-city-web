//! 直接对着 `navigator.gpu` 取得的 WebGPU 句柄与 uniform 布局。
//!
//! **为什么全部是 `JsValue` 而不是 `web_sys::GPUDevice`。**
//!
//! web-sys 的 136 个 `Gpu*` 绑定全部锁在
//! `#[cfg(web_sys_unstable_apis)]` 后面(见
//! `web-sys-0.3.105/src/features/gen_GPUTexture.rs`,整个 `extern`
//! 块都在那个 cfg 里面),而且 `GPUBufferUsage` / `GPUTextureUsage`
//! 这些位掩码常量根本没有导出。打开那个开关要改
//! `.github/workflows/pages.yml` 的 RUSTFLAGS,会把 GitHub Actions
//! 的构建一起弄坏。
//!
//! 所以这里走 euv-engine 内部用的同一条路:对象一律当 `JsValue` 拿着,
//! 属性用 [`js_sys::Reflect::get`] / [`js_sys::Reflect::set`] 取,
//! 方法 `dyn_into::<js_sys::Function>()` 再 `call`。这样**不需要任何
//! unstable cfg,也不需要新依赖** —— `js-sys` 与 `wasm-bindgen` 本来
//! 就是本 crate 的直接依赖。

use super::*;

/// 已经 `configure` 好的 WebGPU 上下文。
#[derive(Debug, Clone)]
pub struct GpuContext {
    /// `GPUCanvasContext`,每帧 `getCurrentTexture()` 靠它。
    pub context: JsValue,
    /// `GPUDevice`,管线 / bind group 创建时取用。
    pub device: JsValue,
    /// 画布首选格式(`bgra8unorm` 或 `rgba8unorm`)。
    pub format: String,
}

/// 一个已上传资产在 GPU 侧的句柄。
///
/// 与 `render::GlMesh` 一一对应且**下标语义完全相同** ——
/// [`crate::render::SceneBatch::mesh_index`] 在两个后端里都当这张表的
/// 下标用,所以两张表必须长度相等、顺序一致,否则批次会取到别人的网格。
#[derive(Debug, Clone)]
pub struct GpuMesh {
    /// 顶点缓冲(`GPUBuffer`)。
    pub vertex_buffer: JsValue,
    /// 索引缓冲(`GPUBuffer`)。
    pub index_buffer: JsValue,
    /// `drawIndexed` 的索引数。
    pub index_count: u32,
}

/// 一帧渲染所需的全部参数。
///
/// 打包成结构体而不是 8 个位置参数:`render` 的参数个数已经越过
/// clippy `too_many_arguments` 的阈值,而这里**不能**加 `#[allow]`。
/// 更重要的是这八个参数天然成组(帧常量 / 着色参数 / 视口),
/// 将来补上 `time` / `quality` 时不必再改一次签名。
#[derive(Clone, Copy, Debug)]
pub struct RenderParams<'a> {
    /// 场景(资产 + 批次)。
    pub scene: &'a crate::render::Scene,
    /// 视投影矩阵。
    pub view_proj: &'a Mat4,
    /// 当前光照参数。
    pub lighting: &'a SceneLighting,
    /// 相机眼点。
    pub eye: Vec3,
    /// 画布宽(像素)。
    pub width: u32,
    /// 画布高(像素)。
    pub height: u32,
    /// 近处剔除半径。
    pub near_cull_radius: f32,
}

/// 着色参数 uniform 的字节布局(12 × vec4 = 192 字节)。
///
/// 字段顺序必须与 [`SHADER_FRAGMENT`] 里的 `Shading` 结构逐项一致,
/// 否则读出来的是错位的颜色。统一用 `[f32; 4]` 是因为 WebGPU 的
/// uniform 布局按 16 字节对齐,而本块恰好是 vec4 的整数倍。
#[derive(Clone, Copy, Debug, Default)]
pub struct ShadingUniforms {
    /// `light_dir` + 1 个 pad。
    pub light_dir: [f32; 4],
    /// `light_color` + 1 个 pad。
    pub light_color: [f32; 4],
    /// `ambient` + 1 个 pad。
    pub ambient: [f32; 4],
    /// `sky_color` + 1 个 pad。
    pub sky_color: [f32; 4],
    /// `sky_ambient` + 1 个 pad。
    pub sky_ambient: [f32; 4],
    /// `ground_ambient` + 1 个 pad。
    pub ground_ambient: [f32; 4],
    /// `x` = `ambient_hemi`,其余为 pad。
    pub ambient_hemi: [f32; 4],
    /// `x` = `emissive_gain`,其余为 pad。
    pub emissive_gain: [f32; 4],
    /// `eye` + 1 个 pad。
    pub eye: [f32; 4],
    /// `x` = `fog_start`,`y` = `fog_end`,其余为 pad。
    pub fog: [f32; 4],
    /// `x` = `ao_height`,`y` = `ao_floor`,其余为 pad。
    pub ao_params: [f32; 4],
    /// `x` = `exposure`,`y` = `tone_map_white`,其余为 pad。
    pub exposure_white: [f32; 4],
}

/// 视投影矩阵的字节序(列主序,直接 `writeBuffer`)。
///
/// # Arguments
///
/// - `&Mat4` - 列主序视投影矩阵。
///
/// # Returns
///
/// WebGPU 不可用的原因。
///
/// 原本是一个 `enum` + `impl`,但 §1.3 规定 `struct.rs` 里只允许
/// `struct`(不允许 `enum` / `type`),而它的 `impl` 也不能跟着待在
/// `struct.rs`。改成**只包一个字符串的 struct**,`Display` 由
/// `impl.rs` 里的 `impl std::fmt::Display` 提供 —— 这样
/// `format!("[vcw] WebGPU unavailable: {}", reason)` 照样能写。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct GpuUnavailable {
    /// 人可读的失败原因,例如 [`super::r#const::GPU_ERR_NO_ADAPTER`]。
    ///
    /// 全部取值见 `super::r#const` 里的 `GPU_ERR_*` 系列(§1.3c:
    /// 硬编码字符串的单一来源)。
    pub reason: &'static str,
}

/// 从 JS 对象读一个**属性**(不是方法)。
///
/// `device.queue` 这类成员必须走这里:拿 `call0` 去调它会得到
/// `queue is not a function`。
///
/// # Arguments
///
/// - `&JsValue` - 承载属性的 JS 对象。
/// - `&str` - 属性名。
///
/// # Returns
///
/// - `Result<JsValue, String>` - 属性值。
#[cfg(test)]
mod r#tests {
    use super::*;

    /// 列主序下标:[行][列] -> 线性下标。
    ///
    /// # Arguments
    ///
    /// - `usize` - 行。
    /// - `usize` - 列。
    ///
    /// # Returns
    ///
    /// - `usize` - 线性下标。
    const fn at(row: usize, column: usize) -> usize {
        column * 4 + row
    }

    /// ⚠️ **y 行必须保持不变** —— 一条反直觉的回归测试。
    ///
    /// 直觉上 WebGPU 的 NDC +Y 朝下、OpenGL 朝上,应该把 y 行取反。
    /// 实测那样做会让整幅画面**上下颠倒**:顶部变成沥青、底部变成天空。
    /// 用**已知正确**的 WebGL2 对照帧校准过(见
    /// `tools/gpu-probe/profile-shot.py` 的垂直剖面):正确形态是
    /// 「顶部天空%、底部沥青暗色%」。
    #[test]
    fn frame_bytes_leaves_the_y_row_untouched() {
        let source: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, -1.0, -1.0, //
            0.0, 0.0, 0.0, 0.0,
        ];
        let matrix: Mat4 = Mat4::from_column_major(source);
        let out: [f32; 16] = frame_bytes(&matrix);

        for column in 0..4usize {
            assert_eq!(
                out[at(1, column)],
                source[at(1, column)],
                "y row must NOT be negated (that flips the image upside down)"
            );
        }
        // 投影矩阵第 1 行是 (0, 1, 0, 0):f 必须仍是 +1.0。
        assert_eq!(out[at(1, 1)], 1.0, "clip.y must keep its sign");
    }

    /// 只有 z 行被搬运,x / y / w 三行逐位不变。
    ///
    /// 这条钉住的就是那个把整场景压成横带的 bug:曾经的循环写的是
    /// `out[8 + column]` / `out[12 + column]`,取到的是**列**而不是
    /// **行**,于是 clip.x / clip.y 的 z 系数被砍半。列主序下第 2 行是
    /// 下标 2/6/10/14,第 3 行是 3/7/11/15 —— 只有逐位比对才能发现。
    #[test]
    fn frame_bytes_only_remaps_the_depth() {
        // 一个带透视的 proj·view:第 2 列有非零的 z 系数,第 3 列的
        // w 行是 1 —— 正是会被旧写法误伤的那两个位置。
        let source: [f32; 16] = [
            1.2, 0.0, 0.0, 0.0, //
            0.0, 1.6, 0.0, 0.0, //
            0.0, 0.0, -1.0, -1.0, //
            0.0, 0.0, -2.0, 0.0,
        ];
        let matrix: Mat4 = Mat4::from_column_major(source);
        let out: [f32; 16] = frame_bytes(&matrix);

        for column in 0..4usize {
            assert_eq!(out[at(0, column)], source[at(0, column)], "row 0 moved");
            assert_eq!(out[at(1, column)], source[at(1, column)], "row 1 moved");
            // 第 2 行:必须是 0.5·z + 0.5·w。
            let expected: f32 =
                0.5 * source[at(2, column)] + 0.5 * source[at(3, column)];
            assert_eq!(out[at(2, column)], expected, "row 2 not remapped");
            // 第 3 行:只有 z 行参与搬运,w 行自己保持不变。
            assert_eq!(out[at(3, column)], source[at(3, column)], "row 3 moved");
        }
    }

    /// 旧写法会砍半 clip.x / clip.y 的 z 系数,这条直接盯住它。
    #[test]
    fn frame_bytes_keeps_projection_sign() {
        // 纯透视矩阵:第 3 列是 (0, 0, -1, 0)。
        let source: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, -1.0, -1.0, //
            0.0, 0.0, 0.0, 0.0,
        ];
        let matrix: Mat4 = Mat4::from_column_major(source);
        let out: [f32; 16] = frame_bytes(&matrix);

        // M[3][3] = -1 必须原样保留:它是「投影把 w 打成 -z」的符号。
        assert_eq!(out[at(3, 2)], -1.0, "projection sign lost");
        // M[0][2] / M[1][2] 是 clip.x / clip.y 的 z 系数,透视矩阵里
        // 本来就是 0;旧写法会把它们改成 0.5·0 + 0.5·0,看似一样,
        // 所以这里真正要盯的是第 2 行的第 0、1 项确实为 0。
        assert_eq!(out[at(2, 0)], 0.0, "clip.x gained a z term");
        assert_eq!(out[at(2, 1)], 0.0, "clip.y gained a z term");
        // 第 2 行第 2 项:0.5·(-1) + 0.5·(-1) = -1。
        assert_eq!(out[at(2, 2)], -1.0, "z row not remapped");
    }
}

/// WebGPU 渲染后端。
///
/// 构造是**异步**的(见 [`acquire`]),但 [`WebGpuRenderer::render`]
/// 是同步的 —— 拿到 device 之后,每帧不再需要任何 await。
#[derive(Debug)]
pub struct WebGpuRenderer {
    /// 已 `configure` 的画布上下文。
    pub(crate) context: JsValue,
    /// `GPUDevice`。
    pub(crate) device: JsValue,
    /// 画布格式。
    pub(crate) format: String,
    /// 主 render pipeline。
    pub(crate) pipeline: JsValue,
    /// pipeline layout(`createBindGroup` 需要它)。
    /// `GPUBindGroupLayout` —— `createBindGroup` 要的是**它**,不是
    /// `GPUPipelineLayout`(后者只是它的容器)。
    pub(crate) group_layout: JsValue,
    pub(crate) pipeline_layout: JsValue,
    /// 深度纹理(尺寸随画布变化重建)。
    pub(crate) depth_texture: JsValue,
    /// 深度纹理当前尺寸,(0, 0) = 还没建。
    pub(crate) depth_size: (u32, u32),
    /// uniform buffer 0:视投影矩阵。
    pub(crate) frame_buffer: JsValue,
    /// uniform buffer 1:着色参数。
    pub(crate) shading_buffer: JsValue,
    /// 与上面两个 buffer 对应的 bind group。
    pub(crate) bind_group: JsValue,
    /// 顶点 / 索引缓冲,下标与 `Scene::meshes` 一一对应。
    pub(crate) meshes: Vec<GpuMesh>,
    /// 所有批次共享的 instance buffer。
    pub(crate) instance_buffer: JsValue,
    /// instance buffer 当前容量(实例数)。
    pub(crate) instance_capacity: usize,
    /// 验收探针:GPU 侧资产表长度。
    pub(crate) gpu_mesh_count: usize,
}
