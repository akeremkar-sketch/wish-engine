//! Bloom: bright light spreads softly around itself (design note
//! `M12-look-2-bloom.md`; the shader is `bloom_pass.wgsl`).
//!
//! The exposed HDR picture is halved again and again into a chain of
//! *levels*, the levels are added back up from the coarsest, and a last
//! step writes the picture with its bloom into a second HDR target, which
//! the tone mapping pass then reads instead of the HDR picture:
//!
//! | Pass | Reads | Writes |
//! |---|---|---|
//! | first | the HDR picture, the setting | level 1 (half the picture's size) |
//! | down, once per further level | the level before | the level |
//! | up, from the coarsest | the level after | the level, blended in place by the pass's blend constant |
//! | composite | the HDR picture, level 1, the setting | the bloomed picture |
//!
//! This file is the CPU side: which levels a picture has and how much each
//! counts ([`BloomPlan`]), the frame's targets and bind groups
//! ([`BloomTargets`]), and the passes ([`draw`]). A frame without bloom has
//! none of them and runs none of the passes.
//!
//! No pass has a uniform of its own. The down- and up-sample passes bind
//! one texture and nothing else; how much of each level goes into the glow
//! is the up-sample pass's **blend constant**; the setting's three numbers
//! are one buffer, read by the first and the last pass through two layouts
//! of their own. So a frame never holds two bind groups of one layout over
//! different uniform buffers (the software adapter's rule, `pool.rs`).

use super::pipelines::Pipelines;
use super::pool::{Pool, TextureKey};
use super::{Bytes, HDR_FORMAT};
use crate::gpu::Gpu;

/// Bytes of the bloom uniform (`Bloom` in `bloom_pass.wgsl`): one vector.
pub(super) const BLOOM_UNIFORM_SIZE: u64 = 16;

/// The most levels a chain has. The largest picture there is (8192 pixels
/// high) has eleven.
const MAX_LEVELS: usize = 12;

/// The pool's names of the levels, finest first. Each level has a name of
/// its own: the pool keeps only a few sizes of one name, and a chain that
/// shared one would push its own levels out.
const LEVEL_NAMES: [&str; MAX_LEVELS] = [
    "bloom 1", "bloom 2", "bloom 3", "bloom 4", "bloom 5", "bloom 6", "bloom 7", "bloom 8",
    "bloom 9", "bloom 10", "bloom 11", "bloom 12",
];

/// A level's texels are at most one in this many of the picture's height:
/// a blur wider than a quarter of the picture is one colour for all of it
/// and would only veil the picture.
const WIDEST_REACH: u64 = 4;

/// Levels whose texels are smaller than one in this many of the picture's
/// height count for less, by the square of how much smaller. Only large
/// pictures have such levels; without this they would take weight from the
/// glow that a small picture of the same scene can show too.
const FINEST_FULL_REACH: f64 = 256.0;

/// Half the width of the threshold's soft start, as a share of the
/// threshold.
const KNEE: f32 = 0.5;

/// The `Bloom` resource with every number cleaned (`look.rs`): a strength
/// above 0 and at most 1, a spread of 0 to 1, a threshold of 0 to 64.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BloomSettings {
    pub strength: f32,
    pub spread: f32,
    pub threshold: f32,
}

/// The sizes of the levels of a `width` × `height` picture, finest first.
///
/// Each level is half the one before, rounded up, so that texel `i` of a
/// level lies over texels `2i` and `2i + 1` of the one before whatever the
/// size. There is always one level; the chain ends when a texel of the next
/// level would be more than a quarter of the picture's height
/// ([`WIDEST_REACH`]), and at [`MAX_LEVELS`].
fn level_sizes((width, height): (u32, u32)) -> Vec<(u32, u32)> {
    let mut sizes = Vec::new();
    let (mut width, mut height_now) = (width.max(1), height.max(1));
    for level in 1..=MAX_LEVELS {
        // A texel of this level is 2^level pixels of the picture.
        if level > 1 && (1u64 << level) * WIDEST_REACH > u64::from(height) {
            break;
        }
        width = width.div_ceil(2);
        height_now = height_now.div_ceil(2);
        sizes.push((width, height_now));
    }
    sizes
}

