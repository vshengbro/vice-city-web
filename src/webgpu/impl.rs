//! WebGPU 渲染后端:每帧一个 render pass,把整座城市 instanced 画进游戏那块画布。
//!
//! 与 [`crate::render::WebGlRenderer`] 的关系是**平级后端**,不是替换:
//! `game.rs` 里三条路径(WebGPU → WebGL2 → Canvas2D)按可用性依次回退,
//! 现有 WebGL2 代码一行都没删。
//!
//! **本切片的范围**:地面 / 建筑 / 玩家角色(以及其余所有
//! [`crate::render::SceneBatch`])全部走同一条 instanced pass,带深度
//! 缓冲 + 背面剔除。阴影贴图、G-buffer、SSAO、SSR、bloom、湿地面这六条
//! 增强管线**尚未移植**。

use super::*;

/// 顶点缓冲步长(字节):12 f32 × 4 = 48 B。
///
/// WebGPU 只要求步长是 4 的倍数(Vulkan 才要求 16),所以这里能直接
/// 沿用 WebGL2 端那份**逐字节相同**的顶点数据,不需要重排。
const VERTEX_STRIDE_BYTES: u64 = (STRIDE_FLOATS * 4) as u64;

/// instance buffer 的预分配实例数(与 WebGL2 端一致)。
const INSTANCE_PREALLOC: usize = 64;

