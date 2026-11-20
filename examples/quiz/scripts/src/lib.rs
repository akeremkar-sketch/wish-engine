//! Quiz: a small question game made almost only of interface, an example
//! for Wish Engine (M7). A menu, five questions with four answer buttons
//! each, a result screen. Everything on screen is `UiNode` boxes; game code
//! only reacts to `ButtonClicked` and writes text.

use wish_sdk::prelude::*;

/// A whole screen (a full-size box); only the current one is shown.
#[component]
pub struct Screen {
    /// 0 menu, 1 questions, 2 result.
    pub kind: u32,
}

/// A button that starts (or restarts) the game.
#[component]
pub struct StartButton {
    /// Unused.
    pub unused: u32,
}

/// An answer button.
#[component]
pub struct Answer {
    /// Which answer it stands for, 0 to 3.
    pub index: u32,
}

/// A text the game writes.
#[component]
pub struct UiLabel {
    /// 0 the question, 1 to 4 the answers, 5 the progress, 6 the result.
    pub slot: u32,
}

/// Where the game stands.
#[resource]
pub struct Game {
    /// 0 menu, 1 questions, 2 result.
    pub screen: u32,
    /// The question being asked.
    pub question: u32,
    /// Right answers so far.
    pub score: u32,
    /// What the texts were written for, so they are written once.
    pub written: u32,
}

/// A question, its four answers and the right one.
struct Question {
    text: &'static str,
    answers: [&'static str; 4],
    right: u32,
}

const QUESTIONS: [Question; 5] = [
    Question {
        text: "What language is Wish Engine written in?",
        answers: ["Rust", "C++", "Go", "Zig"],
        right: 0,
    },
    Question {
        text: "What form does game logic run in?",
        answers: ["Lua", "WebAssembly", "Python", "Native machine code"],
        right: 1,
    },
    Question {
        text: "Which protocol does an AI drive the engine with?",
        answers: ["REST", "gRPC", "MCP", "SOAP"],
        right: 2,
    },
    Question {
        text: "What do the same seed and input give in a simulation?",
        answers: ["The same result", "Something else each time", "Only errors", "Half the same"],
        right: 0,
    },
    Question {
        text: "Which way is up in the coordinate system?",
        answers: ["+Z", "-Y", "-Z", "+Y"],
        right: 3,
    },
];

/// Clicks: the start buttons begin a game, the answer buttons answer.
#[system]
fn click(
    clicks: EventReader<ButtonClicked>,
    starts: Query<Entity, With<StartButton>>,
    answers: Query<(Entity, &Answer)>,
    mut game: ResMut<Game>,
) {
    for click in clicks.read() {
        if starts.iter().any(|b| b == click.entity) && game.screen != 1 {
            game.screen = 1;
            game.question = 0;
            game.score = 0;
            continue;
        }
        if game.screen != 1 {
            continue;
        }
        let Some((_, answer)) = answers.iter().find(|(b, _)| *b == click.entity) else {
            continue;
        };
        if let Some(question) = QUESTIONS.get(game.question as usize) {
            if answer.index == question.right {
                game.score += 1;
            }
        }
        game.question += 1;
        if game.question as usize >= QUESTIONS.len() {
            game.screen = 2;
        }
    }
}

/// Shows the current screen and writes its texts.
#[system(after = "click")]
fn show(
    mut game: ResMut<Game>,
    mut screens: Query<(&Screen, &mut UiNode)>,
    mut labels: Query<(&UiLabel, &mut UiText)>,
) {
    for (screen, node) in screens.iter_mut() {
        let visible = screen.kind == game.screen;
        if node.visible != visible {
            node.visible = visible;
        }
    }
    // What the texts say depends on these three numbers only.
    let key = game.screen * 10_000 + game.question * 100 + game.score;
    if game.written == key + 1 {
        return;
    }
    game.written = key + 1;
    let total = QUESTIONS.len() as u32;
    for (label, words) in labels.iter_mut() {
        let current = QUESTIONS.get(game.question as usize);
        let line = match (label.slot, current) {
            (0, Some(q)) => q.text.to_owned(),
            (slot @ 1..=4, Some(q)) => q.answers[slot as usize - 1].to_owned(),
            (5, _) => format!("Question {}/{} · Right: {}", (game.question + 1).min(total), total, game.score),
            (6, _) => format!("{} of {total} right!", game.score),
            _ => String::new(),
        };
        words.text = text(&line);
    }
}

wish_sdk::export! {
    components: [Screen, StartButton, Answer, UiLabel],
    resources: [Game],
    systems: [click, show],
}
