//! wgpu renderer. Owns device/queue, terrain pipelines (opaque/cutout/
//! translucent), entity pipeline, per-section GPU buffers, atlas texture,
//! depth buffer, and the egui-wgpu painter. Works against a winit window
//! surface OR an offscreen texture (headless/lavapipe).
//!
//! Camera-relative rendering: vertex positions are section-relative; each draw
//! binds a dynamic uniform holding `section_origin - camera_pos` (f32, safe
//! because it's small) + the shared view-proj (built at camera origin).
//! Frustum culling per section AABB before recording draws. Translucent
//! sections sorted back→front by center distance.
//!
//! Shaders live in src/render/shaders/*.wgsl (terrain.wgsl, entity.wgsl).

pub mod camera;

use crate::assets::atlas::Atlas;
use crate::types::{MeshData, MeshVertex, RenderLayer, SectionPos};
use anyhow::{Context, Result, anyhow, bail};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use tracing::warn;
use wgpu::util::DeviceExt;

const TERRAIN_WGSL: &str = include_str!("shaders/terrain.wgsl");
const ENTITY_WGSL: &str = include_str!("shaders/entity.wgsl");

/// Near plane matches camera::view_proj.
const ZNEAR_SLACK: f32 = 128.0;
/// Section AABBs are padded by this much (models may poke past 0..16).
const AABB_PAD: f32 = 1.0;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub enum RenderTarget {
    Window(Arc<winit::window::Window>),
    Offscreen { width: u32, height: u32 },
}

/// Everything the renderer needs for one frame.
pub struct SceneParams {
    /// Eye position, world space (f64 — renderer subtracts internally).
    pub cam_pos: [f64; 3],
    /// Vanilla degrees.
    pub yaw: f32,
    pub pitch: f32,
    pub fov_deg: f32,
    /// 0..1 (from world time; 1 = noon).
    pub daylight: f32,
    /// Fog: linear from start to end (blocks).
    pub fog_start: f32,
    pub fog_end: f32,
    pub sky_color: [f32; 3],
}

pub struct EntityDraw {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub kind: EntityDrawKind,
    /// Simple flat color for boxes (players use skin-ish blue, items yellow...).
    pub color: [f32; 3],
}

pub enum EntityDrawKind {
    /// Player-shaped: head+body+limbs boxes, ~1.8 blocks tall, flat colored v1.
    Humanoid,
    /// Axis-aligned box centered at pos, `h` tall, `w` wide.
    Box { w: f32, h: f32 },
}

/// egui output ready for the painter (app owns the egui Context).
pub struct EguiFrame {
    pub textures_delta: egui::TexturesDelta,
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub pixels_per_point: f32,
}

pub struct FrameStats {
    pub sections_drawn: usize,
    pub sections_total: usize,
    pub draw_calls: usize,
}

// ---------------------------------------------------------------------------
// GPU-side plumbing
// ---------------------------------------------------------------------------

/// Bind group 0 contents; must match `Globals` in both WGSL files (96 B).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlobalsUniform {
    view_proj: [[f32; 4]; 4],
    fog_start: f32,
    fog_end: f32,
    daylight: f32,
    mode: f32,
    sky_color: [f32; 3],
    _pad: f32,
}

/// Per-draw terrain slot (16 B): xyz = section_origin - cam_pos.
const SECTION_SLOT_SIZE: u64 = 16;
/// Per-draw entity slot (80 B): mat4 model + vec4 color.
const ENTITY_SLOT_SIZE: u64 = 80;

const VERTEX_ATTRS: [wgpu::VertexAttribute; 4] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 12, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 20, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 24, shader_location: 3 },
];

