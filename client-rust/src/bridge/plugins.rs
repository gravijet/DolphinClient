//! Our own bevy plugins registered on azalea's `ClientBuilder` — they run
//! inside azalea's ECS schedules and patch behavior the stock plugins get
//! wrong or don't implement (client brand, physics gaps).

// The bevy `Component` derive emits absolute `bevy_ecs::…` paths; alias
// azalea's re-export so they resolve without a direct bevy dependency.
use azalea::ecs as bevy_ecs;

use azalea::app::{App, Plugin, Update};
use azalea::buf::AzBuf;
use azalea::client_information::send_client_information;
use azalea::core::aabb::Aabb;
use azalea::core::position::{BlockPos, Vec3};
use azalea::core::tick::GameTick;
use azalea::ecs::prelude::*;
use azalea::entity::dimensions::EntityDimensions;
use azalea::entity::{
    EntityKindComponent, HasClientLoaded, LocalEntity, LookDirection, Physics, Position,
};
use azalea::packet::config::SendConfigPacketEvent;
use azalea::packet::game::SendGamePacketEvent;
use azalea::packet::login::InLoginState;
use azalea::physics::PhysicsSystems;
use azalea::physics::collision::world_collisions::get_block_collisions;
use azalea::physics::local_player::{Noclip, PhysicsState};
use azalea::protocol::packets::config::s_custom_payload::ServerboundCustomPayload;
use azalea::protocol::packets::game::{ServerboundMoveVehicle, ServerboundPaddleBoat};
use azalea::world::{World, WorldName, Worlds};

/// The brand string servers see (F3, logs, anticheat checks): our real name
/// plus the exact build version instead of azalea's default "vanilla".
pub fn brand_string() -> String {
    format!("Dolphinclient {}", env!("CARGO_PKG_VERSION"))
}

/// Replaces azalea's `BrandPlugin` (disabled in `spawn_bridge`): sends the
/// `minecraft:brand` config payload with [`brand_string`] on the login→config
/// transition, ordered before client information like vanilla.
pub struct DolphinBrandPlugin;

impl Plugin for DolphinBrandPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, send_dolphin_brand.before(send_client_information));
    }
}

fn send_dolphin_brand(mut commands: Commands, mut removed: RemovedComponents<InLoginState>) {
    for entity in removed.read() {
        let mut data = Vec::new();
        brand_string()
            .azalea_write(&mut data)
            .expect("writing to a Vec cannot fail");
        commands.trigger(SendConfigPacketEvent::new(
            entity,
            ServerboundCustomPayload { identifier: "brand".into(), data: data.into() },
        ));
    }
}

/// Physics behaviors azalea doesn't simulate, run inside its GameTick
/// schedule. Currently: pushing the player out of blocks that moved into
/// them (pistons). azalea ignores piston animations entirely and its
/// collision only clips movement deltas — a piston extending a block into
/// the player left them stuck inside geometry until the server corrected
/// the position (the reported "pushed only after the piston retracts").
pub struct DolphinPhysicsPlugin;

impl Plugin for DolphinPhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(GameTick, push_out_of_blocks.before(PhysicsSystems));
    }
}

