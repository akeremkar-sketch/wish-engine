# Wish Engine: recipes

Working pieces of games, each a complete `scripts/src/lib.rs` and the scene operations that go with it. The
engine's own tests build each one, apply it and play it (the player runs and jumps, a coin is picked up, a checkpoint
brings the player back...), so they work as written. Take what you need; rename freely.
Steps for every recipe: write the code, `script_build`, apply the operations with `edit_apply`, then `sim_start` /
`sim_step` with `input` and look (`entity_get` with `session`, `describe_view`). A block of tool calls is run first,
in its order.

## A platformer character

A physics character (`CharacterController`) that runs with `move` and jumps with `jump`. Game code only says how fast
the character wants to go (`CharacterMotion.velocity`, gravity included); physics moves it, stops it at walls and says
whether it stands on something (`CharacterState.grounded`). Things it walks on need a `Collider`.

```rust
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// How fast the player runs.
    #[schema(unit = "m/s", default = 5.0)]
    pub speed: f32,
    /// How fast the player leaves the ground when jumping.
    #[schema(unit = "m/s", default = 6.5)]
    pub jump_speed: f32,
}

/// Gravity, added by the game (a character is moved, not pushed).
const GRAVITY: f32 = 18.0;

/// `move` is [x, y]: x to the right (+X), y forward (-Z).
#[system]
fn run(actions: Actions, time: Res<Time>, mut players: Query<(&Player, &mut CharacterMotion, &CharacterState)>) {
    let go = actions.axis2("move");
    for (player, motion, state) in players.iter_mut() {
        let mut vy = motion.velocity.y - GRAVITY * time.delta;
        if state.grounded {
            vy = if actions.pressed("jump") { player.jump_speed } else { -1.0 };
        }
        motion.velocity = Vec3::new(go.x * player.speed, vy, -go.y * player.speed);
    }
}

wish_sdk::export! {
    components: [Player],
    systems: [run],
}
```

```json ops
[
  { "op": "spawn", "id": "floor", "components": {
    "Transform": { "translation": [0, -0.5, 0], "scale": [20, 1, 20] },
    "Primitive": { "shape": "Cube" }, "Collider": { "shape": "Box" } } },
  { "op": "spawn", "id": "player", "components": {
    "Transform": { "translation": [0, 0.95, 0], "scale": [0.6, 1.8, 0.6] },
    "Primitive": { "shape": "Cube" },
    "RigidBody": { "kind": "Kinematic" }, "Collider": { "shape": "Capsule" },
    "CharacterController": {}, "CharacterMotion": {}, "CharacterState": {}, "Player": {} } }
]
```

A capsule `Collider` is sized by the entity's scale: scale [0.6, 1.8, 0.6] makes a capsule 0.6 m wide and 1.8 m tall,
standing with its centre 0.9 m above its feet. A moving platform (a kinematic `RigidBody` whose `Transform` game code
changes) carries a character standing on it.

## A camera that follows

Moves the camera smoothly behind the player and turns it to look at the player, the same at any tick length.

```rust
use wish_sdk::prelude::*;

/// The player (what the camera follows).
#[component]
pub struct Player {
    /// Unused here.
    pub score: u32,
}

/// A camera that follows the player from behind and above.
#[component]
pub struct FollowCamera {
    /// Where the camera wants to be, from the player.
    #[schema(unit = "m", default = Vec3::new(0.0, 3.5, 7.0))]
    pub offset: Vec3,
    /// How quickly it catches up (higher is stiffer).
    #[schema(unit = "1/s", default = 5.0)]
    pub stiffness: f32,
}

#[system]
fn follow(time: Res<Time>, players: Query<&Transform, With<Player>>, mut cameras: Query<(&FollowCamera, &mut Transform)>) {
    let Some(target) = players.iter().next().map(|t| t.translation) else {
        return;
    };
    for (follow, transform) in cameras.iter_mut() {
        let blend = 1.0 - (-follow.stiffness * time.delta).exp();
        transform.translation += (target + follow.offset - transform.translation) * blend;
        let look = target + Vec3::new(0.0, 1.0, 0.0) - transform.translation;
        if look.length() > 1e-3 {
            let dir = look.normalize();
            let yaw = (-dir.x).atan2(-dir.z);
            let pitch = dir.y.clamp(-1.0, 1.0).asin();
            transform.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
        }
    }
}

wish_sdk::export! {
    components: [Player, FollowCamera],
    systems: [follow],
}
```

