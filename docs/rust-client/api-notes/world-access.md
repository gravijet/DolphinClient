# azalea 0.16.0+mc26.1 — World & chunk data access

Verified against the actual crate sources in
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/azalea-{world,client,core,block,registry,protocol}-0.16.0+mc26.1`.

Key 0.16 renames (old names still exist as `#[deprecated]` type aliases):
- `Instance` → `azalea_world::World`
- `InstanceName` → `azalea_world::WorldName`
- `InstanceContainer` → `azalea_world::Worlds`
- `PartialInstance` → `azalea_world::PartialWorld`
- `azalea_client::local_player::InstanceHolder` → `azalea_client::local_player::WorldHolder`
- `Client` lives in the `azalea` crate root (`azalea::Client`, defined in `azalea::client_impl`); `azalea::Client.ecs` is `Arc<parking_lot::RwLock<bevy_ecs::world::World>>` (public field).

All world locks are **`parking_lot::RwLock`** (NOT `std::sync` — no `.unwrap()` after `.read()`/`.write()`, and beware: parking_lot RwLocks are not reentrant; a read+write in the same thread deadlocks).

---

## 1. Getting the world from a `Client`

`azalea::Client` methods (src: `azalea-0.16.0+mc26.1/src/client_impl/mod.rs`):

```text
pub fn world(&self) -> Arc<RwLock<azalea_world::World>>          // shared world (superset, weak chunk storage)
pub fn partial_world(&self) -> Arc<RwLock<azalea_world::PartialWorld>>  // this client's render-distance slice (strong refs)
pub fn component<T: Component>(&self) -> MappedRwLockReadGuard<'_, T>
pub fn logged_in(&self) -> bool
```

Both are backed by the `WorldHolder` ECS component
(`azalea_client::local_player::WorldHolder`, re-exported so `azalea::local_player::WorldHolder` works):

```rust
pub struct WorldHolder {
    pub partial: Arc<RwLock<PartialWorld>>, // strong refs to chunks in render distance
    pub shared:  Arc<RwLock<World>>,        // Weak chunk map; shared between swarm clients
}
```

`azalea_world::World` (src `azalea-world/src/world.rs`):

```rust
pub struct World {
    pub chunks: ChunkStorage,
    pub entities_by_chunk: HashMap<ChunkPos, HashSet<Entity>>,
    pub entity_by_id: IntMap<MinecraftEntityId, Entity>,
    pub registries: RegistryHolder,           // azalea_core::registry_holder::RegistryHolder
}
impl World {
    pub fn get_block_state(&self, pos: BlockPos) -> Option<BlockState>;
    pub fn get_fluid_state(&self, pos: BlockPos) -> Option<FluidState>;
    pub fn get_biome(&self, pos: BlockPos) -> Option<azalea_registry::data::Biome>;
    pub fn set_block_state(&self, pos: BlockPos, state: BlockState) -> Option<BlockState>;
}
```

Compilable snippet:

```rust
use azalea::Client;
use azalea::core::position::{BlockPos, ChunkPos};

fn read_block(bot: &Client) {
    let world = bot.world();          // Arc<parking_lot::RwLock<azalea_world::World>>
    let world = world.read();         // parking_lot read guard, NOT Result
    let state = world.get_block_state(BlockPos::new(0, 64, 0)); // Option<BlockState>
    let chunk = world.chunks.get(&ChunkPos::new(0, 0));         // Option<Arc<RwLock<Chunk>>>
    drop(world);
    println!("{state:?} {}", chunk.is_some());
}
```

IMPORTANT for a renderer: the shared `World` holds chunks as **`Weak`** pointers
(`WeakChunkStorage`). The strong refs live in `PartialWorld.chunks`
(`PartialChunkStorage`). If your renderer clones the `Arc<RwLock<Chunk>>` it gets
from `world.chunks.get()`, it keeps that chunk alive independently — fine, but
it will NOT be told when azalea drops it; use unload events (§7). A chunk
returned by `get()` on the shared storage upgrades the Weak, so it can return
`None` after unload even if the pos was once loaded.