/// If the player's box overlaps solid collision shapes (a block was pushed
/// into them — normal movement can never produce an overlap), shove them out
/// along the axis with the smallest escape distance, at most 0.51 blocks per
/// tick (vanilla piston speed). Runs before the physics step so the tick's
/// movement starts from the corrected position.
#[allow(clippy::type_complexity)]
fn push_out_of_blocks(
    mut query: Query<
        (&mut Position, &Physics, &EntityDimensions, &WorldName),
        // A spectator is inside blocks on purpose — never shove them out.
        (With<LocalEntity>, With<HasClientLoaded>, Without<Noclip>),
    >,
    worlds: Res<Worlds>,
) {
    for (mut position, _physics, dimensions, world_name) in &mut query {
        let Some(world_lock) = worlds.get(world_name) else {
            continue;
        };
        // Deflate so merely touching faces (standing on the ground) never
        // counts as an overlap.
        const SKIN: f64 = 1.0e-3;
        let aabb = dimensions.make_bounding_box(**position).deflate_all(SKIN);
        let overlapping: Vec<Aabb> = {
            let world = world_lock.read();
            get_block_collisions(&world, &aabb)
                .iter()
                .flat_map(|shape| shape.to_aabbs())
                .filter(|b| {
                    b.min.x < aabb.max.x
                        && b.max.x > aabb.min.x
                        && b.min.y < aabb.max.y
                        && b.max.y > aabb.min.y
                        && b.min.z < aabb.max.z
                        && b.max.z > aabb.min.z
                })
                .collect()
        };
        if overlapping.is_empty() {
            continue;
        }
        // Escape distance needed per direction to clear ALL overlapping boxes.
        let mut needed = [0.0f64; 6]; // +x, -x, +y, -y, +z, -z
        for b in &overlapping {
            needed[0] = needed[0].max(b.max.x - aabb.min.x);
            needed[1] = needed[1].max(aabb.max.x - b.min.x);
            needed[2] = needed[2].max(b.max.y - aabb.min.y);
            needed[3] = needed[3].max(aabb.max.y - b.min.y);
            needed[4] = needed[4].max(b.max.z - aabb.min.z);
            needed[5] = needed[5].max(aabb.max.z - b.min.z);
        }
        let (dir_idx, dist) = needed
            .iter()
            .copied()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .expect("six candidates");
        // Extra SKIN so the resolved box no longer counts as overlapping.
        let step = (dist + 2.0 * SKIN).min(0.51);
        let dir = match dir_idx {
            0 => Vec3 { x: 1.0, y: 0.0, z: 0.0 },
            1 => Vec3 { x: -1.0, y: 0.0, z: 0.0 },
            2 => Vec3 { x: 0.0, y: 1.0, z: 0.0 },
            3 => Vec3 { x: 0.0, y: -1.0, z: 0.0 },
            4 => Vec3 { x: 0.0, y: 0.0, z: 1.0 },
            _ => Vec3 { x: 0.0, y: 0.0, z: -1.0 },
        };
        let new_pos = **position + dir * step;
        **position = new_pos;
    }
}

// ---------------------------------------------------------------------------
// Vehicles (boats)
// ---------------------------------------------------------------------------

/// Marker on the local player while mounted on another entity, inserted by
/// the bridge's `SetPassengers` handler. azalea 0.16 ignores passengers
/// entirely, so riding, boat physics and the vehicle packets are all ours.
#[derive(Component, Clone, Copy)]
pub struct RidingVehicle {
    /// The vehicle's ECS entity.
    pub vehicle: Entity,
    /// Boats/rafts are controller-simulated client-side; other vehicles
    /// (horses, minecarts) only pin the player.
    pub is_boat: bool,
    /// Accumulated steering rotation (vanilla Boat.deltaRotation).
    pub delta_rotation: f32,
    /// Which seat we took (a boat carries two), so the camera sits where
    /// everyone else sees us.
    pub seat: u8,
}

/// Where a passenger sits on its vehicle, in the vehicle's own frame
/// (x right, y up, z forward), in blocks. The same numbers the renderer uses
/// for everyone else's riders, so our camera and our body agree.
pub fn seat_offset(kind: &str, height: f32, seat: u8) -> Vec3 {
    if kind.ends_with("boat") || kind.ends_with("raft") {
        // Vanilla's two boat seats: the front one ahead of centre, the back
        // one behind it.
        return Vec3 { x: 0., y: -0.05, z: if seat == 0 { 0.2 } else { -0.6 } };
    }
    if kind.contains("minecart") {
        return Vec3 { x: 0., y: 0., z: 0. };
    }
    Vec3 { x: 0., y: height as f64 * 0.75, z: -0.1 }
}

