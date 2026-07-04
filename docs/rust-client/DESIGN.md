# DolphinClient Rust — Architecture (v0.1)

Native Minecraft **26.1** client: **azalea 0.16** (protocol, world state, physics,
auth) + **wgpu 29** renderer + **winit 0.30** window + **egui 0.35** HUD.
Multiplayer-first: no worldgen, no world saving — the server owns the world.

## Dataflow

```
                    tokio runtime                         main thread
┌──────────┐  events   ┌────────┐  GameEvent (crossbeam) ┌───────────────┐
│  Server  │◄─────────►│ bridge │───────────────────────►│ app (winit)   │
└──────────┘  azalea   │        │◄───────────────────────│  WorldMirror  │
                       └────────┘  Command (tokio mpsc)  │  Renderer     │
                                                         │  egui HUD     │
                                                         └──────┬────────┘
                                              rayon pool ┌──────▼────────┐
                                              snapshots  │ world::mesher │
                                              ──────────►│ MeshData      │──► GPU upload
                                                         └───────────────┘
```

- **bridge** (owns azalea `Client`): translates azalea events → `GameEvent`
  (plain data, section snapshots are copies), applies `Command`s to azalea.
- **app** (main thread): drains `GameEvent`s each frame → mutates `WorldMirror`,
  marks dirty sections, schedules meshing on rayon (nearest-first), uploads
  finished meshes, renders, draws egui HUD, forwards input as `Command`s.
- **No shared locks** between net and render: only channels + snapshot copies.

## Modules (single crate `dolphinclient`)

| Module | Owns | Key API |
|---|---|---|
| `types` | shared plain types | `SectionPos`, `MeshVertex`, `SectionData`, `RenderLayer`, `MeshData` |
| `bridge` | azalea client task | `spawn_bridge(opts) -> (GameHandle, Receiver<GameEvent>)` |
| `bridge::events` | event/command enums | `GameEvent`, `Command`, `EntitySnapshot`, `ItemSnapshot` |
| `assets` | client-jar zip reading | `AssetPack::open(jar)`, `read_blockstate/model/texture` |
| `assets::atlas` | texture atlas | `AtlasBuilder` → `Atlas { rgba, w, h, uv(name) }` |
| `assets::blockmap` | state id → name+props | `BlockTable::load(blocks.json.gz)` → `entry(state_id)` |
| `models` | blockstate/model JSON | `resolve()`: variants+multipart, parent chain |
| `models::bake` | baked geometry | `BakedModelStore::bake_all(...)` → per-state `BakedModel` (quads with atlas UVs, cullface, tint, rotation, occlusion class, render layer) |
| `world` | render-side world copy | `WorldMirror`: apply events, `snapshot27(pos)` padded 18³ snapshot, dirty queue |
| `world::mesher` | section → mesh | `mesh_section(&PaddedSnapshot, &BakedModelStore) -> MeshData` (rayon-safe, no locks) |
| `render` | wgpu | `Renderer::new(window \| offscreen)`, `frame(scene)`, `upload_mesh`, `drop_mesh` |
| `render::camera` | view/proj | camera-relative rendering (f32 offsets vs f64 eye pos) |
| `app` | winit loop | `run_windowed(opts)`; input → Commands; remesh scheduling |
| `app::hud` | egui | crosshair, hotbar, chat (log+input), F3 debug, connect screen |
| `app::offscreen` | headless test mode | connect, wait for chunks, render N frames to PNGs (lavapipe-friendly) |

## Conventions

- Coordinates: MC standard (x east, y up, z south). Positions `f64` in world
  space; rendering is **camera-relative** (per-draw `section_origin - camera_pos`
  as f32 in a dynamic uniform) to dodge f32 precision loss far from origin.
- `SectionPos` = block >> 4 in all three axes (y can be negative, min_y −64).
- In-section index: `idx = (y*16 + z)*16 + x` (**YZX**, matches vanilla palettes).
- Block state ids = vanilla global palette ids (azalea uses the same ids).
- Air states (`air`, `cave_air`, `void_air`) mesh to nothing.

## Vertex format (28 B)

```rust
struct MeshVertex {
    pos: [f32; 3],   // relative to section origin, 0..16 (can exceed for offset models)
    uv: [f32; 2],    // normalized atlas coords
    color: [u8; 4],  // rgb = biome/constant tint (255 = none), a = 255
    light: [u8; 4],  // [sky 0-15, block 0-15, shade 0-255, ao 0-255]
}
```

Fragment: `b = max(sky/15 * daylight, block/15); rgb = color * shade/255 * ao/255 * max(b, 0.03)`
plus distance fog toward sky color. Cutout pass: `discard` at alpha < 0.5.

## Render layers & passes

1. **Opaque** — depth write, no blend.
2. **Cutout** — depth write, alpha discard (leaves, plants, glass panes' frames).
3. **Translucent** — depth test, no depth write, alpha blend, sections sorted
   back→front by distance (per-section granularity is OK for v1).

Face culling rules: a quad with `cullface=D` is skipped iff the neighbor block
in direction D **occludes** (baked full-cube with all-opaque textures). Water
culls against same-fluid neighbors. Leaves never occlude.

Fluids (water/lava) have **no block model JSON** — the mesher special-cases
them: source/flowing → 14/16-height box with the `_still` texture, translucent
(water) / opaque (lava), tinted `#3F76E4` for water. Waterlogged blocks emit the
fluid box *plus* the block model. (v1 approximation; vanilla-height-flow later.)

## Light

Use server-provided sky/block light per section if the bridge can capture it
(from azalea's stored light or raw `LevelChunkWithLight` packets). Fallback:
sky=15 uniform. Per-vertex light = light of the block the face *emits into*
(pos + face normal). AO: classic 4-sample corner darkening (vanilla-style),
computed per-vertex in the mesher.

## Threading

- main: winit + wgpu + egui + WorldMirror mutation (single writer).
- tokio (bridge): azalea; 20 TPS ticks come from azalea's own scheduler.
- rayon: meshing on immutable `PaddedSnapshot` copies (18×18×18 states +
  light), results returned via crossbeam channel. Budget: schedule ≤ 8
  sections/frame nearest-first; upload all finished results each frame.

## Offscreen test mode (CI / headless)

`dolphinclient --offscreen --server 127.0.0.1:25565 --username DolphinTest
--out shots/ --frames 8 --wait-chunks 49` — no winit: wgpu renders into a
texture (Vulkan lavapipe works headless), PNGs written per frame, camera does a
slow orbit at spawn eye height. Exit code 0 iff connected + ≥ wait-chunks
sections meshed + PNGs non-uniform (not all one color).

## Error handling

`anyhow::Result` at module boundaries; the app never panics on bad server data
(log + skip). Missing textures → magenta/black checker fallback tile in atlas.
Unknown/unbaked states → render as fallback full cube with checker texture.

## Out of scope for v0.1 (documented, honest)

Entity models beyond humanoid+box, block entities (chests render as baked model
placeholder, signs/banners blank), item rendering in world, particles, sounds,
biome-blended colors (constant plains tint), weather, GUI screens beyond chat +
hotbar + debug (no inventory click-through yet), shaders/fancy graphics,
resource-pack switching.
