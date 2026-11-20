//! `wen bench --render`: what drawing a frame costs.
//!
//! A fixed scene (a grid of cubes and spheres on a ground, a sun that casts
//! shadows, sixteen point lights) is drawn many times and each frame is
//! timed from the call to the picture read back, which is the path every
//! frame takes today. The hardware adapter is used when there is one, the
//! software adapter otherwise.
//!
//! This is the only place a frame's time is reported: a command, not a
//! tool. Tool answers carry the counts only (`stats`), which are the same on
//! every machine.

use std::time::Instant;

use serde_json::json;
use wish_core::{Diagnostic, Result};
use wish_ecs::World;
use wish_render::{Created, Gpu, GpuOptions, Renderer, extract};

/// What to measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderBench {
    /// How many surfaces the scene has (the ground is one of them).
    pub draws: u32,
    /// How many frames to time, after the first.
    pub frames: u64,
    /// The picture's width and height in pixels.
    pub size: (u32, u32),
    /// Use the software adapter even if there is hardware.
    pub software: bool,
}

/// The benchmark scene as a scene file: `draws` surfaces in all. The same
/// numbers always give the same scene.
fn scene_source(draws: u32) -> String {
    let objects = draws.saturating_sub(1);
    let side = (f64::from(objects).sqrt().ceil() as u32).max(1);
    let spacing = 1.5f32;
    let half = (side - 1) as f32 * spacing * 0.5;
    let extent = side as f32 * spacing;
    let mut entities = Vec::new();
    // Above and behind the grid, looking down at its middle.
    entities.push(format!(
        r#""camera": {{ "components": {{ "Camera": {{ "fov_y_deg": 50, "far": {} }},
            "Transform": {{ "translation": [0, {}, {}], "rotation": [-0.3162278, 0, 0, 0.9486833] }} }} }}"#,
        (extent * 6.0).max(1000.0),
        extent * 0.75 + 2.0,
        extent + 2.0,
    ));
    entities.push(format!(
        r#""ground": {{ "components": {{ "Primitive": {{ "shape": "Plane" }},
            "Material": {{ "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 }},
            "Transform": {{ "scale": [{0}, 1, {0}] }} }} }}"#,
        extent + 4.0
    ));
    entities.push(
        r#""sun": { "components": { "DirectionalLight": { "illuminance_lux": 100000 },
            "Transform": { "rotation": [-0.4545195, 0.3304543, 0.1786597, 0.8067046] } } }"#
            .to_owned(),
    );
    for index in 0..objects {
        let (column, row) = (index % side, index / side);
        let shape = if index % 2 == 0 { "Cube" } else { "Sphere" };
        // Colours and finishes that repeat with different periods, so that
        // neighbours differ.
        let colour = [
            0.25 + 0.7 * (index % 5) as f32 / 4.0,
            0.25 + 0.7 * (index % 7) as f32 / 6.0,
            0.25 + 0.7 * (index % 3) as f32 / 2.0,
        ];
        entities.push(format!(
            r#""thing_{index:06}": {{ "components": {{ "Primitive": {{ "shape": "{shape}" }},
                "Material": {{ "base_color": [{}, {}, {}, 1], "metallic": {}, "roughness": {} }},
                "Transform": {{ "translation": [{}, 0.5, {}] }} }} }}"#,
            colour[0],
            colour[1],
            colour[2],
            if index % 4 == 0 { 1 } else { 0 },
            0.2 + 0.15 * (index % 5) as f32,
            column as f32 * spacing - half,
            row as f32 * spacing - half,
        ));
    }
    // Sixteen lamps spread over the grid.
    for index in 0..16u32 {
        let (column, row) = (index % 4, index / 4);
        let place = |cell: u32| (cell as f32 + 0.5) / 4.0 * extent - extent * 0.5;
        entities.push(format!(
            r#""lamp_{index:02}": {{ "components": {{
                "PointLight": {{ "color": [1, {}, {}, 1], "intensity_lm": 20000, "range": {} }},
                "Transform": {{ "translation": [{}, 2.5, {}] }} }} }}"#,
            0.6 + 0.1 * column as f32,
            0.5 + 0.1 * row as f32,
            (extent * 0.2).max(4.0),
            place(column),
            place(row),
        ));
    }
    format!(
        "{{ \"format\": {}, \"entities\": {{\n{}\n}} }}",
        wish_scene::FORMAT,
        entities.join(",\n")
    )
}

/// The benchmark scene as a world, ready to extract.
fn build_world(draws: u32) -> Result<World> {
    let mut world = World::new();
    crate::types::register_engine_types(&mut world)?;
    let source = scene_source(draws);
    let loaded = wish_scene::load(&source, "bench/render.scene.json", world.registry());
    if let Some(problem) = loaded
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
    {
        return Err(problem.clone());
    }
    let Some(document) = loaded.document else {
        return Err(Diagnostic::error(
            "internal_error",
            "The benchmark scene did not load.",
            "This is a bug in the engine. Please report it.",
        ));
    };
    document.instantiate(&mut world)?;
    wish_transform::update_global_transforms(&mut world)?;
    Ok(world)
}

