// Skin shader: textured player-model parts. Each part (head, body, limbs) is
// one draw with a per-draw dynamic uniform (camera-relative model matrix +
// tint color) sampling the player's 64x64 skin texture. Overlay layers
// (hat/jacket/sleeves/pants) are transparent where unused → alpha discard.

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

@group(1) @binding(0) var skin_tex: texture_2d<f32>;
@group(1) @binding(1) var skin_samp: sampler;

struct EntityU {
    // Model matrix in camera-relative space (includes entity_pos - cam_pos).
    model: mat4x4<f32>,
    color: vec4<f32>,
};
@group(2) @binding(0) var<uniform> entity: EntityU;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    let world = (entity.model * vec4<f32>(pos, 1.0)).xyz;
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.view_pos = world;
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let tex = textureSample(skin_tex, skin_samp, in.uv);
    if tex.a < 0.5 {
        discard;
    }
    let dist = length(in.view_pos);
    let denom = max(globals.fog_end - globals.fog_start, 0.001);
    let fog = clamp((dist - globals.fog_start) / denom, 0.0, 1.0);
    let rgb = mix(tex.rgb * entity.color.rgb, globals.sky_color, fog);
    return vec4<f32>(rgb, 1.0);
}