```json ops
[
  { "op": "spawn", "id": "hero", "components": { "Transform": {}, "Player": {} } },
  { "op": "set", "id": "camera", "component": "FollowCamera", "value": { "offset": [0, 4, 8] } }
]
```

## Things to pick up, a score on screen, a sound and sparks

Coins are sensor colliders: touching one sends `CollisionStarted { a, b }` (a before b in entity order). The coin is
removed, the score goes up, a sound plays (`asset("...")` makes the `AssetRef` of a file; `asset_create` makes a
placeholder `.wav`) and a burst of sparks starts where it was. The label's text is written with `text(...)`.

```rust
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// Unused here.
    pub lives: u32,
}

/// A coin to pick up.
#[component]
pub struct Coin {
    /// Points it is worth.
    #[schema(default = 1)]
    pub value: u32,
}

/// The text that shows the score.
#[component]
pub struct ScoreLabel {
    /// The score it shows now.
    pub shown: u32,
}

/// Points so far.
#[resource]
pub struct Score {
    /// Points.
    pub points: u32,
}

#[system]
fn pick_up(
    touches: EventReader<CollisionStarted>,
    players: Query<Entity, With<Player>>,
    coins: Query<(Entity, &Transform, &Coin)>,
    time: Res<Time>,
    mut score: ResMut<Score>,
    mut sounds: EventWriter<PlaySound>,
    mut commands: Commands,
) {
    for touch in touches.read() {
        let player = players.iter().any(|p| p == touch.a || p == touch.b);
        if !player {
            continue;
        }
        for (coin, at, worth) in coins.iter() {
            if coin != touch.a && coin != touch.b {
                continue;
            }
            score.points += worth.value;
            commands.despawn(coin);
            sounds.send(PlaySound {
                clip: asset("assets/sounds/coin.wav"),
                bus: AudioBus::Sfx,
                volume: 1.0,
                pitch: 1.0,
                spatial: false,
                position: at.translation,
            });
            commands.spawn().insert(Transform::from_translation(at.translation)).insert(ParticleEmitter {
                start_time: time.elapsed as f32,
                rate: 0.0,
                burst: 20,
                lifetime: 0.5,
                speed: 2.0,
                spread_deg: 90.0,
                size_start: 0.12,
                color_start: Color::rgba(1.0, 0.85, 0.2, 1.0),
                color_end: Color::rgba(1.0, 0.5, 0.1, 0.0),
                ..ParticleEmitter::default()
            });
        }
    }
}

#[system(after = "pick_up")]
fn show_score(score: Res<Score>, mut labels: Query<(&mut ScoreLabel, &mut UiText)>) {
    for (label, words) in labels.iter_mut() {
        if label.shown != score.points || text_of(words.text).is_empty() {
            words.text = text(&format!("Score: {}", score.points));
            label.shown = score.points;
        }
    }
}

wish_sdk::export! {
    components: [Player, Coin, ScoreLabel],
    resources: [Score],
    systems: [pick_up, show_score],
}
```

```json ops
[
  { "op": "spawn", "id": "coin_1", "components": {
    "Transform": { "translation": [2, 0.6, 0], "scale": [0.4, 0.4, 0.4] },
    "Primitive": { "shape": "Sphere" }, "Material": { "base_color": [1, 0.8, 0.2, 1], "metallic": 1 },
    "Collider": { "shape": "Sphere", "size": [1.5, 1.5, 1.5], "sensor": true }, "Coin": {} } },
  { "op": "spawn", "id": "score_label", "components": {
    "UiNode": { "position": "Absolute", "offset": [32, 24] },
    "UiText": { "text": "Score: 0", "size": 40 }, "ScoreLabel": {} } }
]
```

Make the sound first: `asset_create` with `{ "path": "assets/sounds/coin.wav", "sound": { "wave": "sine", "frequency": 900,
"end_frequency": 1400, "seconds": 0.12 } }`. Check it with `logs_get` and kinds ["audio"].

## Falling off and checkpoints

Below a line the player comes back at the last checkpoint touched. A checkpoint is a sensor collider. Set the
player's `respawn` to where it starts in the scene (`{ "op": "set", "id": "player", "component": "Player", "value":
{ "respawn": [0, 0.95, 0] } }`), so a fall before any checkpoint brings it back there, not into the floor.

```rust
use wish_sdk::prelude::*;

