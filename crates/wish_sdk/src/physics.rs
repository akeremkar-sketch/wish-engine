//! Physics questions from game code (design note M6 §3.4): a ray cast
//! answered by the engine during the call, against the physics world as it
//! was at the start of the tick (the same for every system, so the answer
//! does not depend on system order).
//!
//! The request and answer encodings are shared by both sides of the ABI,
//! like commands (`wish.physics_query(request, len, out, capacity) -> len`).

use wish_core::Entity;
use wish_core::math::Vec3;

/// What a ray hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The entity whose collider was hit.
    pub entity: Entity,
    /// Where, in world space.
    pub point: Vec3,
    /// The surface normal there.
    pub normal: Vec3,
    /// How far along the ray, in meters.
    pub distance: f32,
}

/// A ray to cast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    /// Where it starts.
    pub from: Vec3,
    /// Which way it goes (any length but zero).
    pub dir: Vec3,
    /// How far it reaches, in meters.
    pub max: f32,
    /// Only colliders whose layer shares a bit with this mask.
    pub mask: u32,
    /// An entity to ignore (usually the one asking).
    pub ignore: Option<Entity>,
}

/// The size of an encoded request.
pub const REQUEST_LEN: usize = 1 + 12 + 12 + 4 + 4 + 1 + 8;
/// The size of an encoded answer.
pub const ANSWER_LEN: usize = 1 + 8 + 12 + 12 + 4;

const RAY: u8 = 1;

fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    for x in [v.x, v.y, v.z] {
        out.extend(x.to_le_bytes());
    }
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn vec3_at(bytes: &[u8], at: usize) -> Option<Vec3> {
    Some(Vec3::new(
        f32_at(bytes, at)?,
        f32_at(bytes, at + 4)?,
        f32_at(bytes, at + 8)?,
    ))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

impl Ray {
    /// The request bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(REQUEST_LEN);
        out.push(RAY);
        put_vec3(&mut out, self.from);
        put_vec3(&mut out, self.dir);
        out.extend(self.max.to_le_bytes());
        out.extend(self.mask.to_le_bytes());
        out.push(u8::from(self.ignore.is_some()));
        out.extend(self.ignore.map_or(0, Entity::to_bits).to_le_bytes());
        out
    }

    /// Reads request bytes; `None` if they are not a ray request.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != REQUEST_LEN || bytes[0] != RAY {
            return None;
        }
        Some(Self {
            from: vec3_at(bytes, 1)?,
            dir: vec3_at(bytes, 13)?,
            max: f32_at(bytes, 25)?,
            mask: u32::from_le_bytes(bytes.get(29..33)?.try_into().ok()?),
            ignore: (bytes[33] != 0).then(|| u64_at(bytes, 34).map(Entity::from_bits))?,
        })
    }
}

/// The answer bytes for a hit or a miss.
pub fn encode_answer(hit: Option<RayHit>) -> Vec<u8> {
    let mut out = Vec::with_capacity(ANSWER_LEN);
    out.push(u8::from(hit.is_some()));
    let hit = hit.unwrap_or(RayHit {
        entity: Entity::from_bits(0),
        point: Vec3::ZERO,
        normal: Vec3::ZERO,
        distance: 0.0,
    });
    out.extend(hit.entity.to_bits().to_le_bytes());
    put_vec3(&mut out, hit.point);
    put_vec3(&mut out, hit.normal);
    out.extend(hit.distance.to_le_bytes());
    out
}

/// Reads answer bytes.
pub fn decode_answer(bytes: &[u8]) -> Option<RayHit> {
    if bytes.len() != ANSWER_LEN || bytes[0] == 0 {
        return None;
    }
    Some(RayHit {
        entity: Entity::from_bits(u64_at(bytes, 1)?),
        point: vec3_at(bytes, 9)?,
        normal: vec3_at(bytes, 21)?,
        distance: f32_at(bytes, 33)?,
    })
}

/// Casts a ray into the physics world: the first collider it hits within
/// `max` meters (sensors are not hit). Sees the world as it was at the start
/// of the tick; a character controller's collider is where it stood before
/// its last move.
pub fn raycast(from: Vec3, dir: Vec3, max: f32) -> Option<RayHit> {
    cast(Ray {
        from,
        dir,
        max,
        mask: u32::MAX,
        ignore: None,
    })
}

/// [`raycast`] with every option: a layer mask and an entity to ignore.
pub fn cast(ray: Ray) -> Option<RayHit> {
    let request = ray.encode();
    let answer = crate::abi::physics_query(&request)?;
    decode_answer(&answer)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn requests_and_answers_round_trip() {
        let ray = Ray {
            from: Vec3::new(1.0, 2.0, 3.0),
            dir: Vec3::NEG_Y,
            max: 7.5,
            mask: 0b101,
            ignore: Some(Entity::from_bits(42 | (3 << 32))),
        };
        assert_eq!(Ray::decode(&ray.encode()), Some(ray));
        let hit = RayHit {
            entity: Entity::from_bits(9),
            point: Vec3::new(1.0, 0.0, 3.0),
            normal: Vec3::Y,
            distance: 2.0,
        };
        assert_eq!(decode_answer(&encode_answer(Some(hit))), Some(hit));
        assert_eq!(decode_answer(&encode_answer(None)), None);
        assert_eq!(Ray::decode(&[1, 2, 3]), None);
    }
}
