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
pub mod entity_models;

pub use entity_models::MobModel;
use entity_models::PartAnim;

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
const SKIN_WGSL: &str = include_str!("shaders/skin.wgsl");
const PANORAMA_WGSL: &str = include_str!("shaders/panorama.wgsl");

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
    /// Draw the title-screen panorama behind everything (menu only; needs
    /// `set_panorama` to have been called).
    pub panorama: bool,
}

pub struct EntityDraw {
    pub pos: [f64; 3],
    /// Body yaw, vanilla degrees.
    pub yaw: f32,
    pub kind: EntityDrawKind,
    /// RGB multiply applied to the whole model — [1,1,1] = untinted, a reddish
    /// tint flashes a hurt entity (vanilla damage animation).
    pub tint: [f32; 3],
}

pub enum EntityDrawKind {
    /// A skinned player model. `skin` is a key registered via `ensure_skin`
    /// (0 = default Steve); falls back to a blue box if no skin is loaded.
    Player {
        skin: u64,
        slim: bool,
        /// Current limb swing angle in radians (0 = standing).
        swing: f32,
        /// Head pitch, vanilla degrees (positive = looking down).
        head_pitch: f32,
        /// Armor material worn in each slot: [head, chest, legs, feet]. A slot
        /// is `None` when empty or holding a non-armor item. Rendered as an
        /// inflated layer over the model using the material's equipment texture.
        armor: [Option<ArmorMaterial>; 4],
        /// Main-hand item's item-atlas UV rect `[u0,v0,u1,v1]`, drawn as a flat
        /// sprite in the right hand. `None` = empty hand or icon unavailable.
        main_hand: Option<[f32; 4]>,
        /// Off-hand item's item-atlas UV rect, drawn in the left hand.
        off_hand: Option<[f32; 4]>,
    },
    /// Axis-aligned box, `h` tall, `w` wide, flat colored. Centered on pos in
    /// x/z, extends up from pos.y (matches EntitySnapshot's hitbox convention).
    Box { w: f32, h: f32, color: [f32; 3] },
    /// A dropped-item sprite: the item-atlas rect `[u0,v0,u1,v1]` on a two-sided
    /// cross of quads, spun around Y by `EntityDraw::yaw` and floating above the
    /// ground. Falls back to nothing if the item atlas isn't loaded.
    Item { uv: [f32; 4] },
    /// A non-humanoid mob rendered from a prebuilt cuboid model (creeper, pig,
    /// cow, …) using its real entity texture. `tex` is a key registered via
    /// `ensure_skin`; falls back to a grey box if the texture isn't loaded.
    Mob {
        tex: u64,
        model: MobModel,
        /// Limb swing angle in radians (0 = standing).
        swing: f32,
        /// Head pitch, vanilla degrees (positive = looking down).
        head_pitch: f32,
    },
}

/// Armor tier, mapped to the vanilla `entity/equipment/humanoid[_leggings]`
/// textures. The app derives it from the equipped item's registry name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ArmorMaterial {
    Leather,
    Chainmail,
    Iron,
    Gold,
    Diamond,
    Netherite,
    Copper,
    Turtle,
}

impl ArmorMaterial {
    /// Stable small id used as the texture-map key.
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Texture base name under `entity/equipment/humanoid[_leggings]/`.
    pub fn tex_name(self) -> &'static str {
        match self {
            ArmorMaterial::Leather => "leather",
            ArmorMaterial::Chainmail => "chainmail",
            ArmorMaterial::Iron => "iron",
            ArmorMaterial::Gold => "gold",
            ArmorMaterial::Diamond => "diamond",
            ArmorMaterial::Netherite => "netherite",
            ArmorMaterial::Copper => "copper",
            // Turtle shell is a helmet only; its texture lives in humanoid/.
            ArmorMaterial::Turtle => "turtle_scute",
        }
    }

    /// All materials, for pre-loading textures at startup.
    pub fn all() -> [ArmorMaterial; 8] {
        use ArmorMaterial::*;
        [Leather, Chainmail, Iron, Gold, Diamond, Netherite, Copper, Turtle]
    }
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

/// Position + UV vertex, used by the skin and panorama pipelines.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TexVertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

const TEX_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 12, shader_location: 1 },
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

// --- player skin model -------------------------------------------------------

/// Vanilla player render scale: the 32px model is drawn at 0.9375, ≈1.875
/// blocks tall. Converts skin pixels to blocks.
const SKIN_PX: f32 = 0.9375 / 16.0;

const PART_HEAD: usize = 0;
const PART_BODY: usize = 1;
const PART_RIGHT_ARM: usize = 2;
const PART_LEFT_ARM: usize = 3;
const PART_RIGHT_LEG: usize = 4;
const PART_LEFT_LEG: usize = 5;

/// Pre-built vertex buffer for one player model variant (wide or slim).
/// Each part is a contiguous vertex range, positioned relative to its pivot.
struct SkinMesh {
    vbuf: wgpu::Buffer,
    /// (first_vertex, vertex_count) per part.
    parts: [(u32, u32); 6],
    /// Pivot per part, in blocks, relative to the entity's feet position.
    pivots: [Vec3; 6],
}

