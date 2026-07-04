# azalea 0.16.0+mc26.1 — Entities in the ECS (verified against registry source)

Source read from `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/azalea-*-0.16.0+mc26.1`.
azalea 0.16 uses **bevy_ecs 0.18.1** and **parking_lot 0.12** — pin `bevy_ecs = "0.18"` (or use the re-export `azalea::ecs` = `bevy_ecs`, `azalea::app` = `bevy_app`) to avoid trait mismatches.

Key 0.16 facts:
- `azalea::Client` (defined in `azalea-0.16.0/src/client_impl/mod.rs`) is `#[derive(Clone)] pub struct Client { pub entity: bevy_ecs::entity::Entity, pub ecs: Arc<parking_lot::RwLock<bevy_ecs::world::World>> }`. There is no tokio lock; `client.ecs.read()` / `.write()` are parking_lot guards (do NOT hold across `.await`).
- `Instance` was renamed to `World` (`azalea_world::World`); the bevy ECS world is `bevy_ecs::world::World` — two different `World` types, alias on import.
- `azalea::EntityRef` (`azalea-0.16.0/src/entity_ref/mod.rs`) = `{ client: Client, entity: Entity }`; NOT bevy's `bevy_ecs::world::EntityRef`. Methods: `id() -> Entity`, `component::<T>() -> MappedRwLockReadGuard<T>` (panics if absent), `get_component::<T>() -> Option<...>`, `query_self::<D, R>(f)`, `try_query_self`, `kind() -> EntityKind`, `position() -> Vec3`, `eye_position()`, `dimensions() -> EntityDimensions`, `uuid() -> Uuid`, `minecraft_id() -> MinecraftEntityId`, `physics() -> Physics`, `health() -> f32`, `world_name() -> WorldName`, `is_alive()`, `exists()`, `attack()`, `interact()`, `look_at()`, `distance_to_client()`.

## 1. Querying all entities from outside the handler

`Client` helpers (all in `azalea/src/client_impl/entity_query.rs`):
```text
Client::component::<T>() -> MappedRwLockReadGuard<'_, T>                  // client entity, panics
Client::get_component::<T>() -> Option<MappedRwLockReadGuard<'_, T>>
Client::entity_component::<T>(entity: Entity) -> MappedRwLockReadGuard<'_, T>
Client::get_entity_component::<T>(entity: Entity) -> Option<...>
Client::query_self::<D: QueryData, R>(f: impl FnOnce(QueryItem<D>) -> R) -> R
Client::query_entity::<D, R>(entity: Entity, f) -> R
Client::try_query_entity::<D, R>(entity, f) -> Result<R, QueryEntityError>
Client::any_entity_by / nearest_entity_by / nearest_entities_by::<Q, F>(predicate) -> ... EntityRef
Client::nearest_entity_ids_by::<Q, F>(predicate) -> Box<[Entity]>        // sorted nearest-first
Client::entity_by_uuid(Uuid) -> Option<EntityRef>; entity_by_minecraft_id(MinecraftEntityId)
```

Full scan of every entity — compilable snippet (this is what a renderer bridge should do each frame):

