//! Interpolation of entities the client does not predict: drawn a fixed
//! delay behind the host, between the two known states around that time,
//! extrapolated a little past the newest and then held (design note
//! `1.1-prediction.md` §3.7).
//!
//! A state is known at the ticks of the messages that carried it, and also
//! at every complete message read whole after it (which says it did not
//! change). Only `Transform` is interpolated.

use std::collections::BTreeSet;

use wish_schema::Plain;
use wish_types::Transform;

use crate::client::States;
use crate::value::EntityView;

/// Where a drawn transform comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The client's own prediction (the local player and what it touches).
    Predicted,
    /// Between two known states of the host.
    Interpolated,
    /// Past the newest known state, within the extrapolation limit.
    Extrapolated,
    /// A known state held: before the first, still since the last, or past
    /// the extrapolation limit.
    Held,
}

impl Source {
    /// Whether it is the client's own prediction.
    pub fn is_predicted(self) -> bool {
        self == Self::Predicted
    }
}

/// The `Transform` in an entity's state, when it has a valid one.
pub fn transform_of(view: &EntityView, ty: u16) -> Option<Transform> {
    Transform::decode(&view.get(&ty)?.bytes)
}

/// `a` towards `b` by `t` (0 to 1): translation and scale linearly,
/// rotation by the shorter way, normalized. A rotation that is not finite
/// keeps `a`'s.
pub fn blend(a: &Transform, b: &Transform, t: f32) -> Transform {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let rotation = if a.rotation.is_finite()
        && b.rotation.is_finite()
        && a.rotation.length_squared() > 1e-12
        && b.rotation.length_squared() > 1e-12
    {
        a.rotation.normalize().lerp(b.rotation.normalize(), t)
    } else {
        a.rotation
    };
    Transform {
        translation: a.translation.lerp(b.translation, t),
        rotation,
        scale: a.scale.lerp(b.scale, t),
    }
}

/// The newest tick at which state `(tick, _)` is still known to hold: the
/// newest complete tick in `tick..until` (else `tick` itself).
fn known_until(complete: &BTreeSet<u64>, tick: u64, until: u64) -> u64 {
    if until <= tick {
        return tick;
    }
    complete
        .range(tick..until)
        .next_back()
        .copied()
        .unwrap_or(tick)
}

