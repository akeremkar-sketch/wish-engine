//! The game code SDK: systems compiled to WebAssembly (ADR-011, M5).
//!
//! ```ignore
//! use wish_sdk::prelude::*;
//!
//! /// A coin the player can pick up.
//! #[component]
//! pub struct Coin {
//!     /// Points it is worth.
//!     #[schema(range = 1..=100, default = 1)]
//!     pub value: u32,
//! }
//!
//! /// Points collected so far.
//! #[resource]
//! pub struct Score {
//!     /// The total.
//!     pub points: u32,
//! }
//!
//! #[system]
//! fn collect(coins: Query<(Entity, &Transform, &Coin)>, mut score: ResMut<Score>, mut commands: Commands) {
//!     for (entity, at, coin) in coins.iter() {
//!         if at.translation.length() < 1.0 {
//!             score.points += coin.value;
//!             commands.despawn(entity);
//!         }
//!     }
//! }
//!
//! wish_sdk::export! {
//!     components: [Coin],
//!     resources: [Score],
//!     systems: [collect],
//! }
//! ```
//!
//! - Types are declared with the engine's own macros (`#[component]`,
//!   `#[resource]`, `#[event]`, `#[schema]`), so they follow the same rules
//!   (every field documented) and scenes and tools see them exactly as the
//!   game code does. The engine's types (`Transform`, `Camera`, …) are the
//!   engine's own declarations (`wish_types`).
//! - A system is a function marked `#[system]` whose parameters are
//!   [`Query`], [`Res`], [`ResMut`], [`Commands`], [`EventReader`] and
//!   [`EventWriter`]. Ordering: `#[system(after = "move_player")]`.
//! - All state lives in the world. The module itself starts fresh every
//!   tick, so a `static` cannot carry anything from one tick to the next;
//!   this is what makes snapshots complete and code reloads lossless.
//! - No clock, files or network. Randomness comes from the system's own
//!   seeded stream ([`random_u64`], [`random_f32`], [`random_range`]); time
//!   from `Res<Time>`.

pub mod abi;
pub mod command;
pub mod frame;
pub mod manifest;
mod param;
pub mod physics;
mod query;

pub use param::{
    Actions, Commands, EntityCommands, EventReader, EventWriter, NewEntity, Res, ResMut,
    SystemParam,
};
pub use query::{ColumnAccess, Query, QueryData, QueryFilter, With, Without};
pub use wish_core::{AssetRef, Color, Entity, Text, math};
pub use wish_schema::{component, event, resource, schema};
pub use wish_sdk_macros::system;
pub use wish_types::{
    ActionSlot, ActionState, Ambient, AnimationFinished, AnimationPlayer, AudioBus, AudioListener,
    AudioPlayback, AudioSettings, AudioSource, BodyForce, BodyImpulse, BodyKind, BodyVelocity,
    ButtonClicked, Camera, CharacterController, CharacterMotion, CharacterState, Collider,
    ColliderShape, CollisionEnded, CollisionStarted, DirectionalLight, Exposure, GlobalTransform,
    Material, Model, ParticleEmitter, ParticleState, PhysicsSettings, PlaySound, PointLight,
    Pointer, Primitive, Projection, RenderSettings, RigidBody, Shape, Sky, SoundEnded,
    SoundStarted, SpotLight, Sprite, Time, Transform, UiAlign, UiButton, UiDirection, UiImage,
    UiInteraction, UiJustify, UiLayout, UiLength, UiNode, UiPosition, UiSettings, UiText,
    UiTextAlign, UiUnit, world_position,
};
pub use wish_types::{AmbientSource, Antialiasing, Bloom, ColorGrading, Fog};

pub use physics::{Ray, RayHit, raycast};

/// The ABI version this SDK speaks.
pub const ABI_VERSION: i32 = 1;

/// Everything a game module usually needs.
pub mod prelude {
    pub use crate::math::{Mat4, Quat, Vec2, Vec3, Vec4};
    pub use crate::{
        Actions, Ambient, AnimationFinished, AnimationPlayer, AssetRef, AudioBus, AudioListener,
        AudioPlayback, AudioSettings, AudioSource, BodyForce, BodyImpulse, BodyKind, BodyVelocity,
        Camera, CharacterController, CharacterMotion, CharacterState, Collider, ColliderShape,
        CollisionEnded, CollisionStarted, Color, Commands, DirectionalLight, Entity, EventReader,
        EventWriter, Exposure, GlobalTransform, Material, Model, PhysicsSettings, PlaySound,
        PointLight, Primitive, Query, Ray, RayHit, RenderSettings, Res, ResMut, RigidBody, Shape,
        Sky, SoundEnded, SoundStarted, SpotLight, Text, Time, Transform, With, Without, asset,
        component, event, random_f32, random_range, random_u64, raycast, resource, schema, system,
        text, text_of, world_position,
    };
    pub use crate::{AmbientSource, Antialiasing, Bloom, ColorGrading, Fog};
    pub use crate::{
        ButtonClicked, Pointer, UiAlign, UiButton, UiDirection, UiImage, UiInteraction, UiJustify,
        UiLayout, UiLength, UiNode, UiPosition, UiSettings, UiText, UiTextAlign, UiUnit,
    };
    pub use crate::{ParticleEmitter, ParticleState, Projection, Sprite};
}

