//! Rendering for Wish Engine.
//!
//! - [`components`]: what scenes say about how things look (cameras, lights,
//!   shapes, materials, models, the sky).
//! - [`extract`]: copying what to draw out of the world into a
//!   [`RenderScene`]; drawing never touches the simulation.
//! - [`gpu`]: choosing an adapter (hardware or software) and opening a
//!   device; every GPU error becomes a [`wish_core::Diagnostic`] instead of a
//!   crash.
//! - [`capture`]: drawing into a texture and reading it back without a
//!   window.
//! - [`image`]: the read-back image, PNG writing and tolerant comparison for
//!   golden-image tests.
//! - [`shader`]: WGSL checking with positioned diagnostics.
//!
//! Rendering is outside the determinism boundary: nothing here reads from or
//! writes to the simulation.
//!
//! Design: `docs/design/M3-ilk-piksel.md`.

pub mod capture;
mod cluster;
pub mod components;
pub mod extract;
mod gizmo;
pub mod gpu;
pub mod image;
pub mod mesh;
pub mod renderer;
pub mod shader;
mod texture;
pub mod ui;

pub use capture::{MAX_CAPTURE_SIZE, draw_probe};
pub use components::{
    Ambient, Camera, DirectionalLight, Exposure, Material, Model, Primitive, Shape, Sky,
};
pub use extract::{
    CameraView, ColliderView, Draw, LightView, LocalLightView, MaterialData, Mesh, RenderScene,
    SkyData, UiBoxView, UiTextView, extract,
};
pub use gpu::{AdapterSummary, Gpu, GpuOptions};
pub use image::{ImageDiff, RgbaImage, Tolerance};
pub use renderer::{
    Description, ENTRY_SHADERS, Frame, IdBuffer, MAX_VISIBILITY_MEASUREMENTS, Renderer, View,
    VisibleEntity, check_shaders, engine_shaders, engine_shaders_from_dir, exposure_multiplier,
    project_to_pixel,
};
pub use shader::{CheckedShader, ComposedShader, ShaderLibrary, SourceLine, check_wgsl};
pub use ui::{UiElement, UiPicture, ui_elements};
