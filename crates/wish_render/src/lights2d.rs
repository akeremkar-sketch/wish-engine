//! 2D lights and vision (design note `docs/design/1.1-lights-2d.md` §8):
//! what extraction reads from the world (lights and visions with the area
//! each reaches, worked out on the CPU by `wish_sight`), and the triangles
//! the overlay pass draws from them.
//!
//! Every number is cleaned here before it reaches a shader (the look rule of
//! M12): not finite falls back to no effect, ranges are clamped, and a
//! radius or a soft edge sent to the GPU is at least 1 mm (the shaders
//! divide by them).

use wish_core::math::Vec3;
use wish_core::{Color, Diagnostic, Entity};
use wish_ecs::World;
use wish_types::lights2d::{Light2D, Lighting2D, Vision2D};
use wish_types::transform::GlobalTransform;

/// The smallest radius or soft edge sent to a shader, in meters.
const SMALLEST: f32 = 0.001;
/// The brightest a light's colour times its intensity, or the ambient, may
/// be (per channel).
const BRIGHTEST: f32 = 10_000.0;

/// A 2D light where it stands, with the area it reaches.
#[derive(Clone, Debug, PartialEq)]
pub struct Light2DView {
    /// The entity.
    pub entity: Entity,
    /// Its world position.
    pub position: Vec3,
    /// Its colour times its intensity (linear RGB, cleaned).
    pub color: [f32; 3],
    /// How far it reaches (cleaned).
    pub radius: f32,
    /// The outline of what it reaches (world X and Y, counter-clockwise):
    /// its circle, cut by walls when it casts shadows. Empty while 2D
    /// lighting is off.
    pub area: Vec<[f32; 2]>,
}

/// A character's vision where it stands, with what it sees.
#[derive(Clone, Debug, PartialEq)]
pub struct VisionView {
    /// The entity.
    pub entity: Entity,
    /// Its world position.
    pub position: Vec3,
    /// How far it sees (cleaned).
    pub radius: f32,
    /// The soft edge's width (cleaned: at least 1 mm, at most the radius).
    pub softness: f32,
    /// The darkness over the rest (linear RGB and strength, cleaned to 0..1).
    pub darkness: [f32; 4],
    /// The outline of what it sees (world X and Y, counter-clockwise).
    pub area: Vec<[f32; 2]>,
}

/// The 2D lighting of a scene, as extraction reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lighting2DView {
    /// When `Lighting2D.enabled`: the ambient light (linear RGB, cleaned).
    pub ambient: Option<[f32; 3]>,
    /// Every `Light2D`, in entity order (lit areas only while enabled).
    pub lights: Vec<Light2DView>,
    /// Every enabled `Vision2D` that sees anything, in entity order.
    pub visions: Vec<VisionView>,
}

fn clean(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}

fn rgb(color: Color, scale: f32) -> [f32; 3] {
    let [r, g, b, _] = color.to_array();
    [r, g, b].map(|c| clean(c * scale, 0.0, BRIGHTEST, 0.0))
}

/// Reads the 2D lights and visions of `world`, with the areas they reach
/// against its walls (`wish_sight::current`).
pub(crate) fn extract(world: &World) -> Lighting2DView {
    let registry = world.registry();
    let has_lights = registry.component_id_of::<Light2D>().is_some();
    let has_visions = registry.component_id_of::<Vision2D>().is_some();
    let lighting = world.resource::<Lighting2D>().copied();
    let mut view = Lighting2DView {
        ambient: lighting.filter(|l| l.enabled).map(|l| rgb(l.ambient, 1.0)),
        ..Lighting2DView::default()
    };
    if !has_lights && !has_visions {
        return view;
    }
    let position = |entity: Entity| {
        world
            .get::<GlobalTransform>(entity)
            .map(|g| g.matrix.w_axis.truncate())
            .or_else(|| {
                world
                    .get::<wish_types::transform::Transform>(entity)
                    .map(|t| t.translation)
            })
            .filter(|p| p.is_finite())
    };
    let mut cached = None;
    let mut walls = || {
        cached
            .get_or_insert_with(|| wish_sight::current(world))
            .clone()
    };
    for entity in world.entities() {
        if has_lights && let Some(light) = world.get::<Light2D>(entity).copied() {
            let Some(at) = position(entity) else { continue };
            let radius = clean(light.radius, 0.0, wish_sight::MAX_RADIUS, 0.0);
            let color = rgb(light.color, clean(light.intensity, 0.0, 100.0, 0.0));
            let area = if view.ambient.is_some() && radius >= SMALLEST && color != [0.0; 3] {
                if light.shadows {
                    walls().visible_polygon([at.x, at.y], radius).points
                } else {
                    wish_sight::Occluders::default()
                        .visible_polygon([at.x, at.y], radius)
                        .points
                }
            } else {
                Vec::new()
            };
            view.lights.push(Light2DView {
                entity,
                position: at,
                color,
                radius,
                area,
            });
        }
        if has_visions && let Some(vision) = world.get::<Vision2D>(entity).copied() {
            let Some(at) = position(entity) else { continue };
            let radius = clean(vision.radius, 0.0, wish_sight::MAX_RADIUS, 0.0);
            if !vision.enabled || radius < SMALLEST {
                continue;
            }
            let softness = clean(vision.softness, SMALLEST, radius, SMALLEST).max(SMALLEST);
            let darkness = vision.darkness.to_array().map(|c| clean(c, 0.0, 1.0, 0.0));
            let area = walls().visible_polygon([at.x, at.y], radius).points;
            view.visions.push(VisionView {
                entity,
                position: at,
                radius,
                softness,
                darkness,
                area,
            });
        }
    }
    view
}

