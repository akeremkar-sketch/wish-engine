//! A small deterministic online game for the replication tests: one host
//! world and several client worlds stepped together over a `SimNetwork`.
//!
//! Every player has an avatar everyone sees (moved by that player's input)
//! and a secret only its owner sees. A treasure is seen only within 3 m, a
//! ghost light only by the ghosts' group (player 3), and a beacon everyone
//! sees refers to player 2's secret.

#![allow(dead_code)]

use std::collections::BTreeMap;

use wish_core::math::Vec3;
use wish_core::{Entity, Text};
use wish_ecs::{With, World};
use wish_net::{
    Config, Endpoint, Entropy, LinkConditions, RoomCode, SimNetwork, SimTransport, Transport,
};
use wish_replicate::{Client, ClientConfig, Host, ReplicationConfig, View};
use wish_schema::component;
use wish_types::{
    ActionSlot, ActionState, NetGroups, NetOwner, NetViewer, NetVisibility, PlayerActionSlots,
    PlayerActionState, PlayerId, Players, Replicated, Transform, action_id,
};

/// A player's body.
#[component]
pub struct Avatar {
    /// Its colour.
    pub color: u32,
    /// Its name.
    pub name: Text,
}

/// What only its owner may know.
#[component]
pub struct Secret {
    /// A number no other player may receive.
    pub marker: u64,
    /// A text no other player may receive.
    pub role: Text,
}

/// Seen only from near.
#[component]
pub struct Treasure {
    /// A number only players near it may receive.
    pub marker: u64,
}

/// Seen only by ghosts.
#[component]
pub struct Ghostly {
    /// A number only ghosts may receive.
    pub marker: u64,
}

/// Points at another entity.
#[component]
pub struct Follow {
    /// What it follows.
    pub target: Entity,
}

/// The replicated component types.
pub const TYPES: &[&str] = &[
    "Transform",
    "NetOwner",
    "Avatar",
    "Secret",
    "Treasure",
    "Ghostly",
    "Follow",
];

pub const SEED: u64 = 0x5EED;
pub const TICK_MS: u32 = 16;

/// The marker of player `p`'s secret.
pub fn secret_marker(player: PlayerId) -> u64 {
    0x5EC2_E700_0000_0000 | (u64::from(player) * 0x0101_0101)
}

/// The role text of player `p`'s secret.
pub fn secret_role(player: PlayerId) -> String {
    format!("SABOTEUR-OF-ROUND-{player}-ZQX")
}

pub const TREASURE_MARKER: u64 = 0x7EA5_17E5_0000_00AA;
pub const GHOST_MARKER: u64 = 0x6405_7000_0000_00BB;

pub fn room() -> RoomCode {
    RoomCode::parse("STAR42").unwrap()
}

pub fn net_config() -> Config {
    Config::new(room())
}

/// Registers what every world of the game has, in the same order.
pub fn register(world: &mut World) {
    world.register_component::<Transform>().unwrap();
    wish_replicate::register(world).unwrap();
    world.register_resource::<ActionState>().unwrap();
    world.register_resource::<Players>().unwrap();
    world.register_resource::<PlayerActionState>().unwrap();
    world.register_component::<Avatar>().unwrap();
    world.register_component::<Secret>().unwrap();
    world.register_component::<Treasure>().unwrap();
    world.register_component::<Ghostly>().unwrap();
    world.register_component::<Follow>().unwrap();
}

pub fn move_id() -> u32 {
    action_id("move")
}

/// The project's actions, at rest (the host's own player stays idle).
pub fn idle_actions() -> ActionState {
    let mut state = ActionState::default();
    state.slots[0] = ActionSlot {
        id: move_id(),
        ..ActionSlot::default()
    };
    state.slots[1] = ActionSlot {
        id: action_id("use"),
        ..ActionSlot::default()
    };
    state
}

/// What the host's world holds: the per-player entities, by player.
#[derive(Debug, Default, Clone)]
pub struct Cast {
    pub avatars: BTreeMap<PlayerId, Entity>,
    pub secrets: BTreeMap<PlayerId, Entity>,
    pub treasure: Option<Entity>,
    pub ghost: Option<Entity>,
    pub beacon: Option<Entity>,
}

