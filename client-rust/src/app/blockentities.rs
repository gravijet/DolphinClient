//! Block entities that the block model alone cannot draw.
//!
//! Signs, banners, heads, conduits, bells and decorated pots all have a
//! particle-only block model in vanilla — the visible part is drawn by a block
//! entity renderer from the server's NBT. This module holds the decoded NBT the
//! bridge sends, composites the textures each one needs (a banner's pattern
//! stack, a sign's text, a pot's sherd faces) and turns both into the same
//! `EntityDraw` list the mob renderer already consumes.
//!
//! Nothing here touches the terrain mesh: the blocks concerned bake to nothing,
//! so these draws add the missing geometry rather than doubling it.

use std::collections::{HashMap, HashSet};

use image::{Rgba, RgbaImage};

use crate::assets::AssetPack;
use crate::assets::blockmap::BlockTable;
use crate::assets::font::{Font, LINE_HEIGHT};
use crate::bridge::events::{BlockEntityData, ChatSpan, SignFace};
use crate::render::MobModel;
use crate::types::BlockPos;

use super::skins::fnv64;

/// Vanilla `DyeColor.getTextColor()` — the colour dyed sign text is drawn in,
/// in white → black id order. Deliberately not the banner/wool RGB: vanilla
/// keeps a separate, more readable palette for text.
pub const DYE_TEXT_RGB: [[u8; 3]; 16] = [
    [0xF0, 0xF0, 0xF0], // white
    [0xEB, 0x88, 0x04], // orange
    [0xC3, 0x54, 0xCD], // magenta
    [0x66, 0x89, 0xD3], // light_blue
    [0xDE, 0xCF, 0x2A], // yellow
    [0x41, 0xCD, 0x34], // lime
    [0xD8, 0x81, 0x98], // pink
    [0x43, 0x43, 0x43], // gray
    [0xAB, 0xAB, 0xAB], // light_gray
    [0x28, 0x76, 0x97], // cyan
    [0x7B, 0x2F, 0xBE], // purple
    [0x25, 0x31, 0x92], // blue
    [0x51, 0x30, 0x1A], // brown
    [0x3B, 0x51, 0x1A], // green
    [0xB3, 0x31, 0x2C], // red
    [0x1E, 0x1B, 0x1B], // black
];

/// Vanilla `DyeColor.getTextureDiffuseColor()` — the RGB banner cloth, bed and
/// wool are tinted with, in white → black id order.
pub const DYE_CLOTH_RGB: [[u8; 3]; 16] = [
    [0xF9, 0xFF, 0xFE], // white
    [0xF9, 0x80, 0x1D], // orange
    [0xC7, 0x4E, 0xBD], // magenta
    [0x3A, 0xB3, 0xDA], // light_blue
    [0xFE, 0xD8, 0x3D], // yellow
    [0x80, 0xC7, 0x1F], // lime
    [0xF3, 0x8B, 0xAA], // pink
    [0x47, 0x4F, 0x52], // gray
    [0x9D, 0x9D, 0x97], // light_gray
    [0x16, 0x9C, 0x9C], // cyan
    [0x89, 0x32, 0xB8], // purple
    [0x3C, 0x44, 0xAA], // blue
    [0x83, 0x54, 0x32], // brown
    [0x5E, 0x7C, 0x16], // green
    [0xB0, 0x2E, 0x26], // red
    [0x1D, 0x1D, 0x21], // black
];

/// Vanilla's sign text box: 4 lines of 10 pixels, 90 pixels wide, one font
/// pixel = 1/96 of a block.
const SIGN_TEXT_W: u32 = 90;
const SIGN_LINE_H: u32 = 10;
const SIGN_TEXT_H: u32 = SIGN_LINE_H * 4;
const SIGN_PX: f32 = 1.0 / 96.0;

/// Everything the app knows about the block entities around it.
#[derive(Default)]
pub struct BlockEntities {
    /// Decoded NBT by position. Pruned when the block changes or unloads.
    pub map: HashMap<BlockPos, BlockEntityData>,
    /// Textures composited for the block entities in view, waiting to be
    /// uploaded to the renderer on the next frame.
    pending: Vec<(u64, RgbaImage)>,
    /// Texture keys already composited, so a banner is only built once.
    built: HashSet<u64>,
    /// Bells struck recently: position → (when the swing started, the vanilla
    /// Direction it was struck from).
    rings: HashMap<BlockPos, (std::time::Instant, u8)>,
}

