//! What enters the host's world at a tick boundary, as data: the
//! [`TickRecord`], the one function that writes it ([`apply`]) and the
//! host's input log, [`MatchLog`] (design note §2, §3.7).
//!
//! The live host works a record out without writing anything, then calls
//! `apply`; a replay calls the same `apply` with the recorded record before
//! each step of a fresh host app. The world hash covers change ticks, so the
//! two must make the same writes in the same order, and they do: it is the
//! same function on the same data.

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ChangeTick, World};
use wish_replicate::input::{self, Actions};
use wish_replicate::wire::{Reader, put_varint};
use wish_types::{LobbyClosed, MAX_ACTIONS, MAX_PLAYERS, PlayerId, Players};

use crate::lobby::{LobbyState, LocalState, MAX_ROLE_NAME, write_roles};
use crate::message::{MessageType, MessageValue, message_types};
use crate::wire::{self, Down};

/// One message delivered into the host's world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    /// The event type's name.
    pub ty: String,
    /// The player who sent it (0: the host's own game code).
    pub from: PlayerId,
    /// Its value, the envelope as delivered (`from`, `delivered` set).
    pub value: MessageValue,
}

/// Everything that entered the host's world at one tick boundary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickRecord {
    /// The tick about to be simulated.
    pub tick: u64,
    /// `Players.count` was raised to this.
    pub players: Option<u32>,
    /// The host's own player's actions, when they changed (else as before).
    pub local: Option<Actions>,
    /// Every connected player's input frame in force, when any changed
    /// (else as before).
    pub remote: Option<BTreeMap<PlayerId, Actions>>,
    /// The lobby's new value, when it changed.
    pub lobby: Option<LobbyState>,
    /// This machine's (the host's) new `LocalPlayer`, when it changed.
    pub local_player: Option<LocalState>,
    /// Every player's role, when they changed.
    pub roles: Option<[u32; MAX_PLAYERS]>,
    /// The messages delivered, in order.
    pub messages: Vec<Delivered>,
}

impl TickRecord {
    /// A record of `tick` with nothing new.
    pub fn empty(tick: u64) -> Self {
        Self {
            tick,
            ..Self::default()
        }
    }

    /// Whether nothing new entered.
    pub fn is_empty(&self) -> bool {
        self.players.is_none()
            && self.local.is_none()
            && self.remote.is_none()
            && self.lobby.is_none()
            && self.local_player.is_none()
            && self.roles.is_none()
            && self.messages.is_empty()
    }
}

/// What stays in force from one boundary to the next: the actions, and the
/// message types of the world (looked up once).
#[derive(Debug, Default)]
pub struct Carry {
    /// The host's own actions in force.
    pub local: Actions,
    /// The players' actions in force.
    pub remote: BTreeMap<PlayerId, Actions>,
    types: Option<BTreeMap<String, MessageType>>,
}

impl Carry {
    /// Nothing in force.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Writes `record` into `world`: the player count, the host's own actions
/// (`ActionState`), every player's (`PlayerActionState`), the lobby, the
/// local player, the roles, then every message as an event; then advances
/// the world's change tick once. Returns the change tick the writes were
/// made at (events the systems send afterwards are newer) and the messages
/// that could not be delivered.
pub fn apply(
    world: &mut World,
    record: &TickRecord,
    carry: &mut Carry,
) -> Result<(ChangeTick, Vec<Diagnostic>)> {
    let mark = world.change_tick();
    if let Some(count) = record.players {
        input::ensure_players(world, count)?;
    }
    if let Some(local) = &record.local {
        carry.local = local.clone();
    }
    if let Some(remote) = &record.remote {
        carry.remote = remote.clone();
    }
    input::write_local(world, &carry.local)?;
    let remote = &carry.remote;
    input::write_players(world, &|player| remote.get(&player).cloned())?;
    if let Some(lobby) = &record.lobby {
        lobby.write(world)?;
    }
    if let Some(local) = &record.local_player {
        local.write(world)?;
    }
    if let Some(roles) = &record.roles {
        write_roles(world, roles)?;
    }
    let mut problems = Vec::new();
    if !record.messages.is_empty() {
        let types = carry.types.get_or_insert_with(|| {
            message_types(world)
                .0
                .into_iter()
                .map(|t| (t.name.clone(), t))
                .collect()
        });
        for message in &record.messages {
            let Some(ty) = types.get(&message.ty) else {
                problems.push(Diagnostic::warning(
                    "net_types_mismatch",
                    format!("a message of type {:?} was delivered, but this world has no such message type", message.ty),
                    "Replay a match with the same build of the game that hosted it.",
                ));
                continue;
            };
            let sent = message
                .value
                .write(world, ty)
                .and_then(|bytes| world.send_bytes(ty.id, &bytes));
            if let Err(problem) = sent {
                problems.push(problem);
            }
        }
    }
    world.advance_change_tick();
    Ok((mark, problems))
}

/// The host's input log: every tick boundary that let anything in, in tick
/// order. With the same game, `apply` of these records before each step of a
/// fresh host app gives the live host's world at every tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchLog {
    /// The first tick recorded.
    pub first: Option<u64>,
    /// The last tick recorded.
    pub last: Option<u64>,
    /// The records that let anything in, by tick.
    pub records: BTreeMap<u64, TickRecord>,
}

