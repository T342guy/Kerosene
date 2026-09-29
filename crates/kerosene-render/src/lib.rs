// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The renderer.
//!
//! Draws a compiled map with its baked lighting. The design follows Source's
//! in the way that matters most for a BSP engine: *what* to draw is decided by
//! the map's own visibility data long before anything reaches the GPU.
//!
//! Each frame:
//!
//! 1. Find the viewer's cluster in the BSP.
//! 2. Take the PVS row for it -- the set of clusters that can possibly be seen.
//! 3. Frustum-cull the leaves in those clusters, then their surfaces.
//! 4. Draw what is left, batched by material.
//!
//! Steps 1-3 are pure logic over data structures and are tested here without a
//! GPU. Step 4 is a thin wgpu layer in [`gpu`].
//!
//! Lightmaps are packed into a single atlas ([`lightmap`]) so the world draws
//! in as many calls as it has materials, rather than one per face.
use kerosene_rhi::wgpu;

pub mod gpu;
pub mod ui;

// The CPU-side data the renderer draws lives in `kerosene-scene`; these
// re-exports keep the paths callers already use.
pub use kerosene_scene::{brdf, camera, decals, lightmap, lights, mesh, probes};

pub use camera::{Camera, Frustum, vertical_fov};
pub use lightmap::{ATLAS_SIZE, AtlasRect, LightmapAtlas};
pub use mesh::{Batch, NO_PROBE, Surface, WorldMesh, WorldVertex, probe_for};

/// The atlas's texel format.
///
/// Shared-exponent float: three 9-bit mantissas and one 5-bit exponent in 32
/// bits. Light is HDR by nature -- a lamp against a wall bakes to many times
/// what a lit floor does -- and the atlas used to squeeze that into 8-bit
/// unorm through a Reinhard curve, which meant the frame's own tone-mapper
/// then compressed it a second time and nothing downstream could tell a
/// bright surface from a very bright one. This keeps it linear, in the same
/// four bytes a texel cost before, and every GPU wgpu runs on can filter it.
pub const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgb9e5Ufloat;

/// Statistics for a frame, for the `r_speeds`-style overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub surfaces_drawn: usize,
    pub surfaces_total: usize,
    pub triangles: usize,
    pub draw_calls: usize,
    /// The cluster the viewer is in, or -1 outside the world.
    pub cluster: i16,
    /// Dynamic lights drawn, and shadow layers rendered for them.
    pub lights: usize,
    pub shadow_views: usize,
}

impl FrameStats {
    /// Fraction of the world's surfaces that were culled away.
    pub fn culled_fraction(&self) -> f32 {
        if self.surfaces_total == 0 {
            return 0.0;
        }
        1.0 - self.surfaces_drawn as f32 / self.surfaces_total as f32
    }
}
