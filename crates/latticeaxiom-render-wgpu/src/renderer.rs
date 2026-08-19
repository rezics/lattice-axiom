//! Surface-bound wgpu renderer.

use bytemuck::{Pod, Zeroable};
use latticeaxiom_render::{
    FrameReport, MaterialData, MaterialId, MeshData, MeshId, RenderError, RenderWorld, Renderer,
};
use wgpu::util::DeviceExt as _;

/// Clear color for the sky until real sky rendering exists.
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.055,
    g: 0.078,
    b: 0.122,
    a: 1.0,
};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const INITIAL_INSTANCE_CAPACITY: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuInstance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
}

/// One tessellated egui frame waiting to be composited over the 3D scene.
///
/// The host owns input and widget construction through `egui-winit`; this
/// backend owns only the GPU resources and render-pass integration.
#[derive(Debug)]
pub struct UiFrame {
    /// Tessellated and clipped paint primitives.
    pub primitives: Vec<egui::ClippedPrimitive>,
    /// Texture uploads and frees produced by egui this frame.
    pub textures_delta: egui::TexturesDelta,
    /// Logical-to-physical scale used when tessellating the primitives.
    pub pixels_per_point: f32,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4
];

#[derive(Debug)]
struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

#[derive(Clone, Debug)]
struct DrawRun {
    mesh: MeshId,
    instances: std::ops::Range<u32>,
}

/// A [`Renderer`] drawing to a window surface through wgpu.
pub struct WgpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    meshes: Vec<GpuMesh>,
    materials: Vec<MaterialData>,
    ui_renderer: egui_wgpu::Renderer,
    pending_ui: Option<UiFrame>,
}