---

## 2. Chunk / Section storage & efficient 4096-block reads

Module: `azalea_world::chunk` (re-exports at crate root: `azalea_world::{Chunk, Section, ChunkStorage, PartialChunkStorage, BitStorage}`; palettes in `azalea_world::palette`).

```rust
// azalea-world/src/chunk/storage.rs
pub struct ChunkStorage(pub Box<dyn ChunkStorageTrait>);   // Deref<Target = dyn ChunkStorageTrait>
pub trait ChunkStorageTrait: Send + Sync + Any {
    fn min_y(&self) -> i32;                                 // usually -64
    fn height(&self) -> u32;                                // usually 384
    fn get(&self, pos: &ChunkPos) -> Option<Arc<RwLock<Chunk>>>;
    fn upsert(&mut self, pos: ChunkPos, chunk: Chunk) -> Arc<RwLock<Chunk>>;
    fn chunks(&self) -> Box<[&ChunkPos]>;                   // WARNING: on WeakChunkStorage this lists keys incl. dead Weaks
    // provided:
    fn get_block_state(&self, pos: BlockPos) -> Option<BlockState>;
    fn set_block_state(&self, pos: BlockPos, state: BlockState) -> Option<BlockState>;
    fn get_fluid_state(&self, pos: BlockPos) -> Option<FluidState>;
    fn get_biome(&self, pos: BlockPos) -> Option<Biome>;
}
// default impl: WeakChunkStorage { pub height: u32, pub min_y: i32, pub map: IntMap<ChunkPos, Weak<RwLock<Chunk>>> }

// azalea-world/src/chunk/mod.rs
pub struct Chunk {
    pub sections: Box<[Section]>,                                  // len = height/16 (24 for overworld)
    pub heightmaps: HashMap<HeightmapKind, Heightmap>,
}
pub struct Section {
    pub block_count: u16,          // non-air blocks; updated on set. ==0 → skip section when meshing
    pub fluid_count: u16,          // NOT updated by azalea after load
    pub states: PalettedContainer<BlockState>,   // 16x16x16 = 4096
    pub biomes: PalettedContainer<Biome>,        // 4x4x4 = 64
}
impl Section {
    pub fn get_block_state(&self, pos: ChunkSectionBlockPos) -> BlockState;
    pub fn get_and_set_block_state(&mut self, pos: ChunkSectionBlockPos, state: BlockState) -> BlockState;
    pub fn get_biome(&self, pos: ChunkSectionBiomePos) -> Biome;
}
pub fn section_index(y: i32, min_y: i32) -> u32;   // ((y>>4) - (min_y>>4))
pub fn get_block_state_from_sections(sections: &[Section], pos: &ChunkBlockPos, min_y: i32) -> Option<BlockState>;
```

Palette container (`azalea_world::palette`, src `palette/container.rs` + `palette/mod.rs`) — ALL FIELDS PUBLIC, so direct palette/data access is available (not just per-block get):

```rust
pub struct PalettedContainer<S: PalletedContainerKind> {   // note: "Palleted" typo is in the real trait name
    pub bits_per_entry: u8,
    pub palette: Palette<S>,
    pub storage: BitStorage,
}
pub enum Palette<S> { SingleValue(S), Linear(Vec<S>), Hashmap(Vec<S>), Global }
impl<S> Palette<S> { pub fn value_for(&self, id: usize) -> S; }
impl<S> PalettedContainer<S> {
    pub fn get_at_index(&self, index: usize) -> S;          // storage.get + palette.value_for
    pub fn get(&self, pos: S::SectionPos) -> S;
    pub fn index_from_pos(&self, pos: S::SectionPos) -> usize;   // ((y << bits | z) << bits) | x
}
// BlockState: size_bits()=4 → 4096 entries, index = y*256 + z*16 + x  (YZX order)
// Biome:      size_bits()=2 → 64 entries,  index = y*16 + z*4 + x
// Palette kinds for BlockState by bits_per_entry: 0=SingleValue, 1..=4 Linear, 5..=8 Hashmap, else Global
// Palette kinds for Biome: 0=SingleValue, 1..=3 Linear, else Global

pub struct BitStorage { pub data: Box<[u64]>, /* bits, mask, size, values_per_long: private */ }
impl BitStorage {
    pub fn get(&self, index: usize) -> u64;    // panics if index >= size; returns 0 for 0-bit storage
    pub fn size(&self) -> usize;
    pub fn iter(&self) -> BitStorageIter<'_>;  // Iterator<Item = u64>, sequential palette ids
}
```

