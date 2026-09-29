// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The wgpu layer.
//!
//! Deliberately thin: every decision about *what* to draw has already been
//! made by [`crate::mesh::WorldMesh::visible_surfaces`], so this is buffer
//! management, pipeline setup, and a draw loop.
//!
//! Materials each get their own bind group, and surfaces arrive sorted by
//! material, so the loop rebinds only when the material actually changes.

use crate::ATLAS_FORMAT;
use crate::FrameStats;
use crate::camera::Camera;
use crate::lightmap::{ATLAS_SIZE, LightmapAtlas};
use crate::lights::{ClusterMasks, LightFrame, LightsUniform, SHADOW_LAYERS, SHADOW_SIZE};
use crate::mesh::{NO_PROBE, WorldMesh, WorldVertex};
use crate::probes::ProbeChain;
use bytemuck::{Pod, Zeroable};
use kerosene_asset::Model;
use kerosene_bsp::surf;
pub use kerosene_material::{
    MAP_BINDING_BASE, MAP_COUNT, MAP_KINDS, MATERIAL_ALPHA_TEST, MATERIAL_TRANSLUCENT,
    MaterialBindings, MaterialMode, MaterialUniform,
};
use kerosene_material::{NeutralMaps, fallback_texture, load_material_maps, upload_rgba_format};
use kerosene_math::{Mat4, Pose, Vec3};
use kerosene_vfs::Vfs;
use std::collections::HashMap;
use kerosene_rhi::wgpu::util::DeviceExt;

mod decals;
mod draw;
mod frame;
mod map;
mod model;
mod pipelines;
mod probes;
mod renderer;
mod shadow;
#[cfg(test)]
mod tests;
mod uniforms;

pub use decals::*;
pub use map::*;
pub use model::*;
pub use pipelines::*;
pub use probes::*;
pub use renderer::*;
pub use uniforms::*;