/// "WLOG", the match log's magic.
const MAGIC: &[u8; 4] = b"WLOG";
/// The match log's format version.
pub const LOG_VERSION: u8 = 1;
/// Records a log decodes at most.
const MAX_RECORDS: usize = 1 << 24;
/// Messages one record holds at most.
const MAX_RECORD_MESSAGES: usize = 1 << 16;

impl MatchLog {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the record of the next tick (kept only when it let anything in).
    pub fn push(&mut self, record: TickRecord) {
        self.first.get_or_insert(record.tick);
        self.last = Some(record.tick);
        if !record.is_empty() {
            self.records.insert(record.tick, record);
        }
    }

    /// The record of `tick` (an empty one when nothing entered).
    pub fn record(&self, tick: u64) -> TickRecord {
        self.records
            .get(&tick)
            .cloned()
            .unwrap_or_else(|| TickRecord::empty(tick))
    }

    /// The ticks recorded, first to last.
    pub fn ticks(&self) -> std::ops::RangeInclusive<u64> {
        match (self.first, self.last) {
            (Some(first), Some(last)) => first..=last,
            #[allow(clippy::reversed_empty_ranges)]
            _ => 1..=0,
        }
    }

    /// The log in its binary form.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(LOG_VERSION);
        put_varint(&mut out, self.first.map_or(0, |t| t.saturating_add(1)));
        put_varint(&mut out, self.last.map_or(0, |t| t.saturating_add(1)));
        put_varint(&mut out, self.records.len() as u64);
        for record in self.records.values() {
            put_record(&mut out, record);
        }
        out
    }

    /// Reads a log's binary form; a diagnostic when it is not one.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        decode_log(bytes).ok_or_else(|| {
            Diagnostic::error(
                "net_message_invalid",
                "the match log is damaged or of another version",
                "Record the match again with this engine; a log is read only by the engine version that wrote it.",
            )
        })
    }
}

fn put_actions(out: &mut Vec<u8>, actions: &Actions) {
    let n = actions.len().min(MAX_ACTIONS);
    out.push(n as u8);
    for (id, [x, y]) in actions.iter().take(n) {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
    }
}

fn read_actions(reader: &mut Reader<'_>) -> Option<Actions> {
    let n = usize::from(reader.u8()?);
    if n > MAX_ACTIONS || n * 12 > reader.remaining() {
        return None;
    }
    let mut actions = Vec::with_capacity(n);
    for _ in 0..n {
        actions.push((reader.u32()?, [reader.f32()?, reader.f32()?]));
    }
    Some(actions)
}