/// Client-side vehicle handling: the rider simulates the boat exactly like
/// vanilla (the server trusts the controlling passenger and only echoes the
/// position to others) and the player is pinned to the vehicle.
pub struct DolphinVehiclePlugin;

impl Plugin for DolphinVehiclePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(GameTick, drive_boat.before(PhysicsSystems));
        app.add_systems(
            GameTick,
            pin_to_vehicle
                .after(PhysicsSystems)
                .before(azalea::movement::send_position),
        );
    }
}

/// Water level at the boat: the top surface of any water fluid in the boat's
/// column, or `None` when not in water.
fn boat_water_level(world: &World, pos: Vec3) -> Option<f64> {
    let feet = BlockPos::from(pos);
    for probe in [feet.up(1), feet, feet.down(1)] {
        let fluid = world.get_fluid_state(probe).unwrap_or_default();
        if fluid.kind == azalea::block::fluid_state::FluidKind::Water {
            return Some(probe.y as f64 + fluid.height() as f64);
        }
    }
    None
}

/// True when moving the AABB by `delta` on one axis bumps into a block.
fn collides(world: &World, aabb: &Aabb) -> bool {
    !get_block_collisions(world, aabb).is_empty()
}

/// Vanilla-style boat control + float physics (Boat.controlBoat/floatBoat,
/// simplified): steering from the player's WASD move vector, forward thrust
/// 0.04/tick, water friction 0.9, buoyancy toward the water surface, per-axis
/// block collision. Sends PaddleBoat + MoveVehicle like the vanilla client.
#[allow(clippy::type_complexity)]
fn drive_boat(
    mut commands: Commands,
    mut riders: Query<
        (Entity, &mut RidingVehicle, &PhysicsState),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    mut vehicles: Query<
        (&mut Position, &mut LookDirection, &mut Physics, &EntityDimensions, &WorldName),
        Without<LocalEntity>,
    >,
    worlds: Res<Worlds>,
) {
    for (player, mut riding, physics_state) in &mut riders {
        if !riding.is_boat {
            continue;
        }
        let Ok((mut pos, mut look, mut physics, dims, world_name)) =
            vehicles.get_mut(riding.vehicle)
        else {
            continue;
        };
        let Some(world_lock) = worlds.get(world_name) else {
            continue;
        };
        let world = world_lock.read();

        // azalea's move_vector follows vanilla leftImpulse: x > 0 = left key.
        let mv = physics_state.move_vector;
        let (left, right) = (mv.x > 0.01, mv.x < -0.01);
        let (forward, backward) = (mv.y > 0.01, mv.y < -0.01);

        // controlBoat.
        let mut thrust = 0.0f32;
        if left {
            riding.delta_rotation -= 1.0;
        }
        if right {
            riding.delta_rotation += 1.0;
        }
        if right != left && !forward && !backward {
            thrust += 0.005;
        }
        if forward {
            thrust += 0.04;
        }
        if backward {
            thrust -= 0.005;
        }

        // floatBoat, simplified: friction by medium, gravity, buoyancy.
        let water_level = boat_water_level(&world, **pos);
        let on_ground = physics.on_ground();
        let inv_friction: f64 = match (water_level.is_some(), on_ground) {
            (true, _) => 0.9,
            (false, true) => 0.45,
            (false, false) => 0.9,
        };
        riding.delta_rotation *= inv_friction as f32;

        let new_yaw = look.y_rot() + riding.delta_rotation;
        *look = LookDirection::new(new_yaw, look.x_rot());

        let yaw_rad = (new_yaw as f64).to_radians();
        let mut v = physics.velocity;
        v.x += -yaw_rad.sin() * thrust as f64;
        v.z += yaw_rad.cos() * thrust as f64;
        v.y -= 0.04; // gravity
        if let Some(level) = water_level {
            let submerged = level - pos.y;
            if submerged > 0.0 {
                // Vanilla's float-up: pull toward the surface, damped.
                v.y = (v.y + submerged.min(1.0) * 0.0615) * 0.75;
            }
        }
        v.x *= inv_friction;
        v.z *= inv_friction;

        // Per-axis collision clipping against blocks (coarse but stable: an
        // axis that would intersect simply doesn't move this tick).
        let mut moved = Vec3::ZERO;
        let mut grounded = false;
        for axis in 0..3 {
            let delta = match axis {
                0 => Vec3 { x: v.x, y: 0.0, z: 0.0 },
                1 => Vec3 { x: 0.0, y: v.y, z: 0.0 },
                _ => Vec3 { x: 0.0, y: 0.0, z: v.z },
            };
            if delta == Vec3::ZERO {
                continue;
            }
            let target = **pos + moved + delta;
            let aabb = dims.make_bounding_box(target).deflate_all(1.0e-3);
            if collides(&world, &aabb) {
                match axis {
                    0 => v.x = 0.0,
                    1 => {
                        grounded = v.y < 0.0;
                        v.y = 0.0;
                    }
                    _ => v.z = 0.0,
                }
            } else {
                moved = moved + delta;
            }
        }
        drop(world);
        physics.velocity = v;
        physics.set_on_ground(grounded);
        let new_pos = **pos + moved;
        **pos = new_pos;

        // Vanilla vehicle packets: paddle animation state + the authoritative
        // vehicle position from the controlling passenger.
        let (row_left, row_right) =
            (forward || (left && !right), forward || (right && !left));
        commands.trigger(SendGamePacketEvent::new(
            player,
            ServerboundPaddleBoat { left: row_left, right: row_right },
        ));
        // The server never sends our own boat's paddle flags back to us, so set
        // them here — otherwise the oars of the boat you are actually rowing
        // are the only ones in the world that never move.
        commands.entity(riding.vehicle).insert((
            azalea::entity::metadata::PaddleLeft(row_left),
            azalea::entity::metadata::PaddleRight(row_right),
        ));
        commands.trigger(SendGamePacketEvent::new(
            player,
            ServerboundMoveVehicle { pos: new_pos, look_direction: *look },
        ));
    }
}