```rust
use azalea::Client;
use azalea::ecs::prelude::*; // bevy_ecs re-export
use azalea::entity::{
    EntityKindComponent, EntityUuid, LocalEntity, LookDirection, Physics, Position,
    metadata::{CustomName, Pose},
};
use azalea::player::GameProfileComponent; // azalea_client::player
use azalea::world::WorldName; // azalea_world::WorldName(pub Identifier)
use azalea_core::entity_id::MinecraftEntityId;

pub fn snapshot_entities(client: &Client) {
    // our own world name, so we can skip entities from other dimensions/swarm clients
    let my_world: Option<WorldName> = client.get_component::<WorldName>().map(|w| w.clone());

    // World::query* needs &mut World, so take the write lock
    let mut ecs = client.ecs.write();
    let mut query = ecs.query::<(
        Entity,
        &MinecraftEntityId,     // component: azalea_core::entity_id::MinecraftEntityId(pub i32)
        &EntityKindComponent,   // .0 / deref -> azalea_registry::builtin::EntityKind
        &EntityUuid,            // deref -> uuid::Uuid
        &Position,              // deref -> azalea_core::position::Vec3 (f64 x/y/z), feet pos
        &LookDirection,         // .y_rot() yaw deg, .x_rot() pitch deg (fields are private!)
        &Physics,               // .velocity: Vec3, .bounding_box: Aabb, .on_ground()
        &WorldName,
        Option<&Pose>,                 // metadata; present on server-spawned entities
        Option<&CustomName>,           // CustomName(pub Option<Box<FormattedText>>)
        Option<&GameProfileComponent>, // players only (deref -> GameProfile)
        Option<&LocalEntity>,          // marker: this is one of OUR clients
    )>();
    for (ent, mc_id, kind, uuid, pos, look, phys, world_name, pose, name, profile, local) in
        query.iter(&ecs)
    {
        if my_world.as_ref() != Some(world_name) {
            continue; // entity is in another dimension (or another swarm client's world)
        }
        if local.is_some() { /* the local player: render in first person instead */ }
        let display = profile
            .map(|p| p.name.clone())
            .or_else(|| name.and_then(|n| n.0.as_ref().map(|t| t.to_string())));
        println!(
            "{ent} id={} kind={} uuid={} pos={:?} yaw={} pitch={} vel={:?} pose={:?} name={display:?}",
            mc_id.0, kind.0, **uuid, **pos, look.y_rot(), look.x_rot(), phys.velocity, pose.copied()
        );
    }
}
```
Notes:
- `EntityKind` is `azalea_registry::builtin::EntityKind` (path `azalea_registry::EntityKind` still works but is `#[deprecated]`). It has `to_str() -> &'static str` (`"minecraft:zombie"`), `Display`, `Registry::to_u32()`, `TryFrom<u32>`, `FromStr`.
- `LookDirection` fields `y_rot`/`x_rot` are **private in 0.16**; use `look.y_rot()` (yaw, deg, unclamped) and `look.x_rot()` (pitch, deg, clamped ±90). `From<LookDirection> for (f32, f32)` gives `(y_rot, x_rot)`.
- `Position` derefs to `Vec3`; `BlockPos::from(&pos)` / `ChunkPos::from(&pos)` conversions exist.
- Filter for one dimension: compare `&WorldName` (component on every entity) with `client.component::<WorldName>()`; that's exactly what azalea's own `EntityPredicate::find_any` does (`query_filtered::<(Entity, &WorldName, Q), F>`).
- Simpler filtered queries: `ecs.query_filtered::<(Entity, &Position), (With<Player>, Without<LocalEntity>)>()` with `azalea::entity::metadata::Player` marker.

## 2. Which components are on remote entities vs local player only

Every entity (inserted from `ClientboundAddEntity` via `EntityBundle` in `azalea-entity/src/plugin/components.rs`, plus indexing/packet handler):
- `EntityKindComponent(pub EntityKind)`, `EntityUuid`, `WorldName`, `Position`, `LastSentPosition`, `EntityChunkPos`, `Physics`, `LookDirection`, `EntityDimensions { width, height, eye_height, fixed }`, `Attributes`, `Jumping(pub bool)`, `Crouching(bool)`, `FluidOnEyes(FluidKind)`, `OnClimbable(bool)`, `ActiveEffects`
- Added by the packet handler at spawn: `MinecraftEntityId(pub i32)` (this IS a component), `LoadedBy(pub HashSet<Entity>)`, and the kind-specific default metadata bundle via `apply_default_metadata` (so `Pose`, `CustomName`, `OnFire`, `Invisible`, `Sprinting`, `Swimming`, `FallFlying`, `Health` (living), `ItemItem` (item entities), the type-tree markers `AbstractEntity`/`AbstractLiving`/`Player`/… — see `azalea::entity::metadata`).
- Remote entities that are NOT local also get `UpdatesReceived(u32)` (swarm dedup counter; absent on local entities).
- Players additionally: `GameProfileComponent(pub GameProfile)` if the UUID was in the tab list at spawn time (or added retroactively on `AddPlayerEvent`), plus player metadata (`PlayerModeCustomisation(pub u8)` = skin-layer bitmask, `PlayerAbsorption`, `Score`, ...).
- `Dead` marker added when `Health <= 0`.

