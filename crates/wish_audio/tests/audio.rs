//! The sound system in an app: sources start, fade with distance, pan,
//! end, loop; one-off sounds; the log; snapshots.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_app::{App, AppConfig, System};
use wish_audio::{LogKind, LogLine, drain_log};
use wish_core::math::Vec3;
use wish_core::{AssetRef, Entity};
use wish_transform::Transform;
use wish_types::audio::{
    AudioBus, AudioListener, AudioPlayback, AudioSource, PlaySound, SoundEnded, SoundStarted,
};

const JUMP: &str = "assets/sounds/jump.wav";
const MUSIC: &str = "assets/sounds/music.ogg";

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    wish_audio::register(app.world_mut()).unwrap();
    wish_audio::install(
        app.world_mut(),
        [(JUMP.to_owned(), 0.5), (MUSIC.to_owned(), 2.0)],
    );
    app.add_system(wish_audio::system()).unwrap();
    app.add_system(
        System::new("transform_propagation", |world, _| {
            wish_transform::update_global_transforms(world)
        })
        .after(wish_audio::SYSTEM),
    )
    .unwrap();
    app
}

fn clip(app: &mut App, path: &str) -> AssetRef {
    AssetRef::from_text(app.world_mut().intern(path).unwrap())
}

fn listener(app: &mut App, at: Vec3) -> Entity {
    let world = app.world_mut();
    let entity = world.spawn().unwrap();
    world
        .insert(entity, Transform::from_translation(at))
        .unwrap();
    world.insert(entity, AudioListener { volume: 1.0 }).unwrap();
    entity
}

fn source(app: &mut App, path: &str, at: Vec3, looping: bool, playing: bool) -> Entity {
    let clip = clip(app, path);
    let world = app.world_mut();
    let entity = world.spawn().unwrap();
    world
        .insert(entity, Transform::from_translation(at))
        .unwrap();
    world
        .insert(
            entity,
            AudioSource {
                clip,
                bus: AudioBus::Sfx,
                volume: 1.0,
                pitch: 1.0,
                looping,
                spatial: true,
                min_distance: 1.0,
                max_distance: 20.0,
                playing,
            },
        )
        .unwrap();
    entity
}

fn run(app: &mut App, ticks: usize) -> Vec<LogLine> {
    let mut log = Vec::new();
    for _ in 0..ticks {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        log.extend(drain_log(app.world_mut()));
    }
    log
}

#[test]
fn a_sound_starts_ends_and_says_how_it_sounds() {
    let mut app = app();
    listener(&mut app, Vec3::ZERO);
    // 4 m to the right of the listener: a quarter of the volume, all right.
    let jump = source(&mut app, JUMP, Vec3::new(4.0, 0.0, 0.0), false, true);
    let log = run(&mut app, 60);
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(
        (log[0].kind, log[0].tick, log[0].source),
        (LogKind::Started, 0, jump)
    );
    assert!((log[0].gain - 0.25).abs() < 1e-5, "{:?}", log[0]);
    assert!((log[0].pan - 1.0).abs() < 1e-5, "{:?}", log[0]);
    assert_eq!(log[0].clip, JUMP);
    // 0.5 s at 60 ticks per second: ends at tick 30.
    assert_eq!((log[1].kind, log[1].tick), (LogKind::Ended, 30));
    let source = *app.world().get::<AudioSource>(jump).unwrap();
    assert!(!source.playing, "it switched itself off");
    // The events are in the world too, for game code.
    let started: Vec<SoundStarted> = app.world().events().unwrap();
    let ended: Vec<SoundEnded> = app.world().events().unwrap();
    assert!(started.len() <= 1 && ended.len() <= 1);
    // Switch it on again: it plays again.
    let mut again = source;
    again.playing = true;
    app.world_mut().insert(jump, again).unwrap();
    let log = run(&mut app, 5);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, LogKind::Started);
}