impl BlockEntities {
    /// Replace the entries the server just sent.
    pub fn insert_all(&mut self, entries: Vec<crate::bridge::events::BlockEntityInfo>) {
        for e in entries {
            self.map.insert(e.pos, e.data);
        }
    }

    /// Drop the entry at `pos` — the block there is no longer what it was.
    pub fn remove(&mut self, pos: BlockPos) {
        self.map.remove(&pos);
        self.rings.remove(&pos);
    }

    /// Drop everything in a chunk column that just unloaded.
    pub fn retain_chunks(&mut self, keep: impl Fn(i32, i32) -> bool) {
        self.map.retain(|p, _| keep(p.x >> 4, p.z >> 4));
        self.rings.retain(|p, _| keep(p.x >> 4, p.z >> 4));
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.rings.clear();
    }

    /// A bell was struck: start its swing.
    pub fn ring_bell(&mut self, pos: BlockPos, direction: u8) {
        self.rings.insert(pos, (std::time::Instant::now(), direction));
    }

    /// How long ago this bell was struck and from which face, while its swing
    /// is still playing.
    pub fn struck(&self, pos: BlockPos, now: std::time::Instant) -> Option<(f32, u8)> {
        let (at, dir) = self.rings.get(&pos)?;
        let t = now.duration_since(*at).as_secs_f32();
        (t < 2.5).then_some((t, *dir))
    }

    /// Textures composited since the last call, for the renderer to upload.
    pub fn take_pending(&mut self) -> Vec<(u64, RgbaImage)> {
        std::mem::take(&mut self.pending)
    }