/// How much each of `count` levels of a picture `height` pixels high counts
/// in the glow, finest first; the weights add up to 1.
///
/// A level is weighted by its *reach*, the size of one of its texels as a
/// share of the picture's height, not by its number: the same setting then
/// gives the same glow in a small picture and in a large one of the same
/// scene. With a `spread` of 0.5 every level counts the same (a glow that
/// falls off with the square of the distance); at 0 each level counts half
/// as much as the next finer one (a tight glow), at 1 twice as much (a
/// haze). Levels finer than [`FINEST_FULL_REACH`] fade out.
fn level_weights(height: u32, count: usize, spread: f32) -> Vec<f64> {
    // reach^(2 spread - 1), without the factor every level shares.
    let ratio = 2f64.powf(2.0 * f64::from(spread) - 1.0);
    let height = f64::from(height.max(1));
    let mut weights = Vec::with_capacity(count);
    let (mut power, mut texel) = (1.0, 1.0);
    for _ in 0..count {
        power *= ratio;
        texel *= 2.0;
        let fade = (texel / height * FINEST_FULL_REACH).min(1.0);
        weights.push(power * fade * fade);
    }
    let sum: f64 = weights.iter().sum();
    if sum > 0.0 {
        for weight in &mut weights {
            *weight /= sum;
        }
    }
    weights
}

/// For each level but the coarsest, finest first: when the glow of the
/// coarser levels is brought up into it, the share of the result that is
/// theirs (`level = mix(level, coarser, share)`). Going up the whole chain
/// with these leaves every level counting what `weights` says.
fn mix_shares(weights: &[f64]) -> Vec<f32> {
    let mut shares = vec![0.0; weights.len().saturating_sub(1)];
    // The weight of the levels coarser than the one in hand.
    let mut coarser = weights.last().copied().unwrap_or(0.0);
    for (share, weight) in shares.iter_mut().zip(weights).rev() {
        let both = coarser + weight;
        *share = if both > 0.0 {
            (coarser / both) as f32
        } else {
            0.0
        };
        coarser = both;
    }
    shares
}

/// The bloom of one frame, worked out before any GPU call.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BloomPlan {
    /// The picture's size.
    size: (u32, u32),
    /// The sizes of the levels, finest first; never empty.
    levels: Vec<(u32, u32)>,
    /// For each level but the coarsest: the blend constant of the pass that
    /// brings the coarser levels' glow up into it.
    shares: Vec<f32>,
    /// The setting's numbers (`Bloom` in `bloom_pass.wgsl`).
    uniform: Vec<u8>,
}

impl BloomPlan {
    /// The bloom of a picture of `size` pixels with these settings.
    pub(super) fn new(settings: BloomSettings, size: (u32, u32)) -> Self {
        let levels = level_sizes(size);
        let shares = mix_shares(&level_weights(size.1, levels.len(), settings.spread));
        let mut uniform = Bytes::default();
        uniform.floats(&[
            settings.strength,
            settings.threshold,
            settings.threshold * KNEE,
            0.0,
        ]);
        uniform.pad_to(BLOOM_UNIFORM_SIZE);
        Self {
            size,
            levels,
            shares,
            uniform: uniform.0,
        }
    }

    /// How many passes the bloom is, each one triangle over its target: one
    /// down and one up for every level (the last step up is the composite).
    pub(super) fn passes(&self) -> u32 {
        u32::try_from(self.levels.len() * 2).unwrap_or(u32::MAX)
    }
}

/// A picture as the texture at `binding` of a bind group.
fn texture(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

/// The setting's buffer as binding 2 of a bind group, at its struct's size
/// whatever the pooled buffer's capacity.
fn numbers(setting: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding: 2,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: setting,
            offset: 0,
            size: wgpu::BufferSize::new(BLOOM_UNIFORM_SIZE),
        }),
    }
}

/// The GPU side of one frame's bloom: its targets from the pool and the
/// frame's own bind groups over them.
pub(super) struct BloomTargets {
    /// The levels, finest first.
    levels: Vec<wgpu::TextureView>,
    /// The HDR picture and the setting, for the first pass.
    first_group: wgpu::BindGroup,
    /// Each level as the one texture a down- or up-sample pass reads.
    level_groups: Vec<wgpu::BindGroup>,
    /// Level 1, the HDR picture and the setting, for the composite.
    composite_group: wgpu::BindGroup,
    /// Written by the composite: the picture with its bloom, which tone
    /// mapping reads in place of the HDR picture.
    pub bloomed: wgpu::TextureView,
}

