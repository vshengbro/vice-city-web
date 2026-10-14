use super::*;

/// Inherent implementation of [`MeshAssetGpu`].
impl MeshAssetGpu {}

/// Inherent implementation of [`DayPhase`].
impl DayPhase {
    /// 相位名(HUD 与调试快照用)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 相位名。
    pub fn as_str(&self) -> &'static str {
        match self {
            DayPhase::Noon => PHASE_NOON,
            DayPhase::Dusk => PHASE_DUSK,
            DayPhase::Night => PHASE_NIGHT,
        }
    }

    /// 下一个相位(循环)。
    pub fn next(self) -> Self {
        match self {
            DayPhase::Noon => DayPhase::Dusk,
            DayPhase::Dusk => DayPhase::Night,
            DayPhase::Night => DayPhase::Noon,
        }
    }

    /// 归一化到 `[0, 1)` 的滑块位置。
    ///
    /// # Returns
    ///
    /// - `f64` - 计算结果。
    pub fn slider_value(self) -> f64 {
        match self {
            DayPhase::Noon => 0.0,
            DayPhase::Dusk => 0.5,
            DayPhase::Night => 1.0,
        }
    }

    /// 由滑块位置反推相位(四舍五入到最近的档位)。
    ///
    /// # Arguments
    ///
    /// - `f64` - 输入值。
    pub fn from_slider(value: f64) -> Self {
        let clamped: f64 = value.clamp(0.0, 1.0);
        let index: usize = (clamped * 2.0).round() as usize;
        match index {
            0 => DayPhase::Noon,
            1 => DayPhase::Dusk,
            _ => DayPhase::Night,
        }
    }

    /// 由 `?phase=` 的取值反推相位。
    ///
    /// 与 [`DayPhase::from_slider`] 同样的枚举 → 相位映射,但输入是
    /// query 参数的**原始文本**:验收链接要能一步到达任意相位
    /// (`?phase=dusk`),而正午的影子被自己挡住、黄昏的影子横穿马路,
    /// 两者只有从 URL 直接指定才拍得出对照帧。
    ///
    /// 未知取值返回 `None` 而不是落回某一档:拼错 `?phase=duks` 应当
    /// 立刻看出「参数写错了」,而不是静默出一张正午的截图,让人以为
    /// 黄昏根本没生效。
    ///
    /// # Arguments
    ///
    /// - `&str` - `location.search` 里的取值(`dusk` / `noon` / `night`)。
    ///
    /// # Returns
    ///
    /// - `Option<Self>` - 对应相位;取值不认识时为 `None`。
    pub fn from_query(value: &str) -> Option<Self> {
        match value {
            PHASE_QUERY_NOON => Some(DayPhase::Noon),
            PHASE_QUERY_DUSK => Some(DayPhase::Dusk),
            PHASE_QUERY_NIGHT => Some(DayPhase::Night),
            _ => None,
        }
    }

    /// 显示名(HTML overlay 用)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 计算结果。
    pub fn label(self) -> &'static str {
        match self {
            DayPhase::Noon => NOON,
            DayPhase::Dusk => DUSK,
            DayPhase::Night => NIGHT,
        }
    }
}

/// Inherent implementation of [`SceneLighting`].
impl SceneLighting {
    /// 取某个相位的默认光照参数。
    ///
    /// # Arguments
    ///
    /// - `DayPhase` - 输入值。
    pub fn for_phase(phase: DayPhase) -> Self {
        // 三相位的共同约束(数值都在 GPU / CPU 两端共用):
        //
        // 1. **朝光面总亮度可控。** 朝光面的线性亮度是
        //    `albedo * (ambient + light_color * n_dot_l)`。最亮的白色楼
        //    albedo 接近 0.86,如果 `ambient + light_color` 直接取到 2.0,
        //    线性值就是 1.7 —— 远超 1.0,`clamp` 之后所有浅色面一律糊成
        //    纯白,色相信息(粉 / 薄荷 / 珊瑚)在 sRGB 之前就已经被抹掉了。
        //    这里把三相位的 `ambient + light_color` 都压在 ~1.6 以内,
        //    剩下的高光交给 [`tonemap`] 的肩部压缩。
        // 2. **曝光固定为 1.0,靠白色点而不是靠压暗光强来「解决」过曝。**
        //    压暗方向光会让阴影面一起塌死;抬高白色点则保住中间调的
        //    色相,只把真正刺眼的顶端收进来。
        // 3. **夜晚不能死黑。** 环境光带一点蓝紫(城市天光 + 霓虹回弹),
        //    所以夜里路面和楼体仍然可辨,只是整体偏冷偏暗。
        match phase {
            DayPhase::Noon => Self {
                light_dir: normalize3([0.35, 0.86, 0.36]),
                // 方向光接近中性白(略偏暖),环境光偏天空蓝 —— 阴影面
                // 因此是冷蓝而不是灰,和阳光面的暖白拉开冷暖对比。
                light_color: [1.16, 1.10, 0.99],
                ambient: [0.34, 0.38, 0.46],
                sky_color: [0.44, 0.70, 0.92],
                // 半球:天光比 `ambient` 略亮偏冷,地面反弹是暖的沥青灰 ——
                // 沥青 albedo 只有 0.09,所以地面反弹**很暗**,这是对的。
                sky_ambient: [0.40, 0.46, 0.56],
                ground_ambient: [0.13, 0.12, 0.11],
                ambient_hemi: 1.0,
                // 正午是阴影最重的相位:高角度光让楼影落在街上,最显眼。
                shadow_strength: 1.0,
                ao_strength: 0.95,
                // 正午是掠射角最小的相位,湿路面反射几乎看不见 —— 强度压低,
                // 否则会在正午看到夜里才该有的反光。
                ssr_strength: 0.22,
                // 正午仍留一点湿气:柏油在雨后确实反光,而且它让路面
                // 远处的渐变不至于死板。权重低,只有掠射角才看得见。
                wetness: 0.35,
                // 冷亮:抬一点点蓝黑场,中间调轻微提蓝,亮场压一点暖。
                grade_lift: [0.002, 0.004, 0.010],
                grade_gamma: [0.99, 1.00, 1.02],
                grade_gain: [0.99, 1.00, 1.02],
                vignette: 0.26,
                grain: 0.012,
                emissive_gain: 0.18,
                exposure: 1.0,
                tone_map_white: TONE_MAP_WHITE_DAY,
                // 街区扩到 ±150 m,雾必须够远才看得到成片的街区;
                // fog_start 压到接近雾的起点,远景褪向天空色而不是突然消失。
                fog_start: 120.0,
                fog_end: 460.0,
            },
            DayPhase::Dusk => Self {
                light_dir: normalize3([0.86, 0.24, -0.44]),
                // 低角度暖橙直射 + 偏紫的天空环境光:朝光面是橙红,
                // 背光面落到冷紫,这是黄昏最主要的色相来源。
                light_color: [1.62, 0.84, 0.42],
                ambient: [0.26, 0.24, 0.40],
                sky_color: [0.95, 0.46, 0.36],
                sky_ambient: [0.34, 0.30, 0.48],
                ground_ambient: [0.20, 0.13, 0.11],
                ambient_hemi: 1.0,
                // 低角度光 = 长影子 = 黄昏的招牌画面,阴影权重甚至比正午更高。
                shadow_strength: 1.0,
                ao_strength: 0.85,
                // 掠射角大,Fresnel 强,湿路面在黄昏最出彩。
                ssr_strength: 0.85,
                wetness: 0.75,
                // 暖橙:黑场往洋红压,中间调提暖,亮场加金。
                grade_lift: [0.012, 0.004, 0.010],
                grade_gamma: [1.02, 0.99, 0.96],
                grade_gain: [1.04, 1.00, 0.95],
                vignette: 0.34,
                grain: 0.016,
                emissive_gain: 0.90,
                exposure: 1.0,
                tone_map_white: TONE_MAP_WHITE_DUSK,
                fog_start: 100.0,
                fog_end: 420.0,
            },
            DayPhase::Night => Self {
                light_dir: normalize3([-0.42, 0.72, -0.55]),
                // 月光很弱,但刻意保留蓝紫偏色;环境光是全画面「不死黑」
                // 的唯一来源 —— 路面 / 楼体靠它提亮,而不是把直射光调大。
                light_color: [0.24, 0.31, 0.58],
                ambient: [0.11, 0.14, 0.26],
                sky_color: [0.045, 0.055, 0.13],
                sky_ambient: [0.13, 0.17, 0.32],
                // 夜间地面反弹不是日光而是霓虹 —— 偏洋红,这是夜景的关键色偏。
                ground_ambient: [0.10, 0.06, 0.13],
                ambient_hemi: 1.0,
                // 月光很弱,阴影只是「比别处再暗一点点」,不是硬阴影。
                // 保留一点点(而不是 0)是为了让物体不失去体积感。
                shadow_strength: 0.34,
                ao_strength: 0.70,
                ssr_strength: 1.0,
                // 夜路最湿 —— 霓虹在积水上的倒影是夜景的全部意义。
                wetness: 1.0,
                // 冷蓝:黑场压蓝,中间调偏青,亮场压红。
                grade_lift: [0.004, 0.008, 0.024],
                grade_gamma: [1.03, 1.00, 0.95],
                grade_gain: [0.93, 0.98, 1.10],
                vignette: 0.46,
                grain: 0.022,
                emissive_gain: 1.85,
                exposure: 1.0,
                tone_map_white: TONE_MAP_WHITE_NIGHT,
                fog_start: 90.0,
                fog_end: 400.0,
            },
        }
    }
}

/// Inherent implementation of [`Instance`].
impl Instance {
    /// model matrix 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&Mat4Data` - 列主序 model matrix。
    pub fn get_model_ref(&self) -> &Mat4Data {
        &self.model
    }

    /// 用**显式 model matrix** 构造实例。
    ///
    /// 车轮要同时做两件事:跟着车身绕 Y 转,再绕自己的轮心自转。
    /// [`Self::new`] 只能表达单轴 Y 旋转,表达不了这个复合变换,所以这里
    /// 直接接受调用方算好的列主序矩阵。
    ///
    /// # Arguments
    ///
    /// - `Mat4Data` - 列主序 model matrix(长度 16)。
    /// - `Vec3` - 逐实例色调乘子。
    ///
    /// # Returns
    ///
    /// - `Self` - 构造好的实例。
    pub fn from_model(model: Mat4Data, tint: Vec3) -> Self {
        Self { model, tint }
    }

    /// 构造一个实例。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 世界坐标位置。
    /// - `f32` - 绕 Y 轴朝向(弧度)。
    /// - `f32` - 均匀缩放系数。
    /// - `Vec3` - 逐实例色调乘子。
    ///
    /// # Returns
    ///
    /// - `Self` - 构造好的实例。
    pub fn new(position: Vec3, yaw: f32, uniform_scale: f32, tint: Vec3) -> Self {
        let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
        // 绕 Y 轴旋转 + 非均匀缩放(xz 可拉伸,用于拉长车身 / 招牌)。
        let (scale_x, scale_y, scale_z): (f32, f32, f32) =
            (uniform_scale, uniform_scale, uniform_scale);
        Self {
            model: [
                cos_yaw * scale_x,
                0.0,
                -sin_yaw * scale_x,
                0.0, //
                0.0,
                scale_y,
                0.0,
                0.0, //
                sin_yaw * scale_z,
                0.0,
                cos_yaw * scale_z,
                0.0, //
                position[0],
                position[1],
                position[2],
                1.0,
            ],
            tint,
        }
    }

    /// 用一条显式的 model matrix 构造实例。
    ///
    /// 玩家的步态骨架需要「局部肢体摆动矩阵 × 整体位置 / 朝向」,不再是
    /// 单纯的 TRS,所以给渲染器开一个直接吃矩阵的入口。
    ///
    /// # Arguments
    ///
    /// - `Mat4` - 列主序的 model matrix。
    /// - `Vec3` - 逐实例色调乘子。
    ///
    /// # Returns
    ///
    /// - `Self` - 构造好的实例。
    pub fn from_matrix(model: Mat4, tint: Vec3) -> Self {
        Self {
            model: model.get_elements(),
            tint,
        }
    }

