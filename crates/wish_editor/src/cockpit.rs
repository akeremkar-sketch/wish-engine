//! The cockpit (M8b WP5): the agent's panel beside the view. The person
//! writes a request (pointing at things in the scene for context), the
//! agent works through the editor's own MCP endpoint (so its changes land
//! in the one history and the activity feed, live), and the transcript
//! shows what it says and does. A checkpoint is saved before every task.

use std::cell::RefCell;

use crate::i18n::{t, tf};
use egui::{Color32, RichText, Ui};
use wish_agent::{Adapter, Event, Run};
use wish_mcp::http::Endpoint;

/// Who a line of the transcript is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Who {
    /// The person's request.
    Person,
    /// What the agent says.
    Agent,
    /// A tool the agent called (and whether it worked).
    Tool,
    /// The cockpit's own notes (finished, failed, stopped).
    Note,
}

/// One line of the transcript.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub who: Who,
    pub text: String,
    pub ok: Option<bool>,
}

/// The cockpit's state.
pub(crate) struct Cockpit {
    /// The panel is shown.
    pub open: bool,
    /// The way the agent runs.
    pub agent: Box<dyn Adapter>,
    /// The editor's MCP endpoint and its token, once an agent needed it.
    pub mcp: Option<(Endpoint, String)>,
    /// The task running.
    pub run: Option<Run>,
    pub transcript: Vec<Line>,
    /// The request being written.
    pub request: RefCell<String>,
    /// What the person pointed at, for the next request.
    pub context: Vec<String>,
    /// The next click in the view points at something instead of
    /// selecting it.
    pub pointing: bool,
    /// What the last task told the agent (for tests and curiosity).
    pub last_instructions: Option<String>,
    /// The last task given to the agent.
    pub last_task: Option<wish_agent::Task>,
    /// The conversation to continue (the agent's id for it), while the
    /// conversation is remembered.
    pub session: Option<String>,
    /// What this editor's tasks cost so far, in US dollars, as the agent
    /// reported it.
    pub cost: f64,
    /// The checkpoint saved before each task, by the index of the task's
    /// request in the transcript (the task card's "Undo" goes back to it).
    pub before: std::collections::BTreeMap<usize, String>,
    /// The person's choices for the agent (the AI settings).
    pub settings: AgentSettings,
}

/// How the agent works, as the person set it (kept in the preferences).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentSettings {
    /// The model (`None`: the assistant's own default).
    pub model: Option<String>,
    /// Tasks continue one conversation (the agent remembers earlier ones).
    pub memory: bool,
    /// A checkpoint is saved before every task.
    pub checkpoint_first: bool,
    /// The agent may change the project's files itself (game code).
    pub files: bool,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            model: None,
            memory: true,
            checkpoint_first: true,
            files: true,
        }
    }
}

impl Cockpit {
    pub fn new(agent: Box<dyn Adapter>) -> Self {
        Self {
            open: false,
            agent,
            mcp: None,
            run: None,
            transcript: Vec::new(),
            request: RefCell::new(String::new()),
            context: Vec::new(),
            pointing: false,
            last_instructions: None,
            last_task: None,
            session: None,
            cost: 0.0,
            before: std::collections::BTreeMap::new(),
            settings: AgentSettings::default(),
        }
    }

    /// Takes in what the running task reported; returns whether anything
    /// happened, and whether a tool the agent called worked (the scene may
    /// have changed).
    pub fn take_events(&mut self) -> (bool, bool) {
        let Some(run) = self.run.as_mut() else {
            return (false, false);
        };
        let events = run.poll();
        let over = run.is_over();
        let mut changed = false;
        for event in &events {
            match event {
                Event::Said(text) => self.transcript.push(Line {
                    who: Who::Agent,
                    text: text.clone(),
                    ok: None,
                }),
                Event::Called { .. } => {}
                Event::Answered { tool, ok } => {
                    changed |= *ok;
                    self.transcript.push(Line {
                        who: Who::Tool,
                        text: tool.clone(),
                        ok: Some(*ok),
                    });
                }
                Event::Finished { ok, text } => {
                    // Claude Code repeats its last message as the result:
                    // said once is enough (found in the first real run).
                    let repeated = self
                        .transcript
                        .iter()
                        .rev()
                        .find(|line| line.who == Who::Agent)
                        .is_some_and(|line| line.text.trim() == text.trim());
                    self.transcript.push(Line {
                        who: Who::Note,
                        text: if text.trim().is_empty() || text == "done" || repeated {
                            t("Done.").to_owned()
                        } else {
                            text.clone()
                        },
                        ok: Some(*ok),
                    });
                }
                Event::Failed(problem) => self.transcript.push(Line {
                    who: Who::Note,
                    text: problem.clone(),
                    ok: Some(false),
                }),
                Event::Session(id) => {
                    if self.settings.memory {
                        self.session = Some(id.clone());
                    }
                }
                Event::Cost(dollars) => {
                    if dollars.is_finite() && *dollars >= 0.0 {
                        self.cost += dollars;
                    }
                }
            }
        }
        if over {
            self.run = None;
        }
        (!events.is_empty(), changed)
    }
}