/// The host's world with `players` players (ids 1 to `players`; the host's
/// own player 0 has no body).
pub fn host_world(players: u32) -> (World, Cast) {
    let mut world = World::new();
    register(&mut world);
    world.insert_resource(idle_actions()).unwrap();
    let mut groups = NetGroups::default();
    groups.players[3] = 1; // Player 3 is a ghost.
    world.insert_resource(groups).unwrap();
    let mut cast = Cast::default();
    for player in 1..=players {
        add_player(&mut world, &mut cast, player);
    }
    let treasure = world.spawn().unwrap();
    world
        .insert(
            treasure,
            Transform::from_translation(Vec3::new(4.0, 10.0, 0.0)),
        )
        .unwrap();
    world
        .insert(
            treasure,
            Replicated {
                visibility: NetVisibility::Near,
                radius: 3.0,
                ..Replicated::default()
            },
        )
        .unwrap();
    world
        .insert(
            treasure,
            Treasure {
                marker: TREASURE_MARKER,
            },
        )
        .unwrap();
    cast.treasure = Some(treasure);
    let ghost = world.spawn().unwrap();
    world
        .insert(
            ghost,
            Replicated {
                visibility: NetVisibility::Group,
                groups: 1,
                ..Replicated::default()
            },
        )
        .unwrap();
    world
        .insert(
            ghost,
            Ghostly {
                marker: GHOST_MARKER,
            },
        )
        .unwrap();
    cast.ghost = Some(ghost);
    let beacon = world.spawn().unwrap();
    world.insert(beacon, Replicated::default()).unwrap();
    world
        .insert(
            beacon,
            Follow {
                target: cast.secrets.get(&2).copied().unwrap_or(Entity::NULL),
            },
        )
        .unwrap();
    cast.beacon = Some(beacon);
    (world, cast)
}

/// Adds player `player`'s avatar and secret.
pub fn add_player(world: &mut World, cast: &mut Cast, player: PlayerId) {
    let avatar = world.spawn().unwrap();
    let name = world.intern(&format!("Player {player}")).unwrap();
    world
        .insert(
            avatar,
            Transform::from_translation(Vec3::new(0.0, player as f32 * 10.0, 0.0)),
        )
        .unwrap();
    world.insert(avatar, Replicated::default()).unwrap();
    world.insert(avatar, NetOwner { player }).unwrap();
    world.insert(avatar, NetViewer { player }).unwrap();
    world
        .insert(
            avatar,
            Avatar {
                color: 0xFF00_0000 | player,
                name,
            },
        )
        .unwrap();
    let secret = world.spawn().unwrap();
    let role = world.intern(&secret_role(player)).unwrap();
    world
        .insert(
            secret,
            Replicated {
                visibility: NetVisibility::Owner,
                ..Replicated::default()
            },
        )
        .unwrap();
    world.insert(secret, NetOwner { player }).unwrap();
    world
        .insert(
            secret,
            Secret {
                marker: secret_marker(player),
                role,
            },
        )
        .unwrap();
    cast.avatars.insert(player, avatar);
    cast.secrets.insert(player, secret);
}

/// The game's one system: avatars walk by their player's `move` action at
/// 3 m/s.
pub fn move_avatars(world: &mut World) {
    let actions = world.resource::<PlayerActionState>().copied();
    let id = move_id();
    for (owner, mut transform) in world
        .query_filtered::<(&NetOwner, &mut Transform), With<Avatar>>()
        .unwrap()
    {
        let slot = actions
            .as_ref()
            .and_then(|a| a.players.get(owner.player as usize))
            .and_then(|p: &PlayerActionSlots| p.slots.iter().find(|s| s.id == id).copied());
        if let Some(slot) = slot
            && slot.held
        {
            transform.translation += Vec3::new(slot.x, slot.y, 0.0) * (3.0 / 60.0);
        }
    }
}

/// What bot `player` does at host tick `tick`.
pub fn bot(player: PlayerId, tick: u64) -> Vec<(u32, [f32; 2])> {
    let id = move_id();
    match player {
        // Walks past the treasure (x = 4, y = 10) and back, then stops.
        1 if tick < 150 => vec![(id, [1.0, 0.0])],
        1 if tick < 300 => vec![(id, [-1.0, 0.0])],
        2 if tick < 300 => {
            let y = if (tick / 40).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            vec![(id, [0.0, y])]
        }
        4 if tick < 300 => vec![(id, [0.5, 0.5])],
        _ => Vec::new(),
    }
}

/// One client: its endpoint, replication state and world.
pub struct Peer {
    pub endpoint: Endpoint<SimTransport>,
    pub client: Client,
    pub world: World,
    pub seed: u64,
}

impl Peer {
    pub fn addr(&self) -> std::net::SocketAddr {
        self.endpoint.transport().local_addr()
    }
}