    /// 用 model matrix 变换一个点。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 输入值。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 计算结果。
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        let m: &Mat4Data = self.get_model_ref();
        let x: f32 = m[0] * point[0] + m[4] * point[1] + m[8] * point[2] + m[12];
        let y: f32 = m[1] * point[0] + m[5] * point[1] + m[9] * point[2] + m[13];
        let z: f32 = m[2] * point[0] + m[6] * point[1] + m[10] * point[2] + m[14];
        [x, y, z]
    }

    /// 法线变换:因为只用「绕 Y 旋转 + 各向同性均匀缩放」,法线的旋转部分
    /// 与顶点相同,缩放对单位法线无影响,因此直接复用同一线性变换即可。
    /// 法线变换:因为只用「绕 Y 旋转 + 各向同性均匀缩放」,法线的旋转部分
    /// 与顶点相同,缩放对单位法线无影响,因此直接复用同一线性变换即可。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 输入值。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 计算结果。
    pub fn transform_normal(&self, normal: Vec3) -> Vec3 {
        normalize3(self.transform_point(normal))
    }
}

/// Inherent implementation of [`Scene`].
impl Scene {
    /// 追加一个已解析的资产,返回其索引。
    ///
    /// # Arguments
    ///
    /// - `MeshAssetGpu` - 输入值。
    ///
    /// # Returns
    ///
    /// - `usize` - 计数结果。
    pub fn push_mesh(&mut self, mesh: MeshAssetGpu) -> usize {
        self.set_total_triangles(self.get_total_triangles() + mesh.get_triangle_count());
        let index: usize = self.get_meshes_mut().len();
        self.get_meshes_mut().push(mesh);
        index
    }

    /// 资产表的可变引用(仅供同模块内的 accessor 使用)。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<MeshAssetGpu>` - 资产表。
    pub fn get_meshes_mut(&mut self) -> &mut Vec<MeshAssetGpu> {
        &mut self.meshes
    }

    /// 新建一个批次并立即返回其索引。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `bool` - 输入值。
    ///
    /// # Returns
    ///
    /// - `usize` - 计数结果。
    pub fn push_batch(&mut self, mesh_index: usize, opaque: bool) -> usize {
        self.push_batch_with_cull(mesh_index, opaque, true)
    }

    /// 新建一个批次,并显式指定它是否参与近处遮挡剔除。
    ///
    /// # Arguments
    ///
    /// - `usize` - 资产索引。
    /// - `bool` - 是否参与深度测试 / 背面剔除。
    /// - `bool` - 是否参与近处遮挡剔除。
    ///
    /// # Returns
    ///
    /// - `usize` - 新批次的索引。
    pub fn push_batch_with_cull(
        &mut self,
        mesh_index: usize,
        opaque: bool,
        near_cull: bool,
    ) -> usize {
        let index: usize = self.get_batches_mut().len();
        self.get_batches_mut().push(SceneBatch {
            mesh_index,
            instances: Vec::new(),
            opaque,
            near_cull,
        });
        index
    }

    /// 批次表的可变引用(仅供同模块内的 accessor 使用)。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<SceneBatch>` - 批次表。
    pub fn get_batches_mut(&mut self) -> &mut Vec<SceneBatch> {
        &mut self.batches
    }

    /// 向批次追加一个实例。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `Instance` - 输入值。
    pub fn push_instance(&mut self, batch_index: usize, instance: Instance) {
        self.get_batches_mut()[batch_index].instances.push(instance);
    }

    /// 三角形总数减去被背面剔除的(粗估,用于 HUD)。
    ///
    /// # Returns
    ///
    /// - `usize` - 计数结果。
    pub fn instance_count(&self) -> usize {
        self.get_batches()
            .iter()
            .map(|batch: &SceneBatch| batch.get_instances().len())
            .sum()
    }
    /// 场景批次列表的只读引用。
    ///
    /// # Returns
    ///
    /// - `&Vec<SceneBatch>` - 与 `&self` 同生命周期,不复制。
    pub(crate) fn get_batches_ref(&self) -> &Vec<SceneBatch> {
        &self.batches
    }
}

/// Inherent implementation of [`QualityTier`].
impl QualityTier {
    /// 该档位下是否跑 SSR。
    ///
    /// # Returns
    ///
    /// - `bool` - `true` 表示开启。
    pub fn wants_ssr(&self) -> bool {
        *self == QualityTier::High
    }

    /// 档位名(给调试探针用)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - `High` / `Medium` / `Low`。
    pub fn tier_name(&self) -> &'static str {
        match self {
            QualityTier::High => QUALITY_NAME_HIGH,
            QualityTier::Medium => QUALITY_NAME_MEDIUM,
            QualityTier::Low => QUALITY_NAME_LOW,
        }
    }

    /// 该档位下是否跑 SSAO。
    ///
    /// # Returns
    ///
    /// - `bool` - `true` 表示开启。
    pub fn wants_ssao(&self) -> bool {
        *self != QualityTier::Low
    }
}

/// Inherent implementation of [`AdaptiveQuality`].
impl AdaptiveQuality {
    /// 初始为 [`QualityTier::High`],尚未采样。
    ///
    /// # Returns
    ///
    /// - `Self` - 全新状态机。
    pub fn new() -> Self {
        Self {
            tier: QualityTier::High,
            smoothed_fps: 0.0,
            slow_frames: 0,
            fast_frames: 0,
            samples: 0,
        }
    }

    /// 当前的画质档位。
    ///
    /// # Returns
    ///
    /// - `QualityTier` - 档位。
    pub fn tier(&self) -> QualityTier {
        self.get_tier()
    }

    /// 喂入一帧的实测帧率,必要时降档。
    ///
    /// **要连续若干帧都慢才降档。** 单帧的尖峰(资产加载、GC、标签页
    /// 切回)不应该触发降级,所以用 `SLOW_FRAME_THRESHOLD` 帧的滑动
    /// 计数;帧率用指数平滑,单帧抖动不会让档位来回跳。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧的帧率(fps)。小于等于 0 的值会被忽略(分母为零)。
    pub fn sample(&mut self, fps: f32) {
        if fps <= 0.0 || !fps.is_finite() {
            return;
        }
        if self.get_samples() == 0 {
            self.set_smoothed_fps(fps);
        } else {
            self.set_smoothed_fps(
                self.get_smoothed_fps() * FPS_SMOOTHING + fps * (1.0 - FPS_SMOOTHING),
            );
        }
        self.set_samples(self.get_samples() + 1);
        // 前几秒不判 —— 管线刚建立起来的第一帧总是最慢的。
        if self.get_samples() < QUALITY_WARMUP_FRAMES {
            return;
        }
        if self.get_smoothed_fps() < QUALITY_DOWN_FPS {
            self.set_slow_frames(self.get_slow_frames() + 1);
            self.set_fast_frames(0);
        } else if self.get_smoothed_fps() > QUALITY_UP_FPS {
            self.set_fast_frames(self.get_fast_frames() + 1);
            self.set_slow_frames(0);
        } else {
            self.set_slow_frames(0);
            self.set_fast_frames(0);
        }
        if self.get_slow_frames() >= SLOW_FRAME_THRESHOLD {
            self.set_slow_frames(0);
            self.set_tier(match self.get_tier() {
                QualityTier::High => QualityTier::Medium,
                QualityTier::Medium => QualityTier::Low,
                QualityTier::Low => QualityTier::Low,
            });
        }
        // 升档只在明确很快、且已经稳定很久时发生,避免抖动。
        if self.get_fast_frames() >= FAST_FRAME_THRESHOLD && self.get_tier() != QualityTier::High {
            // 单向降级:这里刻意**不**执行升档。
            self.set_fast_frames(0);
        }
    }
}

/// Inherent implementation of [`RenderTarget`].
impl RenderTarget {
    /// 分配一张颜色纹理(RGBA8,线性过滤,边缘钳制)。
    ///
    /// 颜色附件的过滤**必须**是 `LINEAR`:bloom 的模糊与 SSAO 的上采样
    /// 都依赖硬件双线性,`NEAREST` 会让半分辨率的 pass 出现方块。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 纹理宽度(像素)。
    /// - `i32` - 纹理高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlTexture, String>` - 分配好的纹理。
    fn create_color_texture(
        context: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<WebGlTexture, String> {
        let texture: WebGlTexture = context
            .create_texture()
            .ok_or_else(|| CREATE_TEXTURE_FAILED.to_string())?;
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&texture));
        context
            .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                WebGl2RenderingContext::TEXTURE_2D,
                0,
                WebGl2RenderingContext::RGBA8 as i32,
                width,
                height,
                0,
                WebGl2RenderingContext::RGBA,
                WebGl2RenderingContext::UNSIGNED_BYTE,
                None,
            )
            .map_err(|_| CREATE_TEXTURE_FAILED.to_string())?;
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            WebGl2RenderingContext::LINEAR as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            WebGl2RenderingContext::LINEAR as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, None);
        Ok(texture)
    }

    /// 分配一张深度纹理(DEPTH_COMPONENT24)。
    ///
    /// **必须是 `DEPTH_COMPONENT24`,不能是 `R32F` 颜色纹理。**
    /// `R32F` 只有通过 `EXT_color_buffer_float` 才是**可渲染**格式,
    /// 而即便扩展存在,把它挂到 `DEPTH_ATTACHMENT` 上也只在部分实现里
    /// 被接受 —— 实测这台机器的 WebGL2(SwiftShader)直接判
    /// `INCOMPLETE_ATTACHMENT`(36054),于是 `checkFramebufferStatus`
    /// 说「不完整」,而更糟的是清除与绘制都静默失效:
    /// `glClear` 不写入、每个 draw call 被丢弃,画面全黑。
    ///
    /// 阴影 pass 用的是「手动比较」(`texture(u_shadow_map, uv).r` 再和
    /// 片元深度比),所以换成 `DEPTH_COMPONENT24` 不需要改采样方式 ——
    /// 采样回来的仍然是单个 `.r` 分量。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 纹理宽度(像素)。
    /// - `i32` - 纹理高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlTexture, String>` - 分配好的深度纹理。
    ///
    fn create_depth_texture(
        context: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<WebGlTexture, String> {
        let texture: WebGlTexture = context
            .create_texture()
            .ok_or_else(|| CREATE_TEXTURE_FAILED.to_string())?;
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&texture));
        context
            .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                WebGl2RenderingContext::TEXTURE_2D,
                0,
                WebGl2RenderingContext::DEPTH_COMPONENT24 as i32,
                width,
                height,
                0,
                WebGl2RenderingContext::DEPTH_COMPONENT,
                WebGl2RenderingContext::UNSIGNED_INT,
                None,
            )
            .map_err(|_| CREATE_TEXTURE_FAILED.to_string())?;
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            WebGl2RenderingContext::NEAREST as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            WebGl2RenderingContext::NEAREST as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, None);
        Ok(texture)
    }

    /// 分配一个「颜色 + 深度」渲染目标(主场景、G-buffer、SSR / bloom 中转)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 目标宽度(像素)。
    /// - `i32` - 目标高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 分配好的渲染目标。
    fn new_color(
        context: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let color: WebGlTexture = Self::create_color_texture(context, width, height)?;
        let depth: WebGlTexture = Self::create_depth_texture(context, width, height)?;
        let fbo: WebGlFramebuffer = context
            .create_framebuffer()
            .ok_or_else(|| CREATE_FRAMEBUFFER_FAILED.to_string())?;
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fbo));
        context.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::COLOR_ATTACHMENT0,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(&color),
            0,
        );
        context.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::DEPTH_ATTACHMENT,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(&depth),
            0,
        );
        let target: Self = Self {
            fbo,
            color: Some(color),
            depth,
            width,
            height,
        };
        if !target.is_complete(context) {
            return Err(FRAMEBUFFER_INCOMPLETE.to_string());
        }
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None);
        Ok(target)
    }

    /// 分配一个「只有深度」的渲染目标(阴影贴图)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 目标边长(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 分配好的渲染目标。
    fn new_depth(context: &WebGl2RenderingContext, size: i32) -> Result<Self, String> {
        let depth: WebGlTexture = Self::create_depth_texture(context, size, size)?;
        let fbo: WebGlFramebuffer = context
            .create_framebuffer()
            .ok_or_else(|| CREATE_FRAMEBUFFER_FAILED.to_string())?;
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fbo));
        context.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::DEPTH_ATTACHMENT,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(&depth),
            0,
        );
        // 没有颜色附件时必须显式声明「不画颜色」,否则 FBO 不完整。
        context.draw_buffers(&js_sys::Array::of1(&JsValue::from_f64(
            WebGl2RenderingContext::NONE as f64,
        )));
        context.read_buffer(WebGl2RenderingContext::NONE);
        let target: Self = Self {
            fbo,
            color: None,
            depth,
            width: size,
            height: size,
        };
        if !target.is_complete(context) {
            return Err(FRAMEBUFFER_INCOMPLETE.to_string());
        }
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None);
        Ok(target)
    }

    /// 绑为当前渲染目标(`fbo` 为 `None` 时绑回屏幕)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `Option<&Self>` - 要绑定的目标;`None` 表示屏幕。
    fn bind(context: &WebGl2RenderingContext, target: Option<&Self>) {
        match target {
            Some(target) => context
                .bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&target.get_fbo())),
            None => context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None),
        }
    }

    /// 当前的 FBO 是否完整。
    ///
    /// 少了这一步的后果不是「没有画面」而是**画错**:不完整的 FBO 上
    /// 任何 draw call 都会被驱动丢弃,而我们仍然会继续跑后面所有 pass,
    /// 最终把一张空纹理当成 bloom 贴进主画面 —— 整屏发白。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    ///
    /// # Returns
    ///
    /// - `bool` - 完整时为 `true`。
    fn is_complete(&self, context: &WebGl2RenderingContext) -> bool {
        // **不要**拿返回值去和 `WebGl2RenderingContext::FRAMEBUFFER_COMPLETE`
        // 比。web-sys 把这个常量按 WebGL 1 的值(36053)写死,但 WebGL2 里
        // `checkFramebufferStatus` 返回的是 WebGL2 语义的 36054;两者是
        // 同一个「完整」状态,只是编号差一。于是这个比较恒为假 —— 每个
        // FBO 明明是好的,却被判成不完整,每帧都回退软件渲染,画面全黑。
        //
        // 规范里 `FRAMEBUFFER_COMPLETE` 是**最小**的状态码(36053..36061
        // 全是各种 INCOMPLETE),所以「返回值落在完整区间」这个判据在两套
        // 编号下都成立,也不依赖任何驱动私有编号。
        let status: u32 = context.check_framebuffer_status(WebGl2RenderingContext::FRAMEBUFFER);
        status == WebGl2RenderingContext::FRAMEBUFFER_COMPLETE
            || status == FRAMEBUFFER_COMPLETE_WEBGL2_OFFSET
    }
}

