//! A window that shows pictures a [`Shell`] draws on the CPU, and tells it
//! what the person does (M8a): the editor's way onto the screen.
//!
//! This crate knows nothing about the editor or its interface library: input
//! comes out as plain [`ShellEvent`]s and a finished RGBA picture goes in,
//! the same "draw somewhere else, show the result" path as the viewer.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};
use wish_core::{Diagnostic, Result};

/// The smallest picture asked for (a minimised window reports 0).
const MIN_SIZE: u32 = 16;
/// How often the shell is asked for a frame while idle.
const TICK: Duration = Duration::from_millis(16);

/// Which keys of the keyboard modifier row are held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Alt (Option).
    pub alt: bool,
    /// Control.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// The system key (Windows key, Command).
    pub logo: bool,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// The left (primary) button.
    Primary,
    /// The right (secondary) button.
    Secondary,
    /// The middle button.
    Middle,
}

/// What the person did, in plain terms.
#[derive(Clone, Debug, PartialEq)]
pub enum ShellEvent {
    /// The pointer moved to these pixels of the picture.
    Pointer {
        /// Pixels from the left edge.
        x: f32,
        /// Pixels from the top edge.
        y: f32,
    },
    /// The pointer left the window.
    PointerLeft,
    /// A button went down or up (at the pointer's last place).
    Button {
        /// Which one.
        button: Button,
        /// Down (`true`) or up.
        pressed: bool,
        /// The modifier keys held.
        modifiers: Modifiers,
    },
    /// The wheel or a touchpad scrolled, in pixels (positive y: content up).
    Scroll {
        /// Sideways.
        x: f32,
        /// Up and down.
        y: f32,
    },
    /// A key went down or up. `name` is the key's name as egui spells them:
    /// `ArrowLeft`, `Enter`, `Escape`, `A`, `5`, `F2`...
    Key {
        /// The key's name.
        name: String,
        /// Down (`true`) or up.
        pressed: bool,
        /// Held down long enough to repeat.
        repeat: bool,
        /// The modifier keys held.
        modifiers: Modifiers,
    },
    /// Text was typed (what the keys produce, not the keys).
    Text(String),
    /// The modifier keys held changed (they stay as they are until the
    /// next one of these).
    Modifiers(Modifiers),
    /// Text pasted from the system clipboard (Ctrl+V).
    Paste(String),
    /// Copy the selection to the clipboard (Ctrl+C).
    Copy,
    /// Cut the selection to the clipboard (Ctrl+X).
    Cut,
    /// The raw mouse moved by this much while the window had it (for
    /// looking around with a held button).
    MouseDelta {
        /// Sideways.
        x: f32,
        /// Up and down.
        y: f32,
    },
}

/// A finished picture to put on the window.
pub struct Picture<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA.
    pub rgba: &'a [u8],
}

/// Something that draws a window's picture and reacts to input.
pub trait Shell {
    /// The window's title.
    fn title(&self) -> String;

    /// Asked about every 16 ms and after input: `size` is the window's
    /// inner size in pixels, `seconds` the time since the last call, `events`
    /// what happened since. Returns the picture to show, or `None` when
    /// nothing changed (the window keeps the one it has).
    fn frame(
        &mut self,
        size: (u32, u32),
        seconds: f64,
        events: Vec<ShellEvent>,
    ) -> Option<Picture<'_>>;

    /// Whether the person asked to quit from inside (a menu).
    fn wants_exit(&self) -> bool {
        false
    }

    /// Text the shell asked to put on the system clipboard since the last
    /// frame (a copy or a cut), if any.
    fn take_clipboard(&mut self) -> Option<String> {
        None
    }
}

/// How to open the window.
pub struct ShellOptions {
    /// The first size, in pixels.
    pub size: (u32, u32),
    /// Close after this many pictures were shown (for trying the window
    /// without a person).
    pub exit_after_frames: Option<u64>,
    /// The window's icon: straight (not premultiplied) RGBA pixels, width
    /// and height.
    pub icon: Option<(Vec<u8>, u32, u32)>,
}

/// What a run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShellReport {
    /// Pictures shown.
    pub frames: u64,
}

