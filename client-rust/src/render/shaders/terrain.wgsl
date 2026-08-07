// Terrain shader: 3 layers share this module.
// fs_main   -> opaque + translucent
// fs_cutout -> cutout (discard alpha < 0.5)
//
// Vertex layout (28 B): pos f32x3 @0, uv f32x2 @12, color unorm8x4 @20,
// light unorm8x4 @24 ([sky 0-15, block 0-15, shade 0-255, ao 0-255]).
// Camera-relative: group(2) holds section_origin - camera_pos per draw.

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
// Vanilla's light texture: block light across, sky light down.
@group(0) @binding(1) var lightmap_tex: texture_2d<f32>;
@group(0) @binding(2) var lightmap_samp: sampler;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;

// `l` = (block, sky), each 0..1. Sampling the middle of the matching texel
// lets linear filtering blend between two levels without bleeding past the
// ends of the ramp.
fn light_color(l: vec2<f32>) -> vec3<f32> {
    let uv = (clamp(l, vec2<f32>(0.0), vec2<f32>(1.0)) * 15.0 + 0.5) / 16.0;
    return textureSample(lightmap_tex, lightmap_samp, uv).rgb;
}

struct SectionU {
    // xyz = section_origin - camera_pos, w unused.
    offset: vec4<f32>,
};
@group(2) @binding(0) var<uniform> section: SectionU;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) light: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec4<f32>,
    @location(3) view_pos: vec3<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let world = in.pos + section.offset.xyz;
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.light = in.light;
    out.view_pos = world;
    return out;
}

fn shade(in: VsOut, tex: vec4<f32>) -> vec4<f32> {
    // light.x = sky, light.y = block, both already 0..1 (level/15). Smooth
    // lighting puts fractional values here, which is why they are not levels.
    let lm = light_color(vec2<f32>(in.light.y, in.light.x));
    // .z = the face's fixed shade (vanilla darkens sides and undersides),
    // .w = ambient occlusion.
    var rgb = tex.rgb * in.color.rgb * lm * in.light.z * in.light.w;
    let dist = length(in.view_pos);
    let denom = max(globals.fog_end - globals.fog_start, 0.001);
    let fog = clamp((dist - globals.fog_start) / denom, 0.0, 1.0);
    rgb = mix(rgb, globals.sky_color, fog);
    return vec4<f32>(rgb, tex.a * in.color.a);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas_tex, atlas_samp, in.uv);
    return shade(in, tex);
}

@fragment
fn fs_cutout(in: VsOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas_tex, atlas_samp, in.uv);
    if tex.a < 0.5 {
        discard;
    }
    return shade(in, tex);
}
