//! Driving the editor without a person: a script of input steps, run frame
//! by frame, ending in a picture. It is how the editor is tested headless
//! and how `wen edit --headless-frame` takes its pictures.
//!
//! A script is a JSON object `{ "size": [1280, 720], "steps": [...] }`; a
//! step is `{ "events": [...] }` (one frame with these events),
//! `{ "idle": N }` (N frames without input) or `{ "seconds": S }` (the next
//! frame lasts S seconds, default 1/60). An event is one of:
//!
//! - `{ "pointer": [x, y] }`
//! - `{ "press": "primary" }` / `{ "release": "primary" }` (`secondary`, `middle`)
//! - `{ "click": [x, y] }`: move, press and release at a place
//! - `{ "key": "Z", "ctrl": true, "up": false }` (egui key names)
//! - `{ "text": "abc" }`
//! - `{ "scroll": [x, y] }`
//! - `{ "mouse_delta": [x, y] }`
//! - `{ "modifiers": { "ctrl": true } }`: these modifier keys are held from
//!   now on (`{ "modifiers": {} }` lets go)

use serde_json::Value;
use wish_core::{Diagnostic, Result};
use wish_window::{Button, Modifiers, ShellEvent};

use crate::EditorApp;
use crate::raster::Canvas;

/// The size used when a script gives none.
pub const DEFAULT_SIZE: (u32, u32) = (1280, 720);
/// The most frames one script may run (a typo should not run forever).
pub const MAX_FRAMES: u64 = 20_000;

fn bad(message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("invalid_editor_script", message, hint)
}

fn pair(value: &Value, what: &str) -> Result<(f32, f32)> {
    match value.as_array().map(Vec::as_slice) {
        Some([x, y]) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => Ok((x as f32, y as f32)),
            _ => Err(bad(
                format!("'{what}' needs two numbers, not {value}."),
                "Write it like [640, 360].",
            )),
        },
        _ => Err(bad(
            format!("'{what}' needs two numbers, not {value}."),
            "Write it like [640, 360].",
        )),
    }
}

fn button(value: &Value) -> Result<Button> {
    match value.as_str() {
        Some("primary") => Ok(Button::Primary),
        Some("secondary") => Ok(Button::Secondary),
        Some("middle") => Ok(Button::Middle),
        _ => Err(bad(
            format!("'{value}' is not a mouse button."),
            "Use \"primary\", \"secondary\" or \"middle\".",
        )),
    }
}

/// One event object as the events it stands for.
pub fn parse_event(event: &Value) -> Result<Vec<ShellEvent>> {
    let flag = |name: &str| event[name] == true;
    let modifiers = Modifiers {
        alt: flag("alt"),
        ctrl: flag("ctrl"),
        shift: flag("shift"),
        logo: false,
    };
    if let Some(at) = event.get("pointer") {
        let (x, y) = pair(at, "pointer")?;
        return Ok(vec![ShellEvent::Pointer { x, y }]);
    }
    if let Some(which) = event.get("press") {
        return Ok(vec![ShellEvent::Button {
            button: button(which)?,
            pressed: true,
            modifiers,
        }]);
    }
    if let Some(which) = event.get("release") {
        return Ok(vec![ShellEvent::Button {
            button: button(which)?,
            pressed: false,
            modifiers,
        }]);
    }
    if let Some(at) = event.get("click") {
        let (x, y) = pair(at, "click")?;
        return Ok(vec![
            ShellEvent::Pointer { x, y },
            ShellEvent::Button {
                button: Button::Primary,
                pressed: true,
                modifiers,
            },
            ShellEvent::Button {
                button: Button::Primary,
                pressed: false,
                modifiers,
            },
        ]);
    }
    if let Some(name) = event["key"].as_str() {
        return Ok(vec![ShellEvent::Key {
            name: name.to_owned(),
            pressed: !flag("up"),
            repeat: false,
            modifiers,
        }]);
    }
    if let Some(text) = event["text"].as_str() {
        return Ok(vec![ShellEvent::Text(text.to_owned())]);
    }
    if let Some(by) = event.get("scroll") {
        let (x, y) = pair(by, "scroll")?;
        return Ok(vec![ShellEvent::Scroll { x, y }]);
    }
    if let Some(held) = event.get("modifiers") {
        let on = |name: &str| held[name] == true;
        return Ok(vec![ShellEvent::Modifiers(Modifiers {
            alt: on("alt"),
            ctrl: on("ctrl"),
            shift: on("shift"),
            logo: false,
        })]);
    }
    if let Some(by) = event.get("mouse_delta") {
        let (x, y) = pair(by, "mouse_delta")?;
        return Ok(vec![ShellEvent::MouseDelta { x, y }]);
    }
    Err(bad(
        format!("The event {event} is not one the editor script knows."),
        "Use pointer, press, release, click, key, text, scroll or mouse_delta; see the editor script docs in wish_editor::script.",
    ))
}

