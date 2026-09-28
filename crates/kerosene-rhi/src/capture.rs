// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Reading a finished frame back from the GPU.

/// A frame on its way out of the GPU.
pub struct Capture {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    row: u32,
    /// Whether the surface stores blue first, as most do.
    bgra: bool,
}

/// A captured frame, as opaque RGBA8 pixels, top row first.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Capture {
    /// Record a copy of `texture` (a frame of `config`'s surface) into a
    /// buffer the CPU can read. `None`, with a warning, where the surface
    /// cannot be copied from.
    pub fn copy(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Option<Capture> {
        use wgpu::TextureFormat as F;
        let bgra = match config.format {
            F::Bgra8Unorm | F::Bgra8UnormSrgb => true,
            F::Rgba8Unorm | F::Rgba8UnormSrgb => false,
            other => {
                log::warn!("screenshot: cannot read a {other:?} surface");
                return None;
            }
        };
        if !config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
            log::warn!("screenshot: this surface cannot be copied from");
            return None;
        }
        let (width, height) = (config.width, config.height);
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Some(Capture {
            buffer,
            width,
            height,
            row,
            bgra,
        })
    }

    /// Wait for the copy and return it as RGBA, opaque whatever the
    /// surface's alpha holds. The commands that fill it must have been
    /// submitted.
    pub fn read(self, device: &wgpu::Device) -> Result<Pixels, Box<dyn std::error::Error>> {
        let slice = self.buffer.slice(..);
        let (send, receive) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        device.poll(wgpu::PollType::Wait)?;
        receive.recv()??;
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        {
            let bytes = slice.get_mapped_range();
            for y in 0..self.height {
                let start = (y * self.row) as usize;
                let (pixels, _) = bytes[start..start + self.width as usize * 4].as_chunks::<4>();
                for &[a, b, c, _] in pixels {
                    rgba.extend_from_slice(&if self.bgra {
                        [c, b, a, 255]
                    } else {
                        [a, b, c, 255]
                    });
                }
            }
        }
        self.buffer.unmap();
        Ok(Pixels {
            width: self.width,
            height: self.height,
            rgba,
        })
    }
}