/// WebGPU 渲染后端。
///
/// 构造是**异步**的(见 [`acquire`]),但 [`WebGpuRenderer::render`]
/// 是同步的 —— 拿到 device 之后,每帧不再需要任何 await。
impl WebGpuRenderer {
    /// 异步拿到设备并建好管线。
    ///
    /// # Arguments
    ///
    /// - `&HtmlCanvasElement` - 游戏正在用的那块画布。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 建好的后端或可读的失败原因。
    pub async fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        let acquired: GpuContext = acquire(canvas).await.map_err(|reason: GpuUnavailable| {
            format!("[vcw] WebGPU unavailable: {}", reason.reason)
        })?;
        let mut renderer: WebGpuRenderer = WebGpuRenderer {
            context: acquired.context,
            device: acquired.device,
            format: acquired.format,
            pipeline: JsValue::NULL,
            group_layout: JsValue::NULL,
            pipeline_layout: JsValue::NULL,
            depth_texture: JsValue::NULL,
            depth_size: (0, 0),
            frame_buffer: JsValue::NULL,
            shading_buffer: JsValue::NULL,
            bind_group: JsValue::NULL,
            shadow_pipeline: JsValue::NULL,
            shadow_pipeline_layout: JsValue::NULL,
            shadow_group_layout: JsValue::NULL,
            shadow_bind_group: JsValue::NULL,
            shadow_frame_buffer: JsValue::NULL,
            shadow_texture: JsValue::NULL,
            shadow_instance_buffer: JsValue::NULL,
            shadow_instance_capacity: 0,
            shadow_sample_bind_group: JsValue::NULL,
            shadow_sample_group_layout: JsValue::NULL,
            meshes: Vec::new(),
            instance_buffer: JsValue::NULL,
            instance_capacity: 0,
            gpu_mesh_count: 0,
            bloom_scene: JsValue::NULL,
            bloom_bright: JsValue::NULL,
            bloom_ping: JsValue::NULL,
            bloom_pong: JsValue::NULL,
            bloom_size: (0, 0),
            bloom_buffers: empty_bloom_buffers(),
            bloom_sampler: JsValue::NULL,
            bloom_group_layout: JsValue::NULL,
            bloom_composite_group_layout: JsValue::NULL,
            bloom_passes: empty_bloom_passes(),
        };
        renderer.create_pipeline()?;
        renderer.create_shadow_pipeline()?;
        renderer.create_uniform_buffers()?;
        // bloom 的顺序与依赖:
        //   采样器 / uniform buffer / 两条 bind group layout → 四条 pipeline
        //   三张半分辨率目标 → 四个 bind group
        // 所以 pipeline 先建(它们只要 layout),目标与 bind group 留给
        // [`WebGpuRenderer::ensure_bloom_targets`] 在拿到画布尺寸时建。
        renderer.create_bloom_resources()?;
        renderer.reserve_instances(INSTANCE_PREALLOC)?;
        Ok(renderer)
    }

    /// 管线创建(**必须**包在 validation error scope 里)。
    ///
    /// WGSL 编译错误与绑定不匹配都是**异步**报上来的:不做 error scope
    /// 的话画面就是纯黑,而 console 里什么也没有。`pushErrorScope(
    /// "validation")` / `popErrorScope()` 把那段文字抓回来再打出去,
    /// 才算真的诊断过,而不是猜。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 管线创建失败时的可读原因。
    pub fn create_pipeline(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        push_error_scope(&device, VALIDATION_SCOPE);
        let built: Result<(), String> = self.build_pipeline_inner();
        // pop 出来的错误被 `pop_error_scope` 打进 console(它是 Promise),
        // 这里拿不到同步结果,所以只用返回值报告「构建本身失败」的情况。
        pop_error_scope(&device, VALIDATION_SCOPE);
        built
    }

    /// 真正建 pipeline 的那几步(外层负责 error scope)。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 失败时的错误文本。
    fn build_pipeline_inner(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        // ⚠️ **顺序**:group 1 的 layout 必须**先**建出来 ——
        // 主 pipeline 的 `pipeline layout` 里要列它(见
        // [`WebGpuRenderer::build_pipeline_layout`]),而 bind group 又要
        // 等阴影纹理与光源矩阵 buffer 建好才能建。三者的依赖是:
        //   group1 layout → 主 pipeline layout → 主 pipeline
        //   阴影纹理 / 光源矩阵 buffer → group1 bind group
        let shadow_group_layout: JsValue = self.build_shadow_sample_group_layout()?;
        // pipeline layout 必须**留到 bind group 创建时再用**,所以这里
        // 存进字段,而不是建完就丢 —— 否则 bind group 无从创建。
        let (layout, group_layout): (JsValue, JsValue) =
            self.build_pipeline_layout(&device, &shadow_group_layout)?;
        let buffers: JsValue = self.build_vertex_layout()?;

        let vertex_module: JsValue = create_shader_module(&device, SHADER_VERTEX)?;
        let fragment_module: JsValue = create_shader_module(&device, SHADER_FRAGMENT)?;

        let vertex_stage: Object = new_object();
        set(&vertex_stage, FIELD_MODULE, &vertex_module)?;
        set(
            &vertex_stage,
            FIELD_ENTRY_POINT,
            &JsValue::from_str(ENTRY_VERTEX),
        )?;
        set(&vertex_stage, FIELD_BUFFERS, &buffers)?;

        let fragment_stage: Object = new_object();
        set(&fragment_stage, FIELD_MODULE, &fragment_module)?;
        set(
            &fragment_stage,
            FIELD_ENTRY_POINT,
            &JsValue::from_str(ENTRY_FRAGMENT),
        )?;

        let targets: JsValue = make_array();
        let color_target: Object = new_object();
        let color_format: String = self.get_format().to_string();
        set(
            &color_target,
            FIELD_FORMAT,
            &JsValue::from_str(&color_format),
        )?;
        push_into(&targets, 0usize, color_target.as_ref())?;
        // ⚠️ `targets` 是 `GPUFragmentState` 的**成员**。少这一行,
        // WebGPU 会报 `Required member is undefined`,而且是异步的 ——
        // 画布直接黑给你看。
        set(&fragment_stage, FIELD_TARGETS, &targets)?;

        let primitive: Object = new_object();
        set(
            &primitive,
            FIELD_TOPOLOGY,
            &JsValue::from_str(TOPOLOGY_TRIANGLE_LIST),
        )?;
        set(
            &primitive,
            FIELD_CULL_MODE,
            &JsValue::from_str(CULL_MODE_BACK),
        )?;
        // WebGPU 的正面默认就是逆时针,这里显式写出来,免得「为什么
        // WebGL2 剔背面这里也剔背面却朝向相反」成为下一个 debug 谜题。
        // ⚠️ **必须是 `ccw`,而且必须与 [`frame_bytes`] 的 Y 取反配套。**
        //
        // [`frame_bytes`] 把矩阵第 1 行取反,让 WebGPU 的 +Y 朝下 NDC
        // 得到和 WebGL 一样的上下方向。这同时**把三角形的屏幕空间绕序
        // 反了过来**:WebGL 里逆时针(正面)的三角形,取反后在 WebGPU 的
        // 坐标系里是顺时针 —— 也就是 WebGPU 的默认 `frontFace`。
        //
        // 只改 frontFace 不翻 Y(旧写法):画面上下颠倒,但正面判定恰好
        // 自洽,所以仍能看到几何 —— 只是你看到的是「从里往外看」的反面。
        // 只翻 Y 不改 frontFace:画面正了,但**所有正面被剔掉**,只剩大片
        // 挤在近处的背面,看起来正是「几块巨大的平色块」。
        set(
            &primitive,
            FIELD_FRONT_FACE,
            &JsValue::from_str(FRONT_FACE_CCW),
        )?;

        let depth_stencil: Object = new_object();
        set(
            &depth_stencil,
            FIELD_FORMAT,
            &JsValue::from_str(DEPTH_FORMAT),
        )?;
        set(&depth_stencil, FIELD_DEPTH_WRITE_ENABLED, &JsValue::TRUE)?;
        set(
            &depth_stencil,
            FIELD_DEPTH_COMPARE,
            &JsValue::from_str(DEPTH_COMPARE_LESS),
        )?;

        let descriptor: Object = new_object();
        set(&descriptor, FIELD_LAYOUT, &layout)?;
        set(&descriptor, FIELD_VERTEX, &vertex_stage)?;
        set(&descriptor, FIELD_FRAGMENT, &fragment_stage)?;
        set(&descriptor, FIELD_PRIMITIVE, &primitive)?;
        set(&descriptor, FIELD_DEPTH_STENCIL, &depth_stencil)?;
        // ⚠️ `multisample` 是 **GPUMultisampleState 字典**(默认就是
        // `{count: 1}`),直接给个数字 `1` 会被读成「非该类型」。
        let multisample: Object = new_object();
        set(&multisample, FIELD_COUNT, &JsValue::from_f64(1.0))?;
        set(&descriptor, FIELD_MULTISAMPLE, multisample.as_ref())?;

        let pipeline: JsValue = call1(&device, METHOD_CREATE_RENDER_PIPELINE, descriptor.as_ref())?;
        self.set_pipeline(pipeline);
        self.set_pipeline_layout(layout);
        self.set_group_layout(group_layout);
        // group 1 的 layout 留着给 bind group 那一步用(它现在还不能建:
        // 阴影纹理与光源矩阵 buffer 都还没分配)。
        self.set_shadow_sample_group_layout(shadow_group_layout);
        Ok(())
    }

    /// 建 pipeline layout:group 0 上两个 uniform binding,group 1 上阴影资源。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - `GPUDevice`。
    /// - `&JsValue` - group 1 的 `GPUBindGroupLayout`(阴影图 + 采样器 +
    ///   光源矩阵)。
    ///
    /// # Returns
    ///
    /// - `Result<(JsValue, JsValue), String>` - `(GPUPipelineLayout, GPUBindGroupLayout)`。
    ///
    /// 后者(group **0** 那个)要留着:pipeline 用 `GPUPipelineLayout`,
    /// 而 `createBindGroup` 要的是里面的 `GPUBindGroupLayout`。拿前者去
    /// 建 bind group 会报
    /// `Failed to convert value to 'GPUBindGroupLayout'`。
    fn build_pipeline_layout(
        &self,
        device: &JsValue,
        shadow_group_layout: &JsValue,
    ) -> Result<(JsValue, JsValue), String> {
        let entries: JsValue = make_array();
        let visibility: f64 = f64::from(visibility_vertex_fragment());
        for (index, binding) in [BINDING_FRAME, BINDING_SHADING].iter().enumerate() {
            let entry: Object = new_object();
            set(&entry, FIELD_BINDING, &JsValue::from_f64(*binding as f64))?;
            set(&entry, FIELD_VISIBILITY, &JsValue::from_f64(visibility))?;
            // ⚠️ `buffer` 是 **GPUBufferBindingLayout 字典**,不是字符串。
            // 传 `"uniform"` 会被读成「对象」,实测报:
            //   Failed to read the 'buffer' property from
            //   'GPUBindGroupLayoutEntry': The provided value is not of
            //   type 'GPUBufferBindingLayout'.
            let buffer_layout: Object = new_object();
            set(
                &buffer_layout,
                FIELD_TYPE,
                &JsValue::from_str(BUFFER_TYPE_UNIFORM),
            )?;
            set(&entry, FIELD_BUFFER, buffer_layout.as_ref())?;
            push_into(&entries, index, entry.as_ref())?;
        }
        // ⚠️ `createBindGroupLayout` 收的是**描述符对象**,`entries`
        // 是它的**一个属性**。把数组本身当描述符传过去,WebGPU 会去找
        // 描述符上的 `entries` 属性,读到 undefined,于是报:
        //   Failed to read the 'entries' property from
        //   'GPUBindGroupLayoutDescriptor'
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_ENTRIES, &entries)?;
        let group_layout: JsValue =
            call1(device, METHOD_CREATE_BIND_GROUP_LAYOUT, descriptor.as_ref())?;
        let layouts: JsValue = make_array();
        push_into(&layouts, 0usize, group_layout.as_ref())?;
        // ⚠️ group 1 必须**列在这里**:WGSL 在 `@group(1)` 声明了阴影图 /
        // 采样器 / 光源矩阵,`bindGroupLayouts` 里没有这一项的话
        // `createRenderPipeline` 直接被判 invalid ——
        // `The entry-point uses bindings in group 1 but [PipelineLayout]
        // doesn't have a BindGroupLayout for this index`。
        push_into(&layouts, 1usize, shadow_group_layout)?;
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_BIND_GROUP_LAYOUTS, &layouts)?;
        let pipeline_layout: JsValue =
            call1(device, METHOD_CREATE_PIPELINE_LAYOUT, descriptor.as_ref())?;
        Ok((pipeline_layout, group_layout))
    }

    /// 建阴影 pass 的管线(纯深度、无颜色输出)。
    ///
    /// 与 [`WebGpuRenderer::create_pipeline`] 一样整段包在 validation
    /// error scope 里:WGSL 里 `textureSampleCompare` 的控制流约束、
    /// bind group layout 与 bind group 不匹配,全是**异步**报的 ——
    /// 不开 error scope 的话症状只有一个:画面纯黑,console 一句话没有。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 管线创建失败时的可读原因。
    pub fn create_shadow_pipeline(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        push_error_scope(&device, VALIDATION_SCOPE);
        let built: Result<(), String> = self.build_shadow_pipeline_inner();
        pop_error_scope(&device, VALIDATION_SCOPE);
        built
    }

    /// 真正建阴影管线的那几步(外层负责 error scope)。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 失败时的错误文本。
    fn build_shadow_pipeline_inner(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        let (layout, group_layout): (JsValue, JsValue) = self.build_shadow_pipeline_layout()?;
        // ⚠️ **顶点 buffer layout 与主 pass 完全相同**:阴影 pass 吃的是
        // 同一份几何缓冲,不需要第二套上传路径,所以这里直接复用。
        let buffers: JsValue = self.build_vertex_layout()?;
        let module: JsValue = create_shader_module(&device, SHADER_SHADOW_VERTEX)?;

        let vertex_stage: Object = new_object();
        set(&vertex_stage, FIELD_MODULE, &module)?;
        set(
            &vertex_stage,
            FIELD_ENTRY_POINT,
            &JsValue::from_str(ENTRY_SHADOW_VERTEX),
        )?;
        set(&vertex_stage, FIELD_BUFFERS, &buffers)?;

        let primitive: Object = new_object();
        set(
            &primitive,
            FIELD_TOPOLOGY,
            &JsValue::from_str(TOPOLOGY_TRIANGLE_LIST),
        )?;
        // ⚠️ 剔**背面**:阴影贴图存的是「从光看过去最靠后的表面」,
        // 与 WebGL2 端 `cull_face(FRONT)` 逐字对应。拿背面当遮挡体能
        // 把自阴影痤疮与 Peter-Panning 一起压掉一个量级。
        set(
            &primitive,
            FIELD_CULL_MODE,
            &JsValue::from_str(CULL_MODE_FRONT),
        )?;
        set(
            &primitive,
            FIELD_FRONT_FACE,
            &JsValue::from_str(FRONT_FACE_CCW),
        )?;

        let depth_stencil: Object = new_object();
        set(
            &depth_stencil,
            FIELD_FORMAT,
            &JsValue::from_str(SHADOW_DEPTH_FORMAT),
        )?;
        set(&depth_stencil, FIELD_DEPTH_WRITE_ENABLED, &JsValue::TRUE)?;
        set(
            &depth_stencil,
            FIELD_DEPTH_COMPARE,
            &JsValue::from_str(DEPTH_COMPARE_LESS),
        )?;

        let descriptor: Object = new_object();
        set(&descriptor, FIELD_LAYOUT, &layout)?;
        set(&descriptor, FIELD_VERTEX, &vertex_stage)?;
        set(&descriptor, FIELD_PRIMITIVE, &primitive)?;
        set(&descriptor, FIELD_DEPTH_STENCIL, &depth_stencil)?;
        let multisample: Object = new_object();
        set(&multisample, FIELD_COUNT, &JsValue::from_f64(1.0))?;
        set(&descriptor, FIELD_MULTISAMPLE, multisample.as_ref())?;
        // ⚠️ **没有 `fragment`**。纯深度 pass 在 WebGPU 里就是
        // 「`vertex` + `depthStencil`,不带 fragment stage」——
        // 带一个什么都不写的 fragment 反而会被判成「fragment 没有
        // target」。这与 WebGL2 那份 `SHADOW_FRAGMENT_SHADER`
        // (`out_color = vec4(1.0)`)的差别就在这里:GL 的 FBO 即使只
        // 挂深度附件也仍然需要一个会写颜色的片元着色器,WebGPU 不需要。

        let pipeline: JsValue = call1(&device, METHOD_CREATE_RENDER_PIPELINE, descriptor.as_ref())?;
        self.set_shadow_pipeline(pipeline);
        self.set_shadow_pipeline_layout(layout);
        self.set_shadow_group_layout(group_layout);
        Ok(())
    }

    /// 阴影管线的 layout:group 0 只有光源矩阵一个 uniform。
    ///
    /// # Returns
    ///
    /// - `Result<(JsValue, JsValue), String>` - `(GPUPipelineLayout, GPUBindGroupLayout)`。
    fn build_shadow_pipeline_layout(&self) -> Result<(JsValue, JsValue), String> {
        let device: JsValue = self.get_device().clone();
        let entries: JsValue = make_array();
        let entry: Object = new_object();
        set(
            &entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_FRAME as f64),
        )?;
        set(
            &entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_VERTEX)),
        )?;
        let buffer_layout: Object = new_object();
        set(
            &buffer_layout,
            FIELD_TYPE,
            &JsValue::from_str(BUFFER_TYPE_UNIFORM),
        )?;
        set(&entry, FIELD_BUFFER, buffer_layout.as_ref())?;
        push_into(&entries, 0usize, entry.as_ref())?;
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_ENTRIES, &entries)?;
        let group_layout: JsValue = call1(
            &device,
            METHOD_CREATE_BIND_GROUP_LAYOUT,
            descriptor.as_ref(),
        )?;
        let layouts: JsValue = make_array();
        push_into(&layouts, 0usize, group_layout.as_ref())?;
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_BIND_GROUP_LAYOUTS, &layouts)?;
        let pipeline_layout: JsValue =
            call1(&device, METHOD_CREATE_PIPELINE_LAYOUT, descriptor.as_ref())?;
        Ok((pipeline_layout, group_layout))
    }

    /// 主 pass 的 **group 1**:阴影深度图 + 比较采样器 + 光源矩阵。
    ///
    /// 单独一个 group 而不是并进 group 0:阴影管线没有 frame / shading,
    /// 两条管线的 group 0 形状**必须**不同(WebGPU 按 index 逐条对齐
    /// `bindGroupLayouts`,形状不一致会直接判 pipeline invalid)。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - `(GPUBindGroupLayout)`,创建失败时的错误文本。
    fn build_shadow_sample_group_layout(&self) -> Result<JsValue, String> {
        let device: JsValue = self.get_device().clone();
        let entries: JsValue = make_array();
        // binding 0:阴影深度纹理。`sampleType: "depth"` 是硬性要求。
        let texture_entry: Object = new_object();
        set(
            &texture_entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_SHADOW_MAP as f64),
        )?;
        set(
            &texture_entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
        )?;
        let texture_layout: Object = new_object();
        set(
            &texture_layout,
            FIELD_SAMPLE_TYPE,
            &JsValue::from_str(SAMPLE_TYPE_DEPTH),
        )?;
        set(&texture_entry, FIELD_TEXTURE, texture_layout.as_ref())?;
        push_into(&entries, 0usize, texture_entry.as_ref())?;
        // binding 1:比较采样器。`type: "comparison"` 与深度纹理配套。
        let sampler_entry: Object = new_object();
        set(
            &sampler_entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_SHADOW_SAMPLER as f64),
        )?;
        set(
            &sampler_entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
        )?;
        let sampler_layout: Object = new_object();
        set(
            &sampler_layout,
            FIELD_TYPE,
            &JsValue::from_str(SAMPLER_TYPE_COMPARISON),
        )?;
        set(&sampler_entry, FIELD_SAMPLER, sampler_layout.as_ref())?;
        push_into(&entries, 1usize, sampler_entry.as_ref())?;
        // binding 2:光源矩阵 uniform(片元阶段要用它投影世界坐标)。
        let frame_entry: Object = new_object();
        set(
            &frame_entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_SHADOW_FRAME as f64),
        )?;
        set(
            &frame_entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
        )?;
        let frame_layout: Object = new_object();
        set(
            &frame_layout,
            FIELD_TYPE,
            &JsValue::from_str(BUFFER_TYPE_UNIFORM),
        )?;
        set(&frame_entry, FIELD_BUFFER, frame_layout.as_ref())?;
        push_into(&entries, 2usize, frame_entry.as_ref())?;

        let group_descriptor: Object = new_object();
        set(&group_descriptor, FIELD_ENTRIES, &entries)?;
        call1(
            &device,
            METHOD_CREATE_BIND_GROUP_LAYOUT,
            group_descriptor.as_ref(),
        )
    }

    /// 主 pass 的 group 1 **bind group**(真正绑资源的那一步)。
    ///
    /// 与 [`WebGpuRenderer::build_shadow_sample_group_layout`] 分成两步
    /// 的原因:主管线的 `pipeline layout` 里必须**已经**含 group 1 的
    /// `GPUBindGroupLayout`(WGSL 在 `@group(1)` 声明了 binding,
    /// layout 里没有这一项就是 validation error:
    /// `The entry-point uses bindings in group 1 but [PipelineLayout]
    /// doesn't have a BindGroupLayout for this index`),
    /// 而 bind group 又必须等阴影纹理与光源矩阵 buffer 都建好 ——
    /// 两者的先后关系不同,所以不能合成一步。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - group 1 的 `GPUBindGroupLayout`。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    pub fn create_shadow_sample_bind_group(&mut self, layout: &JsValue) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        // ---- 比较采样器 ----
        let sampler_descriptor: Object = new_object();
        set(
            &sampler_descriptor,
            FIELD_COMPARE,
            &JsValue::from_str(COMPARE_LESS),
        )?;
        set(
            &sampler_descriptor,
            FIELD_MAG_FILTER,
            &JsValue::from_str(FILTER_LINEAR),
        )?;
        set(
            &sampler_descriptor,
            FIELD_MIN_FILTER,
            &JsValue::from_str(FILTER_LINEAR),
        )?;
        let sampler: JsValue = call1(&device, METHOD_CREATE_SAMPLER, sampler_descriptor.as_ref())?;

        // ---- bind group ----
        let bind_entries: JsValue = make_array();
        let view: JsValue = create_view(&self.get_shadow_texture().clone())?;
        // ⚠️ **纹理资源直接就是 view 本身,不能再包一层 `{view: …}`。**
        // WebGPU 的 `GPUBindingResource` 是个 union:buffer 槽给
        // `GPUBufferBinding`(`{buffer}`),而**纹理槽直接收
        // GPUTextureView**,规范里没有「view 字段」这种东西。包一层
        // 的话 Chrome 会拿 union 去匹配 `GPUBufferBinding`,报出:
        //
        //   Failed to read the 'buffer' property from 'GPUBufferBinding':
        //   Required member is undefined.
        //
        // 报错指向 buffer 槽,但真正出错的是**纹理**那条 —— union
        // 匹配失败后的报错极具误导性,查这个问题时在这儿绕了很久。
        let texture_binding: Object = new_object();
        set(
            &texture_binding,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_SHADOW_MAP as f64),
        )?;
        set(&texture_binding, FIELD_RESOURCE, &view)?;
        push_into(&bind_entries, 0usize, texture_binding.as_ref())?;
        let sampler_binding: Object = new_object();
        set(
            &sampler_binding,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_SHADOW_SAMPLER as f64),
        )?;
        set(&sampler_binding, FIELD_RESOURCE, &sampler)?;
        push_into(&bind_entries, 1usize, sampler_binding.as_ref())?;
        let frame_resource: Object = new_object();
        set(
            &frame_resource,
            FIELD_BUFFER,
            &self.get_shadow_frame_buffer().clone(),
        )?;
        let frame_binding: Object = new_object();
        set(
            &frame_binding,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_SHADOW_FRAME as f64),
        )?;
        set(&frame_binding, FIELD_RESOURCE, frame_resource.as_ref())?;
        push_into(&bind_entries, 2usize, frame_binding.as_ref())?;

        let bind_descriptor: Object = new_object();
        set(&bind_descriptor, FIELD_LAYOUT, layout)?;
        set(&bind_descriptor, FIELD_ENTRIES, &bind_entries)?;
        let bind_group: JsValue =
            call1(&device, METHOD_CREATE_BIND_GROUP, bind_descriptor.as_ref())
                .map_err(|error: String| format!("group1 createBindGroup: {error}"))?;
        self.set_shadow_sample_bind_group(bind_group);
        Ok(())
    }

    /// 建顶点布局数组:一个 stride 48 B 的顶点 buffer + 一个 stride
    /// 80 B 的 instance buffer,后者用 `stepMode: "instance"` 表达实例化。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - `GPUVertexBufferLayout[]`。
    fn build_vertex_layout(&self) -> Result<JsValue, String> {
        // 顶点:pos(3) | normal(3) | color(3) | emissive(3)。
        let vertex_attributes: JsValue = make_array();
        for index in 0..4u32 {
            let attribute: Object = new_object();
            set(
                &attribute,
                FIELD_SHADER_LOCATION,
                &JsValue::from_f64(index as f64),
            )?;
            set(
                &attribute,
                FIELD_OFFSET,
                &JsValue::from_f64((index as u64 * 12) as f64),
            )?;
            set(
                &attribute,
                FIELD_FORMAT,
                &JsValue::from_str(VERTEX_FORMAT_F32X3),
            )?;
            push_into(&vertex_attributes, index as usize, attribute.as_ref())?;
        }
        // instance:model matrix 4 × vec4 在 0..64 B,tint 在 64 B。
        let instance_attributes: JsValue = make_array();
        for index in 0..5u32 {
            let attribute: Object = new_object();
            set(
                &attribute,
                FIELD_SHADER_LOCATION,
                &JsValue::from_f64((index + 4) as f64),
            )?;
            let offset: u64 = if index < 4 { index as u64 * 16 } else { 64 };
            let format: &str = if index < 4 {
                VERTEX_FORMAT_F32X4
            } else {
                VERTEX_FORMAT_F32X3
            };
            set(&attribute, FIELD_OFFSET, &JsValue::from_f64(offset as f64))?;
            set(&attribute, FIELD_FORMAT, &JsValue::from_str(format))?;
            push_into(&instance_attributes, index as usize, attribute.as_ref())?;
        }

        let vertex_buffer: Object = new_object();
        set(
            &vertex_buffer,
            FIELD_ARRAY_STRIDE,
            &JsValue::from_f64(VERTEX_STRIDE_BYTES as f64),
        )?;
        set(
            &vertex_buffer,
            FIELD_STEP_MODE,
            &JsValue::from_str(STEP_MODE_VERTEX),
        )?;
        set(&vertex_buffer, FIELD_ATTRIBUTES, &vertex_attributes)?;

        let instance_buffer: Object = new_object();
        set(
            &instance_buffer,
            FIELD_ARRAY_STRIDE,
            &JsValue::from_f64(INSTANCE_STRIDE_BYTES as f64),
        )?;
        set(
            &instance_buffer,
            FIELD_STEP_MODE,
            &JsValue::from_str(STEP_MODE_INSTANCE),
        )?;
        set(&instance_buffer, FIELD_ATTRIBUTES, &instance_attributes)?;

        let layouts: JsValue = make_array();
        push_into(&layouts, 0usize, vertex_buffer.as_ref())?;
        push_into(&layouts, 1usize, instance_buffer.as_ref())?;
        Ok(layouts)
    }

    /// 建两个 uniform buffer 与它们共用的 bind group。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    pub fn create_uniform_buffers(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        // ⚠️ 这里要 **GPUBindGroupLayout**,不是 GPUPipelineLayout。
        let layout: JsValue = self.get_group_layout().clone();
        if layout.is_null() {
            return Err(String::from(PIPELINE_LAYOUT_MISSING));
        }
        let frame: JsValue =
            create_buffer(&device, UNIFORM_FRAME_BYTES as usize, USAGE_UNIFORM_BUFFER)?;
        let shading: JsValue = create_buffer(
            &device,
            UNIFORM_SHADING_BYTES as usize,
            USAGE_UNIFORM_BUFFER,
        )?;

        let entries: JsValue = make_array();
        for (index, buffer) in [frame.clone(), shading.clone()].iter().enumerate() {
            // `resource` 是 **GPUBindingResource 字典**,即 `{buffer}`;
            // 直接把 GPUBuffer 塞进 `buffer` 会报
            //   Failed to read the 'buffer' property from 'GPUBufferBinding'
            let resource: Object = new_object();
            set(&resource, FIELD_BUFFER, buffer)?;
            let entry: Object = new_object();
            set(&entry, FIELD_BINDING, &JsValue::from_f64(index as f64))?;
            set(&entry, FIELD_RESOURCE, resource.as_ref())?;
            push_into(&entries, index, entry.as_ref())?;
        }
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_LAYOUT, &layout)?;
        set(&descriptor, FIELD_ENTRIES, &entries)?;
        let bind_group: JsValue = call1(&device, METHOD_CREATE_BIND_GROUP, descriptor.as_ref())?;
        self.set_frame_buffer(frame);
        self.set_shading_buffer(shading);
        self.set_bind_group(bind_group);
        // 阴影贴图与「采样组」必须在主 uniform 之后建:group 1 的
        // binding 2 要绑上面刚建的 `shadow_frame_buffer`,而阴影深度
        // 纹理也要先存在才能 `createView`。
        self.create_shadow_texture()?;
        self.create_shadow_frame_buffer()?;
        self.create_shadow_sample_bind_group(&self.get_shadow_sample_group_layout().clone())?;
        Ok(())
    }

    /// 建 bloom 那四条 pipeline 的资源(采样器 / uniform buffer /
    /// 两条 bind group layout / 四条 pipeline)。
    ///
    /// 与主 / 阴影两条管线一样整段包在 validation error scope 里:
    /// 着色器编译错误、bind group layout 与 bind group 不匹配、颜色
    /// 目标格式对不上,**全是异步报上来的** —— 不开 error scope 的
    /// 症状是「画面纯黑,console 一句话没有」。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的可读原因。
    pub fn create_bloom_resources(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        push_error_scope(&device, VALIDATION_SCOPE);
        let built: Result<(), String> = self.build_bloom_resources_inner();
        pop_error_scope(&device, VALIDATION_SCOPE);
        built
    }

    /// 真正建 bloom 资源的那几步(外层负责 error scope)。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 失败时的错误文本。
    fn build_bloom_resources_inner(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        // ---- 普通采样器 ----
        // ⚠️ **不能**复用阴影那张比较采样器:深度纹理只配
        // `sampler_comparison`,而这里的 `rgba8unorm` 颜色纹理只配
        // 普通 `sampler`。混用的结果是 validation error。
        let sampler_descriptor: Object = new_object();
        set(
            &sampler_descriptor,
            FIELD_MAG_FILTER,
            &JsValue::from_str(FILTER_LINEAR),
        )?;
        set(
            &sampler_descriptor,
            FIELD_MIN_FILTER,
            &JsValue::from_str(FILTER_LINEAR),
        )?;
        let sampler: JsValue = call1(&device, METHOD_CREATE_SAMPLER, sampler_descriptor.as_ref())?;
        self.set_bloom_sampler(sampler);

        // ---- 每条 pass 一个 uniform buffer ----
        // 见 [`WebGpuRenderer::bloom_buffers`]:同帧往**同一个** buffer
        // 写四次,只有最后一次生效,而四份数据互不相同。
        //
        // 收集进 `Vec` 再转成定长数组而不是直接按下标写:数组需要先有
        // 一个占位初值,而 `create_buffer` 返回 `Result`、闭包里没法用
        // `?` 传播错误(`array::from_fn` 的闭包返回裸值)。`Vec` 让错误
        // 走正常的 `?`,数组在收集完之后一次性建出来。
        let mut buffers: Vec<JsValue> = Vec::with_capacity(BLOOM_PASS_COUNT);
        for _slot in 0..BLOOM_PASS_COUNT {
            buffers.push(create_buffer(
                &device,
                UNIFORM_BLOOM_BYTES as usize,
                USAGE_UNIFORM_BUFFER,
            )?);
        }
        self.set_bloom_buffers(buffers.try_into().map_err(|_| {
            String::from("bloom uniform buffer count must match BLOOM_PASS_COUNT")
        })?);

        // ---- 两条 bind group layout ----
        let group_layout: JsValue = self.build_bloom_group_layout()?;
        let composite_layout: JsValue = self.build_bloom_composite_group_layout()?;
        // ⚠️ **四条 pipeline 各要一条自己的 pipeline layout。**
        //
        // 四条管线虽然共享同一个 WGSL 模块,但 `fs_composite` 这个入口
        // **静态引用了 binding 3 与 4**(那对模糊图 sampler + 纹理)。
        // WebGPU 校验的是「这条入口用到的每个 binding 都能在 pipeline
        // layout 里找到」—— 于是拿 3 槽 layout 去建合成那条 pipeline
        // 必然报 validation error,而且是**异步**报上来的:合成 pass
        // 整条不被执行,画面停在 clear 的天空色,console 里只有一行
        // `binding 3 is not declared in the pipeline layout`。
        //
        // 反过来「全都用 5 槽那条」也不行:setBindGroup 要求 bind group
        // 与 pipeline layout 的那一组**逐项等价**(group-equivalent),
        // 3 槽的 bind group 不等价于 5 槽的 layout。所以两条 layout
        // 分别配两条 pipeline layout。
        let pipeline_layout: JsValue = Self::single_group_pipeline_layout(&device, &group_layout)?;
        let composite_pipeline_layout: JsValue =
            Self::single_group_pipeline_layout(&device, &composite_layout)?;

        // ---- 四条 pipeline ----
        // 前三条输出到 bloom 的半分辨率目标(`rgba8unorm`),第四条输出到
        // 画布 swapchain。
        let passes: [(&str, &str); BLOOM_PASS_COUNT] = [
            (ENTRY_BLOOM_EXTRACT, BLOOM_TARGET_FORMAT),
            (ENTRY_BLOOM_BLUR_H, BLOOM_TARGET_FORMAT),
            (ENTRY_BLOOM_BLUR_V, BLOOM_TARGET_FORMAT),
            // ⚠️ 合成那条的 target format **必须**等于画布的
            // preferred format。写错(比如照抄上面那个 `rgba8unorm`)
            // 的症状是:前三条 pass 正常跑,合成 pass 的附件与
            // pipeline 声明不匹配,WebGPU 直接把整条 command buffer
            // 判 invalid —— 画布保持 clear 的那一帧天空色,
            // 而 validation 文本只在 error scope 里看得到。
            (ENTRY_BLOOM_COMPOSITE, self.get_format()),
        ];
        let mut built: [GpuBloomPass; BLOOM_PASS_COUNT] = empty_bloom_passes();
        for (index, (entry, format)) in passes.iter().enumerate() {
            // 合成那条走 5 槽那条 pipeline layout,其余三条走 3 槽那条。
            let layout: &JsValue = if index == BLOOM_PASS_COMPOSITE {
                &composite_pipeline_layout
            } else {
                &pipeline_layout
            };
            built[index] = GpuBloomPass {
                pipeline: self.build_bloom_pipeline(entry, format, layout)?,
                bind_group: JsValue::NULL,
            };
        }
        self.set_bloom_passes(built);
        self.set_bloom_group_layout(group_layout);
        self.set_bloom_composite_group_layout(composite_layout);
        // ⚠️ **bind group 这里还不能建** —— 它们要绑 bloom 那三张
        // 半分辨率目标,而目标要等画布尺寸才知道多大,由
        // [`WebGpuRenderer::ensure_bloom_targets`] 负责。提前建的话
        // 拿到的是 `null` 纹理,`createBindGroup` 会静默建出一个
        // 永不正确的 group(报错要等到真正 `setBindGroup` 那一刻)。
        Ok(())
    }

    /// 建一条只含 group 0 的 `GPUPipelineLayout`。
    ///
    /// bloom 那四条管线都只绑 group 0,所以各自只需要一条这样布局 ——
    /// 但**四条各要一条**:合成那条的入口静态引用了 5 个 binding,
    /// 用 3 槽那条去建会被 WebGPU 判 invalid(见
    /// [`WebGpuRenderer::create_bloom_resources`] 的说明)。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - `GPUDevice`。
    /// - `&JsValue` - group 0 的 `GPUBindGroupLayout`。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - 建好的 `GPUPipelineLayout`。
    fn single_group_pipeline_layout(device: &JsValue, group_layout: &JsValue) -> Result<JsValue, String> {
        let layouts: JsValue = make_array();
        push_into(&layouts, 0usize, group_layout)?;
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_BIND_GROUP_LAYOUTS, &layouts)?;
        call1(device, METHOD_CREATE_PIPELINE_LAYOUT, descriptor.as_ref())
    }

    /// 建 bloom 提取 / 模糊 / 合成三条 pipeline 中的一条。
    ///
    /// # Arguments
    ///
    /// - `&str` - 片元着色器入口点名。
    /// - `&str` - 颜色目标格式。
    /// - `&JsValue` - 共用的 pipeline layout。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - 建好的 `GPURenderPipeline`。
    fn build_bloom_pipeline(
        &self,
        entry: &str,
        format: &str,
        pipeline_layout: &JsValue,
    ) -> Result<JsValue, String> {
        let device: JsValue = self.get_device().clone();
        let vertex_module: JsValue = create_shader_module(&device, SHADER_FULLSCREEN_VERTEX)?;
        let fragment_module: JsValue = create_shader_module(&device, SHADER_BLOOM_FRAGMENT)?;

        let vertex_stage: Object = new_object();
        set(&vertex_stage, FIELD_MODULE, &vertex_module)?;
        set(
            &vertex_stage,
            FIELD_ENTRY_POINT,
            &JsValue::from_str(ENTRY_FULLSCREEN_VERTEX),
        )?;
        // ⚠️ **没有 `buffers`**。全屏三角形靠 `vertex_index` 算位置,
        // 整条 pipeline 一个顶点缓冲都不绑 —— 声明一个空数组是必要的
        // (省略的话 WebGPU 读成 `undefined` 而不是「零个」),但
        // 更重要的是**别**照抄主管线那份 48 B + 80 B 的布局,
        // 那会让着色器的 `@builtin(vertex_index)` 与一堆
        // `setVertexBuffer` 全都变成多余的。
        let empty_buffers: JsValue = make_array();
        set(&vertex_stage, FIELD_BUFFERS, &empty_buffers)?;

        let fragment_stage: Object = new_object();
        set(&fragment_stage, FIELD_MODULE, &fragment_module)?;
        set(&fragment_stage, FIELD_ENTRY_POINT, &JsValue::from_str(entry))?;
        let targets: JsValue = make_array();
        let color_target: Object = new_object();
        set(&color_target, FIELD_FORMAT, &JsValue::from_str(format))?;
        push_into(&targets, 0usize, color_target.as_ref())?;
        set(&fragment_stage, FIELD_TARGETS, &targets)?;

        let primitive: Object = new_object();
        set(
            &primitive,
            FIELD_TOPOLOGY,
            &JsValue::from_str(TOPOLOGY_TRIANGLE_LIST),
        )?;
        // ⚠️ 不剔除:全屏三角形只有一面,而且它的屏幕空间绕序取决于
        // WGSL 里那个 `(0,0) (2,0) (0,2)` —— 与画布的上下方向无关,
        // 所以剔背面会在某一侧平台上把整个 pass 剔没。
        set(
            &primitive,
            FIELD_CULL_MODE,
            &JsValue::from_str(CULL_MODE_NONE),
        )?;
        set(
            &primitive,
            FIELD_FRONT_FACE,
            &JsValue::from_str(FRONT_FACE_CCW),
        )?;

        let descriptor: Object = new_object();
        set(&descriptor, FIELD_LAYOUT, pipeline_layout)?;
        set(&descriptor, FIELD_VERTEX, &vertex_stage)?;
        set(&descriptor, FIELD_FRAGMENT, &fragment_stage)?;
        set(&descriptor, FIELD_PRIMITIVE, &primitive)?;
        // ⚠️ **没有 `depthStencil`**。后处理 pass 深度测试既不需要
        // 也不该要 —— 带着一条深度附件去画全屏三角形,读的是上一 pass
        // 留下的深度值,而不是无条件写满。
        let multisample: Object = new_object();
        set(&multisample, FIELD_COUNT, &JsValue::from_f64(1.0))?;
        set(&descriptor, FIELD_MULTISAMPLE, multisample.as_ref())?;

        call1(&device, METHOD_CREATE_RENDER_PIPELINE, descriptor.as_ref())
    }

    /// bloom 提取 / 两条模糊的 bind group layout(3 槽)。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - `GPUBindGroupLayout`。
    fn build_bloom_group_layout(&self) -> Result<JsValue, String> {
        self.build_bloom_layout(BLOOM_LAYOUT_SHAPE_PLAIN)
    }

    /// bloom 合成的 bind group layout(5 槽:多一对模糊图 sampler + 纹理)。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - `GPUBindGroupLayout`。
    fn build_bloom_composite_group_layout(&self) -> Result<JsValue, String> {
        self.build_bloom_layout(BLOOM_LAYOUT_SHAPE_COMPOSITE)
    }

    /// 按形状建 bloom 的 bind group layout。
    ///
    /// # Arguments
    ///
    /// - `bool` - `true` = 合成那条 5 槽,`false` = 提取 / 模糊那条 3 槽。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - `GPUBindGroupLayout`。
    fn build_bloom_layout(&self, composite: bool) -> Result<JsValue, String> {
        let device: JsValue = self.get_device().clone();
        let entries: JsValue = make_array();
        // binding 0:bloom uniform。
        let params_entry: Object = new_object();
        set(
            &params_entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_BLOOM_PARAMS as f64),
        )?;
        set(
            &params_entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
        )?;
        let params_layout: Object = new_object();
        set(
            &params_layout,
            FIELD_TYPE,
            &JsValue::from_str(BUFFER_TYPE_UNIFORM),
        )?;
        set(&params_entry, FIELD_BUFFER, params_layout.as_ref())?;
        push_into(&entries, 0usize, params_entry.as_ref())?;
        // binding 1:普通(filtering)采样器。
        let sampler_entry: Object = new_object();
        set(
            &sampler_entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_BLOOM_SAMPLER as f64),
        )?;
        set(
            &sampler_entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
        )?;
        let sampler_layout: Object = new_object();
        set(
            &sampler_layout,
            FIELD_TYPE,
            &JsValue::from_str(SAMPLER_TYPE_FILTERING),
        )?;
        set(&sampler_entry, FIELD_SAMPLER, sampler_layout.as_ref())?;
        push_into(&entries, 1usize, sampler_entry.as_ref())?;
        // binding 2:颜色输入纹理(sampleType 必须是 `float`)。
        let texture_entry: Object = new_object();
        set(
            &texture_entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_BLOOM_SOURCE as f64),
        )?;
        set(
            &texture_entry,
            FIELD_VISIBILITY,
            &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
        )?;
        let texture_layout: Object = new_object();
        set(
            &texture_layout,
            FIELD_SAMPLE_TYPE,
            &JsValue::from_str(SAMPLE_TYPE_FLOAT),
        )?;
        set(&texture_entry, FIELD_TEXTURE, texture_layout.as_ref())?;
        push_into(&entries, 2usize, texture_entry.as_ref())?;
        // binding 3 / 4:合成专用的模糊图采样器 + 纹理。
        if composite {
            let blur_sampler_entry: Object = new_object();
            set(
                &blur_sampler_entry,
                FIELD_BINDING,
                &JsValue::from_f64(BINDING_BLOOM_BLUR_SAMPLER as f64),
            )?;
            set(
                &blur_sampler_entry,
                FIELD_VISIBILITY,
                &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
            )?;
            let blur_sampler_layout: Object = new_object();
            set(
                &blur_sampler_layout,
                FIELD_TYPE,
                &JsValue::from_str(SAMPLER_TYPE_FILTERING),
            )?;
            set(&blur_sampler_entry, FIELD_SAMPLER, blur_sampler_layout.as_ref())?;
            push_into(&entries, 3usize, blur_sampler_entry.as_ref())?;
            let blur_texture_entry: Object = new_object();
            set(
                &blur_texture_entry,
                FIELD_BINDING,
                &JsValue::from_f64(BINDING_BLOOM_BLUR_MAP as f64),
            )?;
            set(
                &blur_texture_entry,
                FIELD_VISIBILITY,
                &JsValue::from_f64(f64::from(VISIBILITY_FRAGMENT)),
            )?;
            let blur_texture_layout: Object = new_object();
            set(
                &blur_texture_layout,
                FIELD_SAMPLE_TYPE,
                &JsValue::from_str(SAMPLE_TYPE_FLOAT),
            )?;
            set(&blur_texture_entry, FIELD_TEXTURE, blur_texture_layout.as_ref())?;
            push_into(&entries, 4usize, blur_texture_entry.as_ref())?;
        }

        let descriptor: Object = new_object();
        set(&descriptor, FIELD_ENTRIES, &entries)?;
        call1(&device, METHOD_CREATE_BIND_GROUP_LAYOUT, descriptor.as_ref())
    }

    /// 分配阴影深度纹理(`depth32float`,`SHADOW_MAP_SIZE` 见方)。
    ///
    /// 尺寸**固定**,不随画布变:阴影 frustum 的半宽由
    /// [`crate::r#const::SHADOW_HALF_EXTENT`] 决定,贴图边长直接决定
    /// 一个纹素覆盖多少米(`2 * 45 / 2048 ≈ 4.4 cm`)。跟着画布尺寸
    /// 走会让同一个场景在两个分辨率下影子形状不同。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    pub fn create_shadow_texture(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        let descriptor: Object = new_object();
        set(
            &descriptor,
            FIELD_SIZE,
            &make_extent(SHADOW_MAP_SIZE, SHADOW_MAP_SIZE),
        )?;
        set(
            &descriptor,
            FIELD_FORMAT,
            &JsValue::from_str(SHADOW_DEPTH_FORMAT),
        )?;
        set(
            &descriptor,
            FIELD_USAGE,
            &JsValue::from_f64(f64::from(USAGE_SHADOW_TEXTURE)),
        )?;
        let texture: JsValue = call1(&device, METHOD_CREATE_TEXTURE, descriptor.as_ref())?;
        self.set_shadow_texture(texture);
        Ok(())
    }

    /// 分配光源视投影矩阵的 uniform buffer。
    ///
    /// 同一个 buffer 被绑到**两处**:阴影管线(group 0 slot 0)拿它做
    /// 顶点变换,主管线(group 1 slot 2)拿它在片元阶段算光空间坐标。
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    fn create_shadow_frame_buffer(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        let buffer: JsValue = create_buffer(
            &device,
            UNIFORM_SHADOW_FRAME_BYTES as usize,
            USAGE_UNIFORM_BUFFER,
        )?;
        self.set_shadow_frame_buffer(buffer.clone());
        // 阴影管线自己的 bind group:group 0 只有光源矩阵这一个 binding。
        let layout: JsValue = self.get_shadow_group_layout().clone();
        if layout.is_null() {
            return Err(String::from(PIPELINE_LAYOUT_MISSING));
        }
        let entries: JsValue = make_array();
        let resource: Object = new_object();
        set(&resource, FIELD_BUFFER, &buffer)?;
        let entry: Object = new_object();
        set(
            &entry,
            FIELD_BINDING,
            &JsValue::from_f64(BINDING_FRAME as f64),
        )?;
        set(&entry, FIELD_RESOURCE, resource.as_ref())?;
        push_into(&entries, 0usize, entry.as_ref())?;
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_LAYOUT, &layout)?;
        set(&descriptor, FIELD_ENTRIES, &entries)?;
        let bind_group: JsValue = call1(&device, METHOD_CREATE_BIND_GROUP, descriptor.as_ref())
            .map_err(|error: String| format!("shadow-pass createBindGroup: {error}"))?;
        self.set_shadow_bind_group(bind_group);
        Ok(())
    }

    /// 上传一份资产(vertex + index)。
    ///
    /// **下标语义与 WebGL2 端完全相同**:本表按 `Scene::meshes` 的顺序
    /// `push`,`SceneBatch::mesh_index` 直接当下标用。
    ///
    /// # Arguments
    ///
    /// - `&crate::render::MeshAssetGpu` - 已展开的资产。
    ///
    /// # Returns
    ///
    /// - `Result<usize, String>` - 这份资产在本表里的下标。
    pub fn upload_mesh(&mut self, mesh: &crate::render::MeshAssetGpu) -> Result<usize, String> {
        let device: JsValue = self.get_device().clone();
        let vertex_bytes: &[u8] = f32_slice_to_bytes(&mesh.vertices);
        let index_bytes: &[u8] = u32_slice_to_bytes(&mesh.indices);
        let vertex_buffer: JsValue =
            create_buffer(&device, vertex_bytes.len(), USAGE_VERTEX_BUFFER)?;
        let index_buffer: JsValue = create_buffer(&device, index_bytes.len(), USAGE_INDEX_BUFFER)?;
        write_buffer(&device, &vertex_buffer, 0.0, vertex_bytes)?;
        write_buffer(&device, &index_buffer, 0.0, index_bytes)?;
        let index_count: u32 = mesh.indices.len() as u32;
        self.get_meshes_mut().push(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count,
        });
        self.set_gpu_mesh_count(self.get_meshes().len());
        Ok(self.get_meshes().len() - 1)
    }

    /// 原地替换某个已上传 mesh 的顶点 / 索引数据。
    ///
    /// 与 [`crate::render::WebGlRenderer::replace_mesh`] 同样的理由:
    /// 程序化无限世界会流式重建地面 / 水面,**必须**原地覆盖而不是
    /// `push`,否则 GPU 表长度与 `Scene::meshes` 不再一一对应,所有批次
    /// 整体错位一格。
    ///
    /// # Arguments
    ///
    /// - `usize` - `Scene::meshes` 的下标。
    /// - `&crate::render::MeshAssetGpu` - 新的顶点 / 索引数据。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 下标越界或上传失败时的错误文本。
    pub fn replace_mesh(
        &mut self,
        mesh_index: usize,
        mesh: &crate::render::MeshAssetGpu,
    ) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        let vertex_bytes: &[u8] = f32_slice_to_bytes(&mesh.vertices);
        let index_bytes: &[u8] = u32_slice_to_bytes(&mesh.indices);
        let Some(slot) = self.get_meshes_mut().get_mut(mesh_index) else {
            return Err(format!(
                "{}: {mesh_index} of {}",
                REPLACE_MESH_OUT_OF_RANGE,
                self.get_meshes().len()
            ));
        };
        write_buffer(&device, &slot.vertex_buffer, 0.0, vertex_bytes)?;
        write_buffer(&device, &slot.index_buffer, 0.0, index_bytes)?;
        slot.index_count = (index_bytes.len() / 4) as u32;
        Ok(())
    }

    /// 保证 instance buffer 至少能装下 `capacity` 个实例。
    ///
    /// # Arguments
    ///
    /// - `usize` - 需要的实例数。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    pub fn reserve_instances(&mut self, capacity: usize) -> Result<(), String> {
        if capacity <= self.get_instance_capacity() {
            return Ok(());
        }
        let device: JsValue = self.get_device().clone();
        let next: usize = capacity.next_power_of_two();
        let bytes: usize = next * FLOATS_PER_INSTANCE * 4;
        let buffer: JsValue = create_buffer(&device, bytes, USAGE_INSTANCE_BUFFER)?;
        self.set_instance_buffer(buffer);
        self.set_instance_capacity(next);
        Ok(())
    }

    /// 分配/扩容「阴影 pass 专用」的 instance buffer。
    ///
    /// # Arguments
    ///
    /// - `usize` - 需要的实例容量。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    fn reserve_shadow_instances(&mut self, capacity: usize) -> Result<(), String> {
        if capacity <= self.get_shadow_instance_capacity() {
            return Ok(());
        }
        let device: JsValue = self.get_device().clone();
        let next: usize = capacity.next_power_of_two();
        let bytes: usize = next * FLOATS_PER_INSTANCE * 4;
        let buffer: JsValue = create_buffer(&device, bytes, USAGE_INSTANCE_BUFFER)?;
        self.set_shadow_instance_buffer(buffer);
        self.set_shadow_instance_capacity(next);
        Ok(())
    }

    /// 渲染一帧,返回本帧提交的三角形数。
    ///
    /// 阴影 pass 的三角形**也**计入返回值 —— 验收脚本读的
    /// `gpu_submitted` 因此会随阴影 pass 的加入而上涨,这正是「阴影 pass
    /// 真的跑了」的判据。
    ///
    /// # Arguments
    ///
    /// - `RenderParams<'_>` - 场景 / 矩阵 / 光照 / 视口。
    ///
    /// # Returns
    ///
    /// - `Result<u32, String>` - 提交的三角形数。
    pub fn render(&mut self, params: RenderParams<'_>) -> Result<u32, String> {
        let RenderParams {
            scene,
            view_proj,
            lighting,
            eye,
            width,
            height,
            near_cull_radius,
            shadow_focus,
        } = params;
        self.ensure_depth(width, height)?;
        let device: JsValue = self.get_device().clone();
        // 后处理目标跟着画布尺寸走 —— 与 `ensure_depth` 同一个时机。
        self.ensure_bloom_targets(width, height)?;

        // ---- uniform ----

        let frame: [f32; 16] = frame_bytes(view_proj);
        write_buffer(
            &device,
            &self.get_frame_buffer().clone(),
            0.0,
            f32_slice_to_bytes(&frame),
        )?;
        let shading: [f32; 56] = flatten_shading(shading_from_lighting(lighting, eye));
        write_buffer(
            &device,
            &self.get_shading_buffer().clone(),
            0.0,
            f32_slice_to_bytes(&shading),
        )?;
        // 光源矩阵:**必须**走 `frame_bytes` 那套 GL→WebGPU 的深度搬运。
        // 阴影 pass 的顶点着色器直接把结果当裁剪坐标用,而 WebGPU 的
        // NDC 深度是 `[0, 1]`;不搬的话整张阴影图的深度全落在
        // `[-1, 1]`,`ndc.z > 1.0` 的判据会把**所有**片元判成「视锥外」
        // → 影子全丢,而且画面看不出任何异常。
        let light_matrix: Mat4 = shadow_view_projection(shadow_focus, lighting.light_dir);
        let light_bytes: [f32; 16] = frame_bytes(&light_matrix);
        write_buffer(
            &device,
            &self.get_shadow_frame_buffer().clone(),
            0.0,
            f32_slice_to_bytes(&light_bytes),
        )?;

        // ---- pass ----
        // `createCommandEncoder()` 是**零参数**方法。给它传一个
        // `null` 会让返回值不再是 GPUCommandEncoder,于是 submit 时报
        // `Failed to convert value to 'GPUCommandBuffer'`。
        let encoder: JsValue = call0(&device, METHOD_CREATE_COMMAND_ENCODER)?;

        // ---- 1) 阴影 pass(纯深度)----
        //
        // 必须排在主 pass **之前**:主 pass 的片元着色器要在同一个
        // command buffer 里采样这张图。两条 pass 在**同一个 encoder**
        // 里,WebGPU 保证它们按顺序执行 —— 拆成两个 `submit` 就只能
        // 靠「提交顺序恰好成立」这种运气了。
        //
        // `colorAttachments` 是**空数组**:纯深度 pass 没有颜色附件。
        // 注意 WebGPU 的 `beginRenderPass` 要求这个键**存在**,写 `[]`
        // 而不是省略 —— 省掉它会被读成 `undefined` 而不是「零个附件」。
        let shadow_view: JsValue = create_view(&self.get_shadow_texture().clone())?;
        let shadow_depth: Object = new_object();
        set(&shadow_depth, FIELD_VIEW, &shadow_view)?;
        set(
            &shadow_depth,
            FIELD_DEPTH_CLEAR_VALUE,
            &JsValue::from_f64(1.0),
        )?;
        set(
            &shadow_depth,
            FIELD_DEPTH_LOAD_OP,
            &JsValue::from_str(LOAD_OP_CLEAR),
        )?;
        set(
            &shadow_depth,
            FIELD_DEPTH_STORE_OP,
            &JsValue::from_str(STORE_OP_STORE),
        )?;
        let shadow_pass_descriptor: Object = new_object();
        let no_colors: JsValue = make_array();
        set(&shadow_pass_descriptor, FIELD_COLOR_ATTACHMENTS, &no_colors)?;
        set(
            &shadow_pass_descriptor,
            FIELD_DEPTH_STENCIL_ATTACHMENT,
            &shadow_depth,
        )?;
        let shadow_pass: JsValue = call1(
            &encoder,
            METHOD_BEGIN_RENDER_PASS,
            shadow_pass_descriptor.as_ref(),
        )?;
        call1(
            &shadow_pass,
            METHOD_SET_PIPELINE,
            &self.get_shadow_pipeline().clone(),
        )?;
        set_bind_group(
            &shadow_pass,
            BINDING_GROUP,
            &self.get_shadow_bind_group().clone(),
        )?;
        let shadow_triangles: u32 =
            self.draw_shadow_pass(&shadow_pass, scene, shadow_focus, lighting)?;
        call0(&shadow_pass, METHOD_END)?;

        // ---- 2) 主 pass ----
        // ⚠️ 颜色附件是**离屏**的主场景目标,不是 swapchain ——
        // 提取 pass 要把它采样回来算亮度,而 `getCurrentTexture()`
        // 给的那张不能被采样(只有 `RENDER_ATTACHMENT | COPY_SRC`)。
        // 合成那一条 pass 才把它画回 swapchain。
        let scene_view: JsValue = create_view(&self.get_bloom_scene().clone())?;
        let color_attachment: Object = new_object();
        set(&color_attachment, FIELD_VIEW, &scene_view)?;
        set(
            &color_attachment,
            FIELD_CLEAR_VALUE,
            &make_rgba(
                lighting.sky_color[0],
                lighting.sky_color[1],
                lighting.sky_color[2],
            ),
        )?;
        set(
            &color_attachment,
            FIELD_LOAD_OP,
            &JsValue::from_str(LOAD_OP_CLEAR),
        )?;
        set(
            &color_attachment,
            FIELD_STORE_OP,
            &JsValue::from_str(STORE_OP_STORE),
        )?;

        let depth_attachment: Object = new_object();
        let depth_view: JsValue = create_view(&self.get_depth_texture().clone())?;
        set(&depth_attachment, FIELD_VIEW, &depth_view)?;
        set(
            &depth_attachment,
            FIELD_DEPTH_CLEAR_VALUE,
            &JsValue::from_f64(1.0),
        )?;
        set(
            &depth_attachment,
            FIELD_DEPTH_LOAD_OP,
            &JsValue::from_str(LOAD_OP_CLEAR),
        )?;
        set(
            &depth_attachment,
            FIELD_DEPTH_STORE_OP,
            &JsValue::from_str(STORE_OP_STORE),
        )?;

        let pass_descriptor: Object = new_object();
        let color_list: JsValue = make_array();
        push_into(&color_list, 0usize, color_attachment.as_ref())?;
        set(&pass_descriptor, FIELD_COLOR_ATTACHMENTS, &color_list)?;
        set(
            &pass_descriptor,
            FIELD_DEPTH_STENCIL_ATTACHMENT,
            &depth_attachment,
        )?;
        let pass: JsValue = call1(&encoder, METHOD_BEGIN_RENDER_PASS, pass_descriptor.as_ref())?;

        call1(&pass, METHOD_SET_PIPELINE, &self.get_pipeline().clone())?;
        set_bind_group(&pass, BINDING_GROUP, &self.get_bind_group().clone())?;
        // group 1 = 阴影图 + 比较采样器 + 光源矩阵。**必须**在主
        // pipeline 设好之后绑:bind group 与 pipeline layout 是按
        // index 逐条对齐的,缺了 group 1 会得到「着色器读了未绑定的
        // binding」validation error(整帧中止,画面保持上一帧)。
        set_bind_group(
            &pass,
            BINDING_GROUP_SHADOW,
            &self.get_shadow_sample_bind_group().clone(),
        )?;
        // ⚠️ instance buffer 是**按需**建的(`reserve_instances` 在
        // 第一个批次时才分配)。第一帧这里如果直接绑,拿到的还是
        // `JsValue::NULL` —— `setVertexBuffer` 接受 null,但之后
        // `writeBuffer` 写 null 会抛,而 draw 读的是陈旧布局。
        // 先无条件建一个最小容量的,再在 `draw_batch` 里按需增长。
        self.reserve_instances(super::r#const::INSTANCE_CAPACITY_FLOOR)?;

        // ⚠️⚠️⚠️ **整帧的实例必须一次性拼好再上传。**
        //
        // `queue.writeBuffer` 是**队列时间线**上的操作:它在 `submit`
        // 之前排入队列,但**实际执行发生在 command buffer 之前**。所以
        // 「每个批次 writeBuffer(offset 0) 然后立刻 drawIndexed」是错的:
        // 105 个批次的 105 次写入会**全部先跑完**,然后 105 个 draw
        // 才开始执行 —— 于是**每个 draw 读到的都是最后一次写入的内容**,
        // 整座城市被叠到最后一个批次的变换上,画面上只剩几块巨大的平色块
        // (逐列采样只有 1~2 个色带就是这个症状)。
        //
        // 正确做法:把本帧所有批次的实例**拼成一段连续数据**,一次性
        // 上传,然后用 `drawIndexed` 的 `firstInstance` 参数让每个批次
        // 从自己那一段开始读。
        let mut staged: Vec<(usize, Vec<&crate::render::Instance>)> = Vec::new();
        let mut total: usize = 0;
        let hidden: Vec<usize> = crate::game::hidden_batches();
        for (index, batch) in scene.batches.iter().enumerate() {
            // ⚠️ **不能**按 `opaque` 过滤!这个标志在 WebGL2 端的含义是
            // 「这个批次要不要参与背面剔除」,地面 / 水面等大片是
            // `opaque == false`(单面朝上,剔掉背面反而是对的)。
            // 直接 `!opaque => continue` 会把**地面整批丢掉** ——
            // 画面只剩远处建筑漂在天边,近处完全没有地。
            if batch.instances.is_empty() || hidden.contains(&index) {
                continue;
            }
            let kept: Vec<&crate::render::Instance> = if batch.near_cull {
                batch
                    .instances
                    .iter()
                    .filter(|instance: &&crate::render::Instance| {
                        distance_to(instance, eye) > near_cull_radius
                    })
                    .collect()
            } else {
                batch.instances.iter().collect()
            };
            if kept.is_empty() {
                continue;
            }
            if self.get_meshes().get(batch.mesh_index).is_none() {
                continue;
            }
            total += kept.len();
            staged.push((batch.mesh_index, kept));
        }
        if total == 0 {
            call0(&pass, METHOD_END)?;
            // ⚠️ **不能**在这里直接 finish:主 pass 现在画进的是**离屏**
            // 的 `bloom_scene`,不是画布。于是即使一个实例都没有,合成那条
            // pass 也**必须**跑 —— 它才是把离屏画面搬回 swapchain 的
            // 唯一一步(把那帧天空色拷到屏幕上)。提前返回的话画布这一
            // 帧完全不写,于是停留在上一帧的内容,表现为「切到空场景后
            // 画面冻住」。
            self.draw_bloom_chain(&device, &encoder, lighting)?;
            return self.finish_frame(&device, &encoder, shadow_triangles);
        }

        // 一次上传整帧实例,按批次切段。
        self.reserve_instances(total)?;
        // ⚠️ **必须在 `reserve_instances` 之后绑定**:容量变大时它会
        // **重建** buffer,而 `setVertexBuffer` 记的是对象本身 —— 早一步
        // 绑定的就是那个已经被丢弃的旧 buffer,地面这类大批次会整批消失。
        let instance_buffer: JsValue = self.get_instance_buffer().clone();
        set_vertex_buffer(&pass, SLOT_INSTANCE, &instance_buffer)?;
        let mut all: Vec<f32> = Vec::with_capacity(total * FLOATS_PER_INSTANCE);
        let mut ranges: Vec<(usize, u32, u32)> = Vec::with_capacity(staged.len());
        for (mesh_index, instances) in staged {
            let first: u32 = (all.len() / FLOATS_PER_INSTANCE) as u32;
            let count: u32 = instances.len() as u32;
            for instance in instances {
                let before: usize = all.len();
                all.extend_from_slice(instance.get_model_ref());
                all.extend_from_slice(&instance.tint);
                // pad 到 FLOATS_PER_INSTANCE:model 16 + tint 3 + 1 个 pad。
                // 这一个 pad 是必需的 —— 少写会让第 2 个及以后的实例整体
                // 前移一个 f32,读到错位的 model matrix。
                all.resize(before + FLOATS_PER_INSTANCE, 0.0);
            }
            ranges.push((mesh_index, first, count));
        }
        write_buffer(&device, &instance_buffer, 0.0, f32_slice_to_bytes(&all))?;

        let mut triangles: u32 = shadow_triangles;
        for (mesh_index, first, count) in ranges {
            triangles += self.draw_slice(&pass, mesh_index, first, count)?;
        }
        call0(&pass, METHOD_END)?;

        // ---- 3) bloom:提取 → 模糊 H → 模糊 V ----
        // ---- 4) 合成:主场景 + 辉光 → swapchain ----
        //
        // ⚠️ **必须在同一个 encoder 里、且必须排在主 pass 之后。**
        // 提取要采样主 pass 刚写完的那张离屏目标,合成要把它读回来。
        // 拆成第二个 `submit` 只能靠「提交顺序恰好成立」这种运气,
        // 而且跨 submit 读刚写的纹理是 WebGPU 明确不允许的。
        self.draw_bloom_chain(&device, &encoder, lighting)?;
        self.finish_frame(&device, &encoder, triangles)
    }

    /// 跑完整条 bloom 链并把结果合成到画布。
    ///
    /// 四条 pass 与阴影 / 主 pass **共用同一个 command encoder**,
    /// 整帧只 `finish()` + `submit()` 一次。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - `GPUDevice`。
    /// - `&JsValue` - 本帧的 `GPUCommandEncoder`。
    /// - `&SceneLighting` - 当前光照(合成强度取 `emissive_gain`)。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 失败时的错误文本。
    fn draw_bloom_chain(
        &mut self,
        device: &JsValue,
        encoder: &JsValue,
        lighting: &SceneLighting,
    ) -> Result<(), String> {
        let size: (u32, u32) = self.get_bloom_size();
        let half: (u32, u32) = bloom_target_size(size.0, size.1);
        let spread: f32 = BLOOM_BLUR_SPREAD;
        let strength: f32 = bloom_composite_strength(lighting);
        let texel_x: f32 = 1.0 / (half.0.max(1) as f32);
        let texel_y: f32 = 1.0 / (half.1.max(1) as f32);

        // ⚠️ **四条 pass 各写自己的 uniform buffer。** `queue.writeBuffer`
        // 在队列时间线上先于整条 command buffer 执行,共用一个 buffer
        // 的话只有最后那次上传生效 —— 两条模糊会拿到同一个方向,
        // 光晕变成横向条纹。见 [`WebGpuRenderer::bloom_buffers`]。
        //
        // 下标用 [`BLOOM_PASS_*`] 具名常量标出来,而不是靠位置隐含 ——
        // `uploads` 的顺序与 [`BLOOM_PASS_COUNT`] 那个数组的字面量顺序
        // 必须逐项对上,而提取 / 合成两条的 uniform 长得**完全一样**
        // (`(0.0, 0.0, (0.0, 0.0), …)`),一旦中间插进一条就看不出错位。
        // 方向值与 WebGL2 端 `render_bloom` 逐项相同
        // (`texel * BLOOM_BLUR_SPREAD`,H 取 x、V 取 y)。
        let mut uploads: [BloomPassUpload; BLOOM_PASS_COUNT] =
            [BloomPassUpload::default(); BLOOM_PASS_COUNT];
        // 提取:方向无关,但仍要占两个分量(WGSL 的 struct 布局固定)。
        uploads[BLOOM_PASS_EXTRACT] = BloomPassUpload {
            threshold: BLOOM_THRESHOLD,
            size: half,
            ..BloomPassUpload::default()
        };
        // 模糊 H:沿 u 走。
        uploads[BLOOM_PASS_BLUR_H] = BloomPassUpload {
            direction: (texel_x * spread, 0.0),
            size: half,
            ..BloomPassUpload::default()
        };
        // 模糊 V:沿 v 走。
        uploads[BLOOM_PASS_BLUR_V] = BloomPassUpload {
            direction: (0.0, texel_y * spread),
            size: half,
            ..BloomPassUpload::default()
        };
        // 合成:方向无关,只有强度有意义。
        uploads[BLOOM_PASS_COMPOSITE] = BloomPassUpload {
            strength,
            size,
            ..BloomPassUpload::default()
        };
        let buffers: [JsValue; BLOOM_PASS_COUNT] = self.get_bloom_buffers();
        for (index, upload) in uploads.iter().enumerate() {
            let bytes: [f32; 12] = flatten_bloom(bloom_uniforms(
                upload.threshold,
                upload.strength,
                upload.direction,
                upload.size,
            ));
            write_buffer(device, &buffers[index], 0.0, f32_slice_to_bytes(&bytes))?;
        }

        // 提取 / 两条模糊写进半分辨率目标,合成写进 swapchain。
        let canvas_view: JsValue =
            create_view(&call0(&self.get_context().clone(), METHOD_GET_CURRENT_TEXTURE)?)?;
        let targets: [JsValue; BLOOM_PASS_COUNT] = [
            create_view(&self.get_bloom_bright().clone())?,
            create_view(&self.get_bloom_ping().clone())?,
            create_view(&self.get_bloom_pong().clone())?,
            canvas_view,
        ];
        let passes: [GpuBloomPass; BLOOM_PASS_COUNT] = self.get_bloom_passes().clone();
        for index in 0..BLOOM_PASS_COUNT {
            // 每条 pass 的附件都是纯覆盖 —— 后处理不读上一 pass 的
            // 输出作为附件内容,所以 `loadOp: "clear"` + `clearValue: 0`
            // 既是必须的,也是最省的一条路径。
            let attachment: Object = new_object();
            set(&attachment, FIELD_VIEW, &targets[index])?;
            set(&attachment, FIELD_CLEAR_VALUE, &make_rgba(0.0, 0.0, 0.0))?;
            set(
                &attachment,
                FIELD_LOAD_OP,
                &JsValue::from_str(LOAD_OP_CLEAR),
            )?;
            set(
                &attachment,
                FIELD_STORE_OP,
                &JsValue::from_str(STORE_OP_STORE),
            )?;
            let descriptor: Object = new_object();
            let colors: JsValue = make_array();
            push_into(&colors, 0usize, attachment.as_ref())?;
            set(&descriptor, FIELD_COLOR_ATTACHMENTS, &colors)?;
            // ⚠️ **不**带 `depthStencilAttachment`:后处理三条半分辨率
            // pass 与合成那条都只碰颜色,挂一条深度附件会强制它们
            // 共享主 pass 的深度纹理尺寸 —— 而主 pass 那张是全分辨率。
            let pass: JsValue =
                call1(encoder, METHOD_BEGIN_RENDER_PASS, descriptor.as_ref())?;
            call1(&pass, METHOD_SET_PIPELINE, &passes[index].pipeline)?;
            set_bind_group(&pass, BINDING_GROUP, &passes[index].bind_group)?;
            // 全屏三角形:`draw(3)` 而不是 `drawIndexed` —— 整条
            // pipeline 一个顶点缓冲都没绑。
            call1(&pass, METHOD_DRAW, &JsValue::from_f64(3.0))?;
            call0(&pass, METHOD_END)?;
        }
        Ok(())
    }

    /// 阴影 pass:逐实例剔除后把整帧实例一次性拼好、上传、画一遍。
    ///
    /// **复用主 pass 那份 instance buffer 与逐帧整体上传的策略**
    /// (`queue.writeBuffer` 在队列时间线上先于 command buffer 执行 ——
    /// 逐批次写会全部先跑完,每个 draw 读到最后一次写入的内容)。
    /// 所以这里与主 pass 用**同一个 buffer**,靠 `firstInstance`
    /// 切段;两段的实例内容不同(主 pass 带近处剔除,阴影 pass 不带),
    /// 因此阴影 pass 必须排在主 pass 的上传**之前**。
    ///
    /// 不做近处剔除:被剔掉的实例如果还留着影子,地面上会出现一块
    /// 「无中生有」的暗斑 —— 与 WebGL2 端同一理由。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - 阴影 render pass。
    /// - `&crate::render::Scene` - 场景。
    /// - `Vec3` - 阴影 frustum 中心(世界坐标)。
    /// - `&SceneLighting` - 当前光照(取 `light_dir` 做剔除)。
    ///
    /// # Returns
    ///
    /// - `Result<u32, String>` - 本 pass 提交的三角形数。
    fn draw_shadow_pass(
        &mut self,
        pass: &JsValue,
        scene: &crate::render::Scene,
        shadow_focus: Vec3,
        lighting: &SceneLighting,
    ) -> Result<u32, String> {
        let device: JsValue = self.get_device().clone();
        let hidden: Vec<usize> = crate::game::hidden_batches();
        // 阴影 pass 只画 frustum 内的实例:整座城市每帧都往
        // 2048² 的阴影贴图上提交顶点,而阴影 frustum 只覆盖玩家周围
        // `SHADOW_HALF_EXTENT` 米 —— 视锥外那些画上去的深度**永远
        // 不会被采样到**,是纯粹的浪费(WebGL2 端实测省了 20.9%)。
        //
        // 逐实例剔除而不是整批跳过的原因:一排行道树 / 一排路灯往往跨在
        // 视锥边界上,整批丢会把还在范围内的影子也弄没。
        //
        // ⚠️ 判据函数是 `render.rs` 那份(带**上下两支探针**的版本),
        // 不要换成「只看原点」的简化版:斜光下 40 m 高的楼,原点落点
        // 在视锥外而楼底落点在视锥内,只看原点会凭空抹掉一整块长影子。
        let mut staged: Vec<(usize, Vec<&crate::render::Instance>)> = Vec::new();
        let mut total: usize = 0;
        for (index, batch) in scene.batches.iter().enumerate() {
            if batch.instances.is_empty() || hidden.contains(&index) {
                continue;
            }
            let visible: Vec<&crate::render::Instance> = batch
                .instances
                .iter()
                .filter(|instance: &&crate::render::Instance| {
                    instance_affects_shadow(instance, shadow_focus, lighting.light_dir)
                })
                .collect();
            if visible.is_empty() {
                continue;
            }
            if self.get_meshes().get(batch.mesh_index).is_none() {
                continue;
            }
            total += visible.len();
            staged.push((batch.mesh_index, visible));
        }
        if total == 0 {
            return Ok(0);
        }
        // ⚠️ **必须是阴影 pass 自己的 buffer,不能复用主 pass 那份。**
        //
        // `queue.writeBuffer` 排在**队列时间线**上,整条 command buffer
        // 的所有 draw 之前就执行完了。于是同一帧里两次上传(阴影 135 个
        // 实例、主管线 ~1,980 个)都会落在 draw 之前,**后写的覆盖先写的**:
        // 阴影 pass 的 draw 读到的是主管线那份更大的数据,`firstInstance`
        // 指向的偏移全错,模型矩阵乱七八糟 —— 画进深度图的是一堆乱码
        // 三角形,实际深度比较下来「没有东西挡住光」,于是影子全丢。
        //
        // 症状极具欺骗性:pass 在跑、validation clean、三角形数也对,
        // 但地面上一点影子都没有。
        self.reserve_shadow_instances(total)?;
        let instance_buffer: JsValue = self.get_shadow_instance_buffer().clone();
        set_vertex_buffer(pass, SLOT_INSTANCE, &instance_buffer)?;
        let mut all: Vec<f32> = Vec::with_capacity(total * FLOATS_PER_INSTANCE);
        let mut ranges: Vec<(usize, u32, u32)> = Vec::with_capacity(staged.len());
        for (mesh_index, instances) in staged {
            let first: u32 = (all.len() / FLOATS_PER_INSTANCE) as u32;
            let count: u32 = instances.len() as u32;
            for instance in instances {
                let before: usize = all.len();
                all.extend_from_slice(instance.get_model_ref());
                all.extend_from_slice(&instance.tint);
                // pad 到 FLOATS_PER_INSTANCE:少写会让第 2 个及以后的
                // 实例整体前移一个 f32,读到错位的 model matrix。
                all.resize(before + FLOATS_PER_INSTANCE, 0.0);
            }
            ranges.push((mesh_index, first, count));
        }
        write_buffer(&device, &instance_buffer, 0.0, f32_slice_to_bytes(&all))?;
        let mut triangles: u32 = 0;
        for (mesh_index, first, count) in ranges {
            triangles += self.draw_slice(pass, mesh_index, first, count)?;
        }
        Ok(triangles)
    }

    /// `finish()` + `submit`,并返回本帧三角形数。
    ///
    /// ⚠️ 必须 `finish()`,而且要 submit **finish 的返回值**。
    /// 直接把 encoder 塞进 submit 数组会得到
    /// `Failed to convert value to 'GPUCommandBuffer'`。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - `GPUDevice`。
    /// - `&JsValue` - 本帧的 `GPUCommandEncoder`。
    /// - `u32` - 本帧三角形数。
    ///
    /// # Returns
    ///
    /// - `Result<u32, String>` - 原样返回 `triangles`。
    fn finish_frame(
        &mut self,
        device: &JsValue,
        encoder: &JsValue,
        triangles: u32,
    ) -> Result<u32, String> {
        let command_buffer: JsValue = call0(encoder, METHOD_FINISH)?;
        let commands: JsValue = make_array();
        push_into(&commands, 0usize, &command_buffer)?;
        // `GPUDevice.queue` 是**属性**,不是方法 —— 用 `call0` 去调它
        // 会得到 `queue is not a function`。
        let queue: JsValue = property(device, METHOD_QUEUE)?;
        call1(&queue, METHOD_SUBMIT, &commands)?;
        Ok(triangles)
    }

    /// 一次 instanced `drawIndexed`,实例从整帧缓冲的 `first_instance` 段读。
    ///
    /// ⚠️ 实例数据**已经**由 [`WebGpuRenderer::render`] 整帧一次性上传,
    /// 这里**绝不能**再 `writeBuffer` —— 见 `render()` 里关于队列
    /// 时间线的说明。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - 当前 render pass。
    /// - `usize` - `Scene::meshes` 的下标。
    /// - `u32` - 本批次在整帧实例缓冲里的起始实例号。
    /// - `u32` - 本批次的实例数。
    ///
    /// # Returns
    ///
    /// - `Result<u32, String>` - 本批次提交的三角形数。
    fn draw_slice(
        &mut self,
        pass: &JsValue,
        mesh_index: usize,
        first_instance: u32,
        count: u32,
    ) -> Result<u32, String> {
        let Some(mesh) = self.get_meshes().get(mesh_index).cloned() else {
            // 与 WebGL2 端同一条静默路径:下标越界不报错,只是不画。
            return Ok(0);
        };
        set_vertex_buffer(pass, SLOT_VERTEX, &mesh.vertex_buffer)?;
        set_index_buffer(pass, &mesh.index_buffer)?;
        // `first_instance` 让顶点着色器的 `@location(4..8)` 从
        // instance buffer 的对应位置读 —— instance buffer 是整帧
        // 连续的一段,每个批次占其中 `count` 个实例。
        draw_indexed(pass, mesh.index_count, count, 0u32, 0u32, first_instance)?;
        Ok((mesh.index_count / 3) * count)
    }

    /// 画布尺寸变化时重建深度纹理。
    ///
    /// # Arguments
    ///
    /// - `u32` - 画布宽。
    /// - `u32` - 画布高。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    pub fn ensure_depth(&mut self, width: u32, height: u32) -> Result<(), String> {
        let size: (u32, u32) = (width.max(1), height.max(1));
        if size == self.get_depth_size() && !self.get_depth_texture().is_null() {
            return Ok(());
        }
        let device: JsValue = self.get_device().clone();
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_SIZE, &make_extent(size.0, size.1))?;
        set(&descriptor, FIELD_FORMAT, &JsValue::from_str(DEPTH_FORMAT))?;
        set(
            &descriptor,
            FIELD_USAGE,
            &JsValue::from_f64(f64::from(USAGE_DEPTH_TEXTURE)),
        )?;
        let texture: JsValue = call1(&device, METHOD_CREATE_TEXTURE, descriptor.as_ref())?;
        self.set_depth_texture(texture);
        self.set_depth_size(size);
        Ok(())
    }

    /// 画布尺寸变化时重建 bloom 的四张后处理目标,并重建四个
    /// bind group。
    ///
    /// **必须**跟着 resize 走:bloom 目标不是 `SHADOW_MAP_SIZE` 那样
    /// 的固定尺寸(阴影图的纹素对应固定的世界面积,而 bloom 的纹素
    /// 只是「模糊核的一个步长」),它按 `BLOOM_SCALE` 跟着画布走 ——
    /// 与 WebGL2 端 `ensure_targets` 的处理一致。
    ///
    /// 重建时 bind group 也**必须**一起重建:它们绑的是这些纹理的
    /// view,而纹理换了对象,旧 view 会指向已经销毁的纹理。
    ///
    /// # Arguments
    ///
    /// - `u32` - 画布宽。
    /// - `u32` - 画布高。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    pub fn ensure_bloom_targets(&mut self, width: u32, height: u32) -> Result<(), String> {
        let canvas_size: (u32, u32) = (width.max(1), height.max(1));
        let first_time: bool = self.get_bloom_passes()[BLOOM_PASS_EXTRACT]
            .bind_group
            .is_null();
        if canvas_size != self.get_bloom_size() || first_time {
            let half: (u32, u32) = bloom_target_size(canvas_size.0, canvas_size.1);
            // ---- 全分辨率主场景目标 ----
            // 格式必须与画布 preferred format **逐项相同**,否则合成
            // 那条 pipeline 的 target 与附件不匹配,整帧被判 invalid。
            let scene: JsValue = self.create_postprocess_texture(
                canvas_size.0,
                canvas_size.1,
                self.get_format(),
            )?;
            // ---- 三张半分辨率 bloom 目标 ----
            let bright: JsValue = self.create_postprocess_texture(
                half.0,
                half.1,
                BLOOM_TARGET_FORMAT,
            )?;
            let ping: JsValue =
                self.create_postprocess_texture(half.0, half.1, BLOOM_TARGET_FORMAT)?;
            let pong: JsValue =
                self.create_postprocess_texture(half.0, half.1, BLOOM_TARGET_FORMAT)?;
            self.set_bloom_scene(scene);
            self.set_bloom_bright(bright);
            self.set_bloom_ping(ping);
            self.set_bloom_pong(pong);
            self.set_bloom_size(canvas_size);
            self.create_bloom_bind_groups()?;
        }
        Ok(())
    }

    /// 建一张后处理用的颜色纹理(可渲染 + 可采样)。
    ///
    /// # Arguments
    ///
    /// - `u32` - 宽(像素)。
    /// - `u32` - 高(像素)。
    /// - `&str` - 纹理格式。
    ///
    /// # Returns
    ///
    /// - `Result<JsValue, String>` - 建好的 `GPUTexture`。
    fn create_postprocess_texture(
        &self,
        width: u32,
        height: u32,
        format: &str,
    ) -> Result<JsValue, String> {
        let device: JsValue = self.get_device().clone();
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_SIZE, &make_extent(width, height))?;
        set(&descriptor, FIELD_FORMAT, &JsValue::from_str(format))?;
        // ⚠️ `RENDER_ATTACHMENT | TEXTURE_BINDING` 两个都要:这四张既
        // 是某个 pass 的颜色附件,又是下一个 pass 采样回来的输入。
        set(
            &descriptor,
            FIELD_USAGE,
            &JsValue::from_f64(f64::from(USAGE_BLOOM_TARGET)),
        )?;
        call1(&device, METHOD_CREATE_TEXTURE, descriptor.as_ref())
    }

    /// 给四条 bloom pass 各建一个 bind group。
    ///
    /// ⚠️ 每个 bind group 的**颜色输入**是上一 pass 的输出:
    /// 提取读主场景 → 模糊 H 读 bright → 模糊 V 读 ping →
    /// 合成读「主场景 + pong」。所以这里是一次性的**链式**绑定,
    /// 不能少任何一环。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 创建失败时的错误文本。
    fn create_bloom_bind_groups(&mut self) -> Result<(), String> {
        let device: JsValue = self.get_device().clone();
        let sampler: JsValue = self.get_bloom_sampler().clone();
        let buffers: [JsValue; BLOOM_PASS_COUNT] = self.get_bloom_buffers();
        let scene_view: JsValue = create_view(&self.get_bloom_scene().clone())?;
        // 每条 pass 的颜色输入:前三张是 bloom 目标,合成那条是主场景。
        let sources: [JsValue; BLOOM_PASS_COUNT] = [
            scene_view.clone(),
            create_view(&self.get_bloom_bright().clone())?,
            create_view(&self.get_bloom_ping().clone())?,
            scene_view.clone(),
        ];
        // 合成那条的「模糊图」是竖直模糊的输出。
        let blur_maps: [Option<JsValue>; BLOOM_PASS_COUNT] = [
            None,
            None,
            None,
            Some(create_view(&self.get_bloom_pong().clone())?),
        ];
        let mut passes: [GpuBloomPass; BLOOM_PASS_COUNT] = self.get_bloom_passes().clone();
        for index in 0..BLOOM_PASS_COUNT {
            let layout: JsValue = if index == BLOOM_PASS_COMPOSITE {
                self.get_bloom_composite_group_layout().clone()
            } else {
                self.get_bloom_group_layout().clone()
            };
            let entries: JsValue = make_array();
            // slot 0:bloom uniform。
            let resource: Object = new_object();
            set(&resource, FIELD_BUFFER, &buffers[index])?;
            let entry: Object = new_object();
            set(
                &entry,
                FIELD_BINDING,
                &JsValue::from_f64(BINDING_BLOOM_PARAMS as f64),
            )?;
            set(&entry, FIELD_RESOURCE, resource.as_ref())?;
            push_into(&entries, 0usize, entry.as_ref())?;
            // slot 1:采样器。
            let entry: Object = new_object();
            set(
                &entry,
                FIELD_BINDING,
                &JsValue::from_f64(BINDING_BLOOM_SAMPLER as f64),
            )?;
            set(&entry, FIELD_RESOURCE, &sampler)?;
            push_into(&entries, 1usize, entry.as_ref())?;
            // slot 2:颜色输入。⚠️ **裸 view**,不是 `{view: …}`。
            let entry: Object = new_object();
            set(
                &entry,
                FIELD_BINDING,
                &JsValue::from_f64(BINDING_BLOOM_SOURCE as f64),
            )?;
            set(&entry, FIELD_RESOURCE, &sources[index])?;
            push_into(&entries, 2usize, entry.as_ref())?;
            // slot 3 / 4:合成专用的模糊图。
            if let Some(blur_map) = blur_maps[index].clone() {
                let entry: Object = new_object();
                set(
                    &entry,
                    FIELD_BINDING,
                    &JsValue::from_f64(BINDING_BLOOM_BLUR_SAMPLER as f64),
                )?;
                set(&entry, FIELD_RESOURCE, &sampler)?;
                push_into(&entries, 3usize, entry.as_ref())?;
                let entry: Object = new_object();
                set(
                    &entry,
                    FIELD_BINDING,
                    &JsValue::from_f64(BINDING_BLOOM_BLUR_MAP as f64),
                )?;
                set(&entry, FIELD_RESOURCE, &blur_map)?;
                push_into(&entries, 4usize, entry.as_ref())?;
            }
            let descriptor: Object = new_object();
            set(&descriptor, FIELD_LAYOUT, &layout)?;
            set(&descriptor, FIELD_ENTRIES, &entries)?;
            let group: JsValue =
                call1(&device, METHOD_CREATE_BIND_GROUP, descriptor.as_ref()).map_err(
                    |error: String| format!("bloom pass {index} createBindGroup: {error}"),
                )?;
            passes[index].bind_group = group;
        }
        self.set_bloom_passes(passes);
        Ok(())
    }

    /// `GPUDevice` 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - 设备句柄。
    pub fn get_device(&self) -> &JsValue {
        &self.device
    }

    /// 画布上下文的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUCanvasContext`。
    pub fn get_context(&self) -> &JsValue {
        &self.context
    }

    /// 画布格式。
    ///
    /// # Returns
    ///
    /// - `&str` - `bgra8unorm` 之类。
    pub fn get_format(&self) -> &str {
        &self.format
    }

    /// GPU 侧资产表的只读引用。
    ///
    /// # Returns
    ///
    /// - `&[GpuMesh]` - 资产表。
    pub fn get_meshes(&self) -> &[GpuMesh] {
        &self.meshes
    }

    /// GPU 侧资产表的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<GpuMesh>` - 资产表。
    pub fn get_meshes_mut(&mut self) -> &mut Vec<GpuMesh> {
        &mut self.meshes
    }

    /// 主 pipeline 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - pipeline 句柄。
    pub fn get_pipeline(&self) -> &JsValue {
        &self.pipeline
    }

    /// 设置主 pipeline。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - 建好的 pipeline。
    pub fn set_pipeline(&mut self, pipeline: JsValue) {
        self.pipeline = pipeline;
    }

    /// pipeline layout 的只读引用。
    ///
    /// # Returns
    ///
    /// 设置 pipeline layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - layout 句柄。
    pub fn set_pipeline_layout(&mut self, layout: JsValue) {
        self.pipeline_layout = layout;
    }

    /// bind group layout 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn get_group_layout(&self) -> &JsValue {
        &self.group_layout
    }

    /// 设置 bind group layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn set_group_layout(&mut self, layout: JsValue) {
        self.group_layout = layout;
    }

    /// 视投影 uniform buffer 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - buffer 句柄。
    pub fn get_frame_buffer(&self) -> &JsValue {
        &self.frame_buffer
    }

    /// 设置视投影 uniform buffer。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - buffer 句柄。
    pub fn set_frame_buffer(&mut self, buffer: JsValue) {
        self.frame_buffer = buffer;
    }

    /// 着色参数 uniform buffer 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - buffer 句柄。
    pub fn get_shading_buffer(&self) -> &JsValue {
        &self.shading_buffer
    }

    /// 设置着色参数 uniform buffer。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - buffer 句柄。
    pub fn set_shading_buffer(&mut self, buffer: JsValue) {
        self.shading_buffer = buffer;
    }

    /// bind group 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - bind group 句柄。
    pub fn get_bind_group(&self) -> &JsValue {
        &self.bind_group
    }

    /// 设置 bind group。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - bind group 句柄。
    pub fn set_bind_group(&mut self, group: JsValue) {
        self.bind_group = group;
    }

    /// instance buffer 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - buffer 句柄。
    pub fn get_instance_buffer(&self) -> &JsValue {
        &self.instance_buffer
    }

    /// 设置 instance buffer。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - buffer 句柄。
    pub fn set_instance_buffer(&mut self, buffer: JsValue) {
        self.instance_buffer = buffer;
    }

    /// instance buffer 当前容量的只读副本。
    ///
    /// # Returns
    ///
    /// configure 时记下的画布尺寸。
    ///
    /// # Returns
    ///
    /// 记下 configure 时的画布尺寸。
    ///
    /// # Arguments
    ///
    /// 画布句柄。
    ///
    /// # Returns
    ///
    /// - `usize` - 容量(实例数)。
    pub fn get_instance_capacity(&self) -> usize {
        self.instance_capacity
    }

    /// 设置 instance buffer 容量。
    ///
    /// # Arguments
    ///
    /// - `usize` - 容量(实例数)。
    pub fn set_instance_capacity(&mut self, capacity: usize) {
        self.instance_capacity = capacity;
    }

    /// 深度纹理当前尺寸的只读副本。
    ///
    /// # Returns
    ///
    /// - `(u32, u32)` - 宽高。
    pub fn get_depth_size(&self) -> (u32, u32) {
        self.depth_size
    }

    /// 设置深度纹理尺寸。
    ///
    /// # Arguments
    ///
    /// - `(u32, u32)` - 宽高。
    pub fn set_depth_size(&mut self, size: (u32, u32)) {
        self.depth_size = size;
    }

    /// 深度纹理的只读引用。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - 纹理句柄。
    pub fn get_depth_texture(&self) -> &JsValue {
        &self.depth_texture
    }

    /// 设置深度纹理。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - 纹理句柄。
    pub fn set_depth_texture(&mut self, texture: JsValue) {
        self.depth_texture = texture;
    }

    /// GPU 侧资产表长度的只读副本(验收探针)。
    ///
    /// # Returns
    ///
    /// - `usize` - 资产条数。
    pub fn get_gpu_mesh_count(&self) -> usize {
        self.gpu_mesh_count
    }

    /// 设置 GPU 侧资产表长度(验收探针)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 资产条数。
    pub fn set_gpu_mesh_count(&mut self, count: usize) {
        self.gpu_mesh_count = count;
    }

    /// 阴影管线。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPURenderPipeline` 句柄。
    pub fn get_shadow_pipeline(&self) -> &JsValue {
        &self.shadow_pipeline
    }

    /// 设置阴影管线。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPURenderPipeline` 句柄。
    pub fn set_shadow_pipeline(&mut self, pipeline: JsValue) {
        self.shadow_pipeline = pipeline;
    }

    /// 设置阴影管线的 pipeline layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUPipelineLayout` 句柄。
    pub fn set_shadow_pipeline_layout(&mut self, layout: JsValue) {
        self.shadow_pipeline_layout = layout;
    }

    /// 阴影管线的 bind group layout。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn get_shadow_group_layout(&self) -> &JsValue {
        &self.shadow_group_layout
    }

    /// 设置阴影管线的 bind group layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn set_shadow_group_layout(&mut self, layout: JsValue) {
        self.shadow_group_layout = layout;
    }

    /// 阴影管线的 bind group(group 0 = 光源矩阵)。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroup` 句柄。
    pub fn get_shadow_bind_group(&self) -> &JsValue {
        &self.shadow_bind_group
    }

    /// 设置阴影管线的 bind group。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroup` 句柄。
    pub fn set_shadow_bind_group(&mut self, group: JsValue) {
        self.shadow_bind_group = group;
    }

    /// 光源视投影矩阵 uniform buffer。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBuffer` 句柄。
    pub fn get_shadow_frame_buffer(&self) -> &JsValue {
        &self.shadow_frame_buffer
    }

    /// 设置光源视投影矩阵 uniform buffer。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBuffer` 句柄。
    pub fn set_shadow_frame_buffer(&mut self, buffer: JsValue) {
        self.shadow_frame_buffer = buffer;
    }

    /// 阴影深度纹理。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUTexture` 句柄。
    pub fn get_shadow_texture(&self) -> &JsValue {
        &self.shadow_texture
    }

    /// 设置阴影深度纹理。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUTexture` 句柄。
    pub fn set_shadow_texture(&mut self, texture: JsValue) {
        self.shadow_texture = texture;
    }

    /// 主管线的 group 1 bind group。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroup` 句柄。
    pub fn get_shadow_sample_bind_group(&self) -> &JsValue {
        &self.shadow_sample_bind_group
    }

    /// 取阴影 pass 专用的 instance buffer。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBuffer` 句柄。
    pub fn get_shadow_instance_buffer(&self) -> &JsValue {
        &self.shadow_instance_buffer
    }

    /// 设置阴影 pass 专用的 instance buffer。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBuffer` 句柄。
    pub fn set_shadow_instance_buffer(&mut self, buffer: JsValue) {
        self.shadow_instance_buffer = buffer;
    }

    /// 取阴影 instance buffer 的容量。
    ///
    /// # Returns
    ///
    /// - `usize` - 实例数。
    pub fn get_shadow_instance_capacity(&self) -> usize {
        self.shadow_instance_capacity
    }

    /// 设置阴影 instance buffer 的容量。
    ///
    /// # Arguments
    ///
    /// - `usize` - 实例数。
    pub fn set_shadow_instance_capacity(&mut self, capacity: usize) {
        self.shadow_instance_capacity = capacity;
    }

    /// 设置主管线的 group 1 bind group。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroup` 句柄。
    pub fn set_shadow_sample_bind_group(&mut self, group: JsValue) {
        self.shadow_sample_bind_group = group;
    }

    /// group 1 的 bind group layout。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn get_shadow_sample_group_layout(&self) -> &JsValue {
        &self.shadow_sample_group_layout
    }

    /// 设置 group 1 的 bind group layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn set_shadow_sample_group_layout(&mut self, layout: JsValue) {
        self.shadow_sample_group_layout = layout;
    }

    /// 四条 bloom pipeline 各自的句柄(按值克隆一份)。
    ///
    /// # Returns
    ///
    /// - `[GpuBloomPass; BLOOM_PASS_COUNT]` - pipeline + bind group 配对。
    ///
    /// ⚠️ **按值返回副本**:`GpuBloomPass` 只 `Clone` 不 `Copy`
    /// (字段是 `JsValue`),而调用方往往在持有 `&mut self` 的同时要
    /// 读这张表 —— 例如 [`WebGpuRenderer::draw_bloom_chain`] 先克隆
    /// 再去改别的字段。
    pub fn get_bloom_passes(&self) -> [GpuBloomPass; BLOOM_PASS_COUNT] {
        self.bloom_passes.clone()
    }

    /// 设置四条 bloom pipeline 的句柄。
    ///
    /// # Arguments
    ///
    /// - `[GpuBloomPass; BLOOM_PASS_COUNT]` - pipeline + bind group 配对。
    pub fn set_bloom_passes(&mut self, passes: [GpuBloomPass; BLOOM_PASS_COUNT]) {
        self.bloom_passes = passes;
    }

    /// 四条 bloom pass 各自的 uniform buffer(只读副本)。
    ///
    /// # Returns
    ///
    /// - `[JsValue; BLOOM_PASS_COUNT]` - uniform buffer 句柄。
    pub fn get_bloom_buffers(&self) -> [JsValue; BLOOM_PASS_COUNT] {
        self.bloom_buffers.clone()
    }

    /// 设置四条 bloom pass 的 uniform buffer。
    ///
    /// # Arguments
    ///
    /// - `[JsValue; BLOOM_PASS_COUNT]` - uniform buffer 句柄。
    pub fn set_bloom_buffers(&mut self, buffers: [JsValue; BLOOM_PASS_COUNT]) {
        self.bloom_buffers = buffers;
    }

    /// bloom 颜色输入的普通采样器。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUSampler` 句柄。
    pub fn get_bloom_sampler(&self) -> &JsValue {
        &self.bloom_sampler
    }

    /// 设置 bloom 颜色输入的普通采样器。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUSampler` 句柄。
    pub fn set_bloom_sampler(&mut self, sampler: JsValue) {
        self.bloom_sampler = sampler;
    }

    /// bloom 提取 / 模糊的 bind group layout。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn get_bloom_group_layout(&self) -> &JsValue {
        &self.bloom_group_layout
    }

    /// 设置 bloom 提取 / 模糊的 bind group layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn set_bloom_group_layout(&mut self, layout: JsValue) {
        self.bloom_group_layout = layout;
    }

    /// bloom 合成的 bind group layout。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn get_bloom_composite_group_layout(&self) -> &JsValue {
        &self.bloom_composite_group_layout
    }

    /// 设置 bloom 合成的 bind group layout。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUBindGroupLayout` 句柄。
    pub fn set_bloom_composite_group_layout(&mut self, layout: JsValue) {
        self.bloom_composite_group_layout = layout;
    }

    /// 主场景离屏颜色目标。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUTexture` 句柄。
    pub fn get_bloom_scene(&self) -> &JsValue {
        &self.bloom_scene
    }

    /// 设置主场景离屏颜色目标。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUTexture` 句柄。
    pub fn set_bloom_scene(&mut self, texture: JsValue) {
        self.bloom_scene = texture;
    }

    /// 亮度提取目标。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUTexture` 句柄。
    pub fn get_bloom_bright(&self) -> &JsValue {
        &self.bloom_bright
    }

    /// 设置亮度提取目标。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUTexture` 句柄。
    pub fn set_bloom_bright(&mut self, texture: JsValue) {
        self.bloom_bright = texture;
    }

    /// 水平模糊目标(ping)。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUTexture` 句柄。
    pub fn get_bloom_ping(&self) -> &JsValue {
        &self.bloom_ping
    }

    /// 设置水平模糊目标(ping)。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUTexture` 句柄。
    pub fn set_bloom_ping(&mut self, texture: JsValue) {
        self.bloom_ping = texture;
    }

    /// 竖直模糊目标(pong)。
    ///
    /// # Returns
    ///
    /// - `&JsValue` - `GPUTexture` 句柄。
    pub fn get_bloom_pong(&self) -> &JsValue {
        &self.bloom_pong
    }

    /// 设置竖直模糊目标(pong)。
    ///
    /// # Arguments
    ///
    /// - `JsValue` - `GPUTexture` 句柄。
    pub fn set_bloom_pong(&mut self, texture: JsValue) {
        self.bloom_pong = texture;
    }

    /// 后处理目标当前尺寸(记的是画布尺寸)。
    ///
    /// # Returns
    ///
    /// - `(u32, u32)` - 宽高。
    pub fn get_bloom_size(&self) -> (u32, u32) {
        self.bloom_size
    }

    /// 设置后处理目标当前尺寸(记的是画布尺寸)。
    ///
    /// # Arguments
    ///
    /// - `(u32, u32)` - 宽高。
    pub fn set_bloom_size(&mut self, size: (u32, u32)) {
        self.bloom_size = size;
    }
}

/// Inherent implementation of [`GpuUnavailable`].
impl GpuUnavailable {
    /// 包一条人可读的失败原因。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 失败原因。
    ///
    /// # Returns
    ///
    /// - `GpuUnavailable` - 包好的错误值。
    pub fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}
