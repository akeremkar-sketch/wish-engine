//! Line of sight from game code (design note `docs/design/1.1-lights-2d.md`
//! §5): "can A see B" and "what is visible from here", in the X–Y plane,
//! against walls (the solid tiles of tilemaps' colliding layers, and
//! entities with an `Occluder` and a `Collider`).
//!
//! The answers are the engine's: the same that online play's
//! `NetVisibility::Sight` and the shadows of `Light2D`s use. They see the
//! walls as they were at the end of the previous tick, so they do not
//! depend on the order of systems.
//!
//! The request and answer encodings are shared by both sides of the ABI and
//! travel through the same import as ray casts (`wish.physics_query`), with
//! their own first byte.

use wish_core::Entity;
use wish_core::math::Vec2;

/// The first byte of a "can see" request.
pub const CAN_SEE: u8 = 2;
/// The first byte of a "visible area" request.
pub const VISIBLE_AREA: u8 = 3;
/// The size of an encoded "can see" request.
pub const CAN_SEE_LEN: usize = 1 + 16 + 1 + 8;
/// The size of an encoded "can see" answer.
pub const CAN_SEE_ANSWER_LEN: usize = 1 + 1 + 8;
/// The size of an encoded "visible area" request.
pub const VISIBLE_AREA_LEN: usize = 1 + 12;
/// The most points a visible area's answer carries (the engine's polygons
/// have at most 8 128).
pub const MAX_AREA_POINTS: usize = 8192;
/// The largest "visible area" answer.
pub const VISIBLE_AREA_ANSWER_MAX: usize = 4 + MAX_AREA_POINTS * 8;

/// A "can see" question: is the straight line from `from` to `to` free of
/// walls?
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SightLine {
    /// Where the looking starts (world X and Y).
    pub from: Vec2,
    /// What is looked at (world X and Y).
    pub to: Vec2,
    /// An entity whose own outline does not block (usually the one looked
    /// at, when it is itself an occluder, such as a door).
    pub ignore: Option<Entity>,
}

/// The answer to a "can see" question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SightAnswer {
    /// Whether nothing blocks the line.
    pub visible: bool,
    /// When something does: the entity whose wall is first in the way (a
    /// tilemap or an occluder).
    pub blocked_by: Option<Entity>,
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

impl SightLine {
    /// The request bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CAN_SEE_LEN);
        out.push(CAN_SEE);
        for x in [self.from.x, self.from.y, self.to.x, self.to.y] {
            out.extend(x.to_le_bytes());
        }
        out.push(u8::from(self.ignore.is_some()));
        out.extend(self.ignore.map_or(0, Entity::to_bits).to_le_bytes());
        out
    }

    /// Reads request bytes; `None` if they are not a "can see" request.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CAN_SEE_LEN || bytes[0] != CAN_SEE {
            return None;
        }
        Some(Self {
            from: Vec2::new(f32_at(bytes, 1)?, f32_at(bytes, 5)?),
            to: Vec2::new(f32_at(bytes, 9)?, f32_at(bytes, 13)?),
            ignore: if bytes[17] != 0 {
                Some(Entity::from_bits(u64_at(bytes, 18)?))
            } else {
                None
            },
        })
    }
}

impl SightAnswer {
    /// The answer bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CAN_SEE_ANSWER_LEN);
        out.push(u8::from(self.visible));
        out.push(u8::from(self.blocked_by.is_some()));
        out.extend(self.blocked_by.map_or(0, Entity::to_bits).to_le_bytes());
        out
    }

    /// Reads answer bytes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CAN_SEE_ANSWER_LEN {
            return None;
        }
        Some(Self {
            visible: bytes[0] != 0,
            blocked_by: if bytes[1] != 0 {
                Some(Entity::from_bits(u64_at(bytes, 2)?))
            } else {
                None
            },
        })
    }
}

/// A "visible area" question: what can be seen from `from` within
/// `radius` meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaQuestion {
    /// Where the looking starts (world X and Y).
    pub from: Vec2,
    /// How far, in meters.
    pub radius: f32,
}