Efficient full-section read for a mesher (compilable):

```rust
use azalea::block::BlockState;
use azalea::world::{Chunk, Section, palette::Palette};

/// Decode all 4096 block states of a section into a flat YZX array.
/// out[(y*16 + z)*16 + x]
fn decode_section_blocks(section: &Section, out: &mut [BlockState; 4096]) {
    if section.block_count == 0 && matches!(&section.states.palette, Palette::SingleValue(v) if v.is_air()) {
        out.fill(BlockState::AIR);
        return;
    }
    match &section.states.palette {
        Palette::SingleValue(v) => out.fill(*v),
        // Linear/Hashmap are both plain Vec<BlockState> indexed by palette id:
        Palette::Linear(vals) | Palette::Hashmap(vals) => {
            for (i, id) in section.states.storage.iter().enumerate() {
                out[i] = vals.get(id as usize).copied().unwrap_or_default();
            }
        }
        Palette::Global => {
            for (i, id) in section.states.storage.iter().enumerate() {
                out[i] = BlockState::try_from(id as u32).unwrap_or_default();
            }
        }
    }
}

fn mesh_chunk(chunk: &Chunk, min_y: i32) {
    let mut buf = [BlockState::AIR; 4096];
    for (i, section) in chunk.sections.iter().enumerate() {
        if section.block_count == 0 { continue; }         // fast skip empty sections
        let section_min_y = min_y + (i as i32) * 16;
        decode_section_blocks(section, &mut buf);
        let _ = section_min_y;
        // ... feed buf to mesher
    }
}
```

(`BitStorage::iter()` calls `get()` per index; if that's too slow you can unpack
`storage.data` yourself: `values_per_long = 64 / bits_per_entry`, entries are
LSB-first within each u64, never split across u64s — same layout as vanilla
1.16+. `bits_per_entry` is on the container.)

`BlockState` (azalea-block): `Copy`, private `id: u16` (`BlockStateIntegerRepr = u16`), `pub const AIR`, `pub const fn id(&self) -> u16`, `pub fn is_air(&self) -> bool`, `TryFrom<u32>/TryFrom<u16>`, `From<BlockState> for u32`. Use `state.id()` as the global palette index for your renderer's block-model lookup.

---

## 3. Section Y range / world height

- `world.chunks.min_y() -> i32` and `world.chunks.height() -> u32` (on `ChunkStorage` via deref). Overworld 26.1: min_y = -64, height = 384 → 24 sections.
- `sections[i]` covers block Y in `[min_y + 16*i, min_y + 16*i + 16)`.
- `azalea_world::chunk::section_index(y, min_y) -> u32` maps block Y → section index.
- These values come from the **dimension type registry**: on login/respawn azalea resolves `p.common.dimension_type(registries)` to `azalea_core::registry_holder::dimension_type::DimensionKindElement { pub height: u32, pub min_y: i32, pub has_skylight: bool, pub ambient_light: f32, pub logical_height: u32, ... }` and passes `height`/`min_y` into `Worlds::get_or_insert`. You can read it yourself: `world.registries.dimension_type.map` is an `IndexMap<Identifier, DimensionKindElement>`.
- `ChunkStorage::default()` is 384/-64; a `Chunk::default()` also assumes 384.

