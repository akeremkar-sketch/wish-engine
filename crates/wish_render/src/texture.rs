//! Textures: mip chains made on the CPU, and uploads.
//!
//! The mip chain is made on the CPU with a 2×2 box filter so that every
//! adapter draws from exactly the same data (generating mips on the GPU
//! would differ between drivers). Sampling uses trilinear filtering without
//! anisotropy, for the same reason.

use wish_import::Texture;

/// One level of a mip chain.
pub(crate) struct Level {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// The full mip chain of an RGBA8 image, level 0 first. Odd sizes round down
/// and the filter clamps at the edge.
pub(crate) fn mip_chain(texture: &Texture) -> Vec<Level> {
    let mut levels = vec![Level {
        width: texture.width.max(1),
        height: texture.height.max(1),
        rgba: texture.rgba.clone(),
    }];
    while let Some(last) = levels.last() {
        if last.width == 1 && last.height == 1 {
            break;
        }
        let (width, height) = ((last.width / 2).max(1), (last.height / 2).max(1));
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(last.width - 1);
                    let sy = (y * 2 + dy).min(last.height - 1);
                    let at = ((sy * last.width + sx) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(last.rgba.get(at + channel).copied().unwrap_or(0));
                    }
                }
                rgba.extend(sum.iter().map(|total| ((total + 2) / 4) as u8));
            }
        }
        levels.push(Level {
            width,
            height,
            rgba,
        });
    }
    levels
}

/// Uploads a texture with all its mip levels. `srgb` says whether its bytes
/// are colours (base colour, emission) or data (normals, roughness).
pub(crate) fn upload(
    gpu: &crate::gpu::Gpu,
    texture: &Texture,
    srgb: bool,
    label: &str,
) -> wgpu::TextureView {
    let levels = mip_chain(texture);
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let gpu_texture = gpu.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: texture.width.max(1),
            height: texture.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: u32::try_from(levels.len()).unwrap_or(1),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip, level) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_texture,
                mip_level: u32::try_from(mip).unwrap_or(0),
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(level.width * 4),
                rows_per_image: Some(level.height),
            },
            wgpu::Extent3d {
                width: level.width,
                height: level.height,
                depth_or_array_layers: 1,
            },
        );
    }
    gpu_texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A 1×1 texture of one colour, for materials that have no texture in a slot.
pub(crate) fn solid(
    gpu: &crate::gpu::Gpu,
    rgba: [u8; 4],
    srgb: bool,
    label: &str,
) -> wgpu::TextureView {
    upload(
        gpu,
        &Texture {
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
        },
        srgb,
        label,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(width: u32, height: u32, fill: impl Fn(u32, u32) -> [u8; 4]) -> Texture {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&fill(x, y));
            }
        }
        Texture {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn the_chain_halves_down_to_one_pixel() {
        let sizes: Vec<(u32, u32)> = mip_chain(&texture(8, 2, |_, _| [0; 4]))
            .iter()
            .map(|level| (level.width, level.height))
            .collect();
        assert_eq!(sizes, [(8, 2), (4, 1), (2, 1), (1, 1)]);
        assert_eq!(mip_chain(&texture(1, 1, |_, _| [0; 4])).len(), 1);
        // Odd sizes round down and still end at 1x1.
        let odd = mip_chain(&texture(5, 3, |_, _| [9; 4]));
        assert_eq!(odd.last().map(|l| (l.width, l.height)), Some((1, 1)));
        assert!(
            odd.iter().all(|l| l.rgba.iter().all(|v| *v == 9)),
            "a flat colour stays flat"
        );
    }

    #[test]
    fn a_level_averages_its_four_parents() {
        let chain = mip_chain(&texture(2, 2, |x, y| {
            let value = [0u8, 100, 200, 255][(y * 2 + x) as usize];
            [value, value, value, 255]
        }));
        // (0 + 100 + 200 + 255) / 4 = 138.75, rounded to nearest.
        assert_eq!(chain[1].rgba, [139, 139, 139, 255]);
    }
}