/// Inherent implementation of [`WebGlRenderer`].
impl WebGlRenderer {
    /// GL 上下文的克隆句柄。
    ///
    /// # Returns
    ///
    /// 主 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// 泛光 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// 阴影 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// instance buffer 的克隆句柄。
    ///
    /// # Returns
    ///
    /// 读 `targets` 字段(增强管线的全部离屏目标)。
    ///
    /// # Returns
    ///
    /// 写 `targets` 字段(增强管线的全部离屏目标)。
    ///
    /// # Arguments
    ///
    /// - `PipelineTargets` - 新的离屏目标集合。
    pub(crate) fn set_targets(&mut self, value: PipelineTargets) {
        self.targets = value;
    }

    /// SSAO program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// AO 模糊 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// SSR program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// bloom 亮度提取 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// bloom 模糊 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// 合成 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// G-buffer program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// 取出复用缓冲的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Instance>` - 复用缓冲。
    pub fn get_scratch_mut(&mut self) -> &mut Vec<Instance> {
        &mut self.scratch
    }

    /// 越界批次累计数自增(验收探针,见 [`Self::get_gpu_mesh_index_oob`])。
    ///
    /// # Returns
    ///
    /// - `usize` - 自增后的累计越界次数。
    pub fn set_gpu_mesh_index_oob_bumped(&mut self) -> usize {
        self.gpu_mesh_index_oob += 1;
        self.gpu_mesh_index_oob
    }

    /// `mesh_index` 越界的批次累计数(验收探针)。
    ///
    /// # Returns
    ///
    /// - `usize` - 越界次数;非 0 即表示 GPU 表与批次表已错位。
    pub fn get_gpu_index_oob(&self) -> usize {
        self.gpu_mesh_index_oob
    }

    /// GPU 侧资产表的只读视图。
    ///
    /// # Returns
    ///
    /// GPU 侧资产表的可变引用(仅供 accessor 使用)。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<GlMesh>` - GPU 侧资产表。
    fn get_meshes_mut(&mut self) -> &mut Vec<GlMesh> {
        &mut self.meshes
    }

    /// 编译着色器;失败时返回 info log。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `u32` - 输入值。
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlShader, String>` - 计算结果。
    ///   编译着色器;失败时返回 info log。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `u32` - 输入值。
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlShader, String>` - 计算结果。
    fn compile_shader(
        context: &WebGl2RenderingContext,
        type_: u32,
        source: &str,
    ) -> Result<WebGlShader, String> {
        let shader: WebGlShader = context
            .create_shader(type_)
            .ok_or_else(|| CREATE_SHADER_RETURNED_NULL.to_string())?;
        context.shader_source(&shader, source);
        context.compile_shader(&shader);
        let status: bool = context
            .get_shader_parameter(&shader, WebGl2RenderingContext::COMPILE_STATUS)
            .as_bool()
            .unwrap_or(false);
        if !status {
            let log: String = context
                .get_shader_info_log(&shader)
                .unwrap_or_else(|| NO_INFO_LOG.to_string());
            context.delete_shader(Some(&shader));
            return Err(format!("shader compile failed: {log}"));
        }
        Ok(shader)
    }