/// The player and where it comes back.
#[component]
pub struct Player {
    /// The last checkpoint's place (at first: where the player starts).
    #[schema(unit = "m", default = Vec3::new(0.0, 1.0, 0.0))]
    pub respawn: Vec3,
}

/// A checkpoint: touching it moves the respawn point here.
#[component]
pub struct Checkpoint {
    /// Where the player comes back.
    #[schema(unit = "m")]
    pub respawn: Vec3,
}

#[system]
fn checkpoints(touches: EventReader<CollisionStarted>, mut players: Query<(Entity, &mut Player)>, flags: Query<(Entity, &Checkpoint)>) {
    for touch in touches.read() {
        for (who, player) in players.iter_mut() {
            let other = if touch.a == who { touch.b } else if touch.b == who { touch.a } else { continue };
            if let Some((_, flag)) = flags.iter().find(|(e, _)| *e == other) {
                player.respawn = flag.respawn;
            }
        }
    }
}

#[system]
fn fall(mut players: Query<(&Player, &mut Transform, &mut CharacterMotion)>) {
    for (player, at, motion) in players.iter_mut() {
        if at.translation.y < -8.0 {
            at.translation = player.respawn;
            motion.velocity = Vec3::ZERO;
        }
    }
}

wish_sdk::export! {
    components: [Player, Checkpoint],
    systems: [checkpoints, fall],
}
```

```json ops
[
  { "op": "spawn", "id": "checkpoint_1", "components": {
    "Transform": { "translation": [6, 1, 0] },
    "Collider": { "shape": "Box", "size": [3, 2, 3], "sensor": true },
    "Checkpoint": { "respawn": [6, 1, 0] } } }
]
```

## A moving platform

A kinematic body moved by game code. Its place follows from the time alone, so it needs no state; a character standing
on it is carried exactly.

```rust
use wish_sdk::prelude::*;

/// Goes back and forth between two places.
#[component]
pub struct MovingPlatform {
    /// One end.
    #[schema(unit = "m")]
    pub from: Vec3,
    /// The other end.
    #[schema(unit = "m")]
    pub to: Vec3,
    /// Seconds from one end to the other.
    #[schema(unit = "s", default = 3.0)]
    pub seconds: f32,
}

#[system]
fn platforms(time: Res<Time>, mut moving: Query<(&MovingPlatform, &mut Transform)>) {
    for (platform, at) in moving.iter_mut() {
        let seconds = platform.seconds.max(0.1);
        let t = (time.elapsed as f32 / seconds).rem_euclid(2.0);
        let along = if t < 1.0 { t } else { 2.0 - t };
        let eased = along * along * (3.0 - 2.0 * along);
        at.translation = platform.from + (platform.to - platform.from) * eased;
    }
}

wish_sdk::export! {
    components: [MovingPlatform],
    systems: [platforms],
}
```

```json ops
[
  { "op": "spawn", "id": "lift", "components": {
    "Transform": { "translation": [0, 1, -4], "scale": [2.5, 0.5, 2.5] },
    "Primitive": { "shape": "Cube" }, "Collider": { "shape": "Box" },
    "RigidBody": { "kind": "Kinematic" },
    "MovingPlatform": { "from": [0, 1, -4], "to": [0, 1, -10], "seconds": 3 } } }
]
```

## A win screen and a start button

Boxes lay their children out in sibling order: give `order` to `spawn` (0, 1, 2...) where it matters (without it
siblings go by id, alphabetically).

A hidden interface panel that game code shows, and a button: `ButtonClicked { entity }` arrives the tick after a click
(press and release over it). Interface positions are pixels of a 1280 x 720 screen; `describe_view` lists the
interface, and `sim_step` input `{ "pointer": [640, 360], "click": true }` then `{ "pointer": [640, 360] }` clicks.

```rust
use wish_sdk::prelude::*;

/// The panel shown at the end.
#[component]
pub struct WinPanel {
    /// Whether it is shown.
    pub shown: bool,
}

/// Where the game stands.
#[resource]
pub struct GameState {
    /// Whether the start button was clicked.
    pub started: bool,
    /// Whether the game is won.
    pub won: bool,
}

#[system]
fn start(clicks: EventReader<ButtonClicked>, mut state: ResMut<GameState>) {
    if !clicks.is_empty() {
        state.started = true;
    }
}

