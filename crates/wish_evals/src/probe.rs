//! Looking at a finished project with the engine's own tools, for checks.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

/// A check's verdict: `Err` says what is wrong, for the report.
pub type Verdict = Result<(), String>;

/// One entity as `describe_view` saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub percent: f64,
    pub pixels: f64,
    /// [left, top, right, bottom] in pixels.
    pub bbox: [f64; 4],
}

/// The picture `describe_view` measures in.
pub const VIEW_W: f64 = 640.0;
pub const VIEW_H: f64 = 360.0;

/// The engine, opened on a project, as the checker.
pub struct Probe {
    ws: Workspace,
}

impl Probe {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let mut ws = Workspace::open(WorkspaceOptions {
            root: dir.to_path_buf(),
            example: None,
        })
        .map_err(|problem| problem.message.clone())?;
        ws.set_software(true);
        Ok(Self { ws })
    }

    /// The project folder.
    pub fn root(&self) -> std::path::PathBuf {
        self.ws.root().to_path_buf()
    }

    /// Calls a tool as the checker; a failed call is an `Err` with its message.
    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        let answer = self.ws.call(tool, &args, &parse_actor("checker"));
        if answer["ok"] == true {
            Ok(answer)
        } else {
            Err(format!("{tool} failed: {}", answer["error"]["message"]))
        }
    }

    /// The ids matching a condition in a scene (default scene when `None`).
    pub fn query(&mut self, scene: Option<&str>, condition: &str) -> Result<Vec<String>, String> {
        let mut args = json!({ "where": condition, "limit": 1000 });
        if let Some(scene) = scene {
            args["scene"] = json!(scene);
        }
        let answer = self.call("entity_query", args)?;
        Ok(answer["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| e["id"].as_str().map(str::to_owned))
            .collect())
    }

    /// One entity's effective components.
    pub fn components(&mut self, scene: Option<&str>, id: &str) -> Result<Value, String> {
        let mut args = json!({ "ids": [id] });
        if let Some(scene) = scene {
            args["scene"] = json!(scene);
        }
        let answer = self.call("entity_get", args)?;
        Ok(answer["entities"][0]["components"].clone())
    }

    /// One entity as `entity_get` gives it (with notes, parent, prefab).
    pub fn entity(&mut self, scene: Option<&str>, id: &str) -> Result<Value, String> {
        let mut args = json!({ "ids": [id] });
        if let Some(scene) = scene {
            args["scene"] = json!(scene);
        }
        Ok(self.call("entity_get", args)?["entities"][0].clone())
    }

    /// The world matrix (column-major, 16 numbers) of an entity.
    pub fn matrix(&mut self, scene: Option<&str>, id: &str) -> Result<[f64; 16], String> {
        let mut args = json!({ "ids": [id], "components": ["GlobalTransform"] });
        if let Some(scene) = scene {
            args["scene"] = json!(scene);
        }
        let answer = self.call("entity_get", args)?;
        let values = answer["entities"][0]["components"]["GlobalTransform"]["matrix"]
            .as_array()
            .ok_or_else(|| format!("'{id}' has no world placement (no Transform)"))?;
        let mut matrix = [0.0; 16];
        for (slot, value) in matrix.iter_mut().zip(values) {
            *slot = value.as_f64().unwrap_or(f64::NAN);
        }
        Ok(matrix)
    }

    /// The world position of an entity.
    pub fn position(&mut self, scene: Option<&str>, id: &str) -> Result<[f64; 3], String> {
        let m = self.matrix(scene, id)?;
        Ok([m[12], m[13], m[14]])
    }

    /// What the scene's first camera sees.
    pub fn visible(&mut self, scene: Option<&str>) -> Result<BTreeMap<String, Seen>, String> {
        let mut args = json!({ "width": VIEW_W, "height": VIEW_H, "limit": 500 });
        if let Some(scene) = scene {
            args["scene"] = json!(scene);
        }
        let answer = self.call("describe_view", args)?;
        let mut seen = BTreeMap::new();
        for entry in answer["visible"].as_array().into_iter().flatten() {
            let bbox = &entry["bounding_box"];
            let at = |i: usize| bbox[i].as_f64().unwrap_or(0.0);
            seen.insert(
                entry["entity"].as_str().unwrap_or_default().to_owned(),
                Seen {
                    percent: entry["visible_percent"].as_f64().unwrap_or(0.0),
                    pixels: entry["pixels"].as_f64().unwrap_or(0.0),
                    bbox: [at(0), at(1), at(2), at(3)],
                },
            );
        }
        Ok(seen)
    }

    /// Whether `check` passes.
    pub fn check_passes(&mut self) -> Verdict {
        let answer = self.call("check", json!({}))?;
        if answer["passed"] == true {
            Ok(())
        } else {
            Err(format!("check fails: {}", answer["diagnostics"]))
        }
    }
}

/// Requires `id` to be at least `percent` visible.
pub fn require_visible(seen: &BTreeMap<String, Seen>, id: &str, percent: f64) -> Verdict {
    match seen.get(id) {
        Some(s) if s.percent >= percent => Ok(()),
        Some(s) => Err(format!(
            "'{id}' is only {:.0}% visible (needs {percent}%)",
            s.percent
        )),
        None => Err(format!("'{id}' is not visible from the camera")),
    }
}

/// The y of the bottom of an axis-aligned cube or box (local -0.5..0.5) from
/// its world matrix: the matrix's y column length is the height.
pub fn cube_bottom(m: &[f64; 16]) -> f64 {
    let height = (m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt();
    m[13] - height / 2.0
}