#[test]
fn distance_and_direction_decide_gain_and_pan() {
    let mut app = app();
    listener(&mut app, Vec3::ZERO);
    let near = source(&mut app, MUSIC, Vec3::new(0.0, 0.0, -0.5), true, true);
    let left = source(&mut app, MUSIC, Vec3::new(-5.0, 0.0, 0.0), true, true);
    let far = source(&mut app, MUSIC, Vec3::new(0.0, 30.0, 0.0), true, true);
    run(&mut app, 2);
    let hear = |app: &App, e: Entity| *app.world().get::<AudioPlayback>(e).unwrap();
    assert_eq!(hear(&app, near).gain, 1.0, "inside the minimum distance");
    assert!((hear(&app, left).gain - 0.2).abs() < 1e-5);
    assert!((hear(&app, left).pan + 1.0).abs() < 1e-5);
    assert_eq!(hear(&app, far).gain, 0.0, "beyond the maximum distance");
    // A non-spatial source ignores the distance.
    let mut flat = *app.world().get::<AudioSource>(far).unwrap();
    flat.spatial = false;
    app.world_mut().insert(far, flat).unwrap();
    run(&mut app, 1);
    assert_eq!(hear(&app, far).gain, 1.0);
}

#[test]
fn looping_one_shots_and_vanishing_sources() {
    let mut app = app();
    listener(&mut app, Vec3::ZERO);
    let music = source(&mut app, MUSIC, Vec3::ZERO, true, true);
    // A looping 2 s clip keeps playing past its end and never ends by itself.
    let log = run(&mut app, 300);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(app.world().get::<AudioSource>(music).unwrap().playing);
    let position = app.world().get::<AudioPlayback>(music).unwrap().position;
    assert!(position < 2.0, "{position}");
    // A PlaySound makes a short-lived entity that plays and is gone.
    let before = app.world().entities().count();
    let jump = clip(&mut app, JUMP);
    app.world_mut()
        .send(PlaySound {
            clip: jump,
            bus: AudioBus::Sfx,
            volume: 1.0,
            pitch: 2.0,
            spatial: false,
            position: Vec3::ZERO,
        })
        .unwrap();
    let log = run(&mut app, 30);
    let kinds: Vec<LogKind> = log.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [LogKind::Started, LogKind::Ended], "{log:?}");
    // Pitch 2 plays it twice as fast: 0.25 s, 15 ticks.
    assert_eq!(log[1].tick - log[0].tick, 15, "{log:?}");
    assert_eq!(
        app.world().entities().count(),
        before,
        "the one-shot is gone"
    );
    // Despawning a playing source ends its sound.
    app.world_mut().despawn(music).unwrap();
    let log = run(&mut app, 2);
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].kind, log[0].source), (LogKind::Ended, music));
}

#[test]
fn an_unknown_clip_is_reported_once_and_switched_off() {
    let mut app = app();
    listener(&mut app, Vec3::ZERO);
    let ghost = source(
        &mut app,
        "assets/sounds/missing.wav",
        Vec3::ZERO,
        false,
        true,
    );
    let log = run(&mut app, 5);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].kind, LogKind::Unknown);
    assert!(!app.world().get::<AudioSource>(ghost).unwrap().playing);
    assert!(!wish_audio::unknown_clip(&log[0]).hint.is_empty());
}

#[test]
fn going_back_and_continuing_is_the_same_as_never_going_back() {
    let mut app = app();
    listener(&mut app, Vec3::ZERO);
    source(&mut app, MUSIC, Vec3::new(3.0, 0.0, 0.0), true, true);
    source(&mut app, JUMP, Vec3::new(-2.0, 0.0, 0.0), false, true);
    run(&mut app, 10);
    let snapshot = app.snapshot();
    let straight_log = run(&mut app, 120);
    let straight = app.hash();
    app.restore(&snapshot).unwrap();
    let again_log = run(&mut app, 120);
    assert_eq!(app.hash(), straight);
    assert_eq!(again_log, straight_log, "the same history, the same log");
}
