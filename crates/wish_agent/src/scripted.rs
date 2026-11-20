//! A scripted agent: a fixed list of tool calls, made over MCP like a real
//! agent's. For tests and demonstrations of the cockpit without a model.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use serde_json::Value;

use crate::mcp_client::Client;
use crate::{Adapter, Event, Run, Task};

/// The scripted adapter.
#[derive(Clone, Debug)]
pub struct Scripted {
    /// Its name, as the editor's history shows it (`agent:<name>`).
    pub name: String,
    /// What it says first.
    pub says: String,
    /// The calls, in order: tool and arguments.
    pub calls: Vec<(String, Value)>,
}

impl Adapter for Scripted {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&self, task: &Task) -> Result<Run, String> {
        let (events, received) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let script = self.clone();
        let task = task.clone();
        std::thread::Builder::new()
            .name("wish-agent-scripted".to_owned())
            .spawn(move || {
                let mut client = match Client::connect(&task.endpoint, &task.token, &script.name) {
                    Ok(client) => client,
                    Err(problem) => {
                        let _ = events.send(Event::Failed(problem));
                        return;
                    }
                };
                // Like Claude Code, it names its conversation first: a task
                // that resumes it is told so (tests of the conversation's
                // memory see the id come back).
                let _ = events.send(Event::Session(match &task.resume {
                    Some(earlier) => earlier.clone(),
                    None => format!("{}-conversation", script.name),
                }));
                let _ = events.send(Event::Said(script.says.clone()));
                let mut all_ok = true;
                for (tool, args) in &script.calls {
                    if stop.load(Ordering::Relaxed) {
                        let _ = events.send(Event::Finished {
                            ok: false,
                            text: "stopped".to_owned(),
                        });
                        return;
                    }
                    let _ = events.send(Event::Called {
                        tool: tool.clone(),
                        args: args.clone(),
                    });
                    let ok = client
                        .call(tool, args)
                        .is_ok_and(|answer| answer["ok"] == true);
                    all_ok &= ok;
                    let _ = events.send(Event::Answered {
                        tool: tool.clone(),
                        ok,
                    });
                }
                // Like Claude Code, the result repeats the last message.
                let _ = events.send(Event::Finished {
                    ok: all_ok,
                    text: script.says.clone(),
                });
            })
            .map_err(|e| format!("could not start the agent: {e}"))?;
        Ok(Run::new(
            received,
            Box::new(move || stopping.store(true, Ordering::Relaxed)),
        ))
    }
}