/// A tool call in the transcript: `edit_apply: done` / `failed`.
pub(crate) fn tool_line(tool: &str, ok: bool) -> String {
    if ok {
        tf("{tool}: done", &[("tool", &tool)])
    } else {
        tf("{tool}: failed", &[("tool", &tool)])
    }
}

/// What the panel asked for.
#[derive(Default)]
pub(crate) struct CockpitActions {
    pub send: bool,
    pub stop: bool,
    pub add_selection: bool,
    pub point: bool,
    pub remove_context: Option<usize>,
}

impl CockpitActions {
    pub fn any(&self) -> bool {
        self.send || self.stop || self.add_selection || self.point || self.remove_context.is_some()
    }
}

/// The panel.
pub(crate) fn panel(ui: &mut Ui, cockpit: &Cockpit, mode: &str, actions: &mut CockpitActions) {
    crate::style::panel_title(ui, t("Agent"));
    ui.weak(tf(
        "{agent} · permission: {mode}",
        &[("agent", &cockpit.agent.name()), ("mode", &mode)],
    ));
    ui.separator();
    let height = (ui.available_height() - 190.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("cockpit")
        .max_height(height)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if cockpit.transcript.is_empty() {
                ui.weak(t(
                    "Ask for something: the agent changes the scene while you watch, and every change is a card you can take back (Activity tab).",
                ));
            }
            for line in &cockpit.transcript {
                match line.who {
                    Who::Person => {
                        ui.label(RichText::new(&line.text).strong());
                    }
                    Who::Agent => {
                        ui.label(&line.text);
                    }
                    Who::Tool => {
                        ui.weak(tool_line(&line.text, line.ok == Some(true)));
                    }
                    Who::Note => {
                        let color = if line.ok == Some(false) {
                            Color32::from_rgb(235, 100, 90)
                        } else {
                            Color32::from_rgb(120, 200, 140)
                        };
                        ui.label(RichText::new(&line.text).color(color));
                    }
                }
            }
        });
    ui.separator();
    for (index, item) in cockpit.context.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui
                .small_button("x")
                .on_hover_text(t("Leave it out"))
                .clicked()
            {
                actions.remove_context = Some(index);
            }
            ui.weak(item);
        });
    }
    ui.horizontal(|ui| {
        if ui
            .button(t("Add selection"))
            .on_hover_text(t("The selected entities go with the request"))
            .clicked()
        {
            actions.add_selection = true;
        }
        if ui
            .selectable_label(cockpit.pointing, t("Point in the view"))
            .on_hover_text(t("Click something in the view: it goes with the request"))
            .clicked()
        {
            actions.point = true;
        }
    });
    let mut request = cockpit.request.borrow_mut();
    ui.add(
        egui::TextEdit::multiline(&mut *request)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text(t("What should the agent do?")),
    );
    let empty = request.trim().is_empty();
    drop(request);
    ui.horizontal(|ui| {
        if cockpit.run.is_some() {
            ui.spinner();
            if ui.button(t("Stop")).clicked() {
                actions.stop = true;
            }
        } else if ui
            .add_enabled(!empty, egui::Button::new(t("Send")))
            .on_hover_text(t("A checkpoint is saved first: you can always go back"))
            .clicked()
        {
            actions.send = true;
        }
    });
}