/// Keep the local player glued to their vehicle: azalea's own physics keeps
/// simulating walking/falling while mounted (it knows nothing about
/// passengers), which would drift the camera off the boat and spam wrong
/// positions. Runs after physics, before the position packet goes out.
#[allow(clippy::type_complexity)]
fn pin_to_vehicle(
    mut commands: Commands,
    mut riders: Query<
        (Entity, &RidingVehicle, &mut Position, &mut Physics),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    vehicles: Query<
        (&Position, &EntityDimensions, &LookDirection, &EntityKindComponent),
        Without<LocalEntity>,
    >,
) {
    for (rider, riding, mut pos, mut physics) in &mut riders {
        let Ok((vpos, vdims, vlook, vkind)) = vehicles.get(riding.vehicle) else {
            // Vehicle despawned without a SetPassengers update: dismount so
            // the player isn't frozen to a ghost.
            tracing::warn!("bridge: the vehicle we were riding is gone; dismounting");
            commands.entity(rider).remove::<RidingVehicle>();
            continue;
        };
        // The seat, turned with the vehicle — a boat's back seat has to stay
        // at the back however the boat is pointing.
        let kind = vkind.to_str();
        let kind = kind.strip_prefix("minecraft:").unwrap_or(&kind);
        let off = seat_offset(kind, vdims.height, riding.seat);
        let (sin, cos) = (-(vlook.y_rot() as f64).to_radians()).sin_cos();
        let target = Vec3 {
            x: vpos.x + off.x * cos + off.z * sin,
            y: vpos.y + off.y,
            z: vpos.z + off.z * cos - off.x * sin,
        };
        **pos = target;
        physics.velocity = Vec3::ZERO;
        physics.fall_distance = 0.0;
        physics.set_on_ground(true);
    }
}