struct LayerGpu {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct SectionGpu {
    layers: [Option<LayerGpu>; 3],
}

/// A growable uniform buffer bound with a dynamic offset: one fixed-size slot
/// per draw, slots strided to the device's min uniform offset alignment.
struct DynUniform {
    layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    slot_size: u64,
    stride: u32,
    capacity: u32,
    staging: Vec<u8>,
}

impl DynUniform {
    fn new(device: &wgpu::Device, slot_size: u64, initial_slots: u32, label: &str) -> Self {
        let align = device.limits().min_uniform_buffer_offset_alignment.max(16) as u64;
        let stride = slot_size.next_multiple_of(align) as u32;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(slot_size),
                },
                count: None,
            }],
        });
        let capacity = initial_slots.max(1);
        let (buffer, bind_group) =
            Self::make_buffer(device, &layout, slot_size, stride, capacity, label);
        Self { layout, buffer, bind_group, slot_size, stride, capacity, staging: Vec::new() }
    }

    fn make_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slot_size: u64,
        stride: u32,
        slots: u32,
        label: &str,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: stride as u64 * slots as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: NonZeroU64::new(slot_size),
                }),
            }],
        });
        (buffer, bind_group)
    }

    /// Make sure at least `slots` slots exist; clears + sizes the staging area.
    fn begin_frame(&mut self, device: &wgpu::Device, slots: u32) {
        if slots > self.capacity {
            let new_cap = slots.next_power_of_two();
            let (buffer, bind_group) = Self::make_buffer(
                device,
                &self.layout,
                self.slot_size,
                self.stride,
                new_cap,
                "dyn-uniform (grown)",
            );
            self.buffer = buffer;
            self.bind_group = bind_group;
            self.capacity = new_cap;
        }
        self.staging.clear();
        self.staging.resize(self.stride as usize * slots as usize, 0);
    }

    /// Write one slot's payload into the staging area.
    fn write_slot(&mut self, slot: u32, data: &[u8]) {
        debug_assert!(data.len() as u64 <= self.slot_size);
        let start = slot as usize * self.stride as usize;
        self.staging[start..start + data.len()].copy_from_slice(data);
    }

    fn upload(&self, queue: &wgpu::Queue) {
        if !self.staging.is_empty() {
            queue.write_buffer(&self.buffer, 0, &self.staging);
        }
    }

    fn offset_of(&self, slot: u32) -> u32 {
        slot * self.stride
    }
}

enum Target {
    Window { surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration },
    Offscreen { color: wgpu::Texture, view: wgpu::TextureView },
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,
    color_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    depth_view: wgpu::TextureView,

    pipe_opaque: wgpu::RenderPipeline,
    pipe_cutout: wgpu::RenderPipeline,
    pipe_translucent: wgpu::RenderPipeline,
    pipe_entity: wgpu::RenderPipeline,

    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    atlas_layout: wgpu::BindGroupLayout,
    atlas_sampler: wgpu::Sampler,
    atlas_bg: wgpu::BindGroup,
    section_uniform: DynUniform,
    entity_uniform: DynUniform,

    cube_vbuf: wgpu::Buffer,
    meshes: HashMap<SectionPos, SectionGpu>,
    egui_renderer: egui_wgpu::Renderer,
}