/// The handle of `text`, to write into a field of type `Text` (such as
/// `UiText.text`): `text(&format!("Score: {score}"))`. The engine keeps
/// each distinct text once, for the life of the world, so reuse texts
/// rather than make a new one every tick. A text longer than 4096 bytes,
/// more than 1024 new texts in one system call, or a world already holding
/// 16 MiB of text, is refused: the result is the empty text and a warning
/// is logged.
pub fn text(text: &str) -> Text {
    match abi::text_intern(text) {
        Some(index) => Text::from_index(index),
        None => {
            abi::log(
                1,
                "a text was refused (longer than 4096 bytes, more than 1024 new texts in one system call, or the world's text is full); the empty text is used",
            );
            Text::EMPTY
        }
    }
}

/// A reference to a project file, for a field of type `AssetRef` (such as
/// `PlaySound.clip` or `UiImage.image`): `asset("assets/sounds/jump.wav")`.
pub fn asset(path: &str) -> AssetRef {
    AssetRef::from_text(text(path))
}

/// The string a `Text` handle stands for (empty for an unknown handle).
pub fn text_of(text: Text) -> String {
    abi::text_read(text.index()).unwrap_or_default()
}

/// The next number from this system's own random stream.
pub fn random_u64() -> u64 {
    abi::random_u64()
}

/// A number in `[0, 1)` from this system's own random stream.
pub fn random_f32() -> f32 {
    (random_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// A number in `[low, high)` from this system's own random stream.
pub fn random_range(low: f32, high: f32) -> f32 {
    low + (high - low) * random_f32()
}

/// Logs a message (it appears in the engine's log, with the system's name).
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::abi::log(0, &::std::format!($($arg)*)) };
}

/// Logs a warning.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::abi::log(1, &::std::format!($($arg)*)) };
}

/// Logs an error (the system keeps running; to stop, panic).
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::abi::log(2, &::std::format!($($arg)*)) };
}

#[doc(hidden)]
pub mod __private {
    pub use crate::frame::FrameReader;
    pub use crate::manifest::{TypeTable, manifest_json, system_json};
    pub use serde_json::Value;

    /// What `#[system]` implements for the function's name.
    pub trait SystemDef {
        /// The system's name.
        const NAME: &'static str;
        /// Its manifest entry.
        fn describe(types: &mut TypeTable) -> Value;
        /// Reads its parameters from the frame and calls it.
        fn run(frame: &mut FrameReader);
    }
}

/// Writes the module's exports: lists its own types (those used by systems
/// are found by themselves; list the others so scenes can use them) and its
/// systems.
///
/// ```ignore
/// wish_sdk::export! {
///     components: [Coin, Player],
///     resources: [Score],
///     events: [Collected],
///     systems: [move_player, collect],
/// }
/// ```
#[macro_export]
macro_rules! export {
    (
        $(components: [$($component:ty),* $(,)?],)?
        $(resources: [$($resource:ty),* $(,)?],)?
        $(events: [$($event:ty),* $(,)?],)?
        systems: [$($system:ident),* $(,)?] $(,)?
    ) => {
        #[doc(hidden)]
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_abi_version() -> i32 {
            $crate::ABI_VERSION
        }

        #[doc(hidden)]
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_alloc(len: i32) -> i32 {
            $crate::abi::alloc(len)
        }

        #[doc(hidden)]
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_manifest() -> i64 {
            let mut types = $crate::__private::TypeTable::new();
            $($( types.component::<$component>(); )*)?
            $($( types.resource::<$resource>(); )*)?
            $($( types.event::<$event>(); )*)?
            let systems = ::std::vec![
                $( <$system as $crate::__private::SystemDef>::describe(&mut types) ),*
            ];
            $crate::abi::manifest($crate::__private::manifest_json(&types, systems))
        }

        #[doc(hidden)]
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_run(system: i32, frame: i32) -> i32 {
            let systems: &[fn(&mut $crate::__private::FrameReader)] = &[
                $( <$system as $crate::__private::SystemDef>::run ),*
            ];
            $crate::abi::run(systems, system, frame)
        }
    };
}