/// The size of one fan vertex: position (3), centre (2), radius and soft
/// edge (2), colour (4), all `f32`.
pub(crate) const FAN_VERTEX_BYTES: u64 = 44;

/// The triangles of the 2D lighting of one picture, ready for the GPU.
#[derive(Clone, Debug, Default)]
pub(crate) struct LightingPlan {
    /// The picture's size: the light texture and the mask are as large.
    pub size: (u32, u32),
    /// With 2D lighting: the ambient the light texture starts from.
    pub ambient: Option<[f32; 3]>,
    /// Every light's area as triangles (`FAN_VERTEX_BYTES` a vertex).
    pub light_fans: Vec<u8>,
    /// Every vision's area as triangles.
    pub vision_fans: Vec<u8>,
    /// With visions: the darkness over what none of them sees.
    pub darkness: Option<[f32; 4]>,
}

impl LightingPlan {
    /// How many vertices a fan buffer holds.
    pub(crate) fn vertices(bytes: &[u8]) -> u32 {
        u32::try_from(bytes.len() as u64 / FAN_VERTEX_BYTES).unwrap_or(u32::MAX)
    }

    /// How many draw calls and triangles drawing it takes (the light
    /// texture, the mask and the darkness).
    pub(crate) fn counts(&self) -> (u32, u64) {
        let lights = u64::from(Self::vertices(&self.light_fans)) / 3;
        let visions = u64::from(Self::vertices(&self.vision_fans)) / 3;
        let draws =
            u32::from(lights > 0) + u32::from(visions > 0) + u32::from(self.darkness.is_some());
        (draws, lights + visions + u64::from(self.darkness.is_some()))
    }
}

/// Adds the triangles of a fan around `centre` over `area` to `out`.
fn fan(out: &mut Vec<u8>, centre: Vec3, area: &[[f32; 2]], reach: [f32; 2], color: [f32; 4]) {
    if area.len() < 3 {
        return;
    }
    let mut vertex = |x: f32, y: f32| {
        for value in [x, y, centre.z, centre.x, centre.y, reach[0], reach[1]]
            .into_iter()
            .chain(color)
        {
            out.extend_from_slice(&value.to_le_bytes());
        }
    };
    for (i, [x, y]) in area.iter().enumerate() {
        let [nx, ny] = area[(i + 1) % area.len()];
        vertex(centre.x, centre.y);
        vertex(*x, *y);
        vertex(nx, ny);
    }
}

/// The plan of `view` for a picture of `size`, and what could not be drawn
/// as written; `None` when the picture has neither 2D lighting nor a
/// vision.
pub(crate) fn plan(
    view: &Lighting2DView,
    size: (u32, u32),
    names: &dyn Fn(Entity) -> String,
    warnings: &mut Vec<Diagnostic>,
) -> Option<LightingPlan> {
    if view.ambient.is_none()
        && let Some(light) = view.lights.first()
    {
        warnings.push(Diagnostic::warning(
            "light_2d_without_lighting",
            format!(
                "The scene has 2D lights (the first is entity {}) and 2D lighting is off, so they light nothing.",
                names(light.entity)
            ),
            "Set Lighting2D.enabled to true (and its ambient to how dark the scene is without lights), or remove the Light2D components.",
        ));
    }
    if view.ambient.is_none() && view.visions.is_empty() {
        return None;
    }
    let mut plan = LightingPlan {
        size,
        ambient: view.ambient,
        ..LightingPlan::default()
    };
    if view.ambient.is_some() {
        for light in &view.lights {
            let [r, g, b] = light.color;
            fan(
                &mut plan.light_fans,
                light.position,
                &light.area,
                [light.radius.max(SMALLEST), SMALLEST],
                [r, g, b, 1.0],
            );
        }
    }
    for vision in &view.visions {
        fan(
            &mut plan.vision_fans,
            vision.position,
            &vision.area,
            [vision.radius.max(SMALLEST), vision.softness.max(SMALLEST)],
            [1.0; 4],
        );
    }
    plan.darkness = view.visions.first().map(|v| v.darkness);
    Some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fan_is_three_vertices_a_side() {
        let mut out = Vec::new();
        let square = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
        fan(&mut out, Vec3::ZERO, &square, [1.0, 0.1], [1.0; 4]);
        assert_eq!(LightingPlan::vertices(&out), 12);
        fan(&mut out, Vec3::ZERO, &square[..2], [1.0, 0.1], [1.0; 4]);
        assert_eq!(
            LightingPlan::vertices(&out),
            12,
            "fewer than three points: nothing"
        );
    }

    #[test]
    fn lights_while_lighting_is_off_are_a_warning_and_draw_nothing() {
        let view = Lighting2DView {
            ambient: None,
            lights: vec![Light2DView {
                entity: Entity::default(),
                position: Vec3::ZERO,
                color: [1.0; 3],
                radius: 3.0,
                area: Vec::new(),
            }],
            visions: Vec::new(),
        };
        let mut warnings = Vec::new();
        assert!(plan(&view, (8, 8), &|_| "'lamp'".to_owned(), &mut warnings).is_none());
        assert_eq!(&*warnings[0].code, "light_2d_without_lighting");
    }
}
