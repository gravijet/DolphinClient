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
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use tracing::warn;
use wgpu::util::DeviceExt;

const TERRAIN_WGSL: &str = include_str!("shaders/terrain.wgsl");
const ENTITY_WGSL: &str = include_str!("shaders/entity.wgsl");
const SKIN_WGSL: &str = include_str!("shaders/skin.wgsl");
const SKY_WGSL: &str = include_str!("shaders/sky.wgsl");
const PANORAMA_WGSL: &str = include_str!("shaders/panorama.wgsl");

/// Distance (blocks) the sun/moon/stars are placed from the camera.
const SKY_DIST: f32 = 100.0;
/// Vanilla cloud layer height (world Y).
const CLOUD_HEIGHT: f32 = 192.0;
/// Cloud cell size in blocks (one texel of clouds.png = 12 blocks).
const CLOUD_CELL: f32 = 12.0;
/// Half-extent (blocks) of the camera-relative cloud plane.
const CLOUD_EXTENT: f32 = 512.0;

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
    /// Vanilla selection outline: world-space AABBs (min, max) of the block
    /// under the crosshair. Empty = nothing targeted.
    pub outline: Vec<([f64; 3], [f64; 3])>,
    /// Mining crack overlay: block min-corner + destroy stage 0..=9.
    pub crack: Option<([f64; 3], u32)>,
    /// First-person view model (own hand + held item), drawn last, on top of the
    /// world. `None` in third person / menus.
    pub view_model: Option<ViewModel>,
    /// Celestial sky (sun, moon, stars). `None` in the Nether/End and menus —
    /// those keep the flat sky color only.
    pub sky: Option<SkyParams>,
}

/// Sun/moon/star state for one frame, derived from the world time. The sky
/// rotates about the world Z axis by `sun_angle` (0 = noon, sun overhead).
pub struct SkyParams {
    /// Celestial rotation angle in radians (0 = noon).
    pub sun_angle: f32,
    /// Star field opacity 0..1 (0 by day, up to ~0.9 at midnight).
    pub star_brightness: f32,
    /// Moon phase 0..7 (picks the moon texture).
    pub moon_phase: usize,
    /// Sun disc opacity 0..1 (fades out through dusk).
    pub sun_alpha: f32,
    /// Moon disc opacity 0..1 (fades in through dusk).
    pub moon_alpha: f32,
    /// Cloud-texture scroll offset (blocks); advances slowly with time.
    pub cloud_scroll: f32,
    /// Cloud tint + opacity `[r,g,b,a]` — RGB dims at night, `a`=0 disables.
    pub cloud_color: [f32; 4],
    /// Sunrise/sunset glow tint + strength `[r,g,b,a]` around the sun; `a`=0
    /// disables (daytime / night).
    pub glow_color: [f32; 4],
}

