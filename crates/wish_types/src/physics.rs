//! Physics (ADR-014, design note M6 §3): what a scene or game code writes to
//! make things fall, collide and be pushed, and what the engine writes back
//! every tick.
//!
//! An entity takes part in physics when it has a [`Collider`]. With a
//! [`RigidBody`] it moves; without one it is fixed scenery. The engine
//! writes the body's place back into its `Transform` and its speed into
//! [`BodyVelocity`] after every step; writing either yourself moves the body
//! there (a teleport) or sets its speed.
//!
//! Units: meters, seconds, kilograms (ADR-021); angles in radians, except
//! fields whose name ends in `_deg`.

use wish_core::Entity;
use wish_core::math::Vec3;
use wish_schema::{component, event, resource, schema};

/// How a rigid body moves.
#[schema]
pub enum BodyKind {
    /// Moved by forces, gravity and collisions.
    Dynamic,
    /// Never moves (walls, floors). The same as a collider without a body.
    Fixed,
    /// Moved only by its `Transform` (or a character controller); pushes
    /// dynamic bodies but is not pushed back.
    Kinematic,
}

/// Makes an entity with a [`Collider`] move under physics.
#[component]
pub struct RigidBody {
    /// Dynamic, fixed or kinematic.
    pub kind: BodyKind,
    /// How quickly the body slows down on its own (air drag).
    #[schema(unit = "1/s", range = 0.0..=100.0, default = 0.0)]
    pub linear_damping: f32,
    /// How quickly the body stops spinning on its own.
    #[schema(unit = "1/s", range = 0.0..=100.0, default = 0.05)]
    pub angular_damping: f32,
    /// Multiplies gravity for this body: 0 floats, 2 falls twice as hard.
    #[schema(range = -10.0..=10.0, default = 1.0)]
    pub gravity_scale: f32,
    /// Whether the body may sleep when it comes to rest (it wakes when
    /// touched or moved). Sleeping bodies cost nothing.
    #[schema(default = true)]
    pub can_sleep: bool,
    /// Continuous collision detection, for fast small things (bullets) that
    /// would otherwise pass through thin walls between two ticks.
    pub ccd: bool,
    /// Stops movement along world X, Y, Z. A 2D physics profile
    /// (`PhysicsSettings.profile`) locks Z itself.
    pub lock_translation: [bool; 3],
    /// Stops rotation about world X, Y, Z. A 2D physics profile locks X and
    /// Y itself; for an upright character, lock all three.
    pub lock_rotation: [bool; 3],
}

/// The shape of a collider. In a 2D physics profile each shape is its
/// outline in the X–Y plane, and `size.z` is not used.
#[schema]
pub enum ColliderShape {
    /// A box; `size` is its full width, height and depth. In 2D, a rectangle
    /// of `size.x` by `size.y`.
    Box,
    /// A ball; `size.x` is its diameter. In 2D, a circle.
    Sphere,
    /// A pill standing along Y; `size.x` is its diameter, `size.y` its full
    /// height including the round ends. In 2D, the same outline (a rounded
    /// rectangle); turn the entity about Z to lay it down.
    Capsule,
    /// A cylinder standing along Y; `size.x` is its diameter, `size.y` its
    /// height. 3D only: in 2D it acts as the rectangle seen from its side
    /// (`check` warns; use `Box`).
    Cylinder,
}

/// The solid shape of an entity for physics. The entity's `Transform`
/// scale (and its parents') scales the shape too, so a `Box` of size 1 on a
/// cube `Primitive` matches it at any scale.
#[component]
pub struct Collider {
    /// Box, sphere, capsule or cylinder.
    pub shape: ColliderShape,
    /// The shape's size, before the entity's scale (see each shape).
    #[schema(unit = "m", default = Vec3::ONE)]
    pub size: Vec3,
    /// Where the shape's centre is, in the entity's own space.
    #[schema(unit = "m")]
    pub offset: Vec3,
    /// 0 is ice, 1 is rubber on concrete.
    #[schema(range = 0.0..=10.0, default = 0.5)]
    pub friction: f32,
    /// Bounciness: 0 does not bounce, 1 bounces back to the same height.
    #[schema(range = 0.0..=1.0, default = 0.0)]
    pub restitution: f32,
    /// Mass per volume. Water is 1000; the default makes a 1 m cube weigh
    /// 1000 kg.
    #[schema(unit = "kg/m3", range = 0.001..=100000.0, default = 1000.0)]
    pub density: f32,
    /// A sensor detects overlaps (trigger zones) but does not collide.
    pub sensor: bool,
    /// The groups this collider belongs to, as bits.
    #[schema(default = 1)]
    pub layer: u32,
    /// The groups this collider collides with, as bits: two colliders
    /// interact only if each one's layer is in the other's mask.
    #[schema(default = u32::MAX)]
    pub mask: u32,
}

/// The speed of a rigid body. The engine writes it after every step; write
/// it yourself to set the speed (it then continues from there).
#[component]
pub struct BodyVelocity {
    /// Speed along world X, Y, Z.
    #[schema(unit = "m/s")]
    pub linear: Vec3,
    /// Spin about world X, Y, Z.
    #[schema(unit = "rad/s")]
    pub angular: Vec3,
}