/// Append one skin cuboid: `center`/`size` in skin pixels relative to the part
/// pivot, `uv` = top-left of the box's UV patch on the 64x64 skin, `inflate`
/// grows the geometry (overlay layers) without changing the UV mapping.
/// Face strip order in the skin format: right(-x), front(+z), left(+x),
/// back(-z); top/bottom above at (u0+d, v0) / (u0+d+w, v0).
fn skin_box(out: &mut Vec<TexVertex>, center: [f32; 3], size: [f32; 3], uv: [f32; 2], inflate: f32) {
    const TEX: f32 = 64.0;
    let (w, h, d) = (size[0], size[1], size[2]);
    let hx = (w / 2.0 + inflate) * SKIN_PX;
    let hy = (h / 2.0 + inflate) * SKIN_PX;
    let hz = (d / 2.0 + inflate) * SKIN_PX;
    let c = [center[0] * SKIN_PX, center[1] * SKIN_PX, center[2] * SKIN_PX];
    let (u0, v0) = (uv[0], uv[1]);

    // One quad = 2 triangles from 4 (pos, uv-px) corners, CCW from outside.
    let mut quad = |p: [([f32; 3], [f32; 2]); 4]| {
        for i in [0usize, 1, 2, 0, 2, 3] {
            let (pos, uvp) = p[i];
            out.push(TexVertex {
                pos: [c[0] + pos[0], c[1] + pos[1], c[2] + pos[2]],
                uv: [uvp[0] / TEX, uvp[1] / TEX],
            });
        }
    };

    let (x0, x1, y0, y1, z0, z1) = (-hx, hx, -hy, hy, -hz, hz);
    // Front (+z): u grows toward +x (viewer's right when facing the model).
    quad([
        ([x0, y0, z1], [u0 + d, v0 + d + h]),
        ([x1, y0, z1], [u0 + d + w, v0 + d + h]),
        ([x1, y1, z1], [u0 + d + w, v0 + d]),
        ([x0, y1, z1], [u0 + d, v0 + d]),
    ]);
    // Back (-z).
    quad([
        ([x1, y0, z0], [u0 + 2.0 * d + w, v0 + d + h]),
        ([x0, y0, z0], [u0 + 2.0 * d + 2.0 * w, v0 + d + h]),
        ([x0, y1, z0], [u0 + 2.0 * d + 2.0 * w, v0 + d]),
        ([x1, y1, z0], [u0 + 2.0 * d + w, v0 + d]),
    ]);
    // Right (-x): u grows toward +z (shares its front edge with the front face).
    quad([
        ([x0, y0, z0], [u0, v0 + d + h]),
        ([x0, y0, z1], [u0 + d, v0 + d + h]),
        ([x0, y1, z1], [u0 + d, v0 + d]),
        ([x0, y1, z0], [u0, v0 + d]),
    ]);
    // Left (+x): u grows toward -z.
    quad([
        ([x1, y0, z1], [u0 + d + w, v0 + d + h]),
        ([x1, y0, z0], [u0 + 2.0 * d + w, v0 + d + h]),
        ([x1, y1, z0], [u0 + 2.0 * d + w, v0 + d]),
        ([x1, y1, z1], [u0 + d + w, v0 + d]),
    ]);
    // Top (+y): v grows toward +z (shares its v0+d edge with the front face).
    quad([
        ([x0, y1, z1], [u0 + d, v0 + d]),
        ([x1, y1, z1], [u0 + d + w, v0 + d]),
        ([x1, y1, z0], [u0 + d + w, v0]),
        ([x0, y1, z0], [u0 + d, v0]),
    ]);
    // Bottom (-y): mirrored, v grows toward -z.
    quad([
        ([x0, y0, z0], [u0 + d + w, v0 + d]),
        ([x1, y0, z0], [u0 + d + 2.0 * w, v0 + d]),
        ([x1, y0, z1], [u0 + d + 2.0 * w, v0]),
        ([x0, y0, z1], [u0 + d + w, v0]),
    ]);
}