/// The first-person hand + held item shown in the bottom-right, exactly like
/// vanilla. Animated by the app: a swing arc on attack/use, an equip raise when
/// the held item changes, and a gentle walk bob.
pub struct ViewModel {
    /// Skin key for the arm (0 = default Steve).
    pub skin: u64,
    /// Slim (3px) arm model.
    pub slim: bool,
    /// Held item's item-atlas UV rect `[u0,v0,u1,v1]`, or `None` for an empty
    /// hand (arm only).
    pub item_uv: Option<[f32; 4]>,
    /// Whether the held item is a placeable block (rendered a touch bigger and
    /// flatter, like vanilla's block-in-hand) vs a flat item/tool.
    pub item_is_block: bool,
    /// Baked block geometry `(pos in unit-cube space centered at origin, atlas
    /// uv)` for a held block — rendered as a real 3D cube in the main hand
    /// instead of the flat icon. `None` falls back to the flat `item_uv`.
    pub block_quads: Option<Vec<([f32; 3], [f32; 2])>>,
    /// Off-hand item UV (shield/torch/map), drawn on the opposite side. `None`
    /// leaves the off hand empty (nothing drawn there).
    pub off_hand_uv: Option<[f32; 4]>,
    /// Whether the off-hand item is a block.
    pub off_hand_is_block: bool,
    /// Swing progress 0..1 (0 = idle); one full attack/use arc.
    pub swing: f32,
    /// Equip raise progress 0..1 (1 = fully raised); slides the model up from
    /// below when the held item changes.
    pub equip: f32,
    /// Walk-bob phase (radians) and amount 0..1.
    pub bob_phase: f32,
    pub bob: f32,
    /// Item-use progress 0..1 (eat/drink/bow/shield): raises the main-hand item
    /// toward the mouth. 0 = not using.
    pub using: f32,
    /// A free-running clock (seconds) that drives the eating shake while using.
    pub use_phase: f32,
    /// Left-handed: mirror the model to the bottom-left.
    pub left_handed: bool,
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
        /// One-shot attack/mine arm swing (radians), applied to the main arm
        /// only so the legs don't kick. 0 = not swinging.
        attack_swing: f32,
        /// Crouching: leans the upper body forward at the waist (vanilla sneak).
        sneaking: bool,
        /// Overlay-layer visibility bitmask (bit per part: hat/jacket/sleeves/
        /// pants). `0xFF` = all layers shown; the local player honours the Skin
        /// Customization toggles.
        skin_layers: u8,
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
    /// A dropped *block* item: its real baked geometry `(pos centered at origin,
    /// atlas uv)`, spun and floating like vanilla's 3D item-drops.
    ItemBlock { quads: Vec<([f32; 3], [f32; 2])> },
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
        /// Uniform model scale about the feet (1.0 = authored size; slimes and
        /// baby mobs scale up/down from their natural height).
        scale: f32,
    },
    /// A painting: a flat, wall-aligned slab `w`×`h` blocks. The front face
    /// shows the art texture `art_tex`; the back and the four thin edges use the
    /// tiled wooden `back_tex`. `pos` is the painting's centre; `facing` is the
    /// vanilla Direction index it faces (2 N, 3 S, 4 W, 5 E). Both textures are
    /// keys registered via `ensure_skin`.
    Painting {
        art_tex: u64,
        back_tex: u64,
        w: f32,
        h: f32,
        facing: u8,
    },
    /// An item frame: a 1×1 wooden frame on a wall/floor showing its contained
    /// item. `frame_tex` is the frame face (glow variant uses its own texture);
    /// the held item is either a flat icon (`item_uv` into the item atlas) or a
    /// small 3D block (`block_quads`, block atlas). `rot` is the 0..7 rotation
    /// step (×45°) and `facing` the vanilla Direction it hangs on.
    ItemFrame {
        frame_tex: u64,
        back_tex: u64,
        facing: u8,
        rot: u8,
        item_uv: Option<[f32; 4]>,
        block_quads: Vec<([f32; 3], [f32; 2])>,
    },
    /// A camera-facing particle billboard: the particle-atlas rect `uv`, tinted
    /// by `color` (white for most families, coloured for dust), `size` blocks
    /// square, alpha-blended.
    Particle {
        uv: [f32; 4],
        color: [f32; 3],
        size: f32,
    },
    /// A flying projectile drawn from its texture on two crossed planes along
    /// the flight axis (arrows, tridents). Oriented by `yaw`/`pitch` degrees.
    /// `tex` is a key registered via `ensure_skin`.
    Projectile {
        tex: u64,
        yaw: f32,
        pitch: f32,
    },
    /// A static world block drawn from its `quads` (unit cube centred on origin):
    /// primed TNT, minecart contents, falling blocks. `scale` sizes it, `y_off`
    /// lifts it above the entity position, `flash` brightens it toward white
    /// (0 = normal). No spin (unlike the dropped-item `ItemBlock`).
    StaticBlock {
        quads: Vec<([f32; 3], [f32; 2])>,
        y_off: f32,
        scale: f32,
        flash: f32,
    },
    /// A block-display entity: its block `quads` (corner at origin, 0..1)
    /// transformed by the vanilla display transform (translation, then
    /// left-rotation, scale, right-rotation — quaternions xyzw).
    DisplayBlock {
        quads: Vec<([f32; 3], [f32; 2])>,
        translation: [f32; 3],
        scale: [f32; 3],
        left_rot: [f32; 4],
        right_rot: [f32; 4],
    },
    /// An item-display entity: its item icon `uv` on a flat quad, transformed by
    /// the same display transform.
    DisplayItem {
        uv: [f32; 4],
        translation: [f32; 3],
        scale: [f32; 3],
        left_rot: [f32; 4],
        right_rot: [f32; 4],
    },
    /// An experience orb: a small camera-facing sprite (`tex`), tinted by `color`
    /// (a green↔yellow shimmer) and sized in blocks.
    Orb {
        tex: u64,
        size: f32,
        color: [f32; 3],
    },
    /// A posed armor stand: the armour-stand model (`tex`) with each part turned
    /// by its own Euler pose (degrees: head, body, right arm, left arm, right
    /// leg, left leg). `show_arms`/`show_base` gate the arms and the base plate;
    /// `scale` is 0.5 for a small stand.
    ArmorStandPosed {
        tex: u64,
        scale: f32,
        show_arms: bool,
        show_base: bool,
        poses: [[f32; 3]; 6],
    },
    /// A burning entity's flame: an upright, camera-facing billboard using the
    /// (alpha-keyed) fire texture `tex`. `w`/`h` size it in blocks (a touch
    /// wider/taller than the hitbox); `uv` selects the current animation frame's
    /// sub-rect `[u0,v0,u1,v1]` from the vertical fire strip.
    Fire {
        tex: u64,
        w: f32,
        h: f32,
        uv: [f32; 4],
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
    /// (first_vertex, vertex_count) of the base layer per part. For the armor
    /// mesh this is the whole part (no overlay).
    parts: [(u32, u32); 6],
    /// (first_vertex, vertex_count) of the overlay layer per part (hat/jacket/
    /// sleeves/pants). `(_, 0)` = no overlay (e.g. the armor mesh).
    overlay: [(u32, u32); 6],
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
    let mut overlay = [(0u32, 0u32); 6];
    let mut pivots = [Vec3::ZERO; 6];
    for (i, (pivot, center, size, uv, uv_overlay, inflate)) in parts.into_iter().enumerate() {
        let base_start = verts.len() as u32;
        skin_box(&mut verts, center, size, uv, 0.0);
        let ov_start = verts.len() as u32;
        ranges[i] = (base_start, ov_start - base_start);
        skin_box(&mut verts, center, size, uv_overlay, inflate);
        overlay[i] = (ov_start, verts.len() as u32 - ov_start);
        pivots[i] = Vec3::from(pivot) * SKIN_PX;
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(if slim { "skin-mesh-slim" } else { "skin-mesh-wide" }),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    SkinMesh { vbuf, parts: ranges, overlay, pivots }
}

/// Build the first-person arm: a single skin box (plus its sleeve overlay) with
/// the grip (hand end) at the origin and the forearm running up +Y toward the
/// elbow. The app orients and places it in eye space each frame.
fn build_viewmodel_arm(device: &wgpu::Device, slim: bool) -> (wgpu::Buffer, u32) {
    let aw = if slim { 3.0f32 } else { 4.0 };
    let mut verts: Vec<TexVertex> = Vec::new();
    // Hand at y=0, forearm up to y=~10px; center the 10px-tall box at y=5.
    skin_box(&mut verts, [0.0, 5.0, 0.0], [aw, 10.0, 4.0], [40.0, 20.0], 0.0);
    // Sleeve overlay (jacket sleeve region), slightly inflated.
    skin_box(&mut verts, [0.0, 5.0, 0.0], [aw, 10.0, 4.0], [40.0, 36.0], 0.25);
    let count = verts.len() as u32;
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(if slim { "vm-arm-slim" } else { "vm-arm-wide" }),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    (vbuf, count)
}

/// A unit quad in the XY plane facing +Z with UV 0..1 (two triangles), used for
/// the sun and moon billboards.
fn build_sky_quad(device: &wgpu::Device) -> wgpu::Buffer {
    let v = |x: f32, y: f32, u: f32, w: f32| TexVertex { pos: [x, y, 0.0], uv: [u, w] };
    let verts = [
        v(-1.0, -1.0, 0.0, 1.0),
        v(1.0, -1.0, 1.0, 1.0),
        v(1.0, 1.0, 1.0, 0.0),
        v(-1.0, -1.0, 0.0, 1.0),
        v(1.0, 1.0, 1.0, 0.0),
        v(-1.0, 1.0, 0.0, 0.0),
    ];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sky-quad"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    })
}

/// Build the star field: `count` small camera-facing quads at deterministic
/// pseudo-random directions on the upper part of the sphere, each already
/// oriented toward the origin so a single model matrix (the sky rotation) draws
/// them all. Positions are camera-relative at `SKY_DIST`.
fn build_star_mesh(device: &wgpu::Device, count: usize) -> (wgpu::Buffer, u32) {
    // Tiny deterministic PRNG (Math::random() is unavailable and would break the
    // resume-friendly determinism anyway).
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f32 / (1u64 << 53) as f32
    };
    let mut verts: Vec<TexVertex> = Vec::with_capacity(count * 6);
    for _ in 0..count {
        // Uniform direction on the sphere.
        let u = next() * 2.0 - 1.0; // cos(theta)
        let phi = next() * std::f32::consts::TAU;
        let r = (1.0 - u * u).max(0.0).sqrt();
        let dir = Vec3::new(r * phi.cos(), u, r * phi.sin());
        let center = dir * SKY_DIST;
        // Billboard basis facing the origin (normal = -dir). Small quads so the
        // stars read as points, not squares.
        let right = dir.cross(Vec3::Y).normalize_or(Vec3::X) * (0.13 + next() * 0.22);
        let up = right.normalize_or(Vec3::X).cross(dir) * right.length();
        let quad = [
            (center - right - up, [0.0, 1.0]),
            (center + right - up, [1.0, 1.0]),
            (center + right + up, [1.0, 0.0]),
            (center - right - up, [0.0, 1.0]),
            (center + right + up, [1.0, 0.0]),
            (center - right + up, [0.0, 0.0]),
        ];
        for (p, uv) in quad {
            verts.push(TexVertex { pos: [p.x, p.y, p.z], uv });
        }
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("star-mesh"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    (vbuf, verts.len() as u32)
}

/// Append the first-person held-item quad: a flat, double-sided square in the XY
/// plane centered at the origin, `half` blocks to a side, textured with `uv`.
/// Double-sided so it stays visible as the swing arc turns it away.
fn push_viewmodel_item(out: &mut Vec<TexVertex>, uv: [f32; 4], half: f32) {
    let [u0, v0, u1, v1] = uv;
    let corners = [
        ([-half, -half, 0.0], [u0, v1]),
        ([half, -half, 0.0], [u1, v1]),
        ([half, half, 0.0], [u1, v0]),
        ([-half, half, 0.0], [u0, v0]),
    ];
    // Front (CCW) then back (CW) windings so both faces show under back-face cull.
    for &(a, b, c) in &[(0usize, 1, 2), (0, 2, 3), (0, 2, 1), (0, 3, 2)] {
        for idx in [a, b, c] {
            let (p, t) = corners[idx];
            out.push(TexVertex { pos: p, uv: t });
        }
    }
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
    // Armor has no separate overlay layer.
    SkinMesh { vbuf, parts: ranges, overlay: [(0, 0); 6], pivots }
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
    y_rot: f32,
    z_rot: f32,
) {
    let (w, h, d) = (size[0], size[1], size[2]);
    let hx = (w / 2.0 + inflate) * scale;
    let hy = (h / 2.0 + inflate) * scale;
    let hz = (d / 2.0 + inflate) * scale;
    let c = [center[0] * scale, center[1] * scale, center[2] * scale];
    let (u0, v0) = (uv[0], uv[1]);
    let (tw, th) = (tex[0], tex[1]);
    let (sinx, cosx) = x_rot.sin_cos();
    let (siny, cosy) = y_rot.sin_cos();
    let (sinz, cosz) = z_rot.sin_cos();

    let mut quad = |p: [([f32; 3], [f32; 2]); 4]| {
        for i in [0usize, 1, 2, 0, 2, 3] {
            let (pos, uvp) = p[i];
            let (px, py, pz) = (c[0] + pos[0], c[1] + pos[1], c[2] + pos[2]);
            // Bake the fixed rotations about the pivot (origin of these coords),
            // in vanilla ModelPart order (X, then Y, then Z applied to the point):
            // x_rot lays flat bodies down, y_rot turns boat walls, z_rot rolls
            // legs out to the side (spider) or angles limbs.
            let (ry, rz) = (py * cosx - pz * sinx, py * sinx + pz * cosx);
            let (rx, rz) = (px * cosy + rz * siny, -px * siny + rz * cosy);
            let (rx, ry) = (rx * cosz - ry * sinz, rx * sinz + ry * cosz);
            out.push(TexVertex { pos: [rx, ry, rz], uv: [uvp[0] / tw, uvp[1] / th] });
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
                        part.y_rot,
                        part.z_rot,
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

/// Append a held-item sprite as a small cross of two perpendicular quads gripped
/// in a fist, in skin-pixel space relative to the arm's shoulder pivot (so it
/// swings and sneaks with the arm). `right` picks the hand and `slim` the arm
/// width so the sprite sits centered on the actual forearm; `uv` is the atlas
/// rect. The sprite tilts forward out of the fist (vanilla presents the item
/// ahead of the hand rather than flat against the leg).
fn push_item_quad(out: &mut Vec<TexVertex>, right: bool, slim: bool, uv: [f32; 4]) {
    let arm_x = if slim { 0.5 } else { 1.0 };
    // Fist point: centered on the forearm, just past its lower end, forward of
    // the front face so the item reads as "held out".
    let (cx, cy, cz) = (if right { -arm_x } else { arm_x }, -11.5, 4.5);
    let h = 4.0; // ~0.5-block sprite
    // Forward tilt about local X so the item points ahead-and-down out of the
    // fist instead of lying axis-aligned.
    let (sin, cos) = 0.45f32.sin_cos();
    let [u0, v0, u1, v1] = uv;
    // A quad in a plane, given its four corners as (dy, dz) offsets from centre
    // (x fixed) or (dx, dy) offsets (z fixed); we build both to form a cross.
    let tilt = |dy: f32, dz: f32| -> (f32, f32) {
        // Rotate (dy, dz) about X by the tilt so the sprite leans forward.
        (dy * cos - dz * sin, dy * sin + dz * cos)
    };
    // Facing ±x (side view — the common angle you see other players): the
    // sprite's height runs down the arm and tilts forward.
    let px = |sy: f32, sz: f32| {
        let (ry, rz) = tilt(sy, sz);
        [cx, cy + ry, cz + rz]
    };
    let quad_x = [
        (px(-h, -h), [u0, v1]),
        (px(-h, h), [u1, v1]),
        (px(h, h), [u1, v0]),
        (px(-h, -h), [u0, v1]),
        (px(h, h), [u1, v0]),
        (px(h, -h), [u0, v0]),
    ];
    // Facing ±z (front): vary (x,y), still tilted forward.
    let pz = |sx: f32, sy: f32| {
        let (ry, rz) = tilt(sy, 0.0);
        [cx + sx, cy + ry, cz + rz]
    };
    let quad_z = [
        (pz(-h, -h), [u0, v1]),
        (pz(h, -h), [u1, v1]),
        (pz(h, h), [u1, v0]),
        (pz(-h, -h), [u0, v1]),
        (pz(h, h), [u1, v0]),
        (pz(-h, h), [u0, v0]),
    ];
    for (p, uvp) in quad_x.into_iter().chain(quad_z) {
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

/// Build a flat wall slab `w`×`h` blocks, 1/16 thick, in the canonical frame
/// (front art face on +Z). Returns the vertex ranges for the front art face and
/// for the wooden back + four edges, so each can be drawn with its own texture.
/// Callers place/orient it via the entity model matrix.
fn push_flat_slab(out: &mut Vec<TexVertex>, w: f32, h: f32) -> ((u32, u32), (u32, u32)) {
    let (hw, hh, t) = (w * 0.5, h * 0.5, 0.5 / 16.0);
    // Push a quad from four corners (CCW as seen from outside), uv rect
    // [u0,v0,u1,v1] mapped corner-for-corner (top-left origin).
    fn quad(out: &mut Vec<TexVertex>, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], uv: [f32; 4]) {
        let v = [
            TexVertex { pos: a, uv: [uv[0], uv[3]] },
            TexVertex { pos: b, uv: [uv[2], uv[3]] },
            TexVertex { pos: c, uv: [uv[2], uv[1]] },
            TexVertex { pos: d, uv: [uv[0], uv[1]] },
        ];
        out.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    }
    let full = [0.0, 0.0, 1.0, 1.0];
    // Front art face (+Z), CCW from +Z.
    let fstart = out.len() as u32;
    quad(out, [-hw, -hh, t], [hw, -hh, t], [hw, hh, t], [-hw, hh, t], full);
    let fcount = out.len() as u32 - fstart;
    // Back (−Z) + the four thin edges, wooden texture stretched.
    let bstart = out.len() as u32;
    quad(out, [hw, -hh, -t], [-hw, -hh, -t], [-hw, hh, -t], [hw, hh, -t], full); // back −Z
    quad(out, [-hw, hh, t], [hw, hh, t], [hw, hh, -t], [-hw, hh, -t], full); // top +Y
    quad(out, [-hw, -hh, -t], [hw, -hh, -t], [hw, -hh, t], [-hw, -hh, t], full); // bottom −Y
    quad(out, [-hw, -hh, -t], [-hw, -hh, t], [-hw, hh, t], [-hw, hh, -t], full); // left −X
    quad(out, [hw, -hh, t], [hw, -hh, -t], [hw, hh, -t], [hw, hh, t], full); // right +X
    let bcount = out.len() as u32 - bstart;
    ((fstart, fcount), (bstart, bcount))
}

/// A single flat unit quad in the XY plane facing +Z (item-frame content). The
/// model matrix scales/orients it; `uv` is the item-atlas rect (v downwards).
fn push_flat_item(out: &mut Vec<TexVertex>, uv: [f32; 4]) -> (u32, u32) {
    let [u0, v0, u1, v1] = uv;
    let start = out.len() as u32;
    let v = [
        TexVertex { pos: [-0.5, -0.5, 0.0], uv: [u0, v1] },
        TexVertex { pos: [0.5, -0.5, 0.0], uv: [u1, v1] },
        TexVertex { pos: [0.5, 0.5, 0.0], uv: [u1, v0] },
        TexVertex { pos: [-0.5, 0.5, 0.0], uv: [u0, v0] },
    ];
    out.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    (start, out.len() as u32 - start)
}

/// Like [`push_flat_item`] but double-sided: the same unit quad wound both ways
/// so it shows from either side. Item-display entities carry a free transform,
/// so their icon must never vanish behind back-face culling at some angle.
fn push_flat_item_double(out: &mut Vec<TexVertex>, uv: [f32; 4]) -> (u32, u32) {
    let [u0, v0, u1, v1] = uv;
    let start = out.len() as u32;
    let v = [
        TexVertex { pos: [-0.5, -0.5, 0.0], uv: [u0, v1] },
        TexVertex { pos: [0.5, -0.5, 0.0], uv: [u1, v1] },
        TexVertex { pos: [0.5, 0.5, 0.0], uv: [u1, v0] },
        TexVertex { pos: [-0.5, 0.5, 0.0], uv: [u0, v0] },
    ];
    // Front (CCW from +Z) then back (reversed winding).
    out.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    out.extend_from_slice(&[v[0], v[2], v[1], v[0], v[3], v[2]]);
    (start, out.len() as u32 - start)
}

/// Rotation mapping the canonical +Z-facing flat entity onto a wall/floor facing
/// the given vanilla Direction (0 Down, 1 Up, 2 N, 3 S, 4 W, 5 E).
fn facing_rot(facing: u8) -> Mat4 {
    use std::f32::consts::{FRAC_PI_2, PI};
    match facing {
        0 => Mat4::from_rotation_x(FRAC_PI_2),  // Down: face −Y
        1 => Mat4::from_rotation_x(-FRAC_PI_2), // Up: face +Y
        2 => Mat4::from_rotation_y(PI),         // North: face −Z
        4 => Mat4::from_rotation_y(-FRAC_PI_2), // West: face −X
        5 => Mat4::from_rotation_y(FRAC_PI_2),  // East: face +X
        _ => Mat4::IDENTITY,                    // South (3): face +Z
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
    /// Block selection outline (LineList over the entity shader, alpha-blended).
    pipe_outline: wgpu::RenderPipeline,
    pipe_skin: wgpu::RenderPipeline,
    /// First-person view model: skin shader with depth test/write disabled so
    /// the hand + held item always draw on top of the world (vanilla clears the
    /// depth buffer for the same effect).
    pipe_viewmodel: wgpu::RenderPipeline,
    /// Celestial sky (sun/moon/stars): skin bind-group layout, alpha-blended, no
    /// depth/fog.
    pipe_sky: wgpu::RenderPipeline,
    /// Cloud layer: alpha-blended, depth-tested (terrain occludes it) but no
    /// depth write.
    pipe_clouds: wgpu::RenderPipeline,
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
    /// Unit-cube edges (LineList) for the selection outline.
    cube_lines_vbuf: wgpu::Buffer,
    /// Unit cube with full-face UVs for the mining crack overlay.
    crack_vbuf: wgpu::Buffer,
    /// destroy_stage_0..9 textures (bind groups); empty until loaded.
    crack_tex: Vec<wgpu::BindGroup>,
    skin_mesh_wide: SkinMesh,
    skin_mesh_slim: SkinMesh,
    /// First-person arm meshes (grip at origin, forearm along +Y), wide + slim.
    /// `(buffer, vertex_count)` — a single skin box with its sleeve overlay.
    vm_arm_wide: (wgpu::Buffer, u32),
    vm_arm_slim: (wgpu::Buffer, u32),
    /// Unit quad (XY plane, +Z normal, UV 0..1) for the sun/moon billboards.
    sky_quad: wgpu::Buffer,
    /// Star field: many small quads on a sphere; drawn as one call rotated by
    /// the sky matrix. `(buffer, vertex_count)`.
    star_mesh: (wgpu::Buffer, u32),
    /// Sun texture bind group (loaded from the jar; None until uploaded).
    sun_tex: Option<wgpu::BindGroup>,
    /// Moon phase texture bind groups (0..7); empty until uploaded.
    moon_tex: Vec<wgpu::BindGroup>,
    /// 2×2 white texture for the star quads (tinted per draw).
    white_tex: Option<wgpu::BindGroup>,
    /// Cloud texture bind group (tiled with a repeat sampler); None until loaded.
    cloud_tex: Option<wgpu::BindGroup>,
    /// Repeat sampler for the tiling cloud plane.
    cloud_sampler: wgpu::Sampler,
    /// Soft radial glow texture for the sunrise/sunset haze around the sun.
    glow_tex: Option<wgpu::BindGroup>,
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
    particle_atlas: Option<wgpu::BindGroup>,
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

        // Selection outline: the entity pipeline reduced to lines with alpha
        // blending and no depth writes (vanilla thin black box, α 0.4).
        let pipe_outline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline"),
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
                topology: wgpu::PrimitiveTopology::LineList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        // First-person view model: same skin shader, but depth test/write off so
        // the hand + item always draw over the world (never clip into terrain).
        let pipe_viewmodel = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewmodel"),
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
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
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

        // Celestial sky: alpha-blended, no depth, no cull. Shares the skin bind
        // group layout (globals / texture / model+color).
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
        });
        let pipe_sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&skin_pl),
            vertex: wgpu::VertexState {
                module: &sky_shader,
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
                module: &sky_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    // Additive: the celestial textures are opaque with dark
                    // backgrounds (vanilla renders them additively), so dark
                    // pixels add nothing and only the bright disc/stars glow.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // Cloud layer: alpha-blended, depth-tested so terrain occludes it, but
        // no depth write (translucent water still blends over it).
        let cloud_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cloud-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipe_clouds = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clouds"),
            layout: Some(&skin_pl),
            vertex: wgpu::VertexState {
                module: &sky_shader,
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
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        let cube_lines_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("unit-cube-lines"),
            contents: bytemuck::cast_slice(&unit_cube_line_vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let crack_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("crack-cube"),
            contents: bytemuck::cast_slice(&crack_cube_vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let skin_mesh_wide = build_skin_mesh(&device, false);
        let skin_mesh_slim = build_skin_mesh(&device, true);
        let vm_arm_wide = build_viewmodel_arm(&device, false);
        let vm_arm_slim = build_viewmodel_arm(&device, true);
        let sky_quad = build_sky_quad(&device);
        let star_mesh = build_star_mesh(&device, 450);
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
            pipe_outline,
            pipe_skin,
            pipe_viewmodel,
            pipe_sky,
            pipe_clouds,
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
            cube_lines_vbuf,
            crack_vbuf,
            crack_tex: Vec::new(),
            skin_mesh_wide,
            skin_mesh_slim,
            vm_arm_wide,
            vm_arm_slim,
            sky_quad,
            star_mesh,
            sun_tex: None,
            moon_tex: Vec::new(),
            white_tex: None,
            cloud_tex: None,
            cloud_sampler,
            glow_tex: None,
            armor_mesh_outer,
            armor_mesh_inner,
            mob_meshes,
            skins: HashMap::new(),
            armor_tex: HashMap::new(),
            item_atlas: None,
            particle_atlas: None,
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

    /// Upload the mining crack textures (destroy_stage_0..9, in order). Until
    /// called, the crack overlay simply doesn't draw.
    pub fn set_crack_textures(&mut self, images: &[image::RgbaImage]) {
        self.crack_tex = images
            .iter()
            .filter(|img| img.width() > 0 && img.height() > 0)
            .map(|img| {
                make_atlas_bind_group(
                    &self.device,
                    &self.queue,
                    &self.atlas_layout,
                    &self.atlas_sampler,
                    img.width(),
                    img.height(),
                    img.as_raw(),
                )
            })
            .collect();
    }

    /// Upload the celestial textures: the sun, the moon phases (in phase order
    /// 0..7), and the tiling cloud texture. Also mints a white pixel for the
    /// star field. Until called, the sky is just its flat color.
    pub fn set_sky_textures(
        &mut self,
        sun: &image::RgbaImage,
        moons: &[image::RgbaImage],
        clouds: Option<&image::RgbaImage>,
    ) {
        let mk = |img: &image::RgbaImage, sampler: &wgpu::Sampler| {
            make_atlas_bind_group(
                &self.device,
                &self.queue,
                &self.atlas_layout,
                sampler,
                img.width(),
                img.height(),
                img.as_raw(),
            )
        };
        if sun.width() > 0 && sun.height() > 0 {
            self.sun_tex = Some(mk(sun, &self.atlas_sampler));
        }
        self.moon_tex = moons
            .iter()
            .filter(|m| m.width() > 0 && m.height() > 0)
            .map(|m| mk(m, &self.atlas_sampler))
            .collect();
        let white = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]));
        self.white_tex = Some(mk(&white, &self.atlas_sampler));
        if let Some(c) = clouds.filter(|c| c.width() > 0 && c.height() > 0) {
            self.cloud_tex = Some(mk(c, &self.cloud_sampler));
        }
        // Soft radial glow (white center → transparent edge) for the sunrise/
        // sunset haze drawn additively around the sun.
        let n = 64u32;
        let glow = image::RgbaImage::from_fn(n, n, |x, y| {
            let (dx, dy) = (x as f32 / n as f32 - 0.5, y as f32 / n as f32 - 0.5);
            let d = (dx * dx + dy * dy).sqrt() / 0.5; // 0 center, 1 edge
            let a = (1.0 - d).clamp(0.0, 1.0).powf(2.2);
            image::Rgba([255, 255, 255, (a * 255.0) as u8])
        });
        self.glow_tex = Some(mk(&glow, &self.atlas_sampler));
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

    /// Upload the particle sprite atlas (billboarded particles sample it).
    pub fn ensure_particle_atlas(&mut self, image: &image::RgbaImage) {
        if self.particle_atlas.is_some() || image.width() == 0 || image.height() == 0 {
            return;
        }
        self.particle_atlas = Some(make_atlas_bind_group(
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
        let mut zfar = (scene.fog_end + ZNEAR_SLACK).max(64.0);
        // Keep the overhead cloud layer inside the far plane even at low render
        // distance (otherwise it clips away when the player looks up).
        if scene.sky.is_some() {
            zfar = zfar.max((CLOUD_HEIGHT - scene.cam_pos[1] as f32).abs() + 96.0);
        }
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
            SkinPart { key: u64, slim: bool, part: usize, overlay: bool },
            /// One armor part: `mat` = material id, `leggings` picks the texture
            /// layer, `inner` picks the thinner mesh (leggings vs outer).
            ArmorPart { mat: u8, leggings: bool, inner: bool, part: usize },
            /// A held item sprite: vertex range into `item_verts`.
            ItemQuad { start: u32, count: u32 },
            /// A dropped 3D block: vertex range into `item_verts`, block atlas.
            DropBlock { start: u32, count: u32 },
            /// One part of a prebuilt mob model: `model` picks the mesh, `key`
            /// the texture, `part` the vertex range.
            MobPart { model: MobModel, key: u64, part: usize },
            /// A flat textured quad list (painting front / back / edges): vertex
            /// range into `item_verts`, drawn with `skins[key]` via pipe_skin.
            FlatTex { start: u32, count: u32, key: u64 },
            /// A camera-facing particle billboard: vertex range into `item_verts`,
            /// drawn with the particle atlas via the alpha-blended cloud pipeline.
            ParticleQuad { start: u32, count: u32 },
            /// Selection outline box (LineList unit cube).
            Outline,
            /// Mining crack overlay cube with destroy stage 0..=9.
            Crack { stage: usize },
            /// First-person arm (own hand), drawn last with the view-model pipeline.
            ViewArm { key: u64, slim: bool },
            /// First-person held item quad: vertex range into `item_verts`.
            ViewItem { start: u32, count: u32 },
            /// First-person held 3D block: vertex range into `item_verts`, drawn
            /// with the block atlas.
            ViewBlock { start: u32, count: u32 },
            /// Sun billboard (sky quad, sun texture).
            Sun,
            /// Moon billboard (sky quad, phase texture).
            Moon { phase: usize },
            /// Star field (whole star mesh, one call).
            Stars,
            /// Cloud plane: vertex range into `item_verts`.
            Clouds { start: u32, count: u32 },
            /// Sunrise/sunset glow billboard (sky quad, glow texture).
            Glow,
        }
        let mut slots: Vec<[u8; 80]> = Vec::new();
        let mut cmds: Vec<EntityCmd> = Vec::new();
        // Held-item sprites accumulate here; uploaded once as a dynamic buffer.
        let mut item_verts: Vec<TexVertex> = Vec::new();
        // Camera-facing basis for particle billboards (world space, shared by
        // every particle this frame): `right` = viewer's right, `up` = viewer's up.
        let cam_fwd = camera::view_dir(scene.yaw, scene.pitch);
        let bb_right = Vec3::Y.cross(cam_fwd).normalize_or_zero();
        let bb_up = cam_fwd.cross(bb_right).normalize_or_zero();
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
                EntityDrawKind::Player { skin, slim, swing, attack_swing, sneaking, skin_layers, head_pitch, armor, main_hand, off_hand } => {
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
                    // Vanilla sneak: the upper body (head/chest/arms) leans
                    // forward ~0.5 rad about the waist while the legs stay
                    // planted. `part_matrix` bakes that lean into the upper parts.
                    let waist = Vec3::Y * (12.0 * SKIN_PX);
                    let sneak = if sneaking { 0.5f32 } else { 0.0 };
                    let upper =
                        |p: usize| matches!(p, PART_HEAD | PART_BODY | PART_RIGHT_ARM | PART_LEFT_ARM);
                    // Per-part limb angle (arms/legs swing in opposite pairs). An
                    // attack swing adds a forward sweep to the main (right) arm.
                    // The head counter-rotates the sneak lean so it stays level
                    // (moved forward with the body but still looking ahead).
                    let part_angle = |part: usize| match part {
                        PART_HEAD => head_pitch.to_radians() - sneak,
                        PART_RIGHT_ARM => swing - attack_swing,
                        PART_LEFT_ARM => -swing,
                        PART_RIGHT_LEG => -swing,
                        PART_LEFT_LEG => swing,
                        _ => 0.0,
                    };
                    let part_matrix = |pivot: Vec3, part: usize, angle: f32| -> Mat4 {
                        if sneak != 0.0 && upper(part) {
                            rot * Mat4::from_translation(waist)
                                * Mat4::from_rotation_x(sneak)
                                * Mat4::from_translation(pivot - waist)
                                * Mat4::from_rotation_x(angle)
                        } else {
                            rot * Mat4::from_translation(pivot) * Mat4::from_rotation_x(angle)
                        }
                    };
                    for part in 0..6 {
                        let model = part_matrix(mesh.pivots[part], part, part_angle(part));
                        push(
                            model,
                            [1.0, 1.0, 1.0, 1.0],
                            EntityCmd::SkinPart { key, slim, part, overlay: false },
                        );
                        // Overlay layer (hat/jacket/sleeve/pants), if this part's
                        // customization bit is on. Same matrix as the base part.
                        if skin_layers & (1 << part) != 0 {
                            push(
                                model,
                                [1.0, 1.0, 1.0, 1.0],
                                EntityCmd::SkinPart { key, slim, part, overlay: true },
                            );
                        }
                    }

                    // Armor layers over the model. Each slot maps to a set of
                    // parts, a texture layer (humanoid vs leggings) and a mesh
                    // thickness. Drawn only when the texture is loaded so a
                    // missing/unknown material simply shows no armor (never garbage).
                    // (armor slot, parts, leggings-layer, inner-mesh)
                    let groups: [(usize, &[usize], bool, bool); 4] = [
                        (0, &[PART_HEAD], false, false), // helmet
                        (1, &[PART_BODY, PART_RIGHT_ARM, PART_LEFT_ARM], false, false), // chestplate
                        (2, &[PART_BODY, PART_RIGHT_LEG, PART_LEFT_LEG], true, true), // leggings
                        // Boots use the layer_1 (humanoid) texture like vanilla:
                        // its leg region rows 26-31 hold the boot pixels; the
                        // leggings (layer_2) texture has none there, which is
                        // why boots never showed while this said `true`.
                        (3, &[PART_RIGHT_LEG, PART_LEFT_LEG], false, false), // boots
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
                            let model = part_matrix(amesh.pivots[part], part, part_angle(part));
                            push(
                                model,
                                [1.0, 1.0, 1.0, 1.0],
                                EntityCmd::ArmorPart { mat: mat_id, leggings, inner, part },
                            );
                        }
                    }

                    // Held items: a small 3D sprite in each fist, swinging with
                    // the arm (and leaning with the body when sneaking).
                    if self.item_atlas.is_some() {
                        for (uv, arm_part, right) in [
                            (main_hand, PART_RIGHT_ARM, true),
                            (off_hand, PART_LEFT_ARM, false),
                        ] {
                            let Some(uv) = uv else { continue };
                            let model =
                                part_matrix(mesh.pivots[arm_part], arm_part, part_angle(arm_part));
                            let start = item_verts.len() as u32;
                            push_item_quad(&mut item_verts, right, slim, uv);
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
                EntityDrawKind::ItemBlock { ref quads } => {
                    // A small spinning 3D block, floating like vanilla item-drops.
                    let model = Mat4::from_translation(base + Vec3::Y * 0.22)
                        * Mat4::from_rotation_y(-e.yaw.to_radians())
                        * Mat4::from_scale(Vec3::splat(0.30));
                    let start = item_verts.len() as u32;
                    for &(p, uv) in quads.iter() {
                        item_verts.push(TexVertex { pos: p, uv });
                    }
                    let count = item_verts.len() as u32 - start;
                    if count > 0 {
                        push(model, [1.0, 1.0, 1.0, 1.0], EntityCmd::DropBlock { start, count });
                    }
                }
                EntityDrawKind::Mob { tex, model, swing, head_pitch, scale } => {
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
                    // Scale about the feet (base), then place/animate each part.
                    let rot = Mat4::from_translation(base)
                        * Mat4::from_rotation_y(-e.yaw.to_radians())
                        * Mat4::from_scale(Vec3::splat(scale.max(0.05)));
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
                EntityDrawKind::Painting { art_tex, back_tex, w, h, facing } => {
                    if !self.skins.contains_key(&art_tex) {
                        continue;
                    }
                    // Canonical slab faces +Z; rotate onto the wall direction.
                    let model = Mat4::from_translation(base) * facing_rot(facing);
                    let (front, back) = push_flat_slab(&mut item_verts, w, h);
                    push(model, [1.0, 1.0, 1.0, 1.0],
                        EntityCmd::FlatTex { start: front.0, count: front.1, key: art_tex });
                    if self.skins.contains_key(&back_tex) {
                        push(model, [1.0, 1.0, 1.0, 1.0],
                            EntityCmd::FlatTex { start: back.0, count: back.1, key: back_tex });
                    }
                }
                EntityDrawKind::ItemFrame { frame_tex, back_tex, facing, rot, item_uv, ref block_quads } => {
                    if !self.skins.contains_key(&frame_tex) {
                        continue;
                    }
                    let model = Mat4::from_translation(base) * facing_rot(facing);
                    // Frame face + wooden back/edges (a 1×1 slab).
                    let (front, back) = push_flat_slab(&mut item_verts, 1.0, 1.0);
                    push(model, [1.0, 1.0, 1.0, 1.0],
                        EntityCmd::FlatTex { start: front.0, count: front.1, key: frame_tex });
                    if self.skins.contains_key(&back_tex) {
                        push(model, [1.0, 1.0, 1.0, 1.0],
                            EntityCmd::FlatTex { start: back.0, count: back.1, key: back_tex });
                    }
                    // Contained item: sits just in front of the frame face,
                    // rotated in the frame plane by rot·45°.
                    let outset = 0.5 / 16.0 + 0.02;
                    let item_base = model
                        * Mat4::from_rotation_z(rot as f32 * std::f32::consts::FRAC_PI_4)
                        * Mat4::from_translation(Vec3::Z * outset);
                    if !block_quads.is_empty() {
                        // A small 3D block, drawn with the block atlas.
                        let m = item_base * Mat4::from_scale(Vec3::splat(0.42));
                        let start = item_verts.len() as u32;
                        for &(p, uv) in block_quads.iter() {
                            item_verts.push(TexVertex { pos: p, uv });
                        }
                        let count = item_verts.len() as u32 - start;
                        if count > 0 {
                            push(m, [1.0, 1.0, 1.0, 1.0], EntityCmd::DropBlock { start, count });
                        }
                    } else if let Some(uv) = item_uv {
                        // A flat item icon, drawn with the item atlas.
                        let m = item_base * Mat4::from_scale(Vec3::splat(0.5));
                        let (start, count) = push_flat_item(&mut item_verts, uv);
                        push(m, [1.0, 1.0, 1.0, 1.0], EntityCmd::ItemQuad { start, count });
                    }
                }
                EntityDrawKind::Particle { uv, color, size } => {
                    if self.particle_atlas.is_none() {
                        continue;
                    }
                    // Camera-facing quad in camera-relative world space; the
                    // per-slot matrix is identity, colour carries the tint.
                    let (hw, hh) = (size * 0.5, size * 0.5);
                    let r = bb_right * hw;
                    let u = bb_up * hh;
                    let [u0, v0, u1, v1] = uv;
                    let tl = TexVertex { pos: (base - r + u).into(), uv: [u0, v0] };
                    let tr = TexVertex { pos: (base + r + u).into(), uv: [u1, v0] };
                    let br = TexVertex { pos: (base + r - u).into(), uv: [u1, v1] };
                    let bl = TexVertex { pos: (base - r - u).into(), uv: [u0, v1] };
                    let start = item_verts.len() as u32;
                    item_verts.extend_from_slice(&[tl, bl, br, tl, br, tr]);
                    let count = item_verts.len() as u32 - start;
                    push(
                        Mat4::IDENTITY,
                        [color[0], color[1], color[2], 1.0],
                        EntityCmd::ParticleQuad { start, count },
                    );
                }
                EntityDrawKind::Projectile { tex, yaw, pitch } => {
                    if !self.skins.contains_key(&tex) {
                        continue;
                    }
                    // The arrow lies along local +Z (tip forward); orient it by
                    // yaw then pitch to point along its flight direction.
                    let model = Mat4::from_translation(base)
                        * Mat4::from_rotation_y(-yaw.to_radians())
                        * Mat4::from_rotation_x(pitch.to_radians());
                    // Two crossed planes using the arrow's side-profile strip
                    // (top of arrow.png: u 0..1 length, v 0..5/32 width). Each
                    // plane is emitted both windings so it shows from either side.
                    let (hl, hw) = (0.45f32, 0.11f32);
                    // Emit a quad both windings (pipe_skin culls Back) with the
                    // arrow side-profile strip mapped corner-for-corner.
                    fn quad(out: &mut Vec<TexVertex>, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) {
                        const UV: [f32; 4] = [0.0, 0.0, 1.0, 5.0 / 32.0];
                        let v = [
                            TexVertex { pos: a, uv: [UV[0], UV[3]] },
                            TexVertex { pos: b, uv: [UV[2], UV[3]] },
                            TexVertex { pos: c, uv: [UV[2], UV[1]] },
                            TexVertex { pos: d, uv: [UV[0], UV[1]] },
                        ];
                        out.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
                        out.extend_from_slice(&[v[0], v[2], v[1], v[0], v[3], v[2]]);
                    }
                    let start = item_verts.len() as u32;
                    // Horizontal plane (spans X across the shaft, length along Z).
                    quad(&mut item_verts, [-hw, 0.0, -hl], [hw, 0.0, -hl], [hw, 0.0, hl], [-hw, 0.0, hl]);
                    // Vertical plane (spans Y).
                    quad(&mut item_verts, [0.0, -hw, -hl], [0.0, hw, -hl], [0.0, hw, hl], [0.0, -hw, hl]);
                    let count = item_verts.len() as u32 - start;
                    push(model, [1.0, 1.0, 1.0, 1.0], EntityCmd::FlatTex { start, count, key: tex });
                }
                EntityDrawKind::StaticBlock { ref quads, y_off, scale, flash } => {
                    // A block cube sitting on the entity position (no spin),
                    // optionally brightened toward white by the flash.
                    let model = Mat4::from_translation(base + Vec3::Y * y_off)
                        * Mat4::from_scale(Vec3::splat(scale));
                    let start = item_verts.len() as u32;
                    for &(p, uv) in quads.iter() {
                        item_verts.push(TexVertex { pos: p, uv });
                    }
                    let count = item_verts.len() as u32 - start;
                    if count > 0 {
                        let b = 1.0 + flash;
                        push(model, [b, b, b, 1.0], EntityCmd::DropBlock { start, count });
                    }
                }
                EntityDrawKind::DisplayBlock { ref quads, translation, scale, left_rot, right_rot } => {
                    // Vanilla display transform: T · Lrot · S · Rrot, about the
                    // entity position. Block quads are corner-origin (0..1).
                    let model = Mat4::from_translation(base + Vec3::from_array(translation))
                        * Mat4::from_quat(Quat::from_array(left_rot))
                        * Mat4::from_scale(Vec3::from_array(scale))
                        * Mat4::from_quat(Quat::from_array(right_rot));
                    let start = item_verts.len() as u32;
                    for &(p, uv) in quads.iter() {
                        item_verts.push(TexVertex { pos: p, uv });
                    }
                    let count = item_verts.len() as u32 - start;
                    if count > 0 {
                        push(model, [1.0, 1.0, 1.0, 1.0], EntityCmd::DropBlock { start, count });
                    }
                }
                EntityDrawKind::DisplayItem { uv, translation, scale, left_rot, right_rot } => {
                    if self.item_atlas.is_none() {
                        continue;
                    }
                    let model = Mat4::from_translation(base + Vec3::from_array(translation))
                        * Mat4::from_quat(Quat::from_array(left_rot))
                        * Mat4::from_scale(Vec3::from_array(scale))
                        * Mat4::from_quat(Quat::from_array(right_rot));
                    let (start, count) = push_flat_item_double(&mut item_verts, uv);
                    push(model, [1.0, 1.0, 1.0, 1.0], EntityCmd::ItemQuad { start, count });
                }
                EntityDrawKind::Orb { tex, size, color } => {
                    if !self.skins.contains_key(&tex) {
                        continue;
                    }
                    // A camera-facing sprite, corners baked in camera-relative
                    // world space (model = identity). Emitted both windings so it
                    // shows from any angle through the back-face-culling skin pipe.
                    let (hw, hh) = (size * 0.5, size * 0.5);
                    let r = bb_right * hw;
                    let u = bb_up * hh;
                    let tl = TexVertex { pos: (base - r + u).into(), uv: [0.0, 0.0] };
                    let tr = TexVertex { pos: (base + r + u).into(), uv: [1.0, 0.0] };
                    let br = TexVertex { pos: (base + r - u).into(), uv: [1.0, 1.0] };
                    let bl = TexVertex { pos: (base - r - u).into(), uv: [0.0, 1.0] };
                    let start = item_verts.len() as u32;
                    item_verts.extend_from_slice(&[tl, bl, br, tl, br, tr, tl, br, bl, tl, tr, br]);
                    let count = item_verts.len() as u32 - start;
                    push(
                        Mat4::IDENTITY,
                        [color[0], color[1], color[2], 1.0],
                        EntityCmd::FlatTex { start, count, key: tex },
                    );
                }
                EntityDrawKind::ArmorStandPosed { tex, scale, show_arms, show_base, poses } => {
                    if !self.skins.contains_key(&tex) {
                        continue;
                    }
                    let mesh = &self.mob_meshes[MobModel::ArmorStand.index()];
                    let root = Mat4::from_translation(base)
                        * Mat4::from_rotation_y(-e.yaw.to_radians())
                        * Mat4::from_scale(Vec3::splat(scale.max(0.05)));
                    // Part order in the armour-stand model: 0 head, 1 body,
                    // 2 right arm, 3 left arm, 4 right leg, 5 left leg, 6 base.
                    for (pi, part) in mesh.parts.iter().enumerate() {
                        if (pi == 2 || pi == 3) && !show_arms {
                            continue;
                        }
                        if pi == 6 && !show_base {
                            continue;
                        }
                        let p = if pi < 6 { poses[pi] } else { [0.0, 0.0, 0.0] };
                        // Vanilla applies the pose as Rz·Ry·Rx; our models are
                        // Y-up (vanilla model space is Y-down), so Y and Z flip.
                        let euler = Mat4::from_rotation_z(-p[2].to_radians())
                            * Mat4::from_rotation_y(-p[1].to_radians())
                            * Mat4::from_rotation_x(p[0].to_radians());
                        let m = root * Mat4::from_translation(part.pivot) * euler;
                        push(m, [1.0, 1.0, 1.0, 1.0], EntityCmd::MobPart { model: MobModel::ArmorStand, key: tex, part: pi });
                    }
                }
                EntityDrawKind::Fire { tex, w, h, uv } => {
                    if !self.skins.contains_key(&tex) {
                        continue;
                    }
                    // Upright billboard: turns to face the viewer around Y but
                    // stays vertical. Bottom just under the feet, rising past the
                    // head; emitted both windings so it shows from any angle.
                    let r = bb_right * (w * 0.5);
                    let bottom = base - Vec3::Y * 0.02;
                    let top = base + Vec3::Y * h;
                    let [u0, v0, u1, v1] = uv;
                    let tl = TexVertex { pos: (top - r).into(), uv: [u0, v0] };
                    let tr = TexVertex { pos: (top + r).into(), uv: [u1, v0] };
                    let br = TexVertex { pos: (bottom + r).into(), uv: [u1, v1] };
                    let bl = TexVertex { pos: (bottom - r).into(), uv: [u0, v1] };
                    let start = item_verts.len() as u32;
                    item_verts.extend_from_slice(&[tl, bl, br, tl, br, tr, tl, br, bl, tl, tr, br]);
                    let count = item_verts.len() as u32 - start;
                    push(Mat4::IDENTITY, [1.0, 1.0, 1.0, 1.0], EntityCmd::FlatTex { start, count, key: tex });
                }
            }
        }
        // Selection outline + mining crack ride the same dynamic-slot pipeline
        // as entities (model matrix + color per draw, camera-relative).
        {
            let mut push_raw = |model: Mat4, color: [f32; 4], cmd: EntityCmd| {
                let mut bytes = [0u8; 80];
                bytes[..64].copy_from_slice(bytemuck::cast_slice(&model.to_cols_array()));
                bytes[64..].copy_from_slice(bytemuck::cast_slice(&color));
                slots.push(bytes);
                cmds.push(cmd);
            };
            // Vanilla outline: black, alpha 0.4, inflated 2 mm so it never
            // z-fights the block faces. The line cube is centered/unit.
            const INFLATE: f32 = 0.002;
            for (min, max) in &scene.outline {
                let size = Vec3::new(
                    (max[0] - min[0]) as f32 + 2.0 * INFLATE,
                    (max[1] - min[1]) as f32 + 2.0 * INFLATE,
                    (max[2] - min[2]) as f32 + 2.0 * INFLATE,
                );
                let center = Vec3::new(
                    ((min[0] + max[0]) * 0.5 - scene.cam_pos[0]) as f32,
                    ((min[1] + max[1]) * 0.5 - scene.cam_pos[1]) as f32,
                    ((min[2] + max[2]) * 0.5 - scene.cam_pos[2]) as f32,
                );
                push_raw(
                    Mat4::from_translation(center) * Mat4::from_scale(size),
                    [0.0, 0.0, 0.0, 0.4],
                    EntityCmd::Outline,
                );
            }
            if let Some((bmin, stage)) = scene.crack
                && !self.crack_tex.is_empty()
            {
                let stage = (stage as usize).min(self.crack_tex.len() - 1);
                let center = Vec3::new(
                    (bmin[0] + 0.5 - scene.cam_pos[0]) as f32,
                    (bmin[1] + 0.5 - scene.cam_pos[1]) as f32,
                    (bmin[2] + 0.5 - scene.cam_pos[2]) as f32,
                );
                push_raw(
                    Mat4::from_translation(center) * Mat4::from_scale(Vec3::splat(1.002)),
                    [1.0, 1.0, 1.0, 1.0],
                    EntityCmd::Crack { stage },
                );
            }
        }
        // --- celestial sky (sun / moon / stars) --------------------------------
        // Placed camera-relative at SKY_DIST, rotated about Z by the sun angle.
        // Slots go into the same entity uniform; drawn early (after the clear,
        // before terrain) so the world occludes them.
        if let Some(sky) = &scene.sky {
            let sky_rot = Mat4::from_rotation_z(sky.sun_angle);
            let mut push_sky = |model: Mat4, color: [f32; 4], cmd: EntityCmd| {
                let mut bytes = [0u8; 80];
                bytes[..64].copy_from_slice(bytemuck::cast_slice(&model.to_cols_array()));
                bytes[64..].copy_from_slice(bytemuck::cast_slice(&color));
                slots.push(bytes);
                cmds.push(cmd);
            };
            // Stars: the whole baked field, just rotated with the sky.
            if sky.star_brightness > 0.01 && self.white_tex.is_some() {
                push_sky(sky_rot, [1.0, 1.0, 1.0, sky.star_brightness], EntityCmd::Stars);
            }
            // Billboard a sky body at `local` (pre-rotation position), tinted
            // `color` (rgb) with `color[3]` as the strength.
            let mut body = |local: Vec3, size: f32, color: [f32; 4], cmd: EntityCmd| {
                if color[3] <= 0.01 {
                    return;
                }
                let pos = sky_rot.transform_point3(local);
                let dir = pos.normalize_or(Vec3::Y);
                let face = glam::Quat::from_rotation_arc(Vec3::Z, -dir);
                let model = Mat4::from_translation(pos)
                    * Mat4::from_quat(face)
                    * Mat4::from_scale(Vec3::splat(size));
                push_sky(model, color, cmd);
            };
            // Sunrise/sunset glow: a large soft haze at the sun, behind the disc.
            if self.glow_tex.is_some() {
                body(Vec3::new(0.0, SKY_DIST, 0.0), 52.0, sky.glow_color, EntityCmd::Glow);
            }
            if self.sun_tex.is_some() {
                body(Vec3::new(0.0, SKY_DIST, 0.0), 13.0, [1.0, 1.0, 1.0, sky.sun_alpha], EntityCmd::Sun);
            }
            if !self.moon_tex.is_empty() {
                let phase = sky.moon_phase.min(self.moon_tex.len() - 1);
                body(Vec3::new(0.0, -SKY_DIST, 0.0), 10.0, [1.0, 1.0, 1.0, sky.moon_alpha], EntityCmd::Moon { phase });
            }

            // Cloud plane: one big camera-relative quad at CLOUD_HEIGHT with
            // world-based, scrolling UVs so the clouds stay put as the player
            // moves and drift slowly with time.
            if self.cloud_tex.is_some() && sky.cloud_color[3] > 0.01 {
                let y = CLOUD_HEIGHT - scene.cam_pos[1] as f32;
                let (cx, cz) = (scene.cam_pos[0] as f32, scene.cam_pos[2] as f32);
                // UV maps one 12-block cell to one texel; the sampler repeats.
                let uv = |wx: f32, wz: f32| {
                    [
                        (wx / CLOUD_CELL + sky.cloud_scroll) / 256.0,
                        (wz / CLOUD_CELL) / 256.0,
                    ]
                };
                let e = CLOUD_EXTENT;
                let corner = |sx: f32, sz: f32| {
                    let (wx, wz) = (cx + sx * e, cz + sz * e);
                    (Vec3::new(sx * e, y, sz * e), uv(wx, wz))
                };
                let (p00, u00) = corner(-1.0, -1.0);
                let (p10, u10) = corner(1.0, -1.0);
                let (p11, u11) = corner(1.0, 1.0);
                let (p01, u01) = corner(-1.0, 1.0);
                let start = item_verts.len() as u32;
                for (p, t) in [
                    (p00, u00), (p10, u10), (p11, u11),
                    (p00, u00), (p11, u11), (p01, u01),
                ] {
                    item_verts.push(TexVertex { pos: [p.x, p.y, p.z], uv: t });
                }
                let count = item_verts.len() as u32 - start;
                let mut bytes = [0u8; 80];
                bytes[..64].copy_from_slice(bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()));
                bytes[64..].copy_from_slice(bytemuck::cast_slice(&sky.cloud_color));
                slots.push(bytes);
                cmds.push(EntityCmd::Clouds { start, count });
            }
        }

        // --- first-person view model (own hand + held item) --------------------
        // Placed in eye space (x right, y up, -z forward) then rotated back into
        // the camera-relative world frame the entity shader expects.
        if let Some(vm) = &scene.view_model {
            let f = camera::view_dir(scene.yaw, scene.pitch);
            let s = f.cross(Vec3::Y).normalize_or_zero();
            let u = s.cross(f);
            // view->world rotation (columns = eye-space basis in world space).
            let r_inv = Mat4::from_cols(
                s.extend(0.0),
                u.extend(0.0),
                (-f).extend(0.0),
                glam::Vec4::W,
            );
            let m = if vm.left_handed { -1.0 } else { 1.0 };
            let key = if self.skins.contains_key(&vm.skin) { vm.skin } else { 0 };
            let have_arm = self.skins.contains_key(&key);
            // Equip raise: slide up from below as the item changes (shared).
            let equip_dy = -(1.0 - vm.equip.clamp(0.0, 1.0)) * 0.55;
            let bob = vm.bob.clamp(0.0, 1.0);

            // Draw each hand: (side sign, item, is_block, swings?). The main hand
            // (sign = m) swings on attack; the off hand only appears when it holds
            // something (shield/torch/map) and never swings.
            let hands: [(f32, Option<[f32; 4]>, bool, bool); 2] = [
                (m, vm.item_uv, vm.item_is_block, true),
                (-m, vm.off_hand_uv, vm.off_hand_is_block, false),
            ];
            for (idx, (sign, item_uv, is_block, swings)) in hands.into_iter().enumerate() {
                let is_off = idx == 1;
                if is_off && item_uv.is_none() {
                    continue; // empty off hand draws nothing
                }
                // Swing arc (main hand only): item dips down and rotates.
                let sw = if swings { vm.swing.clamp(0.0, 1.0) } else { 0.0 };
                let sin_sw = (sw * std::f32::consts::PI).sin(); // 0..1..0
                let sin_sqrt = (sw.sqrt() * std::f32::consts::PI).sin();
                let swing_dx = -sin_sqrt * 0.22 * sign;
                let swing_dy =
                    (sw * std::f32::consts::PI * 2.0).sin().abs() * 0.10 - sin_sqrt * 0.30;
                let swing_dz = sin_sw * 0.14;
                let swing_rx = sin_sw * 1.1;
                let bob_dx = vm.bob_phase.sin() * 0.035 * bob * sign;
                let bob_dy = -(vm.bob_phase * 2.0).cos().abs() * 0.025 * bob;
                // Base eye-space placement (x right, y up, -z forward).
                let mut base = Vec3::new(
                    (0.30 + bob_dx) * sign + swing_dx,
                    -0.26 + swing_dy + equip_dy + bob_dy,
                    -0.52 + swing_dz,
                );
                // Item-use pose (main hand only): pull the item up toward the
                // mouth and inward, with a rapid eating/drinking shake (vanilla).
                let using = if is_off { 0.0 } else { vm.using.clamp(0.0, 1.0) };
                if using > 0.0 {
                    let shake = (vm.use_phase * 22.0).sin() * 0.018 * using;
                    base += Vec3::new(
                        (-0.14 * sign) * using + shake * sign,
                        0.20 * using + shake * 0.5,
                        0.16 * using,
                    );
                }

                let mut push_vm = |model: Mat4, cmd: EntityCmd| {
                    let mut bytes = [0u8; 80];
                    bytes[..64].copy_from_slice(bytemuck::cast_slice(&model.to_cols_array()));
                    bytes[64..].copy_from_slice(bytemuck::cast_slice(&[1.0f32, 1.0, 1.0, 1.0]));
                    slots.push(bytes);
                    cmds.push(cmd);
                };

                // Arm: grip near the item, forearm into the corner (Quat aligns
                // the mesh's +Y forearm to that direction).
                if have_arm {
                    let arm_pos = base + Vec3::new(0.05 * sign, -0.03, 0.05);
                    let dir = Vec3::new(0.46 * sign, -0.74, 0.16).normalize();
                    let q = glam::Quat::from_rotation_arc(Vec3::Y, dir);
                    let cam = Mat4::from_translation(arm_pos)
                        * Mat4::from_quat(q)
                        * Mat4::from_scale(Vec3::splat(1.05));
                    push_vm(r_inv * cam, EntityCmd::ViewArm { key, slim: vm.slim });
                }

                // Held block as a real 3D cube (main hand only) — vanilla holds
                // it corner-toward-you. Falls back to the flat icon otherwise.
                let block_geo = if idx == 0 { vm.block_quads.as_deref() } else { None };
                if let Some(quads) = block_geo.filter(|q| !q.is_empty()) {
                    let start = item_verts.len() as u32;
                    for &(p, uv) in quads.iter() {
                        item_verts.push(TexVertex { pos: p, uv });
                    }
                    let count = item_verts.len() as u32 - start;
                    let cam = Mat4::from_translation(base + Vec3::new(0.07 * sign, -0.04, 0.0))
                        * Mat4::from_rotation_y(sign * -0.55)
                        * Mat4::from_rotation_x(0.20)
                        * Mat4::from_scale(Vec3::splat(0.30));
                    push_vm(r_inv * cam, EntityCmd::ViewBlock { start, count });
                } else if let Some(uv) = item_uv {
                    // Flat item/tool sprite gripped in the hand.
                    let start = item_verts.len() as u32;
                    let half = if is_block { 0.18 } else { 0.16 };
                    push_viewmodel_item(&mut item_verts, uv, half);
                    let count = item_verts.len() as u32 - start;
                    let (tilt_z, tilt_x, tilt_y) = if is_block {
                        (sign * 0.20, -0.30, sign * 0.45)
                    } else {
                        (sign * 0.85, swing_rx * 0.3, sign * 0.28)
                    };
                    let cam = Mat4::from_translation(base + Vec3::new(0.02 * sign, 0.06, 0.0))
                        * Mat4::from_rotation_z(tilt_z)
                        * Mat4::from_rotation_y(tilt_y)
                        * Mat4::from_rotation_x(tilt_x)
                        * Mat4::from_scale(Vec3::new(sign, 1.0, 1.0));
                    push_vm(r_inv * cam, EntityCmd::ViewItem { start, count });
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

            // Celestial sky (stars, then sun/moon), after the clear and before
            // the terrain so the world occludes it. Alpha-blended, no depth.
            if scene.sky.is_some() {
                pass.set_pipeline(&self.pipe_sky);
                for (i, cmd) in cmds.iter().enumerate() {
                    let (vbuf, count, tex) = match cmd {
                        EntityCmd::Stars => {
                            let Some(t) = &self.white_tex else { continue };
                            (&self.star_mesh.0, self.star_mesh.1, t)
                        }
                        EntityCmd::Sun => {
                            let Some(t) = &self.sun_tex else { continue };
                            (&self.sky_quad, 6u32, t)
                        }
                        EntityCmd::Glow => {
                            let Some(t) = &self.glow_tex else { continue };
                            (&self.sky_quad, 6u32, t)
                        }
                        EntityCmd::Moon { phase } => {
                            let Some(t) = self.moon_tex.get(*phase) else { continue };
                            (&self.sky_quad, 6u32, t)
                        }
                        _ => continue,
                    };
                    pass.set_vertex_buffer(0, vbuf.slice(..));
                    pass.set_bind_group(1, tex, &[]);
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(0..count, 0..1);
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
                    let EntityCmd::SkinPart { key, slim, part, overlay } = cmd else { continue };
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
                    let (start, count) =
                        if *overlay { mesh.overlay[*part] } else { mesh.parts[*part] };
                    if count == 0 {
                        continue;
                    }
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
            // Dropped 3D blocks, same skin pipeline but bound to the block atlas.
            if let Some(vbuf) = &item_vbuf {
                let mut bound = false;
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::DropBlock { start, count } = cmd else { continue };
                    if !bound {
                        pass.set_pipeline(&self.pipe_skin);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.set_bind_group(1, &self.atlas_bg, &[]);
                        bound = true;
                    }
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(*start..*start + *count, 0..1);
                    draw_calls += 1;
                }
            }

            // Flat wall entities (paintings): dynamic per-entity quads bound to
            // their own texture via the skin pipeline (alpha discard keeps the
            // transparent painting/back edges clean).
            if let Some(vbuf) = &item_vbuf {
                let mut bound = false;
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::FlatTex { start, count, key } = cmd else { continue };
                    let Some(bg) = self.skins.get(key) else { continue };
                    if !bound {
                        pass.set_pipeline(&self.pipe_skin);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        bound = true;
                    }
                    pass.set_bind_group(1, bg, &[]);
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(*start..*start + *count, 0..1);
                    draw_calls += 1;
                }
            }

            // Particle billboards: alpha-blended, depth-tested (terrain occludes)
            // but no depth write, sampling the particle atlas — same pipeline as
            // clouds (the sky shader multiplies texture by the per-draw tint).
            if let (Some(vbuf), Some(atlas)) = (&item_vbuf, &self.particle_atlas)
                && cmds.iter().any(|c| matches!(c, EntityCmd::ParticleQuad { .. }))
            {
                pass.set_pipeline(&self.pipe_clouds);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.set_bind_group(1, atlas, &[]);
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::ParticleQuad { start, count } = cmd else { continue };
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(*start..*start + *count, 0..1);
                    draw_calls += 1;
                }
            }

            // Mining crack overlay: a slightly inflated textured cube over the
            // block being broken (alpha-discard skin shader, so only the
            // crack pixels land on the faces).
            if !self.crack_tex.is_empty()
                && cmds.iter().any(|c| matches!(c, EntityCmd::Crack { .. }))
            {
                pass.set_pipeline(&self.pipe_skin);
                pass.set_vertex_buffer(0, self.crack_vbuf.slice(..));
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::Crack { stage } = cmd else { continue };
                    pass.set_bind_group(1, &self.crack_tex[*stage], &[]);
                    pass.set_bind_group(
                        2,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(0..36, 0..1);
                    draw_calls += 1;
                }
            }
            // Block selection outline (vanilla thin black box), after all
            // solid geometry so depth testing hides occluded edges.
            if cmds.iter().any(|c| matches!(c, EntityCmd::Outline)) {
                pass.set_pipeline(&self.pipe_outline);
                pass.set_vertex_buffer(0, self.cube_lines_vbuf.slice(..));
                for (i, cmd) in cmds.iter().enumerate() {
                    if !matches!(cmd, EntityCmd::Outline) {
                        continue;
                    }
                    pass.set_bind_group(
                        1,
                        &self.entity_uniform.bind_group,
                        &[self.entity_uniform.offset_of(i as u32)],
                    );
                    pass.draw(0..24, 0..1);
                    draw_calls += 1;
                }
            }

            // Cloud plane: after opaque terrain (so it's depth-occluded), before
            // translucent water. One quad, alpha-blended, its own repeat sampler.
            if let (Some(vbuf), Some(cloud)) = (&item_vbuf, &self.cloud_tex) {
                for (i, cmd) in cmds.iter().enumerate() {
                    let EntityCmd::Clouds { start, count } = cmd else { continue };
                    pass.set_pipeline(&self.pipe_clouds);
                    pass.set_vertex_buffer(0, vbuf.slice(..));
                    pass.set_bind_group(1, cloud, &[]);
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

            // First-person view model, last of all, always on top (pipe_viewmodel
            // has depth test/write disabled). Arm (skin) then held item (atlas).
            for (i, cmd) in cmds.iter().enumerate() {
                match cmd {
                    EntityCmd::ViewArm { key, slim } => {
                        let Some(bg) = self.skins.get(key) else { continue };
                        let (buf, count) =
                            if *slim { &self.vm_arm_slim } else { &self.vm_arm_wide };
                        pass.set_pipeline(&self.pipe_viewmodel);
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.set_bind_group(1, bg, &[]);
                        pass.set_bind_group(
                            2,
                            &self.entity_uniform.bind_group,
                            &[self.entity_uniform.offset_of(i as u32)],
                        );
                        pass.draw(0..*count, 0..1);
                        draw_calls += 1;
                    }
                    EntityCmd::ViewItem { start, count } => {
                        let (Some(vbuf), Some(atlas)) = (&item_vbuf, &self.item_atlas) else {
                            continue;
                        };
                        pass.set_pipeline(&self.pipe_viewmodel);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.set_bind_group(1, atlas, &[]);
                        pass.set_bind_group(
                            2,
                            &self.entity_uniform.bind_group,
                            &[self.entity_uniform.offset_of(i as u32)],
                        );
                        pass.draw(*start..*start + *count, 0..1);
                        draw_calls += 1;
                    }
                    EntityCmd::ViewBlock { start, count } => {
                        let Some(vbuf) = &item_vbuf else { continue };
                        pass.set_pipeline(&self.pipe_viewmodel);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.set_bind_group(1, &self.atlas_bg, &[]);
                        pass.set_bind_group(
                            2,
                            &self.entity_uniform.bind_group,
                            &[self.entity_uniform.offset_of(i as u32)],
                        );
                        pass.draw(*start..*start + *count, 0..1);
                        draw_calls += 1;
                    }
                    _ => {}
                }
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
/// The 12 edges of the centered unit cube as a LineList (24 vertices) — the
/// block selection outline, scaled/translated per draw like the entity cube.
fn unit_cube_line_vertices() -> [[f32; 3]; 24] {
    const H: f32 = 0.5;
    let c = |x: i32, y: i32, z: i32| -> [f32; 3] {
        [if x == 0 { -H } else { H }, if y == 0 { -H } else { H }, if z == 0 { -H } else { H }]
    };
    [
        // bottom square
        c(0, 0, 0), c(1, 0, 0), c(1, 0, 0), c(1, 0, 1), c(1, 0, 1), c(0, 0, 1), c(0, 0, 1), c(0, 0, 0),
        // top square
        c(0, 1, 0), c(1, 1, 0), c(1, 1, 0), c(1, 1, 1), c(1, 1, 1), c(0, 1, 1), c(0, 1, 1), c(0, 1, 0),
        // verticals
        c(0, 0, 0), c(0, 1, 0), c(1, 0, 0), c(1, 1, 0), c(1, 0, 1), c(1, 1, 1), c(0, 0, 1), c(0, 1, 1),
    ]
}

/// Centered unit cube with full 0..1 UVs on every face — the mining crack
/// overlay mesh (drawn with a destroy_stage texture through the skin shader).
fn crack_cube_vertices() -> [TexVertex; 36] {
    const H: f32 = 0.5;
    let faces: [[[f32; 3]; 4]; 6] = [
        [[H, -H, -H], [H, H, -H], [H, H, H], [H, -H, H]],
        [[-H, -H, H], [-H, H, H], [-H, H, -H], [-H, -H, -H]],
        [[-H, H, -H], [-H, H, H], [H, H, H], [H, H, -H]],
        [[-H, -H, H], [-H, -H, -H], [H, -H, -H], [H, -H, H]],
        [[-H, -H, H], [H, -H, H], [H, H, H], [-H, H, H]],
        [[H, -H, -H], [-H, -H, -H], [-H, H, -H], [H, H, -H]],
    ];
    let uvs: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
    let mut out = [TexVertex { pos: [0.0; 3], uv: [0.0; 2] }; 36];
    let mut i = 0;
    for f in faces {
        for idx in [0usize, 1, 2, 0, 2, 3] {
            out[i] = TexVertex { pos: f[idx], uv: uvs[idx] };
            i += 1;
        }
    }
    out
}

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
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
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
