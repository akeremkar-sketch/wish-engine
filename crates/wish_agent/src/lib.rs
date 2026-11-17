//! The cockpit's agents (M8b WP5; ADR-024): one interface for every way of
//! bringing an AI into the editor, independent of who provides it.
//!
//! A task is started with an [`Adapter`]; it runs on its own thread and
//! reports [`Event`]s (what the agent says, the tools it calls, how it
//! ends). The agent works through the editor's own MCP endpoint, so every
//! change it makes is in the editor's one history and activity feed, under
//! the agent's name, live.
//!
//! Adapters:
//! - [`claude::ClaudeCode`]: Claude Code run as a subprocess on the
//!   person's own account (no API key, no commission: ADR-024).
//! - [`scripted::Scripted`]: a fixed list of tool calls, for tests and
//!   demonstrations (it talks MCP over HTTP like a real agent).

use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::Value;

pub mod claude;
pub mod mcp_client;
pub mod scripted;

/// A task for an agent.
#[derive(Clone, Debug)]
pub struct Task {
    /// What the person asked, in their words.
    pub request: String,
    /// What the person pointed at ("show and tell"): one line each, such as
    /// "the entity `crate` (Transform at 1, 0, 2)".
    pub context: Vec<String>,
    /// The editor's MCP endpoint (`http://127.0.0.1:<port>/mcp`).
    pub endpoint: String,
    /// The endpoint's token (`Authorization: Bearer`).
    pub token: String,
    /// The project folder (the agent's working folder).
    pub project: PathBuf,
    /// The scene the person is looking at.
    pub scene: String,
}

/// What happens while an agent works.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The agent says something.
    Said(String),
    /// It calls a tool.
    Called {
        /// The tool (without any `mcp__wish__` prefix).
        tool: String,
        /// Its arguments.
        args: Value,
    },
    /// A tool answered.
    Answered {
        /// The tool.
        tool: String,
        /// Whether it worked.
        ok: bool,
    },
    /// The task ended.
    Finished {
        /// Whether it ended well.
        ok: bool,
        /// The agent's last words.
        text: String,
    },
    /// The agent could not run (not installed, not signed in, crashed).
    Failed(String),
}

/// A task running on its own thread.
pub struct Run {
    events: mpsc::Receiver<Event>,
    stop: Box<dyn FnMut() + Send>,
    over: bool,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run")
            .field("over", &self.over)
            .finish_non_exhaustive()
    }
}

impl Run {
    /// A run that reports through `events` and is stopped by `stop`.
    pub fn new(events: mpsc::Receiver<Event>, stop: Box<dyn FnMut() + Send>) -> Self {
        Self {
            events,
            stop,
            over: false,
        }
    }

    /// What happened since the last look (never blocks).
    pub fn poll(&mut self) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    if matches!(event, Event::Finished { .. } | Event::Failed(_)) {
                        self.over = true;
                    }
                    seen.push(event);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.over {
                        self.over = true;
                        seen.push(Event::Failed("the agent stopped without a word".to_owned()));
                    }
                    break;
                }
            }
        }
        seen
    }

    /// Whether the task is over.
    pub fn is_over(&self) -> bool {
        self.over
    }

    /// Stops the task (what was done stays: it is in the history).
    pub fn stop(&mut self) {
        (self.stop)();
    }
}

/// A way of running an agent.
pub trait Adapter: Send {
    /// Its name, for people ("Claude Code").
    fn name(&self) -> &str;
    /// Starts `task`.
    fn start(&self, task: &Task) -> Result<Run, String>;
}

/// What the agent is told besides the person's words: where it works, how
/// to be seen working, what the person pointed at.
pub fn instructions(task: &Task) -> String {
    let mut text = String::from(
        "You are working inside the Wish Engine editor, through its MCP tools (wish). The person watches the scene change live while you work, and every change you make appears as a card in their activity feed, where they can take it back. Give every edit_apply a short \"reason\" (one sentence, the person reads it on the card). Prefer small, separate edits over one huge one. Check your work with describe_view or capture_frame. If the project's permission mode is \"suggest\", your changes wait for the person: say what you proposed.\n",
    );
    text.push_str(&format!(
        "The person is looking at the scene {}.\n",
        task.scene
    ));
    if !task.context.is_empty() {
        text.push_str("They pointed at:\n");
        for item in &task.context {
            text.push_str(&format!("- {item}\n"));
        }
    }
    text.push_str("\nTheir request:\n");
    text.push_str(&task.request);
    text
}
