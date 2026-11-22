//! Playing with several players in the editor (release 1.1, package 14;
//! design note `docs/design/1.1-net-tool.md` §7): the Game menu's Players
//! (Play then hosts an online match over the network simulator with bots in
//! the other seats, one `net` call by `human`), and the network panel: the
//! lobby, every seat with its round trip and bandwidth (a `net status`
//! call), and which player's machine the game view shows. With no game
//! running, the panel hosts for people over UDP and joins a host, through a
//! relay when the person set one (package 14c, the note's §13).

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
    /// The seat whose machine the game view shows (`None`: the person's
    /// own machine, or the host's once `host_view`).
    pub watching: Option<u32>,
    /// The person chose the host's view.
    pub host_view: bool,
    /// The address of the relay the person plays through over the internet
    /// (the `relay` preference: the person's, never the project's).
    pub relay: String,
    /// The room code to join, as typed.
    pub room: String,
    /// A game is running in the editor (online play cannot start).
    pub playing: bool,
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
    /// The relay field now says this (it is remembered).
    pub relay: Option<String>,
    /// The room code field now says this.
    pub room: Option<String>,
    /// Host a match for people on the network (UDP), also through the relay
    /// when one is set.
    pub host_online: bool,
    /// Join a match: through the relay with the room code when a relay is
    /// set, else found on the local network.
    pub join: bool,
}

/// The online section of the network panel when no match runs: the relay's
/// address, a room code, Host online and Join.
fn online(ui: &mut Ui, view: &NetView, actions: &mut NetActions) {
    ui.strong(t("Play with people"));
    ui.horizontal(|ui| {
        // The explanation is the label's, so that the box keeps its hint as
        // its name.
        ui.label(t("Relay")).on_hover_text(t(
            "A relay you run with wen relay carries players from the internet to a host behind a home router. It is kept on this computer, never in the project. Leave it empty to play on the local network only",
        ));
        let mut relay = view.relay.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut relay)
                    .hint_text(t("Relay address (host:port)"))
                    .desired_width(200.0),
            )
            .changed()
        {
            actions.relay = Some(relay);
        }
    });
    ui.horizontal(|ui| {
        ui.label(t("Room code"));
        let mut room = view.room.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut room)
                    .hint_text(t("The host's room code"))
                    .desired_width(120.0),
            )
            .changed()
        {
            actions.room = Some(room);
        }
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!view.playing, egui::Button::new(t("Host online")))
            .on_hover_text(t(
                "Hosts the game for people on the local network, and through the relay when one is set; you play the first seat and the panel shows the room code to give the others",
            ))
            .clicked()
        {
            actions.host_online = true;
        }
        let can_join = !view.playing && !view.room.trim().is_empty();
        if ui
            .add_enabled(can_join, egui::Button::new(t("Join")))
            .on_hover_text(t(
                "Joins the host with this room code: through the relay when one is set, else on the local network. Your own player answers at once (predicted)",
            ))
            .clicked()
        {
            actions.join = true;
        }
    });
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
                    "With 2 or more, Play hosts an online match over a simulated internet (50 ms each way): you play a seat as an online player does, with your own player predicted, and bots take the other seats",
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
        // Below the toolbar, so that it never covers Play and Stop.
        .default_pos(egui::pos2(16.0, 80.0))
        .default_width(360.0)
        .resizable(true)
        .show(ui.ctx(), |ui| {
            let Some(status) = view.status.as_ref() else {
                ui.weak(t(
                    "Not an online game: choose 2 or more Players in the Game menu, then Play.",
                ));
                ui.separator();
                online(ui, view, actions);
                return;
            };
            let phase = status["lobby"]["phase"].as_str().unwrap_or("?").to_owned();
            let round = status["lobby"]["round"].as_u64().unwrap_or(0);
            ui.label(tf(
                "Lobby: {phase}, round {round}",
                &[("phase", &phase), ("round", &round)],
            ));
            if let Some(code) = status["room_code"].as_str() {
                ui.label(tf("Room code: {code}", &[("code", &code)]));
            }
            if let Some(state) = status["relay"]["state"].as_str() {
                let state = match state {
                    "ready" => t("ready").to_owned(),
                    "waiting" => t("waiting").to_owned(),
                    _ => tf(
                        "refused ({reason})",
                        &[("reason", &status["relay"]["reason"].as_str().unwrap_or("?"))],
                    ),
                };
                let address = status["relay"]["address"].as_str().unwrap_or("?");
                ui.label(tf(
                    "Relay {address}: {state}",
                    &[("address", &address), ("state", &state)],
                ));
            }
            ui.separator();
            ui.label(t("Show in the game view:"));
            let seats = status["seats"].as_array().cloned().unwrap_or_default();
            // The person's own seat, shown until another is chosen.
            let you = seats
                .iter()
                .find(|seat| seat["machine"] == "you")
                .and_then(|seat| seat["player"].as_u64())
                .and_then(|p| u32::try_from(p).ok());
            let shown = view.watching.or(if view.host_view { None } else { you });
            ui.horizontal_wrapped(|ui| {
                // A match hosted elsewhere has no host's world here.
                if status["role"] != "player"
                    && ui
                        .add(egui::Button::selectable(shown.is_none(), t("Host")))
                        .clicked()
                {
                    actions.watch = Some(None);
                }
                for seat in &seats {
                    let Some(player) = seat["player"].as_u64().and_then(|p| u32::try_from(p).ok())
                    else {
                        continue;
                    };
                    let local = matches!(seat["machine"].as_str(), Some("bot" | "local" | "you"));
                    if player == 0 || !local {
                        continue;
                    }
                    let label = if you == Some(player) {
                        tf("You (player {seat})", &[("seat", &player)])
                    } else {
                        tf("Player {seat}", &[("seat", &player)])
                    };
                    if ui
                        .add(egui::Button::selectable(shown == Some(player), label))
                        .clicked()
                    {
                        actions.watch = Some(Some(player));
                    }
                }
            });
            ui.separator();
            egui::Grid::new("network-seats")
                .striped(true)
                .num_columns(7)
                .show(ui, |ui| {
                    for header in [
                        t("Seat"),
                        t("Name"),
                        t("Ready"),
                        t("Connected"),
                        t("Path"),
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
                        ui.label(match seat["via"].as_str() {
                            Some("relay") => t("relay"),
                            Some(_) => t("direct"),
                            None => "-",
                        });
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