impl AreaQuestion {
    /// The request bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(VISIBLE_AREA_LEN);
        out.push(VISIBLE_AREA);
        for x in [self.from.x, self.from.y, self.radius] {
            out.extend(x.to_le_bytes());
        }
        out
    }

    /// Reads request bytes; `None` if they are not a "visible area" request.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != VISIBLE_AREA_LEN || bytes[0] != VISIBLE_AREA {
            return None;
        }
        Some(Self {
            from: Vec2::new(f32_at(bytes, 1)?, f32_at(bytes, 5)?),
            radius: f32_at(bytes, 9)?,
        })
    }
}

/// The answer bytes for a visible area (at most [`MAX_AREA_POINTS`] points;
/// more are left out).
pub fn encode_area(points: &[[f32; 2]]) -> Vec<u8> {
    let points = &points[..points.len().min(MAX_AREA_POINTS)];
    let mut out = Vec::with_capacity(4 + points.len() * 8);
    out.extend((points.len() as u32).to_le_bytes());
    for [x, y] in points {
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
    }
    out
}

/// Reads a visible area's answer bytes.
pub fn decode_area(bytes: &[u8]) -> Option<Vec<Vec2>> {
    let count = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
    if count > MAX_AREA_POINTS || bytes.len() != 4 + count * 8 {
        return None;
    }
    (0..count)
        .map(|i| {
            Some(Vec2::new(
                f32_at(bytes, 4 + i * 8)?,
                f32_at(bytes, 8 + i * 8)?,
            ))
        })
        .collect()
}

/// Whether nothing blocks the straight line from `from` to `to` in the X–Y
/// plane: no wall of a tilemap and no `Occluder`. Touching a wall's edge or
/// corner counts as blocked. `false` when the engine cannot answer.
pub fn can_see(from: Vec2, to: Vec2) -> bool {
    look(SightLine {
        from,
        to,
        ignore: None,
    })
    .visible
}

/// [`can_see`], where the outline of `ignore` does not block (an entity
/// looked at that is itself an occluder, such as a door).
pub fn can_see_ignoring(from: Vec2, to: Vec2, ignore: Entity) -> bool {
    look(SightLine {
        from,
        to,
        ignore: Some(ignore),
    })
    .visible
}

/// The full answer to a "can see" question: whether it is visible, and the
/// entity whose wall is first in the way if not.
pub fn look(line: SightLine) -> SightAnswer {
    crate::abi::physics_query_into(&line.encode(), CAN_SEE_ANSWER_LEN)
        .and_then(|answer| SightAnswer::decode(&answer))
        .unwrap_or(SightAnswer {
            visible: false,
            blocked_by: None,
        })
}

/// What can be seen from `from` within `radius` meters: the outline of the
/// visible area in the X–Y plane, as points in order around `from` (a
/// circle cut by walls). Empty when the engine cannot answer. At most
/// 8192 points (a polygon takes at most the 1000 walls nearest in order
/// into account).
pub fn visible_area(from: Vec2, radius: f32) -> Vec<Vec2> {
    let question = AreaQuestion { from, radius };
    crate::abi::physics_query_into(&question.encode(), VISIBLE_AREA_ANSWER_MAX)
        .and_then(|answer| decode_area(&answer))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn requests_and_answers_round_trip() {
        let line = SightLine {
            from: Vec2::new(1.0, 2.0),
            to: Vec2::new(-3.5, 4.0),
            ignore: Some(Entity::from_bits(7 | (2 << 32))),
        };
        assert_eq!(SightLine::decode(&line.encode()), Some(line));
        let plain = SightLine {
            ignore: None,
            ..line
        };
        assert_eq!(SightLine::decode(&plain.encode()), Some(plain));
        let answer = SightAnswer {
            visible: false,
            blocked_by: Some(Entity::from_bits(3)),
        };
        assert_eq!(SightAnswer::decode(&answer.encode()), Some(answer));
        let area = AreaQuestion {
            from: Vec2::new(0.5, -1.0),
            radius: 6.0,
        };
        assert_eq!(AreaQuestion::decode(&area.encode()), Some(area));
        let points = [[1.0, 2.0], [3.0, -4.0]];
        assert_eq!(
            decode_area(&encode_area(&points)),
            Some(vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, -4.0)])
        );
        assert_eq!(decode_area(&[1, 0, 0, 0]), None);
        assert_eq!(SightLine::decode(&area.encode()), None);
        // Off the engine nothing can answer.
        assert!(!can_see(Vec2::ZERO, Vec2::X));
        assert!(visible_area(Vec2::ZERO, 3.0).is_empty());
    }
}
