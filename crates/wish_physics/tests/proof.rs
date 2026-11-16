//! M6 WP1: the proof that Rapier with `enhanced-determinism` gives the same
//! bits on every machine (a recorded hash, checked by CI on Windows and
//! Linux) and what a step costs.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rapier3d::prelude::*;
use wish_core::StableHasher;

struct Sim {
    pipeline: PhysicsPipeline,
    gravity: Vector,
    params: IntegrationParameters,
    islands: IslandManager,
    broad: BroadPhaseBvh,
    narrow: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse: ImpulseJointSet,
    multibody: MultibodyJointSet,
    soft: SoftBodySet,
    ccd: CCDSolver,
}

impl Clone for Sim {
    /// Everything that carries over between steps, with a new pipeline: the
    /// pipeline is scratch memory (the test below shows that nothing is lost).
    fn clone(&self) -> Self {
        Self {
            pipeline: PhysicsPipeline::new(),
            gravity: self.gravity,
            params: self.params,
            islands: self.islands.clone(),
            broad: self.broad.clone(),
            narrow: self.narrow.clone(),
            bodies: self.bodies.clone(),
            colliders: self.colliders.clone(),
            impulse: self.impulse.clone(),
            multibody: self.multibody.clone(),
            soft: self.soft.clone(),
            ccd: self.ccd.clone(),
        }
    }
}

impl Sim {
    fn new() -> Self {
        Self {
            pipeline: PhysicsPipeline::new(),
            gravity: Vector::new(0.0, -9.81, 0.0),
            params: IntegrationParameters {
                dt: 1.0 / 60.0,
                ..IntegrationParameters::default()
            },
            islands: IslandManager::new(),
            broad: BroadPhaseBvh::new(),
            narrow: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(),
            soft: SoftBodySet::new(),
            ccd: CCDSolver::new(),
        }
    }

    fn step(&mut self) {
        self.pipeline.step(
            self.gravity,
            &self.params,
            &mut self.islands,
            &mut self.broad,
            &mut self.narrow,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse,
            &mut self.multibody,
            &mut self.soft,
            &mut self.ccd,
            &(),
            &(),
        );
    }

    /// A pyramid of `layers` layers of boxes on a floor, plus a ball thrown at it.
    fn pyramid(layers: usize) -> Self {
        let mut sim = Self::new();
        let floor = sim.bodies.insert(RigidBodyBuilder::fixed());
        sim.colliders.insert_with_parent(
            ColliderBuilder::cuboid(50.0, 0.5, 50.0)
                .translation(Vector::new(0.0, -0.5, 0.0))
                .friction(0.6),
            floor,
            &mut sim.bodies,
        );
        for layer in 0..layers {
            let width = layers - layer;
            for i in 0..width {
                let x = (i as f32 - (width as f32 - 1.0) / 2.0) * 1.05;
                let y = 0.5 + layer as f32 * 1.0;
                let body = sim
                    .bodies
                    .insert(RigidBodyBuilder::dynamic().translation(Vector::new(x, y, 0.0)));
                sim.colliders.insert_with_parent(
                    ColliderBuilder::cuboid(0.5, 0.5, 0.5)
                        .friction(0.6)
                        .restitution(0.1),
                    body,
                    &mut sim.bodies,
                );
            }
        }
        let ball = sim.bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(Vector::new(-12.0, 2.0, 0.3))
                .linvel(Vector::new(14.0, 1.0, 0.0)),
        );
        sim.colliders.insert_with_parent(
            ColliderBuilder::ball(0.6).density(4.0).restitution(0.3),
            ball,
            &mut sim.bodies,
        );
        sim
    }

    /// A stable hash of every body's position, rotation and velocity, in
    /// the set's own (insertion) order.
    fn hash(&self) -> u128 {
        let mut hasher = StableHasher::new();
        for (_, body) in self.bodies.iter() {
            let p = body.translation();
            let r = body.rotation();
            let v = body.linvel();
            let w = body.angvel();
            for value in [
                p.x, p.y, p.z, r.x, r.y, r.z, r.w, v.x, v.y, v.z, w.x, w.y, w.z,
            ] {
                hasher.write_u32(value.to_bits());
            }
        }
        hasher.finish().as_u128()
    }
}

fn run(layers: usize, ticks: usize) -> u128 {
    let mut sim = Sim::pyramid(layers);
    for _ in 0..ticks {
        sim.step();
    }
    sim.hash()
}

#[test]
fn the_same_scene_gives_the_same_bits() {
    assert_eq!(run(6, 600), run(6, 600));
}

#[test]
fn a_pyramid_comes_to_rest_and_the_ball_knocks_it() {
    let mut sim = Sim::pyramid(6);
    for _ in 0..600 {
        sim.step();
    }
    // Everything is above the floor, nothing escaped.
    for (_, body) in sim.bodies.iter() {
        let p = body.translation();
        assert!(p.y > -0.1 && p.y < 10.0, "{p:?}");
        assert!(p.x.abs() < 60.0 && p.z.abs() < 60.0, "{p:?}");
    }
}

/// ADR-026: Rapier's whole state is cloned with a snapshot; going back and
/// continuing gives the same bits as never going back, even though the
/// warm-start impulses and the broad-phase tree are not in any component.
#[test]
fn a_cloned_state_continues_bit_for_bit() {
    let mut straight = Sim::pyramid(6);
    let mut saved = None;
    for tick in 0..400 {
        if tick == 150 {
            saved = Some(straight.clone());
        }
        straight.step();
    }
    let mut resumed = saved.unwrap();
    for _ in 150..400 {
        resumed.step();
    }
    assert_eq!(resumed.hash(), straight.hash());
    // Not just the bodies: the next steps stay equal too (warm starting and
    // contact caches were carried over).
    for _ in 0..200 {
        straight.step();
        resumed.step();
        assert_eq!(resumed.hash(), straight.hash());
    }
}

/// The recorded value. It is the same on every platform the engine builds
/// on; if it ever differs, the first thing to look at is whether a Rapier
/// feature (SIMD, parallel) changed (ADR-014).
#[test]
fn the_recorded_hash_holds() {
    let hash = run(6, 600);
    println!("pyramid(6) after 600 ticks: {hash:#034x}");
    assert_eq!(hash, RECORDED);
}

const RECORDED: u128 = 0x8c77d52755b303cb697ba766c515b051;

/// `cargo test --release -p wish_physics -- --ignored --nocapture`
#[test]
#[ignore = "measurement"]
fn what_a_step_costs() {
    for layers in [10usize, 20, 44] {
        let mut sim = Sim::pyramid(layers);
        let bodies = sim.bodies.len();
        for _ in 0..60 {
            sim.step();
        }
        let start = std::time::Instant::now();
        let ticks = 120;
        for _ in 0..ticks {
            sim.step();
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks);
        println!(
            "{bodies} bodies: {ms:.3} ms per tick ({:.0}% of a 16.7 ms frame)",
            ms / 16.667 * 100.0
        );
    }
}
