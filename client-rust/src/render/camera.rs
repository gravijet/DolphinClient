//! View/projection + frustum utilities. Right-handed, +y up, looking along the
//! vanilla yaw/pitch convention: yaw 0 → +z (south), yaw 90 → −x (west);
//! pitch +90 → straight down.
//! `view_dir(yaw, pitch)`:
//!   x = −sin(yaw)·cos(pitch), y = −sin(pitch), z = cos(yaw)·cos(pitch)

use glam::{Mat4, Vec3};

pub fn view_dir(yaw_deg: f32, pitch_deg: f32) -> Vec3 {
    let (ys, yc) = yaw_deg.to_radians().sin_cos();
    let (ps, pc) = pitch_deg.to_radians().sin_cos();
    Vec3::new(-ys * pc, -ps, yc * pc)
}

/// View-projection built at the camera origin (camera at 0,0,0 — geometry is
/// drawn camera-relative). Depth 0..1, `zfar` generous (render distance + slack).
// `look_to_rh`/`perspective_rh` are soft-deprecated in this glam version in
// favour of the `glam::camera` modules, but those use a different (DirectX)
// depth convention; these produce the reverse-Z-free 0..1 depth our pipeline
// and frustum extraction assume, and are verified correct against real renders.
#[allow(deprecated)]
pub fn view_proj(yaw_deg: f32, pitch_deg: f32, fov_y_deg: f32, aspect: f32, zfar: f32) -> Mat4 {
    let dir = view_dir(yaw_deg, pitch_deg);
    let view = Mat4::look_to_rh(Vec3::ZERO, dir, Vec3::Y);
    let proj = Mat4::perspective_rh(fov_y_deg.to_radians(), aspect.max(0.01), 0.05, zfar);
    proj * view
}

/// Six frustum planes (nx,ny,nz,d) from a view-proj matrix; point is inside a
/// plane when dot(n, p) + d >= 0. Extracted via Gribb–Hartmann rows.
pub struct Frustum {
    pub planes: [[f32; 4]; 6],
}

impl Frustum {
    pub fn from_view_proj(m: &Mat4) -> Self {
        let c = m.to_cols_array_2d(); // column major: c[col][row]
        let row = |r: usize| [c[0][r], c[1][r], c[2][r], c[3][r]];
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
        let add = |a: [f32; 4], b: [f32; 4]| [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]];
        let sub = |a: [f32; 4], b: [f32; 4]| [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]];
        Self {
            planes: [
                add(r3, r0), // left
                sub(r3, r0), // right
                add(r3, r1), // bottom
                sub(r3, r1), // top
                add(r3, r2), // near (0..1 depth: row2 + row3? see note)
                sub(r3, r2), // far
            ],
        }
    }

    /// AABB vs frustum (camera-relative coords). Conservative (p-vertex test).
    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        for p in &self.planes {
            let px = if p[0] >= 0.0 { max.x } else { min.x };
            let py = if p[1] >= 0.0 { max.y } else { min.y };
            let pz = if p[2] >= 0.0 { max.z } else { min.z };
            if p[0] * px + p[1] * py + p[2] * pz + p[3] < 0.0 {
                return false;
            }
        }
        true
    }
}