impl BloomTargets {
    /// Takes the targets from the pool, fills the setting's buffer and
    /// builds the bind groups over `hdr`, the frame's HDR picture. Call
    /// inside `Gpu::checked`.
    pub(super) fn acquire(
        gpu: &Gpu,
        pool: &mut Pool,
        pipelines: &Pipelines,
        plan: &BloomPlan,
        hdr: &wgpu::TextureView,
    ) -> Self {
        let device = &gpu.device;
        let target = |pool: &mut Pool, name: &'static str, (width, height): (u32, u32)| {
            pool.texture(
                gpu,
                TextureKey {
                    name,
                    width,
                    height,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                },
            )
        };
        let levels: Vec<wgpu::TextureView> = plan
            .levels
            .iter()
            .zip(LEVEL_NAMES)
            .map(|(size, name)| target(pool, name, *size))
            .collect();
        let bloomed = target(pool, "bloomed", plan.size);
        let setting = pool.buffer(gpu, "bloom", wgpu::BufferUsages::UNIFORM, &plan.uniform);
        let first_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom first"),
            layout: &pipelines.bloom_first_layout,
            entries: &[texture(0, hdr), numbers(&setting)],
        });
        let level_groups = levels
            .iter()
            .map(|level| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bloom level"),
                    layout: &pipelines.hdr_layout,
                    entries: &[texture(0, level)],
                })
            })
            .collect();
        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom composite"),
            layout: &pipelines.bloom_composite_layout,
            entries: &[
                // The plan always has a first level.
                texture(0, levels.first().unwrap_or(hdr)),
                texture(1, hdr),
                numbers(&setting),
            ],
        });
        Self {
            levels,
            first_group,
            level_groups,
            composite_group,
            bloomed,
        }
    }
}

