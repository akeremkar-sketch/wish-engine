//! The project file, `wish.json`: what a folder needs to be a project.
//!
//! ```json
//! {
//!   "engine": "0.1.0",
//!   "format": 1,
//!   "name": "My game",
//!   "scene": "scenes/main.scene.json",
//!   "types": { "example": "stress" }
//! }
//! ```
//!
//! `types.example` names the built-in example whose component types the
//! project's scenes use, until game modules bring a project's own types
//! (M5). Like every engine file it is canonical JSON.

use std::path::Path;

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Result};

/// The project file's name.
pub const PROJECT_FILE: &str = "wish.json";
/// The format version this engine writes and reads.
pub const PROJECT_FORMAT: u64 = 1;

const KEYS: &[&str] = &[
    "engine", "format", "kind", "name", "net", "packages", "scene", "types",
];
const TYPE_KEYS: &[&str] = &["example"];

/// What a project is mostly made of (release 1.1): a flat game of sprites
/// or a game in space. The kind sets defaults (the editor's Create menu,
/// a new scene's camera, a new project's templates); it never limits what
/// a project can hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectKind {
    /// Sprites and an orthographic camera.
    D2,
    /// Models and a perspective camera. What a project without a `kind`
    /// is: every project before release 1.1.
    #[default]
    D3,
}

impl ProjectKind {
    /// The spellings `wish.json` and `wen new --kind` take.
    pub const NAMES: &'static [&'static str] = &["2d", "3d"];

    /// The kind spelled `name` (`2d`, `3d`; case does not matter).
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "2d" => Some(Self::D2),
            "3d" => Some(Self::D3),
            _ => None,
        }
    }

    /// Its spelling in `wish.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::D2 => "2d",
            Self::D3 => "3d",
        }
    }
}

/// What `wish.json` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectConfig {
    /// The project's name, for people.
    pub name: String,
    /// The engine version that wrote the file.
    pub engine: String,
    /// The scene tools use when none is named (project-relative).
    pub scene: String,
    /// The built-in example whose types the scenes use, if any.
    pub example: Option<String>,
    /// The project's packages (M10): id → version requirement. Written
    /// only when there is one, so older projects stay byte-identical.
    pub packages: std::collections::BTreeMap<String, String>,
    /// The project's kind, when the file says one. Written only when set,
    /// so projects from before release 1.1 stay byte-identical; read it
    /// with [`ProjectConfig::kind`], which answers `3d` without it.
    pub kind: Option<ProjectKind>,
    /// Online play's settings (release 1.1), as the file holds them (checked
    /// when the file is read): which components are replicated and how the
    /// lobby is set up. Written only when set, so older projects stay
    /// byte-identical; read them with [`ProjectConfig::net`].
    pub net: Option<Value>,
}

impl ProjectConfig {
    /// Online play's settings: what the file says, else the defaults.
    pub fn net(&self) -> NetSettings {
        self.net
            .as_ref()
            .and_then(|value| NetSettings::from_value(value).ok())
            .unwrap_or_default()
    }

    /// The project's kind: what the file says, else `3d`. The one place
    /// that answers the question (the editor's Create menu and new scenes
    /// ask it; the 2D mode will).
    pub fn kind(&self) -> ProjectKind {
        self.kind.unwrap_or_default()
    }

    /// A new project's configuration.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            engine: env!("CARGO_PKG_VERSION").to_owned(),
            scene: "scenes/main.scene.json".to_owned(),
            example: None,
            packages: std::collections::BTreeMap::new(),
            kind: None,
            net: None,
        }
    }

    /// The file's JSON form.
    pub fn to_value(&self) -> Value {
        let mut types = Map::new();
        if let Some(example) = &self.example {
            types.insert("example".to_owned(), Value::String(example.clone()));
        }
        let mut object = Map::new();
        object.insert("engine".to_owned(), Value::String(self.engine.clone()));
        object.insert("format".to_owned(), Value::from(PROJECT_FORMAT));
        object.insert("name".to_owned(), Value::String(self.name.clone()));
        if let Some(net) = &self.net {
            object.insert("net".to_owned(), net.clone());
        }
        if let Some(kind) = self.kind {
            object.insert("kind".to_owned(), Value::String(kind.name().to_owned()));
        }
        if !self.packages.is_empty() {
            object.insert(
                "packages".to_owned(),
                Value::Object(
                    self.packages
                        .iter()
                        .map(|(id, req)| (id.clone(), Value::String(req.clone())))
                        .collect(),
                ),
            );
        }
        object.insert("scene".to_owned(), Value::String(self.scene.clone()));
        object.insert("types".to_owned(), Value::Object(types));
        Value::Object(object)
    }

    /// The file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        wish_scene::json::to_canonical(&self.to_value())
    }
}

