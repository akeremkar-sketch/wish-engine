//! Network messages in game code, run natively: `NetWriter` fills the
//! envelope and sends an ordinary event command, `NetReader` reads only the
//! copies the engine delivered (design note `docs/design/1.1-lobby.md`).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(unsafe_code)] // FrameReader::from_buffer is the only unsafe call here.

use wish_schema::Plain;
use wish_sdk::command::{Command, decode_commands};
use wish_sdk::frame::{FrameReader, FrameWriter};
use wish_sdk::prelude::*;
use wish_sdk::{NetReader, NetWriter, SystemParam};

/// A vote: a network message, with its envelope second.
#[event]
struct Vote {
    /// The round.
    round: u32,
    /// The envelope.
    net: Net,
    /// Who for.
    choice: PlayerId,
}

fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    let mut out = vec![0u8; size_of::<T>()];
    value.encode(&mut out);
    out
}

#[test]
fn a_writer_fills_the_envelope() {
    wish_sdk::abi::take_commands();
    let mut out = <NetWriter<Vote> as SystemParam>::fetch(wish_sdk::frame::Section::EMPTY);
    out.to_host(Vote {
        round: 1,
        choice: 3,
        ..Default::default()
    });
    out.to_player(
        4,
        Vote {
            round: 2,
            net: Net {
                from: 9,
                delivered: true,
                ..Net::default()
            },
            choice: 5,
        },
    );
    out.to_everyone(Vote::default());
    out.to_group(6, Vote::default());
    let commands = decode_commands(&wish_sdk::abi::take_commands(), usize::MAX).unwrap();
    let sent: Vec<Vote> = commands
        .iter()
        .map(|c| match c {
            Command::Send { event, bytes } => {
                assert_eq!(event, "Vote");
                Vote::decode(bytes).unwrap()
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(sent[0].net, Net::to_host());
    assert_eq!((sent[0].round, sent[0].choice), (1, 3));
    // Whatever the game wrote into the envelope, the writer's target wins
    // and `from` / `delivered` are the engine's to fill.
    assert_eq!(sent[1].net, Net::to_player(4));
    assert_eq!(sent[2].net.to, NetTo::Everyone);
    assert_eq!(sent[3].net, Net::to_group(6));
}

#[test]
fn a_reader_reads_only_delivered_messages() {
    let delivered = |choice, from| Vote {
        round: 1,
        net: Net {
            from,
            delivered: true,
            ..Net::to_host()
        },
        choice,
    };
    let outgoing = Vote {
        round: 1,
        net: Net::to_host(),
        choice: 99,
    };
    let rows = [delivered(1, 2), outgoing, delivered(3, 4)];
    let size = size_of::<Vote>();
    let mut section = vec![0u8; 8 + rows.len() * size];
    section[0..4].copy_from_slice(&(rows.len() as u32).to_le_bytes());
    for (row, vote) in rows.iter().enumerate() {
        section[8 + row * size..8 + (row + 1) * size].copy_from_slice(&bytes_of(vote));
    }
    let mut writer = FrameWriter::new();
    writer.section(section);
    let (mut frame, _) = writer.finish(7);
    // SAFETY: `frame` outlives the reader and its sections and is not
    // touched while they are used.
    let mut reader = unsafe { FrameReader::from_buffer(&mut frame) }.unwrap();
    let inbox = <NetReader<Vote> as SystemParam>::fetch(reader.next_section());
    let got: Vec<(PlayerId, PlayerId)> = inbox.read().map(|v| (v.net.from, v.choice)).collect();
    assert_eq!(got, [(2, 1), (4, 3)]);
    assert_eq!(inbox.len(), 2);
}