/// Build the six-part player mesh (base + overlay layer per part).
/// The model faces +z; the character's right side is -x.
fn build_skin_mesh(device: &wgpu::Device, slim: bool) -> SkinMesh {
    let aw = if slim { 3.0f32 } else { 4.0 }; // arm width in px
    let arm_x = aw / 2.0 - 1.0; // arm center offset from the ±5px shoulder pivot

    // (pivot px, center px, size px, base uv, overlay uv, overlay inflate)
    type Part = ([f32; 3], [f32; 3], [f32; 3], [f32; 2], [f32; 2], f32);
    let parts: [Part; 6] = [
        ([0.0, 24.0, 0.0], [0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0], [32.0, 0.0], 0.5),
        ([0.0, 24.0, 0.0], [0.0, -6.0, 0.0], [8.0, 12.0, 4.0], [16.0, 16.0], [16.0, 32.0], 0.25),
        ([-5.0, 22.0, 0.0], [-arm_x, -4.0, 0.0], [aw, 12.0, 4.0], [40.0, 16.0], [40.0, 32.0], 0.25),
        ([5.0, 22.0, 0.0], [arm_x, -4.0, 0.0], [aw, 12.0, 4.0], [32.0, 48.0], [48.0, 48.0], 0.25),
        ([-2.0, 12.0, 0.0], [0.0, -6.0, 0.0], [4.0, 12.0, 4.0], [0.0, 16.0], [0.0, 32.0], 0.25),
        ([2.0, 12.0, 0.0], [0.0, -6.0, 0.0], [4.0, 12.0, 4.0], [16.0, 48.0], [0.0, 48.0], 0.25),
    ];

    let mut verts: Vec<TexVertex> = Vec::new();
    let mut ranges = [(0u32, 0u32); 6];
    let mut pivots = [Vec3::ZERO; 6];
    for (i, (pivot, center, size, uv, uv_overlay, inflate)) in parts.into_iter().enumerate() {
        let start = verts.len() as u32;
        skin_box(&mut verts, center, size, uv, 0.0);
        skin_box(&mut verts, center, size, uv_overlay, inflate);
        ranges[i] = (start, verts.len() as u32 - start);
        pivots[i] = Vec3::from(pivot) * SKIN_PX;
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(if slim { "skin-mesh-slim" } else { "skin-mesh-wide" }),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    SkinMesh { vbuf, parts: ranges, pivots }
}

/// Build the armor overlay mesh: one inflated box per body part using the legacy
/// 64×32 armor UV layout (left limbs mirror the right, arms always 4px wide).
/// The armor texture is padded to 64×64 so `skin_box`'s /64 UV math maps it 1:1.
/// `inflate` selects the layer thickness — outer (~1.0: helmet/chest/boots) or
/// inner (~0.5: leggings). Same pivots as the skin mesh, so parts animate with it.
fn build_armor_mesh(device: &wgpu::Device, inflate: f32) -> SkinMesh {
    // (pivot px, center px relative to pivot, size px, base uv on the 64×64 pad)
    type Part = ([f32; 3], [f32; 3], [f32; 3], [f32; 2]);
    let parts: [Part; 6] = [
        ([0.0, 24.0, 0.0], [0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0]), // head
        ([0.0, 24.0, 0.0], [0.0, -6.0, 0.0], [8.0, 12.0, 4.0], [16.0, 16.0]), // body
        ([-5.0, 22.0, 0.0], [-1.0, -4.0, 0.0], [4.0, 12.0, 4.0], [40.0, 16.0]), // right arm
        ([5.0, 22.0, 0.0], [1.0, -4.0, 0.0], [4.0, 12.0, 4.0], [40.0, 16.0]), // left arm (mirror uv)
        ([-2.0, 12.0, 0.0], [0.0, -6.0, 0.0], [4.0, 12.0, 4.0], [0.0, 16.0]), // right leg
        ([2.0, 12.0, 0.0], [0.0, -6.0, 0.0], [4.0, 12.0, 4.0], [0.0, 16.0]), // left leg (mirror uv)
    ];
    let mut verts: Vec<TexVertex> = Vec::new();
    let mut ranges = [(0u32, 0u32); 6];
    let mut pivots = [Vec3::ZERO; 6];
    for (i, (pivot, center, size, uv)) in parts.into_iter().enumerate() {
        let start = verts.len() as u32;
        skin_box(&mut verts, center, size, uv, inflate);
        ranges[i] = (start, verts.len() as u32 - start);
        pivots[i] = Vec3::from(pivot) * SKIN_PX;
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("armor-mesh"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    SkinMesh { vbuf, parts: ranges, pivots }
}

// --- non-humanoid mob models -------------------------------------------------

/// One animated part of a prebuilt mob model: a contiguous vertex range plus its
/// pivot (blocks, feet-relative) and how it animates.
struct MobMeshPart {
    range: (u32, u32),
    pivot: Vec3,
    anim: PartAnim,
}

/// A prebuilt cuboid mob model (creeper/pig/cow/…): one vertex buffer, its parts
/// drawn individually so each can swing.
struct MobMesh {
    vbuf: wgpu::Buffer,
    parts: Vec<MobMeshPart>,
}

/// Append one textured cuboid to `out`, like `skin_box` but with an arbitrary
/// texture size, a per-model `scale` (blocks per pixel) and an optional baked
/// rotation about X (used to lay flat bodies down while keeping the UV unwrap).
/// Positions are relative to the part pivot; the pivot translation is applied at
/// draw time. Face strip order matches the vanilla box unwrap.
#[allow(clippy::too_many_arguments)]
fn model_box(
    out: &mut Vec<TexVertex>,
    center: [f32; 3],
    size: [f32; 3],
    uv: [f32; 2],
    tex: [f32; 2],
    scale: f32,
    inflate: f32,
    x_rot: f32,
) {
    let (w, h, d) = (size[0], size[1], size[2]);
    let hx = (w / 2.0 + inflate) * scale;
    let hy = (h / 2.0 + inflate) * scale;
    let hz = (d / 2.0 + inflate) * scale;
    let c = [center[0] * scale, center[1] * scale, center[2] * scale];
    let (u0, v0) = (uv[0], uv[1]);
    let (tw, th) = (tex[0], tex[1]);
    let (sin, cos) = x_rot.sin_cos();

    let mut quad = |p: [([f32; 3], [f32; 2]); 4]| {
        for i in [0usize, 1, 2, 0, 2, 3] {
            let (pos, uvp) = p[i];
            let (px, py, pz) = (c[0] + pos[0], c[1] + pos[1], c[2] + pos[2]);
            // Bake the fixed X rotation about the pivot (origin of these coords).
            let ry = py * cos - pz * sin;
            let rz = py * sin + pz * cos;
            out.push(TexVertex { pos: [px, ry, rz], uv: [uvp[0] / tw, uvp[1] / th] });
        }
    };

    let (x0, x1, y0, y1, z0, z1) = (-hx, hx, -hy, hy, -hz, hz);
    // Front (+z).
    quad([
        ([x0, y0, z1], [u0 + d, v0 + d + h]),
        ([x1, y0, z1], [u0 + d + w, v0 + d + h]),
        ([x1, y1, z1], [u0 + d + w, v0 + d]),
        ([x0, y1, z1], [u0 + d, v0 + d]),
    ]);
    // Back (-z).
    quad([
        ([x1, y0, z0], [u0 + 2.0 * d + w, v0 + d + h]),
        ([x0, y0, z0], [u0 + 2.0 * d + 2.0 * w, v0 + d + h]),
        ([x0, y1, z0], [u0 + 2.0 * d + 2.0 * w, v0 + d]),
        ([x1, y1, z0], [u0 + 2.0 * d + w, v0 + d]),
    ]);
    // Right (-x).
    quad([
        ([x0, y0, z0], [u0, v0 + d + h]),
        ([x0, y0, z1], [u0 + d, v0 + d + h]),
        ([x0, y1, z1], [u0 + d, v0 + d]),
        ([x0, y1, z0], [u0, v0 + d]),
    ]);
    // Left (+x).
    quad([
        ([x1, y0, z1], [u0 + d + w, v0 + d + h]),
        ([x1, y0, z0], [u0 + 2.0 * d + w, v0 + d + h]),
        ([x1, y1, z0], [u0 + 2.0 * d + w, v0 + d]),
        ([x1, y1, z1], [u0 + d + w, v0 + d]),
    ]);
    // Top (+y).
    quad([
        ([x0, y1, z1], [u0 + d, v0 + d]),
        ([x1, y1, z1], [u0 + d + w, v0 + d]),
        ([x1, y1, z0], [u0 + d + w, v0]),
        ([x0, y1, z0], [u0 + d, v0]),
    ]);
    // Bottom (-y).
    quad([
        ([x0, y0, z0], [u0 + d + w, v0 + d]),
        ([x1, y0, z0], [u0 + d + 2.0 * w, v0 + d]),
        ([x1, y0, z1], [u0 + d + 2.0 * w, v0]),
        ([x0, y0, z1], [u0 + d + w, v0]),
    ]);
}

/// Build every non-humanoid mob model into a GPU mesh, indexed by
/// `MobModel::index()`.
fn build_mob_meshes(device: &wgpu::Device) -> Vec<MobMesh> {
    entity_models::MobModel::all()
        .iter()
        .map(|&m| {
            let def = entity_models::model_def(m);
            let mut verts: Vec<TexVertex> = Vec::new();
            let mut parts: Vec<MobMeshPart> = Vec::new();
            for part in &def.parts {
                let start = verts.len() as u32;
                for cube in &part.cubes {
                    model_box(
                        &mut verts,
                        cube.center,
                        cube.size,
                        cube.uv,
                        [def.tex_w, def.tex_h],
                        def.scale,
                        cube.inflate,
                        part.x_rot,
                    );
                }
                parts.push(MobMeshPart {
                    range: (start, verts.len() as u32 - start),
                    pivot: Vec3::from(part.pivot) * def.scale,
                    anim: part.anim,
                });
            }
            let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mob-mesh"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            MobMesh { vbuf, parts }
        })
        .collect()
}

/// Append a held-item sprite as a small cross of two perpendicular quads at a
/// hand, in skin-pixel space relative to the arm pivot (so it swings with the
/// arm). `hand_x` picks the right (−1) or left (+1) hand; `uv` is the atlas rect.
/// A cross (rather than one quad) keeps the item visible from the front and side.
fn push_item_quad(out: &mut Vec<TexVertex>, hand_x: f32, uv: [f32; 4]) {
    let (cx, cy, cz) = (hand_x, -11.0, 4.0); // hand point, a bit below/forward
    let h = 5.0; // ~0.625-block sprite
    let [u0, v0, u1, v1] = uv;
    // Facing +z (front); v grows downward in the atlas.
    let face_z = [
        ([cx - h, cy - h, cz], [u0, v1]),
        ([cx + h, cy - h, cz], [u1, v1]),
        ([cx + h, cy + h, cz], [u1, v0]),
        ([cx - h, cy - h, cz], [u0, v1]),
        ([cx + h, cy + h, cz], [u1, v0]),
        ([cx - h, cy + h, cz], [u0, v0]),
    ];
    // Facing +x (side), same sprite so it reads from the side too.
    let face_x = [
        ([cx, cy - h, cz - h], [u0, v1]),
        ([cx, cy - h, cz + h], [u1, v1]),
        ([cx, cy + h, cz + h], [u1, v0]),
        ([cx, cy - h, cz - h], [u0, v1]),
        ([cx, cy + h, cz + h], [u1, v0]),
        ([cx, cy + h, cz - h], [u0, v0]),
    ];
    for (p, uvp) in face_z.into_iter().chain(face_x) {
        out.push(TexVertex { pos: [p[0] * SKIN_PX, p[1] * SKIN_PX, p[2] * SKIN_PX], uv: uvp });
    }
}

/// A dropped-item sprite: a two-sided cross of quads centered at the origin (in
/// block units), so it reads from every angle as the model matrix spins it. The
/// skin pipeline back-face culls, so each quad is emitted with both windings.
fn push_dropped_item(out: &mut Vec<TexVertex>, uv: [f32; 4]) {
    let h = 0.25; // ~0.5-block sprite
    let [u0, v0, u1, v1] = uv;
    // Corner order a,b,c,d with uvs (v grows downward in the atlas).
    let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
    let planes = [
        // facing ±z
        [[-h, -h, 0.0], [h, -h, 0.0], [h, h, 0.0], [-h, h, 0.0]],
        // facing ±x
        [[0.0, -h, -h], [0.0, -h, h], [0.0, h, h], [0.0, h, -h]],
    ];
    for p in planes {
        // Front (a,b,c / a,c,d) then back (a,c,b / a,d,c) windings.
        for &(i, j, k) in &[(0, 1, 2), (0, 2, 3), (0, 2, 1), (0, 3, 2)] {
            for idx in [i, j, k] {
                out.push(TexVertex { pos: p[idx], uv: uvs[idx] });
            }
        }
    }
}

/// Pad an armor texture (typically 64×32) into a 64×64 RGBA image, content in
/// the top-left and the rest transparent, so it shares the skin UV convention.
fn pad_armor_texture(src: &image::RgbaImage) -> image::RgbaImage {
    let mut out = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 0]));
    for (x, y, px) in src.enumerate_pixels() {
        if x < 64 && y < 64 {
            out.put_pixel(x, y, *px);
        }
    }
    out
}

