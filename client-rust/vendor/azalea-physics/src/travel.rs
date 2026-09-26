use azalea_block::{BlockState, BlockTrait, fluid_state::FluidState};
use azalea_core::{
    aabb::Aabb,
    position::{BlockPos, Vec3},
};
use azalea_entity::{
    ActiveEffects, Attributes, HasClientLoaded, Jumping, LocalEntity, LookDirection, OnClimbable,
    Physics, PlayerAbilities, Pose, Position,
    metadata::{FallFlying, Sprinting},
    move_relative,
};
use azalea_registry::builtin::MobEffect;
use azalea_world::{World, WorldName, Worlds};
use bevy_ecs::prelude::*;

use crate::{
    collision::{
        MoveCtx, MoverType, Shapes,
        entity_collisions::{AabbQuery, CollidableEntityQuery, get_entity_collisions},
        move_colliding,
        world_collisions::{get_block_and_liquid_collisions, get_block_collisions},
    },
    get_block_pos_below_that_affects_movement, handle_relative_friction_and_calculate_movement,
    local_player::{Noclip, PhysicsState},
};

/// Move the entity with the given acceleration while handling friction,
/// gravity, collisions, and some other stuff.
#[allow(clippy::type_complexity)]
pub fn travel(
    mut query: Query<
        (
            Entity,
            &Attributes,
            &WorldName,
            &OnClimbable,
            &Jumping,
            Option<&PhysicsState>,
            Option<&Sprinting>,
            Option<&Pose>,
            Option<&PlayerAbilities>,
            Option<&FallFlying>,
            Option<&Noclip>,
            Option<&ActiveEffects>,
            &mut Physics,
            &mut LookDirection,
            &mut Position,
        ),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    worlds: Res<Worlds>,
    aabb_query: AabbQuery,
    collidable_entity_query: CollidableEntityQuery,
) {
    for (
        entity,
        attributes,
        world_name,
        on_climbable,
        jumping,
        physics_state,
        sprinting,
        pose,
        abilities,
        fall_flying,
        noclip,
        active_effects,
        mut physics,
        direction,
        position,
    ) in &mut query
    {
        let Some(world_lock) = worlds.get(world_name) else {
            continue;
        };
        let world = world_lock.read();

        let sprinting = *sprinting.unwrap_or(&Sprinting(false));
        let has_slow_falling = active_effects
            .is_some_and(|e| e.get_level(MobEffect::SlowFalling).is_some());

        let mut ctx = MoveCtx {
            mover_type: MoverType::Own,
            world: &world,
            position,
            physics: &mut physics,
            source_entity: entity,
            aabb_query: &aabb_query,
            collidable_entity_query: &collidable_entity_query,
            physics_state,
            attributes,
            abilities,
            direction: *direction,
            sprinting,
            on_climbable: *on_climbable,
            pose: pose.copied(),
            jumping: *jumping,
            fall_flying: fall_flying.is_some_and(|f| **f),
            noclip: noclip.is_some(),
            has_slow_falling,
        };

        // DolphinClient patch: creative/spectator flight. Vanilla's
        // `Player.travel` runs the ordinary air movement and then throws the
        // gravity away again, keeping six tenths of the vertical speed it
        // started the tick with — which is what makes flying feel like flying
        // and not like a very slow fall. Water does not change that.
        if ctx.abilities.is_some_and(|a| a.flying) {
            travel_flying(&mut ctx);
        } else if ctx.physics.is_in_water() || ctx.physics.is_in_lava() {
            // minecraft also checks for `this.isAffectedByFluids() &&
            // !this.canStandOnFluid(fluidAtBlock)` here but it doesn't matter
            // for players
            travel_in_fluid(&mut ctx);
        } else if ctx.fall_flying {
            // DolphinClient patch: elytra. Vanilla checks the fluids first and
            // only then the fall-flying flag, so an elytra that hits water
            // swims instead of gliding.
            travel_fall_flying(&mut ctx);
        } else {
            travel_in_air(&mut ctx);
        }
    }
}

