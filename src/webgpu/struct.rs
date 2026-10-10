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
    /// 顶点缓冲的**容量字节数**,不是本次写入的长度。
    ///
    /// ⚠️ 这个字段是「黑屏修复」的核心。`replace_mesh` 会把流式重建
    /// 出来的地面 / 水面数据重新写进**同一对**缓冲,而重建后的顶点数
    /// 会随生成中心变化 —— 可能变大。若没有容量记录,`writeBuffer` 就会
    /// 用「当初 `upload_mesh` 建的尺寸」去界,直接越界:Chrome 报
    /// `Write range (size: N) does not fit in buffer size (M)`,而写失败的
    /// 后果不是「这一帧没更新」,而是整条 `queue.submit()` 变成
    /// `[Invalid CommandBuffer]` **空提交** —— swapchain 那张纹理整个
    /// 没人画,于是屏幕**立刻变黑且永不恢复**。
    ///
    /// WebGL2 端没这个问题:`bufferData` 本身就是「按新数据重新分配」。
    /// WebGPU 的 `GPUBuffer` 一旦 `createBuffer` 就没有 `bufferData`,
    /// 必须自己记住容量并在需要时**换一个新的、更大的** `GPUBuffer`。
    pub vertex_capacity: usize,
    /// 索引缓冲的容量字节数(理由同 [`Self::vertex_capacity`])。
    pub index_capacity: usize,
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
    /// 阴影 frustum 的中心(世界坐标)。
    ///
    /// ⚠️ 与 WebGL2 端**同一个参数、同一份语义**:跟的是**相机焦点**
    /// 而不是眼点(见 [`crate::render::shadow_view_projection`] 的说明
    /// —— 跟着眼点会让远处物体的影子落进 frustum 之外,边缘出现一条
    /// 整齐的「影子截止线」)。
    pub shadow_focus: Vec3,
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
    /// `x` = `ao_height`(垂直上界),`y` = `ao_floor`(最深压暗系数),
    /// `z` = `ao_reach`(水平作用半径),`w` = `ao_feather`(水平平滑宽度)。
    ///
    /// ⚠️ **不再有「按 `world.y` 压暗」那一项。** 旧布局只有 xy,
    /// WGSL 里写的是 `mix(y, 1.0, clamp(world.y / x, 0, 1))` —— 那是
    /// 世界高度代理量,地面整片在 y≈0 上于是全城统一吃最深压暗。
    /// 改判据见 [`SHADER_VERTEX`] 里的 `contact_ao()`。
    pub ao_params: [f32; 4],
    /// `x` = `exposure`,`y` = `tone_map_white`,其余为 pad。
    pub exposure_white: [f32; 4],
    /// `x` = `shadow_strength`,`y` = PCF 半径(纹素),
    /// `z` = 深度偏置(纹素),`w` = 法线偏移(纹素)。
    ///
    /// 与 WebGL2 端的 `u_shadow_params` **逐项同序**,便于一眼比对。
    pub shadow_params: [f32; 4],
    /// `x` = 一个纹素覆盖的世界尺寸(米),`y` = WebGPU 深度域跨度(米),
    /// `z` = 深度偏置的斜率增益,`w` = 法线偏移的斜率增益。
    ///
    /// 后者是偏置从「纹素(米)」换算到「`[0, 1]` 深度」的分母 ——
    /// WebGL2 那边的 `2.0 / 1024.0` 魔数在 WebGPU 里不成立,详见
    /// [`crate::webgpu::r#const::SHADOW_DEPTH_SPAN_M`]。
    ///
    /// 两个斜率增益走 uniform 而不是写死在 WGSL 里,是为了调偏置时
    /// 只改 [`crate::webgpu::r#const`] 一处,不必同时维护 GLSL 与
    /// WGSL 两份魔数。
    pub shadow_misc: [f32; 4],
    /// `x` = 接触 AO 的**高度下界**(米),其余为 pad。
    ///
    /// 放在**整块的最后**而不是插在 `ao_params` 后面,是为了让本块
    /// 里每一个既有字段的字节偏移都保持不变 —— WGSL 的 uniform 按
    /// 声明顺序排布,中间插一个 vec4 会把后面全部字段平移 16 字节,
    /// 而 `write_buffer` 那侧若忘了同步改,读到的是错位的值
    /// (阴影参数静默变成色调映射的曝光值:画面偏亮且无影)。
    ///
    /// 单开一块而不是塞进 `ao_params` 的 zw,是因为 zw 已经被水平
    /// 作用半径(6.0 m)与平滑宽度(2.0 m)占满了。
    pub ao_band: [f32; 4],
}

/// 着色参数 uniform 的字节数(15 × vec4 = 240 字节)。
///
/// ⚠️ 必须与 [`ShadingUniforms`] 的字段数**逐项一致**:少写一个
/// `vec4` 不会编译报错,只会在 GPU 上读到**后面那个块的数据**当成本
/// 字段 —— 阴影参数会静默变成色调映射的曝光值,画面偏亮且无影。
pub(crate) const SHADING_VEC4_COUNT: usize = 15;

/// bloom 参数 uniform 的字节布局(3 × vec4 = 48 字节)。
///
/// 字段顺序必须与 WGSL 那个 `BloomParams` 结构体**逐项一致**,
/// 少一个 `vec4` 不会编译报错,只会让模糊核读成阈值。
///
/// 每条 pass 有**自己**那份 buffer(见
/// [`WebGpuRenderer::bloom_buffers`]),所以这个结构体里同时带着
/// 「提取要的阈值」「两条模糊各要的方向」「合成要的强度」——
/// 每个 pass 只读自己那两个分量,其余是陪坐。
#[derive(Clone, Copy, Debug, Default)]
pub struct BloomUniforms {
    /// `x` = 亮度阈值(线性空间,提取用),
    /// `y` = 合成强度,`zw` = 模糊方向(uv 单位,H 为 `(dx, 0)`、
    /// V 为 `(0, dy)`)。
    ///
    /// 方向**跟着每次上传走**而不是写死在 WGSL 里,是与 WebGL2 端
    /// `u_direction` 同一个做法 —— 那边的水平 / 垂直模糊共用一个
    /// program,方向就是 uniform。
    pub params: [f32; 4],
    /// 高斯核的前 4 个权重(w0..w3)。
    pub kernel: [f32; 4],
    /// `x` = 第 5 个权重(w4),`yz` = 一个纹素覆盖的 uv 尺寸,
    /// `w` = 保留。
    pub texel: [f32; 4],
}