```rust
use azalea::Client;

fn world_bounds(bot: &Client) -> (i32, u32, usize) {
    let world = bot.world();
    let world = world.read();
    let min_y = world.chunks.min_y();
    let height = world.chunks.height();
    (min_y, height, (height / 16) as usize) // e.g. (-64, 384, 24)
}
```

---

## 4. Biomes

- Stored per section as `Section.biomes: PalettedContainer<Biome>`, 4x4x4 = 64 entries (each covering a 4-block cube). Index order YZX with 2 size bits: `index = y*16 + z*4 + x`, coords 0..=3.
- `azalea_registry::data::Biome` is a data-driven registry newtype: `pub struct Biome { id: u32 }` with `Copy`, `Default` (id 0), `From<u32>`, `Into<u32>`, and `DataRegistry` (`Biome::NAME == "worldgen/biome"`, `fn protocol_id(&self) -> u32`, `fn new_raw(id: u32) -> Self`). **It is NOT a compile-time enum** — biome ids are per-server (data-driven), so you must resolve names through the world's `RegistryHolder`.
- Per-block accessors: `World::get_biome(BlockPos) -> Option<Biome>`, `Chunk::get_biome(ChunkBiomePos, min_y)`, `Section::get_biome(ChunkSectionBiomePos)`.
- Position types (azalea_core::position): `ChunkBiomePos { x: u8, y: i32, z: u8 }` (`From<BlockPos>` divides x,z by 4… via ChunkBlockPos), `ChunkSectionBiomePos { x,y,z: u8 }` (0..=3).
- Name/data resolution:
  - `bot.resolve_registry_key(&biome) -> Option<BiomeKey>` (`BiomeKey` enum with all vanilla biomes + `Other(Identifier)`),
  - `bot.with_resolved_registry(biome, |name: &Identifier, data: &simdnbt::owned::NbtCompound| ...)` — Biome's `DeserializesTo = NbtCompound` (raw NBT: contains `temperature`, `downfall`, `effects.{sky_color, water_color, grass_color, foliage_color, fog_color}` etc. — you must read the NBT yourself; azalea has no typed BiomeData struct).
  - Without a Client: `world.registries.protocol_id_to_identifier(Identifier::from("minecraft:worldgen/biome"), biome.protocol_id())`; the biome NBT lives in `world.registries.extra[&Identifier::from("minecraft:worldgen/biome")].map` (an `IndexMap<Identifier, NbtCompound>`, indexed by protocol id order → `map.get_index(id as usize)`).

```rust
use azalea::Client;
use azalea::core::position::BlockPos;
use azalea::registry::{DataRegistry, identifier::Identifier};

fn biome_at(bot: &Client, pos: BlockPos) {
    let world = bot.world();
    let world = world.read();
    if let Some(biome) = world.get_biome(pos) {
        let id: u32 = biome.protocol_id();
        let name: Option<Identifier> = world
            .registries
            .protocol_id_to_identifier(Identifier::from("minecraft:worldgen/biome"), id)
            .cloned();
        println!("biome id {id} = {name:?}");
    }
}
```

For grass/foliage/water tint in the renderer: read the biome registry NBT once
per world (`registries.extra`), build `Vec<BiomeColors>` indexed by protocol id,
then per-section `section.biomes.get_at_index(i)` → `protocol_id()` → table lookup.

---

## 5. LIGHT — **NOT stored by azalea** (must capture packets yourself)

Grepped every file in `azalea-world-0.16.0+mc26.1/src`: **zero** occurrences of
"light". `Chunk`/`Section` have no light fields.

In `azalea-client`:
- `level_chunk_with_light` handler forwards the packet to `ReceiveChunkEvent`; `handle_receive_chunk_event` (plugins/chunks.rs) reads **only** `packet.chunk_data` (heightmaps + section data). `packet.light_data` is dropped.
- `light_update` handler is an explicit no-op: `pub fn light_update(&mut self, _p: &ClientboundLightUpdate) {}` (packet/game/mod.rs:553).