/// DolphinClient patch: flying under the creative/spectator ability.
///
/// Vanilla accelerates horizontally at the ability's own flying speed (doubled
/// while sprinting) with no ground friction at all, moves, then damps: 0.91 of
/// the horizontal speed and 0.6 of the vertical speed the tick began with.
fn travel_flying(ctx: &mut MoveCtx) {
    let y_before = ctx.physics.velocity.y;
    let speed = ctx.abilities.map(|a| a.flying_speed).unwrap_or(0.05)
        * if *ctx.sprinting { 2. } else { 1. };

    move_relative(
        ctx.physics,
        ctx.direction,
        speed,
        Vec3::new(
            ctx.physics.x_acceleration as f64,
            ctx.physics.y_acceleration as f64,
            ctx.physics.z_acceleration as f64,
        ),
    );
    // A spectator moves straight through the world; everyone else collides.
    if ctx.noclip {
        let velocity = ctx.physics.velocity;
        **ctx.position = **ctx.position + velocity;
        ctx.physics.set_on_ground(false);
    } else {
        move_colliding(ctx, ctx.physics.velocity);
    }

    ctx.physics.velocity = Vec3 {
        x: ctx.physics.velocity.x * 0.91,
        y: y_before * 0.6,
        z: ctx.physics.velocity.z * 0.91,
    };
}

/// DolphinClient patch: gliding on an elytra
/// (`LivingEntity.travelFallFlying` / `updateFallFlyingMovement`).
///
/// The whole feel of an elytra is in these five terms: gravity is scaled down
/// by how level you are looking, a dive converts fall speed into forward
/// speed, pulling the nose up trades speed for height, the velocity is
/// steered toward where you look, and everything is damped a little every
/// tick. There is no player input at all — you fly by looking.
fn travel_fall_flying(ctx: &mut MoveCtx) {
    let look = look_vector(ctx.direction);
    let x_rot = ctx.direction.x_rot() as f64 * (std::f64::consts::PI / 180.0);
    // Horizontal length of the look vector: zero when staring straight up or
    // down, which is why vanilla guards every steering term with it.
    let look_h = (look.x * look.x + look.z * look.z).sqrt();
    let speed_h = (ctx.physics.velocity.x * ctx.physics.velocity.x
        + ctx.physics.velocity.z * ctx.physics.velocity.z)
        .sqrt();
    let gravity = get_effective_gravity(ctx.physics.velocity.y, ctx.has_slow_falling);
    let level = {
        let c = x_rot.cos();
        c * c
    };

    let mut v = ctx.physics.velocity;
    // Looking level cancels three quarters of gravity; looking straight up or
    // down gives none of that back.
    v.y += gravity * (-1.0 + level * 0.75);
    // Diving: falling turns into forward speed along the look direction.
    if v.y < 0.0 && look_h > 0.0 {
        let d = v.y * -0.1 * level;
        v.x += look.x * d / look_h;
        v.y += d;
        v.z += look.z * d / look_h;
    }
    // Pulling up: forward speed turns into height (and costs it).
    if x_rot < 0.0 && look_h > 0.0 {
        let d = speed_h * (-x_rot.sin()) * 0.04;
        v.x -= look.x * d / look_h;
        v.y += d * 3.2;
        v.z -= look.z * d / look_h;
    }
    // Steer the existing speed toward where the head is pointing.
    if look_h > 0.0 {
        v.x += (look.x / look_h * speed_h - v.x) * 0.1;
        v.z += (look.z / look_h * speed_h - v.z) * 0.1;
    }
    ctx.physics.velocity = Vec3 { x: v.x * 0.99, y: v.y * 0.98, z: v.z * 0.99 };
    let movement = ctx.physics.velocity;
    move_colliding(ctx, movement);
}

/// The unit vector the given look direction points along (vanilla
/// `Entity.getLookAngle`).
fn look_vector(direction: LookDirection) -> Vec3 {
    let yaw = direction.y_rot() as f64 * (std::f64::consts::PI / 180.0);
    let pitch = direction.x_rot() as f64 * (std::f64::consts::PI / 180.0);
    let cos_pitch = pitch.cos();
    Vec3 { x: -yaw.sin() * cos_pitch, y: -pitch.sin(), z: yaw.cos() * cos_pitch }
}

