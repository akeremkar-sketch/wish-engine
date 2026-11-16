//! `wen play`: the project's game in a window, in real time.
//!
//! The window is the same one `wen view` uses; here its content is a play
//! session of the control plane (the one `sim_start` makes), stepped at 60
//! ticks a second with the keys that are held as the game's actions.
//! Saving `scripts/src/lib.rs` builds the new code and puts it into the
//! running session (`script_build`), so the game goes on where it was.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::json;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{Gpu, RenderScene, Renderer, View};
use wish_script::build::SCRIPTS_ENTRY;
use wish_window::{Content, Loaded, Pointing, ViewerOptions, fingerprint_dir};

use crate::args::PlayArgs;
use crate::capture::load_models;

/// Ticks per second the game runs at.
const TICK_RATE: f64 = 60.0;
/// The most ticks one picture may catch up on (a stall must not make the
/// game race to make up the time).
const MAX_CATCH_UP: u64 = 5;

struct Game {
    /// The sound card, where there is one to open (Windows for now).
    #[cfg(windows)]
    speaker: Option<wish_audio::output::Speaker>,
    workspace: Workspace,
    root: PathBuf,
    scene: Option<String>,
    session: Option<String>,
    /// Real time owed to the game, in ticks.
    owed: f64,
    /// What the game reported while it ran (a system that failed).
    messages: Vec<String>,
}

impl Game {
    /// Makes the speakers match the sounds the game says are playing.
    #[cfg(windows)]
    fn hear(&mut self, id: &str) {
        let Some(speaker) = self.speaker.as_mut() else {
            return;
        };
        if let Ok(voices) = self.workspace.play_voices(id) {
            let root = self.root.clone();
            speaker.sync(&voices, &|path| std::fs::read(root.join(path)).ok());
        }
    }

    #[cfg(not(windows))]
    fn hear(&mut self, _id: &str) {}

    fn run_tool(&mut self, tool: &str, args: &serde_json::Value) -> serde_json::Value {
        self.workspace.call(tool, args, &parse_actor("human"))
    }

    /// Builds the game code (if the project has any) and reports what the
    /// compiler said.
    fn build(&mut self) -> Vec<Diagnostic> {
        if !self.root.join(SCRIPTS_ENTRY).is_file() {
            return Vec::new();
        }
        let answer = self.run_tool("script_build", &json!({}));
        if answer["ok"] != true {
            return serde_json::from_value::<Diagnostic>(answer["error"].clone())
                .into_iter()
                .collect();
        }
        answer["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| serde_json::from_value::<Diagnostic>(d.clone()).ok())
            .collect()
    }

    fn start(&mut self) -> Result<String> {
        let mut args = json!({});
        if let Some(scene) = &self.scene {
            args["scene"] = json!(scene);
        }
        let answer = self.run_tool("sim_start", &args);
        if answer["ok"] != true {
            return Err(
                serde_json::from_value::<Diagnostic>(answer["error"].clone()).unwrap_or_else(
                    |_| {
                        Diagnostic::error(
                            "internal_error",
                            "The game could not be started.",
                            "Run `wen check` to see what is wrong with the project.",
                        )
                    },
                ),
            );
        }
        let id = answer["session"].as_str().unwrap_or("s1").to_owned();
        self.session = Some(id.clone());
        Ok(id)
    }

    fn scene_now(&mut self) -> Result<RenderScene> {
        let id = self.session.clone().unwrap_or_default();
        self.workspace.play_scene(&id)
    }
}

impl Content for Game {
    fn load(&mut self, gpu: &Gpu, renderer: &mut Renderer) -> Result<Loaded> {
        let mut warnings = self.build();
        if warnings.iter().any(Diagnostic::is_error) && self.session.is_none() {
            // Nothing runs yet and the code does not compile: say so.
            return Err(warnings.remove(0));
        }
        if self.session.is_none() {
            self.start()?;
        }
        let scene = self.scene_now()?;
        warnings.extend(load_models(&scene, &self.root, gpu, renderer));
        Ok(Loaded { scene, warnings })
    }

    fn name_of(&mut self, entity: Entity) -> String {
        entity.to_string()
    }

    fn fingerprint(&mut self) -> u64 {
        // The game code: a saved change builds and reloads. Scene files are
        // read when the game starts.
        fingerprint_dir(&self.root.join("scripts"), &["rs"])
    }

    fn live(&self) -> bool {
        true
    }

    fn play(
        &mut self,
        keys: &BTreeSet<String>,
        pointing: Option<Pointing>,
        seconds: f64,
    ) -> Option<Result<RenderScene>> {
        let id = self.session.clone()?;
        self.owed += seconds * TICK_RATE;
        let whole = self.owed.floor();
        self.owed -= whole;
        let ticks = (whole as u64).min(MAX_CATCH_UP);
        if ticks > 0 {
            let pointer = pointing.map(|at| wish_control::PointerAt {
                x: at.x,
                y: at.y,
                width: at.width as f32,
                height: at.height as f32,
            });
            let stepped = self.workspace.play_step(&id, ticks, keys, pointer);
            self.hear(&id);
            match stepped {
                Ok(problems) => {
                    for problem in problems {
                        self.messages.push(format!("{problem}"));
                    }
                }
                Err(problem) => return Some(Err(problem)),
            }
        }
        Some(self.scene_now())
    }
}

/// Runs `wen play`.
pub fn play(args: &PlayArgs, as_json: bool) -> Result<String> {
    let root = Path::new(args.root.as_deref().unwrap_or(".")).to_path_buf();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: root.clone(),
        example: None,
    })?;
    workspace.set_software(args.software);
    #[cfg(windows)]
    let speaker = match wish_audio::output::Speaker::open() {
        Ok(speaker) => Some(speaker),
        Err(problem) => {
            if !as_json {
                eprintln!("{problem}");
            }
            None
        }
    };
    let mut game = Game {
        #[cfg(windows)]
        speaker,
        workspace,
        root,
        scene: args.scene.clone(),
        session: None,
        owed: 0.0,
        messages: Vec::new(),
    };
    let report = wish_window::run(
        ViewerOptions {
            title: "wen play".to_owned(),
            size: args.size,
            software: args.software,
            shaders: None,
            view: View::Lit,
            exit_after_frames: args.exit_after,
            on_message: Box::new(move |message| {
                if !as_json {
                    eprintln!("{message}");
                }
            }),
        },
        &mut game,
    )?;
    for message in &game.messages {
        if !as_json {
            eprintln!("{message}");
        }
    }
    Ok(if as_json {
        json!({
            "ok": true,
            "frames": report.frames,
            "code_reloads": report.scene_reloads,
            "rejected": report.rejected,
        })
        .to_string()
    } else {
        format!(
            "Closed after {} frame(s); {} code reload(s), {} rejected.",
            report.frames, report.scene_reloads, report.rejected
        )
    })
}
