// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Cubemap probes on the GPU.
use kerosene_rhi::wgpu;

use super::*;

/// A map's cubemap probes on the GPU. One per map, shared by every section's
/// frame bind group.
pub struct GpuProbes {
    pub(super) _texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    /// How many real probes there are; 0 for the black placeholder.
    pub count: usize,
}

impl GpuProbes {
    /// Upload a map's probes with their mip chain. `None` -- a map without
    /// probes -- uploads a black placeholder no vertex points at.
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cubemaps: Option<&kerosene_bsp::Cubemaps>,
    ) -> GpuProbes {
        let chain = ProbeChain::build(cubemaps);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cubemap probes"),
            size: wgpu::Extent3d {
                width: chain.face_size,
                height: chain.face_size,
                depth_or_array_layers: chain.layers,
            },
            mip_level_count: chain.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATLAS_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, texels) in chain.levels.iter().enumerate() {
            let size = chain.level_size(level);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(texels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * size),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: chain.layers,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        GpuProbes {
            _texture: texture,
            view,
            count: cubemaps.map_or(0, |c| c.probes.len().min(crate::probes::MAX_PROBES)),
        }
    }
}