So a renderer must consume the raw packets. Available data
(azalea-protocol `packets::game::{c_level_chunk_with_light, c_light_update}`):

```rust
pub struct ClientboundLevelChunkWithLight {
    pub x: i32, pub z: i32,
    pub chunk_data: ClientboundLevelChunkPacketData, // heightmaps, data: Arc<Box<[u8]>>, block_entities
    pub light_data: ClientboundLightUpdatePacketData,
}
pub struct ClientboundLightUpdate { pub x: i32 /*#[var]*/, pub z: i32 /*#[var]*/, pub light_data: ClientboundLightUpdatePacketData }
pub struct ClientboundLightUpdatePacketData {
    pub sky_y_mask: BitSet,          // azalea_core::bitset::BitSet; .index(i)->bool, .len(), .iter_ones()
    pub block_y_mask: BitSet,
    pub empty_sky_y_mask: BitSet,    // sections whose sky light is all-zero
    pub empty_block_y_mask: BitSet,
    pub sky_updates: Arc<Box<[Box<[u8]>]>>,   // one 2048-byte nibble array per set bit in sky_y_mask, bottom-up
    pub block_updates: Arc<Box<[Box<[u8]>]>>,
}
```

Vanilla semantics (masks are NOT interpreted anywhere in azalea, you implement
them): light sections span `height/16 + 2` entries — index 0 is the section
*below* the world (y = min_y-16), index `height/16 + 1` is above the top.
Mask bit i set → next array from `sky_updates` (in order) belongs to light-section i.
Nibble layout: 2048 bytes, index = y*256 + z*16 + x (same YZX as blocks),
value = `(arr[idx>>1] >> ((idx&1)*4)) & 0xF`.

Capturing (compilable):

```rust
use std::sync::Arc;
use azalea::Client;
use azalea::events::Event;
use azalea::protocol::packets::game::{
    ClientboundGamePacket, c_light_update::ClientboundLightUpdatePacketData,
};

fn get_light(arr: &[u8], x: usize, y: usize, z: usize) -> u8 {
    let idx = (y << 8) | (z << 4) | x;
    (arr[idx >> 1] >> ((idx & 1) * 4)) & 0xF
}

/// call from your azalea event handler
fn handle(_bot: Client, event: Event, light_store: &mut Vec<(i32, i32, ClientboundLightUpdatePacketData)>) {
    if let Event::Packet(packet) = &event {
        match packet.as_ref() {
            ClientboundGamePacket::LevelChunkWithLight(p) => {
                light_store.push((p.x, p.z, p.light_data.clone()));
            }
            ClientboundGamePacket::LightUpdate(p) => {
                // partial update: only sections with mask bits set are present
                light_store.push((p.x, p.z, p.light_data.clone()));
            }
            _ => {}
        }
    }
    let _ = Arc::strong_count; // silence unused import in doc snippet contexts
}
```