/// One pass of the bloom: a triangle over all of `target`. Without a blend
/// constant the target is replaced, whatever the pooled texture held; with
/// one, what is drawn is blended into what the target holds.
fn fill(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    blend: Option<f32>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: match blend {
                    Some(_) => wgpu::LoadOp::Load,
                    None => wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    if let Some(share) = blend {
        let share = f64::from(share);
        pass.set_blend_constant(wgpu::Color {
            r: share,
            g: share,
            b: share,
            a: share,
        });
    }
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

/// The bloom passes: from the HDR picture (which `targets` was built over)
/// down the chain, back up it, and into the bloomed picture.
pub(super) fn draw(
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &Pipelines,
    plan: &BloomPlan,
    targets: &BloomTargets,
) {
    // Down: the first level from the HDR picture, each next one from the
    // level before.
    let mut read = &targets.first_group;
    let mut pipeline = &pipelines.bloom_first;
    for (level, group) in targets.levels.iter().zip(&targets.level_groups) {
        fill(encoder, "bloom down", level, pipeline, read, None);
        read = group;
        pipeline = &pipelines.bloom_down;
    }
    // Up: from the coarsest, each level takes in the glow of the coarser
    // ones, blended by its share.
    let steps = targets
        .levels
        .iter()
        .zip(targets.level_groups.iter().skip(1))
        .zip(&plan.shares);
    for ((level, coarser), share) in steps.rev() {
        fill(
            encoder,
            "bloom up",
            level,
            &pipelines.bloom_up,
            coarser,
            Some(*share),
        );
    }
    fill(
        encoder,
        "bloom composite",
        &targets.bloomed,
        &pipelines.bloom_composite,
        &targets.composite_group,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_halve_rounding_up_and_end_at_a_quarter_of_the_height() {
        // The smallest pictures have one level, never none and never a
        // level without pixels.
        assert_eq!(level_sizes((1, 1)), [(1, 1)]);
        assert_eq!(level_sizes((1, 48)), [(1, 24), (1, 12), (1, 6)]);
        assert_eq!(level_sizes((48, 1)), [(24, 1)]);
        assert_eq!(level_sizes((7, 7)), [(4, 4)]);
        assert_eq!(level_sizes((5, 8)), [(3, 4)]);
        assert_eq!(level_sizes((33, 17)), [(17, 9), (9, 5)]);
        // Odd sizes round up at every level.
        assert_eq!(
            level_sizes((321, 201)),
            [(161, 101), (81, 51), (41, 26), (21, 13), (11, 7)]
        );
        // 1280x720: seven levels, the coarsest with texels of 128 pixels.
        let hd = level_sizes((1280, 720));
        assert_eq!(hd.len(), 7);
        assert_eq!((hd[0], hd[6]), ((640, 360), (10, 6)));
        assert_eq!(level_sizes((3840, 2160)).len(), 9);
        // The largest picture there is fits the names.
        let largest = level_sizes((8192, 8192));
        assert_eq!(largest.len(), 11);
        assert!(largest.len() <= LEVEL_NAMES.len());
        // Every texel of a level lies under one of the next.
        for size in [(1, 1), (3, 5), (321, 201), (1280, 720), (8191, 4097)] {
            let mut finer = size;
            for coarser in level_sizes(size) {
                assert!(coarser.0 >= 1 && coarser.1 >= 1);
                assert_eq!(coarser, (finer.0.div_ceil(2), finer.1.div_ceil(2)));
                finer = coarser;
            }
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_spread_says_which_levels_count() {
        // 288 pixels high: the finest level's texels are 1/144 of the
        // height, so no level fades.
        let even = level_weights(288, 6, 0.5);
        assert!(even.iter().all(|weight| close(*weight, 1.0 / 6.0)));
        let tight = level_weights(288, 6, 0.0);
        let wide = level_weights(288, 6, 1.0);
        for weights in [&even, &tight, &wide] {
            assert!(close(weights.iter().sum::<f64>(), 1.0));
        }
        for pair in tight.windows(2) {
            assert!(close(pair[1], pair[0] * 0.5), "{tight:?}");
        }
        for pair in wide.windows(2) {
            assert!(close(pair[1], pair[0] * 2.0), "{wide:?}");
        }
        // One level is all of the glow, whatever the spread.
        assert_eq!(level_weights(1, 1, 0.3), [1.0]);
    }

    #[test]
    fn a_larger_picture_keeps_the_wide_glow_and_adds_a_finer_one() {
        // The same scene at 540 and at 2160 pixels high. The larger picture
        // has two finer levels; the levels both have reach as far and
        // count nearly the same.
        let small = level_weights(540, level_sizes((960, 540)).len(), 0.5);
        let large = level_weights(2160, level_sizes((3840, 2160)).len(), 0.5);
        assert_eq!((small.len(), large.len()), (7, 9));
        // Level k of the small picture has the reach of level k + 2 of the
        // large one.
        for (level, weight) in small.iter().enumerate() {
            let same_reach = large[level + 2];
            assert!(
                (same_reach / weight - 1.0).abs() < 0.05,
                "level {level}: {weight} and {same_reach}"
            );
        }
        // The two finest levels of the large picture, which the small one
        // cannot show, count for little.
        assert!(large[0] + large[1] < 0.05, "{large:?}");
        // Without the fade the fine levels would count like any other.
        assert!(large[0] < large[3] * 0.1);
        // It holds for a tight glow too, which lives in the fine levels:
        // each of them counts for less than the level of full weight.
        let tight = level_weights(2160, 9, 0.0);
        assert!(tight[0] < tight[1] && tight[1] < tight[2], "{tight:?}");
        assert!(tight[2] > tight[3] && tight[3] > tight[4], "{tight:?}");
    }

    #[test]
    fn going_up_the_chain_gives_every_level_its_weight() {
        for (height, spread) in [(720, 0.5), (720, 0.0), (2160, 1.0), (97, 0.3), (1, 0.5)] {
            let count = level_sizes((height, height)).len();
            let weights = level_weights(height, count, spread);
            let shares = mix_shares(&weights);
            assert_eq!(shares.len(), count - 1);
            assert!(shares.iter().all(|share| (0.0..=1.0).contains(share)));
            // What each level counts once the chain has been gone up: the
            // coarsest starts as all of the glow, and every step up scales
            // what is there by the step's share and gives the level the
            // rest.
            let mut counted = vec![0.0; count];
            counted[count - 1] = 1.0;
            for level in (0..count - 1).rev() {
                let share = f64::from(shares[level]);
                for weight in &mut counted[level + 1..] {
                    *weight *= share;
                }
                counted[level] = 1.0 - share;
            }
            for (counted, weight) in counted.iter().zip(&weights) {
                assert!(
                    (counted - weight).abs() < 1e-6,
                    "{height}, {spread}: {counted} and {weight}"
                );
            }
        }
        assert!(mix_shares(&[1.0]).is_empty());
        assert!(mix_shares(&[]).is_empty());
    }

    #[test]
    fn a_plan_is_its_levels_and_the_settings_numbers() {
        let settings = BloomSettings {
            strength: 0.25,
            spread: 0.5,
            threshold: 2.0,
        };
        let plan = BloomPlan::new(settings, (1280, 720));
        assert_eq!(plan.levels.len(), 7);
        assert_eq!(plan.shares.len(), 6);
        assert_eq!(plan.passes(), 14);
        let numbers: Vec<f32> = plan
            .uniform
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        assert_eq!(numbers, [0.25, 2.0, 1.0, 0.0]);
        assert_eq!(plan.uniform.len() as u64, BLOOM_UNIFORM_SIZE);
        // A picture of one pixel: one level, two passes, nothing to mix.
        let tiny = BloomPlan::new(settings, (1, 1));
        assert_eq!(
            (tiny.levels.len(), tiny.shares.len(), tiny.passes()),
            (1, 0, 2)
        );
    }
}
