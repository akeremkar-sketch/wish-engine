//! The window, its event loop and the viewer's state.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{Gpu, GpuOptions, RenderScene, Renderer, View};

use crate::camera::{FlyCamera, Movement};

/// How often the files are checked for changes.
const POLL: Duration = Duration::from_millis(400);
/// The smallest picture drawn (a minimised window reports 0).
const MIN_SIZE: u32 = 16;

/// A scene as the viewer needs it.
pub struct Loaded {
    /// What to draw.
    pub scene: RenderScene,
    /// Things worth telling the person looking (a model that did not load).
    pub warnings: Vec<Diagnostic>,
}

/// Where the viewer gets its scene from.
pub trait Content {
    /// Loads (or reloads) the scene and uploads whatever it needs to the
    /// renderer, such as models. A failure keeps the last good scene.
    fn load(&mut self, gpu: &Gpu, renderer: &mut Renderer) -> Result<Loaded>;

    /// What to call an entity in messages (its name in the scene file).
    fn name_of(&mut self, entity: Entity) -> String;

    /// A number that changes when any file the scene is made of changes
    /// (see [`fingerprint_dir`]).
    fn fingerprint(&mut self) -> u64;
}

/// How the viewer starts.
pub struct ViewerOptions {
    /// The window title's first words.
    pub title: String,
    /// The window's inner size in pixels.
    pub size: (u32, u32),
    /// Use the software adapter.
    pub software: bool,
    /// A folder of `<name>.wgsl` files that replace the built-in shaders;
    /// they are reloaded when they change.
    pub shaders: Option<PathBuf>,
    /// The view to start in.
    pub view: View,
    /// Close the window by itself after this many frames (for scripts and
    /// tests); `None` runs until it is closed.
    pub exit_after_frames: Option<u64>,
    /// Receives every message (reloads, mistakes, picks).
    pub on_message: Box<dyn FnMut(&str)>,
}

/// What a viewer session did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Pictures drawn.
    pub frames: u64,
    /// Times the scene was reloaded after a file change.
    pub scene_reloads: u64,
    /// Times the shaders were reloaded after a file change.
    pub shader_reloads: u64,
    /// Changes that were rejected (the last good picture stayed).
    pub rejected: u64,
    /// Entities picked with the mouse, in order.
    pub picks: Vec<Entity>,
}

/// A number that changes when a file under `dir` with one of `extensions`
/// (such as `"json"`) is added, removed, resized or modified. Folders called
/// `target`, `.git` and hidden ones are skipped.
pub fn fingerprint_dir(dir: &Path, extensions: &[&str]) -> u64 {
    fn visit(dir: &Path, extensions: &[&str], total: &mut u64, depth: u32) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if depth < 8 && name != "target" && !name.starts_with('.') {
                    visit(&path, extensions, total, depth + 1);
                }
                continue;
            }
            let wanted = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext));
            if !wanted {
                continue;
            }
            let nanos = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_nanos() as u64);
            // Order-independent, so directory order does not matter.
            let mut entry_hash = nanos ^ meta.len().rotate_left(17);
            for byte in path.to_string_lossy().bytes() {
                entry_hash = entry_hash.wrapping_mul(0x0100_0000_01b3) ^ u64::from(byte);
            }
            *total = total.wrapping_add(entry_hash);
        }
    }
    let mut total = 0;
    visit(dir, extensions, &mut total, 0);
    total
}

/// Opens the window and runs until it is closed (or `exit_after_frames`).
pub fn run(options: ViewerOptions, content: &mut dyn Content) -> Result<Report> {
    let gpu = Gpu::shared(GpuOptions {
        software: options.software,
    })?;
    let mut renderer = match &options.shaders {
        Some(dir) => Renderer::with_shaders(&gpu, &wish_render::engine_shaders_from_dir(dir)?.0)?,
        None => Renderer::new(&gpu)?,
    };
    let loaded = content.load(&gpu, &mut renderer)?;
    let content_stamp = content.fingerprint();
    let shader_stamp = options
        .shaders
        .as_deref()
        .map_or(0, |dir| fingerprint_dir(dir, &["wgsl"]));
    let event_loop =
        EventLoop::new().map_err(|error| window_error("start the event loop", &error))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        gpu,
        renderer,
        content,
        options,
        scene: loaded.scene,
        base_camera: None,
        eye: None,
        keys: Movement::default(),
        looking: false,
        view: View::Lit,
        window: None,
        surface: None,
        cursor: (0.0, 0.0),
        press_at: None,
        dirty: true,
        last_frame: Instant::now(),
        last_poll: Instant::now(),
        content_stamp,
        shader_stamp,
        picked: None,
        status: String::new(),
        report: Report::default(),
        error: None,
    };
    app.view = app.options.view;
    app.reset_camera();
    for warning in &loaded.warnings {
        app.say(&format!("{warning}"));
    }
    event_loop
        .run_app(&mut app)
        .map_err(|error| window_error("run the window", &error))?;
    match app.error {
        Some(problem) => Err(problem),
        None => Ok(app.report),
    }
}