// --- panorama ---------------------------------------------------------------

/// GPU state for the title-screen panorama (six faces, one texture each).
struct PanoramaGpu {
    vbuf: wgpu::Buffer,
    faces: [wgpu::BindGroup; 6],
}

/// Cube [-1,1]³ around the camera; face order matches the vanilla
/// panorama_0..5 images (front, right, back, left, top, bottom). UV mappings
/// chosen so adjacent panels are seam-continuous.
fn panorama_vertices() -> [TexVertex; 36] {
    // 4 corners per face: (pos, uv), fanned as [0,1,2, 0,2,3]. Cull is off.
    let faces: [[([f32; 3], [f32; 2]); 4]; 6] = [
        // panorama_0, front (z = -1): u=(x+1)/2, v=(1-y)/2
        [
            ([-1.0, -1.0, -1.0], [0.0, 1.0]),
            ([1.0, -1.0, -1.0], [1.0, 1.0]),
            ([1.0, 1.0, -1.0], [1.0, 0.0]),
            ([-1.0, 1.0, -1.0], [0.0, 0.0]),
        ],
        // panorama_1, right (x = +1): u=(z+1)/2
        [
            ([1.0, -1.0, -1.0], [0.0, 1.0]),
            ([1.0, -1.0, 1.0], [1.0, 1.0]),
            ([1.0, 1.0, 1.0], [1.0, 0.0]),
            ([1.0, 1.0, -1.0], [0.0, 0.0]),
        ],
        // panorama_2, back (z = +1): u=(1-x)/2
        [
            ([1.0, -1.0, 1.0], [0.0, 1.0]),
            ([-1.0, -1.0, 1.0], [1.0, 1.0]),
            ([-1.0, 1.0, 1.0], [1.0, 0.0]),
            ([1.0, 1.0, 1.0], [0.0, 0.0]),
        ],
        // panorama_3, left (x = -1): u=(1-z)/2
        [
            ([-1.0, -1.0, 1.0], [0.0, 1.0]),
            ([-1.0, -1.0, -1.0], [1.0, 1.0]),
            ([-1.0, 1.0, -1.0], [1.0, 0.0]),
            ([-1.0, 1.0, 1.0], [0.0, 0.0]),
        ],
        // panorama_4, top (y = +1): u=(x+1)/2, v=(z+1)/2
        [
            ([-1.0, 1.0, -1.0], [0.0, 0.0]),
            ([1.0, 1.0, -1.0], [1.0, 0.0]),
            ([1.0, 1.0, 1.0], [1.0, 1.0]),
            ([-1.0, 1.0, 1.0], [0.0, 1.0]),
        ],
        // panorama_5, bottom (y = -1): u=(x+1)/2, v=(1-z)/2
        [
            ([-1.0, -1.0, 1.0], [0.0, 0.0]),
            ([1.0, -1.0, 1.0], [1.0, 0.0]),
            ([1.0, -1.0, -1.0], [1.0, 1.0]),
            ([-1.0, -1.0, -1.0], [0.0, 1.0]),
        ],
    ];
    let mut out = [TexVertex { pos: [0.0; 3], uv: [0.0; 2] }; 36];
    let mut i = 0;
    for f in faces {
        for idx in [0usize, 1, 2, 0, 2, 3] {
            let (pos, uv) = f[idx];
            out[i] = TexVertex { pos, uv };
            i += 1;
        }
    }
    out
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
    pipe_skin: wgpu::RenderPipeline,
    pipe_panorama: wgpu::RenderPipeline,

    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    atlas_layout: wgpu::BindGroupLayout,
    atlas_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    atlas_bg: wgpu::BindGroup,
    section_uniform: DynUniform,
    entity_uniform: DynUniform,

    cube_vbuf: wgpu::Buffer,
    skin_mesh_wide: SkinMesh,
    skin_mesh_slim: SkinMesh,
    /// Armor layer meshes (legacy UVs): outer = helmet/chest/boots, inner = leggings.
    armor_mesh_outer: SkinMesh,
    armor_mesh_inner: SkinMesh,
    /// Prebuilt non-humanoid mob models, indexed by `MobModel::index()`.
    mob_meshes: Vec<MobMesh>,
    /// Uploaded skin textures by key (0 = default Steve).
    skins: HashMap<u64, wgpu::BindGroup>,
    /// Armor textures keyed by (material id, layer): layer 0 = humanoid,
    /// 1 = humanoid_leggings.
    armor_tex: HashMap<(u8, u8), wgpu::BindGroup>,
    /// The item-icon atlas as a skin-style bind group, for held items in hand.
    item_atlas: Option<wgpu::BindGroup>,
    panorama: Option<PanoramaGpu>,
    meshes: HashMap<SectionPos, SectionGpu>,
    egui_renderer: egui_wgpu::Renderer,
    /// Present modes the window surface supports (used to toggle vsync).
    present_modes: Vec<wgpu::PresentMode>,
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
        let mut present_modes: Vec<wgpu::PresentMode> = vec![wgpu::PresentMode::Fifo];
        let (target, color_format) = match surface {
            Some(surface) => {
                let caps = surface.get_capabilities(&adapter);
                present_modes = caps.present_modes.clone();
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
        // Panorama wants smooth filtering (vanilla blurs it too).
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
        let skin_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skin.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SKIN_WGSL.into()),
        });
        let panorama_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("panorama.wgsl"),
            source: wgpu::ShaderSource::Wgsl(PANORAMA_WGSL.into()),
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
        let skin_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skin-pl"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&atlas_layout),
                Some(&entity_uniform.layout),
            ],
            immediate_size: 0,
        });
        let panorama_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("panorama-pl"),
            bind_group_layouts: &[Some(&globals_layout), Some(&atlas_layout)],
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

        let tex_vbl = wgpu::VertexBufferLayout {
            array_stride: size_of::<TexVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &TEX_VERTEX_ATTRS,
        };

        let pipe_skin = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin"),
            layout: Some(&skin_pl),
            vertex: wgpu::VertexState {
                module: &skin_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&tex_vbl),
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
                module: &skin_shader,
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

        // Drawn first, behind everything: depth ignored entirely.
        let pipe_panorama = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("panorama"),
            layout: Some(&panorama_pl),
            vertex: wgpu::VertexState {
                module: &panorama_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&tex_vbl),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &panorama_shader,
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
        let skin_mesh_wide = build_skin_mesh(&device, false);
        let skin_mesh_slim = build_skin_mesh(&device, true);
        let armor_mesh_outer = build_armor_mesh(&device, 1.0);
        let armor_mesh_inner = build_armor_mesh(&device, 0.5);
        let mob_meshes = build_mob_meshes(&device);

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
            pipe_skin,
            pipe_panorama,
            globals_buf,
            globals_bg,
            atlas_layout,
            atlas_sampler,
            linear_sampler,
            atlas_bg,
            section_uniform,
            entity_uniform,
            cube_vbuf,
            skin_mesh_wide,
            skin_mesh_slim,
            armor_mesh_outer,
            armor_mesh_inner,
            mob_meshes,
            skins: HashMap::new(),
            armor_tex: HashMap::new(),
            item_atlas: None,
            panorama: None,
            meshes: HashMap::new(),
            egui_renderer,
            present_modes,
        })
    }

    /// Toggle vsync. On = FIFO (synced to the display refresh). Off = the
    /// fastest uncapped mode the surface supports (Immediate, else Mailbox,
    /// else FIFO) — this is what unlocks "extremely high FPS". No-op offscreen.
    pub fn set_vsync(&mut self, vsync: bool) {
        let want = if vsync {
            wgpu::PresentMode::Fifo
        } else if self.present_modes.contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else if self.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        if let Target::Window { surface, config } = &mut self.target
            && config.present_mode != want
        {
            config.present_mode = want;
            surface.configure(&self.device, config);
        }
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

    /// Upload a player skin (64x64 RGBA, already normalized) under `key`.
    /// Key 0 is the default Steve fallback. No-op if already uploaded.
    pub fn ensure_skin(&mut self, key: u64, image: &image::RgbaImage) {
        if self.skins.contains_key(&key) || image.width() == 0 || image.height() == 0 {
            return;
        }
        let bg = make_atlas_bind_group(
            &self.device,
            &self.queue,
            &self.atlas_layout,
            &self.atlas_sampler,
            image.width(),
            image.height(),
            image.as_raw(),
        );
        self.skins.insert(key, bg);
    }

    pub fn has_skin(&self, key: u64) -> bool {
        self.skins.contains_key(&key)
    }

    /// Upload an armor texture for `material` (`leggings` = the humanoid_leggings
    /// layer). The image is padded to 64×64 to match the skin UV convention.
    pub fn ensure_armor(&mut self, material: ArmorMaterial, leggings: bool, image: &image::RgbaImage) {
        if image.width() == 0 || image.height() == 0 {
            return;
        }
        let key = (material.id(), leggings as u8);
        if self.armor_tex.contains_key(&key) {
            return;
        }
        let padded = pad_armor_texture(image);
        let bg = make_atlas_bind_group(
            &self.device,
            &self.queue,
            &self.atlas_layout,
            &self.atlas_sampler,
            padded.width(),
            padded.height(),
            padded.as_raw(),
        );
        self.armor_tex.insert(key, bg);
    }

    /// Replace the item-icon atlas unconditionally (used after a live re-bake
    /// when a server resource pack changes item textures).
    pub fn set_item_atlas(&mut self, image: &image::RgbaImage) {
        if image.width() == 0 || image.height() == 0 {
            return;
        }
        self.item_atlas = Some(make_atlas_bind_group(
            &self.device,
            &self.queue,
            &self.atlas_layout,
            &self.atlas_sampler,
            image.width(),
            image.height(),
            image.as_raw(),
        ));
    }

    /// Upload the item-icon atlas (used to draw held items in players' hands).
    pub fn ensure_item_atlas(&mut self, image: &image::RgbaImage) {
        if self.item_atlas.is_some() || image.width() == 0 || image.height() == 0 {
            return;
        }
        self.item_atlas = Some(make_atlas_bind_group(
            &self.device,
            &self.queue,
            &self.atlas_layout,
            &self.atlas_sampler,
            image.width(),
            image.height(),
            image.as_raw(),
        ));
    }

    /// Upload the six title-screen panorama faces (vanilla panorama_0..5:
    /// front, right, back, left, top, bottom).
    pub fn set_panorama(&mut self, faces: &[image::RgbaImage; 6]) {
        let bind = |img: &image::RgbaImage| {
            let raw: &[u8] = img.as_raw();
            let fallback: &[u8] = &[64, 64, 64, 255];
            make_atlas_bind_group(
                &self.device,
                &self.queue,
                &self.atlas_layout,
                &self.linear_sampler,
                img.width().max(1),
                img.height().max(1),
                if raw.is_empty() { fallback } else { raw },
            )
        };
        let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("panorama"),
            contents: bytemuck::cast_slice(&panorama_vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.panorama = Some(PanoramaGpu {
            vbuf,
            faces: [
                bind(&faces[0]),
                bind(&faces[1]),
                bind(&faces[2]),
                bind(&faces[3]),
                bind(&faces[4]),
                bind(&faces[5]),
            ],
        });
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

    /// Drop all section meshes (returning to the menu / joining a new world).
    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
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

        // --- entities -----------------------------------------------------------
        // Each draw is one dynamic-uniform slot: model matrix (camera-relative)
        // + color. Boxes use the entity pipeline, player parts the skin one.
        enum EntityCmd {
            Box,
            SkinPart { key: u64, slim: bool, part: usize },
            /// One armor part: `mat` = material id, `leggings` picks the texture
            /// layer, `inner` picks the thinner mesh (leggings vs outer).
            ArmorPart { mat: u8, leggings: bool, inner: bool, part: usize },
            /// A held item sprite: vertex range into `item_verts`.
            ItemQuad { start: u32, count: u32 },
            /// One part of a prebuilt mob model: `model` picks the mesh, `key`
            /// the texture, `part` the vertex range.
            MobPart { model: MobModel, key: u64, part: usize },
        }
        let mut slots: Vec<[u8; 80]> = Vec::new();
        let mut cmds: Vec<EntityCmd> = Vec::new();
        // Held-item sprites accumulate here; uploaded once as a dynamic buffer.
        let mut item_verts: Vec<TexVertex> = Vec::new();
        for e in entities {
            let base = Vec3::new(
                (e.pos[0] - scene.cam_pos[0]) as f32,
                (e.pos[1] - scene.cam_pos[1]) as f32,
                (e.pos[2] - scene.cam_pos[2]) as f32,
            );
            let tint = e.tint;
            let mut push = |model: Mat4, color: [f32; 4], cmd: EntityCmd| {
                // Model-wide tint (damage flash); [1,1,1] leaves the color as-is.
                let color = [color[0] * tint[0], color[1] * tint[1], color[2] * tint[2], color[3]];
                let mut bytes = [0u8; 80];
                bytes[..64].copy_from_slice(bytemuck::cast_slice(&model.to_cols_array()));
                bytes[64..].copy_from_slice(bytemuck::cast_slice(&color));
                slots.push(bytes);
                cmds.push(cmd);
            };
            match e.kind {
                EntityDrawKind::Player { skin, slim, swing, head_pitch, armor, main_hand, off_hand } => {
                    let key = if self.skins.contains_key(&skin) { skin } else { 0 };
                    if !self.skins.contains_key(&key) {
                        // No skin at all (not even Steve): blue box fallback.
                        push(
                            Mat4::from_translation(base + Vec3::Y * 0.9)
                                * Mat4::from_scale(Vec3::new(0.6, 1.8, 0.6)),
                            [0.3, 0.5, 0.9, 1.0],
                            EntityCmd::Box,
                        );
                        continue;
                    }
                    let rot =
                        Mat4::from_translation(base) * Mat4::from_rotation_y(-e.yaw.to_radians());
                    let mesh = if slim { &self.skin_mesh_slim } else { &self.skin_mesh_wide };
                    for part in 0..6 {
                        // Head follows pitch; arms/legs swing in opposite pairs.
                        let angle = match part {
                            PART_HEAD => head_pitch.to_radians(),
                            PART_RIGHT_ARM => swing,
                            PART_LEFT_ARM => -swing,
                            PART_RIGHT_LEG => -swing,
                            PART_LEFT_LEG => swing,
                            PART_BODY => 0.0,
                            _ => 0.0,
                        };
                        let model = rot
                            * Mat4::from_translation(mesh.pivots[part])
                            * Mat4::from_rotation_x(angle);
                        push(
                            model,
                            [1.0, 1.0, 1.0, 1.0],
                            EntityCmd::SkinPart { key, slim, part },
                        );
                    }

                    // Armor layers over the model. Each slot maps to a set of
                    // parts, a texture layer (humanoid vs leggings) and a mesh
                    // thickness. Drawn only when the texture is loaded so a
                    // missing/unknown material simply shows no armor (never garbage).
                    let part_angle = |part: usize| match part {
                        PART_HEAD => head_pitch.to_radians(),
                        PART_RIGHT_ARM => swing,
                        PART_LEFT_ARM => -swing,
                        PART_RIGHT_LEG => -swing,
                        PART_LEFT_LEG => swing,
                        _ => 0.0,
                    };
                    // (armor slot, parts, leggings-layer, inner-mesh)
                    let groups: [(usize, &[usize], bool, bool); 4] = [
                        (0, &[PART_HEAD], false, false), // helmet
                        (1, &[PART_BODY, PART_RIGHT_ARM, PART_LEFT_ARM], false, false), // chestplate
                        (2, &[PART_BODY, PART_RIGHT_LEG, PART_LEFT_LEG], true, true), // leggings
                        (3, &[PART_RIGHT_LEG, PART_LEFT_LEG], true, false), // boots
                    ];
                    for (slot, parts, leggings, inner) in groups {
                        let Some(mat) = armor[slot] else { continue };
                        let mat_id = mat.id();
                        if !self.armor_tex.contains_key(&(mat_id, leggings as u8)) {
                            continue; // texture not loaded: skip this piece
                        }
                        let amesh =
                            if inner { &self.armor_mesh_inner } else { &self.armor_mesh_outer };
                        for &part in parts {
                            let model = rot
                                * Mat4::from_translation(amesh.pivots[part])
                                * Mat4::from_rotation_x(part_angle(part));
                            push(
                                model,
                                [1.0, 1.0, 1.0, 1.0],
                                EntityCmd::ArmorPart { mat: mat_id, leggings, inner, part },
                            );
                        }
                    }

                    // Held items: a flat sprite in each hand, swinging with the arm.
                    if self.item_atlas.is_some() {
                        for (uv, arm_part, hand_x) in [
                            (main_hand, PART_RIGHT_ARM, -1.0f32),
                            (off_hand, PART_LEFT_ARM, 1.0f32),
                        ] {
                            let Some(uv) = uv else { continue };
                            let model = rot
                                * Mat4::from_translation(mesh.pivots[arm_part])
                                * Mat4::from_rotation_x(part_angle(arm_part));
                            let start = item_verts.len() as u32;
                            push_item_quad(&mut item_verts, hand_x, uv);
                            let count = item_verts.len() as u32 - start;
                            push(model, [1.0, 1.0, 1.0, 1.0], EntityCmd::ItemQuad { start, count });
                        }
                    }
                }
                EntityDrawKind::Box { w, h, color } => {
                    push(
                        Mat4::from_translation(base + Vec3::Y * (h / 2.0))
                            * Mat4::from_scale(Vec3::new(w, h, w)),
                        [color[0], color[1], color[2], 1.0],
                        EntityCmd::Box,
                    );
                }
                EntityDrawKind::Item { uv } => {
                    // Only drawable with the item atlas loaded.
                    if self.item_atlas.is_some() {
                        let model = Mat4::from_translation(base + Vec3::Y * 0.25)
                            * Mat4::from_rotation_y(-e.yaw.to_radians());
                        let start = item_verts.len() as u32;
                        push_dropped_item(&mut item_verts, uv);
                        let count = item_verts.len() as u32 - start;
                        push(model, [1.0, 1.0, 1.0, 1.0], EntityCmd::ItemQuad { start, count });
                    }
                }
                EntityDrawKind::Mob { tex, model, swing, head_pitch } => {
                    if !self.skins.contains_key(&tex) {
                        // Texture missing: fall back to a grey box so the mob is
                        // still visible (never invisible).
                        push(
                            Mat4::from_translation(base + Vec3::Y * 0.5)
                                * Mat4::from_scale(Vec3::new(0.7, 1.0, 0.7)),
                            [0.6, 0.62, 0.66, 1.0],
                            EntityCmd::Box,
                        );
                        continue;
                    }
                    let mesh = &self.mob_meshes[model.index()];
                    let rot =
                        Mat4::from_translation(base) * Mat4::from_rotation_y(-e.yaw.to_radians());
                    for (pi, part) in mesh.parts.iter().enumerate() {
                        let angle = match part.anim {
                            PartAnim::Static => 0.0,
                            PartAnim::Head => head_pitch.to_radians(),
                            PartAnim::Leg(sign) => swing * sign,
                        };
                        let m = rot
                            * Mat4::from_translation(part.pivot)
                            * Mat4::from_rotation_x(angle);
                        push(m, [1.0, 1.0, 1.0, 1.0], EntityCmd::MobPart { model, key: tex, part: pi });
                    }
                }
            }
        }
        self.entity_uniform.begin_frame(&self.device, slots.len() as u32);
        for (i, b) in slots.iter().enumerate() {
            self.entity_uniform.write_slot(i as u32, b);
        }
        self.entity_uniform.upload(&self.queue);

        // Held-item sprites: one dynamic vertex buffer for the whole frame.
        let item_vbuf = (!item_verts.is_empty()).then(|| {
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("held-items"),
                contents: bytemuck::cast_slice(&item_verts),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });

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

            // Panorama backdrop (menu only), behind everything.
            if scene.panorama
                && let Some(pano) = &self.panorama
            {
                pass.set_pipeline(&self.pipe_panorama);
                pass.set_vertex_buffer(0, pano.vbuf.slice(..));
                for (i, face) in pano.faces.iter().enumerate() {
                    pass.set_bind_group(1, face, &[]);
                    let start = i as u32 * 6;
                    pass.draw(start..start + 6, 0..1);
                    draw_calls += 1;
                }
            }

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

            // Entities: solid boxes first, then skinned player parts.
            if cmds.iter().any(|c| matches!(c, EntityCmd::Box)) {
                pass.set_pipeline(&self.pipe_entity);
                pass.set_vertex_buffer(0, self.cube_vbuf.slice(..));
                for (i, cmd) in cmds.iter().enumerate() {
                    if !matches!(cmd, EntityCmd::Box) {
                        continue;
                    }
                    pass.set_bind_group(
                        1,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(0..36, 0..1);
                    draw_calls += 1;
                }
            }
            if cmds.iter().any(|c| matches!(c, EntityCmd::SkinPart { .. })) {
                pass.set_pipeline(&self.pipe_skin);
                let mut bound_slim: Option<bool> = None;
                let mut bound_key: Option<u64> = None;
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::SkinPart { key, slim, part } = cmd else { continue };
                    let mesh = if *slim { &self.skin_mesh_slim } else { &self.skin_mesh_wide };
                    if bound_slim != Some(*slim) {
                        pass.set_vertex_buffer(0, mesh.vbuf.slice(..));
                        bound_slim = Some(*slim);
                    }
                    if bound_key != Some(*key) {
                        // Key existence was checked when the cmd was built.
                        pass.set_bind_group(1, &self.skins[key], &[]);
                        bound_key = Some(*key);
                    }
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    let (start, count) = mesh.parts[*part];
                    pass.draw(start..start + count, 0..1);
                    draw_calls += 1;
                }
            }
            // Non-humanoid mob models: same textured skin pipeline, one draw per
            // animated part, textured with the mob's real entity PNG.
            if cmds.iter().any(|c| matches!(c, EntityCmd::MobPart { .. })) {
                pass.set_pipeline(&self.pipe_skin);
                let mut bound_model: Option<usize> = None;
                let mut bound_key: Option<u64> = None;
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::MobPart { model, key, part } = cmd else { continue };
                    let mesh = &self.mob_meshes[model.index()];
                    if bound_model != Some(model.index()) {
                        pass.set_vertex_buffer(0, mesh.vbuf.slice(..));
                        bound_model = Some(model.index());
                    }
                    if bound_key != Some(*key) {
                        pass.set_bind_group(1, &self.skins[key], &[]);
                        bound_key = Some(*key);
                    }
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    let (start, count) = mesh.parts[*part].range;
                    pass.draw(start..start + count, 0..1);
                    draw_calls += 1;
                }
            }
            // Armor layers, same pipeline/shader as skins (alpha-discard covers
            // the transparent regions), over the top of the player parts.
            if cmds.iter().any(|c| matches!(c, EntityCmd::ArmorPart { .. })) {
                pass.set_pipeline(&self.pipe_skin);
                let mut bound_inner: Option<bool> = None;
                let mut bound_tex: Option<(u8, u8)> = None;
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::ArmorPart { mat, leggings, inner, part } = cmd else { continue };
                    let amesh =
                        if *inner { &self.armor_mesh_inner } else { &self.armor_mesh_outer };
                    if bound_inner != Some(*inner) {
                        pass.set_vertex_buffer(0, amesh.vbuf.slice(..));
                        bound_inner = Some(*inner);
                    }
                    let tkey = (*mat, *leggings as u8);
                    if bound_tex != Some(tkey) {
                        let Some(bg) = self.armor_tex.get(&tkey) else { continue };
                        pass.set_bind_group(1, bg, &[]);
                        bound_tex = Some(tkey);
                    }
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    let (start, count) = amesh.parts[*part];
                    pass.draw(start..start + count, 0..1);
                    draw_calls += 1;
                }
            }
            // Held-item sprites (main/off hand), same skin pipeline + alpha discard.
            if let (Some(vbuf), Some(atlas)) = (&item_vbuf, &self.item_atlas) {
                pass.set_pipeline(&self.pipe_skin);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.set_bind_group(1, atlas, &[]);
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::ItemQuad { start, count } = cmd else { continue };
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(*start..*start + *count, 0..1);
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
            panorama: false,
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
