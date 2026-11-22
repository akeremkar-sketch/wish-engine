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

pub mod anim;
pub mod audio;
pub mod input;
pub mod lights2d;
pub mod lobby;
pub mod net;
pub mod particles;
pub mod physics;
pub mod render;
pub mod sprite;
pub mod tilemap;
pub mod time;
pub mod transform;
pub mod ui;

pub use anim::{AnimationFinished, AnimationPlayer};
pub use audio::{
    AudioBus, AudioListener, AudioOneShot, AudioPlayback, AudioSettings, AudioSource, PlaySound,
    SoundEnded, SoundStarted,
};
pub use input::{
    ActionSlot, ActionState, MAX_ACTIONS, MAX_PLAYERS, PlayerActionSlots, PlayerActionState,
    PlayerId, Players, action_id,
};
pub use lights2d::{Light2D, Lighting2D, Occluder, Vision2D};
pub use lobby::{
    Lobby, LobbyAction, LobbyClosed, LobbyPhase, LobbyRequest, LocalPlayer, MatchRoles, Net, NetTo,
    Seat,
};
pub use net::{NetGroups, NetOwner, NetViewer, NetVisibility, Replicated};
pub use particles::{ParticleEmitter, ParticleState};
pub use physics::{
    BodyForce, BodyImpulse, BodyKind, BodyVelocity, CharacterController, CharacterMotion,
    CharacterState, Collider, ColliderShape, CollisionEnded, CollisionStarted, PhysicsProfile,
    PhysicsSettings, RigidBody,
};
pub use render::{
    AlphaMode, Ambient, AmbientSource, Antialiasing, Bloom, Camera, ColorGrading, DirectionalLight,
    EMISSIVE_NITS, Exposure, Fog, Material, MaterialFile, Model, PointLight, Primitive, Projection,
    RenderSettings, Shape, Sky, SpotLight,
};
pub use sprite::{Sprite, SpriteAnimator};
pub use tilemap::Tilemap;
pub use time::Time;
pub use transform::{GLOBAL_TRANSFORM, GlobalTransform, Transform, world_position};
pub use ui::{
    ButtonClicked, Pointer, UiAlign, UiButton, UiDirection, UiImage, UiInteraction, UiJustify,
    UiLayout, UiLength, UiNode, UiPosition, UiSettings, UiText, UiTextAlign, UiUnit,
};