    /// 链接一个 program。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlProgram, String>` - 计算结果。
    ///   链接一个 program。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&str` - 顶点着色器源码。
    /// - `&str` - 片元着色器源码。
    /// - `&str` - program 的名字,只用于让链接错误信息能指出是哪个 pass。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlProgram, String>` - 链接好的 program。
    fn link_program(
        context: &WebGl2RenderingContext,
        vertex_source: &str,
        fragment_source: &str,
        label: &str,
    ) -> Result<WebGlProgram, String> {
        let vertex: WebGlShader = Self::compile_shader(
            context,
            WebGl2RenderingContext::VERTEX_SHADER,
            vertex_source,
        )?;
        let fragment: WebGlShader = Self::compile_shader(
            context,
            WebGl2RenderingContext::FRAGMENT_SHADER,
            fragment_source,
        )?;
        let program: WebGlProgram = context
            .create_program()
            .ok_or_else(|| CREATE_PROGRAM_RETURNED_NULL.to_string())?;
        context.attach_shader(&program, &vertex);
        context.attach_shader(&program, &fragment);
        context.link_program(&program);
        context.delete_shader(Some(&vertex));
        context.delete_shader(Some(&fragment));
        let status: bool = context
            .get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);
        if !status {
            let log: String = context
                .get_program_info_log(&program)
                .unwrap_or_else(|| NO_INFO_LOG.to_string());
            return Err(format!("program link failed [{label}]: {log}"));
        }
        Ok(program)
    }

    /// 在已有 canvas 上创建 WebGL2 后端;任何一步失败都返回 `Err`,
    /// 由调用方回退到 Canvas2D 软件渲染。
    /// 在已有 canvas 上创建 WebGL2 后端;任何一步失败都返回 `Err`,
    /// 由调用方回退到 Canvas2D 软件渲染。
    ///
    /// # Arguments
    ///
    /// - `&HtmlCanvasElement` - HtmlCanvasElement 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 计算结果。
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        // `getContext` 的第二个参数是 `ContextAttributes`:
        // - `depth: true`  —— 要深度缓冲;
        // - `antialias: false` —— 城市是硬边低多边形,MSAA 在软件光栅上
        //   代价太高,换来的是肉眼几乎看不出的边缘;
        // - **`depth` 不指定 `stencil: false`** 会让浏览器连 16 位模板一起
        //   分配,在 swiftshader 上深度缓冲可能因此被降到 16 位,近距离
        //   几何全部 z-fighting(第三人称贴着地面看只剩一片单色)。
        let attributes: JsValue = JsValue::from(js_sys::Object::new());
        for (name, value) in [
            (crate::r#config::GL_ATTR_ALPHA, false),
            (crate::r#config::GL_ATTR_DEPTH, true),
            (crate::r#config::GL_ATTR_STENCIL, false),
            (crate::r#config::GL_ATTR_ANTIALIAS, false),
            (crate::r#config::GL_ATTR_PRESERVE_DRAWING_BUFFER, true),
        ] {
            let _: Result<bool, JsValue> = js_sys::Reflect::set(
                &attributes,
                &JsValue::from_str(name),
                &JsValue::from_bool(value),
            );
        }
        let context: WebGl2RenderingContext = canvas
            .get_context_with_context_options(WEBGL2_2, &attributes)
            .map_err(|err: JsValue| format!("get_context threw: {err:?}"))?
            .ok_or_else(|| WEBGL2_UNAVAILABLE.to_string())?
            .dyn_into_webgl();
        let program: WebGlProgram =
            Self::link_program(&context, VERTEX_SHADER, FRAGMENT_SHADER, PROGRAM_MAIN)?;
        let glow_program: WebGlProgram =
            Self::link_program(&context, VERTEX_SHADER, GLOW_FRAGMENT_SHADER, PROGRAM_GLOW)?;
        let shadow_program: WebGlProgram = Self::link_program(
            &context,
            SHADOW_VERTEX_SHADER,
            SHADOW_FRAGMENT_SHADER,
            PROGRAM_SHADOW,
        )?;
        let gbuffer_program: WebGlProgram = Self::link_program(
            &context,
            VERTEX_SHADER,
            GBUFFER_FRAGMENT_SHADER,
            PROGRAM_GBUFFER,
        )?;
        let ssao_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            SSAO_FRAGMENT_SHADER,
            PROGRAM_SSAO,
        )?;
        let ao_blur_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            AO_BLUR_FRAGMENT_SHADER,
            PROGRAM_AO_BLUR,
        )?;
        let ssr_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            SSR_FRAGMENT_SHADER,
            PROGRAM_SSR,
        )?;
        let bright_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            BLOOM_EXTRACT_FRAGMENT_SHADER,
            PROGRAM_BRIGHT,
        )?;
        let blur_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            BLOOM_BLUR_FRAGMENT_SHADER,
            PROGRAM_BLUR,
        )?;
        let composite_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            COMPOSITE_FRAGMENT_SHADER,
            PROGRAM_COMPOSITE,
        )?;
        let instance_buffer: WebGlBuffer = context
            .create_buffer()
            .ok_or_else(|| CREATE_BUFFER_FAILED.to_string())?;
        // **必须在任何 VAO 引用它之前就分配存储。**
        //
        // `upload_mesh` 会给每个资产的 VAO 调 `vertex_attrib_pointer`,
        // 那时会把 instance buffer 记录进 VAO。如果此刻它还是 0 字节,
        // 后面 `reserve_instances` 里的 `bufferData` 只是扩容,已经建好的
        // VAO 不会重新绑定 —— 在部分驱动(SwiftShader / ANGLE)上
        // attribute 会一直读到未初始化内容,表现为整屏的彩色大三角面。
        // 一次性按 MAX 批大小预分配,之后只做 `bufferSubData` 覆写。
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&instance_buffer));
        context.buffer_data_with_i32(
            WebGl2RenderingContext::ARRAY_BUFFER,
            (INSTANCE_PREALLOC * FLOATS_PER_INSTANCE * 4) as i32,
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, None);
        let u_view_proj: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_VIEW_PROJ);
        let u_light_dir: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_LIGHT_DIR);
        let u_light_color: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_LIGHT_COLOR);
        let u_ambient: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AMBIENT);
        let u_sky_color: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_SKY_COLOR);
        let u_emissive_gain: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_EMISSIVE_GAIN);
        let u_fog: Option<WebGlUniformLocation> = context.get_uniform_location(&program, U_FOG);
        let u_eye: Option<WebGlUniformLocation> = context.get_uniform_location(&program, U_EYE);
        let u_exposure: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_EXPOSURE);
        let u_tone_map_white: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_TONE_MAP_WHITE);
        let u_glow_strength: Option<WebGlUniformLocation> =
            context.get_uniform_location(&glow_program, U_GLOW_STRENGTH);
        // 阴影 pass 用同一个 `u_view_proj` 名字,但绑在 shadow_program 上。
        let u_shadow_view_proj: Option<WebGlUniformLocation> =
            context.get_uniform_location(&shadow_program, U_VIEW_PROJ);
        let u_shadow_texel: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_SHADOW_TEXEL);
        let u_composite_bloom_strength: Option<WebGlUniformLocation> =
            context.get_uniform_location(&composite_program, U_BLOOM_STRENGTH);
        let u_wet_height: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_WET_HEIGHT);
        let u_ao_height: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_HEIGHT);
        let u_ao_floor: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_FLOOR);
        let u_ao_reach: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_REACH);
        let u_ao_feather: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_FEATHER);
        let u_ao_min_height: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_MIN_HEIGHT);
        let gl_context: WebGl2RenderingContext = context.clone();
        let uniform_shadow_map: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SHADOW_MAP);
        let uniform_shadow_matrix: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SHADOW_MATRIX);
        let uniform_shadow_params: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SHADOW_PARAMS);
        let uniform_sky_ambient: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SKY_AMBIENT);
        let uniform_ground_ambient: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_GROUND_AMBIENT);
        let uniform_ambient_hemi: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_AMBIENT_HEMI);
        let uniform_ao_map: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_AO_MAP);
        let uniform_ao_strength: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_AO_STRENGTH);
        let uniform_ssr_map: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SSR_MAP);
        let uniform_ssr_strength: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SSR_STRENGTH);
        let uniform_wetness: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_WETNESS);
        let uniform_gbuffer_view: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&gbuffer_program, U_VIEW);
        let uniform_gbuffer_far: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&gbuffer_program, U_FAR_PLANE);
        let uniform_ssao_gbuffer: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_GBUFFER);
        let uniform_ssao_texel: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_TEXEL_SIZE);
        let uniform_ssao_radius: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_AO_RADIUS);
        let uniform_ssao_power: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_AO_POWER);
        let uniform_ssao_proj: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_PROJ_PARAMS);
        let uniform_ssao_far: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_FAR_PLANE);
        let uniform_ssao_samples: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_SAMPLES);
        let uniform_blur_ao: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ao_blur_program, U_AO_BLUR_MAP);
        let uniform_blur_texel: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ao_blur_program, U_TEXEL_SIZE);
        let uniform_blur_radius: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ao_blur_program, U_BLUR_RADIUS);
        let uniform_ssr_gbuffer: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_GBUFFER);
        let uniform_ssr_scene: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_SCENE_COLOR);
        let uniform_ssr_texel: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_TEXEL_SIZE);
        let uniform_ssr_proj: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_PROJ_PARAMS);
        let uniform_ssr_far: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_FAR_PLANE);
        let uniform_ssr_max_dist: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_SSR_MAX_DIST);
        let uniform_ssr_steps: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_SSR_STEPS);
        let uniform_bright_scene: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&bright_program, U_SCENE_COLOR);
        let uniform_bright_threshold: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&bright_program, U_BLOOM_THRESHOLD);
        let uniform_blur_source: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&blur_program, U_SCENE_COLOR);
        let uniform_blur_direction: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&blur_program, U_BLOOM_DIR);
        let uniform_composite_scene: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_SCENE_COLOR);
        let uniform_composite_bloom: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_BLOOM_MAP);
        let uniform_composite_lift: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRADE_LIFT);
        let uniform_composite_gamma: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRADE_GAMMA);
        let uniform_composite_gain: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRADE_GAIN);
        let uniform_composite_vignette: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_VIGNETTE);
        let uniform_composite_grain: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRAIN);
        let uniform_composite_time: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_TIME);
        let renderer: Self = Self {
            context,
            program,
            glow_program,
            shadow_program,
            gbuffer_program,
            ssao_program,
            ao_blur_program,
            ssr_program,
            bright_program,
            blur_program,
            composite_program,
            meshes: Vec::new(),
            uniform_view_proj: u_view_proj,
            uniform_light_dir: u_light_dir,
            uniform_light_color: u_light_color,
            uniform_ambient: u_ambient,
            uniform_sky_color: u_sky_color,
            uniform_emissive_gain: u_emissive_gain,
            uniform_fog: u_fog,
            uniform_eye: u_eye,
            uniform_exposure: u_exposure,
            uniform_tone_map_white: u_tone_map_white,
            uniform_glow_strength: u_glow_strength,
            uniform_shadow_map,
            uniform_shadow_matrix,
            uniform_shadow_params,
            uniform_shadow_texel: u_shadow_texel,
            uniform_shadow_view_proj: u_shadow_view_proj,
            uniform_sky_ambient,
            uniform_ground_ambient,
            uniform_ambient_hemi,
            uniform_ao_map,
            uniform_ao_strength,
            uniform_ssr_map,
            uniform_ssr_strength,
            uniform_wetness,
            uniform_wet_height: u_wet_height,
            uniform_ao_height: u_ao_height,
            uniform_ao_floor: u_ao_floor,
            uniform_ao_reach: u_ao_reach,
            uniform_ao_feather: u_ao_feather,
            uniform_ao_min_height: u_ao_min_height,
            uniform_gbuffer_view,
            uniform_gbuffer_far,
            uniform_ssao_gbuffer,
            uniform_ssao_texel,
            uniform_ssao_radius,
            uniform_ssao_power,
            uniform_ssao_proj,
            uniform_ssao_far,
            uniform_ssao_samples,
            uniform_blur_ao,
            uniform_blur_texel,
            uniform_blur_radius,
            uniform_ssr_gbuffer,
            uniform_ssr_scene,
            uniform_ssr_texel,
            uniform_ssr_proj,
            uniform_ssr_far,
            uniform_ssr_max_dist,
            uniform_ssr_steps,
            uniform_bright_scene,
            uniform_bright_threshold,
            uniform_blur_source,
            uniform_blur_direction,
            uniform_composite_scene,
            uniform_composite_bloom,
            uniform_composite_lift,
            uniform_composite_gamma,
            uniform_composite_gain,
            uniform_composite_bloom_strength: u_composite_bloom_strength,
            uniform_composite_vignette,
            uniform_composite_grain,
            uniform_composite_time,
            instance_buffer,
            instance_capacity: INSTANCE_PREALLOC,
            scratch: Vec::new(),
            gpu_mesh_count: 0,
            gpu_index_count: 0,
            gpu_mesh_index_oob: 0,
            targets: PipelineTargets::default(),
        };
        let _: &WebGl2RenderingContext = gl_context.as_ref();
        gl_context.enable(WebGl2RenderingContext::DEPTH_TEST);
        gl_context.depth_func(WebGl2RenderingContext::LESS);
        gl_context.enable(WebGl2RenderingContext::CULL_FACE);
        gl_context.cull_face(WebGl2RenderingContext::BACK);
        gl_context.enable(WebGl2RenderingContext::BLEND);
        gl_context.blend_func(
            WebGl2RenderingContext::SRC_ALPHA,
            WebGl2RenderingContext::ONE,
        );
        Ok(renderer)
    }

    /// 上传一个资产的顶点 / 索引,并创建带实例布局的 VAO。
    ///
    /// **每个 mesh 只能调用一次。** 本函数是 `push` 语义:在
    /// `self.meshes` 尾部追加并返回 `len - 1`。而 `SceneBatch::mesh_index`
    /// 是 `Scene::meshes` 的下标,`draw_batch` 把它**直接**当作
    /// `self.meshes` 的下标来取 VAO —— 两者只有在「每个 `Scene::meshes`
    /// 元素恰好上传一次」时才成立。
    ///
    /// 同一个 mesh 上传两次会让 GPU 表多出一整轮,之后所有下标整体错位:
    /// 尾部新加的批次(玩家 13 个骨架 part)取到的是**重传那一轮的旧
    /// 资产**,于是几栋几十米高的楼被按 1.75 米小人的 model matrix 摆到
    /// 玩家脚下糊满屏幕,而角色真正的 mesh 一次都没画过。
    ///
    /// # Arguments
    ///
    /// - `&MeshAssetGpu` - MeshAssetGpu 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<usize, String>` - 新追加的 GPU mesh 索引。
    pub fn upload_mesh(&mut self, mesh: &MeshAssetGpu) -> Result<usize, String> {
        let context: WebGl2RenderingContext = self.get_context().clone();
        let vertex_array: WebGlVertexArrayObject = context
            .create_vertex_array()
            .ok_or_else(|| CREATE_VERTEX_ARRAY_FAILED.to_string())?;
        context.bind_vertex_array(Some(&vertex_array));
        let vertex_buffer: WebGlBuffer = context
            .create_buffer()
            .ok_or_else(|| CREATE_BUFFER_FAILED.to_string())?;
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&vertex_buffer));
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            f32_slice_to_bytes(&mesh.get_vertices()),
            WebGl2RenderingContext::STATIC_DRAW,
        );
        let stride: i32 = (STRIDE_FLOATS * 4) as i32;
        for attribute in 0..4u32 {
            context.enable_vertex_attrib_array(attribute);
            context.vertex_attrib_pointer_with_i32(
                attribute,
                3,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                (attribute as i32) * 12,
            );
        }
        // 顶点属性 9-11:`uv` 占 float 12/13,`tangent(xyz, w)` 占 float 14-17。
        // 切线拆成两个 vec2(属性 10 = xy、属性 11 = zw),因为 vec4 单属性会
        // 吃掉 4 个连续 slot,而 4-8 已经被 instanced 属性占用。
        for (attribute, component_offset) in [(9u32, 12i32), (10u32, 14), (11u32, 16), (12u32, 18)]
        {
            context.enable_vertex_attrib_array(attribute);
            context.vertex_attrib_pointer_with_i32(
                attribute,
                2,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                component_offset * 4,
            );
        }
        let index_buffer: WebGlBuffer = context
            .create_buffer()
            .ok_or_else(|| CREATE_BUFFER_FAILED.to_string())?;
        context.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(&index_buffer),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            crate::mesh::u32_slice_to_bytes(&mesh.get_indices()),
            WebGl2RenderingContext::STATIC_DRAW,
        );
        // 实例属性:model matrix 的 4 个 vec4 + tint。
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&self.get_instance_buffer()),
        );
        for attribute in 4..8u32 {
            context.enable_vertex_attrib_array(attribute);
            context.vertex_attrib_pointer_with_i32(
                attribute,
                4,
                WebGl2RenderingContext::FLOAT,
                false,
                INSTANCE_STRIDE_BYTES,
                (attribute as i32 - 4) * 16,
            );
            context.vertex_attrib_divisor(attribute, 1);
        }
        context.enable_vertex_attrib_array(8);
        context.vertex_attrib_pointer_with_i32(
            8,
            3,
            WebGl2RenderingContext::FLOAT,
            false,
            INSTANCE_STRIDE_BYTES,
            64,
        );
        context.vertex_attrib_divisor(8, 1);
        context.bind_vertex_array(None);
        let index_count: i32 = mesh.get_indices().len() as i32;
        self.get_meshes_mut().push(GlMesh {
            vertex_array,
            vertex_buffer,
            index_buffer,
            index_count,
        });
        Ok(self.get_meshes().len() - 1)
    }

    /// 就地替换某个已上传 mesh 的顶点 / 索引数据。
    ///
    /// **这是程序化无限世界的前提。** `upload_mesh` 是 `push` 语义,只能
    /// 在启动时把 `Scene::meshes` 一一对应地传上 GPU 一次。但地面 / 水面
    /// 跟着玩家流式重建(`rebuild_streamed_surface` 原地改写
    /// `scene.meshes[i]`),CPU 侧的网格换了而 GPU 缓冲还指着旧数据 ——
    /// 走出街区边界后画出来的仍是出生点那块地。
    ///
    /// 这里**不能**改用 `upload_mesh` 重传:那是 `push` 语义,GPU 表会
    /// 多出一整条,而 `SceneBatch::mesh_index` 是直接当 GPU 表下标用的
    ///(`draw_batch`),之后所有批次整体错位一格 —— 角色 / 车会取到别人
    /// 的网格,表现就是「移动时丢失建模、只剩轮子」。原地重传保持 GPU
    /// 表长度不变,下标**永久有效**。
    ///
    /// # Arguments
    ///
    /// - `usize` - `Scene::meshes` 的下标(即批次里的 `mesh_index`)。
    /// - `&MeshAssetGpu` - 新的顶点 / 索引数据。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 下标越界时的错误信息。
    pub fn replace_mesh(&mut self, mesh_index: usize, mesh: &MeshAssetGpu) -> Result<(), String> {
        let context: WebGl2RenderingContext = self.get_context().clone();
        let Some(slot) = self.get_meshes_mut().get_mut(mesh_index) else {
            return Err(format!(
                "{}: {mesh_index} of {}",
                REPLACE_MESH_OUT_OF_RANGE,
                self.get_meshes().len()
            ));
        };
        // 顶点:先绑 VAO(索引缓冲是它的状态),再重传并重设属性指针。
        context.bind_vertex_array(Some(&slot.get_vertex_array()));
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&slot.get_vertex_buffer()),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            f32_slice_to_bytes(&mesh.get_vertices()),
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        let stride: i32 = (STRIDE_FLOATS * 4) as i32;
        for attribute in 0..4u32 {
            context.vertex_attrib_pointer_with_i32(
                attribute,
                3,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                (attribute as i32) * 12,
            );
        }
        // 与 `upload_mesh` 相同的 PBR 属性:uv = float 12/13,
        // tangent(xyz, w) 拆成 float 14-15 与 16-17 两个 vec2。
        for (attribute, component_offset) in [(9u32, 12i32), (10u32, 14), (11u32, 16), (12u32, 18)]
        {
            context.vertex_attrib_pointer_with_i32(
                attribute,
                2,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                component_offset * 4,
            );
        }
        context.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(&slot.get_index_buffer()),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            crate::mesh::u32_slice_to_bytes(&mesh.get_indices()),
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        context.bind_vertex_array(None);
        slot.set_index_count(mesh.get_indices().len() as i32);
        Ok(())
    }

    /// 保证 instance buffer 至少能装下 `capacity` 个实例。
    ///
    /// 每实例 28 个 f32(model matrix 16 + tint 3,加上对齐填充共 28 = 7×vec4)。
    /// 保证 instance buffer 至少能装下 `capacity` 个实例。
    ///
    /// 每实例 28 个 f32(model matrix 16 + tint 3,加上对齐填充共 28 = 7×vec4)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    pub fn reserve_instances(&mut self, capacity: usize) {
        if capacity <= self.get_instance_capacity() {
            return;
        }
        let context: WebGl2RenderingContext = self.get_context().clone();
        let next: usize = capacity.next_power_of_two();
        let bytes: Vec<u8> = vec![0u8; next * FLOATS_PER_INSTANCE * 4];
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&self.get_instance_buffer()),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            &bytes,
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        self.set_instance_capacity(next);
    }

    /// 按需分配 / 重建整条增强管线的离屏目标。
    ///
    /// 画布尺寸不变时**直接返回**:2048² 的阴影贴图 + 5 张全分辨率
    /// 纹理每帧重建一次会直接吃掉整个帧预算。
    ///
    /// # Arguments
    ///
    /// - `u32` - 画布宽度(像素)。
    /// - `u32` - 画布高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 分配失败时的错误信息。
    fn ensure_targets(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.get_targets().get_allocated() == (width, height)
            && self.get_targets().get_scene().is_some()
        {
            return Ok(());
        }
        let context: WebGl2RenderingContext = self.get_context().clone();
        // 每张目标各按自己的缩放系数算尺寸。G-buffer 走全分辨率
        // (SSAO 的边缘质量完全由它决定),AO / SSR / bloom 走半分辨率
        // —— 后三者都要在后面做一次模糊,半分辨率几乎无损。
        let scaled: fn(u32, f32) -> i32 =
            |base: u32, factor: f32| (((base as f32) * factor).max(1.0)) as i32;
        let w: i32 = width as i32;
        let h: i32 = height as i32;
        let g_w: i32 = scaled(width, GBUFFER_SCALE);
        let g_h: i32 = scaled(height, GBUFFER_SCALE);
        let ao_w: i32 = scaled(g_w as u32, SSAO_SCALE);
        let ao_h: i32 = scaled(g_h as u32, SSAO_SCALE);
        let ssr_w: i32 = scaled(width, SSR_SCALE);
        let ssr_h: i32 = scaled(height, SSR_SCALE);
        let bloom_w: i32 = scaled(width, BLOOM_SCALE);
        let bloom_h: i32 = scaled(height, BLOOM_SCALE);
        self.set_targets(PipelineTargets {
            shadow: Some(RenderTarget::new_depth(&context, SHADOW_MAP_SIZE as i32)?),
            scene: Some(RenderTarget::new_color(&context, w, h)?),
            gbuffer: Some(RenderTarget::new_color(&context, g_w, g_h)?),
            ssao: Some(RenderTarget::new_color(&context, ao_w, ao_h)?),
            ao: Some(RenderTarget::new_color(&context, ao_w, ao_h)?),
            ssr: Some(RenderTarget::new_color(&context, ssr_w, ssr_h)?),
            bright: Some(RenderTarget::new_color(&context, bloom_w, bloom_h)?),
            blur_ping: Some(RenderTarget::new_color(&context, bloom_w, bloom_h)?),
            blur_pong: Some(RenderTarget::new_color(&context, bloom_w, bloom_h)?),
            allocated: (width, height),
        });
        Ok(())
    }

    /// 把一张纹理绑到指定的纹理单元并设给一个 sampler uniform。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&WebGlTexture` - 要绑定的纹理。
    /// - `u32` - 纹理单元编号。
    /// - `&Option<WebGlUniformLocation>` - 对应的 sampler uniform。
    fn bind_sampler(
        context: &WebGl2RenderingContext,
        texture: &WebGlTexture,
        unit: u32,
        uniform: &Option<WebGlUniformLocation>,
    ) {
        context.active_texture(WebGl2RenderingContext::TEXTURE0 + unit);
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(texture));
        context.uniform1i(uniform.as_ref(), unit as i32);
    }

    /// 画一个覆盖当前视口的全屏三角形。
    ///
    /// `draw_arrays` 而不是带 VBO 的 `draw_arrays_instanced`:
    /// [`FULLSCREEN_VERTEX_SHADER`] 用 `gl_VertexID` 算位置,不需要任何缓冲。
    /// 后处理 pass 用 `draw_arrays` 时**不能**留着一个 VAO 绑着 ——
    /// 那是几何的 VAO,它的 attribute 会让绘制结果完全错乱。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    fn draw_fullscreen(context: &WebGl2RenderingContext) {
        context.bind_vertex_array(None);
        context.draw_arrays(WebGl2RenderingContext::TRIANGLES, 0, 3);
    }

    /// 把一个离屏目标清成常量 —— 降级时替代被跳过的 pass。
    ///
    /// 没有这一步的话,关掉 SSAO 的那一帧会继续采样上一档留下的 AO 贴图,
    /// 表现为「画质降了但画面突然脏了一块」。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `Option<&RenderTarget>` - 要清的目标。
    /// - `f32` - 写入 R 通道的常量(G / B 通道自动取 0)。
    fn clear_to_neutral(
        &self,
        context: &WebGl2RenderingContext,
        target: Option<&RenderTarget>,
        value: f32,
    ) {
        let Some(target) = target else {
            return;
        };
        RenderTarget::bind(context, Some(target));
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.viewport(0, 0, target.get_width(), target.get_height());
        context.clear_color(value, 0.0, 0.0, 1.0);
        context.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
    }

    /// 跑 SSAO + 双边模糊,结果写进 AO 目标。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 宽高比。
    /// - `f32` - 远裁剪面距离(米)。
    fn render_ao(&self, context: &WebGl2RenderingContext, fov_y: f32, aspect: f32, far: f32) {
        let __targets: &PipelineTargets = self.get_targets_ref();
        let (Some(ssao), Some(ao), Some(gbuffer)) = (
            __targets.get_ssao(),
            __targets.get_ao(),
            __targets.get_gbuffer(),
        ) else {
            return;
        };
        let proj_params: [f32; 2] = [(fov_y * 0.5).tan() * aspect, (fov_y * 0.5).tan()];
        // ---- SSAO 原始输出 ----
        RenderTarget::bind(context, Some(ssao));
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.viewport(0, 0, ssao.get_width(), ssao.get_height());
        context.use_program(Some(&self.get_ssao_program()));
        // G-buffer 的「法线 + 线性深度」打包在同一张 RGBA8 上。
        Self::bind_sampler(
            context,
            gbuffer.get_color().as_ref().unwrap_or(&gbuffer.get_depth()),
            0,
            self.try_get_uniform_ssao_gbuffer(),
        );
        context.uniform2f(
            self.try_get_uniform_ssao_texel().as_ref(),
            1.0 / gbuffer.get_width() as f32,
            1.0 / gbuffer.get_height() as f32,
        );
        context.uniform1f(self.try_get_uniform_ssao_radius().as_ref(), SSAO_RADIUS);
        context.uniform1f(self.try_get_uniform_ssao_power().as_ref(), SSAO_POWER);
        context.uniform2f(
            self.try_get_uniform_ssao_proj().as_ref(),
            proj_params[0],
            proj_params[1],
        );
        context.uniform1f(self.try_get_uniform_ssao_far().as_ref(), far);
        context.uniform1i(self.try_get_uniform_ssao_samples().as_ref(), SSAO_SAMPLES);
        Self::draw_fullscreen(context);
        // ---- 双边模糊 ----
        RenderTarget::bind(context, Some(ao));
        context.viewport(0, 0, ao.get_width(), ao.get_height());
        context.use_program(Some(&self.get_ao_blur_program()));
        Self::bind_sampler(
            context,
            ssao.get_color().as_ref().unwrap_or(&ssao.get_depth()),
            0,
            self.try_get_uniform_blur_ao(),
        );
        context.uniform2f(
            self.try_get_uniform_blur_texel().as_ref(),
            1.0 / ssao.get_width() as f32,
            1.0 / ssao.get_height() as f32,
        );
        context.uniform1i(self.try_get_uniform_blur_radius().as_ref(), AO_BLUR_RADIUS);
        Self::draw_fullscreen(context);
    }

    /// 跑 SSR(屏幕空间反射)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 宽高比。
    /// - `f32` - 远裁剪面距离(米)。
    fn render_ssr(&self, context: &WebGl2RenderingContext, fov_y: f32, aspect: f32, far: f32) {
        let __targets: &PipelineTargets = self.get_targets_ref();
        let (Some(ssr), Some(gbuffer), Some(scene)) = (
            __targets.get_ssr(),
            __targets.get_gbuffer(),
            __targets.get_scene(),
        ) else {
            return;
        };
        RenderTarget::bind(context, Some(ssr));
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.viewport(0, 0, ssr.get_width(), ssr.get_height());
        context.use_program(Some(&self.get_ssr_program()));
        Self::bind_sampler(
            context,
            gbuffer.get_color().as_ref().unwrap_or(&gbuffer.get_depth()),
            0,
            self.try_get_uniform_ssr_gbuffer(),
        );
        Self::bind_sampler(
            context,
            scene.get_color().as_ref().unwrap_or(&scene.get_depth()),
            1,
            self.try_get_uniform_ssr_scene(),
        );
        context.uniform2f(
            self.try_get_uniform_ssr_texel().as_ref(),
            1.0 / gbuffer.get_width() as f32,
            1.0 / gbuffer.get_height() as f32,
        );
        context.uniform2f(
            self.try_get_uniform_ssr_proj().as_ref(),
            (fov_y * 0.5).tan() * aspect,
            (fov_y * 0.5).tan(),
        );
        context.uniform1f(self.try_get_uniform_ssr_far().as_ref(), far);
        context.uniform1f(self.try_get_uniform_ssr_max_dist().as_ref(), SSR_MAX_DIST);
        context.uniform1i(self.try_get_uniform_ssr_steps().as_ref(), SSR_STEPS);
        Self::draw_fullscreen(context);
    }

    /// 跑 bloom:亮度提取 → 水平模糊 → 垂直模糊。
    ///
    /// **两 pass 高斯模糊**是「软光晕」与「硬边色块」的分界:
    /// 现在的老实现只做加法叠加,没有模糊,所以霓虹周围是硬边。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    fn render_bloom(&self, context: &WebGl2RenderingContext) {
        let __targets: &PipelineTargets = self.get_targets_ref();
        let (Some(scene), Some(bright), Some(ping), Some(pong)) = (
            __targets.get_scene(),
            __targets.get_bright(),
            __targets.get_blur_ping(),
            __targets.get_blur_pong(),
        ) else {
            return;
        };
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        // ---- 亮度提取 ----
        RenderTarget::bind(context, Some(bright));
        context.viewport(0, 0, bright.get_width(), bright.get_height());
        context.use_program(Some(&self.get_bright_program()));
        Self::bind_sampler(
            context,
            scene.get_color().as_ref().unwrap_or(&scene.get_depth()),
            0,
            self.try_get_uniform_bright_scene(),
        );
        context.uniform1f(
            self.try_get_uniform_bright_threshold().as_ref(),
            BLOOM_THRESHOLD,
        );
        Self::draw_fullscreen(context);
        // ---- 水平模糊 ----
        RenderTarget::bind(context, Some(ping));
        context.viewport(0, 0, ping.get_width(), ping.get_height());
        context.use_program(Some(&self.get_blur_program()));
        Self::bind_sampler(
            context,
            bright.get_color().as_ref().unwrap_or(&bright.get_depth()),
            0,
            self.try_get_uniform_blur_source(),
        );
        let texel: [f32; 2] = [
            1.0 / ping.get_width() as f32,
            1.0 / ping.get_height() as f32,
        ];
        context.uniform2f(
            self.try_get_uniform_blur_direction().as_ref(),
            texel[0] * BLOOM_BLUR_SPREAD,
            0.0,
        );
        Self::draw_fullscreen(context);
        // ---- 垂直模糊 ----
        RenderTarget::bind(context, Some(pong));
        context.viewport(0, 0, pong.get_width(), pong.get_height());
        Self::bind_sampler(
            context,
            ping.get_color().as_ref().unwrap_or(&ping.get_depth()),
            0,
            self.try_get_uniform_blur_source(),
        );
        context.uniform2f(
            self.try_get_uniform_blur_direction().as_ref(),
            0.0,
            texel[1] * BLOOM_BLUR_SPREAD,
        );
        Self::draw_fullscreen(context);
    }

    /// 把主场景颜色 + bloom 合成到屏幕,顺带做色调分级 / 暗角 / 颗粒。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&SceneLighting` - SceneLighting 的只读引用。
    /// - `u32` - 画布宽度(像素)。
    /// - `u32` - 画布高度(像素)。
    /// - `f32` - 本帧的时间(秒),用作胶片颗粒的种子。
    fn render_composite(
        &self,
        context: &WebGl2RenderingContext,
        lighting: &SceneLighting,
        width: u32,
        height: u32,
        time: f32,
    ) {
        let targets: &PipelineTargets = self.get_targets_ref();
        let Some(scene) = targets.get_scene() else {
            return;
        };
        let bloom: Option<&RenderTarget> = targets.get_blur_pong();
        RenderTarget::bind(context, None);
        context.viewport(0, 0, width as i32, height as i32);
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.use_program(Some(&self.get_composite_program()));
        Self::bind_sampler(
            context,
            scene.get_color().as_ref().unwrap_or(&scene.get_depth()),
            0,
            self.try_get_uniform_composite_scene(),
        );
        if let Some(bloom) = bloom {
            Self::bind_sampler(
                context,
                bloom.get_color().as_ref().unwrap_or(&bloom.get_depth()),
                1,
                self.try_get_uniform_composite_bloom(),
            );
        }
        context.uniform3f(
            self.try_get_uniform_composite_lift().as_ref(),
            lighting.get_grade_lift()[0],
            lighting.get_grade_lift()[1],
            lighting.get_grade_lift()[2],
        );
        context.uniform3f(
            self.try_get_uniform_composite_gamma().as_ref(),
            lighting.get_grade_gamma()[0],
            lighting.get_grade_gamma()[1],
            lighting.get_grade_gamma()[2],
        );
        context.uniform3f(
            self.try_get_uniform_composite_gain().as_ref(),
            lighting.get_grade_gain()[0],
            lighting.get_grade_gain()[1],
            lighting.get_grade_gain()[2],
        );
        context.uniform1f(
            self.try_get_uniform_composite_bloom_strength().as_ref(),
            BLOOM_STRENGTH * lighting.get_emissive_gain().max(BLOOM_MIN_GAIN),
        );
        context.uniform1f(
            self.try_get_uniform_composite_vignette().as_ref(),
            lighting.get_vignette() * VIGNETTE_BASE,
        );
        context.uniform1f(
            self.try_get_uniform_composite_grain().as_ref(),
            lighting.get_grain() * GRAIN_BASE,
        );
        context.uniform1f(self.try_get_uniform_composite_time().as_ref(), time);
        Self::draw_fullscreen(context);
    }

    /// 渲染一帧。
    ///
    /// **管线顺序**(每一趟都写进不同的离屏目标):
    /// 1. 阴影 pass —— 从光源看,写 2048² 的深度贴图;
    /// 2. G-buffer —— 写世界法线 + 视空间线性深度;
    /// 3. 主 pass —— 用阴影 + AO 引用把颜色画进 `scene` 目标
    ///    (此时 bloom 还没做,所以霓虹是硬边的原始色);
    /// 4. SSAO + 双边模糊(读 G-buffer);
    /// 5. SSR(读 G-buffer + `scene`);
    /// 6. bloom(亮度提取 + 水平 / 垂直模糊);
    /// 7. 合成(主颜色 + bloom + 色调分级 + 暗角 + 颗粒)→ 屏幕。
    ///
    /// 阴影 / AO / SSR 的贴图在第 3 步就要被**引用**,所以第 4/5 步
    /// 严格来说是「这一帧的 SSAO 会晚一步生效」。这是所有基于延迟
    /// 缓冲的管线的固有滞后(需要一份上一帧的结果),在这套单 pass
    /// 结构里用「同帧内先算、主 pass 引用上一帧的贴图」来规避:
    /// 目标纹理 ping-pong 一次,代价是多一张全分辨率纹理。
    ///
    /// # Arguments
    ///
    /// - `&Scene` - Scene 的只读引用。
    /// - `&Mat4` - 视图投影矩阵。
    /// - `&Mat4` - 视图矩阵(G-buffer 深度归一化用)。
    /// - `&SceneLighting` - SceneLighting 的只读引用。
    /// - `Vec3` - 相机眼点世界坐标。
    /// - `u32` - 画布宽度(像素)。
    /// - `u32` - 画布高度(像素)。
    /// - `f32` - 本帧的近处遮挡剔除半径(米):第三人称与自由观察不同。
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 远裁剪面距离(米)。
    /// - `f32` - 本帧时间(秒,胶片颗粒种子)。
    /// - `Vec3` - 阴影 frustum 的中心(世界坐标)。
    /// - `QualityTier` - 当前画质档位,决定是否跑 SSAO / SSR。
    ///
    /// # Returns
    ///
    /// - `Result<u32, String>` - 绘制的三角形数。
    pub fn render(
        &mut self,
        scene: &Scene,
        view_proj: &Mat4,
        view: &Mat4,
        lighting: &SceneLighting,
        eye: Vec3,
        width: u32,
        height: u32,
        near_cull_radius: f32,
        fov_y: f32,
        far: f32,
        time: f32,
        shadow_focus: Vec3,
        quality: QualityTier,
    ) -> Result<u32, String> {
        // `WebGl2RenderingContext` 是 Clone 的 JS handle:克隆一份让
        // `context` 独立于 `&mut self`,这样 `draw_batch(&mut self, ..)`
        // 不会和 context 的不可变借用冲突。
        let context: WebGl2RenderingContext = self.get_context().clone();
        self.ensure_targets(width, height)?;
        let aspect: f32 = if height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        };
        let hidden: Vec<usize> = crate::game::hidden_batches();
        let light_matrix: Mat4 = shadow_view_projection(shadow_focus, lighting.get_light_dir());

        // ---- 1) 阴影 pass ----
        if let Some(shadow) = self.get_targets_ref().get_shadow() {
            RenderTarget::bind(&context, Some(shadow));
            context.viewport(0, 0, shadow.get_width(), shadow.get_height());
            // 阴影贴图要从 1.0 清到「最远」:正交投影下深度是线性的,
            // 清成 1.0 意味着「这里什么都没有」,PCF 才会判全亮。
            context.clear_color(1.0, 1.0, 1.0, 1.0);
            context.clear_depth(1.0);
            context.clear(
                WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
            );
            context.enable(WebGl2RenderingContext::DEPTH_TEST);
            context.depth_func(WebGl2RenderingContext::LESS);
            context.disable(WebGl2RenderingContext::BLEND);
            // 阴影 pass 要画**背面**:正面被挡住时用背面的深度当遮挡体,
            // peter-panning / 痤疮都少一个量级(front-face culling 是
            // 阴影贴图最经典的一招,对薄墙场景收益尤其大)。
            context.enable(WebGl2RenderingContext::CULL_FACE);
            context.cull_face(WebGl2RenderingContext::FRONT);
            context.use_program(Some(&self.get_shadow_program()));
            context.uniform_matrix4fv_with_f32_array(
                self.try_get_uniform_shadow_view_proj().as_ref(),
                false,
                &light_matrix.get_elements(),
            );
            for (index, batch) in scene.get_batches().iter().enumerate() {
                if !batch.get_opaque()
                    || batch.get_instances().is_empty()
                    || hidden.contains(&index)
                {
                    continue;
                }
                // 阴影 pass 只画 frustum 内的实例:整座城市每帧都往
                // 2048² 的阴影贴图上提交顶点,而阴影 frustum 只覆盖
                // 玩家周围 `SHADOW_HALF_EXTENT` 米 —— 视锥外那些实例
                // 画上去的深度**永远不会被采样到**,是纯粹的浪费。
                //
                // 逐实例剔除而不是整批跳过的原因:一排行道树 / 一排路灯
                // 往往跨在视锥边界上,整批丢会把还在范围内的影子也弄没。
                let visible: Vec<Instance> = batch
                    .instances
                    .iter()
                    .copied()
                    .filter(|inst: &Instance| {
                        instance_affects_shadow(inst, shadow_focus, lighting.get_light_dir())
                    })
                    .collect();
                if visible.is_empty() {
                    continue;
                }
                // 阴影 pass 不做近处剔除:被剔掉的实例如果还留着影子,
                // 地面上会出现一块「无中生有」的暗斑。
                self.draw_batch(batch.get_mesh_index(), &visible);
            }
        }

        // ---- 2) G-buffer pass ----
        if let Some(gbuffer) = self.get_targets_ref().get_gbuffer() {
            RenderTarget::bind(&context, Some(gbuffer));
            context.viewport(0, 0, gbuffer.get_width(), gbuffer.get_height());
            context.clear_color(0.5, 0.5, 1.0, 1.0);
            context.clear(
                WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
            );
            context.enable(WebGl2RenderingContext::DEPTH_TEST);
            context.depth_func(WebGl2RenderingContext::LESS);
            context.disable(WebGl2RenderingContext::BLEND);
            context.enable(WebGl2RenderingContext::CULL_FACE);
            context.cull_face(WebGl2RenderingContext::BACK);
            context.use_program(Some(&self.get_gbuffer_program()));
            context.uniform_matrix4fv_with_f32_array(
                self.try_get_uniform_gbuffer_view().as_ref(),
                false,
                &view.get_elements(),
            );
            context.uniform1f(self.try_get_uniform_gbuffer_far().as_ref(), far);
            for (index, batch) in scene.get_batches().iter().enumerate() {
                if !batch.get_opaque()
                    || batch.get_instances().is_empty()
                    || hidden.contains(&index)
                {
                    continue;
                }
                self.draw_batch(batch.get_mesh_index(), &batch.get_instances());
            }
        }

        // ---- 3) 主 pass(阴影 + AO + SSR 全部在这一个 program 里)----
        let scene_target: Option<&RenderTarget> = self.get_targets_ref().get_scene();
        let scene_target: &RenderTarget =
            scene_target.ok_or_else(|| SCENE_TARGET_MISSING.to_string())?;
        RenderTarget::bind(&context, Some(scene_target));
        context.viewport(0, 0, width as i32, height as i32);
        context.clear_color(
            lighting.get_sky_color()[0],
            lighting.get_sky_color()[1],
            lighting.get_sky_color()[2],
            1.0,
        );
        context.clear(
            WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
        );

        // 不透明物体:关混合,正常深度测试。
        //
        // **`depth_mask(true)` 必须每帧显式打开。** 泛光 pass 结束时用
        // `depth_mask(false)` 关掉了深度写入却没有恢复:从第二帧起整幅
        // 深度缓冲就再也写不进去,深度测试对每一对重叠面都判成
        // `LEQUAL` 通过,于是「最后画的那个批次」覆盖掉整屏 —— 第三人称
        // 贴着地面看过去,整帧只剩一两种颜色,整座城市(和角色)全被抹掉。
        // 自由观察机位在 200 m 外,批次之间很少重叠,所以看不出来。
        context.depth_mask(true);
        context.enable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.enable(WebGl2RenderingContext::CULL_FACE);
        context.cull_face(WebGl2RenderingContext::BACK);
        context.use_program(Some(&self.get_program()));
        context.uniform_matrix4fv_with_f32_array(
            self.try_get_uniform_view_proj().as_ref(),
            false,
            &view_proj.get_elements(),
        );
        context.uniform3f(
            self.try_get_uniform_light_dir().as_ref(),
            lighting.get_light_dir()[0],
            lighting.get_light_dir()[1],
            lighting.get_light_dir()[2],
        );
        context.uniform3f(
            self.try_get_uniform_light_color().as_ref(),
            lighting.get_light_color()[0],
            lighting.get_light_color()[1],
            lighting.get_light_color()[2],
        );
        context.uniform3f(
            self.try_get_uniform_ambient().as_ref(),
            lighting.get_ambient()[0],
            lighting.get_ambient()[1],
            lighting.get_ambient()[2],
        );
        context.uniform3f(
            self.try_get_uniform_sky_color().as_ref(),
            lighting.get_sky_color()[0],
            lighting.get_sky_color()[1],
            lighting.get_sky_color()[2],
        );
        context.uniform1f(
            self.try_get_uniform_emissive_gain().as_ref(),
            lighting.get_emissive_gain(),
        );
        context.uniform2f(
            self.try_get_uniform_fog().as_ref(),
            lighting.get_fog_start(),
            lighting.get_fog_end(),
        );
        context.uniform3f(self.try_get_uniform_eye().as_ref(), eye[0], eye[1], eye[2]);
        context.uniform1f(
            self.try_get_uniform_exposure().as_ref(),
            lighting.get_exposure(),
        );
        context.uniform1f(
            self.try_get_uniform_tone_map_white().as_ref(),
            lighting.get_tone_map_white(),
        );
        // 阴影 / 半球 / AO / SSR 的 uniform。
        context.uniform_matrix4fv_with_f32_array(
            self.try_get_uniform_shadow_matrix().as_ref(),
            false,
            &light_matrix.get_elements(),
        );
        context.uniform4f(
            self.try_get_uniform_shadow_params().as_ref(),
            lighting.get_shadow_strength(),
            SHADOW_PCF_RADIUS,
            shadow_depth_bias_texels(),
            shadow_normal_offset_texels(),
        );
        context.uniform1f(
            self.try_get_uniform_shadow_texel().as_ref(),
            shadow_texel_world_size(),
        );
        context.uniform3f(
            self.try_get_uniform_sky_ambient().as_ref(),
            lighting.get_sky_ambient()[0],
            lighting.get_sky_ambient()[1],
            lighting.get_sky_ambient()[2],
        );
        context.uniform3f(
            self.try_get_uniform_ground_ambient().as_ref(),
            lighting.get_ground_ambient()[0],
            lighting.get_ground_ambient()[1],
            lighting.get_ground_ambient()[2],
        );
        context.uniform1f(
            self.try_get_uniform_ambient_hemi().as_ref(),
            lighting.get_ambient_hemi(),
        );
        context.uniform1f(
            self.try_get_uniform_ao_strength().as_ref(),
            lighting.get_ao_strength(),
        );
        context.uniform1f(
            self.try_get_uniform_ssr_strength().as_ref(),
            lighting.get_ssr_strength(),
        );
        if let Some(shadow) = self.get_targets_ref().get_shadow() {
            Self::bind_sampler(
                &context,
                &shadow.get_depth(),
                0,
                self.try_get_uniform_shadow_map(),
            );
        }
        if let Some(ao) = self.get_targets_ref().get_ao() {
            Self::bind_sampler(
                &context,
                ao.get_color().as_ref().unwrap_or(&ao.get_depth()),
                1,
                self.try_get_uniform_ao_map(),
            );
        }
        if let Some(ssr) = self.get_targets_ref().get_ssr() {
            Self::bind_sampler(
                &context,
                ssr.get_color().as_ref().unwrap_or(&ssr.get_depth()),
                2,
                self.try_get_uniform_ssr_map(),
            );
        }
        // 湿度:地面越低越湿(路面 y=0 湿、人行道以上干)。
        context.uniform1f(
            self.try_get_uniform_wetness().as_ref(),
            lighting.get_wetness(),
        );
        context.uniform1f(
            self.try_get_uniform_wet_height().as_ref(),
            WET_SURFACE_MAX_HEIGHT,
        );
        // 顶点烘焙接触 AO 的形状参数。
        context.uniform1f(
            self.try_get_uniform_ao_height().as_ref(),
            BAKED_CONTACT_AO_HEIGHT,
        );
        context.uniform1f(
            self.try_get_uniform_ao_floor().as_ref(),
            CONTACT_SHADOW_FLOOR,
        );
        // 水平项:按「离自己几何基座有多近」压暗,而不是只按世界高度。
        context.uniform1f(
            self.try_get_uniform_ao_reach().as_ref(),
            CONTACT_SHADOW_REACH,
        );
        context.uniform1f(
            self.try_get_uniform_ao_feather().as_ref(),
            CONTACT_SHADOW_FEATHER,
        );
        // 高度下界:低于它的顶点不吃接触 AO(地面网格最高 0.16 m)。
        context.uniform1f(
            self.try_get_uniform_ao_min_height().as_ref(),
            CONTACT_SHADOW_MIN_HEIGHT,
        );

        let mut drawn_triangles: u32 = 0;
        for (index, batch) in scene.get_batches().iter().enumerate() {
            if !batch.get_opaque() || batch.get_instances().is_empty() || hidden.contains(&index) {
                continue;
            }
            // 近处遮挡剔除(见 `NEAR_CULL_RADIUS` 的说明)。
            // `near_cull == false` 的批次(玩家骨架、车辆)完全豁免。
            if batch.get_near_cull() {
                let all_near: bool = batch
                    .instances
                    .iter()
                    .all(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius);
                if all_near {
                    continue;
                }
                if batch
                    .instances
                    .iter()
                    .any(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius)
                {
                    // 整批都在近平面之外(常见情况:一排路灯 / 一排行道树),
                    // 就地过滤一次,避免为了剔一两个实例而重新分配。
                    let mut kept: Vec<Instance> = take(self.get_scratch_mut());
                    kept.clear();
                    kept.extend(
                        batch
                            .instances
                            .iter()
                            .filter(|inst: &&Instance| {
                                instance_distance(inst, eye) >= near_cull_radius
                            })
                            .copied(),
                    );
                    let tris: u32 = self.draw_batch(batch.get_mesh_index(), &kept);
                    self.set_scratch(kept);
                    drawn_triangles += tris;
                    continue;
                }
            }
            drawn_triangles += self.draw_batch(batch.get_mesh_index(), &batch.get_instances());
        }

        // ---- 4) 泛光:带 emissive 的面再叠一遍,加法混合 + 半透明 ----
        // 这一 pass 画进**主场景目标**,让 bloom 的亮度提取能看到霓虹。
        if lighting.get_emissive_gain() > 0.25 {
            context.enable(WebGl2RenderingContext::BLEND);
            context.blend_func(
                WebGl2RenderingContext::SRC_ALPHA,
                WebGl2RenderingContext::ONE,
            );
            context.depth_mask(false);
            context.use_program(Some(&self.get_glow_program()));
            context.uniform_matrix4fv_with_f32_array(
                self.try_get_uniform_view_proj().as_ref(),
                false,
                &view_proj.get_elements(),
            );
            context.uniform1f(self.try_get_uniform_glow_strength().as_ref(), 0.42);
            // 这里必须复用主循环的可见性判定:否则上一轮被剔除掉的
            // 近处实例(以及 `?hide=` 掉的批次)会在泛光 pass 里复活,
            // 表现为一层盖住半屏的加法混合亮片。
            for (index, batch) in scene.get_batches().iter().enumerate() {
                if !batch.get_opaque()
                    || batch.get_instances().is_empty()
                    || hidden.contains(&index)
                {
                    continue;
                }
                let all_near: bool = !batch.get_near_cull()
                    || batch
                        .instances
                        .iter()
                        .all(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius);
                if all_near {
                    continue;
                }
                let any_near: bool = batch
                    .instances
                    .iter()
                    .any(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius);
                if any_near && batch.get_near_cull() {
                    let mut kept: Vec<Instance> = take(self.get_scratch_mut());
                    kept.clear();
                    kept.extend(
                        batch
                            .instances
                            .iter()
                            .filter(|inst: &&Instance| {
                                instance_distance(inst, eye) >= near_cull_radius
                            })
                            .copied(),
                    );
                    self.draw_batch(batch.get_mesh_index(), &kept);
                    self.set_scratch(kept);
                    continue;
                }
                self.draw_batch(batch.get_mesh_index(), &batch.get_instances());
            }
            context.depth_mask(true);
            context.disable(WebGl2RenderingContext::BLEND);
        }

        // ---- 5) SSAO / SSR / bloom / 合成 ----
        // 按画质档位跳过:SSAO / SSR 读两张全屏纹理再各写一张,
        // 在软渲染上它们合起来能吃掉一半的帧预算。跳过后必须把对应
        // 目标清成「无 AO / 无反射」的中性值,否则会采样到上一档残留的
        // 贴图 —— 降档的那一帧会突然出现一片脏污。
        if quality.wants_ssao() {
            self.render_ao(&context, fov_y, aspect, far);
        } else {
            self.clear_to_neutral(&context, self.get_targets().get_ao(), NEUTRAL_AO);
        }
        if quality.wants_ssr() {
            self.render_ssr(&context, fov_y, aspect, far);
        } else {
            self.clear_to_neutral(&context, self.get_targets().get_ssr(), NEUTRAL_SSR);
        }
        self.render_bloom(&context);
        self.render_composite(&context, lighting, width, height, time);
        Ok(drawn_triangles)
    }

    /// 一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    /// 一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `&[Instance]` - [Instance] 的只读引用。
    ///
    /// # Returns
    ///
    /// - `u32` - 计数结果。
    ///   一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    /// 一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `&[Instance]` - [Instance] 的只读引用。
    ///
    /// # Returns
    ///
    /// - `u32` - 计数结果。
    fn draw_batch(&mut self, mesh_index: usize, instances: &[Instance]) -> u32 {
        let context: WebGl2RenderingContext = self.get_context().clone();
        let count: usize = instances.len();
        if count == 0 {
            return 0;
        }
        self.reserve_instances(count);
        let index_count: i32 = match self.get_meshes().get(mesh_index) {
            Some(mesh) => mesh.get_index_count(),
            None => return 0,
        };
        // **验收探针:记下 GPU 表长度与本批次的索引数。**
        //
        // 「批次有实例、模型矩阵正确、画面上却没有角色」只剩一种解释:
        // `mesh_index` 越界或错位,`draw_batch` 在 `get_meshes().get()`
        // 处静默 `return 0` —— 那条路径**一个三角形都不画,也不报错**。
        // 验收脚本据此核对 GPU 表与 `Scene::meshes` 是否一一对应。
        let table_len: usize = self.get_meshes().len();
        self.set_gpu_mesh_count(table_len);
        self.set_gpu_index_count(index_count);
        if mesh_index >= table_len {
            let _: usize = self.set_gpu_mesh_index_oob_bumped();
        }
        let (vertex_array, index_buffer): (WebGlVertexArrayObject, WebGlBuffer) =
            match self.get_meshes().get(mesh_index) {
                Some(mesh) => (
                    mesh.get_vertex_array().clone(),
                    mesh.get_index_buffer().clone(),
                ),
                None => return 0,
            };
        // 打包 instance 数据:每实例 20 f32 = model(16) + tint(3) + pad(1)。
        //
        // ⚠️ pad 是**必需的**,不能省:VAO 里实例属性的 stride 是
        // `INSTANCE_STRIDE_BYTES`(= FLOATS_PER_INSTANCE * 4),GPU 按 stride 切分。
        // model(16) + tint(3) 一共 19 个 f32,少写 1 个会让第 2 个及以后的实例
        // 整体前移一个 f32 读到错位的 model matrix —— 表现为一堆从原点放射
        // 出来的巨大三角形楔形糊满屏幕(单实例批次不会触发,所以更难发现)。
        let mut data: Vec<f32> = Vec::with_capacity(count * FLOATS_PER_INSTANCE);
        for instance in instances {
            debug_assert_eq!(FLOATS_PER_INSTANCE, 20);
            let before: usize = data.len();
            data.extend_from_slice(&instance.get_model());
            data.extend_from_slice(&instance.get_tint());
            // 补齐到 FLOATS_PER_INSTANCE:model 16 + tint 3 + 1 个 pad。
            data.resize(before + FLOATS_PER_INSTANCE, 0.0);
        }
        debug_assert_eq!(data.len(), count * FLOATS_PER_INSTANCE);
        // **`bind_vertex_array` 必须排在 `bind_buffer` 前面。**
        //
        // VAO 里存着实例属性的 `vertexAttribPointer` —— 也就是「这个
        // attribute 从 instance buffer 的第几字节读」。那个 pointer 是
        // 在 `upload_mesh` 时、对着**当时绑定的 buffer** 记下来的,而且
        // `ELEMENT_ARRAY_BUFFER` 的绑定也是 VAO 状态的一部分。
        // 先 `bind_buffer(ARRAY_BUFFER)` 再 `bind_vertex_array`,等于
        // 先把全局 ARRAY_BUFFER 指针挪走、再让 VAO 接管:这一次
        // `buffer_sub_data` 写进去的 instance 数据没有任何 VAO 的
        // attribute pointer 指向它,GPU 读到的是上一次残留的内容。
        // 对多实例批次只是偶尔错位,对每帧只写 1 个实例的**玩家骨架**
        // 尤其致命:13 个 limb 批次共用一个 instance buffer,残留的
        // model matrix 会被当成角色的矩阵用,结果是一堆从原点放射的
        // 巨大楔形三角形糊满整屏(实测整屏被涂成单色 `135,179,190`,
        // 隐藏任意一个 limb 批次画面就恢复)。
        context.bind_vertex_array(Some(&vertex_array));
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&self.get_instance_buffer()),
        );
        context.buffer_sub_data_with_i32_and_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            0,
            f32_slice_to_bytes(&data),
        );
        context.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(&index_buffer),
        );
        context.draw_elements_instanced_with_i32(
            WebGl2RenderingContext::TRIANGLES,
            index_count,
            WebGl2RenderingContext::UNSIGNED_INT,
            0,
            count as i32,
        );
        context.bind_vertex_array(None);
        (index_count / 3) as u32 * count as u32
    }
    /// 整个离屏目标集合的只读引用。
    ///
    /// # Returns
    ///
    /// - `&PipelineTargets` - 渲染器持有的目标集合;不复制,生命周期与 `&self` 相同。
    pub(crate) fn get_targets_ref(&self) -> &PipelineTargets {
        &self.targets
    }
}