impl std::fmt::Debug for WgpuRenderer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WgpuRenderer")
            .field("config", &self.config)
            .field("instance_capacity", &self.instance_capacity)
            .field("mesh_count", &self.meshes.len())
            .field("material_count", &self.materials.len())
            .field("has_pending_ui", &self.pending_ui.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for WgpuRenderer {
    fn drop(&mut self) {
        if let Some(ui) = &mut self.pending_ui {
            // Dropping the renderer intentionally abandons an unpresented UI
            // frame. Clear egui's must-consume delta before its debug guard
            // runs; all corresponding GPU textures are being dropped too.
            ui.textures_delta.clear();
        }
    }
}

fn backend_error(context: &str, detail: impl std::fmt::Display) -> RenderError {
    RenderError::Backend {
        message: format!("{context}: {detail}"),
    }
}

fn to_u32(value: usize, what: &str) -> Result<u32, RenderError> {
    u32::try_from(value).map_err(|_| RenderError::Backend {
        message: format!("{what} exceeds u32 range"),
    })
}

fn gpu_vertices(mesh: &MeshData) -> Vec<GpuVertex> {
    mesh.positions
        .iter()
        .zip(&mesh.normals)
        .enumerate()
        .map(|(index, (&position, &normal))| GpuVertex {
            position,
            normal,
            color: mesh.colors.get(index).copied().unwrap_or([1.0; 4]),
        })
        .collect()
}

impl WgpuRenderer {
    /// Creates a renderer drawing to `target` (typically an `Arc<Window>`).
    ///
    /// Blocks on adapter and device acquisition; call it once during host
    /// startup.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Backend`] when no suitable adapter or device is
    /// available or the surface cannot be created and configured.
    pub fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(target)
            .map_err(|e| backend_error("failed to create surface", e))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| backend_error("no suitable GPU adapter", e))?;

        let adapter_info = adapter.get_info();
        tracing::info!(
            adapter = %adapter_info.name,
            backend = %adapter_info.backend,
            "creating wgpu renderer"
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("latticeaxiom-render-wgpu device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| backend_error("failed to acquire GPU device", e))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| backend_error("surface reports no texture formats", "empty list"))?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, &config);
        let (globals_buffer, globals_layout, globals_bind_group) = create_globals(&device);
        let pipeline = create_forward_pipeline(&device, config.format, &globals_layout);
        let instance_buffer = create_instance_buffer(&device, INITIAL_INSTANCE_CAPACITY);
        let ui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            pipeline,
            globals_buffer,
            globals_bind_group,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            meshes: Vec::new(),
            materials: Vec::new(),
            ui_renderer,
            pending_ui: None,
        })
    }

    /// Queues an egui frame to be drawn over the next submitted world.
    ///
    /// Calling this more than once before [`Renderer::submit`] replaces the
    /// older paint jobs while preserving every pending texture update; UI
    /// state remains owned by the host.
    pub fn queue_ui(&mut self, mut frame: UiFrame) {
        if let Some(mut pending) = self.pending_ui.take() {
            pending.textures_delta.append(frame.textures_delta);
            frame.textures_delta = pending.textures_delta;
        }
        self.pending_ui = Some(frame);
    }

    fn mesh_count(&self) -> u32 {
        u32::try_from(self.meshes.len()).unwrap_or(u32::MAX)
    }

    fn material_count(&self) -> u32 {
        u32::try_from(self.materials.len()).unwrap_or(u32::MAX)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "surface dimensions are far below f32 integer precision limits"
    )]
    fn aspect_ratio(&self) -> f32 {
        self.config.width as f32 / self.config.height.max(1) as f32
    }

    /// Builds the per-frame instance data, sorted by mesh so instanced draws
    /// stay contiguous. Returns the draw order (indices into
    /// `world.instances`).
    fn build_instance_data(&self, world: &RenderWorld) -> (Vec<usize>, Vec<GpuInstance>) {
        let mut order: Vec<usize> = (0..world.instances.len()).collect();
        order.sort_by_key(|&i| world.instances[i].mesh);
        let data = order
            .iter()
            .map(|&i| {
                let instance = &world.instances[i];
                let material = &self.materials[instance.material.to_raw() as usize];
                GpuInstance {
                    model: instance.transform.to_cols_array_2d(),
                    color: material.base_color,
                }
            })
            .collect();
        (order, data)
    }

    fn build_draw_runs(world: &RenderWorld, order: &[usize]) -> Result<Vec<DrawRun>, RenderError> {
        let mut runs = Vec::new();
        let mut run_start = 0;
        while run_start < order.len() {
            let mesh = world.instances[order[run_start]].mesh;
            let mut run_end = run_start + 1;
            while run_end < order.len() && world.instances[order[run_end]].mesh == mesh {
                run_end += 1;
            }
            runs.push(DrawRun {
                mesh,
                instances: to_u32(run_start, "instance range")?..to_u32(run_end, "instance range")?,
            });
            run_start = run_end;
        }
        Ok(runs)
    }

    fn ensure_instance_capacity(&mut self, required: usize) {
        if required > self.instance_capacity {
            let capacity = required.next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, capacity);
            self.instance_capacity = capacity;
        }
    }

    fn create_gpu_mesh(&self, mesh: &MeshData) -> Result<GpuMesh, RenderError> {
        mesh.validate()?;
        let index_count = to_u32(mesh.indices.len(), "index count")?;
        let vertices = gpu_vertices(mesh);

        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertex buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh index buffer"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

        Ok(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count,
        })
    }

    fn prepare_ui(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        ui: &UiFrame,
        screen: &egui_wgpu::ScreenDescriptor,
    ) -> Vec<wgpu::CommandBuffer> {
        for (id, deltas) in &ui.textures_delta.set {
            for delta in deltas {
                self.ui_renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        self.ui_renderer
            .update_buffers(&self.device, &self.queue, encoder, &ui.primitives, screen)
    }

    fn encode_world_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        draw_runs: &[DrawRun],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("forward pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));

        for run in draw_runs {
            let mesh = &self.meshes[run.mesh.to_raw() as usize];
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, run.instances.clone());
        }
    }

    fn encode_ui_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        ui: &UiFrame,
        screen: &egui_wgpu::ScreenDescriptor,
    ) {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui overlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.ui_renderer
            .render(&mut pass.forget_lifetime(), &ui.primitives, screen);
    }

    fn release_ui_textures(&mut self, ui: &mut UiFrame) {
        for id in &ui.textures_delta.free {
            self.ui_renderer.free_texture(id);
        }
        ui.textures_delta.clear();
    }

    /// Acquires the next surface texture; `Ok(None)` means "skip this frame"
    /// (window occluded or acquisition timed out), which is routine, not an
    /// error.
    fn acquire_frame(&mut self) -> Result<Option<wgpu::SurfaceTexture>, RenderError> {
        use wgpu::CurrentSurfaceTexture as Current;

        match self.surface.get_current_texture() {
            Current::Success(frame) | Current::Suboptimal(frame) => Ok(Some(frame)),
            Current::Timeout | Current::Occluded => Ok(None),
            Current::Outdated | Current::Lost => {
                // Surface contents were invalidated (resize race, driver
                // reset); reconfigure once and retry.
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    Current::Success(frame) | Current::Suboptimal(frame) => Ok(Some(frame)),
                    Current::Timeout | Current::Occluded => Ok(None),
                    Current::Outdated | Current::Lost => Err(backend_error(
                        "surface acquisition failed after reconfigure",
                        "surface still outdated or lost",
                    )),
                    Current::Validation => Err(backend_error(
                        "surface acquisition failed after reconfigure",
                        "validation error",
                    )),
                }
            }
            Current::Validation => Err(backend_error(
                "surface acquisition failed",
                "validation error",
            )),
        }
    }
}

