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
    /// `x` = `ao_height`,`y` = `ao_floor`,其余为 pad。
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
}

/// 着色参数 uniform 的字节数(14 × vec4 = 224 字节)。
///
/// ⚠️ 必须与 [`ShadingUniforms`] 的字段数**逐项一致**:少写一个
/// `vec4` 不会编译报错,只会在 GPU 上读到**后面那个块的数据**当成本
/// 字段 —— 阴影参数会静默变成色调映射的曝光值,画面偏亮且无影。
pub(crate) const SHADING_VEC4_COUNT: usize = 14;

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
