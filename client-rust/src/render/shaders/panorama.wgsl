// Title-screen panorama: six textured quads forming a cube around the camera.
// Positions are already camera-relative (cube centered at origin, edge 2), so
// the shared rotation-only view_proj works directly. No fog, no depth write —
// drawn first, everything else covers it.

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

@group(1) @binding(0) var pano_tex: texture_2d<f32>;
@group(1) @binding(1) var pano_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(pos, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(pano_tex, pano_samp, in.uv).rgb, 1.0);
}
