# azalea 0.16.0+mc26.1 — Controlling the player

Verified against crate sources in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/azalea{,-client,-physics,-auth,-entity,-inventory,-core,-protocol,-registry}-0.16.0+mc26.1`.

Global facts that affect everything below:

- **Nightly Rust required.** `azalea` uses `#![feature(type_changing_struct_update)]`, `azalea-client` uses `error_generic_member_access`, `azalea-physics` uses `trait_alias`, `azalea-inventory` uses `min_specialization`. Edition 2024.
- 0.16 renames: `Instance` → `World` everywhere (`azalea_world::World`, `WorldName`, `WorldHolder`; `InstanceHolder` is a deprecated alias). `ServerAddress` → `ServerAddr`. `Client` lives in the **`azalea` crate root** (`azalea::Client`, defined in `azalea/src/client_impl/mod.rs`); `azalea_client` no longer has a `Client` struct.
- `Client` is `Clone` and holds `pub entity: bevy_ecs::entity::Entity` and `pub ecs: Arc<parking_lot::RwLock<bevy_ecs::world::World>>`.
- ECS deps: `bevy_ecs`/`bevy_app` **0.18.1**, `parking_lot` 0.12.5, `tokio` 1.50, `uuid` 1.22.
- azalea features (all default-on): `log`, `serde`, `packet-event`, `online-mode`.

---

## 1. Account creation (`azalea_client::account`, re-exported as `azalea::Account` via `azalea::prelude`)

```rust
// azalea-client/src/account/mod.rs
#[derive(Clone, Component, Debug)]
pub struct Account(Arc<dyn AccountTrait>);   // Derefs to dyn AccountTrait
// AccountTrait: username()->&str, uuid()->Uuid, access_token()->Option<String>, refresh(), join(...)
```

Constructors:

```rust
// offline (sync, no auth; UUID = azalea_crypto::offline::generate_uuid(username))
pub fn offline(username: &str) -> Account;

// Microsoft (async, feature "online-mode"; cache_key is arbitrary, typically the email)
pub async fn microsoft(cache_key: &str) -> Result<Account, azalea_auth::AuthError>;

pub async fn microsoft_with_opts(
    cache_key: &str,
    auth_opts: MicrosoftAccountOpts,           // azalea_client::account::microsoft::MicrosoftAccountOpts
) -> Result<Account, azalea_auth::AuthError>;

// bring-your-own token (skips azalea's cache; refreshes if expired)
pub async fn with_microsoft_access_token(
    msa: azalea_auth::cache::ExpiringValue<azalea_auth::AccessTokenResponse>,
) -> Result<Account, azalea_auth::AuthError>;
```

```rust
#[derive(Clone, Debug, Default)]
pub struct MicrosoftAccountOpts {
    pub check_ownership: bool,          // fails for Game Pass if true; default false
    pub cache_file: Option<PathBuf>,    // None => default cache file
    pub client_id: Option<String>,      // default "00000000441cc96b" (Nintendo Switch client)
    pub scope: Option<String>,          // default "service::user.auth.xboxlive.com::MBI_SSL"
}
```

**What `Account::microsoft` does interactively:** it is a **device-code flow, not a browser redirect**. `azalea_auth::auth()` → `interactive_get_ms_auth_token()` → `get_ms_link_code()` (POST `https://login.live.com/oauth20_connect.srf`, `response_type=device_code`), then it **`println!`s to stdout**: `Go to {verification_uri}?otc={user_code} and log in for {cache_key}` and polls `https://login.live.com/oauth20_token.srf` every `interval` seconds until login or timeout (`GetMicrosoftAuthTokenError::Timeout`). It does NOT open a browser.

**Token cache:** `~/.minecraft/azalea-auth.json` (from the `minecraft-folder-path` crate: `$HOME/.minecraft` on Linux, `%APPDATA%\.minecraft` on Windows, `~/Library/Application Support/minecraft` on macOS). Contains msa/xbl/mca tokens + profile; refreshed automatically when expired. Override location with `MicrosoftAccountOpts::cache_file`.