fn window_error(what: &str, error: &dyn std::fmt::Display) -> Diagnostic {
    Diagnostic::error(
        "window_failed",
        format!("Could not {what}: {error}."),
        "A window needs a desktop session. On a server or in CI there is none; use `wen capture` to draw without a window.",
    )
}

struct App<'a> {
    gpu: std::sync::Arc<Gpu>,
    renderer: Renderer,
    content: &'a mut dyn Content,
    options: ViewerOptions,
    scene: RenderScene,
    /// The scene's own first camera, which the fly camera is based on.
    base_camera: Option<wish_render::CameraView>,
    eye: Option<FlyCamera>,
    keys: Movement,
    looking: bool,
    view: View,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    cursor: (f64, f64),
    press_at: Option<(f64, f64)>,
    dirty: bool,
    last_frame: Instant,
    last_poll: Instant,
    content_stamp: u64,
    shader_stamp: u64,
    picked: Option<String>,
    status: String,
    report: Report,
    error: Option<Diagnostic>,
}

impl App<'_> {
    fn say(&mut self, message: &str) {
        (self.options.on_message)(message);
        self.status = message.lines().next().unwrap_or_default().to_owned();
    }

    /// Goes back to the scene's own camera.
    fn reset_camera(&mut self) {
        self.base_camera = self.scene.cameras.first().copied();
        self.eye = self.base_camera.as_ref().map(FlyCamera::from_view);
        self.dirty = true;
    }

    fn title(&self) -> String {
        let mut title = format!("{} - {}", self.options.title, self.view_name());
        if let Some(picked) = &self.picked {
            title.push_str(&format!(" - picked: {picked}"));
        }
        if !self.status.is_empty() {
            title.push_str(&format!(" - {}", self.status));
        }
        title
    }

    fn view_name(&self) -> &'static str {
        self.view.name()
    }

    /// Looks for changed files and reloads what changed.
    fn poll_files(&mut self) {
        let stamp = self.content.fingerprint();
        if stamp != self.content_stamp {
            self.content_stamp = stamp;
            match self.content.load(&self.gpu, &mut self.renderer) {
                Ok(loaded) => {
                    self.scene = loaded.scene;
                    // Keep where the person is looking from.
                    self.base_camera = self.scene.cameras.first().copied();
                    if self.eye.is_none() {
                        self.eye = self.base_camera.as_ref().map(FlyCamera::from_view);
                    }
                    self.report.scene_reloads += 1;
                    self.status.clear();
                    self.say("scene reloaded");
                    for warning in &loaded.warnings {
                        (self.options.on_message)(&format!("{warning}"));
                    }
                }
                Err(problem) => {
                    self.report.rejected += 1;
                    self.say(&format!(
                        "scene not reloaded, showing the last good one: {problem}"
                    ));
                }
            }
            self.dirty = true;
        }
        if let Some(dir) = self.options.shaders.clone() {
            let stamp = fingerprint_dir(&dir, &["wgsl"]);
            if stamp != self.shader_stamp {
                self.shader_stamp = stamp;
                let reloaded = wish_render::engine_shaders_from_dir(&dir)
                    .and_then(|(library, _)| self.renderer.reload_shaders(&self.gpu, &library));
                match reloaded {
                    Ok(()) => {
                        self.report.shader_reloads += 1;
                        self.status.clear();
                        self.say("shaders reloaded");
                    }
                    Err(problem) => {
                        self.report.rejected += 1;
                        self.say(&format!(
                            "shaders not reloaded, using the last good ones: {problem}"
                        ));
                    }
                }
                self.dirty = true;
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        self.window.as_ref().map_or(self.options.size, |window| {
            let size = window.inner_size();
            (size.width.max(MIN_SIZE), size.height.max(MIN_SIZE))
        })
    }

    /// Draws a picture and puts it on the window.
    fn redraw(&mut self) {
        let (width, height) = self.size();
        if let (Some(base), Some(eye)) = (self.base_camera, self.eye)
            && let Some(first) = self.scene.cameras.first_mut()
        {
            *first = eye.apply(&base);
        }
        let frame =
            self.renderer
                .render_view(&self.gpu, &self.scene, None, width, height, self.view);
        let frame = match frame {
            Ok(frame) => frame,
            Err(problem) => {
                // A scene with no camera is not a reason to close the window:
                // say so and wait for a change.
                self.say(&format!("{problem}"));
                self.dirty = false;
                return;
            }
        };
        let (Some(window), Some(surface)) = (self.window.clone(), self.surface.as_mut()) else {
            return;
        };
        let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buffer) = surface.buffer_mut() {
            for (target, rgba) in buffer.iter_mut().zip(frame.image.pixels.as_chunks::<4>().0) {
                *target = u32::from(rgba[0]) << 16 | u32::from(rgba[1]) << 8 | u32::from(rgba[2]);
            }
            window.pre_present_notify();
            let _ = buffer.present();
        }
        self.report.frames += 1;
        self.dirty = false;
        window.set_title(&self.title());
    }

    /// The entity under the cursor, named.
    fn pick(&mut self) {
        let (width, height) = self.size();
        let (x, y) = (self.cursor.0 as u32, self.cursor.1 as u32);
        let ids = self
            .renderer
            .id_buffer(&self.gpu, &self.scene, None, width, height);
        match ids {
            Ok(ids) => match ids.entity_at(x, y) {
                Some(entity) => {
                    let name = self.content.name_of(entity);
                    self.report.picks.push(entity);
                    self.say(&format!("picked {name} at ({x}, {y})"));
                    self.picked = Some(name);
                }
                None => {
                    self.picked = None;
                    self.say(&format!("nothing at ({x}, {y})"));
                }
            },
            Err(problem) => self.say(&format!("{problem}")),
        }
        self.dirty = true;
    }

    fn key(&mut self, code: KeyCode, down: bool, event_loop: &ActiveEventLoop) {
        match code {
            KeyCode::KeyW => self.keys.forward = down,
            KeyCode::KeyS => self.keys.back = down,
            KeyCode::KeyA => self.keys.left = down,
            KeyCode::KeyD => self.keys.right = down,
            KeyCode::KeyE => self.keys.up = down,
            KeyCode::KeyQ => self.keys.down = down,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => self.keys.fast = down,
            _ if !down => {}
            KeyCode::Digit1 => self.set_view(View::Lit),
            KeyCode::Digit2 => self.set_view(View::Ids),
            KeyCode::Digit3 => self.set_view(View::Depth),
            KeyCode::Digit4 => self.set_view(View::Normals),
            KeyCode::Digit5 => self.set_view(View::Wireframe),
            KeyCode::KeyF => self.reset_camera(),
            KeyCode::Escape => event_loop.exit(),
            _ => {}
        }
    }

    fn set_view(&mut self, view: View) {
        self.view = view;
        self.dirty = true;
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (width, height) = self.options.size;
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(PhysicalSize::new(width, height));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(error) => {
                self.error = Some(window_error("open the window", &error));
                event_loop.exit();
                return;
            }
        };
        let surface = softbuffer::Context::new(Rc::clone(&window))
            .and_then(|context| softbuffer::Surface::new(&context, Rc::clone(&window)));
        match surface {
            Ok(surface) => self.surface = Some(surface),
            Err(error) => {
                self.error = Some(window_error("show pictures in the window", &error));
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => self.dirty = true,
            WindowEvent::RedrawRequested => {
                self.redraw();
                if let Some(limit) = self.options.exit_after_frames
                    && self.report.frames >= limit
                {
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.key(code, event.state == ElementState::Pressed, event_loop);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Right, state) => self.looking = state == ElementState::Pressed,
                (MouseButton::Left, ElementState::Pressed) => self.press_at = Some(self.cursor),
                (MouseButton::Left, ElementState::Released) => {
                    // A click, not a drag.
                    if let Some(start) = self.press_at.take()
                        && (start.0 - self.cursor.0).abs() < 4.0
                        && (start.1 - self.cursor.1).abs() < 4.0
                    {
                        self.pick();
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.looking
            && let Some(eye) = self.eye.as_mut()
        {
            eye.look(delta.0 as f32, delta.1 as f32);
            self.dirty = true;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let seconds = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if self.keys.any()
            && let Some(eye) = self.eye.as_mut()
        {
            eye.step(self.keys, seconds);
            self.dirty = true;
        }
        if now.duration_since(self.last_poll) >= POLL {
            self.last_poll = now;
            self.poll_files();
        }
        if self.error.is_some() {
            event_loop.exit();
        }
        if let Some(window) = &self.window {
            if self.dirty {
                window.request_redraw();
            } else {
                // Nothing to draw: sleep until the next poll instead of
                // spinning.
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + POLL));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_changes_with_files_and_ignores_other_ones() {
        let dir = std::env::temp_dir().join(format!("wish-fingerprint-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let empty = fingerprint_dir(&dir, &["json"]);
        std::fs::write(dir.join("sub/a.json"), "{}").unwrap();
        let one = fingerprint_dir(&dir, &["json"]);
        assert_ne!(empty, one, "a new file");
        assert_eq!(
            one,
            fingerprint_dir(&dir, &["json"]),
            "stable while nothing changes"
        );
        std::fs::write(dir.join("sub/a.json"), "{ \"changed\": true }").unwrap();
        assert_ne!(one, fingerprint_dir(&dir, &["json"]), "a changed file");
        let two = fingerprint_dir(&dir, &["json"]);
        // Other extensions and skipped folders do not count.
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("target/b.json"), "{}").unwrap();
        assert_eq!(two, fingerprint_dir(&dir, &["json"]));
        std::fs::remove_file(dir.join("sub/a.json")).unwrap();
        assert_eq!(empty, fingerprint_dir(&dir, &["json"]), "a removed file");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
