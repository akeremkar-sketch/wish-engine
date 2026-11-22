//! Playing with several players in the editor (release 1.1, package 14;
//! design note `docs/design/1.1-net-tool.md` §7): the Game menu's Players
//! (Play then hosts an online match over the network simulator with bots in
//! the other seats, one `net` call by `human`), and the network panel: the
//! lobby, every seat with its round trip and bandwidth (a `net status`
//! call), and which player's machine the game view shows.

use egui::Ui;
use serde_json::Value;

use crate::i18n::{t, tf};

/// The most players the Players menu offers (a match takes up to 16; the
/// control plane and `wen play --players` take the rest).
pub(crate) const MENU_PLAYERS: usize = 8;

/// The network panel as it was last read.
#[derive(Default)]
pub(crate) struct NetView {
    /// The panel is shown.
    pub open: bool,
    /// The last `net status` answer, while playing online.
    pub status: Option<Value>,
    /// The seat whose machine the game view shows (`None`: the host's).
    pub watching: Option<u32>,
}

/// What the person asked for in the menu or the panel.
#[derive(Default)]
pub(crate) struct NetActions {
    /// Play with this many players from now on.
    pub players: Option<usize>,
    /// Show or hide the network panel.
    pub toggle: bool,
    /// Show this seat's machine in the game view (`Some(None)`: the host's).
    pub watch: Option<Option<u32>>,
}

/// The Game menu's Players submenu and the network panel's switch.
pub(crate) fn menu(
    ui: &mut Ui,
    players: usize,
    editing: bool,
    open: bool,
    actions: &mut NetActions,
) {
    ui.menu_button(t("Players"), |ui| {
        for count in 1..=MENU_PLAYERS {
            let label = if count == 1 {
                t("1 player").to_owned()
            } else {
                tf("{count} players", &[("count", &count)])
            };
            if ui
                .add_enabled(editing, egui::Button::selectable(players == count, label))
                .on_hover_text(t(
                    "With 2 or more, Play hosts an online match: you are the host, bots take the other seats over a simulated network",
                ))
                .clicked()
            {
                actions.players = Some(count);
                ui.close();
            }
        }
    });
    if ui
        .add(egui::Button::selectable(open, t("Network panel")))
        .clicked()
    {
        actions.toggle = true;
        ui.close();
    }
}

/// The network panel.
pub(crate) fn window(ui: &mut Ui, view: &NetView, actions: &mut NetActions) {
    if !view.open {
        return;
    }
    let mut open = true;
    egui::Window::new(t("Network"))
        .open(&mut open)
        .default_width(360.0)
        .resizable(true)
        .show(ui.ctx(), |ui| {
            let Some(status) = view.status.as_ref() else {
                ui.weak(t(
                    "Not an online game: choose 2 or more Players in the Game menu, then Play.",
                ));
                return;
            };
            let phase = status["lobby"]["phase"].as_str().unwrap_or("?").to_owned();
            let round = status["lobby"]["round"].as_u64().unwrap_or(0);
            ui.label(tf(
                "Lobby: {phase}, round {round}",
                &[("phase", &phase), ("round", &round)],
            ));
            ui.separator();
            ui.label(t("Show in the game view:"));
            let seats = status["seats"].as_array().cloned().unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(egui::Button::selectable(view.watching.is_none(), t("Host")))
                    .clicked()
                {
                    actions.watch = Some(None);
                }
                for seat in &seats {
                    let Some(player) = seat["player"].as_u64().and_then(|p| u32::try_from(p).ok())
                    else {
                        continue;
                    };
                    let local = matches!(seat["machine"].as_str(), Some("bot" | "local"));
                    if player == 0 || !local {
                        continue;
                    }
                    let label = tf("Player {seat}", &[("seat", &player)]);
                    if ui
                        .add(egui::Button::selectable(
                            view.watching == Some(player),
                            label,
                        ))
                        .clicked()
                    {
                        actions.watch = Some(Some(player));
                    }
                }
            });
            ui.separator();
            egui::Grid::new("network-seats")
                .striped(true)
                .num_columns(6)
                .show(ui, |ui| {
                    for header in [
                        t("Seat"),
                        t("Name"),
                        t("Ready"),
                        t("Connected"),
                        t("Round trip"),
                        t("Received"),
                    ] {
                        ui.strong(header);
                    }
                    ui.end_row();
                    for seat in &seats {
                        let yes = |flag: &Value| if *flag == true { t("yes") } else { t("no") };
                        ui.label(seat["player"].to_string());
                        ui.label(seat["name"].as_str().unwrap_or("?"));
                        ui.label(yes(&seat["ready"]));
                        ui.label(yes(&seat["connected"]));
                        match seat["rtt_ms"].as_u64() {
                            Some(ms) => ui.label(tf("{ms} ms", &[("ms", &ms)])),
                            None => ui.label("-"),
                        };
                        match seat["received_per_second"].as_u64() {
                            Some(bytes) => ui.label(tf("{bytes} B/s", &[("bytes", &bytes)])),
                            None => ui.label("-"),
                        };
                        ui.end_row();
                    }
                });
            let host = &status["host"];
            if host.is_object() {
                ui.separator();
                ui.weak(tf(
                    "host: {states} states sent, {bytes} bytes, {messages} messages delivered",
                    &[
                        ("states", &host["states_sent"].as_u64().unwrap_or(0)),
                        ("bytes", &host["bytes_sent"].as_u64().unwrap_or(0)),
                        (
                            "messages",
                            &host["messages_delivered"].as_u64().unwrap_or(0),
                        ),
                    ],
                ));
            }
        });
    if !open {
        actions.toggle = true;
    }
}