/// Opens the window and runs the shell until it is closed.
pub fn run_shell(options: ShellOptions, shell: &mut dyn Shell) -> Result<ShellReport> {
    let event_loop = EventLoop::new().map_err(|error| failed("start the event loop", &error))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = ShellApp {
        shell,
        options,
        window: None,
        surface: None,
        events: Vec::new(),
        modifiers: Modifiers::default(),
        last: Instant::now(),
        report: ShellReport::default(),
        error: None,
        clipboard: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| failed("run the window", &error))?;
    match app.error {
        Some(problem) => Err(problem),
        None => Ok(app.report),
    }
}

fn failed(what: &str, error: &dyn std::fmt::Display) -> Diagnostic {
    Diagnostic::error(
        "window_failed",
        format!("Could not {what}: {error}."),
        "A window needs a desktop session. On a server or in CI there is none; use `wen edit --headless-frame` to draw without a window.",
    )
}

struct ShellApp<'a> {
    shell: &'a mut dyn Shell,
    options: ShellOptions,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    events: Vec<ShellEvent>,
    modifiers: Modifiers,
    last: Instant,
    report: ShellReport,
    error: Option<Diagnostic>,
    /// The system clipboard, opened when first needed (a desktop without
    /// one only loses copy and paste).
    clipboard: Option<arboard::Clipboard>,
}

/// The name egui gives a key, if we know one for it.
fn key_name(key: &Key) -> Option<String> {
    Some(match key {
        Key::Named(named) => match named {
            NamedKey::Space => "Space".to_owned(),
            NamedKey::Enter => "Enter".to_owned(),
            NamedKey::Tab => "Tab".to_owned(),
            NamedKey::Escape => "Escape".to_owned(),
            NamedKey::Backspace => "Backspace".to_owned(),
            NamedKey::Delete => "Delete".to_owned(),
            NamedKey::Insert => "Insert".to_owned(),
            NamedKey::Home => "Home".to_owned(),
            NamedKey::End => "End".to_owned(),
            NamedKey::PageUp => "PageUp".to_owned(),
            NamedKey::PageDown => "PageDown".to_owned(),
            NamedKey::ArrowLeft => "ArrowLeft".to_owned(),
            NamedKey::ArrowRight => "ArrowRight".to_owned(),
            NamedKey::ArrowUp => "ArrowUp".to_owned(),
            NamedKey::ArrowDown => "ArrowDown".to_owned(),
            NamedKey::F1 => "F1".to_owned(),
            NamedKey::F2 => "F2".to_owned(),
            NamedKey::F3 => "F3".to_owned(),
            NamedKey::F4 => "F4".to_owned(),
            NamedKey::F5 => "F5".to_owned(),
            _ => return None,
        },
        Key::Character(text) => text.to_uppercase(),
        _ => return None,
    })
}

impl ShellApp<'_> {
    /// The system clipboard, if this desktop has one.
    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    fn size(&self) -> (u32, u32) {
        self.window.as_ref().map_or(self.options.size, |window| {
            let size = window.inner_size();
            (size.width.max(MIN_SIZE), size.height.max(MIN_SIZE))
        })
    }

    /// Asks the shell for a frame and puts it on the window.
    fn step(&mut self) {
        let now = Instant::now();
        let seconds = now.duration_since(self.last).as_secs_f64().min(0.1);
        self.last = now;
        let size = self.size();
        let events = std::mem::take(&mut self.events);
        // What the last frame copied goes to the system clipboard.
        if let Some(text) = self.shell.take_clipboard()
            && let Some(clipboard) = self.clipboard()
        {
            let _ = clipboard.set_text(text);
        }
        let Some(picture) = self.shell.frame(size, seconds, events) else {
            return;
        };
        let (Some(window), Some(surface)) = (self.window.clone(), self.surface.as_mut()) else {
            return;
        };
        let (Some(w), Some(h)) = (
            NonZeroU32::new(picture.width),
            NonZeroU32::new(picture.height),
        ) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buffer) = surface.buffer_mut() {
            for (target, rgba) in buffer.iter_mut().zip(picture.rgba.as_chunks::<4>().0) {
                *target = u32::from(rgba[0]) << 16 | u32::from(rgba[1]) << 8 | u32::from(rgba[2]);
            }
            window.pre_present_notify();
            let _ = buffer.present();
        }
        self.report.frames += 1;
        window.set_title(&self.shell.title());
    }
}