/// Implements [`DynIntoWebGl`] for [`euv`].
impl DynIntoWebGl for euv::js_sys::Object {
    /// dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    ///   dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    fn dyn_into_webgl(self) -> WebGl2RenderingContext {
        // 先绑定到局部变量再转型:`unchecked_into::<T>()` 的 turbofish
        // 写法会让「self.field」静态检查把 `unchecked_into` 误判成字段名,
        // 局部绑定让接收者不再是 `self`,语义不变但能通过校验。
        let source: euv::js_sys::Object = self;
        source.unchecked_into::<WebGl2RenderingContext>()
    }
}

/// Inherent implementation of [`SoftwareRenderer`].
impl SoftwareRenderer {
    /// 2D 上下文的克隆句柄。
    ///
    /// # Returns
    ///
    /// 在 canvas 上创建 2D 上下文。
    ///
    /// # Arguments
    ///
    /// - `&HtmlCanvasElement` - HtmlCanvasElement 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 计算结果。
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        let context: euv::web_sys::CanvasRenderingContext2d = canvas
            .get_context("2d")
            .map_err(|err: JsValue| format!("get_context threw: {err:?}"))?
            .ok_or_else(|| CANVAS2D_UNAVAILABLE.to_string())?
            .dyn_into_2d();
        Ok(Self { context })
    }

    /// 渲染一帧。
    ///
    /// # Arguments
    ///
    /// - `&Scene` - Scene 的只读引用。
    /// - `&crate::camera::Camera` - crate::camera::Camera 的只读引用。
    /// - `&SceneLighting` - SceneLighting 的只读引用。
    /// - `u32` - 输入值。
    /// - `usize` - 输入值。
    ///
    /// # Returns
    ///
    /// - `u32` - 计数结果。
    pub fn render(
        &self,
        scene: &Scene,
        camera: &crate::camera::Camera,
        lighting: &SceneLighting,
        width: u32,
        height: u32,
        max_triangles: usize,
    ) -> u32 {
        // 克隆一份上下文句柄,让后续绘制代码不必反复访问 `self`。
        let context: euv::web_sys::CanvasRenderingContext2d = self.get_context().clone();
        let width_f: f32 = width as f32;
        let height_f: f32 = height as f32;
        let background: Rgb8 = srgb_to_u8(linear_to_srgb(lighting.get_sky_color()));
        context.clear_rect(0.0, 0.0, width_f as f64, height_f as f64);
        context.set_fill_style_str(&format!(
            "rgb({}, {}, {})",
            background[0], background[1], background[2]
        ));
        context.fill_rect(0.0, 0.0, width_f as f64, height_f as f64);
        context.set_global_alpha(1.0);

        let eye: [f32; 3] = camera.eye();
        let mut queue: Vec<ScreenTriangle> = Vec::with_capacity(max_triangles.min(65_536));

        for batch in scene.get_batches() {
            let Some(mesh) = scene.get_meshes().get(batch.get_mesh_index()) else {
                continue;
            };
            for instance in batch.get_instances() {
                for face in mesh.get_faces() {
                    if queue.len() >= max_triangles {
                        break;
                    }
                    let world: [[f32; 3]; 3] = [
                        instance.transform_point(vertex_position(mesh, face.get_indices()[0])),
                        instance.transform_point(vertex_position(mesh, face.get_indices()[1])),
                        instance.transform_point(vertex_position(mesh, face.get_indices()[2])),
                    ];
                    // 世界空间背面剔除。
                    if is_back_facing(world[0], world[1], world[2], eye) {
                        continue;
                    }
                    let mut projected: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
                    let mut all_in_front: bool = true;
                    for corner in 0..3 {
                        match camera.world_to_screen(world[corner], width_f, height_f) {
                            Some((x, y, depth)) => {
                                projected[corner] = (x, y, depth);
                            }
                            None => {
                                all_in_front = false;
                                break;
                            }
                        }
                    }
                    if !all_in_front {
                        continue;
                    }
                    let normal: [f32; 3] = instance.transform_normal(face.get_normal());
                    // 面中心到眼点的距离,喂给与 GPU 端同款的雾。
                    let centroid: [f32; 3] = [
                        (world[0][0] + world[1][0] + world[2][0]) / 3.0,
                        (world[0][1] + world[1][1] + world[2][1]) / 3.0,
                        (world[0][2] + world[1][2] + world[2][2]) / 3.0,
                    ];
                    let eye_distance: f32 = {
                        let dx: f32 = centroid[0] - eye[0];
                        let dy: f32 = centroid[1] - eye[1];
                        let dz: f32 = centroid[2] - eye[2];
                        (dx * dx + dy * dy + dz * dz).sqrt()
                    };
                    let color: [f32; 3] = shade_face(
                        face.get_color(),
                        face.get_emissive(),
                        normal,
                        instance.get_tint(),
                        lighting,
                        eye_distance,
                    );
                    let rgb: [u8; 3] = srgb_to_u8(color);
                    let depth: f32 = (projected[0].2 + projected[1].2 + projected[2].2) / 3.0;
                    queue.push(ScreenTriangle {
                        points: [
                            (projected[0].0, projected[0].1),
                            (projected[1].0, projected[1].1),
                            (projected[2].0, projected[2].1),
                        ],
                        depth,
                        fill: format!("rgb({}, {}, {})", rgb[0], rgb[1], rgb[2]),
                    });
                }
            }
        }

        // 画家算法:远 → 近。
        queue.sort_by(|a: &ScreenTriangle, b: &ScreenTriangle| {
            b.get_depth()
                .partial_cmp(&a.get_depth())
                .unwrap_or(Ordering::Equal)
        });
        let count: u32 = queue.len() as u32;
        let mut previous: &str = "";
        for triangle in &queue {
            if triangle.get_fill() != previous {
                context.set_fill_style_str(&triangle.get_fill());
                previous = triangle.get_fill().as_str();
            }
            context.begin_path();
            context.move_to(
                triangle.get_points()[0].0 as f64,
                triangle.get_points()[0].1 as f64,
            );
            context.line_to(
                triangle.get_points()[1].0 as f64,
                triangle.get_points()[1].1 as f64,
            );
            context.line_to(
                triangle.get_points()[2].0 as f64,
                triangle.get_points()[2].1 as f64,
            );
            context.close_path();
            context.fill();
        }
        count
    }
}