**For a GUI client** (show the code in your own UI instead of stdout):

```rust
use azalea::Account; // = azalea_client::account::Account

async fn login_gui() -> Result<Account, Box<dyn std::error::Error>> {
    let http = reqwest::Client::new();
    let code = azalea_auth::get_ms_link_code(&http, None, None).await?;
    // display these in your UI:
    println!("Open {} and enter code {}", code.verification_uri, code.user_code);
    let msa = azalea_auth::get_ms_auth_token(&http, code, None).await?; // polls until done
    Ok(Account::with_microsoft_access_token(msa).await?)
}
```

`DeviceCodeResponse { user_code, device_code, verification_uri, expires_in, interval }`.

---

## 2. Connecting

### ClientBuilder (azalea crate root)

```rust
impl ClientBuilder<NoState, ()> {
    pub fn new() -> Self;
    pub fn new_without_plugins() -> Self;
    pub fn set_handler<S, Fut, R>(self, handler: HandleFn<S, Fut>) -> ClientBuilder<S, R>
        where S: Default + Send + Sync + Clone + Component + 'static,
              Fut: Future<Output = R> + Send + 'static, R: Send + 'static;
}
impl<S, R> ClientBuilder<S, R> {
    pub fn set_state(self, state: S) -> Self;
    pub fn add_plugins<M>(self, plugins: impl Plugins<M>) -> Self;
    pub fn reconnect_after(self, delay: impl Into<Option<Duration>>) -> Self; // default 5 s
    pub async fn start(self, account: Account, address: impl ResolvableAddr) -> AppExit;
    pub async fn start_with_opts(self, account: Account, address: impl ResolvableAddr, opts: JoinOpts) -> AppExit;
}
// type HandleFn<S, Fut> = fn(Client, Event, S) -> Fut;
```

`start()` **never returns** while the bot runs (auto-reconnect keeps the swarm alive; call `client.exit()` to make it return `AppExit`). Internally it creates its own `tokio::task::LocalSet` and `run_until`s it, so it is safe to call from any multithreaded tokio runtime — it just blocks that task.

```rust
use azalea::prelude::*; // Account, Client, ClientBuilder, Event, Component, GameTick, ...

#[derive(Clone, Component, Default)]
pub struct State;

#[tokio::main]
async fn main() {
    let account = Account::offline("dolphin");
    ClientBuilder::new()
        .set_handler(handle)
        .start(account, "localhost:25565") // &str impls ResolvableAddr
        .await;
}

async fn handle(bot: Client, event: Event, _state: State) -> eyre::Result<()> {
    if let Event::Spawn = event { bot.chat("hello"); }
    Ok(())
}
```

### Joining from an existing runtime without the builder: `Client::join`

```rust
// azalea/src/client_impl/mod.rs
pub async fn join(account: Account, address: impl ResolvableAddr)
    -> Result<(Client, tokio::sync::mpsc::UnboundedReceiver<Event>), azalea_protocol::resolve::ResolveError>;
```

**CAVEAT:** `Client::join` calls `azalea_client::start_ecs_runner`, which uses `tokio::task::spawn_local` and **panics if not inside a `tokio::task::LocalSet` (or `LocalRuntime`)**. The tick loop lives on that LocalSet, so keep it alive forever. Pattern for a wgpu app (azalea on its own thread):

```rust
use azalea::{Account, Client, Event};
use tokio::task::LocalSet;

fn spawn_bot_thread(tx_to_render: std::sync::mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = LocalSet::new();
        local.block_on(&rt, async move {
            let account = Account::offline("dolphin");
            let (client, mut rx) = Client::join(account, "localhost:25565").await.unwrap();
            let _ = client; // Client is Clone; can be sent to other threads
            while let Some(event) = rx.recv().await {
                let _ = tx_to_render.send(event);
            }
            // when rx closes the loop ends; LocalSet (and game ticks) die with it
        });
    });
}
```

