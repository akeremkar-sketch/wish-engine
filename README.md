# Wish Engine

*Dile, olsun.* (Turkish for "Make a wish, and it is done.")

A free, open-source, commission-free 3D game engine. It is fully integrated with AI, and it is also
fully usable without any AI. You can do everything by hand, and an AI assistant can drive the same
engine fully through MCP.

## What it does

- **Editor:**
  - Hierarchy, inspector, scene gizmos, asset browser, prefabs.
  - Play / pause / step, and a timeline.
  - Replays and scenario tests.
  - 12 languages.
- **AI cockpit:**
  - Show and tell; a live activity feed.
  - Every change can be undone on its own.
  - A checkpoint before every task.
  - Permission modes: suggest only / apply and I review / fully autonomous.
  - Any outside MCP agent can connect.
- **Game systems:**
  - Physics (Rapier) and character movement.
  - Audio; point and spot lights (clustered forward+); shadows.
  - glTF models and skeletal animation.
  - In-game interface, particles, 2D sprites.
- **Game code:** Rust, sandboxed as WebAssembly.
  - Hot reload without stopping the game.
  - The compiler ships in the package; nothing else to install.
- **Determinism:** the same scene and seed play out bit for bit the same on every machine. Tests and
  replays rely on this.
- **Export:** a standalone game folder that runs on a machine without the engine installed.

## Getting started

```bash
wen edit
```

With no project, the welcome screen opens:
1. choose a helper (Claude Code, or by hand);
2. choose a template;
3. give a name and create.

Details: the [user guide](docs/user/en/guide.md). A Turkish guide is in
[docs/user/tr/guide.md](docs/user/tr/guide.md).

## Building from source

You need Rust 1.100.0 (pinned by `rust-toolchain.toml`). On Windows you also need Visual Studio
Build Tools (C++).

```bash
cargo build --release -p wish_cli -p wish_player
```

```bash
cargo test --workspace
```

## Documents

| Document | Contents |
|---|---|
| [docs/user/en/guide.md](docs/user/en/guide.md) | User guide (English) |
| [docs/user/tr/guide.md](docs/user/tr/guide.md) | User guide (Turkish) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contributing guide: invariants, rules, verification |
| [docs/VISION.md](docs/VISION.md) | What we are making, and why |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Layers and crates |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decisions |
| [docs/MCP_API.md](docs/MCP_API.md) | The AI interface |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones |

## License

MIT or Apache-2.0, whichever suits you. Third-party licenses:
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