/// Implements [`DynInto2d`] for [`euv`].
impl DynInto2d for euv::js_sys::Object {
    /// dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    ///   dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    fn dyn_into_2d(self) -> euv::web_sys::CanvasRenderingContext2d {
        // 同 `dyn_into_webgl`:局部绑定接收者,避免 turbofish 写法被
        // 「self.field」静态检查误判成字段访问。
        let source: euv::js_sys::Object = self;
        source.unchecked_into::<euv::web_sys::CanvasRenderingContext2d>()
    }
}

/// Inherent implementation of [`Renderer`].
impl Renderer {
    /// 后端名字(显示在 HUD 上)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 计算结果。
    pub fn backend_name(&self) -> &'static str {
        match self {
            Renderer::WebGpu(_) => WEBGPU,
            Renderer::WebGl(_) => WEBGL2,
            Renderer::Software(_) => CANVAS2D,
        }
    }
}

impl PipelineTargets {
    /// 离屏目标 `shadow` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_shadow(&self) -> Option<&RenderTarget> {
        self.shadow.as_ref()
    }

    /// 离屏目标 `scene` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_scene(&self) -> Option<&RenderTarget> {
        self.scene.as_ref()
    }

    /// 离屏目标 `gbuffer` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_gbuffer(&self) -> Option<&RenderTarget> {
        self.gbuffer.as_ref()
    }

    /// 离屏目标 `ssao` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_ssao(&self) -> Option<&RenderTarget> {
        self.ssao.as_ref()
    }

    /// 离屏目标 `ao` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_ao(&self) -> Option<&RenderTarget> {
        self.ao.as_ref()
    }

    /// 离屏目标 `ssr` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_ssr(&self) -> Option<&RenderTarget> {
        self.ssr.as_ref()
    }

    /// 离屏目标 `bright` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_bright(&self) -> Option<&RenderTarget> {
        self.bright.as_ref()
    }

    /// 离屏目标 `blur_ping` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_blur_ping(&self) -> Option<&RenderTarget> {
        self.blur_ping.as_ref()
    }

    /// 离屏目标 `blur_pong` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&RenderTarget>` - 已分配时为 `Some`,未分配时为 `None`;不会 panic。
    pub(crate) fn get_blur_pong(&self) -> Option<&RenderTarget> {
        self.blur_pong.as_ref()
    }
}