impl Renderer for WgpuRenderer {
    fn upload_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, RenderError> {
        let id = MeshId::from_raw(to_u32(self.meshes.len(), "mesh table size")?);
        let gpu_mesh = self.create_gpu_mesh(mesh)?;
        self.meshes.push(gpu_mesh);
        Ok(id)
    }

    fn replace_mesh(&mut self, id: MeshId, mesh: &MeshData) -> Result<(), RenderError> {
        if id.to_raw() >= self.mesh_count() {
            return Err(RenderError::UnknownMesh(id));
        }
        let replacement = self.create_gpu_mesh(mesh)?;
        self.meshes[id.to_raw() as usize] = replacement;
        Ok(())
    }

    fn upload_material(&mut self, material: &MaterialData) -> Result<MaterialId, RenderError> {
        if !material.base_color.iter().all(|c| c.is_finite()) {
            return Err(RenderError::InvalidMaterial {
                reason: "base color contains non-finite components".to_owned(),
            });
        }
        let id = MaterialId::from_raw(to_u32(self.materials.len(), "material table size")?);
        self.materials.push(*material);
        Ok(id)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
    }

    fn submit(&mut self, world: &RenderWorld) -> Result<FrameReport, RenderError> {
        world.validate(self.mesh_count(), self.material_count())?;

        let globals = Globals {
            view_proj: world
                .camera
                .view_projection(self.aspect_ratio())
                .to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let (order, instance_data) = self.build_instance_data(world);
        let draw_runs = Self::build_draw_runs(world, &order)?;
        self.ensure_instance_capacity(instance_data.len());
        if !instance_data.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&instance_data),
            );
        }

        let Some(frame) = self.acquire_frame()? else {
            tracing::trace!("frame skipped: surface occluded or timed out");
            return Ok(FrameReport { instances_drawn: 0 });
        };
        let color_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        let mut pending_ui = self.pending_ui.take();
        let mut ui_command_buffers = Vec::new();
        let screen_descriptor = pending_ui.as_ref().map(|ui| egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ui.pixels_per_point,
        });
        if let (Some(ui), Some(screen)) = (&pending_ui, &screen_descriptor) {
            ui_command_buffers = self.prepare_ui(&mut encoder, ui, screen);
        }
        self.encode_world_pass(&mut encoder, &color_view, &draw_runs);

        if let (Some(ui), Some(screen)) = (&pending_ui, &screen_descriptor) {
            self.encode_ui_pass(&mut encoder, &color_view, ui, screen);
        }

        self.queue.submit(
            ui_command_buffers
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        self.queue.present(frame);
        if let Some(ui) = &mut pending_ui {
            self.release_ui_textures(ui);
        }

        Ok(FrameReport {
            instances_drawn: order.len(),
        })
    }
}

/// Creates the per-frame uniform buffer with its layout and bind group.
fn create_globals(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::BindGroupLayout, wgpu::BindGroup) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals bind group layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("globals uniform buffer"),
        size: std::mem::size_of::<Globals>() as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals bind group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });

    (buffer, layout, bind_group)
}

/// Builds the single forward pipeline used for flat-colored instanced meshes.
fn create_forward_pipeline(
    device: &wgpu::Device,
    color_format: wgpu::TextureFormat,
    globals_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("latticeaxiom forward shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("forward pipeline layout"),
        bind_group_layouts: &[Some(globals_layout)],
        immediate_size: 0,
    });

    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GpuVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    };
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GpuInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE_ATTRIBUTES,
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("forward pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout), Some(instance_layout)],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth buffer"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance buffer"),
        size: (capacity * std::mem::size_of::<GpuInstance>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use latticeaxiom_render::conformance::test_triangle;

    use super::gpu_vertices;

    #[test]
    fn omitted_vertex_colors_pack_as_white() {
        let vertices = gpu_vertices(&test_triangle());
        let expected = [1.0_f32; 4].map(f32::to_bits);
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.color.map(f32::to_bits) == expected)
        );
    }

    #[test]
    fn explicit_vertex_colors_are_preserved() {
        let mut mesh = test_triangle();
        mesh.colors = vec![[0.2, 0.4, 0.6, 0.8]; 3];

        let vertices = gpu_vertices(&mesh);
        let expected = [0.2_f32, 0.4, 0.6, 0.8].map(f32::to_bits);
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.color.map(f32::to_bits) == expected)
        );
    }
}
