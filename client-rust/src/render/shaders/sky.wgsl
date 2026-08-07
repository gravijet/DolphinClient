// Sky shader: celestial bodies (sun, moon) and stars, drawn camera-relative at
// a large distance right after the sky-color clear, before the terrain. Shares
// the skin bind-group layout (globals / texture / per-draw model+color) so it
// reuses the entity uniform and texture bind groups. No fog, no depth — the
// terrain draws over it. Alpha-blended so soft edges and dim stars read right.

struct Globals {
    view_proj: mat4x4<f32>,
    fog_start: f32,
    fog_end: f32,
    daylight: f32,
    mode: f32,
    sky_color: vec3<f32>,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;

@group(1) @binding(0) var sky_tex: texture_2d<f32>;
@group(1) @binding(1) var sky_samp: sampler;

struct EntityU {
    model: mat4x4<f32>,
    color: vec4<f32>,
    // Shared slot layout with the entity pipelines; the sky is its own light.
    light: vec4<f32>,
};
@group(2) @binding(0) var<uniform> entity: EntityU;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    let world = (entity.model * vec4<f32>(pos, 1.0)).xyz;
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let tex = textureSample(sky_tex, sky_samp, in.uv);
    return vec4<f32>(tex.rgb * entity.color.rgb, tex.a * entity.color.a);
}