`Client` methods are thread-safe (everything goes through `Arc<RwLock<bevy_ecs::world::World>>`), so the render thread can hold a `Client` clone and poll components each frame.

### Address parsing (`azalea_protocol::address`)

```rust
pub struct ServerAddr { pub host: String, pub port: u16 }       // renamed from ServerAddress
impl TryFrom<&str> for ServerAddr { type Error = ServerAddrParseError; } // "host" or "host:port"; port defaults to 25565
impl From<SocketAddr> for ServerAddr;
pub struct ResolvedAddr { pub server: ServerAddr, pub socket: SocketAddr }
impl ResolvedAddr { pub async fn new(server: impl Into<ServerAddr>) -> Result<Self, ResolveError>; } // SRV+DNS
pub trait ResolvableAddr { fn server_addr(self) -> Result<ServerAddr, ResolveError>;
                           fn resolve(self) -> impl Future<Output = Result<ResolvedAddr, ResolveError>> + Send; }
// impl'd for &str, String, ServerAddr, SocketAddr, &ResolvedAddr
```

```rust
use azalea::protocol::address::ServerAddr;
let addr = ServerAddr::try_from("play.example.com:25566").unwrap();
assert_eq!((addr.host.as_str(), addr.port), ("play.example.com", 25566));
```

---

## 3. Movement & reading physics state

### Commands (all on `azalea::Client`, all sync, from `azalea/src/client_impl/movement.rs` + `azalea/src/bot.rs`)

```rust
pub fn walk(&self, direction: WalkDirection);        // WalkDirection::None stops
pub fn sprint(&self, direction: SprintDirection);
pub fn set_jumping(&self, jumping: bool);            // like holding space
pub fn jumping(&self) -> bool;
pub fn jump(&self);                                  // BotPlugin: jump exactly once next tick
pub fn set_crouching(&self, crouching: bool);
pub fn crouching(&self) -> bool;
pub fn set_direction(&self, y_rot: f32, x_rot: f32); // yaw -180..180, pitch -90..90 (degrees)
pub fn direction(&self) -> LookDirection;
pub fn look_at(&self, position: Vec3);               // BotPlugin; use BlockPos::center()
```

Enums (defined in `azalea_physics::local_player`, re-exported as `azalea_client::{WalkDirection, SprintDirection, PhysicsState}` and therefore also `azalea::{WalkDirection, SprintDirection, PhysicsState}`):

```rust
pub enum WalkDirection { #[default] None, Forward, Backward, Left, Right,
                         ForwardRight, ForwardLeft, BackwardRight, BackwardLeft }
pub enum SprintDirection { Forward, ForwardRight, ForwardLeft }
```

### Ticking — automatic 20 TPS

`azalea_client::start_ecs_runner` spawns `run_schedule_loop` (azalea-client/src/client.rs), which runs the Bevy `Update` schedule at **60 Hz** and the `azalea_core::tick::GameTick` schedule at **20 TPS** automatically once the client starts (both `ClientBuilder::start` and `Client::join` do this for you). `azalea_physics::PhysicsPlugin` registers `(update_in_water_state_and_do_fluid_pushing, update_old_position, update_swimming, ai_step, travel::travel, apply_effects_from_blocks).chain()` on `GameTick` in the `PhysicsSystems` SystemSet; `azalea_client::movement` sends `send_position` / `send_player_input_packet` / `send_sprinting_if_needed` each `GameTick`. You never step physics yourself. `set_direction`/`walk` just mutate components/state that the next tick consumes.

### Reading local-player state