impl ApplicationHandler for ShellApp<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (width, height) = self.options.size;
        let icon =
            self.options.icon.as_ref().and_then(|(pixels, w, h)| {
                winit::window::Icon::from_rgba(pixels.clone(), *w, *h).ok()
            });
        let attributes = Window::default_attributes()
            .with_title(self.shell.title())
            .with_inner_size(PhysicalSize::new(width, height))
            .with_window_icon(icon);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(error) => {
                self.error = Some(failed("open the window", &error));
                event_loop.exit();
                return;
            }
        };
        let surface = softbuffer::Context::new(Rc::clone(&window))
            .and_then(|context| softbuffer::Surface::new(&context, Rc::clone(&window)));
        match surface {
            Ok(surface) => self.surface = Some(surface),
            Err(error) => {
                self.error = Some(failed("show pictures in the window", &error));
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(state) => {
                let state = state.state();
                self.modifiers = Modifiers {
                    alt: state.alt_key(),
                    ctrl: state.control_key(),
                    shift: state.shift_key(),
                    logo: state.super_key(),
                };
                self.events.push(ShellEvent::Modifiers(self.modifiers));
            }
            WindowEvent::CursorMoved { position, .. } => self.events.push(ShellEvent::Pointer {
                x: position.x as f32,
                y: position.y as f32,
            }),
            WindowEvent::CursorLeft { .. } => self.events.push(ShellEvent::PointerLeft),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Button::Primary,
                    MouseButton::Right => Button::Secondary,
                    MouseButton::Middle => Button::Middle,
                    _ => return,
                };
                self.events.push(ShellEvent::Button {
                    button,
                    pressed: state == ElementState::Pressed,
                    modifiers: self.modifiers,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 48.0, y * 48.0),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x as f32, position.y as f32)
                    }
                };
                self.events.push(ShellEvent::Scroll { x, y });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let Some(name) = key_name(&event.logical_key) {
                    self.events.push(ShellEvent::Key {
                        name,
                        pressed,
                        repeat: event.repeat,
                        modifiers: self.modifiers,
                    });
                }
                // Copy, cut and paste, as the system does them.
                if pressed
                    && self.modifiers.ctrl
                    && let Key::Character(text) = &event.logical_key
                {
                    match text.to_lowercase().as_str() {
                        "c" => self.events.push(ShellEvent::Copy),
                        "x" => self.events.push(ShellEvent::Cut),
                        "v" => {
                            if let Some(text) = self
                                .clipboard()
                                .and_then(|clipboard| clipboard.get_text().ok())
                            {
                                self.events.push(ShellEvent::Paste(text));
                            }
                        }
                        _ => {}
                    }
                }
                if pressed
                    && !self.modifiers.ctrl
                    && !self.modifiers.logo
                    && let Some(text) = &event.text
                    && text.chars().all(|c| !c.is_control())
                {
                    self.events.push(ShellEvent::Text(text.to_string()));
                }
            }
            WindowEvent::RedrawRequested => self.step(),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _: &ActiveEventLoop,
        _: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if let winit::event::DeviceEvent::MouseMotion { delta } = event {
            self.events.push(ShellEvent::MouseDelta {
                x: delta.0 as f32,
                y: delta.1 as f32,
            });
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.error.is_some() || self.shell.wants_exit() {
            event_loop.exit();
            return;
        }
        if let Some(limit) = self.options.exit_after_frames
            && self.report.frames >= limit
        {
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + TICK));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_get_egui_names() {
        assert_eq!(
            key_name(&Key::Named(NamedKey::ArrowLeft)).as_deref(),
            Some("ArrowLeft")
        );
        assert_eq!(key_name(&Key::Character("a".into())).as_deref(), Some("A"));
        assert_eq!(key_name(&Key::Named(NamedKey::Shift)), None);
    }
}