fn milliseconds(start: Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0
}

/// Runs `wen bench --render`.
pub fn bench_render(options: &RenderBench, as_json: bool) -> Result<String> {
    let world = build_world(options.draws)?;
    let scene = extract(&world)?;
    // Hardware if there is any, unless the software adapter was asked for.
    let (gpu, fell_back) = if options.software {
        (Gpu::shared(GpuOptions { software: true })?, None)
    } else {
        match Gpu::shared(GpuOptions { software: false }) {
            Ok(gpu) => (gpu, None),
            Err(problem) => (Gpu::shared(GpuOptions { software: true })?, Some(problem)),
        }
    };
    let renderer = Renderer::new(&gpu)?;
    let (width, height) = options.size;

    // The first frame makes the targets; the frames after it reuse them.
    // The device counts every texture and buffer made on it, by the
    // renderer's pool or by a pass on its own, so "none" below means none.
    let started = Instant::now();
    let first = renderer.render(&gpu, &scene, None, width, height)?;
    let first_ms = milliseconds(started);
    let after_first = gpu.created();
    let mut frame_ms: Vec<f64> = Vec::with_capacity(usize::try_from(options.frames).unwrap_or(0));
    for _ in 0..options.frames {
        let started = Instant::now();
        renderer.render(&gpu, &scene, None, width, height)?;
        frame_ms.push(milliseconds(started));
    }
    let end = gpu.created();
    let created = Created {
        textures: end.textures - after_first.textures,
        buffers: end.buffers - after_first.buffers,
    };

    let mut sorted = frame_ms.clone();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| -> f64 {
        let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
        sorted.get(index).copied().unwrap_or(0.0)
    };
    let mean = if frame_ms.is_empty() {
        0.0
    } else {
        frame_ms.iter().sum::<f64>() / frame_ms.len() as f64
    };
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    let per_second = if mean > 0.0 { 1000.0 / mean } else { 0.0 };
    let adapter = gpu.adapter();
    let stats = first.stats;
    let mut notes: Vec<Diagnostic> = first.warnings;
    if let Some(problem) = fell_back {
        notes.push(Diagnostic::warning(
            "no_gpu_adapter",
            format!(
                "No hardware adapter could be used ({}); the software adapter was measured instead.",
                problem.message
            ),
            "Numbers from the software adapter say little about a player's machine. Update the graphics driver to measure the hardware.",
        ));
    }

    if as_json {
        return Ok(json!({
            "ok": true,
            "workload": "render",
            "adapter": { "name": adapter.name, "backend": adapter.backend, "software": adapter.software },
            "frames": options.frames,
            "stats": {
                "width": stats.width,
                "height": stats.height,
                "draw_calls": stats.draw_calls,
                "triangles": stats.triangles,
                "shadow_casters": stats.shadow_casters,
                "point_lights": stats.point_lights,
                "spot_lights": stats.spot_lights,
            },
            "first_frame_ms": first_ms,
            "frame_ms": {
                "mean": round(mean),
                "median": round(percentile(0.5)),
                "p95": round(percentile(0.95)),
                "max": round(percentile(1.0)),
            },
            "frames_per_second": round(per_second),
            "created_after_first_frame": {
                "textures": created.textures,
                "buffers": created.buffers,
            },
            "diagnostics": notes,
        })
        .to_string());
    }
    let mut text = format!(
        "render - {} draws - {width}x{height} - {} frames\n\
         adapter:  {} ({}{})\n\
         frame:    mean {mean:.3} ms, median {:.3} ms, p95 {:.3} ms, max {:.3} ms ({per_second:.0} frames a second), drawn and read back\n\
         first:    {first_ms:.1} ms (makes the targets)\n\
         counts:   {} draw calls, {} triangles, {} shadow casters, {} point lights, {} spot lights\n\
         GPU objects created after the first frame: {} textures, {} buffers",
        first.drawn,
        options.frames,
        adapter.name,
        adapter.backend,
        if adapter.software { ", software" } else { "" },
        percentile(0.5),
        percentile(0.95),
        percentile(1.0),
        stats.draw_calls,
        stats.triangles,
        stats.shadow_casters,
        stats.point_lights,
        stats.spot_lights,
        created.textures,
        created.buffers,
    );
    for note in &notes {
        text.push_str(&format!("\n{note}"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_has_the_surfaces_it_was_asked_for() {
        for draws in [1, 2, 10, 101] {
            let world = build_world(draws).unwrap();
            let scene = extract(&world).unwrap();
            assert_eq!(scene.draws.len(), draws as usize, "{draws}");
            assert_eq!(scene.cameras.len(), 1);
            assert_eq!(scene.lights.len(), 1);
            assert_eq!(scene.local_lights.len(), 16);
        }
        assert_eq!(scene_source(50), scene_source(50), "the same every time");
    }
}