/// An entity's transform at display time `at` (thousandths of a tick), from
/// its received states, the complete ticks and the `Transform` type index.
/// Past the newest known state it moves on at the last known speed for at
/// most `max_extrapolation` (thousandths of a tick), then holds. `None` when
/// no state has a transform.
pub fn sample(
    states: &States,
    complete: &BTreeSet<u64>,
    ty: u16,
    at: u64,
    max_extrapolation: u64,
) -> Option<(Transform, Source)> {
    let floor = at / 1000;
    let before = states.range(..=floor).next_back();
    let after = states.range(floor.saturating_add(1)..).next();
    let present = |state: &Option<std::sync::Arc<EntityView>>| {
        state.as_deref().and_then(|view| transform_of(view, ty))
    };
    match (before, after) {
        (None, None) => None,
        (None, Some((_, b))) => present(b).map(|b| (b, Source::Held)),
        (Some((&ta, a)), Some((&tb, b))) => {
            let (Some(a), Some(b)) = (present(a), present(b)) else {
                return present(a).or_else(|| present(b)).map(|t| (t, Source::Held));
            };
            // Still up to the newest complete tick before the change.
            let from = known_until(complete, ta, tb);
            if at <= from * 1000 {
                return Some((a, Source::Interpolated));
            }
            let span = (tb - from) * 1000;
            let t = (at - from * 1000) as f32 / span as f32;
            Some((blend(&a, &b, t), Source::Interpolated))
        }
        (Some((&ta, a)), None) => {
            let a = present(a)?;
            let newest_complete = complete.iter().next_back().copied().unwrap_or(ta);
            let known = newest_complete.max(ta);
            if at <= known * 1000 {
                return Some((a, Source::Interpolated));
            }
            if known > ta {
                // A complete message after it said it was still.
                return Some((a, Source::Held));
            }
            let Some((&tp, p)) = states.range(..ta).next_back() else {
                return Some((a, Source::Held));
            };
            let Some(p) = present(p) else {
                return Some((a, Source::Held));
            };
            let from = known_until(complete, tp, ta);
            let span = (ta - from) as f32;
            let past = at - ta * 1000;
            let ahead = past.min(max_extrapolation) as f32 / 1000.0;
            let velocity = (a.translation - p.translation) / span;
            let moved = a.translation + velocity * ahead;
            let source = if past <= max_extrapolation {
                Source::Extrapolated
            } else {
                Source::Held
            };
            if !moved.is_finite() {
                return Some((a, Source::Held));
            }
            Some((
                Transform {
                    translation: moved,
                    ..a
                },
                source,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use super::*;
    use crate::value::Value;
    use wish_core::math::Vec3;

    fn state(x: f32) -> Option<Arc<EntityView>> {
        let transform = Transform::from_translation(Vec3::new(x, 0.0, 0.0));
        let mut bytes = vec![0; std::mem::size_of::<Transform>()];
        transform.encode(&mut bytes);
        let mut view = EntityView::new();
        view.insert(
            0,
            Arc::new(Value {
                bytes,
                texts: Vec::new(),
                refs: Vec::new(),
            }),
        );
        Some(Arc::new(view))
    }

    fn x(sampled: Option<(Transform, Source)>) -> (f32, Source) {
        let (t, s) = sampled.unwrap();
        (t.translation.x, s)
    }

    #[test]
    fn between_two_states_it_blends() {
        let states: States = [(10, state(0.0)), (20, state(10.0))].into();
        let none = BTreeSet::new();
        assert_eq!(
            x(sample(&states, &none, 0, 15_000, 0)),
            (5.0, Source::Interpolated)
        );
        assert_eq!(x(sample(&states, &none, 0, 5_000, 0)), (0.0, Source::Held));
    }

    #[test]
    fn a_complete_tick_says_it_was_still_until_then() {
        let states: States = [(10, state(0.0)), (20, state(10.0))].into();
        let complete: BTreeSet<u64> = [15].into();
        // Still at 0 up to tick 15, then moving to 10 by tick 20.
        assert_eq!(
            x(sample(&states, &complete, 0, 14_000, 0)),
            (0.0, Source::Interpolated)
        );
        assert_eq!(
            x(sample(&states, &complete, 0, 17_500, 0)),
            (5.0, Source::Interpolated)
        );
    }

    #[test]
    fn past_the_newest_state_it_extrapolates_a_little_then_holds() {
        let states: States = [(10, state(0.0)), (11, state(1.0))].into();
        let none = BTreeSet::new();
        assert_eq!(
            x(sample(&states, &none, 0, 13_000, 6_000)),
            (3.0, Source::Extrapolated)
        );
        assert_eq!(
            x(sample(&states, &none, 0, 30_000, 6_000)),
            (7.0, Source::Held)
        );
        // Known to be still: held at once.
        let complete: BTreeSet<u64> = [14].into();
        assert_eq!(
            x(sample(&states, &complete, 0, 13_000, 6_000)),
            (1.0, Source::Interpolated)
        );
        assert_eq!(
            x(sample(&states, &complete, 0, 16_000, 6_000)),
            (1.0, Source::Held)
        );
    }

    #[test]
    fn rotations_take_the_shorter_way() {
        let a = Transform::default();
        let b = Transform {
            rotation: -a.rotation,
            ..Transform::default()
        };
        let mid = blend(&a, &b, 0.5);
        assert!((mid.rotation.length() - 1.0).abs() < 1e-5);
        assert!(mid.rotation.w.abs() > 0.99);
    }
}
