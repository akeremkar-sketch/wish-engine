//! The particle formula (M6 WP8): counts worked out by hand, positions that
//! follow from the emitter alone.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::math::{Mat4, Vec3};
use wish_ecs::World;
use wish_particles::{ParticleEmitter, ParticleState, alive, count, particles};
use wish_types::time::Time;

fn emitter() -> ParticleEmitter {
    ParticleEmitter {
        rate: 10.0,
        lifetime: 1.0,
        ..ParticleEmitter::default()
    }
}

#[test]
fn a_steady_emitter_keeps_rate_times_lifetime_alive() {
    let e = emitter();
    // One particle every 0.1 s, each living 1 s.
    assert_eq!(count(&e, 0.0), 1, "the first is born at the start");
    assert_eq!(count(&e, 0.45), 5, "born at 0, 0.1, 0.2, 0.3, 0.4");
    assert_eq!(count(&e, 5.05), 10, "steady state: rate x lifetime");
    // The oldest alive at 5.05 was born at 4.1.
    assert_eq!(alive(&e, 5.05), 41..51);
    assert_eq!(count(&e, -1.0), 0, "before the start");
}

#[test]
fn bursts_duration_max_and_stopping() {
    let burst = ParticleEmitter {
        rate: 0.0,
        burst: 30,
        lifetime: 2.0,
        ..ParticleEmitter::default()
    };
    assert_eq!(count(&burst, 0.0), 30);
    assert_eq!(count(&burst, 1.99), 30);
    assert_eq!(count(&burst, 2.0), 0, "they die at their lifetime");

    // A burst and a stream together: numbers run on from the burst.
    let both = ParticleEmitter {
        burst: 5,
        ..emitter()
    };
    assert_eq!(alive(&both, 0.25), 0..8);

    // Emitting for half a second: five particles, gone a second later.
    let short = ParticleEmitter {
        duration: 0.5,
        ..emitter()
    };
    assert_eq!(count(&short, 0.45), 5);
    assert_eq!(count(&short, 0.95), 5, "no more born after 0.5 s");
    assert_eq!(count(&short, 1.45), 0);

    let capped = ParticleEmitter {
        max: 3,
        ..emitter()
    };
    assert_eq!(alive(&capped, 5.05), 48..51, "the newest are kept");

    let stopped = ParticleEmitter {
        playing: false,
        ..emitter()
    };
    assert_eq!(count(&stopped, 3.0), 0);

    let restarted = ParticleEmitter {
        start_time: 10.0,
        ..emitter()
    };
    assert_eq!(count(&restarted, 9.0), 0);
    assert_eq!(count(&restarted, 10.25), 3);
}

#[test]
fn particles_follow_from_the_emitter_alone() {
    let e = ParticleEmitter {
        speed: 2.0,
        speed_variation: 0.0,
        spread_deg: 0.0,
        size_start: 1.0,
        size_end: 0.0,
        ..emitter()
    };
    let at = Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0));
    let shown = particles(&e, &at, 0.55, usize::MAX);
    assert_eq!(shown.len(), 6);
    // Straight up at 2 m/s from the emitter: the oldest is 0.55 s old.
    let oldest = shown[0];
    assert!((oldest.position - Vec3::new(3.0, 1.1, 0.0)).length() < 1e-4);
    assert!((oldest.size - 0.45).abs() < 1e-4, "{}", oldest.size);
    // Halfway through its life the colour is halfway to transparent.
    let half = particles(&e, &at, 0.5, usize::MAX)[0];
    assert!((half.color[3] - 0.5).abs() < 1e-4);
    // The same moment gives the same particles.
    assert_eq!(particles(&e, &at, 0.55, usize::MAX), shown);

    // Gravity bends them; another seed scatters them differently.
    let falling = ParticleEmitter {
        gravity: Vec3::new(0.0, -10.0, 0.0),
        ..e
    };
    let p = particles(&falling, &Mat4::IDENTITY, 1.0, usize::MAX)[0];
    assert!(p.position.y < 2.0 * 0.95);
    let wide = ParticleEmitter {
        spread_deg: 60.0,
        speed_variation: 0.5,
        ..e
    };
    let a = particles(&wide, &Mat4::IDENTITY, 2.0, usize::MAX);
    let b = particles(
        &ParticleEmitter { seed: 7, ..wide },
        &Mat4::IDENTITY,
        2.0,
        usize::MAX,
    );
    assert_ne!(a, b);
    // Within the cone: no particle more than 60 degrees from +Y.
    for p in a.iter().filter(|p| p.position.length() > 1e-3) {
        let direction = p.position.normalize();
        assert!(
            direction.y >= 60f32.to_radians().cos() - 1e-4,
            "{direction}"
        );
    }
}

#[test]
fn hostile_numbers_give_no_particles_and_no_panics() {
    for e in [
        ParticleEmitter {
            rate: f32::NAN,
            lifetime: f32::INFINITY,
            ..emitter()
        },
        ParticleEmitter {
            rate: 1.0e30,
            ..emitter()
        },
        ParticleEmitter {
            start_time: f32::NAN,
            speed: f32::NAN,
            gravity: Vec3::splat(f32::NAN),
            ..emitter()
        },
    ] {
        let alive = count(&e, 3.0);
        assert!(alive <= e.max);
        for p in particles(&e, &Mat4::IDENTITY, 3.0, usize::MAX) {
            assert!(p.size.is_finite());
        }
    }
}

#[test]
fn the_system_writes_how_many_are_alive() {
    let mut world = World::new();
    wish_particles::register(&mut world).unwrap();
    let entity = world.spawn().unwrap();
    world.insert(entity, emitter()).unwrap();
    world.insert_resource(Time::at(30, 60)).unwrap();
    wish_particles::step(&mut world).unwrap();
    assert_eq!(world.get::<ParticleState>(entity).unwrap().alive, 6);
    let hash = world.hash();
    wish_particles::step(&mut world).unwrap();
    assert_eq!(world.hash(), hash, "unchanged counts are not rewritten");
}

#[test]
fn values_beyond_the_ranges_from_game_code_are_bounded() {
    // Game code is not held to the schema's ranges (M6 closing review).
    let wild = ParticleEmitter {
        max: u32::MAX,
        lifetime: 1.0e9,
        rate: 1.0e9,
        burst: u32::MAX,
        start_time: -1.0e30,
        ..emitter()
    };
    let living = count(&wild, 100.0);
    assert!(living <= wish_particles::MAX_ALIVE, "{living}");
    // A drawing budget is kept: the newest come first.
    let shown = particles(&wild, &Mat4::IDENTITY, 100.0, 500);
    assert_eq!(shown.len(), 500);
    let all = alive(&wild, 100.0);
    assert_eq!(shown.last().unwrap().number, all.end - 1);
}