    /// Composite `img` under `key` unless it has been built already, and return
    /// the key so callers can use it either way.
    pub fn build(&mut self, key: u64, img: impl FnOnce() -> Option<RgbaImage>) -> bool {
        if self.built.contains(&key) {
            return true;
        }
        match img() {
            Some(img) => {
                self.built.insert(key);
                self.pending.push((key, img));
                true
            }
            // Remember the failure too: a missing texture must not be retried
            // every single frame.
            None => {
                self.built.insert(key);
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// What to draw for one block entity
// ---------------------------------------------------------------------------

/// One piece of a block entity to render: a cuboid model with a texture, or a
/// flat text panel. Positions are block-local (0..1) and get offset by the app.
pub struct BePart {
    pub model: MobModel,
    pub tex: u64,
    /// Yaw in vanilla degrees.
    pub yaw: f32,
    /// Offset from the block's lower-north-west corner, in blocks.
    pub offset: [f32; 3],
    /// Uniform scale on top of the model's own.
    pub scale: f32,
    /// Swing angle in radians for the model's animated part (banner sway, bell
    /// swing); 0 for everything static.
    pub swing: f32,
}

/// A flat, world-space panel of pre-rendered text (a sign face).
pub struct BeText {
    pub tex: u64,
    pub yaw: f32,
    pub offset: [f32; 3],
    /// Panel size in blocks.
    pub size: [f32; 2],
    /// Draw it full-bright (glowing ink).
    pub glowing: bool,
}

/// An item lying on a block entity — food cooking on a campfire. The app looks
/// its icon up in the item atlas and draws it flat.
pub struct BeItem {
    /// Registry name without namespace.
    pub item: String,
    /// Offset from the block's centre, in blocks, already in world axes.
    pub offset: [f32; 3],
    /// Yaw the item quad is turned by, vanilla degrees.
    pub yaw: f32,
}

/// Everything one block entity contributes to the frame.
#[derive(Default)]
pub struct BeDraw {
    pub parts: Vec<BePart>,
    pub texts: Vec<BeText>,
    pub items: Vec<BeItem>,
}

// ---------------------------------------------------------------------------
// Banners
// ---------------------------------------------------------------------------

/// The 16 dye colours, in id order, as the block prefix banners use.
const DYE_NAMES: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray",
    "cyan", "purple", "blue", "brown", "green", "red", "black",
];

/// `red_banner` / `red_wall_banner` → (dye id, is wall banner).
pub fn banner_base(short: &str) -> Option<(u8, bool)> {
    let (name, wall) = match short.strip_suffix("_wall_banner") {
        Some(n) => (n, true),
        None => (short.strip_suffix("_banner")?, false),
    };
    let id = DYE_NAMES.iter().position(|d| *d == name)? as u8;
    Some((id, wall))
}

/// Composite a banner sheet: the untinted model texture (pole, crossbar and a
/// blank cloth), then the cloth tinted with the base colour, then every pattern
/// layer in paint order. Exactly vanilla's draw order, done once on the CPU
/// instead of once per layer per frame.
fn banner_texture(pack: &mut AssetPack, base: u8, layers: &[(String, u8)]) -> Option<RgbaImage> {
    let mut sheet = pack.texture_png("entity/banner/banner_base").ok()?;
    let mut paint = |pack: &mut AssetPack, mask: &str, color: u8| {
        if let Ok(m) = pack.texture_png(&format!("entity/banner/{mask}")) {
            overlay_tinted(&mut sheet, &m, DYE_CLOTH_RGB[color as usize & 15]);
        }
    };
    paint(pack, "base", base);
    for (asset, color) in layers {
        paint(pack, asset, *color);
    }
    Some(sheet)
}

/// The patterns a loom offers with nothing but a banner and a dye, in the order
/// vanilla numbers them — its `#minecraft:no_item_required` banner-pattern tag,
/// which is what the loom's buttons are indexed by. The ten patterns that need
/// a pattern item (creeper, skull, flower, mojang, globe, piglin, flow, guster,
/// bricks and the curly border) are deliberately not here: a loom only offers
/// those when the item is in its third slot.
pub const LOOM_PATTERNS: [&str; 32] = [
    "square_bottom_left", "square_bottom_right", "square_top_left", "square_top_right",
    "stripe_bottom", "stripe_top", "stripe_left", "stripe_right", "stripe_center",
    "stripe_middle", "stripe_downright", "stripe_downleft", "small_stripes", "cross",
    "straight_cross", "triangle_bottom", "triangle_top", "triangles_bottom", "triangles_top",
    "diagonal_left", "diagonal_up_right", "diagonal_up_left", "diagonal_right", "circle",
    "rhombus", "half_vertical", "half_horizontal", "half_vertical_right",
    "half_horizontal_bottom", "border", "gradient", "gradient_up",
];

/// A flat picture of a banner: the front of the cloth, cut out of the composited
/// entity sheet. The loom draws one of these per pattern so you can see what you
/// are about to weave, exactly like vanilla — which renders the real banner
/// model into each button.
pub fn banner_preview(
    pack: &mut AssetPack,
    base: u8,
    layers: &[(String, u8)],
) -> Option<RgbaImage> {
    let sheet = banner_texture(pack, base, layers)?;
    // The cloth's front face on the 64×64 banner sheet: a 20×40 patch one pixel
    // in from the top-left corner.
    let (w, h) = sheet.dimensions();
    let scale = w as f32 / 64.0;
    let px = |v: f32| (v * scale) as u32;
    if w < 22 || h < 42 {
        return None;
    }
    Some(image::imageops::crop_imm(&sheet, px(1.0), px(1.0), px(20.0), px(40.0)).to_image())
}

/// Alpha-composite `src` over `dst`, multiplying `src` by `tint` first. Both
/// images must be the same size; anything else is a broken resource pack and is
/// skipped rather than panicking.
fn overlay_tinted(dst: &mut RgbaImage, src: &RgbaImage, tint: [u8; 3]) {
    if src.dimensions() != dst.dimensions() {
        return;
    }
    for (d, s) in dst.pixels_mut().zip(src.pixels()) {
        let a = s.0[3] as u32;
        if a == 0 {
            continue;
        }
        for i in 0..3 {
            let over = s.0[i] as u32 * tint[i] as u32 / 255;
            d.0[i] = ((over * a + d.0[i] as u32 * (255 - a)) / 255) as u8;
        }
        d.0[3] = (a + d.0[3] as u32 * (255 - a) / 255).min(255) as u8;
    }
}

// ---------------------------------------------------------------------------
// Heads
// ---------------------------------------------------------------------------

/// A skull block's `(model, entity texture)`. Player heads are handled
/// separately — their texture is a downloaded skin.
pub fn skull_look(short: &str) -> Option<(MobModel, &'static str)> {
    let base = short
        .strip_suffix("_wall_head")
        .or_else(|| short.strip_suffix("_wall_skull"))
        .or_else(|| short.strip_suffix("_head"))
        .or_else(|| short.strip_suffix("_skull"))?;
    Some(match base {
        "skeleton" => (MobModel::Skull, "entity/skeleton/skeleton"),
        "wither_skeleton" => (MobModel::Skull, "entity/skeleton/wither_skeleton"),
        "zombie" => (MobModel::Skull, "entity/zombie/zombie"),
        "creeper" => (MobModel::Skull, "entity/creeper/creeper"),
        "piglin" => (MobModel::SkullPiglin, "entity/piglin/piglin"),
        "dragon" => (MobModel::SkullDragon, "entity/enderdragon/dragon"),
        _ => return None,
    })
}

/// Load a skull texture, padding a 64×32 sheet to 64×64 so one model can use
/// both layouts (the head cube only ever reads the top half).
fn skull_texture(pack: &mut AssetPack, tex: &str) -> Option<RgbaImage> {
    let img = pack.texture_png(tex).ok()?;
    if img.height() >= img.width() {
        return Some(img);
    }
    let mut padded = RgbaImage::new(img.width(), img.width());
    for (x, y, p) in img.enumerate_pixels() {
        padded.put_pixel(x, y, *p);
    }
    Some(padded)
}

// ---------------------------------------------------------------------------
// Decorated pots
// ---------------------------------------------------------------------------

/// Pot faces in the order the model's box unwrap lays them out, paired with the
/// `sherds` index vanilla stores them under (back, left, right, front).
/// `(u, v)` is the face's top-left on the composited 64×64 sheet.
const POT_FACES: [(u32, u32, usize); 4] = [
    (0, 14, 1),  // +X → left sherd
    (14, 14, 3), // front (+Z) → front sherd
    (28, 14, 2), // −X → right sherd
    (42, 14, 0), // back (−Z) → back sherd
];

/// Composite a pot sheet: the four sides from their sherd (or the plain side
/// texture), the top and bottom from the pot's base sheet.
fn pot_texture(pack: &mut AssetPack, sherds: &[Option<String>; 4]) -> Option<RgbaImage> {
    let plain = pack.texture_png("entity/decorated_pot/decorated_pot_side").ok()?;
    let mut sheet = RgbaImage::new(64, 64);
    for (u, v, slot) in POT_FACES {
        let face = match &sherds[slot] {
            Some(name) => pack
                .texture_png(&format!("entity/decorated_pot/{name}"))
                .unwrap_or_else(|_| plain.clone()),
            None => plain.clone(),
        };
        // The side is 14 px wide on a 16 px sheet: vanilla crops one column
        // off each edge.
        blit(&mut sheet, &face, u, v, 1, 0, 14, 16);
    }
    // Top and bottom come off the neck's box unwrap on the pot base sheet.
    if let Ok(base) = pack.texture_png("entity/decorated_pot/decorated_pot_base") {
        blit_scaled(&mut sheet, &base, 8, 0, 8, 8, 14, 0, 14, 14);
        blit_scaled(&mut sheet, &base, 16, 0, 8, 8, 28, 0, 14, 14);
    }
    Some(sheet)
}

/// Copy a `w`×`h` region of `src` starting at `(sx, sy)` to `(dx, dy)` in `dst`.
fn blit(dst: &mut RgbaImage, src: &RgbaImage, dx: u32, dy: u32, sx: u32, sy: u32, w: u32, h: u32) {
    for y in 0..h {
        for x in 0..w {
            if sx + x >= src.width() || sy + y >= src.height() {
                continue;
            }
            if dx + x >= dst.width() || dy + y >= dst.height() {
                continue;
            }
            dst.put_pixel(dx + x, dy + y, *src.get_pixel(sx + x, sy + y));
        }
    }
}

/// Nearest-neighbour copy of a source region into a differently sized
/// destination region.
#[allow(clippy::too_many_arguments)]
fn blit_scaled(
    dst: &mut RgbaImage, src: &RgbaImage,
    sx: u32, sy: u32, sw: u32, sh: u32,
    dx: u32, dy: u32, dw: u32, dh: u32,
) {
    for y in 0..dh {
        for x in 0..dw {
            let (px, py) = (sx + x * sw / dw.max(1), sy + y * sh / dh.max(1));
            if px >= src.width() || py >= src.height() {
                continue;
            }
            if dx + x >= dst.width() || dy + y >= dst.height() {
                continue;
            }
            dst.put_pixel(dx + x, dy + y, *src.get_pixel(px, py));
        }
    }
}

// ---------------------------------------------------------------------------
// Sign text
// ---------------------------------------------------------------------------

/// Render one sign face into a 90×40 texture — vanilla's exact text box, one
/// texel per font pixel, so the result is pixel-identical to the real game at
/// any distance.
fn sign_texture(font: &Font, face: &SignFace) -> Option<RgbaImage> {
    if face.lines.iter().all(|l| line_text(l).trim().is_empty()) {
        return None;
    }
    let dye = dye_index(&face.color).unwrap_or(15);
    let mut img = RgbaImage::from_pixel(SIGN_TEXT_W, SIGN_TEXT_H, Rgba([0, 0, 0, 0]));
    for (i, spans) in face.lines.iter().enumerate() {
        let y = (i as u32 * SIGN_LINE_H) as i32 + (SIGN_LINE_H - LINE_HEIGHT) as i32;
        // Vanilla centres each line inside the text box.
        let width: f32 = spans.iter().map(|s| font.width(&s.text, s.bold)).sum();
        let mut x = (SIGN_TEXT_W as f32 - width) / 2.0;
        for span in spans {
            let color = span.color.unwrap_or(DYE_TEXT_RGB[dye]);
            if face.glowing {
                // Glowing ink: vanilla outlines the glyph in the dye colour and
                // fills it with a darkened version of the same.
                let outline = span.color.unwrap_or(DYE_TEXT_RGB[dye]);
                for (ox, oy) in [(-1.0, 0), (1.0, 0), (0.0, -1), (0.0, 1)] {
                    font.draw(&mut img, x + ox, y + oy, &span.text, outline, span.bold);
                }
                font.draw(&mut img, x, y, &span.text, dark(color, dye), span.bold);
            } else {
                font.draw(&mut img, x, y, &span.text, color, span.bold);
            }
            x += font.width(&span.text, span.bold);
        }
    }
    Some(img)
}

/// Vanilla's dark fill for glowing text: 40 % of the ink colour, except black
/// ink, which is drawn in the near-white the game special-cases so it stays
/// readable against its own outline.
fn dark(color: [u8; 3], dye: usize) -> [u8; 3] {
    if dye == 15 {
        return [0xF0, 0xEB, 0xCC];
    }
    [
        (color[0] as f32 * 0.4) as u8,
        (color[1] as f32 * 0.4) as u8,
        (color[2] as f32 * 0.4) as u8,
    ]
}

fn line_text(spans: &[ChatSpan]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn dye_index(name: &str) -> Option<usize> {
    DYE_NAMES.iter().position(|d| *d == name)
}

/// `red_dye` → the dye id a loom would weave in.
pub fn dye_id(item: &str) -> Option<u8> {
    dye_index(item.strip_suffix("_dye")?).map(|i| i as u8)
}

// ---------------------------------------------------------------------------
// Turning a block entity into draws
// ---------------------------------------------------------------------------

/// A block's state as far as the block-entity renderers care: its short name,
/// its 16-step `rotation` and its `facing`, plus whatever the app had to look up
/// itself (a player head's skin key, a conduit's activation).
pub struct BeState<'a> {
    pub short: &'a str,
    pub rotation: Option<f32>,
    pub facing: Option<&'a str>,
    /// Texture key for a player head, once the owner's skin has downloaded.
    pub player_head: Option<u64>,
    /// A conduit whose water box and prismarine frame are complete.
    pub conduit_active: bool,
    /// Seconds since this frame's reference instant — drives the banner sway
    /// and the bell swing.
    pub time: f32,
    /// Seconds since this bell was struck, and from which vanilla Direction.
    pub struck: Option<(f32, u8)>,
}

/// A blockstate `facing` → the yaw that turns a model's +Z face that way.
fn facing_yaw(facing: Option<&str>) -> f32 {
    match facing {
        Some("north") => 180.0,
        Some("south") => 0.0,
        Some("west") => 90.0,
        _ => 270.0,
    }
}

/// Where a sign's board sits on its block: `(centre height, yaw, front face z,
/// back face z)`, all in the sign's own frame (+Z = the way it reads). Mirrors
/// `models::bake::bake_sign`, which builds the board these panels land on.
fn sign_layout(short: &str, rotation: Option<f32>, facing: Option<&str>) -> Option<(f32, f32, f32, f32)> {
    let hanging = short.contains("hanging_sign");
    let wall = short.contains("wall_sign") || short.contains("wall_hanging_sign");
    // A wall sign's `facing` is where its face points; a standing sign turns in
    // 16 steps, 0 = facing south (+Z).
    let yaw = if wall { facing_yaw(facing) } else { -22.5 * rotation? };
    // A wall sign's board hugs the support behind it, so it sits at the far
    // side of its own block; free-standing boards straddle the centre.
    Some(match (wall, hanging) {
        (true, false) => (0.53125, yaw, -0.375, -0.5),
        (_, true) => (0.4375, yaw, 0.03125, -0.03125),
        _ => (0.71875, yaw, 0.03125, -0.03125),
    })
}

/// Build the draw list for one block entity.
pub fn draw_for(
    be: &mut BlockEntities,
    pack: &mut AssetPack,
    font: &Font,
    data: &BlockEntityData,
    st: &BeState,
) -> BeDraw {
    let mut out = BeDraw::default();
    match data {
        BlockEntityData::Sign { front, back } => {
            let Some((centre_y, yaw, front_z, back_z)) =
                sign_layout(st.short, st.rotation, st.facing)
            else {
                return out;
            };
            let size = [SIGN_TEXT_W as f32 * SIGN_PX, SIGN_TEXT_H as f32 * SIGN_PX];
            for (face, is_front) in [(front, true), (back, false)] {
                let key = fnv64(format!("sign:{}", sign_key(face)).as_bytes());
                if !be.build(key, || sign_texture(font, face)) {
                    continue;
                }
                // A hair clear of the board so the text never z-fights it. The
                // back panel is turned around, so its own +Z is the sign's −Z.
                let z = if is_front { front_z + 0.005 } else { -(back_z - 0.005) };
                out.texts.push(BeText {
                    tex: key,
                    yaw: yaw + if is_front { 0.0 } else { 180.0 },
                    offset: [0.0, centre_y, z],
                    size,
                    glowing: face.glowing,
                });
            }
        }
        BlockEntityData::Banner { layers } => {
            let Some((base, wall)) = banner_base(st.short) else { return out };
            let key = fnv64(banner_key(base, layers).as_bytes());
            if !be.build(key, || banner_texture(pack, base, layers)) {
                return out;
            }
            let yaw = if wall { facing_yaw(st.facing) } else { -22.5 * st.rotation.unwrap_or(0.0) };
            // A standing banner stands on its own block; a wall banner hangs
            // from the top of it, flush against the wall behind.
            let offset = if wall { [0.0, -0.75, -0.375] } else { [0.0; 3] };
            out.parts.push(BePart {
                model: if wall { MobModel::BannerWall } else { MobModel::Banner },
                tex: key,
                yaw,
                offset,
                scale: 1.0,
                // Vanilla's cloth sway is a slow, very shallow wave; a shared
                // clock offset per banner keeps neighbours out of lockstep.
                swing: 0.04 * (st.time * 1.4 + yaw).sin(),
            });
        }
        BlockEntityData::Skull { texture_url, .. } => {
            let wall = st.short.contains("_wall_");
            let (model, key) = match skull_look(st.short) {
                Some((model, tex)) => {
                    let key = fnv64(format!("head:{tex}").as_bytes());
                    if !be.build(key, || skull_texture(pack, tex)) {
                        return out;
                    }
                    (model, key)
                }
                // Player head: the texture is the owner's own skin, downloaded
                // by the skin manager exactly like a player's.
                None => {
                    if texture_url.is_none() {
                        return out;
                    }
                    let Some(key) = st.player_head else { return out };
                    (MobModel::PlayerHead, key)
                }
            };
            out.parts.push(skull_part(model, key, wall, st));
        }
        BlockEntityData::DecoratedPot { sherds } => {
            let key = fnv64(format!("pot:{sherds:?}").as_bytes());
            if !be.build(key, || pot_texture(pack, sherds)) {
                return out;
            }
            out.parts.push(BePart {
                model: MobModel::DecoratedPot,
                tex: key,
                yaw: facing_yaw(st.facing),
                offset: [0.0; 3],
                scale: 1.0,
                swing: 0.0,
            });
        }
        BlockEntityData::Bell => {
            let Some(key) = simple_texture(be, pack, "entity/bell/bell_body") else { return out };
            // Vanilla rings for 50 ticks: a damped swing at one wobble per
            // 10 ticks, decaying to nothing.
            let swing = match st.struck {
                Some((t, _)) if t < 2.5 => {
                    0.32 * (1.0 - t / 2.5) * (t * std::f32::consts::TAU / 0.5).sin()
                }
                _ => 0.0,
            };
            // The bell swings toward whoever rang it.
            let yaw = match st.struck.map(|(_, d)| d) {
                Some(2) => 180.0,
                Some(4) => 90.0,
                Some(5) => 270.0,
                _ => 0.0,
            };
            out.parts.push(BePart {
                model: MobModel::Bell,
                tex: key,
                yaw,
                offset: [0.0; 3],
                scale: 1.0,
                swing,
            });
        }
        BlockEntityData::Conduit => {
            let tex = conduit_texture(st.conduit_active);
            let Some(key) = simple_texture(be, pack, tex) else { return out };
            out.parts.push(BePart {
                model: MobModel::Conduit,
                tex: key,
                // An active conduit spins; an inactive shell sits still.
                yaw: if st.conduit_active { st.time * 60.0 % 360.0 } else { 0.0 },
                offset: [0.0; 3],
                scale: 1.0,
                swing: 0.0,
            });
        }
        BlockEntityData::Campfire { items } => {
            // Vanilla lays the four cooking slots flat on the fire, one per
            // quadrant, each turned a further quarter turn.
            let facing = facing_yaw(st.facing);
            for (slot, item) in items.iter().enumerate() {
                let Some(item) = item else { continue };
                let local = match slot {
                    0 => [-0.156, 0.44, -0.156],
                    1 => [0.156, 0.44, -0.156],
                    2 => [0.156, 0.44, 0.156],
                    _ => [-0.156, 0.44, 0.156],
                };
                out.items.push(BeItem {
                    item: item.clone(),
                    offset: local,
                    yaw: facing + slot as f32 * 90.0,
                });
            }
        }
    }
    out
}

/// A skull's part: floor skulls turn by their 16-step `rotation`, wall skulls
/// face out of the wall and sit half a block up against it.
fn skull_part(model: MobModel, tex: u64, wall: bool, st: &BeState) -> BePart {
    let (yaw, offset) = if wall {
        (facing_yaw(st.facing), [0.0, 0.25, -0.25])
    } else {
        (-22.5 * st.rotation.unwrap_or(0.0), [0.0; 3])
    };
    BePart { model, tex, yaw, offset, scale: 1.0, swing: 0.0 }
}

/// Vanilla's conduit frame: a block counts when it is 2 away on one axis, level
/// on another, and within 2 on the third — the four square rings around the
/// conduit. Returns the offsets to test.
pub fn conduit_frame_offsets() -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    for i in -2..=2i32 {
        for j in -2..=2i32 {
            for k in -2..=2i32 {
                let (l, m, n) = (i.abs(), j.abs(), k.abs());
                let ring = (i == 0 && (m == 2 || n == 2))
                    || (j == 0 && (l == 2 || n == 2))
                    || (k == 0 && (l == 2 || m == 2));
                if (l > 1 || m > 1 || n > 1) && ring {
                    out.push([i, j, k]);
                }
            }
        }
    }
    out
}

/// The blocks vanilla accepts as a conduit frame.
pub fn is_conduit_frame(short: &str) -> bool {
    matches!(short, "prismarine" | "prismarine_bricks" | "sea_lantern" | "dark_prismarine")
}

/// A stable cache key for a banner's full pattern stack.
fn banner_key(base: u8, layers: &[(String, u8)]) -> String {
    let mut s = format!("banner:{base}");
    for (a, c) in layers {
        s.push('|');
        s.push_str(a);
        s.push(':');
        s.push_str(&c.to_string());
    }
    s
}

/// A stable cache key for one rendered sign face.
fn sign_key(face: &SignFace) -> String {
    let mut s = format!("{}:{}", face.color, face.glowing as u8);
    for line in &face.lines {
        s.push('\n');
        for span in line {
            let c = span.color.map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
            s.push_str(&format!("{}#{}{}", span.text, c.unwrap_or_default(), span.bold as u8));
        }
    }
    s
}

/// Which conduit texture to use: vanilla shows the closed base when the conduit
/// is inactive and the open cage when it is powered.
pub fn conduit_texture(active: bool) -> &'static str {
    if active { "entity/conduit/cage" } else { "entity/conduit/base" }
}

/// Load one of the fixed block-entity textures (conduit shell, bell body) and
/// hand back the key it was cached under.
pub fn simple_texture(be: &mut BlockEntities, pack: &mut AssetPack, tex: &str) -> Option<u64> {
    let key = fnv64(format!("be:{tex}").as_bytes());
    let path = tex.to_owned();
    be.build(key, || pack.texture_png(&path).ok()).then_some(key)
}

/// Does this block have a block entity we draw? Used to prune the map when a
/// block changes — anything else at that position means the entity is gone.
pub fn draws_block_entity(table: &BlockTable, state: crate::types::StateId) -> bool {
    let Some(entry) = table.entry(state) else { return false };
    let n = entry.short_name.as_str();
    n.ends_with("_sign") || n.ends_with("_banner") || n.ends_with("_head") || n.ends_with("_skull")
        || n == "decorated_pot"
        || n == "conduit"
        || n == "bell"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_base_reads_colour_and_wall() {
        assert_eq!(banner_base("red_banner"), Some((14, false)));
        assert_eq!(banner_base("light_blue_wall_banner"), Some((3, true)));
        assert_eq!(banner_base("oak_sign"), None);
    }

    #[test]
    fn skull_look_covers_every_mob_head() {
        for (block, want) in [
            ("skeleton_skull", "entity/skeleton/skeleton"),
            ("wither_skeleton_wall_skull", "entity/skeleton/wither_skeleton"),
            ("zombie_head", "entity/zombie/zombie"),
            ("creeper_wall_head", "entity/creeper/creeper"),
            ("piglin_head", "entity/piglin/piglin"),
            ("dragon_head", "entity/enderdragon/dragon"),
        ] {
            assert_eq!(skull_look(block).map(|(_, t)| t), Some(want), "{block}");
        }
        // Player heads carry no fixed texture — they use the owner's skin.
        assert!(skull_look("player_head").is_none());
    }

    #[test]
    fn sign_layout_puts_each_kind_on_its_board() {
        let (y, yaw, front, back) = sign_layout("oak_wall_sign", None, Some("north")).unwrap();
        assert_eq!(yaw, 180.0);
        // The board hugs the support: its readable face is 2 px out from the
        // far side of the block, and its back is flush with that side.
        assert_eq!((front, back), (-0.375, -0.5));
        assert!((y - 0.53125).abs() < 1e-6);

        let (y, yaw, front, back) = sign_layout("oak_sign", Some(4.0), None).unwrap();
        assert_eq!(yaw, -90.0); // 4 sixteenths clockwise from south
        assert_eq!((front, back), (0.03125, -0.03125));
        assert!((y - 0.71875).abs() < 1e-6);

        let (y, _, _, _) = sign_layout("oak_hanging_sign", Some(0.0), None).unwrap();
        assert!((y - 0.4375).abs() < 1e-6);

        // A standing sign with no rotation property is not a sign we can place.
        assert!(sign_layout("oak_sign", None, None).is_none());
    }

    #[test]
    fn conduit_frame_is_the_four_vanilla_rings() {
        let offs = conduit_frame_offsets();
        // Three axis-aligned rings of 16 positions each (the 5×5 square outline
        // in that plane), sharing two blocks with each of the other two.
        assert_eq!(offs.len(), 3 * 16 - 6);
        assert!(offs.contains(&[2, 0, 2])); // ring corner
        assert!(offs.contains(&[2, 0, 0])); // middle of a ring edge
        assert!(offs.contains(&[0, 2, -2]));
        // Never the conduit itself, and never off a ring plane.
        assert!(!offs.contains(&[0, 0, 0]));
        assert!(!offs.contains(&[2, 1, 1]));
        assert!(is_conduit_frame("dark_prismarine"));
        assert!(!is_conduit_frame("stone"));
    }

    #[test]
    fn banner_and_sign_keys_are_stable_and_distinct() {
        let a = banner_key(14, &[("stripe_bottom".into(), 0)]);
        let b = banner_key(14, &[("stripe_bottom".into(), 1)]);
        assert_ne!(a, b);
        assert_eq!(a, banner_key(14, &[("stripe_bottom".into(), 0)]));

        let mut face = SignFace { color: "black".into(), ..SignFace::default() };
        face.lines[0] = vec![ChatSpan::plain("hi")];
        let k1 = sign_key(&face);
        face.glowing = true;
        assert_ne!(k1, sign_key(&face));
    }

    #[test]
    fn dark_glow_fill_special_cases_black() {
        assert_eq!(dark([0x1E, 0x1B, 0x1B], 15), [0xF0, 0xEB, 0xCC]);
        assert_eq!(dark([100, 100, 100], 0), [40, 40, 40]);
    }
}