/// What a script drives: the editor, or the launcher (welcome screen, then
/// the editor) that `wen edit` opens without a project.
pub trait Driven {
    /// Runs one frame with these events; returns whether it drew.
    fn frame_canvas(&mut self, size: (u32, u32), seconds: f64, events: Vec<ShellEvent>) -> bool;
    /// The last picture.
    fn canvas(&self) -> &Canvas;
}

impl Driven for EditorApp {
    fn frame_canvas(&mut self, size: (u32, u32), seconds: f64, events: Vec<ShellEvent>) -> bool {
        EditorApp::frame_canvas(self, size, seconds, events)
    }

    fn canvas(&self) -> &Canvas {
        EditorApp::canvas(self)
    }
}

/// What a script did.
pub struct Ran {
    /// The last picture.
    pub canvas: Canvas,
    /// Frames run.
    pub frames: u64,
}

/// Runs a script on the editor and returns the last picture.
pub fn run_script<D: Driven + ?Sized>(app: &mut D, script: &Value) -> Result<Ran> {
    let size = match script.get("size") {
        Some(size) => {
            let (w, h) = pair(size, "size")?;
            (w as u32, h as u32)
        }
        None => DEFAULT_SIZE,
    };
    if !(64..=4096).contains(&size.0) || !(64..=4096).contains(&size.1) {
        return Err(bad(
            format!("The size {}x{} is outside 64..4096.", size.0, size.1),
            "Give a size such as [1280, 720].",
        ));
    }
    let mut frames = 0u64;
    let mut seconds = 1.0 / 60.0;
    let mut run = |app: &mut D, events: Vec<ShellEvent>, seconds: f64| -> Result<()> {
        frames += 1;
        if frames > MAX_FRAMES {
            return Err(bad(
                format!("The script runs more than {MAX_FRAMES} frames."),
                "Cut the idle steps down.",
            ));
        }
        app.frame_canvas(size, seconds, events);
        Ok(())
    };
    // The first frames make the layout and the fonts; the script starts
    // from a settled window.
    for _ in 0..3 {
        run(app, Vec::new(), seconds)?;
    }
    for step in script["steps"].as_array().into_iter().flatten() {
        if let Some(events) = step.get("events") {
            let mut all = Vec::new();
            for event in events.as_array().into_iter().flatten() {
                all.extend(parse_event(event)?);
            }
            run(app, all, seconds)?;
            seconds = 1.0 / 60.0;
        } else if let Some(count) = step["idle"].as_u64() {
            for _ in 0..count {
                run(app, Vec::new(), seconds)?;
            }
        } else if let Some(next) = step["seconds"].as_f64() {
            seconds = next.clamp(0.0, 1.0);
        } else {
            return Err(bad(
                format!("The step {step} is not events, idle or seconds."),
                "A step is { \"events\": [...] }, { \"idle\": N } or { \"seconds\": S }.",
            ));
        }
    }
    // One more frame so that what the last step caused is drawn.
    run(app, Vec::new(), seconds)?;
    Ok(Ran {
        canvas: app.canvas().clone(),
        frames,
    })
}
