//! The SDK's side of the frame, run natively (and under Miri): queries
//! decode their columns and write back only `&mut` ones, `ResMut` writes
//! back, events decode, commands encode and decode, hostile frames and
//! command bytes are refused.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::Entity;
use wish_core::math::Vec3;
use wish_schema::Plain;
use wish_sdk::command::{Command, Target, decode_commands};
use wish_sdk::frame::{FrameReader, FrameWriter, query_layout};
use wish_sdk::prelude::*;
use wish_sdk::{EventReader, SystemParam};

/// How fast.
#[component]
struct Speed {
    /// Meters per second.
    value: f32,
}

/// Something happened.
#[event]
struct Ping {
    /// How loud.
    level: u32,
}

/// A counter.
#[resource]
struct Counter {
    /// The count.
    n: u64,
}

fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    let mut out = vec![0u8; size_of::<T>()];
    value.encode(&mut out);
    out
}

fn query_section(entities: &[Entity], transforms: &[Transform], speeds: &[Speed]) -> Vec<u8> {
    let sizes = [size_of::<Transform>(), size_of::<Speed>()];
    let (len, offsets) = query_layout(entities.len(), &sizes);
    let mut out = vec![0u8; len];
    out[0..4].copy_from_slice(&(entities.len() as u32).to_le_bytes());
    out[4..8].copy_from_slice(&2u32.to_le_bytes());
    for (row, entity) in entities.iter().enumerate() {
        out[8 + row * 8..16 + row * 8].copy_from_slice(&entity.to_bits().to_le_bytes());
    }
    for (row, t) in transforms.iter().enumerate() {
        let at = offsets[0] + row * sizes[0];
        out[at..at + sizes[0]].copy_from_slice(&bytes_of(t));
    }
    for (row, s) in speeds.iter().enumerate() {
        let at = offsets[1] + row * sizes[1];
        out[at..at + sizes[1]].copy_from_slice(&bytes_of(s));
    }
    out
}

#[test]
fn a_query_reads_its_columns_and_writes_back_what_is_mutable() {
    let entities = [Entity::new(3, 0), Entity::new(7, 2)];
    let transforms = [
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
    ];
    let speeds = [Speed { value: 10.0 }, Speed { value: 20.0 }];
    let mut writer = FrameWriter::new();
    writer.section(query_section(&entities, &transforms, &speeds));
    writer.section(bytes_of(&Counter { n: 41 }));
    let mut events = vec![0u8; 8 + 2 * size_of::<Ping>()];
    events[0..4].copy_from_slice(&2u32.to_le_bytes());
    events[8..12].copy_from_slice(&5u32.to_le_bytes());
    events[12..16].copy_from_slice(&6u32.to_le_bytes());
    writer.section(events);
    let (mut frame, places) = writer.finish(9);
    let before = frame.clone();

    {
        let mut reader = FrameReader::from_buffer(&mut frame).unwrap();
        assert_eq!(reader.tick(), 9);
        let mut query =
            <Query<(Entity, &'static mut Transform, &'static Speed)> as SystemParam>::fetch(
                reader.next_section(),
            );
        assert_eq!(query.len(), 2);
        assert_eq!(query.entities(), entities);
        for (entity, transform, speed) in query.iter_mut() {
            assert!(entities.contains(&entity));
            transform.translation.x += speed.value;
        }
        let (_, t, s) = query.get(entities[1]).unwrap();
        assert_eq!((t.translation.x, s.value), (22.0, 20.0));
        let mut counter = <ResMut<Counter> as SystemParam>::fetch(reader.next_section());
        counter.n += 1;
        let pings = <EventReader<Ping> as SystemParam>::fetch(reader.next_section());
        let levels: Vec<u32> = pings.read().map(|p| p.level).collect();
        assert_eq!(levels, [5, 6]);
    }

    // Written back: the transforms and the counter; the speeds are as sent.
    let (start, len) = places[0];
    let section = &frame[start..start + len];
    let sizes = [size_of::<Transform>(), size_of::<Speed>()];
    let (_, offsets) = query_layout(2, &sizes);
    let t1 = Transform::decode(&section[offsets[0] + sizes[0]..offsets[0] + 2 * sizes[0]]).unwrap();
    assert_eq!(t1.translation.x, 22.0);
    assert_eq!(
        &section[offsets[1]..offsets[1] + 2 * sizes[1]],
        &before[start + offsets[1]..start + offsets[1] + 2 * sizes[1]]
    );
    let (start, _) = places[1];
    assert_eq!(
        Counter::decode(&frame[start..start + 8]).unwrap(),
        Counter { n: 42 }
    );
}

#[test]
fn commands_round_trip_and_garbage_is_refused() {
    wish_sdk::abi::take_commands();
    let mut commands = <Commands as SystemParam>::fetch(wish_sdk::frame::Section::EMPTY);
    let made = commands.spawn().insert(Speed { value: 3.0 }).id();
    commands.despawn(Entity::new(1, 1));
    commands.remove::<Speed>(Entity::new(2, 0));
    commands.set_parent(made, Entity::new(4, 0));
    let bytes = wish_sdk::abi::take_commands();
    let decoded = decode_commands(&bytes).unwrap();
    assert_eq!(
        decoded,
        [
            Command::Spawn { index: 0 },
            Command::Insert {
                target: Target::New(0),
                component: "Speed".to_owned(),
                bytes: bytes_of(&Speed { value: 3.0 }),
            },
            Command::Despawn(Target::Existing(Entity::new(1, 1))),
            Command::Remove {
                target: Target::Existing(Entity::new(2, 0)),
                component: "Speed".to_owned(),
            },
            Command::SetParent {
                child: Target::New(0),
                parent: Target::Existing(Entity::new(4, 0)),
            },
        ]
    );
    // Every prefix of a valid buffer but the empty one and the whole is cut
    // short or ends cleanly between commands; none of them panics.
    for end in 0..bytes.len() {
        let _ = decode_commands(&bytes[..end]);
    }
    assert!(decode_commands(&[99]).is_err());
    assert!(decode_commands(&[3, 0]).is_err());
    assert!(decode_commands(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255]).is_err());
}

#[test]
fn a_frame_that_does_not_fit_its_buffer_is_refused() {
    let mut writer = FrameWriter::new();
    writer.section(vec![1, 2, 3]);
    let (mut frame, _) = writer.finish(0);
    // The section claims to run past the end.
    frame[28..32].copy_from_slice(&1000u32.to_le_bytes());
    assert!(FrameReader::from_buffer(&mut frame).is_none());
    // Wrong magic.
    let (mut other, _) = FrameWriter::new().finish(0);
    other[0] = 0;
    assert!(FrameReader::from_buffer(&mut other).is_none());
    assert!(FrameReader::from_buffer(&mut [0u8; 4]).is_none());
}