/// A host and its clients over a simulated network.
pub struct Match {
    pub net: SimNetwork,
    pub endpoint: Endpoint<SimTransport>,
    pub host: Host,
    pub world: World,
    pub cast: Cast,
    pub peers: Vec<Peer>,
    pub tick: u64,
    /// The host's target for every player at every tick.
    pub records: BTreeMap<(PlayerId, u64), View>,
    /// Ticks at which each player's client found its world equal to the
    /// record of the tick it applied.
    pub exact: BTreeMap<PlayerId, u64>,
    /// Every client world hash, per tick.
    pub hashes: Vec<Vec<u128>>,
    pub host_seed: u64,
    pub check_exact: bool,
}

impl Match {
    pub fn new(
        seed: u64,
        conditions: LinkConditions,
        clients: u32,
        config: ReplicationConfig,
    ) -> Self {
        let net = SimNetwork::new(seed, TICK_MS);
        net.set_conditions(conditions);
        let (world, cast) = host_world(clients);
        let host_seed = seed ^ 0xA5;
        let endpoint =
            Endpoint::host(net.add_node(), net_config(), Entropy::seeded(host_seed)).unwrap();
        let host = Host::new(&world, config).unwrap();
        let mut this = Self {
            net,
            endpoint,
            host,
            world,
            cast,
            peers: Vec::new(),
            tick: 0,
            records: BTreeMap::new(),
            exact: BTreeMap::new(),
            hashes: Vec::new(),
            host_seed,
            check_exact: true,
        };
        for _ in 0..clients {
            this.join();
        }
        this
    }

    /// Starts one more client (it connects over the next ticks).
    pub fn join(&mut self) {
        let index = self.peers.len() as u64;
        let seed = self.host_seed ^ (0x100 + index);
        let mut world = World::new();
        register(&mut world);
        let endpoint = Endpoint::connect(
            self.net.add_node(),
            self.endpoint.local_addr(),
            net_config(),
            Entropy::seeded(seed),
        )
        .unwrap();
        self.peers.push(Peer {
            endpoint,
            client: Client::new(ClientConfig::default()),
            world,
            seed,
        });
    }

    /// One tick of the whole match.
    pub fn step(&mut self) {
        self.net.step();
        let now = self.net.now_ms();
        self.endpoint.update(now);
        let rest = self.host.pump(&mut self.world, &mut self.endpoint);
        drop(rest);
        self.tick += 1;
        let tick = self.tick;
        self.host.apply_inputs(&mut self.world, tick).unwrap();
        move_avatars(&mut self.world);
        self.host
            .send(&self.world, tick, &mut self.endpoint)
            .unwrap();
        self.endpoint.update(now);
        for (_, player) in self.host.players() {
            if let Some(target) = self.host.target(player) {
                self.records.insert((player, tick), target.clone());
            }
        }
        let mut hashes = Vec::new();
        for peer in &mut self.peers {
            peer.endpoint.update(now);
            let before = peer.client.applied_tick();
            peer.client.pump(&mut peer.world, &mut peer.endpoint);
            let after = peer.client.applied_tick();
            if let (Some(player), Some(applied)) = (peer.client.player(), after)
                && after != before
                && self.check_exact
                && peer.client.newest_shown() == Some(applied)
            {
                let mirror = peer.client.mirror(&peer.world).unwrap();
                let expected = self
                    .records
                    .get(&(player, applied))
                    .unwrap_or_else(|| panic!("no record of player {player} at tick {applied}"));
                assert_eq!(
                    &mirror, expected,
                    "player {player}'s world differs from the host's target at tick {applied}"
                );
                *self.exact.entry(player).or_insert(0) += 1;
            }
            if let Some(player) = peer.client.player() {
                let stamp = peer.client.stamp();
                peer.client
                    .send_input(&mut peer.endpoint, bot(player, stamp))
                    .unwrap();
            }
            peer.endpoint.update(now);
            hashes.push(peer.world.hash().as_u128());
        }
        self.hashes.push(hashes);
    }

    pub fn run(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.step();
        }
    }

    /// The peer of `player`.
    pub fn peer(&self, player: PlayerId) -> &Peer {
        self.peers
            .iter()
            .find(|p| p.client.player() == Some(player))
            .unwrap_or_else(|| panic!("no client is player {player}"))
    }

    /// Whether every client's world equals the host's current target.
    pub fn all_current(&self) -> bool {
        self.peers.iter().all(|peer| {
            let Some(player) = peer.client.player() else {
                return false;
            };
            let Some(target) = self.host.target(player) else {
                return false;
            };
            peer.client.mirror(&peer.world).unwrap() == *target
        })
    }
}

/// The defaults with the test game's types.
pub fn replication() -> ReplicationConfig {
    ReplicationConfig::new(TYPES)
}

/// Every value of component `T` in `world`, in entity order.
pub fn all<T: wish_schema::Component>(world: &World) -> Vec<T> {
    world
        .entities()
        .filter_map(|e| world.get::<T>(e).copied())
        .collect()
}