fn put_str(out: &mut Vec<u8>, text: &str) {
    put_varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

fn read_str(reader: &mut Reader<'_>, max: usize) -> Option<String> {
    let len = usize::try_from(reader.varint()?).ok()?;
    if len > max {
        return None;
    }
    Some(std::str::from_utf8(reader.take(len)?).ok()?.to_owned())
}

fn closed_code(closed: LobbyClosed) -> u8 {
    match closed {
        LobbyClosed::None => 0,
        LobbyClosed::HostLeft => 1,
        LobbyClosed::Refused => 2,
        LobbyClosed::Lost => 3,
        LobbyClosed::Left => 4,
    }
}

fn closed_from(code: u8) -> Option<LobbyClosed> {
    Some(match code {
        0 => LobbyClosed::None,
        1 => LobbyClosed::HostLeft,
        2 => LobbyClosed::Refused,
        3 => LobbyClosed::Lost,
        4 => LobbyClosed::Left,
        _ => return None,
    })
}

fn put_record(out: &mut Vec<u8>, record: &TickRecord) {
    put_varint(out, record.tick);
    let flags = u8::from(record.players.is_some())
        | u8::from(record.local.is_some()) << 1
        | u8::from(record.remote.is_some()) << 2
        | u8::from(record.lobby.is_some()) << 3
        | u8::from(record.local_player.is_some()) << 4
        | u8::from(record.roles.is_some()) << 5;
    out.push(flags);
    if let Some(count) = record.players {
        put_varint(out, u64::from(count));
    }
    if let Some(local) = &record.local {
        put_actions(out, local);
    }
    if let Some(remote) = &record.remote {
        put_varint(out, remote.len() as u64);
        for (player, actions) in remote {
            put_varint(out, u64::from(*player));
            put_actions(out, actions);
        }
    }
    if let Some(lobby) = &record.lobby {
        let body = wire::encode_down(&Down::Lobby(Box::new(lobby.clone())));
        put_varint(out, body.len() as u64);
        out.extend_from_slice(&body);
        out.push(closed_code(lobby.closed));
    }
    if let Some(local) = &record.local_player {
        put_varint(out, u64::from(local.player));
        out.push(u8::from(local.joined) | u8::from(local.host) << 1);
        put_varint(out, u64::from(local.role));
        put_str(out, &local.role_name);
    }
    if let Some(roles) = &record.roles {
        for role in roles {
            put_varint(out, u64::from(*role));
        }
    }
    put_varint(out, record.messages.len() as u64);
    for message in &record.messages {
        put_str(out, &message.ty);
        put_varint(out, u64::from(message.from));
        put_varint(out, message.value.bytes.len() as u64);
        out.extend_from_slice(&message.value.bytes);
        put_varint(out, message.value.texts.len() as u64);
        for text in &message.value.texts {
            put_str(out, text);
        }
        put_varint(out, message.value.refs.len() as u64);
        for entity in &message.value.refs {
            out.extend_from_slice(&entity.to_bits().to_le_bytes());
        }
    }
}

fn read_record(reader: &mut Reader<'_>) -> Option<TickRecord> {
    let tick = reader.varint()?;
    let flags = reader.u8()?;
    if flags & !0x3F != 0 {
        return None;
    }
    let mut record = TickRecord::empty(tick);
    if flags & 1 != 0 {
        record.players = Some(reader.varint_u32()?);
    }
    if flags & 2 != 0 {
        record.local = Some(read_actions(reader)?);
    }
    if flags & 4 != 0 {
        let n = reader.count(MAX_PLAYERS, 2)?;
        let mut remote = BTreeMap::new();
        for _ in 0..n {
            let player = reader.varint_u32()?;
            remote.insert(player, read_actions(reader)?);
        }
        record.remote = Some(remote);
    }
    if flags & 8 != 0 {
        let len = reader.count(64 * 1024, 1)?;
        let body = reader.take(len)?;
        let Some(Down::Lobby(mut lobby)) = wire::decode_down(body) else {
            return None;
        };
        lobby.closed = closed_from(reader.u8()?)?;
        record.lobby = Some(*lobby);
    }
    if flags & 16 != 0 {
        let player = reader.varint_u32()?;
        let bits = reader.u8()?;
        if bits & !3 != 0 {
            return None;
        }
        let role = reader.varint_u32()?;
        let role_name = read_str(reader, MAX_ROLE_NAME)?;
        record.local_player = Some(LocalState {
            player,
            joined: bits & 1 != 0,
            host: bits & 2 != 0,
            role,
            role_name,
        });
    }
    if flags & 32 != 0 {
        let mut roles = [0u32; MAX_PLAYERS];
        for role in &mut roles {
            *role = reader.varint_u32()?;
        }
        record.roles = Some(roles);
    }
    let n = reader.count(MAX_RECORD_MESSAGES, 5)?;
    for _ in 0..n {
        let ty = read_str(reader, 128)?;
        let from = reader.varint_u32()?;
        let len = reader.count(1 << 20, 1)?;
        let bytes = reader.take(len)?.to_vec();
        let text_count = reader.count(1 << 16, 1)?;
        let mut texts = Vec::with_capacity(text_count);
        for _ in 0..text_count {
            texts.push(read_str(reader, 64 * 1024)?);
        }
        let ref_count = reader.count(1 << 16, 8)?;
        let mut refs = Vec::with_capacity(ref_count);
        for _ in 0..ref_count {
            let image: [u8; 8] = reader.take(8)?.try_into().ok()?;
            refs.push(Entity::from_bits(u64::from_le_bytes(image)));
        }
        record.messages.push(Delivered {
            ty,
            from,
            value: MessageValue { bytes, texts, refs },
        });
    }
    Some(record)
}

fn decode_log(bytes: &[u8]) -> Option<MatchLog> {
    let mut reader = Reader::new(bytes);
    if reader.take(4)? != MAGIC || reader.u8()? != LOG_VERSION {
        return None;
    }
    let first = reader.varint()?.checked_sub(1);
    let last = reader.varint()?.checked_sub(1);
    let n = reader.count(MAX_RECORDS, 3)?;
    let mut records = BTreeMap::new();
    for _ in 0..n {
        let record = read_record(&mut reader)?;
        if records
            .keys()
            .next_back()
            .is_some_and(|t| *t >= record.tick)
        {
            return None;
        }
        records.insert(record.tick, record);
    }
    reader.done().then_some(MatchLog {
        first,
        last,
        records,
    })
}

/// The number of players a world has now (`Players.count`, 1 without it).
pub fn player_count(world: &World) -> u32 {
    world.resource::<Players>().map_or(1, |p| p.count)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use wish_core::Rng;
    use wish_types::LobbyPhase;

    fn sample() -> MatchLog {
        let mut log = MatchLog::new();
        log.push(TickRecord::empty(1));
        let mut lobby = LobbyState {
            phase: LobbyPhase::Open,
            room: "ROOM42".into(),
            closed: LobbyClosed::HostLeft,
            ..LobbyState::default()
        };
        lobby.seats[0].taken = true;
        lobby.seats[0].name = "Host".into();
        log.push(TickRecord {
            tick: 2,
            players: Some(3),
            local: Some(vec![(5, [1.0, -0.5])]),
            remote: Some([(1, vec![(5, [0.0, 1.0])]), (2, Vec::new())].into()),
            lobby: Some(lobby),
            local_player: Some(LocalState {
                player: 0,
                joined: true,
                host: true,
                role: 1,
                role_name: "saboteur".into(),
            }),
            roles: Some([1; MAX_PLAYERS]),
            messages: vec![Delivered {
                ty: "Vote".into(),
                from: 2,
                value: MessageValue {
                    bytes: vec![1, 2, 3, 4],
                    texts: vec!["hi".into()],
                    refs: vec![Entity::new(3, 1)],
                },
            }],
        });
        log.push(TickRecord::empty(3));
        log
    }

    #[test]
    fn a_log_round_trips_through_bytes() {
        let log = sample();
        assert_eq!(log.records.len(), 1, "empty ticks are not stored");
        assert_eq!(log.ticks(), 1..=3);
        let bytes = log.to_bytes();
        assert_eq!(MatchLog::from_bytes(&bytes).unwrap(), log);
        assert!(MatchLog::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn damaged_logs_never_panic() {
        let bytes = sample().to_bytes();
        let mut rng = Rng::from_seed(3);
        for _ in 0..20_000 {
            let mut copy = bytes.clone();
            match rng.below(3) {
                0 => {
                    let at = rng.below(copy.len() as u32) as usize;
                    copy[at] ^= 1 << rng.below(8);
                }
                1 => copy.truncate(rng.below(copy.len() as u32) as usize),
                _ => copy.extend((0..rng.below(16)).map(|_| rng.next_u32() as u8)),
            }
            let _ = MatchLog::from_bytes(&copy);
        }
    }
}