/// The usual movement when we're not in water or using an elytra.
fn travel_in_air(ctx: &mut MoveCtx) {
    let gravity = get_effective_gravity(ctx.physics.velocity.y, ctx.has_slow_falling);

    let block_pos_below = get_block_pos_below_that_affects_movement(*ctx.position);

    let block_state_below = ctx
        .world
        .chunks
        .get_block_state(block_pos_below)
        .unwrap_or(BlockState::AIR);
    let block_below: Box<dyn BlockTrait> = block_state_below.into();
    let block_friction = block_below.behavior().friction;

    let inertia = if ctx.physics.on_ground() {
        block_friction * 0.91
    } else {
        0.91
    };

    // this applies the current delta
    let mut movement = handle_relative_friction_and_calculate_movement(ctx, block_friction);

    movement.y -= gravity;

    // if (this.shouldDiscardFriction()) {
    //     this.setDeltaMovement(movement.x, yMovement, movement.z);
    // } else {
    //     this.setDeltaMovement(movement.x * (double)inertia, yMovement *
    // 0.9800000190734863D, movement.z * (double)inertia); }

    // if should_discard_friction(self) {
    if false {
        ctx.physics.velocity = movement;
    } else {
        ctx.physics.velocity = Vec3 {
            x: movement.x * inertia as f64,
            y: movement.y * 0.9800000190734863f64,
            z: movement.z * inertia as f64,
        };
    }
}

fn travel_in_fluid(ctx: &mut MoveCtx) {
    let moving_down = ctx.physics.velocity.y <= 0.;
    let y = ctx.position.y;
    let gravity = get_effective_gravity(ctx.physics.velocity.y, ctx.has_slow_falling);

    let acceleration = Vec3::new(
        ctx.physics.x_acceleration as f64,
        ctx.physics.y_acceleration as f64,
        ctx.physics.z_acceleration as f64,
    );

    if ctx.physics.was_touching_water {
        let mut water_movement_speed = if *ctx.sprinting { 0.9 } else { 0.8 };
        let mut speed = 0.02;
        let mut water_efficiency_modifier =
            ctx.attributes.water_movement_efficiency.calculate() as f32;
        if !ctx.physics.on_ground() {
            water_efficiency_modifier *= 0.5;
        }

        if water_efficiency_modifier > 0. {
            water_movement_speed += (0.54600006 - water_movement_speed) * water_efficiency_modifier;
            speed += (ctx.attributes.movement_speed.calculate() as f32 - speed)
                * water_efficiency_modifier;
        }

        // if (this.hasEffect(MobEffects.DOLPHINS_GRACE)) {
        //     waterMovementSpeed = 0.96F;
        // }

        move_relative(ctx.physics, ctx.direction, speed, acceleration);
        move_colliding(ctx, ctx.physics.velocity);

        let mut new_velocity = ctx.physics.velocity;
        if ctx.physics.horizontal_collision && *ctx.on_climbable {
            // underwater ladders
            new_velocity.y = 0.2;
        }
        new_velocity.x *= water_movement_speed as f64;
        new_velocity.y *= 0.8;
        new_velocity.z *= water_movement_speed as f64;
        ctx.physics.velocity =
            get_fluid_falling_adjusted_movement(gravity, moving_down, new_velocity, ctx.sprinting);
    } else {
        move_relative(ctx.physics, ctx.direction, 0.02, acceleration);
        move_colliding(ctx, ctx.physics.velocity);

        if ctx.physics.lava_fluid_height <= fluid_jump_threshold() {
            ctx.physics.velocity.x *= 0.5;
            ctx.physics.velocity.y *= 0.8;
            ctx.physics.velocity.z *= 0.5;
            let new_velocity = get_fluid_falling_adjusted_movement(
                gravity,
                moving_down,
                ctx.physics.velocity,
                ctx.sprinting,
            );
            ctx.physics.velocity = new_velocity;
        } else {
            ctx.physics.velocity *= 0.5;
        }

        if gravity != 0.0 {
            ctx.physics.velocity.y -= gravity / 4.0;
        }
    }

    let velocity = ctx.physics.velocity;
    if ctx.physics.horizontal_collision
        && is_free(
            ctx.world,
            ctx.source_entity,
            ctx.aabb_query,
            ctx.collidable_entity_query,
            ctx.physics,
            ctx.physics.bounding_box,
            velocity.up(0.6).down(ctx.position.y).up(y),
        )
    {
        ctx.physics.velocity.y = 0.3;
    }
}

