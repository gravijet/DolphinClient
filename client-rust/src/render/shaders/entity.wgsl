// Entity shader: solid-color boxes. A shared unit cube (centered at origin,
// edge 1) is instanced via a per-draw dynamic uniform holding a full model
// matrix (already camera-relative) and a flat color.

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

struct EntityU {
    // Model matrix in camera-relative space (includes entity_pos - cam_pos).
    model: mat4x4<f32>,
    color: vec4<f32>,
};
@group(1) @binding(0) var<uniform> entity: EntityU;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_pos: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> VsOut {
    let world = (entity.model * vec4<f32>(pos, 1.0)).xyz;
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.view_pos = world;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dist = length(in.view_pos);
    let denom = max(globals.fog_end - globals.fog_start, 0.001);
    let fog = clamp((dist - globals.fog_start) / denom, 0.0, 1.0);
    let rgb = mix(entity.color.rgb, globals.sky_color, fog);
    return vec4<f32>(rgb, entity.color.a);
}