Then to read sky light for world-section i (0-based from min_y) of a stored
`ClientboundLightUpdatePacketData d`: light-section index `li = i + 1`;
if `d.sky_y_mask.index(li)` → the nibble array is
`d.sky_updates[d.sky_y_mask.iter_ones().position(|b| b == li).unwrap()]`;
else if `d.empty_sky_y_mask.index(li)` → all zeros; else → unchanged from previous data (for LevelChunkWithLight, servers send all non-empty sections).
`BitSet` API: `pub fn index(&self, index: usize) -> bool` (false when out of range… actually panics? it's backed by `Vec<u64>` — use `.get(i) -> Option<bool>` if unsure; both exist).

---

## 6. Heightmaps

Stored on every chunk: `Chunk.heightmaps: HashMap<HeightmapKind, Heightmap>`.
Servers typically send only `WorldSurface` and `MotionBlocking` to clients.

- `HeightmapKind` is `azalea_core::heightmap_kind::HeightmapKind` (enum: `WorldSurfaceWg, WorldSurface, OceanFloorWg, OceanFloor, MotionBlocking, MotionBlockingNoLeaves`). The old `azalea_world::heightmap::HeightmapKind` is a deprecated alias.
- `azalea_world::heightmap::Heightmap`:

```rust
pub struct Heightmap { pub data: BitStorage, pub min_y: i32, pub kind: HeightmapKind }
impl Heightmap {
    pub fn get_first_available(&self, x: u8, z: u8) -> i32; // lowest air Y above surface
    pub fn get_highest_taken(&self, x: u8, z: u8) -> i32;   // Y of top block
    pub fn iter_highest_taken(&self) -> impl Iterator<Item = ChunkBlockPos> + '_;
}
```

Azalea keeps them updated on block changes (`Heightmap::update` is called from `Chunk::set_block_state`).

```rust
use azalea::core::heightmap_kind::HeightmapKind;
use azalea::world::Chunk;

fn surface_y(chunk: &Chunk, x: u8, z: u8) -> Option<i32> {
    chunk.heightmaps.get(&HeightmapKind::WorldSurface)
        .map(|hm| hm.get_highest_taken(x, z))
}
```

---

## 7. Chunk load/unload + block change notifications

High-level (`azalea::events::Event`, the enum your handler receives):
- **Chunk loaded/updated**: `Event::ReceiveChunk(ChunkPos)` — fired (after the chunk is parsed into the world? No—) fired when the `ReceiveChunkEvent` ECS message is written; it is emitted the same `Update` in which `handle_receive_chunk_event` runs, and event-channel delivery happens after systems, so by the time your async handler sees it the chunk IS in the world. Re-sent chunks (full chunk resend) also fire this.
- **Chunk unloaded**: NO dedicated `Event` variant. Listen to raw packets: `Event::Packet(p)` with `ClientboundGamePacket::ForgetLevelChunk(ClientboundForgetLevelChunk { pos: ChunkPos })`. Azalea's handler only clears the partial storage slot (`partial_world.chunks.limited_set(&p.pos, None)`); the shared Weak entry dies when the last strong ref drops.
- **Block changes**: NO dedicated `Event` variant. Listen to `Event::Packet`:
  - `ClientboundGamePacket::BlockUpdate(ClientboundBlockUpdate { pos: BlockPos, block_state: BlockState })`
  - `ClientboundGamePacket::SectionBlocksUpdate(ClientboundSectionBlocksUpdate { section_pos: ChunkSectionPos, states: Vec<BlockStateWithPosition { pos: ChunkSectionBlockPos, state: BlockState }> })` (multi-block change; absolute pos = `p.section_pos + s.pos` → BlockPos)
  Azalea applies these to the world itself via the `QueuedServerBlockUpdates` component (system `handle_block_update_event`, runs in `Update` after chunk receive) — you only need the packets to know *what* to re-mesh, not to apply them.

ECS-level alternatives (if you write a bevy plugin instead of the async handler):
`azalea_client::chunks::ReceiveChunkEvent { entity: Entity, packet: ClientboundLevelChunkWithLight }` (a bevy `Message` — read with `MessageReader`; this one DOES carry `light_data`), and `azalea_client::block_update::QueuedServerBlockUpdates { pub list: Vec<(BlockPos, BlockState)> }` component (drained by azalea's own system, so read it in a system ordered after `read_packets`/before `handle_block_update_event` — ordering is fiddly; prefer `Event::Packet`).

```rust
use azalea::Client;
use azalea::events::Event;
use azalea::protocol::packets::game::ClientboundGamePacket;
use azalea::core::position::{BlockPos, ChunkPos};

async fn handle(bot: Client, event: Event) -> eyre::Result<()> {
    match event {
        Event::ReceiveChunk(pos) => {
            // chunk at `pos` is now readable:
            let world = bot.world();
            let has = world.read().chunks.get(&pos).is_some();
            println!("chunk {pos:?} loaded: {has}");
        }
        Event::Packet(packet) => match packet.as_ref() {
            ClientboundGamePacket::ForgetLevelChunk(p) => {
                println!("chunk unloaded: {:?}", p.pos);
            }
            ClientboundGamePacket::BlockUpdate(p) => {
                println!("block {:?} -> {:?}", p.pos, p.block_state);
            }
            ClientboundGamePacket::SectionBlocksUpdate(p) => {
                for s in &p.states {
                    let abs: BlockPos = p.section_pos + s.pos;
                    println!("block {abs:?} -> {:?}", s.state);
                }
            }
            _ => {}
        },
        _ => {}
    }
    let _ = ChunkPos::new(0, 0);
    Ok(())
}
```

Also relevant: `Event::Login` fires per world join; the world is swapped on
respawn/dimension change (`WorldHolder.shared` is replaced, `partial` reset) — a
renderer should flush all meshes on `Event::Login` and on
`ClientboundGamePacket::Respawn`. There's also `WorldLoadedEvent` (ECS message,
`azalea_client::packet::game::events` — carries `Weak<RwLock<World>>`).

`PartialChunkStorage` extras useful for renderers:
`view_center() -> ChunkPos`, `chunk_radius` (private, but `view_range() -> u32` = 2r+1),
`limited_get(&ChunkPos) -> Option<&Arc<RwLock<Chunk>>>`, `chunks() -> impl Iterator<Item = &Option<Arc<RwLock<Chunk>>>>`,
`in_range(&ChunkPos) -> bool`. Stored range = `max(view_distance, 2) + 3` (`calculate_chunk_storage_range`).
Iterating loaded chunk positions from the shared world: `world.chunks.chunks()` returns keys but includes possibly-dead Weak entries — filter with `.get()`.

---

## Open questions

1. **`Event::ReceiveChunk` ordering vs. world state**: the event message is written by the packet handler in `PreUpdate`-ish (`as_system` during packet read) while the chunk is inserted by `handle_receive_chunk_event` in `Update`. Both happen before the tokio channel forwarder system (`events.rs` reads `ReceiveChunkEvent` messages) in the *same or next* Update. I did not fully trace bevy system ordering to guarantee the chunk is queryable at the moment your async handler runs; empirically it is (handler runs on a different task after the Update completes), but verify at runtime before relying on it for meshing (safe fallback: re-check `world.chunks.get(pos).is_some()`).
2. **Light-section count**: the `height/16 + 2` light-section convention (one below, one above) is the vanilla protocol spec; azalea never decodes the masks so this is untested against azalea types. Verify with a real server dump (check `sky_y_mask.len()` ≈ 26 for the overworld).
3. **`BitSet::index` out-of-range behavior**: azalea-core's `BitSet::index(i)` computes `self.data[i/64]` — likely panics if the mask Vec is shorter than expected; use `.get(i).unwrap_or(false)` (`get` exists, returns `Option<bool>`) to be safe. Not verified which servers send truncated masks.
4. **Biome NBT schema**: exact NBT field names inside the biome registry (`effects/sky_color` etc.) come from vanilla; azalea stores it as opaque `NbtCompound` in `registries.extra`, so double-check field names against a live 26.1 server's registry data (also confirm whether the 26.1 key is still `minecraft:worldgen/biome`).
5. **`fluid_count`** is populated from the packet but never updated by azalea after block changes — don't rely on it for skipping fluid meshing after edits.
6. **BitStorage packing**: entries never straddle u64 boundaries (verified from `values_per_long = 64/bits` + `cell_index` math), LSB-first — matches vanilla 1.16+; a manual fast unpacker is safe, but note bits_per_entry can be any of 0..=8 for blocks plus ~15 for Global palette (Global uses `ceil_log2(MAX_STATE)`-ish bits — actual bpe comes from the packet, just trust `container.bits_per_entry`).
