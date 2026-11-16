//! Commands: changes to the world a system asks for, applied by the engine
//! after the call, in the order they were given (ADR-005).
//!
//! Encoding (ABI v1): `wish_run` returns a pointer to `u32 len` followed by
//! `len` bytes of commands. Each command is an op byte and its operands;
//! types are named, entities are either existing (`0` + `u64` bits) or new
//! in this call (`1` + `u32` index, counting spawns from 0).

use wish_core::Entity;

/// An entity a command refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// An entity that already exists.
    Existing(Entity),
    /// An entity spawned earlier in the same call (the n-th spawn).
    New(u32),
}

impl From<Entity> for Target {
    fn from(entity: Entity) -> Self {
        Self::Existing(entity)
    }
}

/// One command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Spawns an empty entity; later commands call it `Target::New(index)`.
    Spawn {
        /// Which spawn of the call this is.
        index: u32,
    },
    /// Despawns an entity.
    Despawn(Target),
    /// Adds or replaces a component.
    Insert {
        /// The entity.
        target: Target,
        /// The component's type name.
        component: String,
        /// The value in the type's byte layout.
        bytes: Vec<u8>,
    },
    /// Removes a component.
    Remove {
        /// The entity.
        target: Target,
        /// The component's type name.
        component: String,
    },
    /// Sends an event.
    Send {
        /// The event's type name.
        event: String,
        /// The value in the type's byte layout.
        bytes: Vec<u8>,
    },
    /// Makes `child` a child of `parent`.
    SetParent {
        /// The child.
        child: Target,
        /// The new parent.
        parent: Target,
    },
    /// Makes an entity a root.
    RemoveParent(Target),
}

const SPAWN: u8 = 1;
const DESPAWN: u8 = 2;
const INSERT: u8 = 3;
const REMOVE: u8 = 4;
const SEND: u8 = 5;
const SET_PARENT: u8 = 6;
const REMOVE_PARENT: u8 = 7;

fn put_target(out: &mut Vec<u8>, target: Target) {
    match target {
        Target::Existing(entity) => {
            out.push(0);
            out.extend(entity.to_bits().to_le_bytes());
        }
        Target::New(index) => {
            out.push(1);
            out.extend(index.to_le_bytes());
        }
    }
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    let len = u16::try_from(name.len()).unwrap_or(u16::MAX);
    out.extend(len.to_le_bytes());
    out.extend(&name.as_bytes()[..usize::from(len)]);
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}

impl Command {
    /// Appends the command's encoding.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Spawn { index } => {
                out.push(SPAWN);
                out.extend(index.to_le_bytes());
            }
            Self::Despawn(target) => {
                out.push(DESPAWN);
                put_target(out, *target);
            }
            Self::Insert {
                target,
                component,
                bytes,
            } => {
                out.push(INSERT);
                put_target(out, *target);
                put_name(out, component);
                put_bytes(out, bytes);
            }
            Self::Remove { target, component } => {
                out.push(REMOVE);
                put_target(out, *target);
                put_name(out, component);
            }
            Self::Send { event, bytes } => {
                out.push(SEND);
                put_name(out, event);
                put_bytes(out, bytes);
            }
            Self::SetParent { child, parent } => {
                out.push(SET_PARENT);
                put_target(out, *child);
                put_target(out, *parent);
            }
            Self::RemoveParent(child) => {
                out.push(REMOVE_PARENT);
                put_target(out, *child);
            }
        }
    }
}

/// Reads commands written by a module. Every length is checked: hostile
/// bytes give an error naming what broke, never a panic.
pub fn decode_commands(mut bytes: &[u8], max: usize) -> Result<Vec<Command>, String> {
    let mut commands = Vec::new();
    while let Some((&op, rest)) = bytes.split_first() {
        if commands.len() >= max {
            return Err(format!("there are more than {max} commands"));
        }
        bytes = rest;
        let command = match op {
            SPAWN => Command::Spawn {
                index: read_u32(&mut bytes)?,
            },
            DESPAWN => Command::Despawn(read_target(&mut bytes)?),
            INSERT => Command::Insert {
                target: read_target(&mut bytes)?,
                component: read_name(&mut bytes)?,
                bytes: read_value(&mut bytes)?,
            },
            REMOVE => Command::Remove {
                target: read_target(&mut bytes)?,
                component: read_name(&mut bytes)?,
            },
            SEND => Command::Send {
                event: read_name(&mut bytes)?,
                bytes: read_value(&mut bytes)?,
            },
            SET_PARENT => Command::SetParent {
                child: read_target(&mut bytes)?,
                parent: read_target(&mut bytes)?,
            },
            REMOVE_PARENT => Command::RemoveParent(read_target(&mut bytes)?),
            other => {
                return Err(format!(
                    "command {} has the unknown op {other}",
                    commands.len()
                ));
            }
        };
        commands.push(command);
    }
    Ok(commands)
}

fn read_u32(bytes: &mut &[u8]) -> Result<u32, String> {
    let b = read_n(bytes, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_target(bytes: &mut &[u8]) -> Result<Target, String> {
    let tag = read_n(bytes, 1)?;
    match tag[0] {
        0 => {
            let b = read_n(bytes, 8)?;
            let bits = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            Ok(Target::Existing(Entity::from_bits(bits)))
        }
        1 => Ok(Target::New(read_u32(bytes)?)),
        other => Err(format!("unknown entity tag {other}")),
    }
}

fn read_n(bytes: &mut &[u8], n: usize) -> Result<Vec<u8>, String> {
    if bytes.len() < n {
        return Err("a command is cut short".to_owned());
    }
    let (head, rest) = bytes.split_at(n);
    *bytes = rest;
    Ok(head.to_vec())
}

fn read_name(bytes: &mut &[u8]) -> Result<String, String> {
    let len = read_n(bytes, 2)?;
    let len = usize::from(u16::from_le_bytes([len[0], len[1]]));
    String::from_utf8(read_n(bytes, len)?).map_err(|_| "a type name is not UTF-8".to_owned())
}

fn read_value(bytes: &mut &[u8]) -> Result<Vec<u8>, String> {
    let len = read_u32(bytes)? as usize;
    read_n(bytes, len)
}
