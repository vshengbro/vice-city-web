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
            meshes: Vec::new(),
            instance_buffer: JsValue::NULL,
            instance_capacity: 0,
            gpu_mesh_count: 0,
        };
        renderer.create_pipeline()?;
        renderer.create_uniform_buffers()?;
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
        // pipeline layout 必须**留到 bind group 创建时再用**,所以这里
        // 存进字段,而不是建完就丢 —— 否则 bind group 无从创建。
        let (layout, group_layout): (JsValue, JsValue) = self.build_pipeline_layout(&device)?;
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
        set(&primitive, FIELD_CULL_MODE, &JsValue::from_str(CULL_MODE_BACK))?;
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
        Ok(())
    }

    /// 建 pipeline layout:group 0 上两个 uniform binding。
    ///
    /// # Arguments
    ///
    /// - `&JsValue` - `GPUDevice`。
    ///
    /// # Returns
    ///
    /// - `Result<(JsValue, JsValue), String>` - `(GPUPipelineLayout, GPUBindGroupLayout)`。
    ///
    /// 两者都要留着:pipeline 用 `GPUPipelineLayout`,而
    /// `createBindGroup` 要的是里面的 `GPUBindGroupLayout`。拿前者去
    /// 建 bind group 会报
    /// `Failed to convert value to 'GPUBindGroupLayout'`。
    fn build_pipeline_layout(
        &self,
        device: &JsValue,
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
            set(&buffer_layout, FIELD_TYPE, &JsValue::from_str(BUFFER_TYPE_UNIFORM))?;
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
        let descriptor: Object = new_object();
        set(&descriptor, FIELD_BIND_GROUP_LAYOUTS, &layouts)?;
        let pipeline_layout: JsValue =
            call1(device, METHOD_CREATE_PIPELINE_LAYOUT, descriptor.as_ref())?;
        Ok((pipeline_layout, group_layout))
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

    /// 渲染一帧,返回本帧提交的三角形数。
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
        } = params;
        self.ensure_depth(width, height)?;
        let device: JsValue = self.get_device().clone();

        // ---- uniform ----

        let frame: [f32; 16] = frame_bytes(view_proj);
        write_buffer(
            &device,
            &self.get_frame_buffer().clone(),
            0.0,
            f32_slice_to_bytes(&frame),
        )?;
        let shading: [f32; 48] = flatten_shading(shading_from_lighting(lighting, eye));
        write_buffer(
            &device,
            &self.get_shading_buffer().clone(),
            0.0,
            f32_slice_to_bytes(&shading),
        )?;

        // ---- 画布纹理 ----
        let texture: JsValue = call0(&self.get_context().clone(), METHOD_GET_CURRENT_TEXTURE)?;
        let view: JsValue = create_view(&texture)?;

// ---- pass ----
        // `createCommandEncoder()` 是**零参数**方法。给它传一个
        // `null` 会让返回值不再是 GPUCommandEncoder,于是 submit 时报
        // `Failed to convert value to 'GPUCommandBuffer'`。
        let encoder: JsValue = call0(&device, METHOD_CREATE_COMMAND_ENCODER)?;
        let color_attachment: Object = new_object();
        set(&color_attachment, FIELD_VIEW, &view)?;
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
            return self.finish_frame(&device, &encoder, 0u32);
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
        write_buffer(
            &device,
            &instance_buffer,
            0.0,
            f32_slice_to_bytes(&all),
        )?;

        let mut triangles: u32 = 0;
        for (mesh_index, first, count) in ranges {
            triangles += self.draw_slice(&pass, mesh_index, first, count)?;
        }
        call0(&pass, METHOD_END)?;
        self.finish_frame(&device, &encoder, triangles)
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