/// A force that pushes a dynamic body every tick while the component is
/// there (thrusters, wind). For a single kick, send a [`BodyImpulse`].
#[component]
pub struct BodyForce {
    /// Push along world X, Y, Z.
    #[schema(unit = "N")]
    pub force: Vec3,
    /// Twist about world X, Y, Z.
    #[schema(unit = "N*m")]
    pub torque: Vec3,
}

/// A one-off kick to a dynamic body (a jump, a hit), applied at the next
/// physics step.
#[event]
pub struct BodyImpulse {
    /// The body.
    pub entity: Entity,
    /// Change of momentum along world X, Y, Z (mass times change of speed).
    #[schema(unit = "N*s")]
    pub linear: Vec3,
    /// Change of angular momentum about world X, Y, Z.
    #[schema(unit = "N*m*s")]
    pub angular: Vec3,
}

/// Whether physics is 3D or 2D, and which kind of 2D (design note
/// 1.1-physics-2d). A 2D world lies in the X–Y plane (+Y up); the camera
/// looks at it along −Z.
#[schema]
pub enum PhysicsProfile {
    /// Full 3D physics.
    Full3D,
    /// 2D seen from above (a map of rooms and corridors): no gravity
    /// (`gravity` is not used) and no ground; characters slide along every
    /// wall, and `CharacterState.grounded` stays false.
    TopDown2D,
    /// 2D seen from the side (a platformer): gravity pulls along −Y
    /// (`gravity`'s z is not used); characters stand on the ground, climb
    /// steps and jump as in 3D.
    SideView2D,
}

/// World-wide physics settings.
#[resource]
pub struct PhysicsSettings {
    /// Gravity.
    #[schema(unit = "m/s2", default = Vec3::new(0.0, -9.81, 0.0))]
    pub gravity: Vec3,
    /// Physics steps per tick: more is steadier for fast or stacked things,
    /// and costs more.
    #[schema(range = 1..=8, default = 1)]
    pub substeps: u32,
    /// 3D, or 2D in the X–Y plane. In a 2D profile everything that moves
    /// stays in the plane and turns only about Z (the locks of `RigidBody`
    /// are set for it), every collider is placed at z = 0 whatever the
    /// entity's z (so things at different depths still collide; their own z
    /// is kept), shapes are their outline in the plane (`size.z` is not
    /// used; a collider weighs `density` × its area × 1 m), and ray and
    /// overlap questions are asked in the plane.
    pub profile: PhysicsProfile,
}

/// Two colliders started touching (or a collider entered a sensor). `a`
/// comes before `b` in entity order.
#[event]
pub struct CollisionStarted {
    /// One of the two entities.
    pub a: Entity,
    /// The other.
    pub b: Entity,
    /// Whether one of them is a sensor (a trigger, not a contact).
    pub sensor: bool,
}

/// Two colliders stopped touching (or a collider left a sensor).
#[event]
pub struct CollisionEnded {
    /// One of the two entities.
    pub a: Entity,
    /// The other.
    pub b: Entity,
    /// Whether one of them is a sensor.
    pub sensor: bool,
}

/// Moves a kinematic body like a game character: it slides along walls,
/// climbs slopes and steps, and stays on the ground. Put it with a
/// [`Collider`] (a capsule is usual) and a kinematic [`RigidBody`]; write
/// the wanted motion into [`CharacterMotion`] and read the outcome from
/// [`CharacterState`]. In a 2D side-view profile it walks as in 3D, in the
/// plane; in a top-down profile there is no ground: slopes, steps and
/// `snap_to_ground` are not used, and it slides along every wall.
#[component]
pub struct CharacterController {
    /// The steepest slope the character can walk up.
    #[schema(unit = "deg", range = 0.0..=89.0, default = 45.0)]
    pub max_slope_deg: f32,
    /// The highest step the character climbs without jumping (0: none).
    #[schema(unit = "m", range = 0.0..=2.0, default = 0.3)]
    pub step_height: f32,
    /// How far below its feet the character looks for ground to stick to
    /// when walking down slopes and stairs (0: off).
    #[schema(unit = "m", range = 0.0..=2.0, default = 0.2)]
    pub snap_to_ground: f32,
    /// A thin gap kept between the character and what it touches.
    #[schema(unit = "m", range = 0.001..=0.5, default = 0.02)]
    pub skin: f32,
}

/// What a character wants to do this tick, written by game code.
#[component]
pub struct CharacterMotion {
    /// The wanted speed along world X, Y, Z, gravity included (game code
    /// adds gravity to `y` itself, so jumping is just setting `y`). In a 2D
    /// profile `z` is not used; seen from above, `y` is a direction on the
    /// map like `x`.
    #[schema(unit = "m/s")]
    pub velocity: Vec3,
}

/// What a character's last move did, written by the engine.
#[component]
pub struct CharacterState {
    /// Whether the character stands on something (always false in a 2D
    /// top-down profile, which has no ground).
    pub grounded: bool,
    /// Whether the move was stopped by a wall or ceiling (seen from above:
    /// by anything).
    pub blocked: bool,
    /// How far the character actually moved.
    #[schema(unit = "m")]
    pub moved: Vec3,
}