impl Renderer {
    /// Instance → adapter (any backend incl. software lavapipe) → device.
    /// Window target: create+configure surface (FIFO). Offscreen: RGBA8 texture.
    pub fn new(target: RenderTarget) -> Result<Self> {
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(instance_desc);

        // Surface first (adapter must be compatible with it).
        let (surface, width, height) = match &target {
            RenderTarget::Window(window) => {
                let size = window.inner_size();
                let surface = instance
                    .create_surface(window.clone())
                    .context("creating wgpu surface for window")?;
                (Some(surface), size.width.max(1), size.height.max(1))
            }
            RenderTarget::Offscreen { width, height } => (None, (*width).max(1), (*height).max(1)),
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: surface.as_ref(),
        }))
        .map_err(|e| anyhow!("no compatible GPU adapter found: {e}"))?;
        let info = adapter.get_info();
        tracing::info!(
            "wgpu adapter: {} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("dolphin-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|e| anyhow!("requesting wgpu device: {e}"))?;

        // Target: configure surface / create offscreen color texture.
        let (target, color_format) = match surface {
            Some(surface) => {
                let caps = surface.get_capabilities(&adapter);
                let format = caps
                    .formats
                    .iter()
                    .copied()
                    .find(|f| f.is_srgb())
                    .or_else(|| caps.formats.first().copied())
                    .context("surface reports no supported formats")?;
                let alpha_mode = caps
                    .alpha_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::CompositeAlphaMode::Auto);
                let config = wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format,
                    width,
                    height,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode,
                    view_formats: vec![],
                };
                surface.configure(&device, &config);
                (Target::Window { surface, config }, format)
            }
            None => {
                let format = wgpu::TextureFormat::Rgba8UnormSrgb;
                let (color, view) = create_offscreen_color(&device, width, height, format);
                (Target::Offscreen { color, view }, format)
            }
        };

        let depth_view = create_depth(&device, width, height);

        // --- bind group layouts + shared resources ---------------------------
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<GlobalsUniform>() as u64),
                },
                count: None,
            }],
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<GlobalsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals-bg"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        // 1x1 white placeholder so frames render before set_atlas().
        let atlas_bg = make_atlas_bind_group(
            &device,
            &queue,
            &atlas_layout,
            &atlas_sampler,
            1,
            1,
            &[255, 255, 255, 255],
        );

        let section_uniform = DynUniform::new(&device, SECTION_SLOT_SIZE, 1024, "section-uniform");
        let entity_uniform = DynUniform::new(&device, ENTITY_SLOT_SIZE, 64, "entity-uniform");

        // --- pipelines --------------------------------------------------------
        let terrain_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain.wgsl"),
            source: wgpu::ShaderSource::Wgsl(TERRAIN_WGSL.into()),
        });
        let entity_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("entity.wgsl"),
            source: wgpu::ShaderSource::Wgsl(ENTITY_WGSL.into()),
        });

        let terrain_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain-pl"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&atlas_layout),
                Some(&section_uniform.layout),
            ],
            immediate_size: 0,
        });
        let entity_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("entity-pl"),
            bind_group_layouts: &[Some(&globals_layout), Some(&entity_uniform.layout)],
            immediate_size: 0,
        });

        let terrain_vbl = wgpu::VertexBufferLayout {
            array_stride: size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRS,
        };

        let make_terrain_pipeline = |label: &str,
                                     fs_entry: &str,
                                     blend: Option<wgpu::BlendState>,
                                     depth_write: bool,
                                     cull: Option<wgpu::Face>|
         -> wgpu::RenderPipeline {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&terrain_pl),
                vertex: wgpu::VertexState {
                    module: &terrain_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: std::slice::from_ref(&terrain_vbl),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &terrain_shader,
                    entry_point: Some(fs_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let pipe_opaque =
            make_terrain_pipeline("terrain-opaque", "fs_main", None, true, Some(wgpu::Face::Back));
        // Cutout: crossed plants etc. need both sides visible.
        let pipe_cutout = make_terrain_pipeline("terrain-cutout", "fs_cutout", None, true, None);
        let pipe_translucent = make_terrain_pipeline(
            "terrain-translucent",
            "fs_main",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            None,
        );

        let pipe_entity = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity"),
            layout: Some(&entity_pl),
            vertex: wgpu::VertexState {
                module: &entity_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &entity_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let cube_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("unit-cube"),
            contents: bytemuck::cast_slice(&unit_cube_vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let egui_renderer =
            egui_wgpu::Renderer::new(&device, color_format, egui_wgpu::RendererOptions::default());

        Ok(Self {
            device,
            queue,
            target,
            color_format,
            width,
            height,
            depth_view,
            pipe_opaque,
            pipe_cutout,
            pipe_translucent,
            pipe_entity,
            globals_buf,
            globals_bg,
            atlas_layout,
            atlas_sampler,
            atlas_bg,
            section_uniform,
            entity_uniform,
            cube_vbuf,
            meshes: HashMap::new(),
            egui_renderer,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        match &mut self.target {
            Target::Window { surface, config } => {
                config.width = width;
                config.height = height;
                surface.configure(&self.device, config);
            }
            Target::Offscreen { color, view } => {
                let (c, v) = create_offscreen_color(&self.device, width, height, self.color_format);
                *color = c;
                *view = v;
            }
        }
        self.depth_view = create_depth(&self.device, width, height);
    }

    /// Upload the atlas texture (call once before the first frame).
    pub fn set_atlas(&mut self, atlas: &Atlas) {
        let (w, h) = (atlas.image.width(), atlas.image.height());
        if w == 0 || h == 0 {
            warn!("set_atlas called with empty atlas image; keeping placeholder");
            return;
        }
        self.atlas_bg = make_atlas_bind_group(
            &self.device,
            &self.queue,
            &self.atlas_layout,
            &self.atlas_sampler,
            w,
            h,
            atlas.image.as_raw(),
        );
    }

    /// Create/replace GPU buffers for a section. Empty meshes remove the entry.
    pub fn upload_mesh(&mut self, mesh: MeshData) {
        if mesh.is_empty() {
            self.meshes.remove(&mesh.pos);
            return;
        }
        let mut layers: [Option<LayerGpu>; 3] = [None, None, None];
        for layer in RenderLayer::ALL {
            let lm = &mesh[layer];
            if lm.is_empty() {
                continue;
            }
            let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("section-vertices"),
                contents: bytemuck::cast_slice(&lm.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("section-indices"),
                contents: bytemuck::cast_slice(&lm.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            layers[layer as usize] =
                Some(LayerGpu { vertices, indices, index_count: lm.indices.len() as u32 });
        }
        self.meshes.insert(mesh.pos, SectionGpu { layers });
    }

    pub fn remove_mesh(&mut self, pos: SectionPos) {
        self.meshes.remove(&pos);
    }

    /// Render one frame. `egui` may be None (offscreen mode).
    pub fn frame(
        &mut self,
        scene: &SceneParams,
        entities: &[EntityDraw],
        egui: Option<EguiFrame>,
    ) -> Result<FrameStats> {
        let sections_total = self.meshes.len();
        let empty_stats =
            || FrameStats { sections_drawn: 0, sections_total, draw_calls: 0 };

        // Acquire the color target first: for window targets this can tell us
        // to skip/reconfigure without doing any work.
        let (color_view, surface_texture) = match &self.target {
            Target::Window { surface, config } => {
                use wgpu::CurrentSurfaceTexture::*;
                match surface.get_current_texture() {
                    Success(t) | Suboptimal(t) => {
                        let view = t.texture.create_view(&Default::default());
                        (view, Some(t))
                    }
                    Lost | Outdated => {
                        surface.configure(&self.device, config);
                        return Ok(empty_stats());
                    }
                    Timeout | Occluded => return Ok(empty_stats()),
                    Validation => bail!("surface texture acquisition failed validation"),
                }
            }
            Target::Offscreen { view, .. } => (view.clone(), None),
        };

        // --- camera / globals -------------------------------------------------
        let aspect = self.width as f32 / self.height as f32;
        let zfar = (scene.fog_end + ZNEAR_SLACK).max(64.0);
        let vp = camera::view_proj(scene.yaw, scene.pitch, scene.fov_deg, aspect, zfar);
        let frustum = camera::Frustum::from_view_proj(&vp);
        let globals = GlobalsUniform {
            view_proj: vp.to_cols_array_2d(),
            fog_start: scene.fog_start,
            fog_end: scene.fog_end,
            daylight: scene.daylight.clamp(0.0, 1.0),
            mode: 0.0,
            sky_color: scene.sky_color,
            _pad: 0.0,
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));

        // --- frustum cull, camera-relative offsets -----------------------------
        struct Visible {
            pos: SectionPos,
            offset: Vec3,
            dist2: f32,
            slot: u32,
        }
        let mut visible: Vec<Visible> = Vec::new();
        for pos in self.meshes.keys() {
            let o = pos.origin();
            let offset = Vec3::new(
                (o[0] - scene.cam_pos[0]) as f32,
                (o[1] - scene.cam_pos[1]) as f32,
                (o[2] - scene.cam_pos[2]) as f32,
            );
            let min = offset - Vec3::splat(AABB_PAD);
            let max = offset + Vec3::splat(16.0 + AABB_PAD);
            if !frustum.intersects_aabb(min, max) {
                continue;
            }
            let center = offset + Vec3::splat(8.0);
            visible.push(Visible { pos: *pos, offset, dist2: center.length_squared(), slot: 0 });
        }
        // Front-to-back for opaque early-z; translucent iterates in reverse.
        visible.sort_by(|a, b| a.dist2.total_cmp(&b.dist2));

        self.section_uniform.begin_frame(&self.device, visible.len() as u32);
        for (i, v) in visible.iter_mut().enumerate() {
            v.slot = i as u32;
            let data: [f32; 4] = [v.offset.x, v.offset.y, v.offset.z, 0.0];
            self.section_uniform.write_slot(v.slot, bytemuck::cast_slice(&data));
        }
        self.section_uniform.upload(&self.queue);

        // --- entity boxes -------------------------------------------------------
        // Each box is one dynamic-uniform slot: model matrix (camera-relative)
        // + flat color.
        let mut boxes: Vec<[u8; 80]> = Vec::new();
        for e in entities {
            let base = Vec3::new(
                (e.pos[0] - scene.cam_pos[0]) as f32,
                (e.pos[1] - scene.cam_pos[1]) as f32,
                (e.pos[2] - scene.cam_pos[2]) as f32,
            );
            let color = [e.color[0], e.color[1], e.color[2], 1.0f32];
            let mut push = |model: Mat4| {
                let mut bytes = [0u8; 80];
                bytes[..64].copy_from_slice(bytemuck::cast_slice(&model.to_cols_array()));
                bytes[64..].copy_from_slice(bytemuck::cast_slice(&color));
                boxes.push(bytes);
            };
            match e.kind {
                EntityDrawKind::Humanoid => {
                    let rot = Mat4::from_translation(base)
                        * Mat4::from_rotation_y(-e.yaw.to_radians());
                    // (center xyz, size xyz) in blocks; 16 px = 1 block.
                    const P: f32 = 1.0 / 16.0;
                    let parts: [([f32; 3], [f32; 3]); 6] = [
                        ([0.0, 28.0 * P, 0.0], [8.0 * P, 8.0 * P, 8.0 * P]), // head
                        ([0.0, 18.0 * P, 0.0], [8.0 * P, 12.0 * P, 4.0 * P]), // torso
                        ([-6.0 * P, 18.0 * P, 0.0], [4.0 * P, 12.0 * P, 4.0 * P]), // arm L
                        ([6.0 * P, 18.0 * P, 0.0], [4.0 * P, 12.0 * P, 4.0 * P]), // arm R
                        ([-2.0 * P, 6.0 * P, 0.0], [4.0 * P, 12.0 * P, 4.0 * P]), // leg L
                        ([2.0 * P, 6.0 * P, 0.0], [4.0 * P, 12.0 * P, 4.0 * P]), // leg R
                    ];
                    for (center, size) in parts {
                        push(
                            rot * Mat4::from_translation(Vec3::from(center))
                                * Mat4::from_scale(Vec3::from(size)),
                        );
                    }
                }
                EntityDrawKind::Box { w, h } => {
                    push(Mat4::from_translation(base) * Mat4::from_scale(Vec3::new(w, h, w)));
                }
            }
        }
        self.entity_uniform.begin_frame(&self.device, boxes.len() as u32);
        for (i, b) in boxes.iter().enumerate() {
            self.entity_uniform.write_slot(i as u32, b);
        }
        self.entity_uniform.upload(&self.queue);

        // --- record ------------------------------------------------------------
        let mut draw_calls = 0usize;
        let mut sections_drawn = 0usize;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: scene.sky_color[0] as f64,
                            g: scene.sky_color[1] as f64,
                            b: scene.sky_color[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);

            // Opaque + cutout, front to back.
            for (pipeline, layer) in [
                (&self.pipe_opaque, RenderLayer::Opaque),
                (&self.pipe_cutout, RenderLayer::Cutout),
            ] {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, &self.atlas_bg, &[]);
                for v in &visible {
                    let Some(gpu) = self.meshes.get(&v.pos) else { continue };
                    let Some(lg) = &gpu.layers[layer as usize] else { continue };
                    pass.set_bind_group(
                        2,
                        &self.section_uniform.bind_group,
                        &[self.section_uniform.offset_of(v.slot)],
                    );
                    pass.set_vertex_buffer(0, lg.vertices.slice(..));
                    pass.set_index_buffer(lg.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..lg.index_count, 0, 0..1);
                    draw_calls += 1;
                    if layer == RenderLayer::Opaque {
                        sections_drawn += 1;
                    }
                }
            }

            // Entities (solid boxes).
            if !boxes.is_empty() {
                pass.set_pipeline(&self.pipe_entity);
                pass.set_vertex_buffer(0, self.cube_vbuf.slice(..));
                for i in 0..boxes.len() as u32 {
                    pass.set_bind_group(
                        1,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i)],
                    );
                    pass.draw(0..36, 0..1);
                    draw_calls += 1;
                }
            }

            // Translucent, back to front, depth write off.
            pass.set_pipeline(&self.pipe_translucent);
            pass.set_bind_group(1, &self.atlas_bg, &[]);
            for v in visible.iter().rev() {
                let Some(gpu) = self.meshes.get(&v.pos) else { continue };
                let Some(lg) = &gpu.layers[RenderLayer::Translucent as usize] else { continue };
                pass.set_bind_group(
                    2,
                    &self.section_uniform.bind_group,
                    &[self.section_uniform.offset_of(v.slot)],
                );
                pass.set_vertex_buffer(0, lg.vertices.slice(..));
                pass.set_index_buffer(lg.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..lg.index_count, 0, 0..1);
                draw_calls += 1;
            }
        }

        // --- egui overlay -------------------------------------------------------
        let mut user_cmd_bufs = Vec::new();
        let egui_free = egui.as_ref().map(|f| f.textures_delta.free.clone());
        if let Some(frame) = &egui {
            for (id, delta) in &frame.textures_delta.set {
                self.egui_renderer.update_texture(&self.device, &self.queue, *id, delta);
            }
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [self.width, self.height],
                pixels_per_point: frame.pixels_per_point,
            };
            user_cmd_bufs = self.egui_renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut encoder,
                &frame.primitives,
                &screen,
            );
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
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
            let mut pass = pass.forget_lifetime();
            self.egui_renderer.render(&mut pass, &frame.primitives, &screen);
            draw_calls += 1;
        }

        self.queue
            .submit(user_cmd_bufs.into_iter().chain(std::iter::once(encoder.finish())));

        if let Some(free) = egui_free {
            for id in &free {
                self.egui_renderer.free_texture(id);
            }
        }

        if let Some(t) = surface_texture {
            t.present();
        }

        Ok(FrameStats { sections_drawn, sections_total, draw_calls })
    }

    /// Offscreen only: copy the last frame to CPU. (Window targets error.)
    pub fn read_screenshot(&mut self) -> Result<image::RgbaImage> {
        let Target::Offscreen { color, .. } = &self.target else {
            bail!("read_screenshot is only supported for offscreen render targets");
        };
        let (w, h) = (self.width, self.height);
        let bytes_per_row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot-readback"),
            size: bytes_per_row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("screenshot") });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| anyhow!("waiting for screenshot readback: {e}"))?;
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .context("screenshot map callback never fired")?
            .map_err(|e| anyhow!("mapping screenshot buffer: {e}"))?;

        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        {
            let data = buffer.slice(..).get_mapped_range();
            for row in 0..h {
                let start = (row * bytes_per_row) as usize;
                pixels.extend_from_slice(&data[start..start + (w * 4) as usize]);
            }
        }
        buffer.unmap();
        image::RgbaImage::from_raw(w, h, pixels)
            .context("assembling screenshot image (size mismatch)")
    }

    /// For egui-winit integration the app needs the device/queue pixels-per-point
    /// free — expose the wgpu handles the hud painter setup needs.
    pub fn egui_render_state(&self) -> (&wgpu::Device, &wgpu::Queue, wgpu::TextureFormat) {
        (&self.device, &self.queue, self.color_format)
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&Default::default())
}

