//! A window onto a scene: a thin viewer for looking at what the engine draws.
//!
//! The renderer draws offscreen (the same path as `wen capture`, so what you
//! see is what a capture or a test sees) and the window shows the finished
//! picture. Putting the picture on screen is deliberately the simplest thing
//! that works everywhere, software adapters included; a direct GPU
//! presentation path comes with the game runtime.
//!
//! What it does:
//! - a **fly camera** (right mouse button + mouse to look, W A S D to move,
//!   Q / E down / up, Shift to go faster, F to return to the scene's own
//!   camera);
//! - the **debug views** on the keys 1 (lit), 2 (ids), 3 (depth), 4 (normals),
//!   5 (wireframe);
//! - **picking**: a left click names the entity under the cursor, from the
//!   identity buffer;
//! - **hot reload**: when a scene, prefab, model or shader file changes the
//!   picture is rebuilt; a mistake is reported and the last good picture
//!   stays.
//!
//! The scene itself comes through the [`Content`] trait, so that this crate
//! knows nothing about scene files.

mod camera;
mod viewer;

pub use camera::{FAST, FlyCamera, LOOK_SENSITIVITY, Movement, SPEED};
pub use viewer::{
    Content, Loaded, Pointing, Report, ViewerOptions, fingerprint_dir, key_name, run,
};