fn problem(
    node: &wish_scene::json::Node,
    pointer: &str,
    message: String,
    hint: &str,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::error("invalid_project_file", message, hint)
        .at_file(PROJECT_FILE)
        .at_path(pointer);
    if let Some(found) = node.locate(pointer) {
        let span = found.key.unwrap_or(found.value);
        diagnostic = diagnostic.at_line(span.line, Some(span.column));
    }
    diagnostic
}

fn unknown_keys(
    node: &wish_scene::json::Node,
    object: &Map<String, Value>,
    prefix: &str,
    keys: &[&str],
) -> Result<()> {
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(problem(
                node,
                &format!("{prefix}/{key}"),
                format!("'{PROJECT_FILE}' has no setting '{key}'."),
                &wish_core::suggest::unknown_name_hint(key, keys),
            )
            .with_candidates(keys.iter().copied()));
        }
    }
    Ok(())
}

fn text(
    node: &wish_scene::json::Node,
    object: &Map<String, Value>,
    key: &str,
    hint: &str,
) -> Result<String> {
    match object.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(_) => Err(problem(
            node,
            &format!("/{key}"),
            format!("'{key}' in '{PROJECT_FILE}' must be a non-empty string."),
            hint,
        )),
        None => Err(problem(
            node,
            "",
            format!("'{PROJECT_FILE}' has no '{key}'."),
            hint,
        )),
    }
}