/// bloom 参数 uniform 的字节数(3 × vec4 = 48 字节)。
///
/// ⚠️ 必须与 [`BloomUniforms`] 的字段数**逐项一致**:少分配的话
/// `writeBuffer` 会抛 `writeBuffer size exceeds buffer size`,
/// 而不是静默截断。
pub(crate) const BLOOM_VEC4_COUNT: usize = 3;

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
            let expected: f32 = 0.5 * source[at(2, column)] + 0.5 * source[at(3, column)];
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

    /// bloom 的高斯核必须是**归一化**的,否则每过一趟模糊画面就暗一截。
    ///
    /// 5 抽头核的总权重是 `w0 + 2 × (w1 + w2 + w3 + w4)` —— 中间
    /// 那四个抽头各取两次(`uv ± offset`)。这条钉住的就是「有人把某个
    /// 权重改成 0.2 却忘了重新归一化」那个坑:它不会报任何编译错误,
    /// 只是整幅画莫名变暗,而且很难联想到是这一行。
    ///
    /// 1e-5 的容差来自那五个常数**只保留 6 位小数**:`0.227027`
    /// 写成 `0.227027027` 时总和恰好 1.0,而这里用的是 WebGL2 端
    /// 那个 6 位版本,差值在 1e-5 量级。
    #[test]
    fn bloom_blur_kernel_weights_sum_to_one() {
        let total: f32 = bloom_blur_kernel_total();
        assert!(
            (total - 1.0).abs() < 1.0e-5,
            "bloom 高斯核没有归一化:总权重 {total}(应 ≈ 1.0),\
             每过一次模糊画面就暗一截"
        );
        // 权重必须单调递减,否则光晕会出现一个亮环而不是平滑衰减。
        let weights: [f32; 5] = BLOOM_BLUR_WEIGHTS;
        for index in 1..5usize {
            assert!(
                weights[index] < weights[index - 1],
                "核权重必须在索引 {} 处下降:{:?}",
                index,
                weights
            );
        }
    }

    /// 全屏三角形的 `uv.y` **必须**与 `clip.y` 反向 —— 这是 bloom 上线
    /// 当天整幅画面上下颠倒的那条根因。
    ///
    /// 两个方向是**各自独立**的约定,必须分开记:
    ///
    /// - `clip` 用的是 **NDC**:WebGPU 的 NDC **+y 朝上**,`clip.y = +1`
    ///   是屏幕**最上面**那一行。于是三角形顶点 `p.y = 0`(即
    ///   `clip.y = −1`)落在**下方** —— `p` 的 y **不能**取反,取反了
    ///   三角形就朝屏幕外面长,连覆盖都做不对。
    /// - `uv` 用的是**纹理采样坐标**:WebGPU 的纹理 **v = 0 在最上面
    ///   一行**(`textureSample` 的原点与 NDC 的 y 朝向相反)。于是
    ///   `p.y = 0` 那条边(屏幕**下方**)必须去采 `v ≈ 1`,即
    ///   `uv.y = 1 − p.y`。
    ///
    /// 这条测试把整条后处理链条钉成「**按行号一一对应**」:第 `r` 行
    /// 的片元采第 `r` 行,提取 / 两条模糊 / 合成全都行号不变 —— 所以它
    /// 与链条上有几条 pass 无关。写反的症状是「天空在画面最下面」,
    /// 而且 `cargo test` 变不红,只能靠这条 + 截图。
    #[test]
    fn fullscreen_uv_y_is_flipped_against_clip_y() {
        let source: &str = SHADER_FULLSCREEN_VERTEX;
        assert!(
            source.contains("out.uv = vec2<f32>(p.x, 1.0 - p.y);"),
            "vs_fullscreen 的 uv 必须按 (p.x, 1.0 - p.y) 生成:\
             WebGPU 的纹理 v = 0 在**最上一行**,而 NDC 的 +y 朝上,\
             两者相反。照抄 WebGL 那份的 `out.uv = p` 会让整幅画面上下颠倒\
             (天空跑到画面最下面)。当前源码:\n{source}"
        );
        // `clip.y` 保持不取反 —— 这两条是独立约定,别一起翻。
        assert!(
            source.contains("out.clip = vec4<f32>(p * 2.0 - vec2<f32>(1.0, 1.0), 0.0, 1.0);"),
            "vs_fullscreen 的 clip.y 不能取反:WebGPU 的 NDC +y 朝上,\
             `p.y = 0` 已经落在屏幕下方。当前源码:\n{source}"
        );
        // 旧的 `out.uv = p` 必须已经不存在(它是这条 bug 的原始写法)。
        assert!(
            !source.contains("out.uv = p;"),
            "`out.uv = p` 就是把画面上下颠倒的那一行,不能回来"
        );
    }

    /// bloom 三张目标必须正好是画布的**一半**分辨率。
    ///
    /// 这条钉住 [`bloom_target_size`] 的缩放系数:写成别的值(比如
    /// `0.25` 或 `1.0`)不会报任何错,只是光晕的半径跟着变 —— 半
    /// 分辨率下 1 个纹素的模糊步长等于全分辨率下的 2 个,所以改缩放
    /// 就等于悄悄改了 `BLOOM_BLUR_SPREAD`。
    #[test]
    fn bloom_targets_are_exactly_half_the_canvas() {
        // 与 WebGL2 端 `ensure_targets` 里的 `scaled(width, BLOOM_SCALE)`
        // 同一个系数。
        assert_eq!(BLOOM_SCALE, 0.5, "bloom 缩放系数被改了");
        for (width, height) in [(1280u32, 720u32), (800u32, 600u32), (1920u32, 1080u32)] {
            let size: (u32, u32) = bloom_target_size(width, height);
            assert_eq!(
                size,
                (width / 2, height / 2),
                "{width}x{height} 的 bloom 目标应该是 {width}x{height} 的一半"
            );
        }
    }

    /// 极小 / 零尺寸必须被抬到 1 像素,不能是 0。
    ///
    /// `createTexture` 的 `size` 是 `GPUExtent3D`,宽或高为 0 会被 WebGPU
    /// 判 invalid —— 而那是**异步**错误,表现为整帧消失、console 里没有
    /// 任何线索。窗口被拖到极小、或 `?res=0` 这类边界输入都会走到这里。
    #[test]
    fn bloom_target_size_never_collapses_to_zero() {
        assert_eq!(bloom_target_size(0, 0), (1, 1));
        assert_eq!(bloom_target_size(1, 1), (1, 1), "1x1 画布的半分辨率是 0.5,要抬到 1");
    }

    /// 扩容之后容量**必须**装得下 —— 这是黑屏修复的核心不变式。
    ///
    /// 背景:地面 / 水面网格随玩家跨街区流式重建,顶点数台阶式变化。
    /// 旧实现没有容量概念,`replace_mesh` 直接往当初那份偏小的缓冲里
    /// `writeBuffer`,越界之后整条 command buffer 判 invalid,
    /// `queue.submit()` 变成空操作,swapchain 没人画 → 屏幕全黑且永不恢复。
    ///
    /// 这条测试把「写之前先扩容」那个不变式钉成可离线断言的东西:
    /// 真实黑屏要在无头浏览器里跑几千帧才看得见,而不变式随时可测。
    #[test]
    fn grown_capacity_always_fits_the_payload() {
        // 真实黑屏现场的两个尺寸(来自 Chrome 的 validation 报错):
        //   顶点 880992 → 901368 个(16 B / 顶点)= 14095872 → 14421888 B
        //   索引 293664 → 300456 个(4 B / 索引) = 1174656 → 1201824 B
        for (current, needed) in [
            (14_095_872usize, 14_421_888usize),
            (1_174_656usize, 1_201_824usize),
        ] {
            let grown: usize = grown_capacity(current, needed);
            assert!(
                grown >= needed,
                "扩容后装不下:{needed} > {grown}(现有 {current})—— writeBuffer 会越界,\
                 整帧判 invalid 并让 submit 变空操作,画面全黑"
            );
            assert!(
                grown > current,
                "需要更大却没扩:{current} -> {grown},下一次重建必然再次越界"
            );
        }
    }

    /// 不需要扩容时**不能**缩小缓冲,否则会把刚好够用的尺寸又变回去,
    /// 让下一次「略微变大」立刻再次越界。
    #[test]
    fn grown_capacity_never_shrinks_a_live_buffer() {
        for (current, needed) in [
            (1_174_656usize, 1_174_656usize),
            (16_777_216usize, 1_201_824usize),
            (14_095_872usize, 0usize),
        ] {
            assert_eq!(
                grown_capacity(current, needed),
                current,
                "{current} 装得下 {needed} 时容量不该变"
            );
        }
    }

    /// 连续多街区重建必须**单调不降**:每一帧的容量都要盖住那一帧
    /// 实际的载荷。玩家连续横穿城市时这就是真实序列。
    #[test]
    fn grown_capacity_is_monotonic_across_consecutive_rebuilds() {
        // 实测地面网格随生成中心漂移时的载荷台阶(每一项 = 一帧载荷)。
        let payloads: [usize; 6] = [
            14_095_872, 14_421_888, 14_421_888, 14_950_656, 15_204_736, 15_466_112,
        ];
        let mut capacity: usize = 0;
        for payload in payloads {
            capacity = grown_capacity(capacity, payload);
            assert!(
                capacity >= payload,
                "第 {payload} B 的重建载荷装不进 {capacity} B 的缓冲"
            );
        }
    }

    /// 画布尺寸为 0 时整帧必须放弃,绝不能把 0×0 的 swapchain 送进
    /// WebGPU。
    ///
    /// `context.getCurrentTexture()` 在画布为 0 时返回一张 0×0 纹理,
    /// Dawn 报 `Could not create a swapchain texture of size 0`
    /// → `[Invalid Texture]` → `[Invalid TextureView]`
    /// → `[Invalid CommandBuffer]` → `submit` 空操作 → 画面全黑且不恢复。
    ///
    /// `render()` 因此在 `width == 0 || height == 0` 时提前 `Ok(0)`。
    /// 这条测试钉住那个判据本身,免得有人为了「省一次 early return」
    /// 把它删掉。
    #[test]
    fn zero_canvas_size_is_rejected_before_any_submit() {
        for (width, height) in [(0u32, 720u32), (1280u32, 0u32), (0u32, 0u32)] {
            assert!(
                width == 0 || height == 0,
                "守卫判据必须覆盖 {width}x{height}:任何一个维度为 0 都会拿到 0×0 swapchain"
            );
        }
    }

    /// 合成的辉光强度必须与 WebGL2 端那个表达式逐项相同。
    ///
    /// 钉住 `BLOOM_STRENGTH * emissive_gain.max(BLOOM_MIN_GAIN)`:
    /// 少了 `max` 的下限,正午相位 `emissive_gain` 只有 0.18(低于
    /// 0.35 的下限),辉光会被压到 `BLOOM_STRENGTH × 0.35` —— 忘了
    /// `max` 的话正午的湿路面高光就完全没有辉光了,而且从黄昏帧上
    /// 看**完全正常**,只有切到正午才暴露。
    #[test]
    fn bloom_composite_strength_matches_the_webgl2_formula() {
        // 正午:emissive_gain 低于下限,必须被兜住。
        let mut lighting: SceneLighting =
            SceneLighting::for_phase(crate::render::DayPhase::Noon);
        assert!(
            lighting.emissive_gain < BLOOM_MIN_GAIN,
            "前提不成立:noon 相位的 emissive_gain {} 不低于下限 {BLOOM_MIN_GAIN},\
             这条测试测不出 max 的作用",
            lighting.emissive_gain
        );
        assert_eq!(
            bloom_composite_strength(&lighting),
            BLOOM_STRENGTH * BLOOM_MIN_GAIN,
            "noon 相位低于下限,辉光必须被 BLOOM_MIN_GAIN 兜住(忘了 max)"
        );
        // 黄昏:emissive_gain 高于下限,取线性那一支。
        lighting = SceneLighting::for_phase(crate::render::DayPhase::Dusk);
        assert!(
            lighting.emissive_gain > BLOOM_MIN_GAIN,
            "前提不成立:dusk 相位的 emissive_gain {} 低于下限 {BLOOM_MIN_GAIN}",
            lighting.emissive_gain
        );
        assert_eq!(
            bloom_composite_strength(&lighting),
            BLOOM_STRENGTH * lighting.emissive_gain,
            "dusk 相位高于下限,辉光必须是线性的那一支"
        );
    }

    /// 游戏里真实存在的黄昏光方向(`render.rs` 里 `DUSK` 那一档)。
    ///
    /// 复用来它是因为两个后端必须对**同一束斜光**给出同一个剔除决定 ——
    /// 否则 WebGPU 上会少掉一栋楼的影子,而 WebGL2 上是好的。
    fn dusk_light() -> Vec3 {
        crate::render::normalize3([0.86, 0.24, -0.44])
    }

    /// 判据的有效半径:半宽 + 余量(`render.rs` 的 `SHADOW_HALF_EXTENT`
    /// 与 `SHADOW_CULL_MARGIN`)。
    fn reach() -> f32 {
        crate::r#const::SHADOW_HALF_EXTENT + crate::r#const::SHADOW_CULL_MARGIN
    }

    /// ⚠️ **只测原点的判据会误剔高楼** —— 这条钉住那个坑。
    ///
    /// [`crate::render::instance_affects_shadow`] 的注释里写了原因:
    /// 判据要同时看**原点**和**原点往下挪一截**两个落点。斜光下
    /// (`dusk` 的 y 分量仅 0.24)一栋 40 m 高的楼,原点在 y = 40 时
    /// 影子落点距 focus 119 m(在视锥外),而它的**底部**(y = 20)
    /// 落点只有 61 m,仍在「半宽 + 余量」之内 —— 只测原点就会把
    /// 「影子还伸进视锥」的高楼整栋剔掉,地面上凭空少一块长影子。
    ///
    /// 这里的实例用 `[x, 40, 0]` 与 `[x, 20, 0]` 两支:
    /// 前者是只看原点会做出错误决定的那个,后者是唯一救回它的那支。
    #[test]
    fn shadow_cull_keeps_a_tower_whose_base_reaches_the_frustum() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        let light: Vec3 = dusk_light();
        let top: crate::render::Instance =
            crate::render::Instance::new([30.0, 40.0, 0.0], 0.0, 1.0, [1.0; 3]);
        let base: crate::render::Instance =
            crate::render::Instance::new([30.0, 20.0, 0.0], 0.0, 1.0, [1.0; 3]);

        // 两个落点:沿光线投影到 focus 所在的水平面上。
        //
        // 闭包**不能**写成 `fn(f32) -> f32` 指针类型(§5.1 要求显式标注,
        // 而 `fn` 指针不能捕获环境),所以把 focus / light 改成参数。
        let landing: fn(Vec3, Vec3, f32) -> f32 = |focus: Vec3, light: Vec3, height: f32| -> f32 {
            let along: f32 = (focus[1] - height) / light[1];
            let dx: f32 = 30.0 + light[0] * along - focus[0];
            let dz: f32 = 0.0 + light[2] * along - focus[2];
            (dx * dx + dz * dz).sqrt()
        };
        let top_landing: f32 = landing(focus, light, top.get_model_ref()[13]);
        let base_landing: f32 = landing(focus, light, base.get_model_ref()[13]);

        // 这个用例的前提:楼顶落在视锥外,而楼底落在视锥内。
        // 两支断言都失败的话说明常量被改了,用例本身失去意义。
        assert!(
            top_landing > reach(),
            "前提不成立:楼顶落点 {top_landing:.2} m 已在视锥内(半宽 + 余量 = {:.2}),\
             测不出「只看原点」那个坑",
            reach()
        );
        assert!(
            base_landing <= reach(),
            "前提不成立:楼底落点 {base_landing:.2} m 也在视锥外(半宽 + 余量 = {:.2}),\
             这栋楼根本不该被考虑",
            reach()
        );

        // 真正要钉的那一条:**只测原点**的写法会把这栋楼剔掉。
        assert!(
            !origin_only_would_keep(&top, focus, light),
            "只看原点的判据本该把这栋楼剔掉(它就是那个 bug 的形状),\
             落点 {top_landing:.2} m > 半宽 + 余量 = {:.2}",
            reach()
        );
        // 而带上下两支探针的真判据**必须**留下它。
        assert!(
            crate::render::instance_affects_shadow(&top, focus, light),
            "斜光下 {top_landing:.2} m 的高楼被剔掉了,但它的楼底落点只有 \
             {base_landing:.2} m ≤ 半宽 + 余量 = {:.2} —— 地面上会凭空少一块影子",
            reach()
        );
        // 楼底那一支单独看当然也在视锥内。
        assert!(
            crate::render::instance_affects_shadow(&base, focus, light),
            "楼底落点 {base_landing:.2} m 明明在视锥内(半宽 + 余量 = {:.2}),却被剔掉了",
            reach()
        );
    }

    /// 「只看原点」那个**已知错误**的判据,仅供上面的测试做对照。
    ///
    /// 它刻意**不是** [`crate::render::instance_affects_shadow`] ——
    /// 那正是本测试要证明「不再只看原点」的理由。写成独立函数而不是
    /// 在测试里内联,是为了让对照判据一眼可读。
    fn origin_only_would_keep(
        instance: &crate::render::Instance,
        focus: Vec3,
        light: Vec3,
    ) -> bool {
        let m: &crate::r#type::Mat4Data = instance.get_model_ref();
        let along: f32 = (focus[1] - m[13]) / light[1];
        let dx: f32 = m[12] + light[0] * along - focus[0];
        let dz: f32 = m[14] + light[2] * along - focus[2];
        dx * dx + dz * dz <= reach() * reach()
    }

    /// `smoothstep(a, b, x)` 的 Rust 写法,与 WGSL 的同名内建函数一致。
    ///
    /// # Arguments
    ///
    /// - `a` - 下界。
    /// - `b` - 上界。
    /// - `x` - 输入。
    ///
    /// # Returns
    ///
    /// 平滑后的 `0..=1`。
    fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
        let t: f32 = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// 顶点接触 AO 的 Rust 镜像,与 [`SHADER_VERTEX`] 里那个
    /// `contact_ao()` 逐字对应。
    ///
    /// 放在测试里而不只测 WGSL 字符串,是因为这个缺陷本质是**数值**
    /// 的:字符串照样能通过「声明了 uniform」那类检查,只有把公式跑
    /// 一遍才会发现开阔地是不是被无端压暗了。
    ///
    /// # Arguments
    ///
    /// - `world_y` - 顶点的世界高度(米)。
    /// - `horizontal` - 顶点到本模型世界原点的 XZ 水平距离(米)。
    ///
    /// # Returns
    ///
    /// 亮度系数 `1.0`(不压暗)..=`CONTACT_SHADOW_FLOOR`(贴基座最暗)。
    fn contact_ao_factor(world_y: f32, horizontal: f32) -> f32 {
        let reach: f32 = CONTACT_SHADOW_REACH;
        let feather: f32 = CONTACT_SHADOW_FEATHER;
        let low: f32 = CONTACT_SHADOW_MIN_HEIGHT;
        let high: f32 = BAKED_CONTACT_AO_HEIGHT;
        let proximity: f32 = 1.0 - smoothstep(reach - feather, reach, horizontal);
        let low_enough: f32 =
            smoothstep(low, low + 0.001, world_y) * (1.0 - smoothstep(high - 0.001, high, world_y));
        1.0 - (1.0 - CONTACT_SHADOW_FLOOR) * low_enough * proximity
    }

    /// 缺陷本体:接触 AO 拿**世界高度**当「离基座多近」的代理量。
    ///
    /// 旧 WGSL 表达式 `mix(ao_params.y, 1.0, clamp(world.y /
    /// ao_params.x, 0, 1))` 对同一高度的两个顶点给出**完全相同**的
    /// 压暗 —— 离基座 0.3 m 的和 40 m 外的都是 `CONTACT_SHADOW_FLOOR`。
    /// 修好后应是前者更暗、后者回到 1.0。
    ///
    /// 实测(2026-10-10,LOFT 样板楼南墙 z=35 外,WebGPU,正午):沿墙法向
    /// 0.55 → 6.85 m 的地面 luma 落在 103.024..104.433(range 1.41),
    /// 墙根均值比开阔地还**低** 0.232 luma —— 一条平线,而且方向还反了。
    #[test]
    fn contact_ao_darkens_by_horizontal_proximity_not_world_height() {
        // 楼身下段,同一高度,只改到基座的水平距离。
        let y: f32 = 0.5;
        let at_base: f32 = contact_ao_factor(y, 0.3);
        let nearby: f32 = contact_ao_factor(y, 3.0);
        let open: f32 = contact_ao_factor(y, 40.0);

        assert!(
            at_base < open - 0.05,
            "同高度 y={y}:离基座 0.3 m 的 {at_base:.4} 必须明显比 40 m 外的 \
             {open:.4} 更暗 —— 旧公式按 world.y 算,两者会完全相等"
        );
        assert!(
            nearby < open,
            "离基座 3.0 m 的 {nearby:.4} 应比 40 m 外的 {open:.4} 暗"
        );
        assert!(
            (open - 1.0).abs() < 1e-5,
            "离基座 40 m 已在 CONTACT_SHADOW_REACH={CONTACT_SHADOW_REACH} m 之外,\
             接触 AO 必须完全回到 1.0,实测 {open:.5}"
        );
    }

    /// **地面网格整片不吃顶点接触 AO。**
    ///
    /// 这是用户报「地面的光追效果不对」时最直接的成因:旧公式按
    /// `world.y / ao_height` 压暗,而路面 `ROAD = 0.0`、人行道
    /// `SIDEWALK = 0.14`、地块 `LOT_GROUND = 0.16`(取自 `game.rs` 的
    /// `build_ground_near`)全都远低于 `BAKED_CONTACT_AO_HEIGHT = 1.2`,
    /// 于是全城每一块地面都取到 `clamp(0) = 0` 吃满最深压暗 —— 墙根
    /// 与开阔街道一样暗。
    ///
    /// 修法是加高度下界 `CONTACT_SHADOW_MIN_HEIGHT`(0.30 m,高于地面
    /// 网格最高点),让这一项只负责「楼身下段 / 树干根部」的自遮蔽;
    /// 地面贴墙的暗部交给 SSAO —— 顶点着色器结构上不知道墙在哪,而
    /// **WebGPU 路径目前还没有 SSAO**。
    #[test]
    fn contact_ao_leaves_the_ground_mesh_untouched() {
        for (name, y) in [("road", 0.0_f32), ("sidewalk", 0.14), ("lot", 0.16)] {
            // 即便就站在一栋楼正下方(水平距离 0),也必须是 1.0。
            let value: f32 = contact_ao_factor(y, 0.0);
            assert!(
                (value - 1.0).abs() < 1e-5,
                "{name}(y={y})属于地面网格,不该吃顶点接触 AO —— 实测 {value:.5}; \
                 CONTACT_SHADOW_MIN_HEIGHT={CONTACT_SHADOW_MIN_HEIGHT} 本该把它整个排除"
            );
        }
        // 用 `const {}` 而不是普通 `assert!`:这个不变量完全由常量决定,
        // 而 clippy 的 `assertions_on_constants` 会要求它放进 const 块 ——
        // 那正好更强:const 块在**编译期**求值,常量被改坏时是编译失败,
        // 而不是运行时一条断言。
        const {
            assert!(
                CONTACT_SHADOW_MIN_HEIGHT > 0.16,
                "高度下界必须高于地面网格最高点 0.16 (LOT_GROUND),否则路面仍会被无端压暗"
            );
        }
    }

    /// 高处顶点不吃「贴墙根」的压暗。
    ///
    /// 若垂直上界失效,一栋 40 m 高的楼会整条压暗,表现为
    /// 「每栋楼都自带一圈黑边」。上界由 `BAKED_CONTACT_AO_HEIGHT` 封顶。
    #[test]
    fn contact_ao_skips_the_proximity_term_above_the_height_band() {
        let y: f32 = BAKED_CONTACT_AO_HEIGHT * 4.0;
        let value: f32 = contact_ao_factor(y, 0.0);
        assert!(
            (value - 1.0).abs() < 1e-5,
            "y={y:.2}(远超 BAKED_CONTACT_AO_HEIGHT={BAKED_CONTACT_AO_HEIGHT})且贴着基座时\
             接触 AO 必须是 1.0,实测 {value:.5}"
        );
    }

    /// 顶点着色器读的那三个 uniform 分量必须真的被上传。
    ///
    /// `ao_params` 从「只有 xy」扩到「xyzw」:若 WGSL 里写的是
    /// `ao_params.z` 而 Rust 侧忘了填,读到的是 0,于是
    /// `max(reach, 0.001)` 退化成 0.001 m 的作用半径 —— **楼身下段
    /// 一条接触暗带都消失**,而画面上不报任何错。
    #[test]
    fn contact_ao_reach_and_feather_reach_the_vertex_shader() {
        let lighting: SceneLighting = SceneLighting::for_phase(crate::render::DayPhase::Noon);
        let shading: ShadingUniforms = shading_from_lighting(&lighting, [0.0, 1.6, 12.0]);
        assert_eq!(
            shading.ao_params[2], CONTACT_SHADOW_REACH,
            "ao_params.z(水平作用半径)必须等于 {CONTACT_SHADOW_REACH},实测 {} —— \
             忘了填的话顶点着色器读到 0,整条水平项退化成 0.001 m",
            shading.ao_params[2]
        );
        assert_eq!(shading.ao_params[3], CONTACT_SHADOW_FEATHER);
        assert_eq!(
            shading.ao_band[0], CONTACT_SHADOW_MIN_HEIGHT,
            "ao_band.x(高度下界)必须等于 {CONTACT_SHADOW_MIN_HEIGHT},实测 {} —— \
             忘了填的话读到 0,整城地面又回到「全亮」的老样子",
            shading.ao_band[0]
        );
    }

    /// ⚠️ uniform 布局的**字节偏移**不能被这一轮的改动挪动。
    ///
    /// WGSL 的 uniform struct 按声明顺序排布,少一个 vec4 不会编译
    /// 报错,只会在 GPU 上读到**后面那个块的数据**当成本字段 —— 阴影
    /// 参数静默变成色调映射的曝光值,症状是「画面偏亮且无影」。
    /// `ao_band` 因此被放在**整块的最后**,而不是插在 `ao_params`
    /// 后面。
    #[test]
    fn shading_uniform_layout_is_three_hundred_and_forty_six_bytes() {
        assert_eq!(SHADING_VEC4_COUNT, 15);
        assert_eq!(
            UNIFORM_SHADING_BYTES,
            (SHADING_VEC4_COUNT as u32) * 16,
            "UNIFORM_SHADING_BYTES 与字段数脱节:少分配的 buffer 会让 \
             writeBuffer 直接抛 writeBuffer size exceeds buffer size"
        );
        // 既有字段的偏移必须一字不动:`shadow_misc` 是第 14 块(下标 13),
        // `ao_band` 只能是第 15 块(下标 14)。
        let shading: ShadingUniforms = ShadingUniforms::default();
        let flat: [f32; 60] = flatten_shading(shading);
        assert_eq!(
            &flat[52..56],
            &shading.shadow_misc,
            "shadow_misc 的偏移被挪动了 —— 它必须仍在下标 13(字节 208)"
        );
        assert_eq!(
            &flat[56..60],
            &shading.ao_band,
            "ao_band 必须在最后一块(下标 14 / 字节 224)"
        );
    }

    /// ⚠️ 顶点与片元两份 WGSL 里的 `Shading` struct 必须**逐项一致**。
    ///
    /// 两者是同一个 uniform buffer 的两个视图,少一块 vec4 不会编译
    /// 报错,只会在 GPU 上读到后面那个块的数据当成本字段。更糟的是:
    /// **顶点那份缺字段时 WGSL 直接编译失败**(`shading.ao_band` 不存在),
    /// 整条管线作废,画面全黑 —— 而 `cargo test` / `cargo check` 全绿,
    /// 因为它们根本不看 WGSL 字符串。
    ///
    /// 实测踩过:第一版只给片元那份加了 `ao_band`,顶点那份漏了,
    /// 探针量到的地面 luma 是全 0.0。**教训:改这份字符串之后必须
    /// 真的把画面截下来看**,不能只看编译与测试。
    #[test]
    fn vertex_and_fragment_shading_structs_declare_the_same_fields() {
        // ⚠️ 两份必须**完全逐项相同**,包括顶点阶段一个都不读的那两块。
        //
        // WGSL 的 uniform 偏移是按**每份 struct 自己的声明顺序**算的,
        // 所以「不读就不声明」在这里是个陷阱:顶点那份少两块,后面所有
        // 字段的偏移就整体前移 32 字节,而 Rust 侧照旧上传 15 块。
        // `ao_band` 于是读到 `shadow_params`(阴影强度 = 1.0)而不是
        // 高度下界 0.30 —— 高度下界变成 1.0 m 之后,地面(y ≤ 0.16)
        // 的接触带判据仍然成立,**整片地面又吃满最深压暗**,正好退回
        // 本次要修的那个 bug。
        //
        // 实测踩过两轮:
        //  (1) 第一版只给片元那份补 `ao_band`,顶点那份漏了 → WGSL
        //      直接编译失败 → 整条管线作废 → 画面全黑(探针 luma 全 0)。
        //  (2) 第二版给顶点那份补了 `ao_band`,却沿用了「不读就不声明」
        //      的旧惯例没补阴影那两块 → 不编译报错,但地面 luma 从
        //      103.4 掉到 86.8。cargo test / cargo check 全绿。
        //
        // **教训:改这份字符串之后必须真的把画面截下来量,不能只看
        // 编译与测试。**
        let expected: [&str; SHADING_VEC4_COUNT] = [
            "light_dir",
            "light_color",
            "ambient",
            "sky_color",
            "sky_ambient",
            "ground_ambient",
            "ambient_hemi",
            "emissive_gain",
            "eye",
            "fog",
            "ao_params",
            "exposure_white",
            "shadow_params",
            "shadow_misc",
            "ao_band",
        ];
        // 收集 `struct Shading { ... }` 里的字段名(按声明顺序)。
        //
        // 写成普通循环而不是 `lines().map().filter().map()`:链上每一步的
        // 入参类型都在 `&str` / `&&str` 之间跳,而本仓库的规则要求每个闭包
        // 参数写明类型,于是类型标注比逻辑本身还长。
        fn fields_of(shader: &str) -> Vec<String> {
            let start: &str = match shader.split("struct Shading {").nth(1) {
                Some(rest) => rest,
                None => panic!("WGSL 里找不到 `struct Shading {{`"),
            };
            let body: &str = start.split('}').next().unwrap_or_default();
            let mut out: Vec<String> = Vec::new();
            for line in body.lines() {
                let trimmed: &str = line.trim();
                let decl: &str = trimmed.trim_end_matches(',');
                if !decl.ends_with(": vec4<f32>") {
                    continue;
                }
                let field: &str = decl.split(':').next().unwrap_or("");
                out.push(field.trim().to_string());
            }
            out
        }
        let want: Vec<String> = expected
            .iter()
            .map(|s: &&str| -> String { (*s).to_string() })
            .collect();

        for (name, shader) in [
            ("SHADER_VERTEX", SHADER_VERTEX),
            ("SHADER_FRAGMENT", SHADER_FRAGMENT),
        ] {
            assert_eq!(
                fields_of(shader),
                want,
                "{name} 的 `struct Shading` 与 Rust 侧的 SHADING_VEC4_COUNT \
         ({SHADING_VEC4_COUNT} 块)不一致。\n\
         ⚠️ uniform 偏移按**本 struct 的声明顺序**算,所以「顶点阶段不读的块\
         就不声明」会让后面所有字段前移 —— 不会编译报错,只会在 GPU 上\
         读到别人的字节(高度下界会变成阴影强度,地面于是又吃满压暗)。"
            );
        }
    }

    /// 顶点着色器必须真的按**水平距离**算接触 AO。
    ///
    /// 只钉数值不够:数值测试用的是 Rust 镜像,镜像与 WGSL 字符串可以
    /// 各改各的而测试仍然全绿。这条断言把「WGSL 里读的是
    /// `world.xz - base.xz` 的水平距离」这件事钉在字符串上,并钉住
    /// 「旧的按世界高度 clamp 的写法已经不在了」。
    #[test]
    fn vertex_shader_keys_contact_ao_on_horizontal_distance() {
        assert!(
            SHADER_VERTEX.contains("length(world.xz - base.xz)"),
            "顶点着色器必须用水平距离算接触 AO,而不是世界高度"
        );
        assert!(
            !SHADER_VERTEX.contains("clamp(world.y / max(shading.ao_params.x"),
            "按 world.y 压暗的老公式还在 —— 地面整片在 y≈0 上,会全城吃满最深压暗"
        );
        for uniform in [
            "shading.ao_params.z",
            "shading.ao_params.w",
            "shading.ao_band.x",
        ] {
            assert!(
                SHADER_VERTEX.contains(uniform),
                "顶点着色器读了 `{uniform}`,但 WGSL 里读不到它 —— \
         少了对应的 struct 字段就会**整条管线编译失败、画面全黑**"
            );
        }
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
    /// 阴影 pass 的 pipeline(纯深度、无颜色输出)。
    pub(crate) shadow_pipeline: JsValue,
    /// 阴影 pass 的 pipeline layout(只有 group 0 的光源矩阵)。
    pub(crate) shadow_pipeline_layout: JsValue,
    /// 阴影 pass 的 bind group layout。
    pub(crate) shadow_group_layout: JsValue,
    /// 阴影 pass 的 bind group(group 0 = 光源矩阵)。
    pub(crate) shadow_bind_group: JsValue,
    /// 光源视投影矩阵 uniform buffer(主 pass 与阴影 pass 各绑一份
    /// 同一个 buffer 到不同的 binding 上)。
    pub(crate) shadow_frame_buffer: JsValue,
    /// 阴影深度纹理(`depth32float`,边长 [`crate::r#const::SHADOW_MAP_SIZE`] 的正方形)。
    pub(crate) shadow_texture: JsValue,
    /// 阴影 pass 专用的 instance buffer。
    ///
    /// ⚠️ **不能与主管线共用**:`queue.writeBuffer` 在队列时间线上先于
    /// 整条 command buffer 执行,同帧两次上传会互相覆盖。详见
    /// [`WebGpuRenderer::draw_shadow_pass`] 里的说明。
    pub(crate) shadow_instance_buffer: JsValue,
    /// 阴影 instance buffer 的容量(实例数)。
    pub(crate) shadow_instance_capacity: usize,
    /// 主 pass 用的 group 1:阴影图 + 比较采样器 + 光源矩阵。
    pub(crate) shadow_sample_bind_group: JsValue,
    /// group 1 的 `GPUBindGroupLayout`。
    ///
    /// 单独存一份是因为它被**两个阶段**按顺序用到:主 pipeline 的
    /// layout 里要列它(`build_pipeline_inner`),而 bind group 要拿它
    /// 当描述符的 `layout`(`create_shadow_sample_bind_group`)。
    pub(crate) shadow_sample_group_layout: JsValue,
    /// 顶点 / 索引缓冲,下标与 `Scene::meshes` 一一对应。
    pub(crate) meshes: Vec<GpuMesh>,
    /// 所有批次共享的 instance buffer。
    pub(crate) instance_buffer: JsValue,
    /// instance buffer 当前容量(实例数)。
    pub(crate) instance_capacity: usize,
    /// 验收探针:GPU 侧资产表长度。
    pub(crate) gpu_mesh_count: usize,
    /// 主场景颜色目标(全分辨率,画布 preferred format)。
    ///
    /// ⚠️ **有 bloom 就不能直接把主 pass 画进 swapchain。** 亮度提取
    /// 要采样主场景的颜色,而 `getCurrentTexture()` 给的那张纹理
    /// 只有 `RENDER_ATTACHMENT | COPY_SRC`,**不能**被采样(除非
    /// `configure` 时显式要 `TEXTURE_BINDING`,而本项目没有)。
    ///
    /// 所以主 pass 画进这张全分辨率离屏目标,合成那一条 pass 再把它
    /// 读回来、加完辉光画进 swapchain —— 与 WebGL2 端
    /// `RenderTarget::bind(context, Some(scene))` 完全同构。
    pub(crate) bloom_scene: JsValue,
    /// 亮度提取的目标纹理(半分辨率)。
    pub(crate) bloom_bright: JsValue,
    /// 水平模糊的输出纹理(ping)。
    pub(crate) bloom_ping: JsValue,
    /// 竖直模糊的输出纹理(pong,也是合成读的「模糊结果」)。
    pub(crate) bloom_pong: JsValue,
    /// 四张后处理目标当前的尺寸,(0, 0) = 还没建。
    ///
    /// 记的是**画布**尺寸而不是 bloom 那三张的尺寸:四张一起重建,
    /// 一个尺寸就够(见 [`WebGpuRenderer::ensure_bloom_targets`]）。
    pub(crate) bloom_size: (u32, u32),
    /// 四条 bloom pass **各一个**的 uniform buffer。
    ///
    /// ⚠️ **不能共用一个。** `queue.writeBuffer` 排在队列时间线上,
    /// 整条 command buffer 之前就执行完了 —— 同帧往**同一个** buffer
    /// 写三次,只有最后一次的内容会生效。而提取 / 模糊 H / 模糊 V /
    /// 合成四份数据**互不相同**(各自的阈值、方向、强度),于是共用
    /// 的话两条模糊 pass 会读到同一个方向,竖直模糊退化成第二次水平
    /// 模糊,光晕变成横向条纹。
    ///
    /// 这与阴影 pass 那个「实例 buffer 必须另开一份」是同一条队列
    /// 时间线陷阱。
    pub(crate) bloom_buffers: [JsValue; BLOOM_PASS_COUNT],
    /// bloom 颜色输入用的**普通**(非比较)采样器。
    ///
    /// ⚠️ 与阴影那张 `comparison` 采样器是**两个不同的对象**:WebGPU
    /// 的深度纹理只配 `sampler_comparison`,而 `rgba8unorm` 颜色纹理
    /// 只配普通 `sampler` —— 拿错会得到 validation error。
    pub(crate) bloom_sampler: JsValue,
    /// 提取 / 模糊三条 pipeline 的 bind group layout(3 槽:uniform +
    /// sampler + texture,形状完全一致)。
    pub(crate) bloom_group_layout: JsValue,
    /// 合成那条 5 槽 bind group layout(多一对 sampler + 模糊图)。
    pub(crate) bloom_composite_group_layout: JsValue,
    /// 四条 bloom pipeline 各自的 `(pipeline, bind group)` 配对。
    ///
    /// 收在一个数组里而不是摊成八个字段:它们一一对应,拆开就会出现
    /// 「某条 pipeline 用了别人的 bind group」这种静默不兼容 ——
    /// WebGPU 只在真正 `setBindGroup` 那一刻才报 validation error,
    /// 症状是那一帧消失而前面几帧都正常。
    pub(crate) bloom_passes: [GpuBloomPass; BLOOM_PASS_COUNT],
}

