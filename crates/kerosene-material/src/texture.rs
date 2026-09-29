// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Getting texels onto the GPU.
use kerosene_rhi::wgpu;

use kerosene_asset::Texture;
use kerosene_vfs::Vfs;

/// Read one compiled texture and put it on the GPU.
///
/// The texture's own flags decide how: a normal map or a roughness map holds
/// measurements rather than colour, and sampling it through an sRGB transfer
/// would bend every value in it. That intent was recorded at compile time
/// precisely so this decision did not have to be made from the filename.
pub fn load_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vfs: &Vfs,
    name: &str,
) -> Option<wgpu::Texture> {
    let bytes = vfs.read(&kerosene_asset::texture_path(name)).ok()?;
    let texture = Texture::from_bytes(&bytes).ok()?;

    let format = if texture.flags.is_color() {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };

    // The whole chain, not just level 0: Alchemy compiled the mips so the
    // sampler's trilinear and anisotropic settings have something to pick
    // from. Uploading one level made both a no-op, and every tiled floor
    // shimmer at distance.
    let levels: Vec<(u32, u32, Vec<u8>)> = (0..texture.mip_count())
        .filter_map(|level| {
            let mip = &texture.mips[level];
            texture
                .mip_as_rgba8(level)
                .map(|pixels| (mip.width, mip.height, pixels))
        })
        .collect();
    if levels.is_empty() {
        return None;
    }
    Some(upload_rgba_chain(device, queue, name, &levels, format))
}

/// Upload a full mip chain, largest first, in a chosen format.
fn upload_rgba_chain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    levels: &[(u32, u32, Vec<u8>)],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let (width, height, _) = levels[0];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, pixels)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

/// Upload RGBA8 texels as linear data.
///
/// For what is not material art: the missing-texture checkerboard, which
/// only has to be visible.
fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    upload_rgba_format(
        device,
        queue,
        label,
        width,
        height,
        pixels,
        wgpu::TextureFormat::Rgba8Unorm,
    )
}

/// Upload four-byte texels in a chosen format.
#[allow(clippy::too_many_arguments)]
pub fn upload_rgba_format(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    texture
}

/// A checkerboard for materials that will not load.
///
/// Deliberately garish: a missing texture should be obvious in a screenshot,
/// not blend in as a slightly wrong grey.
pub fn fallback_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    const SIZE: u32 = 32;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            if on {
                pixels.extend_from_slice(&[255, 0, 220, 255]);
            } else {
                pixels.extend_from_slice(&[20, 20, 20, 255]);
            }
        }
    }
    upload_rgba(device, queue, "missing material", SIZE, SIZE, &pixels)
}