/// A warning when `engine` in `wish.json` (its text, `source`) is there but
/// is not a version: the project opens and plays, but no engine can be
/// chosen for it, so the running one opens it with no word (ADR-027,
/// rule 3). `None` when the key is a version, is missing, or the file
/// cannot be read as JSON (its own error says that).
pub fn engine_problem(source: &str) -> Option<Diagnostic> {
    let node = wish_scene::json::parse(source, PROJECT_FILE).ok()?;
    let value = node.to_value();
    let engine = value.get("engine")?;
    if engine
        .as_str()
        .is_some_and(|text| crate::packages::Version::parse(text).is_ok())
    {
        return None;
    }
    let shown = engine
        .as_str()
        .map_or_else(|| engine.to_string(), |text| format!("'{text}'"));
    let mut diagnostic = Diagnostic::warning(
        "invalid_version",
        format!(
            "'engine' in '{PROJECT_FILE}' is {shown}, which is not a version; no engine can be chosen for the project, so whichever one opens it does."
        ),
        format!(
            "Write the version of the engine that made the project: three numbers, such as \"{}\" (this engine's).",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .at_file(PROJECT_FILE)
    .at_path("/engine");
    if let Some(found) = node.locate("/engine") {
        let span = found.value;
        diagnostic = diagnostic.at_line(span.line, Some(span.column));
    }
    Some(diagnostic)
}

/// Reads `source` (the text of `wish.json`).
pub fn parse_project(source: &str) -> Result<ProjectConfig> {
    let node = wish_scene::json::parse(source, PROJECT_FILE)?;
    let value = node.to_value();
    let Some(object) = value.as_object() else {
        return Err(problem(
            &node,
            "",
            format!("'{PROJECT_FILE}' must be a JSON object."),
            "Start it with { \"format\": 1, \"name\": \"...\", \"scene\": \"scenes/main.scene.json\" }, or run `wen new` to write one.",
        ));
    };
    unknown_keys(&node, object, "", KEYS)?;
    match object.get("format").and_then(Value::as_u64) {
        Some(PROJECT_FORMAT) => {}
        Some(other) => {
            return Err(problem(
                &node,
                "/format",
                format!(
                    "'{PROJECT_FILE}' has format {other}; this engine reads format {PROJECT_FORMAT}."
                ),
                "Open the project with the engine version that wrote it, or run `wen migrate`.",
            ));
        }
        None => {
            return Err(problem(
                &node,
                "",
                format!("'{PROJECT_FILE}' needs \"format\": {PROJECT_FORMAT}."),
                "Add \"format\": 1.",
            ));
        }
    }
    let name = text(&node, object, "name", "Add \"name\": \"My game\".")?;
    let scene = text(
        &node,
        object,
        "scene",
        "Add \"scene\": \"scenes/main.scene.json\": the scene tools use when none is named.",
    )?;
    if !scene.ends_with(".scene.json") {
        return Err(problem(
            &node,
            "/scene",
            format!("'scene' must name a .scene.json file, but is '{scene}'."),
            "Write a project-relative path such as \"scenes/main.scene.json\".",
        ));
    }
    let engine = object
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_owned();
    let mut example = None;
    match object.get("types") {
        None => {}
        Some(Value::Object(types)) => {
            unknown_keys(&node, types, "/types", TYPE_KEYS)?;
            match types.get("example") {
                None => {}
                Some(Value::String(name)) if wish_examples::NAMES.contains(&name.as_str()) => {
                    example = Some(name.clone());
                }
                Some(other) => {
                    let given = other.as_str().unwrap_or("");
                    return Err(problem(
                        &node,
                        "/types/example",
                        format!("'types.example' must name a built-in example, but is {other}."),
                        &wish_core::suggest::unknown_name_hint(given, wish_examples::NAMES),
                    )
                    .with_candidates(wish_examples::NAMES.iter().copied()));
                }
            }
        }
        Some(_) => {
            return Err(problem(
                &node,
                "/types",
                "'types' must be an object.".to_owned(),
                "Write \"types\": { \"example\": \"stress\" }, or {} for the engine's types only.",
            ));
        }
    }
    let mut packages = std::collections::BTreeMap::new();
    match object.get("packages") {
        None => {}
        Some(Value::Object(listed)) => {
            for (id, req) in listed {
                let pointer = format!("/packages/{id}");
                if !crate::packages::valid_id(id) {
                    return Err(problem(
                        &node,
                        &pointer,
                        format!("'{id}' is not a package id."),
                        "An id is lowercase parts of a-z, 0-9 and -, joined with dots, such as \"com.example.dusk\".",
                    ));
                }
                let Some(text) = req.as_str() else {
                    return Err(problem(
                        &node,
                        &pointer,
                        format!("The version of '{id}' must be a text."),
                        "Write it like \"com.example.dusk\": \"^1.0.0\".",
                    ));
                };
                if crate::packages::VersionReq::parse(text).is_err() {
                    return Err(problem(
                        &node,
                        &pointer,
                        format!("'{text}' is not a version requirement."),
                        "Write it like \"^1.0.0\" or \">=1.0, <2.0\".",
                    ));
                }
                packages.insert(id.clone(), text.to_owned());
            }
        }
        Some(_) => {
            return Err(problem(
                &node,
                "/packages",
                "'packages' must be an object.".to_owned(),
                "Write it like \"packages\": { \"com.example.dusk\": \"^1.0.0\" }; the packages tool keeps it for you.",
            ));
        }
    }
    let kind = match object.get("kind") {
        None => None,
        Some(Value::String(given)) => match ProjectKind::parse(given) {
            Some(kind) => Some(kind),
            None => {
                return Err(problem(
                    &node,
                    "/kind",
                    format!("'kind' in '{PROJECT_FILE}' must be \"2d\" or \"3d\", but is '{given}'."),
                    &format!(
                        "{} Leave 'kind' out to open the project as a 3D project (what every project from before release 1.1 is).",
                        wish_core::suggest::unknown_name_hint(given, ProjectKind::NAMES)
                    ),
                )
                .with_candidates(ProjectKind::NAMES.iter().copied()));
            }
        },
        Some(other) => {
            return Err(problem(
                &node,
                "/kind",
                format!(
                    "'kind' in '{PROJECT_FILE}' must be a text, \"2d\" or \"3d\", but is {other}."
                ),
                "Write \"kind\": \"2d\" for a flat game of sprites, \"3d\" for a game in space, or leave it out (3D).",
            ));
        }
    };
    let net = match object.get("net") {
        None => None,
        Some(value) => {
            NetSettings::from_value(value).map_err(|(pointer, message, hint)| {
                problem(&node, &format!("/net{pointer}"), message, &hint)
            })?;
            Some(value.clone())
        }
    };
    Ok(ProjectConfig {
        name,
        engine,
        scene,
        example,
        packages,
        kind,
        net,
    })
}

/// Online play's settings in `wish.json` (`net`, release 1.1; design note
/// `docs/design/1.1-net-tool.md` §4): what a host of the project replicates
/// and how its lobby is set up.
///
/// ```json
/// "net": {
///   "replicate": ["Transform", "Avatar", "NetOwner"],
///   "budget_bytes": 1100,
///   "lobby": { "min_players": 4, "roles": [{ "name": "crew" }, { "name": "saboteur", "count": 2 }] }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetSettings {
    /// The replicated component types, by name.
    pub replicate: Vec<String>,
    /// The largest state message per player per tick, in bytes.
    pub budget_bytes: Option<usize>,
    /// Players seated before the host may start.
    pub min_players: Option<u32>,
    /// Colours of the game's palette.
    pub colors: Option<u32>,
    /// Ticks a player who dropped keeps its seat.
    pub rejoin_grace_ticks: Option<u64>,
    /// The largest message a player may send, in bytes.
    pub message_bytes: Option<usize>,
    /// Messages a player may send per second.
    pub messages_per_second: Option<u32>,
    /// The roles: a name and how many players get it (the first is
    /// everyone else's).
    pub roles: Vec<(String, u32)>,
}

impl Default for NetSettings {
    fn default() -> Self {
        Self {
            replicate: vec!["Transform".to_owned(), "NetOwner".to_owned()],
            budget_bytes: None,
            min_players: None,
            colors: None,
            rejoin_grace_ticks: None,
            message_bytes: None,
            messages_per_second: None,
            roles: Vec::new(),
        }
    }
}

/// A problem in `net`: where (a pointer below `/net`), what, and the hint.
pub type NetProblem = (String, String, String);

const NET_KEYS: &[&str] = &["budget_bytes", "lobby", "replicate"];
const LOBBY_KEYS: &[&str] = &[
    "colors",
    "message_bytes",
    "messages_per_second",
    "min_players",
    "rejoin_grace_ticks",
    "roles",
];

fn unknown_net_key(object: &Map<String, Value>, prefix: &str, keys: &[&str]) -> Option<NetProblem> {
    object
        .keys()
        .find(|key| !keys.contains(&key.as_str()))
        .map(|key| {
            (
                format!("{prefix}/{key}"),
                format!("'net' in '{PROJECT_FILE}' has no setting '{key}'."),
                wish_core::suggest::unknown_name_hint(key, keys),
            )
        })
}

fn whole(value: &Value, min: u64, max: u64) -> Option<u64> {
    value.as_u64().filter(|n| (min..=max).contains(n))
}

/// A whole number setting of `lobby`, in its range.
fn lobby_number(
    lobby: &Map<String, Value>,
    key: &str,
    (min, max): (u64, u64),
    hint: &str,
) -> std::result::Result<Option<u64>, NetProblem> {
    match lobby.get(key) {
        None => Ok(None),
        Some(value) => whole(value, min, max).map(Some).ok_or_else(|| {
            (
                format!("/lobby/{key}"),
                format!("'{key}' is {value}, outside {min} to {max}."),
                hint.to_owned(),
            )
        }),
    }
}

impl NetSettings {
    /// Reads `net`; a problem names where it is below `/net`.
    pub fn from_value(value: &Value) -> std::result::Result<Self, NetProblem> {
        let bad = |pointer: &str, message: &str, hint: &str| {
            Err((pointer.to_owned(), message.to_owned(), hint.to_owned()))
        };
        let Some(object) = value.as_object() else {
            return bad(
                "",
                "'net' must be an object.",
                "Write it like \"net\": { \"replicate\": [\"Transform\", \"NetOwner\"] }.",
            );
        };
        if let Some(found) = unknown_net_key(object, "", NET_KEYS) {
            return Err(found);
        }
        let mut settings = Self::default();
        if let Some(listed) = object.get("replicate") {
            let names: Option<Vec<String>> = listed.as_array().and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
                    .collect()
            });
            match names {
                Some(names) if names.len() <= 256 => settings.replicate = names,
                _ => {
                    return bad(
                        "/replicate",
                        "'replicate' must be a list of component names.",
                        "Name the component types a host sends for entities with Replicated, like [\"Transform\", \"NetOwner\", \"Avatar\"] (at most 256).",
                    );
                }
            }
        }
        if let Some(budget) = object.get("budget_bytes") {
            match whole(budget, 64, 1 << 20) {
                Some(n) => settings.budget_bytes = usize::try_from(n).ok(),
                None => {
                    return bad(
                        "/budget_bytes",
                        &format!("'budget_bytes' is {budget}."),
                        "Give each player 64 bytes to 1 MiB of state per tick (the default, 1024, fits one datagram).",
                    );
                }
            }
        }
        let Some(lobby) = object.get("lobby") else {
            return Ok(settings);
        };
        let Some(lobby) = lobby.as_object() else {
            return bad(
                "/lobby",
                "'lobby' must be an object.",
                "Write it like \"lobby\": { \"min_players\": 4 }.",
            );
        };
        if let Some(found) = unknown_net_key(lobby, "/lobby", LOBBY_KEYS) {
            return Err(found);
        }
        settings.min_players = lobby_number(
            lobby,
            "min_players",
            (1, 16),
            "Ask for 1 to 16 players before the host may start.",
        )?
        .and_then(|n| u32::try_from(n).ok());
        settings.colors = lobby_number(
            lobby,
            "colors",
            (1, 64),
            "Give the game's palette 1 to 64 colours.",
        )?
        .and_then(|n| u32::try_from(n).ok());
        settings.rejoin_grace_ticks = lobby_number(
            lobby,
            "rejoin_grace_ticks",
            (0, 1_000_000),
            "Keep a dropped player's seat 0 to 1,000,000 ticks (60 a second).",
        )?;
        settings.message_bytes = lobby_number(
            lobby,
            "message_bytes",
            (16, 60 * 1024),
            "Let a player's message be 16 bytes to 60 KiB.",
        )?
        .and_then(|n| usize::try_from(n).ok());
        settings.messages_per_second = lobby_number(
            lobby,
            "messages_per_second",
            (1, 1000),
            "Let a player send 1 to 1000 messages a second.",
        )?
        .and_then(|n| u32::try_from(n).ok());
        if let Some(roles) = lobby.get("roles") {
            let hint = "List the roles like [{ \"name\": \"crew\" }, { \"name\": \"saboteur\", \"count\": 2 }]: the first is everyone else's, each later one goes to count players (at most 16 roles).";
            let Some(items) = roles.as_array().filter(|items| items.len() <= 16) else {
                return bad("/lobby/roles", "'roles' must be a list.", hint);
            };
            for (index, item) in items.iter().enumerate() {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|n| !n.trim().is_empty() && n.len() <= 64);
                let count = match item.get("count") {
                    None => Some(0),
                    Some(v) => whole(v, 0, 16).and_then(|n| u32::try_from(n).ok()),
                };
                let extra = item
                    .as_object()
                    .and_then(|o| o.keys().find(|k| *k != "name" && *k != "count"));
                match (name, count, extra) {
                    (Some(name), Some(count), None) => {
                        settings.roles.push((name.to_owned(), count))
                    }
                    _ => {
                        return bad(
                            &format!("/lobby/roles/{index}"),
                            &format!("Role {index} is not a name and a count."),
                            hint,
                        );
                    }
                }
            }
        }
        Ok(settings)
    }
}