Component access API (azalea/src/client_impl/entity_query.rs) — **`component()` returns a mapped parking_lot read guard in 0.16** (the ECS is read-locked while the guard lives; don't hold it across a frame):

```rust
pub fn component<T: Component>(&self) -> parking_lot::MappedRwLockReadGuard<'_, T>; // panics if absent
pub fn get_component<T: Component>(&self) -> Option<MappedRwLockReadGuard<'_, T>>;
pub fn query_self<D: QueryData, R>(&self, f: impl FnOnce(QueryItem<D>) -> R) -> R;  // for &mut access
pub fn entity_component<T: Component>(&self, entity: Entity) -> MappedRwLockReadGuard<'_, T>;
```

Convenience getters (azalea/src/entity_ref/shared_impls.rs — exist on both `Client` and `EntityRef`):

```rust
pub fn position(&self) -> Vec3;                 // feet pos; **component::<Position>()
pub fn eye_position(&self) -> Vec3;             // position().up(dimensions().eye_height as f64)
pub fn dimensions(&self) -> EntityDimensions;   // { width: f32, height: f32, eye_height: f32, fixed: bool }; player eye_height = 1.62
pub fn physics(&self) -> Physics;               // CLONE of the Physics component
pub fn health(&self) -> f32;
pub fn attributes(&self) -> Attributes;
pub fn uuid(&self) -> Uuid;
pub fn minecraft_id(&self) -> MinecraftEntityId;
pub fn world_name(&self) -> WorldName;
pub fn is_alive(&self) -> bool;
```

Exact component types (crate `azalea_entity`, module root unless noted):

| Data | Component | Notes |
|---|---|---|
| position (feet) | `azalea_entity::Position` | tuple newtype, `Deref<Target = Vec3>`; `Vec3 { x, y, z: f64 }` |
| look direction | `azalea_entity::LookDirection` | fields **private**; `y_rot() -> f32` (yaw), `x_rot() -> f32` (pitch); `LookDirection::new(y_rot, x_rot)`; mutate with `.update(new)` (anticheat-safe rounding) |
| velocity, on_ground, bbox | `azalea_entity::Physics` | `pub velocity: Vec3` (Y ≈ −0.0784 when grounded), `on_ground(&self) -> bool` (private field + getter), `pub bounding_box: Aabb`, `old_position`, `fall_distance: f64`, `horizontal_collision`, `is_in_water()` |
| jump key held | `azalea_entity::Jumping` | `Deref<Target = bool>` |
| input state | `azalea_physics::local_player::PhysicsState` | `move_direction: WalkDirection`, `trying_to_sprint`, `trying_to_crouch`, `move_vector: Vec2` |
| eye height | `azalea_entity::dimensions::EntityDimensions` | `eye_height: f32` |

```rust
use azalea::{Client, Vec3};
use azalea_entity::{LookDirection, Physics, Position};

fn camera_state(bot: &Client) -> (Vec3, f32, f32, Vec3, bool) {
    let eye = bot.eye_position();
    let look: LookDirection = bot.direction();
    // raw component reads (0.16: mapped RwLock read guards):
    let feet: Vec3 = **bot.component::<Position>();
    let _ = feet;
    let phys = bot.component::<Physics>(); // guard; ECS is read-locked while held
    (eye, look.y_rot(), look.x_rot(), phys.velocity, phys.on_ground())
}
```

---

## 4. Health, food, XP, game mode

```rust
// azalea::Client shortcuts
pub fn health(&self) -> f32;              // component azalea_entity::metadata::Health(pub f32), 0..=20
pub fn hunger(&self) -> Hunger;           // azalea_client::local_player::Hunger { food: u32 /*0..=20*/, saturation: f32 }
pub fn experience(&self) -> Experience;   // azalea_client::local_player::Experience { progress: f32 /*0..1*/, level: u32, total: u32 }
```

Game mode has **no shortcut method**; read the component:

```rust
use azalea::Client;
use azalea::core::game_type::GameMode;              // { Survival, Creative, Adventure, Spectator }
use azalea::local_player::LocalGameMode;            // azalea_client::local_player::LocalGameMode

fn hud_stats(bot: &Client) -> (f32, u32, f32, u32, GameMode) {
    let hp = bot.health();
    let hunger = bot.hunger();          // clones the component
    let xp = bot.experience();
    let gm = bot.component::<LocalGameMode>().current; // LocalGameMode { current: GameMode, previous: Option<GameMode> }
    (hp, hunger.food, xp.progress, xp.level, gm)
}
```

(`azalea::local_player` works because `azalea` does `pub use azalea_client::*` and azalea-client has `pub mod local_player`.) `Health`/`Hunger`/`Experience` update from server packets; these components only exist after joining (use `get_component` before `Event::Spawn`).

---

## 5. Actions: mining, interacting, attacking, hotbar, use item

### Mining (`azalea/src/client_impl/mining.rs`, `azalea/src/bot.rs`, plugin `azalea_client::mining`)

```rust
pub fn start_mining(&self, position: BlockPos);   // fire-and-forget; sends StartMiningBlockEvent { force: true }
pub async fn mine(&self, position: BlockPos);     // start_mining + awaits ticks until the Mining component is gone
pub fn is_mining(&self) -> bool;                  // Mining component present?
pub fn left_click_mine(&self, enabled: bool);     // toggle LeftClickMine marker: auto-mine whatever the crosshair hits
```

How breaking works: `StartMiningBlockEvent` → swing + `ServerboundPlayerAction(StartDestroyBlock)`; each `GameTick` `continue_mining_block` increments `MineProgress` (component, `pub f32` 0..1; `destroy_stage() -> Option<u32>` gives the vanilla 0-9 crack overlay stage — use this for the block-crack texture) and swings the arm; when progress ≥ 1 it triggers `FinishMiningBlockEvent` and sends `StopDestroyBlock`. It is NOT "left-click once" — creative insta-break is handled automatically, survival requires the per-tick progress that azalea drives for you. For a GUI client, `left_click_mine(true)` while LMB is held (and `false` on release) is the vanilla-like behavior; it uses the client's own `HitResultComponent` ray-cast. Related components on the client entity: `Mining { pos: BlockPos, dir: Direction, force: bool }`, `MineProgress(f32)`, `MineTicks(f32)`, `MineDelay(u32)`, `MineBlockPos(Option<BlockPos>)`, `MineItem(ItemStack)`. ECS message to stop early: `azalea_client::mining::StopMiningBlockEvent { entity }`.

### Interacting / using items (`azalea/src/client_impl/interact.rs`)

```rust
pub fn block_interact(&self, position: BlockPos);  // right-click block (place/lever/door...) MainHand
pub fn start_use_item(&self);                      // right-click held item (eating holds until consumed)
pub fn entity_interact(&self, entity: Entity);     // right-click entity (can go through walls)
pub fn hit_result(&self) -> HitResult;             // azalea_core::hit_result::HitResult — crosshair block/entity/miss
```

### Attack & swing (`azalea/src/client_impl/attack.rs`, `azalea_client::interact::SwingArmEvent`)

```rust
pub fn attack(&self, entity: Entity);                       // left-click entity; no aim/range checks
pub fn has_attack_cooldown(&self) -> bool;
pub fn attack_cooldown_remaining_ticks(&self) -> usize;
```

Swing arm without attacking (visual only) — `SwingArmEvent` is a bevy `EntityEvent` (observer trigger, not a Message):

```rust
use azalea::Client;
use azalea_client::interact::SwingArmEvent;

fn swing(bot: &Client) {
    bot.ecs.write().trigger(SwingArmEvent { entity: bot.entity });
}
```

### Hotbar (`azalea/src/client_impl/inventory.rs`)

```rust
pub fn selected_hotbar_slot(&self) -> u8;                    // 0..=8
pub fn set_selected_hotbar_slot(&self, new_hotbar_slot_index: u8); // panics if > 8; applies next Update
```

Held item: `bot.component::<azalea_entity::inventory::Inventory>().held_item() -> &ItemStack`.

---

## 6. Chat

Sending (`azalea/src/client_impl/chat.rs`):

```rust
pub fn chat(&self, content: impl Into<String>);   // leading '/' => sent as command packet
pub fn write_chat_packet(&self, message: &str);   // always chat packet
pub fn write_command_packet(&self, command: &str);// no leading slash
```

Receiving — handler gets `Event::Chat(ChatPacket)` (`azalea::Event`, payload `azalea_client::chat::ChatPacket`):

```rust
pub enum ChatPacket { System(Arc<ClientboundSystemChat>),
                      Player(Arc<ClientboundPlayerChat>),
                      Disguised(Arc<ClientboundDisguisedChat>) }
impl ChatPacket {
    pub fn message(&self) -> FormattedText;                        // full styled message; .to_string() = plain text, .to_ansi() = terminal colors
    pub fn split_sender_and_content(&self) -> (Option<String>, String); // regex heuristics for System messages
    pub fn sender(&self) -> Option<String>;
    pub fn sender_uuid(&self) -> Option<Uuid>;                     // Some only for Player packets
    pub fn content(&self) -> String;                               // plain text, no formatting codes
    pub fn is_whisper(&self) -> bool;
}
```

```rust
use azalea::prelude::*;

async fn handle(bot: Client, event: Event, _state: azalea::NoState) -> eyre::Result<()> {
    if let Event::Chat(m) = event {
        let plain = m.message().to_string();     // what to render in the chat HUD (unstyled)
        let (sender, content) = m.split_sender_and_content();
        println!("[{}] {}  (raw: {plain})", sender.unwrap_or_default(), content);
        if content == "ping" { bot.chat("pong"); }
    }
    Ok(())
}
```

For styled chat rendering, walk the `azalea::FormattedText` tree (azalea-chat) instead of `.to_string()`.

---

## 7. Inventory reading (hotbar HUD)

Component: `azalea_entity::inventory::Inventory` — fields `inventory_menu: azalea_inventory::Menu` (always `Menu::Player`), `container_menu: Option<Menu>`, `id: i32`, `selected_hotbar_slot: u8`; `menu(&self) -> &Menu` returns the open container or the player inventory. `Client::menu(&self) -> Menu` returns a clone.

`azalea_inventory::Menu` (macro-generated): `slots() -> Vec<ItemStack>` (all slots, protocol order), `slot(i: usize) -> Option<&ItemStack>`, `len()`, `hotbar_slots_range() -> RangeInclusive<usize>`, `player_slots_range()`, `is_hotbar_slot(i)`. Player-menu protocol indices: 0 craft_result, 1–4 craft, 5–8 armor, 9–44 main inventory (36), 45 offhand; **hotbar = slots 36..=44** (`Player::HOTBAR_SLOTS`).

`azalea_inventory::ItemStack`:

```rust
pub enum ItemStack { #[default] Empty, Present(ItemStackData) }
impl ItemStack {
    pub fn is_empty(&self) -> bool;  pub fn is_present(&self) -> bool;
    pub fn count(&self) -> i32;                       // 0 if Empty
    pub fn kind(&self) -> ItemKind;                   // azalea_registry::builtin::ItemKind; Air if Empty
    pub fn as_present(&self) -> Option<&ItemStackData>;
    pub fn get_component<'a, T: DataComponentTrait>(&'a self) -> Option<Cow<'a, T>>; // durability etc.
}
pub struct ItemStackData { pub kind: ItemKind, pub count: i32, pub component_patch: DataComponentPatch }
```

`ItemKind` is a fieldless registry enum; `kind.to_str() -> &'static str` / `Display` give `"minecraft:stone"`; `u32::from(kind)` (registry id) and `ItemKind::from_str("stone")` also exist.

```rust
use azalea::Client;
use azalea_inventory::{ItemStack, Menu};

/// (item id string, count) x9 for the hotbar HUD.
fn hotbar(bot: &Client) -> Vec<(String, i32)> {
    let menu: Menu = bot.menu();                    // player inv or open container
    let slots = menu.slots();
    slots[menu.hotbar_slots_range()]
        .iter()
        .map(|s: &ItemStack| (s.kind().to_str().to_owned(), s.count()))
        .collect()
}

fn selected(bot: &Client) -> u8 { bot.selected_hotbar_slot() }
```

For textures/HUD icons key off `ItemStack::kind()`; per-item NBT-like data lives in `component_patch` (data components, e.g. `azalea_inventory::components::Damage`).

---

## 8. Respawn on death

**Automatic by default** when using `ClientBuilder::new()` / `Client::join`: `azalea::bot::DefaultBotPlugins` includes `azalea::auto_respawn::AutoRespawnPlugin`, which converts every `DeathEvent` into a `PerformRespawnEvent`; `azalea_client::respawn::RespawnPlugin` (in `DefaultPlugins`) then sends `ServerboundClientCommand { action: Action::PerformRespawn }`. So the death screen is skipped automatically; you still get `Event::Death(Option<Arc<ClientboundPlayerCombatKill>>)` in your handler for UI.

To disable auto-respawn (so the GUI can show a death screen with a Respawn button):

```rust
use azalea::app::PluginGroup;
use azalea::prelude::*;

let builder = ClientBuilder::new_without_plugins()
    .add_plugins(azalea::DefaultPlugins)
    .add_plugins(azalea::bot::DefaultBotPlugins.build()
        .disable::<azalea::auto_respawn::AutoRespawnPlugin>());
```

Manual respawn (what the Respawn button does) — write the ECS message:

```rust
use azalea::Client;
use azalea_client::respawn::PerformRespawnEvent;

fn respawn(bot: &Client) {
    bot.ecs.write().write_message(PerformRespawnEvent { entity: bot.entity });
}
```

---

## Event enum (azalea::events::Event) — variants relevant to a client UI

`Init`, `Login`, `Spawn` (in a loaded chunk, ready), `Chat(ChatPacket)`, `Tick` (20/s, only in-world), `Packet(Arc<ClientboundGamePacket>)` (feature `packet-event`, default on), `AddPlayer/RemovePlayer/UpdatePlayer(PlayerInfo)`, `Death(Option<Arc<ClientboundPlayerCombatKill>>)`, `KeepAlive(u64)`, `Disconnect(Option<FormattedText>)`, `ConnectionFailed(Arc<ConnectionError>)`, `ReceiveChunk(ChunkPos)`. The enum is `#[non_exhaustive]`.

## Open questions

- **`Client: Send` assumption**: `Client { entity, ecs: Arc<RwLock<bevy_ecs::world::World>> }` should be `Send + Sync` (bevy `World` is), letting the render thread poll components; I did not find an explicit `unsafe impl` or a doc guarantee — verify with a `fn assert_send<T: Send>()` compile check.
- `Client::join`'s LocalSet requirement is documented on `start_ecs_runner` ("panics if called outside of a Tokio LocalSet"); the panic actually comes from `tokio::task::spawn_local`. The thread must also keep the LocalSet alive or systems stop running — the "keep `rx.recv()` loop on that thread" pattern above does this, but if you move the receiver elsewhere you must `local.block_on(&rt, std::future::pending::<()>())` or await the `appexit_rx`.
- Holding a `component::<T>()` guard while calling any method that takes `ecs.write()` (e.g. `walk`, `chat`) deadlocks (parking_lot RwLock is not reentrant). Clone data out of guards before issuing commands.
- `MicrosoftAccountOpts.check_ownership` defaults to `false` via `Default`, and `Account::microsoft` also uses `false` — Game Pass accounts work by default.
- Exact behavior of `left_click_mine` in creative (insta-break loop speed) untested; also `StartMiningBlockEvent.force: true` ignores line-of-sight, which may trip anticheats — for vanilla-like behavior mine only what `hit_result()` reports.
- FormattedText -> styled runs for the chat HUD: use the `FormattedText` tree (`Text`/`Translatable` + `Style`) directly; `to_ansi()` exists but is terminal-oriented. Translatable components need `azalea-language` for en_us resolution (not checked in detail).
- Nightly toolchain: sources compile with edition 2024 + several feature gates; pin a recent nightly (crate published ~mid-2026). Exact minimum nightly version not determined.