Local player only (from `JoinedClientBundle` in `azalea-client/src/client.rs` + `LocalPlayerBundle` in plugins/join.rs):
- `LocalEntity` (marker, azalea_entity), `PhysicsState`, `Inventory`, `TabList`, `PlayerAbilities`, `Hunger`, `Experience`, `PermissionLevel`, `EntityIdIndex`, `LocalGameMode`, `InGameState`/`InConfigState`, `HasClientLoaded`, `InLoadedChunk` (actually set for all entities in loaded chunks, but used for players), `TicksConnected`, `RawConnection`, `WorldHolder`, `LocalPlayerEvents`, `Account`, `PlayerMetadataBundle` (the local player's metadata is inserted at join, not from packets).
- `LastSentPosition` exists on ALL entities (part of `EntityBundle`) but per its doc comment "is currently only updated for our own local player entities" — do not use it for remote interpolation.
- `Pose` is a real component (`azalea::entity::Pose`, enum `Standing=0, FallFlying, Sleeping, Swimming, SpinAttack, Crouching, ...`) present on any entity whose metadata bundle was applied (i.e. all server-spawned entities and the local player).
- Physics simulation (`azalea-physics`) only runs for `(With<LocalEntity>, With<HasClientLoaded>)` — remote entities' `Physics.velocity` is only what the server sends in `SetEntityMotion`/spawn, and gravity is NOT simulated for them.

## 3. Interpolation: none — azalea snaps positions

Handlers in `azalea-client/src/plugins/packet/game/mod.rs`:
- `move_entity_pos` / `move_entity_pos_rot` / `move_entity_rot` (`ClientboundMoveEntityPos{,Rot}`) → `fn move_entity(...)` decodes the delta with `physics.vec_delta_codec.decode(&delta)`, calls `vec_delta_codec.set_base(new_position)`, then **directly writes** `**position = new_position` and `*look_direction = new` and `physics.set_on_ground(...)`. No lerp, no target/step fields (vanilla's `lerpTo`/`posRot` interpolation is not implemented).
- `entity_position_sync` (`ClientboundEntityPositionSync`) and `teleport_entity` (`ClientboundTeleportEntity`) also write `Position`/`LookDirection` immediately (teleport also sets `physics.old_position` to pre-teleport pos via `set_old_pos`).
- All of these go through `RelativeEntityUpdate` / `should_apply_entity_update` (swarm dedup via `UpdatesReceived`); updates are skipped for `LocalEntity` entities.
- `Physics.old_position` is "position before it moved this tick", but it is only maintained by the physics systems (local players) and teleports — for remote entities it is stale.

**Renderer consequence:** you must interpolate yourself. Packets are applied during the `Update` schedule which azalea runs at ~60 Hz (`run_schedule_loop` in azalea-client/src/client.rs; GameTick at 20 Hz). Practical approach: each render frame, read `Position` per entity, keep your own `prev/curr + timestamp` ring per `Entity`, and lerp with ~50–100 ms delay. `Changed<Position>` detection from outside is unreliable (see §5 caveat).

## 4. Player skins (GameProfile textures)

`azalea_auth::game_profile::GameProfile { uuid: Uuid, name: String, properties: Arc<GameProfileProperties> }`;
`GameProfileProperties { map: IndexMap<String, ProfilePropertyValue> }`; `ProfilePropertyValue { value: String /* base64 JSON */, signature: Option<String> }`.
The `"textures"` property value is base64 of `{"timestamp":...,"profileId":"...","profileName":"...","textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/<hash>","metadata":{"model":"slim"}?},"CAPE":{"url":...}?}}`. azalea does NOT decode it for you — decode it yourself (deps: `base64`, `serde`, `serde_json`).

Two sources:
- Tab list: `client.tab_list() -> HashMap<Uuid, PlayerInfo>` (clones `TabList` component; `TabList` is also an ECS `Resource`). `azalea_client::player::PlayerInfo { profile: GameProfile, uuid, gamemode: GameMode, latency: i32, display_name: Option<Box<FormattedText>> }`. Covers players even outside render distance.
- Per spawned player entity: `GameProfileComponent(pub GameProfile)` (`azalea::player::GameProfileComponent`, Deref to GameProfile).

```rust
use azalea::Client;
use base64::Engine;
use serde::Deserialize;

#[derive(Deserialize)]
struct TexturesPayload { textures: TexturesMap }
#[derive(Deserialize, Default)]
struct TexturesMap {
    #[serde(rename = "SKIN")] skin: Option<Texture>,
    #[serde(rename = "CAPE")] cape: Option<Texture>,
}
#[derive(Deserialize)]
struct Texture { url: String, metadata: Option<TextureMeta> }
#[derive(Deserialize)]
struct TextureMeta { model: Option<String> } // "slim" => Alex arms

pub fn skin_urls(client: &Client) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for (_uuid, info) in client.tab_list() {
        let Some(prop) = info.profile.properties.map.get("textures") else { continue };
        let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(&prop.value) else { continue };
        let Ok(p) = serde_json::from_slice::<TexturesPayload>(&raw) else { continue };
        if let Some(skin) = p.textures.skin {
            let slim = skin.metadata.as_ref().and_then(|m| m.model.as_deref()) == Some("slim");
            out.push((info.profile.name.clone(), skin.url, slim));
        }
    }
    out
}
```
Offline-mode servers usually send an empty `properties` map → fall back to default Steve/Alex by UUID parity.

## 5. Entity added/removed detection from outside

How azalea manages lifecycle:
- Spawn: `ClientboundAddEntity` handler spawns `(MinecraftEntityId, LoadedBy(HashSet<client entity>), EntityBundle)` + metadata; if the entity already exists (swarm), it only adds our client to `LoadedBy`.
- Remove: `ClientboundRemoveEntities` only removes our client from `LoadedBy`; the actual `despawn()` happens in `azalea_entity::indexing::remove_despawned_entities_from_indexes` (PostUpdate, set `EntityUpdateSystems::Deindex`) once `LoadedBy` is empty (also on world unload). ECS despawn is therefore slightly delayed and swarm-safe.
- Events available on the `Client::join` receiver (`azalea::Event`): `AddPlayer/RemovePlayer/UpdatePlayer(PlayerInfo)` are TAB-LIST events, not entity spawn events. There is NO high-level per-entity spawn/despawn Event. `Event::Packet(Arc<ClientboundGamePacket>)` (feature `packet-event`, on by default) lets you watch `AddEntity`/`RemoveEntities` packets directly.

Practical outside-ECS approach — diff a snapshot (robust, no change-detection pitfalls):
```rust
use std::collections::HashSet;
use azalea::Client;
use azalea::ecs::prelude::*;
use azalea_core::entity_id::MinecraftEntityId;

#[derive(Default)]
pub struct EntityTracker { known: HashSet<Entity> }

impl EntityTracker {
    pub fn poll(&mut self, client: &Client) -> (Vec<Entity>, Vec<Entity>) {
        let mut ecs = client.ecs.write();
        let mut q = ecs.query::<(Entity, &MinecraftEntityId)>();
        let current: HashSet<Entity> = q.iter(&ecs).map(|(e, _)| e).collect();
        let added = current.difference(&self.known).copied().collect();
        let removed = self.known.difference(&current).copied().collect();
        self.known = current;
        (added, removed)
    }
}
```
Alternative (the "bevy way"): register your own plugin before starting — `ClientBuilder::new().add_plugins(MyPlugin)` (azalea/src/builder.rs `pub fn add_plugins<M>(mut self, plugins: impl Plugins<M>) -> Self`) — and inside use real systems with `Query<(Entity, ...), Added<MinecraftEntityId>>` and `RemovedComponents<MinecraftEntityId>`, forwarding over an `std::sync::mpsc`/`flume` channel to the render thread. Change-detection filters (`Added`/`Changed`) only work reliably inside scheduled systems that keep their `QueryState` alive; a fresh `world.query_filtered::<_, Added<T>>()` from outside compares against the world's last change tick and will miss/duplicate events — don't do that.

## 6. Item entities / falling blocks (components for rendering)

All in `azalea::entity::metadata` (azalea-entity/src/metadata.rs, generated):
- Dropped item (`minecraft:item`): marker `Item`, stack in `ItemItem(pub ItemStack)` (metadata index 8). `azalea_inventory::ItemStack` is `enum { Empty, Present(ItemStackData) }`, `ItemStackData { kind: azalea_registry::builtin::ItemKind, count: i32, component_patch: DataComponentPatch }`.
- Item frames: `ItemFrame` marker + `ItemFrameItem(pub ItemStack)`, `Rotation(pub i32)`, `ItemFrameDirection(pub Direction)`.
- Item display: `ItemDisplayItemStack(pub ItemStack)`; block display: `BlockDisplayBlockState(pub azalea_block::BlockState)`.
- TNT: `TntBlockState(pub BlockState)`; Enderman carried block: `CarryState(pub BlockState)`.
- **Falling block (`minecraft:falling_block`): the `BlockState` is NOT stored as a component.** Metadata only gives `StartPos(pub BlockPos)`. The state is in `ClientboundAddEntity.data: i32` ("object data"), which azalea's `as_entity_bundle` discards. Recover it via the packet event:

```rust
use std::collections::HashMap;
use azalea::{Client, Event};
use azalea::protocol::packets::game::ClientboundGamePacket;
use azalea::registry::builtin::EntityKind;
use azalea_block::BlockState;
use azalea_core::entity_id::MinecraftEntityId;

pub fn on_event(_client: &Client, event: &Event,
                falling_blocks: &mut HashMap<MinecraftEntityId, BlockState>) {
    if let Event::Packet(packet) = event {
        match &**packet {
            ClientboundGamePacket::AddEntity(p) if p.entity_type == EntityKind::FallingBlock => {
                if let Ok(state) = BlockState::try_from(p.data as u32) {
                    falling_blocks.insert(p.id, state);
                }
            }
            ClientboundGamePacket::RemoveEntities(p) => {
                for id in &p.entity_ids { falling_blocks.remove(id); }
            }
            _ => {}
        }
    }
}
```
Map `MinecraftEntityId -> Entity` afterwards with `client.entity_by_minecraft_id(id)` / the `MinecraftEntityId` component. (ECS alternative: `azalea_client::packet::game::ReceiveGamePacketEvent { entity, packet }` as a bevy `Message` in your own plugin.)

## Open questions
- **Change-tick semantics for outside queries**: my claim that fresh `Added<T>`/`Changed<T>` query states are unreliable from outside scheduled systems is based on bevy 0.18 change-detection design (QueryState `last_run` vs `world.last_change_tick()`), not on an azalea test. The snapshot-diff approach sidesteps this; verify before relying on ad-hoc `Added<>` filters.
- `Event::Packet` requires the `packet-event` feature — it IS in azalea's default features; only relevant if you use `default-features = false`.
- `p.data as u32 -> BlockState::try_from` for falling blocks: `TryFrom<u32> for BlockState` exists (azalea-block/src/block_state.rs:82); the vanilla protocol defines the data field as a raw block-state id, but I did not runtime-verify a falling-block spawn round-trip in 26.1.
- Head yaw: `ClientboundRotateHead` is a **no-op** in azalea 0.16 (`pub fn rotate_head(&mut self, _p) {}`), so body-vs-head yaw for rendering isn't tracked; only `LookDirection` is available. If you need head yaw, capture `RotateHead` packets yourself (field `y_head_rot: i8`, degrees = `i8 * 360 / 256`).
- `ClientboundAddEntity.movement: LpVec3` (initial velocity) and `y_head_rot` are also discarded by `as_entity_bundle`; spawn `x_rot`/`y_rot` (i8) are not applied to `LookDirection` at spawn either — remote entities start with `LookDirection::default()` until the first move/teleport packet. Capture the AddEntity packet if spawn orientation matters.
- Armor/equipment rendering: `ClientboundSetEquipment` handling was not investigated in this pass (no obvious equipment component in azalea-entity; check `set_equipment` handler if needed).