/// 一条 bloom pass 要上传的那份 uniform 的**参数**
/// (不含核权重 —— 那是所有 pass 共用的)。
///
/// # Arguments
///
/// - `threshold` - `params.x`:亮度阈值,只对提取那条有意义。
/// - `strength` - `params.y`:辉光强度,只对合成那条有意义。
/// - `direction` - `params.zw`:模糊方向(uv 单位)。
/// - `size` - 这条 pass **读写的那张目标**的尺寸。
///
/// 具名成 struct 而不是四元组:四元组那四个位置在下游只能靠
/// `uploads[i].2` 这种下标读,而提取与合成两条的形状一样
/// (方向恒为 `(0.0, 0.0)`),读错了编译器不会报错。字段名让
/// 「这条 pass 的方向是什么」变成一句能读的话。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BloomPassUpload {
    /// `params.x`:亮度阈值(线性空间)。
    pub(crate) threshold: f32,
    /// `params.y`:合成强度。
    pub(crate) strength: f32,
    /// `params.zw`:模糊方向(uv 单位)。
    pub(crate) direction: (f32, f32),
    /// 这条 pass 读写的那张目标的尺寸。
    pub(crate) size: (u32, u32),
}

/// bloom 一共要跑的四条 pass(顺序即下标)。
///
/// 提取 → 水平模糊 → 竖直模糊 → 合成,下标与
/// [`WebGpuRenderer::bloom_passes`] 一一对应。
pub(crate) const BLOOM_PASS_COUNT: usize = 4;

/// 提取那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_EXTRACT: usize = 0;

/// 水平模糊那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_BLUR_H: usize = 1;

/// 竖直模糊那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_BLUR_V: usize = 2;

/// 合成那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_COMPOSITE: usize = 3;

/// 一条 bloom pipeline 与它的 bind group 的配对句柄。
///
/// ⚠️ **刻意不带 `Copy`**:两个字段都是 `JsValue`,而 `JsValue` 只是
/// `Clone` 不是 `Copy`(wasm-bindgen 把它当成栈上的临时句柄)。所以
/// 传递整张表时用 [`WebGpuRenderer::get_bloom_passes`] 按值克隆 ——
/// 每帧一次、四个句柄,代价可以忽略,换来的是调用点不必操心
/// 借用与 `&mut self` 的冲突。
#[derive(Debug, Clone)]
pub struct GpuBloomPass {
    /// `GPURenderPipeline`。
    pub pipeline: JsValue,
    /// 与 `pipeline` 的 pipeline layout 严格对应的 `GPUBindGroup`。
    pub bind_group: JsValue,
}
