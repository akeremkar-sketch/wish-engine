//! The engine's own data types, declared once and shared.
//!
//! These are plain declarations (`#[component]`, `#[resource]`): no systems,
//! no world. That lets two very different users depend on them:
//!
//! - the engine's crates (`wish_transform`, `wish_render`, `wish_app`),
//!   which re-export them and add the systems that use them;
//! - game code compiled to WebAssembly through `wish_sdk` (ADR-011, M5),
//!   which sees **the same declarations**, so the bytes the engine stores and
//!   the bytes the game reads cannot drift apart (design note M5 §3).
//!
//! Nothing here may depend on anything that does not build for
//! `wasm32-unknown-unknown`.

pub mod input;
pub mod render;
pub mod time;
pub mod transform;

pub use input::{ActionSlot, ActionState, MAX_ACTIONS, action_id};
pub use render::{
    Ambient, Camera, DirectionalLight, EMISSIVE_NITS, Exposure, Material, Model, Primitive, Shape,
    Sky,
};
pub use time::Time;
pub use transform::{GLOBAL_TRANSFORM, GlobalTransform, Transform, world_position};