fn create_offscreen_color(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen-color"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    (tex, view)
}

fn make_atlas_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::BindGroup {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        tex.as_image_copy(),
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: None,
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    let view = tex.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("atlas-bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

/// 36 vertices (12 triangles), unit cube centered at origin, CCW from outside.
fn unit_cube_vertices() -> [[f32; 3]; 36] {
    const H: f32 = 0.5;
    // Each face: 4 corners CCW viewed from outside.
    let faces: [[[f32; 3]; 4]; 6] = [
        // +X
        [[H, -H, -H], [H, H, -H], [H, H, H], [H, -H, H]],
        // -X
        [[-H, -H, H], [-H, H, H], [-H, H, -H], [-H, -H, -H]],
        // +Y
        [[-H, H, -H], [-H, H, H], [H, H, H], [H, H, -H]],
        // -Y
        [[-H, -H, H], [-H, -H, -H], [H, -H, -H], [H, -H, H]],
        // +Z
        [[-H, -H, H], [H, -H, H], [H, H, H], [-H, H, H]],
        // -Z
        [[H, -H, -H], [-H, -H, -H], [-H, H, -H], [H, H, -H]],
    ];
    let mut out = [[0.0f32; 3]; 36];
    let mut i = 0;
    for f in faces {
        for idx in [0usize, 1, 2, 0, 2, 3] {
            out[i] = f[idx];
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn mesh_vertex_layout_matches_pipeline() {
        assert_eq!(size_of::<MeshVertex>(), 28);
        assert_eq!(offset_of!(MeshVertex, pos), VERTEX_ATTRS[0].offset as usize);
        assert_eq!(offset_of!(MeshVertex, uv), VERTEX_ATTRS[1].offset as usize);
        assert_eq!(offset_of!(MeshVertex, color), VERTEX_ATTRS[2].offset as usize);
        assert_eq!(offset_of!(MeshVertex, light), VERTEX_ATTRS[3].offset as usize);
        assert_eq!(VERTEX_ATTRS[0].format, wgpu::VertexFormat::Float32x3);
        assert_eq!(VERTEX_ATTRS[1].format, wgpu::VertexFormat::Float32x2);
        assert_eq!(VERTEX_ATTRS[2].format, wgpu::VertexFormat::Unorm8x4);
        assert_eq!(VERTEX_ATTRS[3].format, wgpu::VertexFormat::Unorm8x4);
    }

    #[test]
    fn globals_uniform_size() {
        // Must match the WGSL `Globals` struct layout (96 bytes).
        assert_eq!(size_of::<GlobalsUniform>(), 96);
    }

    fn srgb_encode(l: f32) -> u8 {
        let v = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    #[test]
    fn offscreen_smoke() {
        let mut r = match Renderer::new(RenderTarget::Offscreen { width: 64, height: 64 }) {
            Ok(r) => r,
            Err(e) => {
                // No adapter at all (not even lavapipe): don't fail the suite.
                eprintln!("offscreen_smoke skipped: {e:#}");
                return;
            }
        };
        let scene = SceneParams {
            cam_pos: [0.0, 80.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 96.0,
            fog_end: 128.0,
            sky_color: [0.5, 0.7, 1.0],
        };
        let stats = r.frame(&scene, &[], None).expect("frame");
        assert_eq!(stats.sections_total, 0);
        let img = r.read_screenshot().expect("screenshot");
        assert_eq!((img.width(), img.height()), (64, 64));
        let px = img.get_pixel(0, 0);
        let expected = [srgb_encode(0.5), srgb_encode(0.7), srgb_encode(1.0)];
        for (i, e) in expected.iter().enumerate() {
            let d = (px[i] as i32 - *e as i32).unsigned_abs();
            assert!(
                d <= 20,
                "channel {i}: got {} expected ~{e} (pixel {:?})",
                px[i],
                px
            );
        }
        assert_eq!(px[3], 255);
    }
}