/// Reads `root/wish.json`; `Ok(None)` if the folder has none.
pub fn read_project(root: &Path) -> Result<Option<ProjectConfig>> {
    let path = root.join(PROJECT_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{PROJECT_FILE}': {error}."),
            "Check that the file can be read and is UTF-8 text.",
        )
        .at_file(PROJECT_FILE)
    })?;
    parse_project(&source).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_file_reads_back_the_same() {
        let mut config = ProjectConfig::new("Test");
        config.example = Some("stress".to_owned());
        let text = config.to_canonical().unwrap();
        assert_eq!(parse_project(&text).unwrap(), config);
        assert!(wish_scene::json::is_canonical(&text));
    }

    #[test]
    fn mistakes_point_at_the_setting_with_a_hint() {
        let cases = [
            ("[]", ""),
            (
                "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.scene.json\", \"sceen\": 1 }",
                "/sceen",
            ),
            (
                "{ \"format\": 2, \"name\": \"x\", \"scene\": \"a.scene.json\" }",
                "/format",
            ),
            (
                "{ \"format\": 1, \"name\": \"\", \"scene\": \"a.scene.json\" }",
                "/name",
            ),
            (
                "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.json\" }",
                "/scene",
            ),
            (
                "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.scene.json\", \"types\": { \"example\": \"bonce\" } }",
                "/types/example",
            ),
        ];
        for (source, pointer) in cases {
            let problem = parse_project(source).unwrap_err();
            assert_eq!(problem.code, "invalid_project_file", "{source}");
            assert_eq!(problem.location.path.as_deref(), Some(pointer), "{source}");
            assert!(!problem.hint.is_empty());
        }
        let problem = parse_project(
            "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.scene.json\", \"types\": { \"example\": \"bonce\" } }",
        )
        .unwrap_err();
        assert_eq!(problem.hint, "Did you mean 'bounce'?");
        assert_eq!(problem.location.line, Some(1));
        assert_eq!(parse_project("{").unwrap_err().code, "json_syntax");
    }
}