/// Shows the panel once the game is won: the rest of your game sets
/// `GameState.won` (for example when the last coin is gone).
#[system]
fn show_win(state: Res<GameState>, mut panels: Query<(&mut WinPanel, &mut UiNode)>) {
    for (panel, node) in panels.iter_mut() {
        if state.won && !panel.shown {
            panel.shown = true;
            node.visible = true;
        }
    }
}

wish_sdk::export! {
    components: [WinPanel],
    resources: [GameState],
    systems: [start, show_win],
}
```

```json ops
[
  { "op": "spawn", "id": "menu", "components": { "UiNode": {
    "width": { "unit": "Percent", "value": 100 }, "height": { "unit": "Percent", "value": 100 },
    "justify": "Center", "align": "Center" } } },
  { "op": "spawn", "id": "start_button", "parent": "menu", "components": {
    "UiNode": { "padding": [16, 40, 16, 40], "background": [0.1, 0.3, 0.9, 1] }, "UiButton": {} } },
  { "op": "spawn", "id": "start_label", "parent": "start_button", "components": {
    "UiNode": {}, "UiText": { "text": "Start", "size": 40 } } },
  { "op": "spawn", "id": "win_panel", "components": {
    "UiNode": { "position": "Absolute", "offset": [440, 300], "padding": [24, 48, 24, 48],
      "background": [0.05, 0.1, 0.25, 0.85], "visible": false }, "WinPanel": {} } },
  { "op": "spawn", "id": "win_text", "parent": "win_panel", "components": {
    "UiNode": {}, "UiText": { "text": "You win!", "size": 56 } } }
]
```

## A 2D game

An orthographic camera looks at sprites standing in the XY plane. For 2D physics lock movement along Z and rotation
about X and Y. (A new project's 3D `ground` plane is removed first.)

```json ops
[
  { "op": "despawn", "id": "ground" },
  { "op": "set", "id": "camera", "component": "Camera", "value": { "projection": "Orthographic", "ortho_height": 10 } },
  { "op": "replace", "id": "camera", "component": "Transform", "value": { "translation": [0, 0, 10] } },
  { "op": "spawn", "id": "ball", "components": {
    "Transform": { "translation": [0, 3, 0] },
    "Sprite": { "rect": [0, 0, 50, 50], "color": [1, 0.4, 0.2, 1] },
    "RigidBody": { "kind": "Dynamic", "lock_translation": [false, false, true], "lock_rotation": [true, true, false] },
    "Collider": { "shape": "Sphere", "size": [0.5, 0.5, 0.5], "restitution": 0.8 } } },
  { "op": "spawn", "id": "floor", "components": {
    "Transform": { "translation": [0, -4, 0] },
    "Sprite": { "rect": [0, 0, 1600, 50], "color": [0.3, 0.6, 0.3, 1] },
    "Collider": { "shape": "Box", "size": [16, 0.5, 1] } } }
]
```

A sprite with an `image` (a PNG; `asset_create` makes placeholders) shows it at `pixels_per_unit` pixels per meter;
`rect` picks a frame of a sprite sheet. `describe_view` lists the sprites with their boxes.

## A texture on a box

A shape's `Material` is four numbers for a plain look. A picture on its surface comes from a material file
(`*.mat.json`): `asset_create` makes one from the fields of a `MaterialFile`, and the entity names the file in its
`Material.file`. Every entity that names the file shares the look and changes when the file does. (The picture here
is a placeholder made by `asset_create`; any PNG of the project works.)

```json calls
[
  { "tool": "asset_create", "args": { "path": "assets/textures/tile.png",
    "picture": { "width": 64, "height": 64, "shape": "diamond", "color": [0.9, 0.55, 0.2, 1], "background": [0.25, 0.2, 0.2, 1] } } },
  { "tool": "asset_create", "args": { "path": "assets/materials/crate.mat.json",
    "material": { "base_color_texture": "assets/textures/tile.png", "roughness": 0.6 } } },
  { "tool": "asset_create", "args": { "path": "assets/materials/tiles.mat.json",
    "material": { "base_color_texture": "assets/textures/tile.png", "roughness": 0.8, "uv_scale": [12, 12] } } }
]
```

```json ops
[
  { "op": "spawn", "id": "crate", "components": {
    "Primitive": { "shape": "Cube" },
    "Transform": { "translation": [0, 0.5, 0] },
    "Material": { "file": "assets/materials/crate.mat.json" } } },
  { "op": "replace", "id": "ground", "component": "Material", "value": { "file": "assets/materials/tiles.mat.json" } }
]
```

- The textures of a material file: `base_color_texture` (colour; its alpha cuts holes with `alpha_mode: "Mask"`),
  `normal_texture` (bumps), `metallic_roughness_texture`, `occlusion_texture`, `emissive_texture` (set `emissive` to
  `[1, 1, 1]` for it to show). Each multiplies the number it belongs to: `base_color` tints the picture.
- `uv_scale: [12, 12]` repeats the picture 12 by 12 across each face; `uv_offset` slides it. On a cube every face shows
  the picture upright; on a plane its top is at -Z; around a sphere it wraps once.
- With `file` set the numbers of that entity's own `Material` are not used (`check` says so). To change a material
  file, make it again with `asset_create` and `"overwrite": true`, or edit the file: it is read again when it changes.
- On a `Model` entity a material file replaces every material of the model; without `file` a model keeps its own.
- `capture_frame` warns (`texture_not_loaded`, `material_not_loaded`) when a file is missing or broken and draws
  the surface without it.

## A glass window

A surface is see-through when its material blends: a material file with `alpha_mode: "Blend"`, or a plain `Material`
whose `base_color` alpha is below 1. Its alpha is how much of it covers what is behind: 1 solid, 0.5 half, 0 nothing.

```json calls
[
  { "tool": "asset_create", "args": { "path": "assets/materials/glass.mat.json",
    "material": { "base_color": [0.7, 0.85, 0.95, 0.25], "roughness": 0.05, "alpha_mode": "Blend", "double_sided": true } } }
]
```

```json ops
[
  { "op": "spawn", "id": "window", "components": {
    "Primitive": { "shape": "Cube" },
    "Transform": { "translation": [0, 1, 0], "scale": [2, 1.5, 0.05] },
    "Material": { "file": "assets/materials/glass.mat.json" } } },
  { "op": "spawn", "id": "ghost", "components": {
    "Primitive": { "shape": "Sphere" },
    "Transform": { "translation": [1.8, 0.6, 1] },
    "Material": { "base_color": [0.85, 0.9, 1, 0.4], "emissive": [0.1, 0.1, 0.15] } } }
]
```

- To fade something out, game code lowers `Material.base_color` alpha each tick; at 0 it is not drawn, not picked
  and `describe_view` lists it as hidden.
- See-through surfaces are lit, shadowed and fogged like any surface and bloom shows through them, but they cast no
  shadow. They are drawn farthest first, one object at a time (by the middle of each one): two that cut through
  each other are not sorted inside, and a small one over a large one (a bottle over water) can be drawn under it;
  split the large one if it is.
- A material file's alpha does nothing unless `alpha_mode` is `"Blend"`; glTF materials with `BLEND` are see-through.
- `capture_frame` and `describe_view` count them in `stats.blended_surfaces`; `describe_view` marks each one
  `"blended": true` and still lists what is seen through it as visible.

## Proving it: scenario tests and replays

A scenario test plays the game from a scene with inputs and checks values; `test_run` says which step failed, at which
tick, and what the values were. Conditions read components of one entity (`"entity": "player"`) or resources.

```json test
{
  "format": 1,
  "scene": "scenes/main.scene.json",
  "seed": 1,
  "steps": [
    { "frames": 20 },
    { "expect": "CharacterState.grounded == true", "entity": "player", "message": "the player stands" },
    { "input": { "move": [1, 0] }, "until": "Score.points >= 1", "max_frames": 240 },
    { "input": { "move": [0, 0], "jump": true }, "frames": 1 },
    { "frames": 30 },
    { "expect": "count(Coin) == 0", "message": "every coin is gone" },
    { "capture": { "golden": "tests/golden/end.png", "width": 320, "height": 180 } }
  ]
}
```

- `until` with an entity condition: `"has(Player) && Transform.translation.x > 6"` (any entity matching stops it).
- Read values while you play: `entity_get` with `session`, `ids` and `resources` (`["Score"]`).
- Record a run you played with `replay` (`record` at tick 0, `save`, later `run`): it checks the world after every tick,
  so after a code change it tells whether the game still plays the same.