fn get_fluid_falling_adjusted_movement(
    gravity: f64,
    moving_down: bool,
    new_velocity: Vec3,
    sprinting: Sprinting,
) -> Vec3 {
    if gravity != 0. && !*sprinting {
        let new_y_velocity = if moving_down
            && (new_velocity.y - 0.005).abs() >= 0.003
            && f64::abs(new_velocity.y - gravity / 16.0) < 0.003
        {
            -0.003
        } else {
            new_velocity.y - gravity / 16.0
        };

        Vec3 {
            x: new_velocity.x,
            y: new_y_velocity,
            z: new_velocity.z,
        }
    } else {
        new_velocity
    }
}

fn is_free(
    world: &World,
    source_entity: Entity,
    aabb_query: &AabbQuery,
    collidable_entity_query: &CollidableEntityQuery,
    entity_physics: &Physics,
    bounding_box: Aabb,
    delta: Vec3,
) -> bool {
    let bounding_box = bounding_box.move_relative(delta);

    no_collision(
        world,
        Some(source_entity),
        aabb_query,
        collidable_entity_query,
        entity_physics,
        &bounding_box,
        false,
    ) && !contains_any_liquid(world, bounding_box)
}

pub fn no_collision(
    world: &World,
    source_entity: Option<Entity>,
    aabb_query: &AabbQuery,
    collidable_entity_query: &CollidableEntityQuery,
    entity_physics: &Physics,
    aabb: &Aabb,
    include_liquid_collisions: bool,
) -> bool {
    let collisions = if include_liquid_collisions {
        get_block_and_liquid_collisions(world, aabb)
    } else {
        get_block_collisions(world, aabb)
    };

    for collision in collisions {
        if !collision.is_empty() {
            return false;
        }
    }

    if !get_entity_collisions(
        world,
        aabb,
        source_entity,
        aabb_query,
        collidable_entity_query,
    )
    .is_empty()
    {
        false
    } else if source_entity.is_none() {
        true
    } else {
        let collision = border_collision(entity_physics, aabb);
        if let Some(collision) = collision {
            // !Shapes.joinIsNotEmpty(collision, Shapes.create(aabb), BooleanOp.AND);
            !Shapes::matches_anywhere(&collision.into(), &aabb.into(), |a, b| a && b)
        } else {
            true
        }
    }
}

fn border_collision(_entity_physics: &Physics, _aabb: &Aabb) -> Option<Aabb> {
    // TODO: implement world border, see CollisionGetter.borderCollision

    None
}

fn contains_any_liquid(world: &World, bounding_box: Aabb) -> bool {
    let min = bounding_box.min.to_block_pos_floor();
    let max = bounding_box.max.to_block_pos_ceil();

    for x in min.x..max.x {
        for y in min.y..max.y {
            for z in min.z..max.z {
                let block_state = world
                    .chunks
                    .get_block_state(BlockPos::new(x, y, z))
                    .unwrap_or_default();
                if !FluidState::from(block_state).is_empty() {
                    return true;
                }
            }
        }
    }

    false
}

/// `LivingEntity.getEffectiveGravity`: falling (non-positive vertical
/// velocity) under Slow Falling caps gravity at 0.01, regardless of the
/// entity's normal gravity.
fn get_effective_gravity(velocity_y: f64, has_slow_falling: bool) -> f64 {
    let is_falling = velocity_y <= 0.0;
    if is_falling && has_slow_falling {
        return BASE_GRAVITY.min(0.01);
    }
    BASE_GRAVITY
}

const BASE_GRAVITY: f64 = 0.08;

pub fn fluid_jump_threshold() -> f64 {
    // this is 0.0 for entities with an eye height lower than 0.4, but that's not
    // implemented since it's usually not relevant for players (unless the player
    // was shrunk)
    0.4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_falling_caps_gravity_only_while_falling() {
        assert_eq!(get_effective_gravity(-0.1, true), 0.01);
        assert_eq!(get_effective_gravity(0.0, true), 0.01);
    }

    #[test]
    fn slow_falling_does_not_affect_rising_entities() {
        assert_eq!(get_effective_gravity(0.5, true), BASE_GRAVITY);
    }

    #[test]
    fn without_slow_falling_gravity_is_unaffected_either_way() {
        assert_eq!(get_effective_gravity(-0.1, false), BASE_GRAVITY);
        assert_eq!(get_effective_gravity(0.5, false), BASE_GRAVITY);
    }
}
